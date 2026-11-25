//! What the container may ask of a panel.
//!
//! The workspace shows, saves, retargets and annotates panels, but it never
//! reaches into one: every question and change goes through here, so the
//! panel's own state stays something only the panel edits. Changes that the
//! panel must redraw for notify, so a caller cannot forget to.
use super::*;

impl Panel {
    pub(crate) fn is_preview(&self) -> bool {
        self.preview_state.is_some()
    }

    /// A conversation the runtime has stored, as opposed to a draft or a
    /// built-in panel.
    pub(crate) fn is_persisted_session(&self) -> bool {
        self.session_id.starts_with("session_")
    }

    pub(crate) fn is_remote_draft(&self) -> bool {
        self.session_id.starts_with("startup://draft/remote/")
    }

    pub(crate) fn is_help_draft(&self) -> bool {
        self.session_id.starts_with("startup://draft/help/")
    }

    /// The session a review or accounts panel was opened from.
    pub(crate) fn review_source_id(&self) -> Option<&str> {
        self.session_id
            .strip_prefix("review://")
            .or_else(|| self.session_id.strip_prefix("accounts://"))
    }

    pub(crate) fn history_len(&self) -> usize {
        self.items.len()
    }

    pub(crate) fn has_history(&self) -> bool {
        !self.items.is_empty()
    }

    pub(crate) fn title_text(&self) -> String {
        self.title.to_string()
    }

    /// Whether the title already reads `title`.
    pub(crate) fn is_titled(&self, title: &str) -> bool {
        self.title.as_ref() == title
    }

    /// Whether the panel already points at `directory`.
    pub(crate) fn is_in(&self, directory: &str) -> bool {
        self.working_dir.as_deref() == Some(directory)
    }

    /// The latest user or assistant text, cut to one overview line.
    pub(crate) fn overview_preview(&self) -> &str {
        let text = self
            .items
            .iter()
            .rev()
            .find_map(|item| match item {
                Item::Assistant(text) | Item::User(text) => Some(text.as_str()),
                _ => None,
            })
            .unwrap_or_default();
        let end = text
            .char_indices()
            .nth(220)
            .map_or(text.len(), |(offset, _)| offset);
        &text[..end]
    }

    /// How much is typed in the composer. Length only: callers that dump
    /// state must never see the draft.
    pub(crate) fn draft_chars(&self, cx: &App) -> usize {
        self.input.read(cx).snapshot().content.chars().count()
    }

    pub(crate) fn composer_voice_bounds(&self, cx: &App) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.input.read(cx).voice_bounds()
    }

    pub(crate) fn composer_wheel_position(&self, cx: &App) -> gpui::Point<gpui::Pixels> {
        self.input.read(cx).composer_wheel_position()
    }

    pub(crate) fn composer_focused(&self, window: &Window, cx: &App) -> bool {
        self.input.read(cx).focus_handle.is_focused(window)
    }

    /// The composer's draft, and the revision of its text that automation
    /// compares to tell whether the agent wrote it.
    pub(crate) fn draft_fingerprint(&self, cx: &App) -> (PromptInputSnapshot, u64) {
        let input = self.input.read(cx);
        (input.snapshot(), input.control_revision())
    }

    /// Whether the session can be renamed: it is a conversation of this
    /// computer or a remote one, not a built-in panel.
    pub(crate) fn can_rename(&self) -> bool {
        self.can_fork()
            && (!self.session_id.contains("://")
                || crate::harness::is_remote_session(&self.session_id))
    }

    /// The session's title with its folder, as the rename dialog starts it.
    pub(crate) fn folder_title(&self) -> String {
        folder_session_title(&self.session_id, self.title.as_ref()).to_string()
    }

    /// Whether creating this draft's session already failed.
    pub(crate) fn creation_failed(&self) -> bool {
        self.status.starts_with("Session creation failed")
    }

    /// A progress message or failure for a draft that is connecting. A
    /// buffered success must not erase an earlier failure, so a retry, which
    /// gets a fresh request id, is what clears one.
    pub(crate) fn note_connection_phase(
        &mut self,
        message: &str,
        failed: bool,
        cx: &mut Context<Self>,
    ) {
        if !failed && self.status.starts_with("Session creation failed:") {
            return;
        }
        self.status = if failed {
            format!("Session creation failed: {message}")
        } else {
            message.to_owned()
        };
        cx.notify();
    }

    /// The draft now waits on the session created under `request_id`.
    pub(crate) fn adopt_request(&mut self, request_id: String, cx: &mut Context<Self>) {
        self.session_id = request_id;
        cx.notify();
    }

    pub(crate) fn set_title(&mut self, title: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.title = title.into();
        cx.notify();
    }

    pub(crate) fn set_status(&mut self, status: impl Into<String>, cx: &mut Context<Self>) {
        self.status = status.into();
        cx.notify();
    }

    pub(crate) fn set_working_dir(&mut self, directory: Option<String>, cx: &mut Context<Self>) {
        self.working_dir = directory;
        cx.notify();
    }

    /// Adds an error row to the transcript.
    pub(crate) fn report_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.items.push(Item::Error(message));
        cx.notify();
    }

    pub(crate) fn set_search_terms(&mut self, terms: &[String], cx: &mut Context<Self>) {
        if self.search_terms != terms {
            self.search_terms = terms.to_vec();
            cx.notify();
        }
    }

    /// The composer is another view's editor, shared with the panel.
    pub(crate) fn replace_input(&mut self, input: Entity<PromptInput>, cx: &mut Context<Self>) {
        self.input = input;
        cx.notify();
    }

    /// Reads as a transcript only: no composer, no runtime.
    pub(crate) fn show_transcript_only(&mut self, cx: &mut Context<Self>) {
        self.transcript_only = true;
        cx.notify();
    }

    /// Shows the account the onboarding demo claims to be signed in as.
    pub(crate) fn show_identity(
        &mut self,
        provider: Option<String>,
        model: Option<String>,
        method: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.provider == provider && self.model == model && self.auth_method == method {
            return;
        }
        self.provider = provider;
        self.auth_method = method;
        if self.model != model {
            self.model = model.clone();
            self.input
                .update(cx, |input, cx| input.set_current_model(model, cx));
        }
        cx.notify();
    }

    /// Starts another attempt to create this draft's session under a fresh
    /// request id, so a late reply to the earlier attempt is rejected.
    pub(crate) fn restart_session_request(
        &mut self,
        request_id: String,
        status: String,
        cx: &mut Context<Self>,
    ) {
        self.session_id = request_id;
        self.status = status;
        cx.notify();
    }

    /// Points a failed remote draft at this computer, keeping its editor,
    /// attachments and queued prompts.
    pub(crate) fn reconnect_locally(
        &mut self,
        request_id: String,
        directory: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.session_id = request_id;
        self.working_dir = directory;
        self.title = "New local session".into();
        self.status = "Connecting to this computer…".into();
        cx.notify();
    }
}
