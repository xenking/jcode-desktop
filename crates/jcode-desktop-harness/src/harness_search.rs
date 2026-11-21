//! Global session search: one request per debounced keystroke.
//!
//! Search runs on its own short-lived connection rather than on the panel
//! connections. A query is a read of the daemon's index: it needs no
//! attachment, and a slow first query (a cold backfill over every stored
//! session) must not stall the turn traffic of a session the user is watching.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// The daemon sections result cap. Titles and messages are each capped at this,
/// which is more than one screen of rows but keeps a common word from
/// materialising tens of thousands of hits in the UI thread.
pub const SEARCH_LIMIT: u32 = 50;

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Issue a search on a background thread.
///
/// `generation` lets the UI drop an answer for a query the user has already
/// typed past: results arrive out of order because they are separate threads,
/// and the newest query is the only one worth showing.
pub(crate) fn spawn_search(
    updates: UpdateSender,
    query: String,
    generation: u64,
) {
    let newest = GENERATION.fetch_max(generation, Ordering::AcqRel);
    if generation < newest {
        return;
    }
    std::thread::Builder::new()
        .name("jcode-bridge-search".into())
        .spawn(move || {
            let started = std::time::Instant::now();
            let result = connect("search").and_then(|client| {
                client.search_sessions(&query, Some(SEARCH_LIMIT))
            });
            // A newer query may have been sent while this one ran; its answer
            // is the one the user is waiting for.
            if GENERATION.load(Ordering::Acquire) > generation {
                return;
            }
            let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
            let _ = updates.send(Update::SessionSearch {
                query,
                generation,
                result: result.map_err(|error| error.to_string()),
                elapsed_ms,
            });
        })
        .expect("spawn search thread");
}
