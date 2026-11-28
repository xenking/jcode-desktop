//! The background feed of routing views and the offline screenshot fixture.

use jcode_session_types::routing::RoutingView;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

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

    /// Start `tx` on its views: the fixture offline, else the shared poller.
    fn start(tx: Sender<RoutingView>) {
        if crate::platform::screenshot_mode() {
            let _ = tx.send(fixture());
        } else {
            shared_poller().subscribe(tx);
        }
    }
}

/// Subscribe to the process-wide poller. Every window shares one thread and
/// one short connection per poll.
pub fn spawn() -> Feed {
    let (tx, rx) = channel();
    Feed::start(tx);
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
            icon: None,
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
