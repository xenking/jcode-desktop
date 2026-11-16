//! Semantic control of a specific workspace window and session.
use super::*;
use jcode_sdk::desktop_control::{
    DESKTOP_CONTROL_VERSION, DesktopAction, DesktopRequest, DesktopResponse, DesktopTarget,
};
use serde_json::json;

fn error(code: &str, message: &str) -> DesktopResponse {
    DesktopResponse::Error {
        code: code.into(),
        message: message.into(),
    }
}
fn ok(result: serde_json::Value) -> DesktopResponse {
    DesktopResponse::Ok { result }
}
fn window_id(window: &Window) -> String {
    format!(
        "{}:{}",
        std::process::id(),
        window.window_handle().window_id().as_u64()
    )
}

impl Workspace {
    #[cfg(unix)]
    pub(super) fn start_desktop_control(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = window.window_handle().window_id().as_u64();
        let (server, requests) = match crate::desktop_control::Server::start(id) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("desktop control unavailable: {error}");
                return;
            }
        };
        self.desktop_control = Some(server);
        self.desktop_control_task = Some(cx.spawn_in(window, async move |this, cx| {
            while let Ok(pending) = requests.recv().await {
                if Instant::now() >= pending.deadline {
                    continue;
                }
                let response = this.update_in(cx, |workspace, window, cx| {
                    workspace.handle_desktop_request(pending.request, window, cx)
                });
                let Ok(response) = response else { break };
                let _ = pending.reply.send(response);
            }
        }));
    }

    fn control_target(
        &self,
        target: &DesktopTarget,
        window: &Window,
        cx: &App,
    ) -> Result<usize, DesktopResponse> {
        if target.window_id != window_id(window) {
            return Err(error(
                "window_not_found",
                "window id does not match endpoint",
            ));
        }
        let mut found = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| {
                !slot.closing && slot.panel.read(cx).session_id == target.session_id
            })
            .map(|(i, _)| i);
        let Some(index) = found.next() else {
            return Err(error(
                "session_not_found",
                "session is not open in this window",
            ));
        };
        if found.next().is_some() {
            return Err(error(
                "ambiguous_session",
                "session appears in multiple panels",
            ));
        }
        Ok(index)
    }

    pub(super) fn handle_desktop_request(
        &mut self,
        request: DesktopRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> DesktopResponse {
        if request.v != DESKTOP_CONTROL_VERSION {
            return error("version", "unsupported Desktop control version");
        }
        match request.action {
            DesktopAction::OpenSession {
                session_id,
                destination:
                    jcode_sdk::desktop_control::OpenDestination::BackgroundPanel {
                        window_id: target_window,
                    },
            } => {
                if target_window != window_id(window) {
                    return error("window_not_found", "window id does not match endpoint");
                }
                if self
                    .slots
                    .iter()
                    .any(|slot| !slot.closing && slot.panel.read(cx).session_id == session_id)
                {
                    return error("already_open", "session is already open in this window");
                }
                let Some(session) = self
                    .sessions
                    .iter()
                    .find(|session| session.session_id == session_id)
                    .cloned()
                else {
                    return error("session_not_found", "session is not in the session catalog");
                };
                let index = self.open_session(session, cx);
                let panel_id = self.slots[index].panel.entity_id().as_u64();
                cx.notify();
                ok(
                    json!({"window_id": target_window, "session_id": session_id, "panel_id": panel_id}),
                )
            }
            DesktopAction::List {} => ok(
                json!({"window_id":window_id(window), "navigation": self.navigation_state(window, cx), "sessions":self.sessions}),
            ),
            DesktopAction::State { target } => {
                let index = match self.control_target(&target, window, cx) {
                    Ok(index) => index,
                    Err(error) => return error,
                };
                let panel = self.slots[index].panel.read(cx);
                ok(
                    json!({"window_id":target.window_id, "panel_id":self.slots[index].panel.entity_id().as_u64(), "panel":panel.control_state(cx), "search_open":self.search_screen.is_some(), "navigation":self.navigation_state(window,cx)}),
                )
            }
            DesktopAction::SetComposer {
                target,
                text,
                allow_user_window,
            } => {
                let index = match self.control_target(&target, window, cx) {
                    Ok(index) => index,
                    Err(error) => return error,
                };
                if self.slots[index]
                    .panel
                    .read(cx)
                    .input_focus_handle(cx)
                    .is_focused(window)
                    && !allow_user_window
                {
                    return error(
                        "user_window_focused",
                        "refusing to modify the user's focused composer without allow_user_window",
                    );
                }
                let input = self.slots[index].panel.read(cx).input.clone();
                let draft = input.read(cx).snapshot();
                if !allow_user_window
                    && (!draft.content.is_empty() || !draft.attachments.is_empty())
                {
                    return error(
                        "draft_not_empty",
                        "refusing to replace an existing draft without allow_user_window",
                    );
                }
                input.update(cx, |input, cx| input.set_content(text, cx));
                ok(json!({}))
            }
            DesktopAction::Render {
                target,
                selector,
                path,
            } => {
                if let Err(e) = self.control_target(&target, window, cx) {
                    return e;
                }
                if selector.is_some() {
                    return error("unsupported", "element rendering is not implemented");
                }
                let result = (|| -> anyhow::Result<()> {
                    let image = window.render_to_image()?;
                    let mut file = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)?;
                    image.write_to(&mut file, image::ImageFormat::Png)?;
                    Ok(())
                })();
                match result {
                    Ok(()) => ok(json!({"path":path})),
                    Err(e) => error("render_failed", &e.to_string()),
                }
            }
            _ => error("unsupported", "this Desktop action is not implemented yet"),
        }
    }
}
