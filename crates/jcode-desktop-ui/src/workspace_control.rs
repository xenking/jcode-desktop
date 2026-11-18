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
                if let DesktopAction::Click { target, selector, allow_user_window } = &pending.request.action
                    && crate::swarm_control_targets::is_selector(selector)
                {
                    let result = if pending.request.v != DESKTOP_CONTROL_VERSION {
                        error("version", "unsupported Desktop control version")
                    } else if !allow_user_window || !crate::swarm_control_targets::enabled() {
                        error("user_window_protected", "swarm clicks require the isolated control fixture and allow_user_window")
                    } else {
                        let Ok(validated) = this.update_in(cx, |workspace, window, cx| {
                            workspace.control_target(target, window, cx)
                        }) else { break };
                        match validated {
                            Err(response) => response,
                            Ok(_) => {
                                // Dispatch outside the Workspace borrow: its listeners may update it.
                                let Ok(response) = cx.update(|window, cx| {
                                    if crate::swarm_control_targets::click(selector, &target.session_id, window, cx) {
                                        ok(json!({"selector": selector}))
                                    } else {
                                        error("selector_not_visible", "swarm selector is not uniquely visible")
                                    }
                                }) else { break };
                                response
                            }
                        }
                    };
                    let _ = pending.reply.send(result);
                    continue;
                }
                // Hidden windows do not receive compositor frames. Draw outside
                // the Workspace borrow so capture reflects the latest UI actions.
                if matches!(&pending.request.action, DesktopAction::Render { .. })
                    && cx
                        .update(|window, cx| {
                            window.refresh();
                            window.draw(cx).clear(cx);
                        })
                        .is_err()
                {
                    break;
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
                    json!({"window_id":target.window_id, "panel_id":self.slots[index].panel.entity_id().as_u64(), "panel":panel.control_state(window, cx), "search_open":self.resume_open(), "search":self.control_search_state(cx), "navigation":self.navigation_state(window,cx)}),
                )
            }
            DesktopAction::Search {
                target,
                query,
                allow_user_window,
            } => {
                if let Err(e) = self.control_target(&target, window, cx) {
                    return e;
                }
                if !allow_user_window {
                    return error(
                        "user_window_protected",
                        "search changes window focus; set allow_user_window to permit it",
                    );
                }
                self.search_sessions_for(&query, window, cx);
                ok(json!({"query":query}))
            }
            DesktopAction::Click {
                target,
                selector,
                allow_user_window,
            } => {
                let action = if selector == "composer.send" {
                    DesktopAction::Send {
                        target,
                        allow_user_window,
                    }
                } else if let Some(ask) = selector.strip_prefix("ask:") {
                    if !allow_user_window {
                        return error(
                            "user_window_protected",
                            "choosing an ask option changes focus and user selections; set allow_user_window to permit it",
                        );
                    }
                    let index = match self.control_target(&target, window, cx) {
                        Ok(index) => index,
                        Err(error) => return error,
                    };
                    return if self.slots[index]
                        .panel
                        .update(cx, |panel, cx| panel.control_click_ask(ask, window, cx))
                    {
                        ok(json!({"selector": selector}))
                    } else {
                        error(
                            "selector_not_found",
                            "ask option is absent, stale, or not visible",
                        )
                    };
                } else if let Some(call_id) = selector.strip_prefix("tool:") {
                    let index = match self.control_target(&target, window, cx) {
                        Ok(index) => index,
                        Err(error) => return error,
                    };
                    return if self.slots[index]
                        .panel
                        .update(cx, |panel, cx| panel.control_toggle_tool(call_id, cx))
                    {
                        ok(json!({"selector": selector}))
                    } else {
                        error(
                            "selector_not_found",
                            "tool selector is missing or ambiguous",
                        )
                    };
                } else {
                    return error(
                        "selector_not_found",
                        "supported selectors: composer.send, tool:<call_id>, ask:<request_id>:<question>:<option>",
                    );
                };
                self.handle_desktop_request(
                    DesktopRequest {
                        v: DESKTOP_CONTROL_VERSION,
                        action,
                    },
                    window,
                    cx,
                )
            }
            DesktopAction::PressKey {
                target,
                key,
                allow_user_window,
            } => {
                if key.eq_ignore_ascii_case("escape") || key.eq_ignore_ascii_case("esc") {
                    if let Err(error) = self.control_target(&target, window, cx) {
                        return error;
                    }
                    if !allow_user_window {
                        return error(
                            "user_window_protected",
                            "closing search changes window focus; set allow_user_window to permit it",
                        );
                    }
                    if !self.resume_open() {
                        return error("not_open", "search is not open");
                    }
                    self.close_session_search(window, cx);
                    return ok(json!({"search_open": false}));
                }
                if !key.eq_ignore_ascii_case("enter") && !key.eq_ignore_ascii_case("return") {
                    return error(
                        "unsupported",
                        "supported keys: Enter to send, Escape to close search",
                    );
                }
                self.handle_desktop_request(
                    DesktopRequest {
                        v: DESKTOP_CONTROL_VERSION,
                        action: DesktopAction::Send {
                            target,
                            allow_user_window,
                        },
                    },
                    window,
                    cx,
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
                let panel_id = self.slots[index].panel.entity_id();
                let owned = self.control_drafts.get(&panel_id)
                    == Some(&(draft.content.clone(), input.read(cx).control_revision()))
                    && draft.attachments.is_empty()
                    && draft.stashed_prompt.is_none();
                if !allow_user_window
                    && (!draft.content.is_empty()
                        || !draft.attachments.is_empty()
                        || draft.stashed_prompt.is_some())
                    && !owned
                {
                    return error(
                        "draft_not_empty",
                        "refusing to replace an existing draft without allow_user_window",
                    );
                }
                input.update(cx, |input, cx| input.set_content(text.clone(), cx));
                if allow_user_window {
                    self.control_drafts.remove(&panel_id);
                } else {
                    self.control_drafts
                        .insert(panel_id, (text, input.read(cx).control_revision()));
                }
                ok(json!({}))
            }
            DesktopAction::Send {
                target,
                allow_user_window,
            } => {
                let index = match self.control_target(&target, window, cx) {
                    Ok(index) => index,
                    Err(error) => return error,
                };
                let panel = &self.slots[index].panel;
                if panel.read(cx).input_focus_handle(cx).is_focused(window) && !allow_user_window {
                    return error(
                        "user_window_focused",
                        "refusing to send from a focused composer without allow_user_window",
                    );
                }
                let panel_id = panel.entity_id();
                let input = panel.read(cx).input.clone();
                let draft = input.read(cx).snapshot();
                if !allow_user_window
                    && (self.control_drafts.get(&panel_id)
                        != Some(&(draft.content.clone(), input.read(cx).control_revision()))
                        || !draft.attachments.is_empty()
                        || draft.stashed_prompt.is_some())
                {
                    return error(
                        "draft_not_owned",
                        "refusing to send a draft not set by desktop control",
                    );
                }
                let submitted = input.update(cx, |input, cx| input.control_submit(window, cx));
                if !submitted {
                    return error(
                        "not_submitted",
                        "composer is empty, disabled, or contains a slash command",
                    );
                }
                self.control_drafts.remove(&panel_id);
                ok(json!({"submitted":true}))
            }
            DesktopAction::ExpandTool {
                target, selector, ..
            } => {
                let index = match self.control_target(&target, window, cx) {
                    Ok(index) => index,
                    Err(error) => return error,
                };
                if self.slots[index]
                    .panel
                    .update(cx, |panel, cx| panel.control_expand_tool(&selector, cx))
                {
                    ok(json!({"selector": selector, "expanded": true}))
                } else {
                    error(
                        "selector_not_found",
                        "tool call ID is not present in this session",
                    )
                }
            }
            DesktopAction::Render {
                target,
                selector,
                path,
            } => {
                let index = match self.control_target(&target, window, cx) {
                    Ok(index) => index,
                    Err(e) => return e,
                };
                let bounds = match selector.as_deref() {
                    Some(selector) => match self.slots[index]
                        .panel
                        .read(cx)
                        .control_element_bounds(selector, window)
                    {
                        Some(bounds) => Some(bounds),
                        None => {
                            return error(
                                "selector_not_visible",
                                "selector is not uniquely visible in the last frame",
                            );
                        }
                    },
                    None => None,
                };
                let result = (|| -> anyhow::Result<()> {
                    let mut image = window.render_to_image()?;
                    if let Some(bounds) = bounds {
                        let viewport = window.viewport_size();
                        let sx = image.width() as f32 / f32::from(viewport.width);
                        let sy = image.height() as f32 / f32::from(viewport.height);
                        let x = (f32::from(bounds.left()) * sx).floor().max(0.) as u32;
                        let y = (f32::from(bounds.top()) * sy).floor().max(0.) as u32;
                        let right =
                            ((f32::from(bounds.right()) * sx).ceil() as u32).min(image.width());
                        let bottom =
                            ((f32::from(bounds.bottom()) * sy).ceil() as u32).min(image.height());
                        if right <= x || bottom <= y {
                            anyhow::bail!("selector has empty bounds");
                        }
                        image = image::imageops::crop_imm(&image, x, y, right - x, bottom - y)
                            .to_image();
                    }
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
