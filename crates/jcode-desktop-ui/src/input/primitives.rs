//! The prompt's construction, configuration, getters and selection primitives:
//! what the menus and pickers beneath the editor build on.
use super::*;

///
/// Keeps the current model when the target provider serves it. Otherwise
/// lands on the provider's newest flagship using the daemon's shared ranking
/// (`preferred_model_for_provider`), not the alphabetically first route, which
/// previously put Copilot users on `claude-opus-4.6`.
pub(super) fn provider_route_spec_in(
    details: &HashMap<String, model_menu::ModelDetails>,
    provider: &str,
    current: Option<&str>,
) -> Option<String> {
    let wanted = provider_route_methods(provider);
    let mut matches: Vec<_> = details
        .iter()
        .filter(|(_, detail)| {
            let method = detail.api_method.to_ascii_lowercase();
            let owner = detail.provider.to_ascii_lowercase();
            wanted.iter().any(|wanted| {
                method == *wanted || method.starts_with(&format!("{wanted}-")) || owner == *wanted
            })
        })
        .collect();
    if let Some((spec, _)) = matches
        .iter()
        .find(|(_, detail)| current.is_some_and(|model| detail.model == model))
    {
        return Some((*spec).clone());
    }
    let candidates: Vec<&str> = matches
        .iter()
        .map(|(_, detail)| detail.model.as_str())
        .collect();
    if let Some(best) =
        jcode_base::auth::lifecycle::preferred_model_for_provider(provider, &candidates)
        && let Some((spec, _)) = matches.iter().find(|(_, detail)| detail.model == best)
    {
        return Some((*spec).clone());
    }
    matches.sort_by(|(a, left), (b, right)| {
        right
            .recommended
            .cmp(&left.recommended)
            .then_with(|| a.cmp(b))
    });
    matches.first().map(|(spec, _)| (*spec).clone())
}

#[cfg(test)]
mod provider_route_spec_tests {
    use super::*;

    fn details(routes: &[(&str, &str, &str)]) -> HashMap<String, model_menu::ModelDetails> {
        routes
            .iter()
            .map(|(model, provider, method)| {
                (
                    format!("{method}:{model}"),
                    model_menu::ModelDetails {
                        model: (*model).into(),
                        recommended: false,
                        provider: (*provider).into(),
                        api_method: (*method).into(),
                        usage: None,
                        speed_options: Vec::new(),
                        account_name: None,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn switching_provider_lands_on_newest_flagship() {
        let catalog = details(&[
            ("claude-haiku-4.5", "Copilot", "copilot"),
            ("claude-opus-4.6", "Copilot", "copilot"),
            ("claude-opus-5.5", "Copilot", "copilot"),
            ("anthropic/claude-sonnet-4", "auto", "openrouter"),
            ("anthropic/claude-opus-4.8", "auto", "openrouter"),
            ("grok-code-fast-1", "Grok Build", "grok-build-acp"),
            ("grok-4.7", "Grok Build", "grok-build-acp"),
            ("claude-opus-4-6", "Anthropic", "claude-oauth"),
            ("claude-opus-5-5", "Anthropic", "claude-oauth"),
            ("gpt-5.5", "OpenAI", "openai-oauth"),
            ("gpt-6-astra", "OpenAI", "openai-oauth"),
        ]);
        let cases = [
            ("copilot", "copilot:claude-opus-5.5"),
            ("openrouter", "openrouter:anthropic/claude-opus-4.8"),
            ("grok-build", "grok-build-acp:grok-4.7"),
            ("claude", "claude-oauth:claude-opus-5-5"),
            ("openai", "openai-oauth:gpt-6-astra"),
        ];
        for (provider, expected) in cases {
            assert_eq!(
                provider_route_spec_in(&catalog, provider, Some("some-other-model")).as_deref(),
                Some(expected),
                "{provider}"
            );
        }
    }

    #[test]
    fn switching_provider_keeps_current_model_when_served() {
        let catalog = details(&[
            ("claude-opus-4.6", "Copilot", "copilot"),
            ("claude-opus-5.5", "Copilot", "copilot"),
        ]);
        assert_eq!(
            provider_route_spec_in(&catalog, "copilot", Some("claude-opus-4.6")).as_deref(),
            Some("copilot:claude-opus-4.6")
        );
    }
}

/// A route for a signed-in provider the session catalog does not list yet.
///
/// A session's catalog follows its current runtime, so after a switch to one
/// provider the others can be missing from it. Without this the picker had no
/// way back: every other row was inert until the user signed in again.
pub(crate) fn provider_fallback_spec(provider: &str) -> Option<String> {
    let prefix = match provider {
        "claude" => "claude-oauth",
        "anthropic-api" => "claude-api",
        "openai" => "openai-oauth",
        "openai-api" => "openai-api",
        "copilot" | "cursor" | "gemini" | "antigravity" => provider,
        other => {
            let profile = jcode_base::provider_catalog::openai_compatible_profile_by_id(other)?;
            return profile
                .default_model
                .map(|model| format!("{}:{model}", profile.id));
        }
    };
    let model = jcode_base::auth::lifecycle::default_model_for_provider(provider)?;
    Some(format!("{prefix}:{model}"))
}

pub(super) fn provider_route_methods(provider: &str) -> Vec<String> {
    match provider {
        "openai" => vec!["openai-oauth".into()],
        "openai-api" => vec!["openai-api-key".into(), "openai-api".into()],
        "claude" => vec!["claude-oauth".into()],
        "anthropic-api" => vec![
            "anthropic-api-key".into(),
            "anthropic-api".into(),
            "claude-api".into(),
        ],
        other => vec![other.into()],
    }
}

impl PromptInput {
    pub(crate) fn composer_wheel_position(&self) -> Point<Pixels> {
        self.editor_scroll.bounds().center()
    }

    pub(crate) fn has_draft_images(&self) -> bool {
        self.attachments
            .iter()
            .chain(self.live_draft.iter().flat_map(|draft| &draft.attachments))
            .chain(
                self.stashed_prompt
                    .iter()
                    .flat_map(|(_, _, attachments)| attachments),
            )
            .any(|attachment| {
                matches!(
                    attachment.payload,
                    AttachmentPayload::Image { .. }
                        | AttachmentPayload::File { image: Some(_), .. }
                )
            })
    }

    pub(crate) fn editor_wheel_ownership(
        &self,
        event: &gpui::ScrollWheelEvent,
        in_composer: bool,
    ) -> (bool, bool) {
        let over_editor = in_composer && self.editor_scroll.bounds().contains(&event.position);
        let owned_gesture = event.delta.precise()
            && match event.touch_phase {
                gpui::TouchPhase::Started => {
                    self.editor_wheel_gesture.set(over_editor);
                    over_editor
                }
                gpui::TouchPhase::Ended | gpui::TouchPhase::Cancelled => {
                    self.editor_wheel_gesture.replace(false)
                }
                gpui::TouchPhase::Moved => self.editor_wheel_gesture.get(),
            };
        (over_editor, owned_gesture)
    }

    #[cfg(test)]
    pub(crate) fn test_editor_scroll(&self) -> gpui::ScrollHandle {
        self.editor_scroll.clone()
    }

    pub(crate) fn picker_settings(&self) -> PickerSettings {
        PickerSettings {
            models: self.command_models.clone(),
            details: self.model_details.clone(),
            current_model: self.current_model.clone(),
            effort_ladder: self.effort_ladder.clone(),
            current_effort: self.current_effort.clone(),
            skills: self.command_skills.clone(),
            current_modes: self.current_modes.clone(),
            mode_catalog: self.mode_catalog.clone(),
        }
    }

    pub(crate) fn apply_picker_settings(
        &mut self,
        settings: PickerSettings,
        cx: &mut Context<Self>,
    ) {
        self.command_models = settings.models;
        self.model_details = settings.details;
        self.current_model = settings.current_model;
        self.effort_ladder = settings.effort_ladder;
        self.current_effort = settings.current_effort;
        self.command_skills = settings.skills;
        self.current_modes = settings.current_modes;
        self.mode_catalog = settings.mode_catalog;
        self.suggestions_revision += 1;
        cx.notify();
    }

    pub(super) fn chip_range(&self, attachment: &Attachment) -> Option<Range<usize>> {
        let marker = attachment.label.as_ref();
        let start = self.content.find(marker)?;
        Some(start..start + marker.len())
    }

    pub(super) fn chip_ranges(&self) -> Vec<(usize, Range<usize>)> {
        self.attachments
            .iter()
            .enumerate()
            .filter_map(|(index, attachment)| {
                self.chip_range(attachment).map(|range| (index, range))
            })
            .collect()
    }

    pub(super) fn image_chip_numbers(&self) -> Vec<Option<usize>> {
        let mut ranges = self.chip_ranges();
        ranges.sort_unstable_by_key(|(_, range)| range.start);
        let mut numbers = vec![None; self.attachments.len()];
        let mut number = self.session_image_base;
        for (index, _) in ranges {
            if matches!(
                self.attachments[index].payload,
                AttachmentPayload::Image { .. }
                    | AttachmentPayload::Queued {
                        source: QueuedAttachment::Image(..),
                        ..
                    }
            ) {
                number += 1;
                numbers[index] = Some(number);
            }
        }
        numbers
    }

    #[cfg(test)]
    pub(crate) fn session_image_base(&self) -> usize {
        self.session_image_base
    }

    pub(crate) fn set_session_image_base(&mut self, base: usize, cx: &mut Context<Self>) {
        if self.session_image_base < base {
            self.session_image_base = base;
            cx.notify();
        }
    }

    pub(super) fn remove_chips_in_range(&mut self, range: &Range<usize>) {
        let remove: Vec<usize> = self
            .chip_ranges()
            .into_iter()
            .filter_map(|(index, chip)| {
                (range.start < chip.end && range.end > chip.start).then_some(index)
            })
            .collect();
        for index in remove.into_iter().rev() {
            self.attachments.remove(index);
        }
        self.attachment_preview = None;
    }

    pub fn new(
        cx: &mut Context<Self>,
        placeholder: impl Into<SharedString>,
        on_submit: impl Fn(String, Vec<(String, String)>, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self::new_with_queue(cx, placeholder, move |text, images, _, window, cx| {
            on_submit(text, images, window, cx)
        })
    }

    pub(crate) fn new_with_queue(
        cx: &mut Context<Self>,
        placeholder: impl Into<SharedString>,
        on_submit: impl Fn(String, Vec<(String, String)>, bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        let mut input = Self::new_with_queue_files(
            cx,
            placeholder,
            move |_, text, images, _, queued, window, cx| {
                on_submit(text, images, queued, window, cx)
            },
        );
        input.file_attachments_enabled = false;
        input
    }

    pub(crate) fn new_with_queue_files(
        cx: &mut Context<Self>,
        placeholder: impl Into<SharedString>,
        on_submit: impl Fn(
            SubmissionKind,
            String,
            Vec<(String, String)>,
            Vec<SubmittedFile>,
            bool,
            &mut Window,
            &mut App,
        ) + 'static,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            session_image_base: 0,
            #[cfg(target_os = "macos")]
            ime_log_focus_subscriptions: None,
            content: "".into(),
            placeholder: placeholder.into(),
            external_picker_open: false,
            sel: FieldSelection::default(),
            last_layout: None,
            ime_geometry: None,
            last_layout_revision: 0,
            vertical_caret: None,
            last_bounds: None,
            chip_positions: Vec::new(),
            last_line_height: None,
            voice_preview: None,
            live_dictation: None,
            voice_preview_hint: None,
            voice_preview_scroll: gpui::ScrollHandle::new(),
            composer_bounds: Default::default(),
            visual_line_count: 1,
            is_selecting: false,
            undo: Vec::new(),
            redo: Vec::new(),
            history: Vec::new(),
            history_index: None,
            live_draft: None,
            attachments: Vec::new(),
            stashed_prompt: None,
            model_menu_draft: None,
            attachment_notice: None,
            transient_notice: None,
            transient_notice_generation: 0,
            attachment_preview: None,
            preview_close_focus: cx.focus_handle(),
            preview_remove_focus: cx.focus_handle(),
            preview_panel_bounds: Default::default(),
            file_attachments_enabled: true,
            paste_mode: PasteMode::Attachments,
            queue_editor: false,
            on_submit: Box::new(on_submit),
            on_change: None,
            on_overlay_cancel: None,
            on_picker_outside: None,
            command_models: Vec::new(),
            command_skills: Vec::new(),
            context_picker: None,
            picker_prefix: None,
            command_completion: true,
            standalone_btw: false,
            command_menu_closed: None,
            command_selection_explicit: None,
            command_selection: 0,
            model_details: HashMap::new(),
            applied_model_routes: None,
            expanded_model_groups: HashSet::new(),
            model_stage: Default::default(),
            current_model: None,
            effort_ladder: crate::effort::ladder(None, None),
            current_effort: None,
            speed_picker: None,
            on_model_speed: None,
            current_modes: Vec::new(),
            mode_catalog: Default::default(),
            picker_effort: None,
            command_scroll: gpui::ListState::new(0, gpui::ListAlignment::Top, px(48.)),
            suggestions_revision: 0,
            control_revision: 0,
            suggestions_cache: RefCell::new(None),
            command_layout_key: None,
            command_entrance: jcode_desktop_motion::MenuEntrance::default(),
            editor_scroll: gpui::ScrollHandle::new(),
            editor_wheel_gesture: Default::default(),
            editor_resize: Default::default(),
            revealed_caret: None,
            submission_enabled: true,
            host_handles_submission: false,
            empty_submit: false,
            pending_session: false,
            spacious: false,
            compact_padding: false,
            trailing_inset: 0.,
            chrome: true,
            example_prompts: false,
            working_dir: None,
            motion: Default::default(),
            ghost: None,
        }
    }

    /// A preview dialog is open over this field, so Esc closes it first.
    pub(crate) fn has_open_preview(&self) -> bool {
        self.attachment_preview.is_some()
    }

    /// Test hook: laid-out line count and how far the editor can scroll.
    #[cfg(test)]
    pub(crate) fn editor_layout(&self) -> (usize, Pixels) {
        (self.visual_line_count, self.editor_scroll.max_offset().y)
    }

    pub(crate) fn with_empty_submit(mut self) -> Self {
        self.empty_submit = true;
        self
    }

    /// Suggest inline completions (ghost text, Tab accepts) when enabled in
    /// `[desktop.completions]`. `context` supplies recent transcript turns.
    pub(crate) fn with_ghost_completions(
        mut self,
        context: impl Fn(&App) -> Vec<crate::prompt_completion::Turn> + 'static,
    ) -> Self {
        if crate::config::completions().enabled {
            self.ghost = Some(GhostState {
                machine: Default::default(),
                context: Box::new(context),
                task: None,
                disabled: false,
                cache: Default::default(),
                #[cfg(test)]
                request_counter: None,
            });
        }
        self
    }

    /// Put the caret at a byte offset for dictation fixtures.
    pub(crate) fn set_caret(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.move_to(offset, cx);
    }

    /// The draft text.
    pub(crate) fn text(&self) -> SharedString {
        self.content.clone()
    }

    /// The draft text as an owned string.
    pub(crate) fn text_owned(&self) -> String {
        self.content.to_string()
    }

    /// The draft text without surrounding whitespace.
    pub(crate) fn trimmed(&self) -> &str {
        self.content.trim()
    }

    /// The handle that routes keyboard focus to this editor.
    pub(crate) fn editor_focus(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub(crate) fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    /// A picker the composer's owner shows hides the example prompts.
    pub(crate) fn set_external_picker_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.external_picker_open != open {
            self.external_picker_open = open;
            cx.notify();
        }
    }

    /// Test hook: current caret byte offset.
    #[cfg(test)]
    pub(crate) fn caret_offset(&self) -> usize {
        self.sel.cursor()
    }

    pub(super) fn dismiss_ghost(&mut self) -> bool {
        let Some(ghost) = self.ghost.as_mut() else {
            return false;
        };
        ghost.task = None;
        ghost.machine.dismiss()
    }

    /// Chat composers show animated example prompts while empty.
    pub(crate) fn with_example_prompts(mut self) -> Self {
        self.example_prompts = true;
        self
    }

    /// Scope personalized example prompts to this project.
    pub(crate) fn set_working_dir(&mut self, working_dir: Option<&str>) {
        if self.working_dir.as_deref() != working_dir {
            self.working_dir = working_dir.map(str::to_owned);
        }
    }

    pub(crate) fn working_dir(&self) -> Option<&str> {
        self.working_dir.as_deref()
    }

    /// Whether `set_spacious` or `set_trailing_inset` with these values would
    /// change anything. Checked before updating while a frame is drawn.
    pub(crate) fn needs_layout_update(&self, spacious: bool, trailing_inset: f32) -> bool {
        self.spacious != spacious || self.trailing_inset != trailing_inset
    }

    pub(crate) fn set_spacious(&mut self, spacious: bool, cx: &mut Context<Self>) {
        if self.spacious != spacious {
            self.spacious = spacious;
            cx.notify();
        }
    }

    /// Reserve room at the right edge for a host-drawn control.
    pub(crate) fn set_trailing_inset(&mut self, inset: f32, cx: &mut Context<Self>) {
        if self.trailing_inset != inset {
            self.trailing_inset = inset;
            cx.notify();
        }
    }

    /// The host draws the fill, border and corners around this field.
    pub(crate) fn without_chrome(mut self) -> Self {
        self.chrome = false;
        self
    }

    pub(crate) fn with_compact_padding(mut self) -> Self {
        self.compact_padding = true;
        self
    }

    pub(crate) fn embedded_editor(cx: &mut Context<Self>, placeholder: &'static str) -> Self {
        let mut editor = Self::new(cx, placeholder, |_, _, _, _| {})
            .without_command_completion()
            .without_chrome()
            .with_compact_padding();
        editor.host_handles_submission = true;
        editor
    }

    /// Plain metadata editors should not interpret titles as slash commands.
    pub(crate) fn without_command_completion(mut self) -> Self {
        self.command_completion = false;
        self
    }

    pub(crate) fn with_standalone_btw(mut self) -> Self {
        self.standalone_btw = true;
        self
    }

    pub(crate) fn text_only(mut self) -> Self {
        self.paste_mode = PasteMode::InlineText;
        self
    }

    pub(crate) fn text_attachments_only(mut self) -> Self {
        self.paste_mode = PasteMode::TextAttachments;
        self
    }

    pub fn with_on_change(mut self, on_change: impl Fn(&str, &mut App) + 'static) -> Self {
        self.on_change = Some(Box::new(on_change));
        self
    }

    /// Explicitly disable submission without disabling editing or focus.
    /// Startup panels instead queue prompts and separately gate slash commands.
    pub fn set_submission_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.submission_enabled = enabled;
        cx.notify();
    }

    pub(crate) fn set_pending_session(&mut self, pending: bool, cx: &mut Context<Self>) {
        self.pending_session = pending;
        if !pending && self.attachment_notice.as_deref() == Some(PENDING_COMMAND_NOTICE) {
            self.attachment_notice = None;
        }
        cx.notify();
    }

    pub fn with_on_overlay_cancel(mut self, cancel: impl Fn(&mut App) -> bool + 'static) -> Self {
        self.on_overlay_cancel = Some(Box::new(cancel));
        self
    }

    pub(crate) fn with_on_model_speed(
        mut self,
        change: impl Fn(ModelSpeedChoice<'_>, &mut App) + 'static,
    ) -> Self {
        self.on_model_speed = Some(Box::new(change));
        self
    }

    pub(crate) fn with_on_picker_outside(
        mut self,
        dismiss: impl Fn(&FocusHandle, &mut Window, &mut App) + 'static,
    ) -> Self {
        let focus = self.focus_handle.clone();
        self.on_picker_outside = Some(std::rc::Rc::new(move |window, cx| {
            dismiss(&focus, window, cx);
        }));
        self
    }

    pub fn set_command_models(&mut self, models: Vec<String>, cx: &mut Context<Self>) {
        if self.command_models != models {
            self.command_models = models;
            self.applied_model_routes = None;
            self.suggestions_revision += 1;
            self.command_selection = 0;
            cx.notify();
        }
    }

    pub fn set_current_model(&mut self, model: Option<String>, cx: &mut Context<Self>) {
        if self.current_model != model {
            self.current_model = model;
            self.suggestions_revision += 1;
            cx.notify();
        }
    }

    pub(crate) fn set_serving_model(
        &mut self,
        model: Option<&str>,
        provider: Option<&str>,
        auth: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let spec = model.and_then(|model| {
            let mut matches = self.model_details.iter().filter(|(_, route)| {
                route.model == model
                    && provider.is_none_or(|provider| route.provider == provider)
                    && auth.is_none_or(|auth| {
                        crate::provider_mark::human_auth_method(&route.api_method) == auth
                    })
            });
            let (spec, _) = matches.next()?;
            matches.next().is_none().then(|| spec.clone())
        });
        self.set_current_model(spec.or_else(|| model.map(str::to_owned)), cx);
    }

    #[cfg(test)]
    pub(crate) fn command_models(&self) -> &[String] {
        &self.command_models
    }

    /// An available catalog spec for a signed-in provider, preferring the current model.
    pub(crate) fn provider_route_spec(
        &self,
        provider: &str,
        current: Option<&str>,
    ) -> Option<String> {
        provider_route_spec_in(&self.model_details, provider, current)
            .or_else(|| provider_fallback_spec(provider))
    }

    pub(super) fn insert_newline(
        &mut self,
        _: &InsertNewline,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_text_in_range(None, "\n", window, cx);
    }

    pub(super) fn literal_submission(text: &str) -> String {
        if text.contains('/') {
            text.to_owned()
        } else {
            text.trim().to_owned()
        }
    }

    pub(super) fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.sel.cursor()), cx);
    }

    pub(super) fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.sel.cursor()), cx);
    }

    pub(super) fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    pub(super) fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    pub(super) fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    pub(super) fn finish_paste(&mut self) {
        if self.picker_prefix.is_none() && self.content.starts_with('/') {
            self.close_command_menu(CommandMenuClose::Literal);
        }
        self.dismiss_ghost();
    }

    pub(super) fn word_left(&mut self, _: &MoveWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(previous_word_boundary(&self.content, self.sel.cursor()), cx);
    }

    pub(super) fn word_right(&mut self, _: &MoveWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(next_word_boundary(&self.content, self.sel.cursor()), cx);
    }

    pub(super) fn select_word_left(
        &mut self,
        _: &SelectWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(previous_word_boundary(&self.content, self.sel.cursor()), cx);
    }

    pub(super) fn select_word_right(
        &mut self,
        _: &SelectWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(next_word_boundary(&self.content, self.sel.cursor()), cx);
    }

    pub(super) fn kill_to_start(
        &mut self,
        _: &KillToStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(0, cx);
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(super) fn kill_to_end(
        &mut self,
        _: &KillToEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.content.len(), cx);
        self.replace_text_in_range(None, "", window, cx);
    }

    /// Last painted composer bounds in window coordinates, if available.
    /// Fall back to the editor viewport, then text bounds before the root is measured.
    pub(crate) fn voice_bounds(&self) -> Option<Bounds<Pixels>> {
        let viewport = self.editor_scroll.bounds();
        let valid = |bounds: &Bounds<Pixels>| {
            f32::from(bounds.origin.x).is_finite()
                && f32::from(bounds.origin.y).is_finite()
                && f32::from(bounds.size.width).is_finite()
                && f32::from(bounds.size.height).is_finite()
                && bounds.size.width > px(0.0)
                && bounds.size.height > px(0.0)
        };
        if let Some(bounds) = self.composer_bounds.get().filter(valid) {
            Some(bounds)
        } else if valid(&viewport) {
            Some(viewport)
        } else {
            self.last_bounds.filter(valid)
        }
    }

    /// Display recognition partials without changing the draft or edit history.
    pub(crate) fn set_voice_preview(&mut self, preview: Option<String>, cx: &mut Context<Self>) {
        if self.voice_preview != preview {
            if preview.is_some() {
                self.voice_preview_scroll.scroll_to_bottom();
            }
            self.voice_preview = preview;
            cx.notify();
        }
    }

    /// Test hook: the dim recognition partial currently shown, if any.
    #[cfg(test)]
    pub(crate) fn voice_preview(&self) -> Option<&str> {
        self.voice_preview.as_deref()
    }

    pub(crate) fn set_speed_picker(
        &mut self,
        picker: crate::speed_icon::SpeedPicker,
        cx: &mut Context<Self>,
    ) {
        if self.speed_picker.as_ref() == Some(&picker) {
            return;
        }
        self.speed_picker = Some(picker);
        self.suggestions_revision += 1;
        cx.notify();
    }

    /// Short feedback under the composer that clears itself, e.g. after an
    /// effort key. A newer notice restarts the timer.
    pub fn set_transient_notice(&mut self, notice: String, cx: &mut Context<Self>) {
        self.transient_notice_generation += 1;
        let generation = self.transient_notice_generation;
        self.transient_notice = Some(notice.into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(2_500))
                .await;
            let _ = this.update(cx, |input, cx| {
                if input.transient_notice_generation == generation {
                    input.transient_notice = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    #[cfg(test)]
    pub(crate) fn transient_notice(&self) -> Option<&str> {
        self.transient_notice.as_deref()
    }

    /// The serving model's effort ladder and level, for the `/effort` menu.
    pub fn set_effort_state(
        &mut self,
        ladder: Vec<&'static str>,
        current: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.effort_ladder != ladder || self.current_effort != current {
            self.effort_ladder = ladder;
            self.current_effort = current;
            self.suggestions_revision += 1;
            cx.notify();
        }
    }

    pub(crate) fn effort_menu_open(&self) -> bool {
        let content = self.content.trim_start();
        content == "/effort" || content.starts_with("/effort ")
    }

    pub(crate) fn control_revision(&self) -> u64 {
        self.control_revision
    }

    pub(crate) fn set_skills(&mut self, skills: Vec<(String, String)>, cx: &mut Context<Self>) {
        self.command_skills = skills;
        self.suggestions_revision += 1;
        cx.notify();
    }

    pub(super) fn toggle_mode_row(&self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let command = if name == "off" {
            "/mode off".to_owned()
        } else {
            format!("/mode toggle {name}")
        };
        (self.on_submit)(
            SubmissionKind::Command,
            command,
            Vec::new(),
            Vec::new(),
            false,
            window,
            cx,
        );
    }

    pub(super) fn current_command_menu_close(&self) -> Option<CommandMenuClose> {
        self.command_menu_closed
            .as_ref()
            .filter(|(draft, _)| draft == &self.content)
            .map(|(_, close)| *close)
    }

    pub(super) fn close_command_menu(&mut self, close: CommandMenuClose) {
        self.command_menu_closed = Some((self.content.clone(), close));
        self.command_selection_explicit = None;
    }

    pub(super) fn on_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.is_selecting = false;
    }

    pub(super) fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.vertical_caret = None;
        self.sel.collapse_to(offset.min(self.content.len()));
        cx.notify()
    }

    pub(super) fn painted_line_height(&self, window: &Window) -> Pixels {
        self.last_line_height
            .unwrap_or_else(|| window.line_height())
    }

    pub(super) fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.vertical_caret = None;
        self.sel.extend_to(offset);
        cx.notify()
    }

    pub(super) fn previous_boundary(&self, offset: usize) -> usize {
        if let Some((_, range)) = self
            .chip_ranges()
            .into_iter()
            .find(|(_, range)| offset > range.start && offset <= range.end)
        {
            return range.start;
        }
        previous_grapheme_boundary(&self.content, offset)
    }

    pub(super) fn next_boundary(&self, offset: usize) -> usize {
        if let Some((_, range)) = self
            .chip_ranges()
            .into_iter()
            .find(|(_, range)| offset >= range.start && offset < range.end)
        {
            return range.end;
        }
        next_grapheme_boundary(&self.content, offset)
    }
}

impl PromptInput {
    /// Nothing typed at all, whitespace included.
    pub(crate) fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
}
