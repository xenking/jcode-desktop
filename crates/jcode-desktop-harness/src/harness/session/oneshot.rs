//! Short-lived operations on a session that need no worker: context
//! breakdown, tool shaking and pinning a session without a live panel.

use jcode_sdk::ApiEvent;

use crate::harness::local_runtime::connect;
use crate::harness::protocol::{ContextBreakdownTarget, ContextInspection, Update};
use crate::harness::remote;
use crate::harness::remote::transport;
use crate::harness::updates::UpdateSender;

pub(in crate::harness) fn read_context_breakdown(
    target: ContextBreakdownTarget,
    transports: &transport::RemoteTransports,
) -> ContextInspection {
    let summary_only = matches!(target, ContextBreakdownTarget::SessionMeter(_));
    let (host, session_id, model, cwd) = match target {
        ContextBreakdownTarget::Session(session_id)
        | ContextBreakdownTarget::SessionMeter(session_id) => {
            let Ok(address) = remote::SessionAddress::parse(&session_id) else {
                return ContextInspection::default();
            };
            (address.host, Some(address.session_id), None, None)
        }
        ContextBreakdownTarget::Draft { model, cwd } => (None, None, model, cwd),
    };
    let client = match host {
        Some(host) => transports.connect(&host),
        None => connect("context-view"),
    };
    let Ok(client) = client else {
        return ContextInspection::default();
    };
    let shake_supported =
        session_id.is_some() && client.supports(jcode_sdk::api::SHAKE_TOOLS_CAPABILITY);
    let breakdown = if client.supports(jcode_sdk::api::CONTEXT_BREAKDOWN_CAPABILITY) {
        client
            .request(jcode_sdk::api::ApiRequest::SessionContextBreakdown {
                session_id: session_id.clone(),
                model,
                cwd,
                summary_only,
                include_snapcompact: !summary_only,
            })
            .ok()
            .and_then(|frame| match frame.event {
                ApiEvent::ContextBreakdown { breakdown } if breakdown.session_id == session_id => {
                    Some(breakdown)
                }
                _ => None,
            })
    } else {
        None
    };
    ContextInspection {
        breakdown,
        shake_supported,
    }
}

pub(in crate::harness) fn shake_tools(
    session_id: &str,
    action: jcode_sdk::api::ShakeToolsAction,
    transports: &transport::RemoteTransports,
) -> Result<jcode_sdk::api::ShakeToolsResult, String> {
    let address = remote::SessionAddress::parse(session_id).map_err(|error| error.to_string())?;
    let client = match &address.host {
        Some(host) => transports.connect(host),
        None => connect("context-shake"),
    }
    .map_err(|error| error.to_string())?;
    if !client.supports(jcode_sdk::api::SHAKE_TOOLS_CAPABILITY) {
        return Err("This runtime does not support manual Shake tools.".into());
    }
    let session_id = address.session_id;
    let frame = client
        .request(jcode_sdk::api::ApiRequest::SessionShakeTools {
            session_id: session_id.clone(),
            action,
        })
        .map_err(|error| error.to_string())?;
    match frame.event {
        ApiEvent::ShakeTools {
            session_id: target,
            result,
        } if target == session_id => Ok(result),
        ApiEvent::Error { message, .. } => Err(message),
        _ => Err("Unexpected runtime reply. Shake outcome unknown.".into()),
    }
}

/// Pin or unpin a session that has no live panel. The daemon applies saves to
/// the connection's attached session, so attach briefly, save, and detach
/// again: pinning history must not leave it resident or owned by Desktop.
pub(in crate::harness) fn set_saved_detached(
    session_id: String,
    saved: bool,
    updates: UpdateSender,
    transports: transport::RemoteTransports,
) {
    let result = (|| -> Result<(), String> {
        let address = remote::SessionAddress::parse(&session_id)?;
        let client = match &address.host {
            Some(host) => transports.connect(host),
            None => connect("pin"),
        }
        .map_err(|error| error.to_string())?;
        let real_id = address.session_id.as_str();
        client
            .attach_session(real_id)
            .map_err(|error| error.to_string())?;
        let saved_result = client.set_session_saved(real_id, saved, None);
        let _ = client.detach_session(real_id);
        saved_result.map_err(|error| error.to_string())
    })();
    match result {
        Ok(()) => {
            let _ = updates.send(Update::SessionSaved {
                session_id,
                saved,
                label: None,
            });
        }
        Err(error) => {
            let verb = if saved { "pin" } else { "unpin" };
            let _ = updates.send(Update::Status(format!("Could not {verb} session: {error}")));
        }
    }
}
