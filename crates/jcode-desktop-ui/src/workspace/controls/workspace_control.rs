//! Semantic control of a specific workspace window and session.
use super::*;
use jcode_sdk::desktop_control::{
    DESKTOP_CONTROL_VERSION, DesktopAction, DesktopRequest, DesktopResponse, DesktopTarget,
};
use serde_json::json;

#[cfg(target_os = "macos")]
#[path = "computer_control.rs"]
mod computer_control;

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

#[cfg(unix)]
fn handle_window_request(
    request: DesktopRequest,
    resolve_chat: impl Fn(&DesktopTarget, &Window, &App) -> Result<Entity<Panel>, DesktopResponse>,
    semantic: impl FnOnce(DesktopRequest, &mut Window, &mut App) -> DesktopResponse,
    fixture_input_enabled: bool,
    window: &mut Window,
    cx: &mut App,
) -> DesktopResponse {
    if request.v != DESKTOP_CONTROL_VERSION {
        return error("version", "unsupported Desktop control version");
    }
    if let DesktopAction::Click {
        target,
        selector,
        allow_user_window,
    } = &request.action
        && crate::fixture_control_targets::is_selector(selector)
    {
        if !allow_user_window || !fixture_input_enabled {
            return error(
                "user_window_protected",
                "fixture input requires the isolated control fixture and allow_user_window",
            );
        }
        let panel = match resolve_chat(target, window, cx) {
            Ok(panel) => panel,
            Err(response) => return response,
        };
        if matches!(
            selector.as_str(),
            "composer:wheel-up" | "composer:wheel-down"
        ) {
            window.refresh();
            window.draw(cx).clear(cx);
            let position = panel.read(cx).composer_wheel_position(cx);
            let y = if selector == "composer:wheel-up" {
                12_000.
            } else {
                -12_000.
            };
            window.dispatch_event(
                gpui::PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                    position,
                    delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(y))),
                    touch_phase: gpui::TouchPhase::Moved,
                    modifiers: Default::default(),
                }),
                cx,
            );
            return ok(json!({"selector": selector}));
        }
        // Platform input can synchronously call listeners that update the Workspace.
        // Route every synthetic event here, before taking its update lease.
        return if crate::fixture_control_targets::dispatch(selector, &target.session_id, window, cx)
        {
            ok(json!({"selector": selector}))
        } else {
            error(
                "selector_not_visible",
                "fixture selector is not uniquely visible",
            )
        };
    }
    if let DesktopAction::PressKey {
        target,
        key,
        allow_user_window,
    } = &request.action
        && let Some(key) = key.strip_prefix("fixture:")
    {
        if !allow_user_window || !fixture_input_enabled {
            return error(
                "user_window_protected",
                "fixture keys require the isolated fixture",
            );
        }
        let panel = match resolve_chat(target, window, cx) {
            Ok(panel) => panel,
            Err(response) => return response,
        };
        if let Some(text) = key.strip_prefix("follow-up-text:")
            && crate::fixture_control_targets::side_questions()
        {
            let Some(editor) = panel.read(cx).side_question_test_editor() else {
                return error(
                    "selector_not_visible",
                    "side question follow-up is not open",
                );
            };
            if !editor.read(cx).is_focused(window) {
                return error(
                    "selector_not_focused",
                    "side question follow-up must have native focus",
                );
            }
            editor.update(cx, |input, cx| input.recall_prompt(text, Vec::new(), cx));
            return ok(json!({"key": "follow-up-text"}));
        }
        if key == "seed-composer-image"
            && crate::fixture_control_targets::transcript_scroll_fixture()
        {
            let input = panel.read(cx).input.clone();
            input.update(cx, |input, cx| {
                input.attach_image(
                    crate::clipboard_image::ClipboardImage {
                        media_type: "image/png".into(),
                        bytes: include_bytes!("../../../../../assets/previews/image-preview.png")
                            .to_vec(),
                        width: 640,
                        height: 480,
                    },
                    cx,
                );
            });
            return ok(json!({"key": key}));
        }
        if !matches!(
            key,
            "tab"
                | "shift-tab"
                | "enter"
                | "space"
                | "escape"
                | "up"
                | "down"
                | "left"
                | "right"
                | "ctrl-s"
                | "ctrl-enter"
                | "home"
                | "end"
                | "0"
                | "1"
                | "2"
                | "3"
                | "4"
                | "5"
                | "6"
                | "7"
                | "8"
                | "9"
                | "e"
                | "n"
                | "backspace"
                | "pageup"
                | "alt-shift-up"
                | "alt-shift-down"
                | "alt-shift-left"
                | "alt-shift-right"
                | "super-shift-j"
                | crate::panel::shortcuts::CLEAN_CHAT_CHORD
        ) && !(cfg!(target_os = "macos") && key == "super-v")
        {
            return error("unsupported_key", "unknown fixture key");
        }
        let Ok(keystroke) = gpui::Keystroke::parse(key) else {
            return error("unsupported_key", "invalid fixture key");
        };
        // Never read or change the system pasteboard from a hidden smoke run.
        #[cfg(target_os = "macos")]
        if key == "super-v" {
            if !panel.read(cx).composer_focused(window, cx) {
                return error(
                    "selector_not_focused",
                    "image paste requires composer focus",
                );
            }
            cx.set_global(crate::clipboard_image::ControlPasteImage);
        }
        window.dispatch_event(
            gpui::PlatformInput::KeyDown(gpui::KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            }),
            cx,
        );
        window.dispatch_event(
            gpui::PlatformInput::KeyUp(gpui::KeyUpEvent { keystroke }),
            cx,
        );
        #[cfg(target_os = "macos")]
        if key == "super-v" {
            cx.remove_global::<crate::clipboard_image::ControlPasteImage>();
        }
        window.refresh();
        window.draw(cx).clear(cx);
        return ok(json!({"key": key}));
    }
    // Hidden windows do not receive compositor frames. Drawing can also
    // update entities, so refresh before leasing the Workspace for capture.
    if matches!(&request.action, DesktopAction::Render { .. })
        || (fixture_input_enabled
            && (crate::fixture_control_targets::resume_preview()
                || crate::fixture_control_targets::gallery_navigation()
                || crate::fixture_control_targets::transcript_scroll_fixture())
            && matches!(&request.action, DesktopAction::State { .. }))
    {
        window.refresh();
        window.draw(cx).clear(cx);
    }
    semantic(request, window, cx)
}

impl Workspace {
    #[cfg(unix)]
    pub(super) fn route_window_request(
        workspace: &Entity<Self>,
        request: DesktopRequest,
        fixture_input_enabled: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> DesktopResponse {
        handle_window_request(
            request,
            |target, window, cx| workspace.read(cx).control_chat(target, window, cx),
            |request, window, cx| workspace.update(cx, |workspace, cx| {
                workspace.handle_desktop_request(request, window, cx)
            }),
            fixture_input_enabled,
            window,
            cx,
        )
    }
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
                let Some(workspace) = this.upgrade() else {
                    break;
                };
                #[cfg(target_os = "macos")]
                if matches!(&pending.request.action, DesktopAction::ComputerUse { .. }) {
                    let workspace = workspace.downgrade();
                    computer_control::handle(pending, cx, move |target, window, cx| {
                        let workspace = workspace.upgrade().ok_or_else(|| {
                            error("desktop_unavailable", "Desktop workspace closed")
                        })?;
                        workspace.read(cx).control_target(target, window, cx)?;
                        Ok(())
                    });
                    continue;
                }
                let Ok(response) = cx.update(|window, cx| {
                    Self::route_window_request(
                        &workspace,
                        pending.request,
                        crate::fixture_control_targets::enabled(),
                        window,
                        cx,
                    )
                }) else {
                    break;
                };
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
            .filter(|(_, slot)| !slot.closing && slot.panel.session_id(cx) == target.session_id)
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

    /// The conversation behind `target`, for actions on its transcript or composer.
    fn control_chat(
        &self,
        target: &DesktopTarget,
        window: &Window,
        cx: &App,
    ) -> Result<Entity<Panel>, DesktopResponse> {
        let index = self.control_target(target, window, cx)?;
        self.slots[index]
            .panel
            .chat()
            .cloned()
            .ok_or_else(|| error("unsupported_panel", "this panel has no conversation"))
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
            DesktopAction::ComputerTarget { session_id } => {
                #[cfg(target_os = "macos")]
                {
                    let target = DesktopTarget {
                        window_id: window_id(window),
                        session_id,
                    };
                    match self.control_target(&target, window, cx)
                        .and_then(|_| computer_control::require_local_session(&target)) {
                        Ok(()) => ok(json!(target)),
                        Err(response) => response,
                    }
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = session_id;
                    error("unsupported", "native computer use requires macOS")
                }
            }
            DesktopAction::ComputerUse { .. } => error(
                "unsupported",
                "native computer use requires the Desktop broker",
            ),
            DesktopAction::OpenSession {
                session_id,
                destination: jcode_sdk::desktop_control::OpenDestination::NewWindow,
            } => {
                if !self
                    .sessions
                    .iter()
                    .any(|session| session.session_id == session_id)
                {
                    return error("session_not_found", "session is not in the session catalog");
                }
                let launch = jcode_desktop_api::WindowLaunch::from_parts(
                    [
                        "--single-panel".to_owned(),
                        format!("--session={session_id}"),
                    ],
                    [],
                );
                match jcode_desktop_api::WindowLaunches::open_in_background(cx, launch) {
                    Ok(id) => ok(
                        json!({"window_id": format!("{}:{id}", std::process::id()), "session_id": session_id}),
                    ),
                    Err(reason) => error("open_window_failed", &reason),
                }
            }
            DesktopAction::OpenSession {
                session_id,
                destination:
                    jcode_sdk::desktop_control::OpenDestination::BackgroundPanel {
                        window_id: target_window,
                        workspace,
                    },
            } => {
                if target_window != window_id(window) {
                    return error("window_not_found", "window id does not match endpoint");
                }
                // One-based, as the user sees it. Every workspace of the strip
                // exists from the start, so an empty one is not created, only
                // filled. Absent means the active workspace.
                let row = match workspace {
                    None => self.active_row,
                    Some(number) if (1..=STRIP_COUNT).contains(&number) => number - 1,
                    Some(_) => {
                        return error(
                            "workspace_out_of_range",
                            &format!("workspace must be between 1 and {STRIP_COUNT}"),
                        );
                    }
                };
                if self
                    .slots
                    .iter()
                    .any(|slot| !slot.closing && slot.panel.session_id(cx) == session_id)
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
                let index = self.open_session_in_row(session, row, cx);
                let panel_id = self.slots[index].panel.entity_id().as_u64();
                cx.notify();
                ok(
                    json!({"window_id": target_window, "session_id": session_id, "panel_id": panel_id, "workspace": row + 1}),
                )
            }
            DesktopAction::List {} => {
                let sessions = match serde_json::to_value(&self.sessions) {
                    Ok(sessions) => sessions,
                    Err(err) => return error("state_serialization", &err.to_string()),
                };
                ok(
                    json!({"window_id": window_id(window), "navigation": self.navigation_state(window, cx), "sessions": sessions}),
                )
            }
            DesktopAction::Focus {
                target,
                allow_user_window,
            } => {
                if !allow_user_window {
                    return error("user_window_protected", "focus requires allow_user_window");
                }
                let index = match self.control_target(&target, window, cx) {
                    Ok(index) => index,
                    Err(response) => return response,
                };
                if self.account_sign_in_visible || self.onboarding_launch.error.is_some() {
                    return error(
                        "window_busy",
                        "dismiss the current dialog before focusing a session",
                    );
                }
                if self.resume_open() {
                    self.close_session_search(window, cx);
                }
                self.set_active(index, cx);
                self.overview = false;
                self.overview_progress.set(0.0, Instant::now());
                self.focus_active(window, cx);
                if !crate::fixture_control_targets::enabled() {
                    window.activate_window();
                    cx.activate(true);
                }
                ok(json!({"window_id": target.window_id, "session_id": target.session_id}))
            }
            DesktopAction::Reveal { target, row } => {
                let panel = match self.control_chat(&target, window, cx) {
                    Ok(panel) => panel,
                    Err(response) => return response,
                };
                match panel.update(cx, |panel, cx| panel.control_reveal(row, cx)) {
                    Ok(range) => ok(json!({"index": range.start(), "end_index": range.end()})),
                    Err((code, message)) => error(code, message),
                }
            }
            DesktopAction::State { target } => {
                let entity = match self.control_chat(&target, window, cx) {
                    Ok(panel) => panel,
                    Err(error) => return error,
                };
                let panel = entity.read(cx);
                let (draft, draft_revision) = panel.draft_fingerprint(cx);
                let mut state = match panel.control_state(window, cx) {
                    Ok(state) => state,
                    Err(err) => return error("state_serialization", &err.to_string()),
                };
                let search = match self.control_search_state(window, cx) {
                    Ok(search) => search,
                    Err(err) => return error("state_serialization", &err.to_string()),
                };
                state["agent_owned_draft"] = json!(
                    self.control_drafts.get(&entity.entity_id())
                        == Some(&(draft.content, draft_revision))
                        && draft.attachments.is_empty()
                        && draft.stashed_prompt.is_none()
                );
                ok(
                    json!({"window_id":target.window_id, "panel_id":entity.entity_id().as_u64(), "panel":state, "search_open":self.resume_open(), "search":search, "navigation":self.navigation_state(window,cx), "fixture_sidebar_sessions": (crate::fixture_control_targets::draft_session() || crate::fixture_control_targets::side_questions()).then(|| self.sidebar_session_layout.iter().map(|row| row.session_id.as_str()).collect::<Vec<_>>())}),
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
                    let panel = match self.control_chat(&target, window, cx) {
                        Ok(panel) => panel,
                        Err(error) => return error,
                    };
                    if panel.read(cx).ask_needs_human(ask) {
                        return error(
                            "human_approval_required",
                            "this approval can only be answered by a person directly in the chat",
                        );
                    }
                    return if panel.update(cx, |panel, cx| panel.control_click_ask(ask, window, cx))
                    {
                        ok(json!({"selector": selector}))
                    } else {
                        error(
                            "selector_not_found",
                            "ask option is absent, stale, or not visible",
                        )
                    };
                } else if let Some(call_id) = selector.strip_prefix("tool:") {
                    let panel = match self.control_chat(&target, window, cx) {
                        Ok(panel) => panel,
                        Err(error) => return error,
                    };
                    return if panel.update(cx, |panel, cx| panel.control_toggle_tool(call_id, cx)) {
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
                let panel = match self.control_chat(&target, window, cx) {
                    Ok(panel) => panel,
                    Err(error) => return error,
                };
                if panel.read(cx).input_focus_handle(cx).is_focused(window) && !allow_user_window {
                    return error(
                        "user_window_focused",
                        "refusing to modify the user's focused composer without allow_user_window",
                    );
                }
                let input = panel.read(cx).input.clone();
                let draft = input.read(cx).snapshot();
                let panel_id = panel.entity_id();
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
                let panel = match self.control_chat(&target, window, cx) {
                    Ok(panel) => panel,
                    Err(error) => return error,
                };
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
                    return error("not_submitted", "composer is empty or disabled");
                }
                self.control_drafts.remove(&panel_id);
                ok(json!({"submitted":true}))
            }
            DesktopAction::ExpandTool {
                target, selector, ..
            } => {
                let panel = match self.control_chat(&target, window, cx) {
                    Ok(panel) => panel,
                    Err(error) => return error,
                };
                if panel.update(cx, |panel, cx| panel.control_expand_tool(&selector, cx)) {
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
                        .read_chat(cx)
                        .and_then(|panel| panel.control_element_bounds(selector, window))
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
