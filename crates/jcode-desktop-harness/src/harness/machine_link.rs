//! One Desktop state link per machine: this one and each paired one.
//!
//! A link is a dedicated connection to the machine's daemon that holds the
//! Desktop state the daemon keeps for every Desktop attached to it. It reports
//! one status per machine, taken from a real connect and capability check, and
//! forwards what the daemon holds and what other Desktops change. Links start
//! when asked (`Command::AttachMachine`), never at startup for every machine,
//! and a machine that cannot sync does not stop its sessions from working.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use jcode_sdk::api::DesktopStateChange;
use jcode_sdk::desktop_state::{DesktopStateSync, DesktopStateWriter, SyncEvent, SyncOptions};
use jcode_sdk::{ErrorKind, JcodeClient};

use crate::harness::protocol::{LinkStatus, MachineStateEvent, Update};
use crate::harness::updates::UpdateSender;

/// The one space every Desktop of a machine shares in that machine's daemon.
const SPACE: &str = "desktop";

/// How long a daemon without Desktop state waits before being asked again, in
/// case its owner has updated Jcode there meanwhile.
const RECHECK_UNSUPPORTED: Duration = Duration::from_secs(30);

type Connector = Arc<dyn Fn() -> jcode_sdk::Result<JcodeClient> + Send + Sync>;

struct Link {
    writer: Arc<Mutex<Option<DesktopStateWriter>>>,
    stop: Arc<AtomicBool>,
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

/// The machines this Desktop has attached, by host (`None` is this machine).
pub(super) struct MachineLinks {
    links: HashMap<Option<String>, Link>,
    updates: UpdateSender,
    device: String,
    options: SyncOptions,
    recheck: Duration,
}

impl MachineLinks {
    pub(super) fn new(updates: UpdateSender) -> Self {
        Self::with_options(updates, SyncOptions::default(), RECHECK_UNSUPPORTED)
    }

    pub(super) fn with_options(
        updates: UpdateSender,
        options: SyncOptions,
        recheck: Duration,
    ) -> Self {
        Self {
            links: HashMap::new(),
            updates,
            device: format!(
                "desktop-{}",
                &uuid::Uuid::new_v4().simple().to_string()[..8]
            ),
            options,
            recheck,
        }
    }

    /// Start the link to `host` unless it is running. `connect` makes one new
    /// connection to that machine's daemon.
    pub(super) fn attach(&mut self, host: Option<String>, connect: Connector) {
        if self.links.contains_key(&host) {
            return;
        }
        let link = Link {
            writer: Arc::new(Mutex::new(None)),
            stop: Arc::new(AtomicBool::new(false)),
        };
        let run = Run {
            host: host.clone(),
            connect,
            updates: self.updates.clone(),
            device: self.device.clone(),
            options: self.options,
            recheck: self.recheck,
            writer: Arc::clone(&link.writer),
            stop: Arc::clone(&link.stop),
        };
        std::thread::Builder::new()
            .name("jcode-machine-link".into())
            .spawn(move || run.supervise())
            .expect("spawn machine link thread");
        self.links.insert(host, link);
    }

    /// Write `changes` and report the outcome as `Update::MachineStatePut`.
    pub(super) fn put(
        &self,
        host: Option<String>,
        token: u64,
        expected_rev: u64,
        changes: Vec<DesktopStateChange>,
    ) {
        let writer = self
            .links
            .get(&host)
            .and_then(|link| link.writer.lock().ok()?.clone());
        let updates = self.updates.clone();
        std::thread::spawn(move || {
            let result = match writer {
                Some(writer) => writer
                    .put(expected_rev, changes)
                    .map_err(|error| error.to_string()),
                None => Err("not connected".to_owned()),
            };
            let _ = updates.send(Update::MachineStatePut {
                host,
                token,
                result,
            });
        });
    }
}

struct Run {
    host: Option<String>,
    connect: Connector,
    updates: UpdateSender,
    device: String,
    options: SyncOptions,
    recheck: Duration,
    writer: Arc<Mutex<Option<DesktopStateWriter>>>,
    stop: Arc<AtomicBool>,
}

impl Run {
    fn status(&self, status: LinkStatus) -> bool {
        self.updates
            .send(Update::MachineLink {
                host: self.host.clone(),
                status,
            })
            .is_ok()
    }

    fn supervise(self) {
        if !self.status(LinkStatus::Connecting) {
            return;
        }
        while !self.stop.load(Ordering::Acquire) {
            let connect = Arc::clone(&self.connect);
            let sync =
                DesktopStateSync::start(move || connect(), SPACE, &self.device, self.options);
            if let Ok(mut current) = self.writer.lock() {
                *current = Some(sync.writer());
            }
            let unsupported = self.forward(&sync);
            if let Ok(mut current) = self.writer.lock() {
                *current = None;
            }
            if !unsupported {
                return;
            }
            // Not syncing, but worth asking again later without a restart.
            let until = Instant::now() + self.recheck;
            while Instant::now() < until && !self.stop.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }

    /// Relay the sync's events until it reports an unsupported daemon (returns
    /// true) or this link is stopped or the UI is gone (false).
    fn forward(&self, sync: &DesktopStateSync) -> bool {
        loop {
            if self.stop.load(Ordering::Acquire) {
                return false;
            }
            let Some(event) = sync.recv_timeout(Duration::from_millis(100)) else {
                continue;
            };
            let delivered = match event {
                SyncEvent::Snapshot(snapshot) => {
                    self.status(LinkStatus::Connected)
                        && self
                            .updates
                            .send(Update::MachineState {
                                host: self.host.clone(),
                                event: MachineStateEvent::Snapshot(snapshot),
                            })
                            .is_ok()
                }
                SyncEvent::Changed {
                    rev,
                    device,
                    entries,
                } => self
                    .updates
                    .send(Update::MachineState {
                        host: self.host.clone(),
                        event: MachineStateEvent::Changed {
                            rev,
                            device,
                            entries,
                        },
                    })
                    .is_ok(),
                // A machine that has not synced yet and cannot be reached reads as
                // lost too: there is no separate "never" state.
                SyncEvent::Lost(_) => self.status(LinkStatus::Lost),
                SyncEvent::Unsupported(error) => {
                    debug_assert_eq!(error.kind, ErrorKind::UnsupportedCapability);
                    return self.status(LinkStatus::NeedsUpdate);
                }
            };
            if !delivered {
                return false;
            }
        }
    }
}
