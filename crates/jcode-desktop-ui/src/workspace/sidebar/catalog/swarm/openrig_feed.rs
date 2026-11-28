use gpui::{App, AppContext, Context, Entity, Global, Task};
use jcode_desktop_openrig::{
    Snapshot, default_base_url, fetch_hud_fleet, fetch_snapshot, fleet::Fleet,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::Duration;

const BROWSER_INTERVAL: Duration = Duration::from_secs(2);
const HUD_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Scope {
    Idle,
    Hud(BTreeSet<String>),
    Browser,
}

#[derive(Default)]
struct Interest {
    browser: usize,
    hud: BTreeMap<String, usize>,
}

impl Interest {
    fn scope(&self) -> Scope {
        if self.browser > 0 {
            Scope::Browser
        } else if self.hud.is_empty() {
            Scope::Idle
        } else {
            Scope::Hud(self.hud.keys().cloned().collect())
        }
    }
}

/// Kept by the panel, so closing the last interested panel also stops the I/O.
pub(crate) struct Demand {
    interest: Rc<RefCell<Interest>>,
    changed: async_channel::Sender<()>,
    host: Option<String>,
}

impl Drop for Demand {
    fn drop(&mut self) {
        let mut interest = self.interest.borrow_mut();
        if let Some(host) = &self.host {
            if let Some(count) = interest.hud.get_mut(host) {
                *count -= 1;
                if *count == 0 {
                    interest.hud.remove(host);
                    let _ = self.changed.try_send(());
                }
            }
        } else {
            interest.browser -= 1;
            if interest.browser == 0 {
                let _ = self.changed.try_send(());
            }
        }
    }
}

enum PollResult {
    Browser(Snapshot),
    Hud(Option<Fleet>),
}

pub(crate) struct Feed {
    pub snapshot: Option<Snapshot>,
    hud: Option<Fleet>,
    interest: Rc<RefCell<Interest>>,
    changed: async_channel::Sender<()>,
    _poll: Option<Task<()>>,
}

impl Feed {
    fn new(cx: &mut Context<Self>) -> Self {
        let (changed, changes) = async_channel::bounded(1);
        let interest = Rc::new(RefCell::new(Interest::default()));
        // A private native acceptance instance can exercise the real poller
        // against its loopback fixture without activating the hidden window.
        let native_probe = crate::harness::screenshot_mode()
            && std::env::var_os("JCODE_TEST_SESSION").is_some_and(|v| v == "1")
            && std::env::var_os("JCODE_DESKTOP_CONTROL_TEST").is_some_and(|v| v == "1")
            && std::env::var_os("JCODE_DESKTOP_TEST_OPENRIG_POLL").is_some_and(|v| v == "1")
            && default_base_url().starts_with("http://127.0.0.1:");
        let poll =
            (!cfg!(test) && (!crate::harness::screenshot_mode() || native_probe)).then(|| {
                let interest = interest.clone();
                cx.spawn(async move |this, cx| {
                    loop {
                        while changes.try_recv().is_ok() {}
                        let scope = interest.borrow().scope();
                        if scope == Scope::Idle
                            || (!native_probe && !cx.update(|app| app.active_window().is_some()))
                        {
                            if changes.recv().await.is_err() {
                                return;
                            }
                            continue;
                        }
                        let request = scope.clone();
                        let result = cx
                            .background_executor()
                            .spawn(async move {
                                let base = default_base_url();
                                match request {
                                    Scope::Idle => unreachable!(),
                                    Scope::Hud(hosts) => {
                                        PollResult::Hud(fetch_hud_fleet(&base, &hosts))
                                    }
                                    Scope::Browser => PollResult::Browser(fetch_snapshot(&base)),
                                }
                            })
                            .await;
                        if interest.borrow().scope() != scope {
                            continue;
                        }
                        if this
                            .update(cx, |feed, cx| match result {
                                PollResult::Hud(Some(fleet)) if feed.fleet() != Some(&fleet) => {
                                    feed.hud = Some(fleet);
                                    cx.notify();
                                }
                                PollResult::Browser(snapshot) => {
                                    let changed = feed.hud.take().is_some()
                                        || feed.snapshot.as_ref() != Some(&snapshot);
                                    if changed {
                                        feed.snapshot = Some(snapshot);
                                        cx.notify();
                                    }
                                }
                                PollResult::Hud(_) => {}
                            })
                            .is_err()
                        {
                            break;
                        }
                        let interval = if scope == Scope::Browser {
                            BROWSER_INTERVAL
                        } else {
                            HUD_INTERVAL
                        };
                        futures_lite::future::race(
                            async {
                                let _ = changes.recv().await;
                            },
                            cx.background_executor().timer(interval),
                        )
                        .await;
                    }
                })
            });
        let snapshot = if crate::harness::screenshot_mode()
            && std::env::var_os("JCODE_DESKTOP_SCREENSHOT_RIG_HUD").is_some()
        {
            let mut snapshot = Snapshot::fixture();
            for seat in snapshot
                .fleet
                .hosts
                .iter_mut()
                .take(1)
                .flat_map(|host| host.rigs.iter_mut().flatten())
                .flat_map(|rig| rig.pods.iter_mut().flatten())
                .flat_map(|pod| &mut pod.seats)
                .filter(|seat| seat.jcode_session_id.is_some())
            {
                seat.jcode_session_id = Some("screenshot-fixture".into());
            }
            Some(snapshot)
        } else {
            None
        };
        Self {
            snapshot,
            hud: None,
            interest,
            changed,
            _poll: poll,
        }
    }

    pub fn fleet(&self) -> Option<&Fleet> {
        self.hud
            .as_ref()
            .or_else(|| self.snapshot.as_ref().map(|snapshot| &snapshot.fleet))
    }

    /// Ask the poller to re-read what it should poll, now.
    fn wake(&self) {
        let _ = self.changed.try_send(());
    }

    pub fn demand(&self, host: Option<String>) -> Demand {
        let mut interest = self.interest.borrow_mut();
        let first = if let Some(host) = &host {
            let count = interest.hud.entry(host.clone()).or_default();
            *count += 1;
            *count == 1
        } else {
            interest.browser += 1;
            interest.browser == 1
        };
        if first {
            self.wake();
        }
        Demand {
            interest: self.interest.clone(),
            changed: self.changed.clone(),
            host,
        }
    }
}

struct Shared(Entity<Feed>);
impl Global for Shared {}

pub(crate) fn window_activation_changed(cx: &App) {
    if let Some(shared) = cx.try_global::<Shared>() {
        shared.0.read(cx).wake();
    }
}

pub(crate) fn get(cx: &mut App) -> Entity<Feed> {
    if !cx.has_global::<Shared>() {
        let feed = cx.new(Feed::new);
        cx.set_global(Shared(feed));
    }
    cx.global::<Shared>().0.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn demand_is_shared_and_ends_when_panels_drop(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let feed = get(cx);
            let feed = feed.read(cx);
            let scope = || feed.interest.borrow().scope();
            assert_eq!(scope(), Scope::Idle);
            let first = feed.demand(Some("host".into()));
            let second = feed.demand(Some("host".into()));
            let hud = Scope::Hud(BTreeSet::from(["host".into()]));
            assert_eq!(scope(), hud);
            drop(first);
            assert_eq!(scope(), hud);
            let browser = feed.demand(None);
            assert_eq!(scope(), Scope::Browser);
            drop(browser);
            assert_eq!(scope(), hud);
            drop(second);
            assert_eq!(scope(), Scope::Idle);
        });
    }
}
