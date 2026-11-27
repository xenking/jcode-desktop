//! The per-machine state link against an in-process daemon that speaks the
//! Desktop state requests: one status per machine from a real connect and
//! capability check, the daemon's state and other Desktops' changes forwarded,
//! and writes routed with their outcome.
use super::*;
use crate::harness::machine_link::MachineLinks;
use crate::harness::protocol::{LinkStatus, MachineStateEvent, Update};
use crate::harness::updates::update_channel;
use jcode_sdk::api::{
    API_VERSION_MAJOR, ClientFrame, DesktopStateChange, DesktopStateEntry, ServerFrame, read_frame,
    write_frame,
};
use jcode_sdk::desktop_state::{DesktopStatePut, SyncOptions};
use jcode_sdk::{ApiEvent, ApiRequest, Transport};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Store {
    rev: u64,
    entries: BTreeMap<String, (u64, Value)>,
    subscribers: Vec<(u64, Arc<Mutex<UnixStream>>)>,
    connections: Vec<UnixStream>,
    next_connection: u64,
}

/// A daemon with Desktop state, or (`modern: false`) one that predates it.
#[derive(Clone)]
pub(super) struct FakeDaemon {
    store: Arc<Mutex<Store>>,
    modern: Arc<AtomicBool>,
    refusing: Arc<AtomicBool>,
}

struct Pipe(UnixStream);

impl Transport for Pipe {
    fn split(
        self: Box<Self>,
    ) -> jcode_sdk::Result<(Box<dyn BufRead + Send>, Box<dyn Write + Send>)> {
        let writer = self.0.try_clone().expect("clone");
        Ok((Box::new(BufReader::new(self.0)), Box::new(writer)))
    }
}

fn entries_since(store: &Store, rev: u64) -> Vec<DesktopStateEntry> {
    store
        .entries
        .iter()
        .filter(|(_, (changed, _))| *changed > rev)
        .map(|(key, (changed, value))| DesktopStateEntry {
            key: key.clone(),
            rev: *changed,
            value: Some(value.clone()),
        })
        .collect()
}

impl FakeDaemon {
    pub(super) fn new(modern: bool) -> Self {
        Self {
            store: Arc::default(),
            modern: Arc::new(AtomicBool::new(modern)),
            refusing: Arc::default(),
        }
    }

    pub(super) fn set_modern(&self, modern: bool) {
        self.modern.store(modern, Ordering::Release);
    }

    pub(super) fn set_refusing(&self, refusing: bool) {
        self.refusing.store(refusing, Ordering::Release);
    }

    /// Cut every open connection, like a daemon restart.
    pub(super) fn drop_connections(&self) {
        for stream in self.store.lock().unwrap().connections.drain(..) {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        self.store.lock().unwrap().subscribers.clear();
    }

    /// What another Desktop would write, applied straight to the store.
    pub(super) fn write_as_other_desktop(&self, key: &str, value: Value) {
        let mut store = self.store.lock().unwrap();
        store.rev += 1;
        let rev = store.rev;
        store.entries.insert(key.to_owned(), (rev, value));
        let push = ServerFrame::event(ApiEvent::DesktopStateChanged {
            space: "desktop".into(),
            rev,
            device: "other".into(),
            entries: entries_since(&store, rev - 1),
        });
        for (_, writer) in &store.subscribers {
            let _ = write_frame(&mut *writer.lock().unwrap(), &push);
        }
    }

    pub(super) fn connect(&self) -> jcode_sdk::Result<jcode_sdk::JcodeClient> {
        if self.refusing.load(Ordering::Acquire) {
            return Err(jcode_sdk::Error::new(
                jcode_sdk::ErrorKind::ConnectFailed,
                "refused",
            ));
        }
        let (client_end, server_end) = UnixStream::pair().expect("pair");
        let id = {
            let mut store = self.store.lock().unwrap();
            store.next_connection += 1;
            store
                .connections
                .push(server_end.try_clone().expect("clone"));
            store.next_connection
        };
        let daemon = self.clone();
        std::thread::spawn(move || daemon.serve(id, server_end));
        jcode_sdk::JcodeClient::connect_with(
            Box::new(Pipe(client_end)),
            jcode_sdk::ConnectOptions::default(),
        )
    }

    fn serve(&self, id: u64, stream: UnixStream) {
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let writer = Arc::new(Mutex::new(stream));
        let send = |frame: ServerFrame| {
            let _ = write_frame(&mut *writer.lock().unwrap(), &frame);
        };
        while let Ok(frame) = read_frame::<_, ClientFrame>(&mut reader) {
            let reply = |event| send(ServerFrame::reply(frame.id, event));
            let mut store = self.store.lock().unwrap();
            match frame.request {
                ApiRequest::Hello { .. } => reply(ApiEvent::HelloOk {
                    version: API_VERSION_MAJOR,
                    server: "fake".into(),
                    runtime_identity: None,
                    capabilities: if self.modern.load(Ordering::Acquire) {
                        vec!["desktop_state_v1".into()]
                    } else {
                        Vec::new()
                    },
                }),
                ApiRequest::DesktopStateGet { space }
                | ApiRequest::DesktopStateSubscribe { space } => {
                    store.subscribers.push((id, Arc::clone(&writer)));
                    reply(ApiEvent::DesktopState {
                        space,
                        rev: store.rev,
                        entries: entries_since(&store, 0),
                    });
                }
                ApiRequest::DesktopStatePut {
                    space,
                    expected_rev,
                    changes,
                    device,
                } => {
                    let moved = changes.iter().any(|change| {
                        store
                            .entries
                            .get(&change.key)
                            .is_some_and(|(changed, _)| *changed > expected_rev)
                    });
                    if moved {
                        reply(ApiEvent::DesktopStateConflict {
                            space,
                            rev: store.rev,
                            full: false,
                            entries: entries_since(&store, expected_rev),
                        });
                        continue;
                    }
                    store.rev += 1;
                    let rev = store.rev;
                    for change in changes {
                        match change.value {
                            Some(value) => store.entries.insert(change.key, (rev, value)),
                            None => store.entries.remove(&change.key),
                        };
                    }
                    let entries = entries_since(&store, rev - 1);
                    let push = ServerFrame::event(ApiEvent::DesktopStateChanged {
                        space: space.clone(),
                        rev,
                        device,
                        entries: entries.clone(),
                    });
                    for (other, subscriber) in &store.subscribers {
                        if *other != id {
                            let _ = write_frame(&mut *subscriber.lock().unwrap(), &push);
                        }
                    }
                    reply(ApiEvent::DesktopStateApplied {
                        space,
                        rev,
                        entries,
                    });
                }
                _ => {}
            }
        }
    }
}

fn fast() -> SyncOptions {
    SyncOptions {
        initial_backoff: Duration::from_millis(20),
        max_backoff: Duration::from_millis(60),
        poll: Duration::from_millis(20),
    }
}

fn links() -> (MachineLinks, crate::harness::updates::UpdateReceiver) {
    let (updates, receiver) = update_channel();
    (
        MachineLinks::with_options(updates, fast(), Duration::from_millis(50)),
        receiver,
    )
}

fn attach(links: &mut MachineLinks, host: Option<&str>, daemon: &FakeDaemon) {
    let daemon = daemon.clone();
    links.attach(host.map(str::to_owned), Arc::new(move || daemon.connect()));
}

/// Waits for an update `accept` takes, within a few seconds.
fn next(
    receiver: &crate::harness::updates::UpdateReceiver,
    what: &str,
    mut accept: impl FnMut(Update) -> Option<Update>,
) -> Update {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        match receiver.try_recv() {
            Ok(update) => {
                if let Some(found) = accept(update) {
                    return found;
                }
            }
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    panic!("timed out waiting for {what}");
}

fn status(receiver: &crate::harness::updates::UpdateReceiver, want: LinkStatus) {
    next(receiver, &format!("{want:?}"), |update| match update {
        Update::MachineLink { status, .. } if status == want => Some(update),
        _ => None,
    });
}

fn draft(text: &str) -> Value {
    json!({"v": 1, "text": text, "cursor": text.len()})
}

#[test]
fn a_machine_reads_connecting_then_connected_and_delivers_what_its_daemon_holds() {
    let daemon = FakeDaemon::new(true);
    daemon.write_as_other_desktop("draft/p1", draft("held"));
    let (mut links, updates) = links();
    attach(&mut links, Some("paired:aa"), &daemon);
    status(&updates, LinkStatus::Connecting);
    status(&updates, LinkStatus::Connected);
    let Update::MachineState { host, event } = next(&updates, "the snapshot", |update| {
        matches!(update, Update::MachineState { .. }).then_some(update)
    }) else {
        unreachable!()
    };
    assert_eq!(host.as_deref(), Some("paired:aa"));
    let MachineStateEvent::Snapshot(snapshot) = event else {
        panic!("a snapshot comes first")
    };
    assert_eq!(snapshot.rev, 1);
    assert_eq!(snapshot.entries[0].value, Some(draft("held")));
}

#[test]
fn another_desktops_change_arrives_and_a_put_on_the_head_is_applied() {
    let daemon = FakeDaemon::new(true);
    let (mut links, updates) = links();
    attach(&mut links, None, &daemon);
    status(&updates, LinkStatus::Connected);
    daemon.write_as_other_desktop("draft/p1", draft("from the other desktop"));
    let Update::MachineState { event, .. } = next(&updates, "the push", |update| {
        matches!(
            update,
            Update::MachineState {
                event: MachineStateEvent::Changed { .. },
                ..
            }
        )
        .then_some(update)
    }) else {
        unreachable!()
    };
    let MachineStateEvent::Changed { rev, device, .. } = event else {
        unreachable!()
    };
    assert_eq!((rev, device.as_str()), (1, "other"));

    links.put(
        None,
        7,
        rev,
        vec![DesktopStateChange {
            key: "draft/p2".into(),
            value: Some(draft("mine")),
        }],
    );
    let Update::MachineStatePut { token, result, .. } = next(&updates, "the put", |update| {
        matches!(update, Update::MachineStatePut { .. }).then_some(update)
    }) else {
        unreachable!()
    };
    assert_eq!(token, 7);
    assert!(
        matches!(result, Ok(DesktopStatePut::Applied { rev: 2, .. })),
        "{result:?}"
    );
}

#[test]
fn a_daemon_without_desktop_state_reads_needs_update_and_never_connected() {
    let daemon = FakeDaemon::new(false);
    let (mut links, updates) = links();
    attach(&mut links, Some("paired:aa"), &daemon);
    status(&updates, LinkStatus::NeedsUpdate);
    // It never claims to be connected, and a put is refused rather than lost.
    links.put(Some("paired:aa".into()), 1, 0, Vec::new());
    let mut connected = false;
    let Update::MachineStatePut { result, .. } = next(&updates, "the refused put", |update| {
        match &update {
            Update::MachineLink {
                status: LinkStatus::Connected,
                ..
            } => connected = true,
            Update::MachineStatePut { .. } => return Some(update),
            _ => {}
        }
        None
    }) else {
        unreachable!()
    };
    assert!(!connected);
    assert!(result.is_err());
}

#[test]
fn a_lost_machine_reads_lost_then_connected_again_with_its_state_intact() {
    let daemon = FakeDaemon::new(true);
    daemon.write_as_other_desktop("draft/p1", draft("kept"));
    let (mut links, updates) = links();
    attach(&mut links, None, &daemon);
    status(&updates, LinkStatus::Connected);

    daemon.set_refusing(true);
    daemon.drop_connections();
    status(&updates, LinkStatus::Lost);
    daemon.set_refusing(false);
    status(&updates, LinkStatus::Connected);
    let Update::MachineState { event, .. } = next(&updates, "the resync", |update| {
        matches!(
            update,
            Update::MachineState {
                event: MachineStateEvent::Snapshot(_),
                ..
            }
        )
        .then_some(update)
    }) else {
        unreachable!()
    };
    let MachineStateEvent::Snapshot(snapshot) = event else {
        unreachable!()
    };
    assert_eq!(snapshot.entries[0].value, Some(draft("kept")));
}

#[test]
fn a_machine_that_was_updated_stops_needing_an_update_without_a_restart() {
    let daemon = FakeDaemon::new(false);
    let (mut links, updates) = links();
    attach(&mut links, None, &daemon);
    status(&updates, LinkStatus::NeedsUpdate);
    // Jcode is updated on that machine; the link asks again by itself.
    daemon.set_modern(true);
    status(&updates, LinkStatus::Connected);
}

#[test]
fn only_the_attached_machine_connects() {
    let (a, b) = (FakeDaemon::new(true), FakeDaemon::new(true));
    let (mut links, updates) = links();
    attach(&mut links, None, &a);
    status(&updates, LinkStatus::Connected);
    assert_eq!(
        b.store.lock().unwrap().next_connection,
        0,
        "lazy: nothing asked for it"
    );
    attach(&mut links, Some("paired:bb"), &b);
    next(&updates, "the second machine", |update| match update {
        Update::MachineLink {
            host: Some(_),
            status: LinkStatus::Connected,
        } => Some(update),
        _ => None,
    });
    // Attaching twice reuses the link.
    attach(&mut links, Some("paired:bb"), &b);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(b.store.lock().unwrap().next_connection, 1);
}
