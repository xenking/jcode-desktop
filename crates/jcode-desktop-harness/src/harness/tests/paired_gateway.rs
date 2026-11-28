//! Real sockets, no mocks: Desktop connects to fixture paired gateways through
//! the same transport the app uses. Each failure arrives classified, in its own
//! words, and a gateway that comes back is reachable on the next try.
use super::*;
use crate::connect_failure::{ConnectFailure, FailureAction, FailureKind};
use futures::{SinkExt, StreamExt};
use jcode_sdk::PairedFailure;
use std::net::TcpListener;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::{Message, http::Response};

const GOOD_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const STALE_TOKEN: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// A minimal Harness gateway: checks the bearer token, answers hello and ping.
fn gateway_on(listener: TcpListener, accepted: &'static str) {
    listener.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            loop {
                let Ok((stream, _)) = listener.accept().await else { return };
                tokio::spawn(async move {
                    let callback = |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                                    response| {
                        let expected = format!("Bearer {accepted}");
                        if request.headers().get("authorization").and_then(|v| v.to_str().ok()) == Some(expected.as_str()) {
                            Ok(response)
                        } else {
                            Err(Response::builder()
                                .status(401)
                                .body(Some("Unknown or revoked auth token; re-pair this device".to_owned()))
                                .unwrap())
                        }
                    };
                    let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(stream, callback).await else { return };
                    while let Some(Ok(Message::Text(text))) = ws.next().await {
                        let frame: serde_json::Value = serde_json::from_str(&text).unwrap();
                        let reply = match frame["req"].as_str() {
                            Some("hello") => serde_json::json!({"v":1,"reply_to":frame["id"],"ev":"hello_ok","version":1,"server":"fixture","capabilities":[]}),
                            Some("ping") => serde_json::json!({"v":1,"reply_to":frame["id"],"ev":"pong"}),
                            _ => continue,
                        };
                        if ws.send(Message::Text(reply.to_string())).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
    });
}

fn classify(host: &str) -> Result<(), ConnectFailure> {
    let transports = transport::RemoteTransports::default();
    match transports.connect(host) {
        Ok(client) => {
            client.ping().expect("a connected gateway answers ping");
            Ok(())
        }
        Err(error) => Err(ConnectFailure::from_error(&error)),
    }
}

#[test]
#[cfg_attr(not(debug_assertions), ignore = "fixture gateways need the debug-only loopback pairing")]
fn paired_gateways_report_why_they_fail_and_recover_on_retry() {
    crate::paired::with_home(|_| {
        // The fixture gateways speak plain HTTP on loopback.
        crate::paired::enable_debug_loopback();
        let endpoint =
            |listener: &TcpListener| format!("http://{}", listener.local_addr().unwrap());
        let machine = |id: &str, label: &str, endpoint: &str, token: &str| {
            let destination = crate::paired::Destination::with_id(
                id.repeat(32 / id.len()),
                endpoint,
                label,
                "/work",
            )
            .unwrap();
            crate::paired::set_test_token(&destination.id, token);
            destination
        };

        let up = TcpListener::bind("127.0.0.1:0").unwrap();
        let up_endpoint = endpoint(&up);
        gateway_on(up, GOOD_TOKEN);
        let revoked = TcpListener::bind("127.0.0.1:0").unwrap();
        let revoked_endpoint = endpoint(&revoked);
        gateway_on(revoked, GOOD_TOKEN);
        let down = TcpListener::bind("127.0.0.1:0").unwrap();
        let down_address = down.local_addr().unwrap();
        let down_endpoint = endpoint(&down);
        drop(down);

        let machines = [
            machine("aa", "UP", &up_endpoint, GOOD_TOKEN),
            machine("bb", "REVOKED", &revoked_endpoint, STALE_TOKEN),
            machine("cc", "DOWN", &down_endpoint, GOOD_TOKEN),
            machine("dd", "NODNS", "https://gateway.invalid", GOOD_TOKEN),
        ];
        crate::paired::write_all(&machines).unwrap();
        let host = |index: usize| machines[index].host();

        assert!(classify(&host(0)).is_ok(), "a reachable gateway connects");

        let revoked = classify(&host(1)).unwrap_err();
        assert_eq!(revoked.kind, FailureKind::Paired(PairedFailure::Revoked));
        assert_eq!(revoked.action(), FailureAction::Repair);
        assert_eq!(
            revoked.summary("REVOKED"),
            "REVOKED no longer accepts this device. Pair it again."
        );

        let down = classify(&host(2)).unwrap_err();
        assert_eq!(down.kind, FailureKind::Paired(PairedFailure::Refused));
        assert_eq!(down.action(), FailureAction::Retry);

        let started = Instant::now();
        let dns = classify(&host(3)).unwrap_err();
        assert_eq!(dns.kind, FailureKind::Paired(PairedFailure::Unresolved));
        assert_eq!(dns.action(), FailureAction::Retry);
        assert_eq!(
            dns.summary("NODNS"),
            "Can't reach NODNS (name did not resolve). Is Tailscale or the VPN on?"
        );
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "DNS failure must not wait out the connect deadline"
        );

        // The fixture comes back on the same address. Retry connects.
        let back = TcpListener::bind(down_address).expect("rebind the fixture's port");
        gateway_on(back, GOOD_TOKEN);
        assert!(
            classify(&host(2)).is_ok(),
            "Retry reaches the gateway once it is back"
        );

        let no_token =
            crate::paired::Destination::with_id("ee".repeat(16), &up_endpoint, "NOTOKEN", "/work")
                .unwrap();
        crate::paired::write_all(&[no_token.clone()]).unwrap();
        let missing = classify(&no_token.host()).unwrap_err();
        assert_eq!(missing.kind, FailureKind::Paired(PairedFailure::NoToken));
        assert_eq!(missing.action(), FailureAction::Repair);
    });
}
