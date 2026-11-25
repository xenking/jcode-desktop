//! Account routing state a session panel shows: the account chip and the
//! spending-limit gate card. GPUI-free so the behaviour is tested without a
//! window. The gate's keys and rows come from the model every client shares.

pub mod rows;

use jcode_session_types::routing::{RoutingGateChoice, RoutingGateOption, RoutingView};
use jcode_session_types::routing_gate::{GateKey, GateOutcome, RoutingGateModel};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// The account serving a session, why, and what the daemon warned about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingChip {
    pub account: String,
    pub reason: String,
    pub warnings: Vec<String>,
    /// The account a warm session moves to at its next cache-cold seam.
    pub pending: Option<String>,
}

impl RoutingChip {
    pub fn new(
        account: String,
        reason: String,
        warnings: Vec<String>,
        pending: Option<String>,
    ) -> Option<Self> {
        let account = clean(&account);
        (!account.is_empty()).then(|| Self {
            account,
            reason: clean(&reason),
            warnings: warnings
                .iter()
                .map(|w| clean(w))
                .filter(|w| !w.is_empty())
                .collect(),
            pending: pending.map(|p| clean(&p)).filter(|p| !p.is_empty()),
        })
    }

    /// `⚠` for one warning, `⚠3` for several, nothing for none.
    pub fn warning_marker(&self) -> Option<String> {
        match self.warnings.len() {
            0 => None,
            1 => Some("⚠".into()),
            count => Some(format!("⚠{count}")),
        }
    }

    /// The chip's one-line text: account, warning marker, then `→ target`.
    pub fn label(&self) -> String {
        let mut label = self.account.clone();
        if let Some(marker) = self.warning_marker() {
            label.push(' ');
            label.push_str(&marker);
        }
        if let Some(pending) = &self.pending {
            label.push_str(" → ");
            label.push_str(pending);
        }
        label
    }

    /// The hover text, one entry per line.
    pub fn tooltip(&self) -> Vec<String> {
        let mut lines = vec![format!("Running on {}", self.account)];
        if !self.reason.is_empty() {
            lines.push(format!("Why: {}", self.reason));
        }
        if let Some(pending) = &self.pending {
            lines.push(format!("Moves to {pending} at the next cache-cold turn"));
        }
        lines.extend(self.warnings.iter().map(|warning| format!("⚠ {warning}")));
        lines
    }
}

fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// An open gate: the shared state machine plus the answer in flight.
#[derive(Debug, Clone)]
pub struct GateCard {
    model: RoutingGateModel,
    /// Attempt number of the answer sent and not yet settled.
    pub attempt: Option<u64>,
    /// Why the last answer failed, shown on the card.
    pub error: Option<String>,
}

/// A read-only snapshot of an open gate for rendering.
pub struct GateView<'a> {
    pub gate_id: &'a str,
    pub reason: &'a str,
    /// Display text of each answer row, the cancel row last.
    pub rows: Vec<String>,
    /// The focused row.
    pub cursor: usize,
    /// An answer is in flight, so the card accepts no input.
    pub submitting: bool,
}

impl GateCard {
    pub fn new(gate_id: String, reason: String, options: Vec<RoutingGateOption>) -> Self {
        Self {
            model: RoutingGateModel::new(gate_id, reason, options),
            attempt: None,
            error: None,
        }
    }

    /// What the card shows right now.
    pub fn view(&self) -> GateView<'_> {
        GateView {
            gate_id: &self.model.gate_id,
            reason: &self.model.reason,
            rows: self.model.rows(),
            cursor: self.model.cursor,
            submitting: self.submitting(),
        }
    }

    fn submitting(&self) -> bool {
        self.attempt.is_some()
    }

    pub fn press(&mut self, key: GateKey) -> Option<RoutingGateChoice> {
        if self.submitting() {
            return None;
        }
        match self.model.press(key) {
            GateOutcome::Pending => None,
            GateOutcome::Done(choice) => Some(choice),
        }
    }

    /// A click on `row` answers it, and also moves the focus there.
    pub fn choose_row(&mut self, row: usize) -> Option<RoutingGateChoice> {
        if self.submitting() || row >= self.model.row_count() {
            return None;
        }
        self.model.cursor = row;
        Some(self.model.choice_at(row))
    }

    pub fn mark_submitting(&mut self, attempt: u64) {
        self.attempt = Some(attempt);
        self.error = None;
    }

    pub fn answer_failed(&mut self, error: String) {
        self.attempt = None;
        self.error = Some(error);
    }
}

/// How often the routing view is read.
const POLL: Duration = Duration::from_secs(60);
/// How often to ask again once the runtime has said it has no routing.
const UNSUPPORTED_POLL: Duration = Duration::from_secs(600);

/// Where the routing view comes from. The real source is the local runtime.
trait ViewSource: Send + 'static {
    fn fetch(&mut self) -> Result<RoutingView, FetchError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FetchError {
    /// The runtime is up but carries no routing. Ask rarely.
    Unsupported,
    /// Anything else: not running, mid-reload, timed out. Ask again soon.
    Unavailable,
}

struct LocalRuntime;

impl ViewSource for LocalRuntime {
    fn fetch(&mut self) -> Result<RoutingView, FetchError> {
        let client = crate::harness::connect("routing").map_err(|_| FetchError::Unavailable)?;
        client.routing_view().map_err(|error| match error.kind {
            jcode_sdk::ErrorKind::UnsupportedCapability => FetchError::Unsupported,
            _ => FetchError::Unavailable,
        })
    }
}

/// Keeps the last good view and reports only changes.
#[derive(Default)]
struct Publisher {
    latest: Option<RoutingView>,
}

impl Publisher {
    /// The view to publish after `result`, if it differs from the last good
    /// one. A failure keeps what is already shown.
    fn observe(&mut self, result: Result<RoutingView, FetchError>) -> Option<RoutingView> {
        let view = result.ok()?;
        (self.latest.as_ref() != Some(&view)).then(|| {
            self.latest = Some(view.clone());
            view
        })
    }

    /// How long to wait before the next fetch.
    fn next_poll(result: &Result<RoutingView, FetchError>) -> Duration {
        match result {
            Err(FetchError::Unsupported) => UNSUPPORTED_POLL,
            _ => POLL,
        }
    }
}

/// Background feed of routing views. The UI polls `latest()`.
#[derive(Clone)]
pub struct Feed {
    updates: Arc<Mutex<Receiver<RoutingView>>>,
}

impl Feed {
    /// The most recent view, if one arrived since the last poll.
    pub fn latest(&self) -> Option<RoutingView> {
        let receiver = self.updates.lock().ok()?;
        let mut latest = None;
        while let Ok(view) = receiver.try_recv() {
            latest = Some(view);
        }
        latest
    }
}

/// Subscribe to the process-wide poller. Every window shares one thread and
/// one short connection per poll.
pub fn spawn() -> Feed {
    let (tx, rx) = channel();
    if crate::platform::screenshot_mode() {
        let _ = tx.send(fixture());
    } else {
        shared_poller().subscribe(tx);
    }
    Feed {
        updates: Arc::new(Mutex::new(rx)),
    }
}

#[derive(Default)]
struct Poller {
    state: Mutex<PollerState>,
}

#[derive(Default)]
struct PollerState {
    subscribers: Vec<Sender<RoutingView>>,
    latest: Option<RoutingView>,
}

impl Poller {
    fn subscribe(&self, tx: Sender<RoutingView>) {
        let mut state = self.state.lock().unwrap();
        if let Some(latest) = state.latest.clone() {
            let _ = tx.send(latest);
        }
        state.subscribers.push(tx);
    }

    fn publish(&self, view: RoutingView) {
        let mut state = self.state.lock().unwrap();
        state
            .subscribers
            .retain(|subscriber| subscriber.send(view.clone()).is_ok());
        state.latest = Some(view);
    }
}

fn shared_poller() -> &'static Poller {
    static POLLER: OnceLock<Poller> = OnceLock::new();
    POLLER.get_or_init(|| {
        std::thread::Builder::new()
            .name("jcode-routing".into())
            .spawn(|| {
                let mut source = LocalRuntime;
                let mut publisher = Publisher::default();
                loop {
                    let result = source.fetch();
                    let wait = Publisher::next_poll(&result);
                    if let Some(view) = publisher.observe(result) {
                        shared_poller().publish(view);
                    }
                    std::thread::sleep(wait);
                }
            })
            .expect("spawn routing thread");
        Poller::default()
    })
}

/// Synthetic data for offline screenshots: one account, two labelled keys of
/// one provider, and three budgets, one of them unreadable.
pub fn fixture() -> RoutingView {
    use jcode_session_types::routing::{
        RoutingAccountView, RoutingApiKeyView, RoutingBudgetView, RoutingKeyView, RoutingWindowView,
    };
    let window = |window: &str, used: f64, resets_in_secs: i64| RoutingWindowView {
        window: window.into(),
        unit: "percent".into(),
        used,
        limit: None,
        resets_at: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |now| now.as_secs() as i64)
                + resets_in_secs,
        ),
        model_class: None,
    };
    let key = |account: &str, label: Option<&str>, active: bool, windows| RoutingKeyView {
        account: account.into(),
        label: label.map(str::to_string),
        source: "stored".into(),
        active,
        routed: true,
        windows,
    };
    let budget =
        |name: &str, scope: &str, unit: &str, limit: f64, warn: f64, used| RoutingBudgetView {
            name: name.into(),
            scope_kind: "provider".into(),
            scope: scope.into(),
            meter: if unit == "usd" { "ledger" } else { "provider" }.into(),
            window: if unit == "usd" {
                "calendar-month"
            } else {
                "5h"
            }
            .into(),
            unit: unit.into(),
            warn: Some(warn),
            limit,
            action: "ask".into(),
            used,
        };
    RoutingView {
        mode: "auto".into(),
        paid: "off".into(),
        problems: Vec::new(),
        accounts: vec![RoutingAccountView {
            id: "claude:team".into(),
            kind: "subscription".into(),
            priority: Some(1),
            class_priority: Default::default(),
            models: Vec::new(),
            egress: None,
            on_limit: "ask".into(),
            windows: vec![window("5h", 42.0, 9_000)],
        }],
        budgets: vec![
            budget("hf-month", "huggingface", "usd", 100.0, 80.0, Some(82.0)),
            budget("claude-5h", "claude", "percent", 90.0, 70.0, Some(41.0)),
            budget("opencode-week", "opencode-go", "percent", 80.0, 60.0, None),
        ],
        api_keys: vec![RoutingApiKeyView {
            provider: "opencode-go".into(),
            keys: vec![
                key(
                    "opencode-go",
                    None,
                    false,
                    vec![window("5h", 12.0, 7_200), window("1w", 31.0, 200_000)],
                ),
                key(
                    "opencode-go:second",
                    Some("second"),
                    true,
                    vec![window("5h", 74.0, 3_600), window("1w", 93.0, 90_000)],
                ),
            ],
        }],
    }
}

#[cfg(test)]
mod tests;
