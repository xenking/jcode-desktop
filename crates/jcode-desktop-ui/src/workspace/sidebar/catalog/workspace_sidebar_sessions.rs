//! What the sidebar lists: session rows, their order, titles and metadata.
use super::*;

/// Local sessions some jcode process, normally the shared daemon, is running a
/// turn for right now. Reads only the small streaming-marker directory.
pub(super) fn daemon_running_sessions() -> HashSet<String> {
    jcode_base::session::streaming_session_ids()
        .into_iter()
        .collect()
}

/// Inputs that can change a virtual row's height, including its section header
/// and saved divider. Titles stay in a fixed-height box and do not belong here.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct SidebarSessionLayout {
    pub(super) session_id: String,
    pub(super) open: bool,
    pub(super) project: usize,
    pub(super) header: bool,
    pub(super) checkout_header: bool,
    pub(super) divider: bool,
    pub(super) section: Option<&'static str>,
    pub(super) collapsed: bool,
    pub(super) selected: bool,
    pub(super) saved: bool,
    pub(super) details: bool,
    /// The row's "Move to collection" menu is open beneath it.
    pub(super) menu: bool,
    /// Nested worker rows (names and states) change the item height.
    pub(super) workers: String,
}

pub(super) struct SidebarRow {
    /// Local panel membership drives selection and navigation.
    pub(super) open: bool,
    /// A local panel or an externally connected user client, not a loaded agent.
    pub(super) live: bool,
    pub(super) session: jcode_sdk::SessionInfo,
    pub(super) project: usize,
    pub(super) subdirectory: Option<String>,
    pub(super) header: bool,
    pub(super) collapsed: bool,
    pub(super) divider: bool,
    pub(super) checkout: Option<sidebar_projects::Checkout>,
    /// First thread of a checkout, which carries its branch row.
    pub(super) checkout_header: bool,
    /// "Live" or "Past" caption above the first row of each group.
    pub(super) section: Option<&'static str>,
    /// Nested swarm agents: (total, working).
    pub(super) agents: (usize, usize),
}

pub(super) fn sync_sidebar_session_layout(
    state: &gpui::ListState,
    previous: &mut Vec<SidebarSessionLayout>,
    next: Vec<SidebarSessionLayout>,
) {
    if *previous == next {
        return;
    }
    let old_len = previous.len();
    let new_len = next.len();
    if old_len != new_len {
        let shared = old_len.min(new_len);
        state.splice(shared..old_len, new_len - shared);
    }
    // A changed row can also add/remove the next row's heading or divider.
    // Appending/removing history also moves the final row's bottom padding.
    let changed = previous.iter().zip(&next).position(|(old, new)| old != new);
    let boundary = (old_len != new_len).then(|| old_len.min(new_len).saturating_sub(1));
    if let Some(first) = changed.into_iter().chain(boundary).min() {
        state.remeasure_items(first..new_len);
    }
    // Hints give the scrollbar a useful initial size without measuring history.
    state.clone().with_uniform_item_height(px(46.0));
    *previous = next;
}

pub(super) fn sidebar_session_title(session: &jcode_sdk::SessionInfo) -> (&'static str, String) {
    sidebar_session_title_with_open_title(session, None)
}

pub(super) fn sidebar_session_title_with_open_title(
    session: &jcode_sdk::SessionInfo,
    open_title: Option<&str>,
) -> (&'static str, String) {
    let animal = jcode_core::id::extract_session_name(&session.session_id);
    let icon = animal.map(jcode_core::id::session_icon).unwrap_or("💫");
    // An open panel owns the tab title, including provisional first-prompt titles
    // that have not reached the session catalog yet.
    let title = open_title
        .or(session.title.as_deref())
        .or(session.agent_label.as_deref())
        .unwrap_or_default();
    (
        icon,
        crate::panel::folder_session_title(&session.session_id, title).to_string(),
    )
}

#[cfg(test)]
pub(super) fn sidebar_session_order(sessions: &[jcode_sdk::SessionInfo]) -> Vec<jcode_sdk::SessionInfo> {
    let mut ordered = sessions.to_vec();
    sort_sidebar_sessions(&mut ordered, sidebar_session_catalog::Sort::Recent);
    ordered
}

pub(super) fn sort_sidebar_sessions(
    ordered: &mut [jcode_sdk::SessionInfo],
    sort: sidebar_session_catalog::Sort,
) {
    match sort {
        sidebar_session_catalog::Sort::Recent => ordered.sort_by_key(sidebar_session_sort_key),
        sidebar_session_catalog::Sort::AgentActivity => ordered.sort_by_key(|session| {
            (
                std::cmp::Reverse(session.last_agent_activity_at_ms()),
                sidebar_session_sort_key(session),
            )
        }),
    }
}

pub(super) fn sidebar_session_sort_key(session: &jcode_sdk::SessionInfo) -> std::cmp::Reverse<(bool, i64)> {
    std::cmp::Reverse((session.saved, sidebar_session_recency_ms(session)))
}

pub(super) fn same_session_catalog(a: &[jcode_sdk::SessionInfo], b: &[jcode_sdk::SessionInfo]) -> bool {
    if a == b {
        return true;
    }
    if a.len() != b.len() || !a.iter().zip(b).all(|(a, b)| same_session_row(a, b)) {
        return false;
    }
    // Only timestamps changed. Avoid invalidating views unless their stable
    // newest-first order changes. Compare borrowed rows, not transcript data.
    let mut a: Vec<_> = a.iter().collect();
    let mut b: Vec<_> = b.iter().collect();
    a.sort_by_key(|session| sidebar_session_sort_key(session));
    b.sort_by_key(|session| sidebar_session_sort_key(session));
    a.into_iter()
        .zip(b)
        .all(|(a, b)| a.session_id == b.session_id)
}

pub(super) fn same_session_row(a: &jcode_sdk::SessionInfo, b: &jcode_sdk::SessionInfo) -> bool {
    let jcode_sdk::SessionInfo {
        user_connected,
        edit_stats,
        context,
        session_id,
        parent_session_id,
        agent_label,
        swarm_status,
        working_dir,
        title,
        status,
        transcript_bytes,
        saved,
        save_label,
        updated_at_ms: _,
        last_active_at_ms,
        archived,
        archived_at_ms,
    } = a;
    *user_connected == b.user_connected
        && edit_stats == &b.edit_stats
        && context == &b.context
        && session_id == &b.session_id
        && parent_session_id == &b.parent_session_id
        && agent_label == &b.agent_label
        && swarm_status == &b.swarm_status
        && working_dir == &b.working_dir
        && title == &b.title
        && status == &b.status
        && transcript_bytes == &b.transcript_bytes
        && *saved == b.saved
        && save_label == &b.save_label
        && last_active_at_ms == &b.last_active_at_ms
        && *archived == b.archived
        && archived_at_ms == &b.archived_at_ms
}

pub(super) fn sidebar_session_recency_ms(session: &jcode_sdk::SessionInfo) -> i64 {
    session
        .updated_at_ms
        .or_else(|| {
            sidebar_session_created_ms(&session.session_id)
                .and_then(|value| i64::try_from(value).ok())
        })
        .unwrap_or_default()
}

/// Creation timestamp embedded in modern session ids (`session_fox_<ms>_...`).
pub(super) fn sidebar_session_created_ms(session_id: &str) -> Option<u64> {
    session_id
        .split('_')
        .filter_map(|part| part.parse::<u64>().ok())
        .find(|value| (1_000_000_000_000..10_000_000_000_000).contains(value))
}

/// `3m ago` / `2h ago` / `5d ago`, matching the TUI picker's time labels.
pub(super) fn format_time_ago(created_ms: u64, now_ms: u64) -> String {
    let seconds = now_ms.saturating_sub(created_ms) / 1_000;
    if seconds < 60 {
        return format!("{seconds}s ago");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m ago");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = hours / 24;
    if days < 7 {
        return format!("{days}d ago");
    }
    if days < 30 {
        return format!("{}w ago", days / 7);
    }
    format!("{}mo ago", days / 30)
}

/// `~1.2k tok`, matching the TUI picker's token display. The desktop only has
/// the stored transcript size, so estimate from bytes with the family-neutral
/// ratio.
pub(super) fn format_estimated_tokens(tokens: u64) -> String {
    if tokens < 1_000 {
        return format!("{tokens} tok");
    }
    const UNITS: &[(f64, &str)] = &[(1.0, ""), (1_000.0, "k"), (1_000_000.0, "M")];
    let value = tokens as f64;
    let mut index = 0;
    while index + 1 < UNITS.len() && value >= UNITS[index + 1].0 {
        index += 1;
    }
    let scaled = value / UNITS[index].0;
    if scaled >= 100.0 {
        format!("{:.0}{} tok", scaled, UNITS[index].1)
    } else {
        format!("{:.1}{} tok", scaled, UNITS[index].1)
    }
}

/// Opacity for a working session's sidebar title, a gentle cosine breath.
pub(super) fn sidebar_title_pulse(phase: f32) -> f32 {
    0.75 + 0.25 * (phase * std::f32::consts::TAU).cos()
}

/// The TUI picker's per-session metadata line, from the fields the desktop
/// API actually carries: `12m ago · ~4.2k tok`.
pub(super) fn sidebar_session_meta(session: &jcode_sdk::SessionInfo) -> Option<String> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default();
    let created = sidebar_session_created_ms(&session.session_id)
        .map(|created_ms| format_time_ago(created_ms, now_ms));
    session_meta(session, created)
}

pub(super) fn session_meta(session: &jcode_sdk::SessionInfo, age: Option<String>) -> Option<String> {
    let tokens = session
        .transcript_bytes
        .filter(|bytes| *bytes > 0)
        .map(|bytes| {
            let tokens = jcode_base::message::TokenFamily::default().tokens_for_bytes(bytes as usize);
            format_estimated_tokens(tokens as u64)
        });
    match (age, tokens) {
        (Some(age), Some(tokens)) => Some(format!("{age} · {tokens}")),
        (age, tokens) => age.or(tokens),
    }
}
