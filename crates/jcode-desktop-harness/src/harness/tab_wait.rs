use std::time::{Duration, Instant};

use jcode_sdk::JcodeClient;
use jcode_sdk::todo::{
    TodoSnapshot, is_incomplete_todo, todo_assigned_elsewhere, todo_goal_cannot_complete,
    todo_waits_on_running_task,
};

use super::{Update, UpdateSender};

const RECHECK: Duration = Duration::from_secs(3);

#[derive(Default)]
pub(super) struct State {
    waiting: bool,
    recheck_at: Option<Instant>,
}

impl State {
    pub(super) fn refresh_due(&self) -> bool {
        self.recheck_at.is_some_and(|at| Instant::now() >= at)
    }

    pub(super) fn refresh(
        &mut self,
        client: &JcodeClient,
        real_id: &str,
        display_id: &str,
        updates: &UpdateSender,
    ) {
        if !client.supports("todo_state") {
            return;
        }
        if let Ok(snapshot) = client.todo_state(real_id) {
            let waiting =
                waiting_on_dependencies(&snapshot, real_id, super::remote_host(display_id).is_some());
            if std::mem::replace(&mut self.waiting, waiting) != waiting {
                let _ = updates.send(Update::TabWaitingState {
                    session_id: display_id.to_string(),
                    waiting,
                });
            }
        }
        // A known wait may expire without streaming an event. Keep polling
        // only those sessions, not every open tab, and retry transient errors.
        self.recheck_at = self.waiting.then(|| Instant::now() + RECHECK);
    }
}

fn waiting_on_dependencies(state: &TodoSnapshot, session_id: &str, remote: bool) -> bool {
    // Local tabs follow the named registrations in the workspace. Remote tabs
    // cannot see those files and use the bridge's boolean instead.
    (remote && state.pending_swarm_await == Some(true))
        || state.todos.iter().any(|todo| {
            is_incomplete_todo(todo)
                && !todo_assigned_elsewhere(todo, session_id)
                && !todo_goal_cannot_complete(todo, &state.goals)
                && todo_waits_on_running_task(todo, |id| {
                    state
                        .running_background_tasks
                        .iter()
                        .any(|running| running == id)
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_live_owned_dependency_or_pending_swarm_await_marks_waiting() {
        let mut state: TodoSnapshot = serde_json::from_value(serde_json::json!({
            "session_id":"s1", "running_background_tasks":["build"],
            "todos":[{"id":"a","content":"Check", "priority":"high", "status":"pending", "waiting_on":["build"]}]
        })).unwrap();
        assert!(waiting_on_dependencies(&state, "s1", false));
        state.todos[0].assigned_to = Some("some-other-session".into());
        assert!(!waiting_on_dependencies(&state, "s1", false));
        state.todos[0].assigned_to = None;
        state.todos[0].status = "completed".into();
        assert!(!waiting_on_dependencies(&state, "s1", false));
        state.todos[0].status = "pending".into();
        state.running_background_tasks.clear();
        assert!(!waiting_on_dependencies(&state, "s1", false));
        state.pending_swarm_await = Some(true);
        assert!(waiting_on_dependencies(&state, "s1", true));
        assert!(!waiting_on_dependencies(&state, "s1", false));
        state.pending_swarm_await = Some(false);
        assert!(!waiting_on_dependencies(&state, "s1", true));
    }
}
