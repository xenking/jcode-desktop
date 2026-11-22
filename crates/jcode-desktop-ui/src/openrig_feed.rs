use gpui::{App, AppContext, Context, Entity, Global, Task};
use jcode_desktop_openrig::{Snapshot, default_base_url, fetch_snapshot};
use std::time::Duration;

pub(crate) struct Feed {
    pub snapshot: Option<Snapshot>,
    focus_wake: async_channel::Sender<()>,
    _poll: Option<Task<()>>,
}

impl Feed {
    fn new(cx: &mut Context<Self>) -> Self {
        let (focus_wake, focus_changes) = async_channel::bounded(1);
        let poll = (!cfg!(test) && !crate::harness::screenshot_mode()).then(|| {
            cx.spawn(async move |this, cx| {
                loop {
                    if !cx.update(|app| app.active_window().is_some()) {
                        // The HUD is not visible. Keep its last good data and
                        // wait for workspace activation instead of doing I/O.
                        if focus_changes.recv().await.is_err() {
                            return;
                        }
                        continue;
                    }
                    let snapshot = cx
                        .background_executor()
                        .spawn(async { fetch_snapshot(&default_base_url()) })
                        .await;
                    if this
                        .update(cx, |feed, cx| {
                            if feed.snapshot.as_ref() != Some(&snapshot) {
                                feed.snapshot = Some(snapshot);
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                    futures_lite::future::race(
                        async {
                            let _ = focus_changes.recv().await;
                        },
                        cx.background_executor().timer(Duration::from_secs(2)),
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
            focus_wake,
            _poll: poll,
        }
    }
}

struct Shared(Entity<Feed>);
impl Global for Shared {}

pub(crate) fn window_activation_changed(cx: &App) {
    if let Some(shared) = cx.try_global::<Shared>() {
        let _ = shared.0.read(cx).focus_wake.try_send(());
    }
}

pub(crate) fn get(cx: &mut App) -> Entity<Feed> {
    if !cx.has_global::<Shared>() {
        let feed = cx.new(Feed::new);
        cx.set_global(Shared(feed));
    }
    cx.global::<Shared>().0.clone()
}
