//! Run filesystem probes off the UI thread and apply their answers on it.
//!
//! Render reads only in-memory caches (`jcode_desktop_model::probe_cache`) and
//! queues the keys it could not answer. The owner of the cache calls [`run`]
//! after the frame, which probes the keys on the background executor and
//! applies the answers back on the main thread, where the cache lives.

use gpui::App;
use jcode_desktop_model::probe_cache::ProbeCache;

/// Probe `keys` in the background, then call `apply` on the main thread with
/// the answers. `apply` is the place to fill the cache and notify.
pub(crate) fn run<K, V>(
    cx: &App,
    keys: Vec<K>,
    probe: fn(&K) -> V,
    apply: impl FnOnce(Vec<(K, V)>, &mut App) + 'static,
) where
    K: Send + 'static,
    V: Send + 'static,
{
    if keys.is_empty() {
        return;
    }
    let executor = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        let answers = executor
            .spawn(async move {
                keys.into_iter()
                    .map(|key| {
                        let answer = probe(&key);
                        (key, answer)
                    })
                    .collect::<Vec<_>>()
            })
            .await;
        cx.update(|cx| apply(answers, cx));
    })
    .detach();
}

/// Probe whatever render queued in a thread-local cache. The windows redraw
/// once if an answer changed what the frame shows.
pub(crate) fn drain_local<K, V>(
    cx: &App,
    cache: &'static std::thread::LocalKey<std::cell::RefCell<ProbeCache<K, V>>>,
    probe: fn(&K) -> V,
) where
    K: std::hash::Hash + Eq + Clone + Send + 'static,
    V: PartialEq + Send + 'static,
{
    let queued = cache.with_borrow_mut(ProbeCache::take_queued);
    run(cx, queued, probe, move |answers, cx| {
        let changed = cache.with_borrow_mut(|cache| {
            answers.into_iter().fold(false, |changed, (key, answer)| {
                cache.fill(key, answer) | changed
            })
        });
        if changed {
            cx.refresh_windows();
        }
    });
}
