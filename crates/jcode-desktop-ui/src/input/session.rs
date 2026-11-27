//! The prompt as part of a session: snapshots, queued edits, model and mode
//! menus, placeholders.
use super::*;

impl PromptInput {
    pub(crate) fn composer_proof_state(&self) -> serde_json::Value {
        let bounds = self.editor_scroll.bounds();
        serde_json::json!({
            "command_menu": !self.command_suggestions().is_empty(),
            "attachments": self.attachments.len(),
            "editor_height": f32::from(bounds.size.height),
            "editor_y": f32::from(self.editor_scroll.offset().y),
        })
    }

    pub fn snapshot(&self) -> PromptInputSnapshot {
        // A transient picker must never replace the user's persisted draft.
        let (content, selection, attachments) = match &self.model_menu_draft {
            Some((content, attachments)) => (
                std::borrow::Cow::Borrowed(content.as_ref()),
                content.len()..content.len(),
                attachments,
            ),
            None => {
                let (content, selection) = self.draft_without_live_dictation();
                (content, selection, &self.attachments)
            }
        };
        PromptInputSnapshot {
            content: content.to_string(),
            selection_start: selection.start,
            selection_end: selection.end,
            selection_reversed: self.sel.reversed,
            history: self.history.clone(),
            history_index: self.history_index,
            live_draft: String::new(),
            history_draft: self.live_draft.as_ref().map(|draft| HistoryDraftSnapshot {
                draft: StashedPromptSnapshot {
                    content: draft.content.clone(),
                    selection_start: draft.selection.start,
                    selection_end: draft.selection.end,
                    attachments: draft.attachments.iter().map(Attachment::snapshot).collect(),
                },
                selection_reversed: draft.selection_reversed,
            }),
            attachments: attachments.iter().map(Attachment::snapshot).collect(),
            stashed_prompt: self.stashed_prompt.as_ref().map(
                |(content, selection, attachments)| StashedPromptSnapshot {
                    content: content.clone(),
                    selection_start: selection.start,
                    selection_end: selection.end,
                    attachments: attachments.iter().map(Attachment::snapshot).collect(),
                },
            ),
        }
    }

    /// The draft as another Desktop of the same machine may show it: its text,
    /// the caret, and the attachments that mean the same elsewhere (images and
    /// pasted text; a file reference names a path on this Desktop only).
    pub(crate) fn shared_draft(&self) -> (String, usize, Vec<AttachmentSnapshot>) {
        let (content, selection) = self.draft_without_live_dictation();
        let attachments = self
            .attachments
            .iter()
            .map(Attachment::snapshot)
            .filter(|attachment| {
                attachment.media_type.starts_with("image/")
                    || attachment.media_type == SNIPPET_MEDIA_TYPE
            })
            .collect();
        (content.to_string(), selection.end, attachments)
    }

    #[cfg(test)]
    pub(crate) fn prompt_history(&self) -> Vec<String> {
        self.history.clone()
    }

    /// Keep text the composer no longer shows one Up-arrow away.
    pub(crate) fn keep_in_history(&mut self, text: &str) {
        keep_in_history(&mut self.history, text);
    }

    /// Show a draft another Desktop wrote, keeping this prompt's history.
    pub(crate) fn apply_shared_draft(
        &mut self,
        text: String,
        cursor: usize,
        attachments: Vec<AttachmentSnapshot>,
        cx: &mut Context<Self>,
    ) {
        let mut snapshot = self.snapshot();
        let cursor = text.floor_char_boundary(cursor.min(text.len()));
        // Text this replaces stays one Up-arrow away.
        if snapshot.content != text {
            keep_in_history(&mut snapshot.history, &snapshot.content);
        }
        snapshot.content = text;
        snapshot.selection_start = cursor;
        snapshot.selection_end = cursor;
        snapshot.selection_reversed = false;
        snapshot.attachments = attachments;
        self.restore(snapshot, cx);
    }

    pub fn restore(&mut self, snapshot: PromptInputSnapshot, cx: &mut Context<Self>) {
        self.live_dictation = None;
        self.model_menu_draft = None;
        self.stashed_prompt = snapshot.stashed_prompt.map(|stash| {
            let mut content = stash.content;
            let mut attachments = restore_attachments(stash.attachments);
            ensure_visible_chips(&mut content, &mut attachments);
            let len = content.len();
            let start = stash.selection_start.min(len);
            let end = stash.selection_end.min(len);
            (content, start.min(end)..start.max(end), attachments)
        });
        let mut content = snapshot.content;
        self.attachments = restore_attachments(snapshot.attachments);
        ensure_visible_chips(&mut content, &mut self.attachments);
        let len = content.len();
        self.control_revision = self.control_revision.wrapping_add(1);
        self.content = content.into();
        self.sel.range = snapshot.selection_start.min(len)..snapshot.selection_end.min(len);
        if self.sel.range.end < self.sel.range.start {
            self.sel.range = self.sel.range.end..self.sel.range.start;
        }
        self.sel.reversed = snapshot.selection_reversed;
        self.history = snapshot.history;
        self.history_index = snapshot
            .history_index
            .filter(|index| *index < self.history.len());
        self.live_draft = snapshot
            .history_draft
            .map(|snapshot| {
                let mut content = snapshot.draft.content;
                let mut attachments = restore_attachments(snapshot.draft.attachments);
                ensure_visible_chips(&mut content, &mut attachments);
                let boundary = |offset| content.floor_char_boundary(offset);
                let start = boundary(snapshot.draft.selection_start);
                let end = boundary(snapshot.draft.selection_end);
                HistoryDraft {
                    content,
                    selection: start.min(end)..start.max(end),
                    selection_reversed: snapshot.selection_reversed,
                    attachments,
                }
            })
            .or_else(|| {
                self.history_index.map(|_| {
                    let len = snapshot.live_draft.len();
                    HistoryDraft {
                        content: snapshot.live_draft,
                        selection: len..len,
                        selection_reversed: false,
                        attachments: Vec::new(),
                    }
                })
            });
        if self.history_index.is_some() {
            self.close_command_menu(CommandMenuClose::Literal);
        }
        self.attachment_preview = None;
        self.attachment_notice = None;
        self.sel.marked = None;
        self.undo.clear();
        self.redo.clear();
        cx.notify();
    }

    /// An editor for one queued message, embedded in its queue row. It shows
    /// the message exactly as queued, with its images, files and pasted text
    /// as chips, and never submits by itself: the queue saves it in place.
    pub(crate) fn queued_editor(
        content: String,
        images: Vec<(String, String)>,
        attachments: Vec<jcode_sdk::api::Attachment>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut input =
            Self::new_with_queue_files(cx, "Edit queued message…", |_, _, _, _, _, _, _| {});
        input.queue_editor = true;
        input.command_completion = false;
        let mut text = content;
        let markers: Vec<String> = image_marker_ranges(&text)
            .into_iter()
            .map(|range| text[range].to_string())
            .collect();
        let mut chips: Vec<(String, QueuedAttachment, String)> = Vec::new();
        let mut appended: Vec<(String, QueuedAttachment)> = Vec::new();
        for (index, (mime, data)) in images.into_iter().enumerate() {
            let source = QueuedAttachment::Image(mime, data);
            match markers.get(index) {
                Some(marker) if !chips.iter().any(|(label, ..)| label == marker) => {
                    chips.push((marker.clone(), source, marker.clone()));
                }
                _ => appended.push(("[#image 1]".into(), source)),
            }
        }
        for attachment in attachments {
            let base = match &attachment {
                jcode_sdk::api::Attachment::File { name, size, .. } => {
                    format!("[#{}, {size} B]", name.replace(['[', ']', '\n', '\r'], " "))
                }
                jcode_sdk::api::Attachment::PastedText { text } => {
                    format!("[Pasted text, {} chars]", text.encode_utf16().count())
                }
                jcode_sdk::api::Attachment::Unknown => "[#attachment]".into(),
            };
            appended.push((base, QueuedAttachment::File(attachment)));
        }
        // Attachments the message text does not name get a chip at the end.
        // Saving removes that chip and the space before it again, so an
        // untouched message round-trips byte for byte.
        for (base, source) in appended {
            let label = (1..)
                .map(|n| match (n, base.strip_prefix("[#image ")) {
                    (_, Some(_)) => format!("[#image {n}]"),
                    (1, None) => base.clone(),
                    (n, None) => format!("{} #{n}]", &base[..base.len() - 1]),
                })
                .find(|label| {
                    !text.contains(label.as_str()) && !chips.iter().any(|(used, ..)| used == label)
                })
                .expect("an unused chip label");
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&label);
            chips.push((label, source, String::new()));
        }
        input.attachments = chips
            .into_iter()
            .map(|(label, source, replacement)| Attachment {
                label: label.into(),
                payload: AttachmentPayload::Queued {
                    preview: source.preview(),
                    source,
                    replacement,
                },
            })
            .collect();
        input.set_content(text, cx);
        input
    }

    /// What a queue editor saves: its text, then every image and attachment it
    /// still shows. Attachments the user did not touch go back exactly as they
    /// were queued, whether inline bytes or a path on the runtime's machine.
    pub(crate) fn queued_edit_payload(
        &self,
    ) -> (
        String,
        Vec<(String, String)>,
        Vec<jcode_sdk::api::Attachment>,
    ) {
        let mut located: Vec<_> = self
            .attachments
            .iter()
            .map(|attachment| (self.chip_range(attachment), attachment))
            .collect();
        located.sort_by_key(|(range, _)| range.as_ref().map_or(usize::MAX, |range| range.start));
        let mut images = Vec::new();
        let mut files = Vec::new();
        let mut edits = Vec::new();
        for (range, attachment) in located {
            let replacement = match &attachment.payload {
                AttachmentPayload::Queued {
                    source,
                    replacement,
                    ..
                } => {
                    match source {
                        QueuedAttachment::Image(mime, data) => {
                            images.push((mime.clone(), data.clone()))
                        }
                        QueuedAttachment::File(file) => files.push(file.clone()),
                    }
                    (replacement.as_str() != attachment.label.as_ref()).then(|| replacement.clone())
                }
                AttachmentPayload::Image {
                    media_type,
                    encoded,
                    ..
                } => {
                    images.push((media_type.clone(), encoded.to_string()));
                    None
                }
                AttachmentPayload::Text(text) => Some(text.to_string()),
                AttachmentPayload::Context(context) => {
                    Some(context.prompt_text().unwrap_or_default())
                }
                AttachmentPayload::File { path, file, .. } => {
                    files.push(jcode_sdk::api::Attachment::File {
                        name: file.name.clone(),
                        mime: file.mime.clone(),
                        size: file.size,
                        data: Some(file.data.clone()),
                        path: None,
                    });
                    Some(if path.as_os_str().is_empty() {
                        String::new()
                    } else {
                        file_drop::path_text(std::slice::from_ref(path))
                    })
                }
            };
            if let (Some(range), Some(replacement)) = (range, replacement) {
                edits.push((range, replacement));
            }
        }
        let mut content = self.content.to_string();
        for (range, replacement) in edits.into_iter().rev() {
            let start = if replacement.is_empty() && content[..range.start].ends_with(' ') {
                range.start - 1
            } else {
                range.start
            };
            content.replace_range(start..range.end, &replacement);
        }
        (content, images, files)
    }

    /// Placeholder shown at `now`, the example it belongs to, and whether it
    /// is still animating.
    pub(super) fn placeholder_text(
        &self,
        now: Instant,
        cx: &App,
    ) -> (SharedString, Option<SharedString>, bool) {
        if !self.example_prompts {
            return (self.placeholder.clone(), None, false);
        }
        if self.external_picker_open {
            return (SharedString::default(), None, false);
        }
        let reduced = motion_reduced(cx);
        let mut state = self.motion.borrow_mut();
        let source = cx
            .try_global::<crate::example_prompts::ExamplePrompts>()
            .map(|global| Arc::as_ptr(&global.0) as usize);
        let fresh = state.prompts.as_ref().is_some_and(|picked| {
            picked.seed == state.seed
                && picked.source == source
                && picked.working_dir == self.working_dir
        });
        if !fresh {
            // A shared project is not a session boundary: another session's
            // unfinished todos must never become this composer's examples.
            let prompts =
                motion::pick_prompts(&[] as &[SharedString], state.seed).map(SharedString::from);
            if state
                .prompts
                .as_ref()
                .is_some_and(|picked| picked.prompts != prompts)
            {
                // Sessions loaded after the composer appeared. Start the new
                // example from its first letter rather than mid-word.
                state.epoch = now;
            }
            state.prompts = Some(PickedPrompts {
                seed: state.seed,
                source,
                working_dir: self.working_dir.clone(),
                prompts,
            });
        }
        let picked = state
            .prompts
            .as_ref()
            .expect("example prompts were just picked");
        let elapsed = now.saturating_duration_since(state.epoch);
        let prompts = [picked.prompts[0].as_ref(), picked.prompts[1].as_ref()];
        let (shown, full, live) = motion::placeholder_at(elapsed, prompts, reduced);
        let full = picked
            .prompts
            .iter()
            .find(|prompt| prompt.as_ref() == full)
            .cloned();
        let shown = if full.as_ref().is_some_and(|full| full.len() == shown.len()) {
            full.clone().unwrap_or_default()
        } else {
            SharedString::from(shown.to_owned())
        };
        (shown, full, live)
    }

    pub(crate) fn has_user_draft(&self) -> bool {
        !self.content.trim().is_empty()
            || !self.attachments.is_empty()
            || self
                .stashed_prompt
                .as_ref()
                .is_some_and(|(text, _, attachments)| {
                    !text.trim().is_empty() || !attachments.is_empty()
                })
    }

    /// The icon the daemon resolved for `account` (`provider:label`), when a
    /// route names it.
    pub fn route_icon(&self, account: &str) -> Option<String> {
        let (_, routes) = self.applied_model_routes.as_ref()?;
        routes
            .iter()
            .find(|route| route.account.as_deref() == Some(account))
            .and_then(|route| route.icon.clone())
    }

    /// The icon of the OAuth login `label`, from the route that names that
    /// login. `provider` only breaks a tie when two providers use the label.
    pub fn login_icon(&self, provider: Option<&str>, label: &str) -> Option<String> {
        let (_, routes) = self.applied_model_routes.as_ref()?;
        let named: Vec<_> = routes
            .iter()
            .filter(|route| {
                route
                    .account
                    .as_deref()
                    .is_some_and(|account| account.rsplit(':').next() == Some(label))
            })
            .collect();
        let accounts: std::collections::BTreeSet<_> = named
            .iter()
            .filter_map(|route| route.account.as_deref())
            .collect();
        let route = if accounts.len() > 1 {
            named
                .iter()
                .find(|route| provider.is_some_and(|provider| route.provider == provider))?
        } else {
            named.first()?
        };
        route.icon.clone()
    }

    pub fn set_model_routes(
        &mut self,
        mut models: Vec<String>,
        routes: &[jcode_sdk::ModelRouteInfo],
        current_model: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self
            .applied_model_routes
            .as_ref()
            .is_some_and(|(applied_models, applied_routes)| {
                *applied_models == models && applied_routes.as_slice() == routes
            })
        {
            self.set_current_model(current_model, cx);
            return;
        }
        let applied = (models.clone(), routes.to_vec());
        let selected = self
            .command_suggestions()
            .get(self.command_selection)
            .cloned();
        self.model_details = model_menu::from_routes(routes);
        self.suggestions_revision += 1;
        self.current_model = current_model;
        if !routes.is_empty() {
            models = self.model_details.keys().cloned().collect();
        }
        model_menu::rank(&mut models, &self.model_details);
        self.set_command_models(models, cx);
        self.applied_model_routes = Some(applied);
        if let Some(model) = selected
            .as_ref()
            .and_then(|row| row.value.strip_prefix("/model "))
            && !self
                .command_suggestions()
                .iter()
                .any(|row| row.value == format!("/model {model}"))
            && self
                .command_models
                .iter()
                .any(|candidate| candidate == model)
        {
            self.expanded_model_groups
                .insert(model_menu::group_key(model, &self.model_details));
            self.suggestions_revision += 1;
        }
        // A background usage update must not move Enter onto a different model.
        if let Some(index) = selected.and_then(|selected| {
            self.command_suggestions().iter().position(|row| {
                if selected.toggle.is_some() {
                    row.toggle == selected.toggle
                } else {
                    row.toggle.is_none() && row.value == selected.value
                }
            })
        }) {
            self.command_selection = index;
            self.reveal_command(index);
        }
        cx.notify();
    }

    #[cfg(test)]
    pub fn model_picker_rows(&self) -> Vec<(String, bool)> {
        self.command_suggestions()
            .iter()
            .enumerate()
            .filter_map(|(index, suggestion)| {
                suggestion.value.strip_prefix("/model ").map(|model| {
                    (
                        self.model_details
                            .get(model)
                            .map_or_else(|| model.to_string(), |detail| detail.model.clone()),
                        index == self.command_selection,
                    )
                })
            })
            .collect()
    }

    pub(super) fn stash(&mut self, _: &Stash, _: &mut Window, cx: &mut Context<Self>) {
        let (draft, selection) = self.draft_without_live_dictation();
        let draft = draft.into_owned();
        let current = (!draft.is_empty() || !self.attachments.is_empty())
            .then(|| (draft, selection, std::mem::take(&mut self.attachments)));
        let previous = std::mem::replace(&mut self.stashed_prompt, current);
        let (text, selection, attachments) = previous.unwrap_or_default();
        self.set_content(text, cx);
        self.sel.range = selection;
        self.attachments = attachments;
        self.sel.marked = None;
        self.attachment_notice = None;
        self.attachment_preview = None;
        self.undo.clear();
        self.redo.clear();
    }

    pub(crate) fn control_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.live_dictation.is_some()
            || !self.submission_enabled
            || self.queue_editor
            || self.content.trim().is_empty()
        {
            return false;
        }
        self.dispatch_submission(
            SubmissionKind::Message,
            Self::literal_submission(&self.content),
            false,
            window,
            cx,
        );
        self.content.is_empty()
    }

    pub(super) fn submit_prompt(
        &mut self,
        queued: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.live_dictation.is_some() {
            self.set_transient_notice("Stop voice recording before sending".into(), cx);
            return;
        }
        if !self.submission_enabled || self.queue_editor {
            return;
        }
        let raw_content = self.command_content();
        let content = raw_content.trim().to_string();
        if self.attachments.is_empty() && content.split_whitespace().next() == Some("/speed") {
            if crate::speed_icon::command_tier(&content).is_some() {
                self.submit_command(content, queued, window, cx);
            } else if let Some(command) = self
                .command_suggestions()
                .get(self.command_selection)
                .map(|row| row.value.clone())
            {
                self.submit_command(command, queued, window, cx);
            } else {
                self.set_transient_notice(
                    "Use /speed standard, /speed fast, or /speed ultrafast".into(),
                    cx,
                );
            }
            return;
        }
        if self.standalone_btw
            && !queued
            && self.picker_prefix.is_none()
            && self.attachments.is_empty()
            && self.content.trim() == "btw"
        {
            self.dispatch_submission(SubmissionKind::Command, content, false, window, cx);
            return;
        }
        if content.is_empty() && self.attachments.is_empty() {
            if self.empty_submit && !queued {
                (self.on_submit)(
                    SubmissionKind::Message,
                    String::new(),
                    Vec::new(),
                    Vec::new(),
                    false,
                    window,
                    cx,
                );
            }
            return;
        }
        if self.command_completion && self.attachments.is_empty() {
            if self.current_command_menu_close() == Some(CommandMenuClose::Completed) {
                self.submit_command(content, queued, window, cx);
                return;
            }
            let suggestions = self.command_suggestions();
            let embedded = self.embedded_command_token();
            let picker_command = raw_content.starts_with('/')
                && !raw_content.contains(['\n', '\r'])
                && matches!(
                    content.split_whitespace().next(),
                    Some("/model" | "/mode" | "/effort")
                );
            if let Some(row) = suggestions.get(self.command_selection)
                && (self.picker_prefix.is_some()
                    || ((!queued || embedded.is_none())
                        && self.command_selection_explicit.as_ref() == Some(&row.value))
                    || row.value == content
                    || picker_command)
            {
                if let Some(key) = row.toggle.clone() {
                    self.toggle_model_group(key, cx);
                } else if let Some(step) = row.action.clone() {
                    self.run_stage_step(step, queued, window, cx);
                } else if let Some(command) =
                    accepted_command_submission(&suggestions, self.command_selection)
                {
                    self.submit_command(command, queued, window, cx);
                }
                return;
            }
            if picker_command
                && content.split_whitespace().next() == Some("/model")
                && self.current_command_menu_close().is_none()
            {
                self.set_transient_notice("No matching models".into(), cx);
                return;
            }
        }
        if self.picker_prefix.is_none() {
            self.dispatch_submission(
                SubmissionKind::Message,
                Self::literal_submission(&raw_content),
                queued,
                window,
                cx,
            );
        }
    }

    /// Files copied in the file manager. Only images are read, so the model
    /// can see them. Any other file, a folder or an oversized image is named
    /// by path: pasting a file points at it, it does not upload it.
    pub(super) fn attach_pasted_files(
        &mut self,
        paths: &[std::path::PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (paths, referenced): (Vec<_>, Vec<_>) = paths.iter().cloned().partition(|path| {
            file_drop::image_media_type(path).is_some()
                && std::fs::metadata(path)
                    .is_ok_and(|meta| meta.is_file() && meta.len() <= 10 * 1024 * 1024)
        });
        self.reference_paths(&referenced, paths.is_empty(), window, cx);
        let paths = &paths;
        let mut attached_size: u64 = self
            .attachments
            .iter()
            .filter_map(|attachment| match &attachment.payload {
                AttachmentPayload::File { file, .. } => Some(file.size),
                AttachmentPayload::Queued {
                    source: QueuedAttachment::File(jcode_sdk::api::Attachment::File { size, .. }),
                    ..
                } => Some(*size),
                _ => None,
            })
            .sum();
        for path in paths {
            let metadata = match std::fs::metadata(path) {
                Ok(metadata) if metadata.is_file() && metadata.len() <= 10 * 1024 * 1024 => {
                    metadata
                }
                Ok(_) => {
                    self.attachment_notice =
                        Some("File must be at most 10 MiB and not a directory".into());
                    continue;
                }
                Err(error) => {
                    self.attachment_notice = Some(format!("Cannot read file: {error}").into());
                    continue;
                }
            };
            if attached_size.saturating_add(metadata.len()) > 20 * 1024 * 1024 {
                self.attachment_notice = Some("File attachments exceed 20 MiB total".into());
                continue;
            }
            let bytes = match std::fs::File::open(path).and_then(|file| {
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(
                    &mut std::io::Read::take(file, 10 * 1024 * 1024 + 1),
                    &mut bytes,
                )?;
                Ok(bytes)
            }) {
                Ok(bytes) if bytes.len() as u64 == metadata.len() => bytes,
                Ok(_) => {
                    self.attachment_notice = Some("File changed while attaching".into());
                    continue;
                }
                Err(error) => {
                    self.attachment_notice = Some(format!("Cannot read file: {error}").into());
                    continue;
                }
            };
            attached_size += metadata.len();
            let size = Some(metadata.len());
            let file = SubmittedFile {
                name: path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .into_owned(),
                mime: mime_guess::from_path(path)
                    .first_or_octet_stream()
                    .to_string(),
                size: metadata.len(),
                data: base64::engine::general_purpose::STANDARD.encode(&bytes),
            };
            let name = path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .replace(['[', ']', '\n', '\r'], " ");
            let size_label =
                size.map_or_else(|| "unknown size".to_string(), |size| format!("{size} B"));
            let description = format!("{name}, {size_label}");
            let mut marker = format!("[#{description}]");
            let mut n = 2;
            while self.content.contains(&marker) {
                marker = format!("[#{description} #{n}]");
                n += 1;
            }
            let image = file_drop::image_media_type(path).and_then(|media_type| {
                if !size.is_some_and(|size| size <= 20 * 1024 * 1024) {
                    return None;
                }
                let image =
                    crate::clipboard_image::from_bytes(media_type.to_owned(), bytes.clone())
                        .ok()?;
                let preview = preview_image(&image.media_type, image.bytes.clone())?;
                Some(FileImage {
                    media_type: image.media_type,
                    encoded: base64::engine::general_purpose::STANDARD
                        .encode(image.bytes)
                        .into(),
                    preview,
                })
            });
            self.undo.push(self.edit_snapshot());
            self.redo.clear();
            let range =
                self.expanded_chip_range(clamp_range(&self.content, self.sel.range.clone()));
            self.remove_chips_in_range(&range);
            let mut content = self.content.to_string();
            let mut inserted = String::new();
            if content[..range.start]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_whitespace())
            {
                inserted.push(' ');
            }
            inserted.push_str(&marker);
            inserted.push(' ');
            content.replace_range(range.clone(), &inserted);
            self.rebase_live_dictation_after_edit(&range, inserted.len());
            self.content = content.into();
            self.control_revision = self.control_revision.wrapping_add(1);
            let cursor = range.start + inserted.len();
            self.sel.range = cursor..cursor;
            self.attachments.push(Attachment {
                label: marker.into(),
                payload: AttachmentPayload::File {
                    path: path.clone(),
                    image,
                    file,
                },
            });
        }
        self.attachment_preview = None;
        self.sel.marked = None;
        self.ghost_on_edit(cx);
        if let Some(on_change) = &self.on_change {
            on_change(&self.content, cx);
        }
        cx.notify();
    }

    pub(super) fn try_attach_pasted_text(
        &mut self,
        range: &Range<usize>,
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.paste_mode == PasteMode::InlineText || !is_large_paste(text) {
            return false;
        }
        let lines = text.split('\n').count();
        let description = if lines > 10 {
            format!("Pasted text, {lines} lines")
        } else {
            format!("Pasted text, {} chars", text.encode_utf16().count())
        };
        let mut marker = format!("[{description}]");
        let mut n = 2;
        while self.content.contains(&marker)
            || self.attachments.iter().any(|a| a.label.as_ref() == marker)
        {
            marker = format!("[{description} #{n}]");
            n += 1;
        }
        let attachment = Attachment {
            label: marker.as_str().into(),
            payload: AttachmentPayload::Text(text.into()),
        };
        self.attachment_preview = None;
        self.attachment_notice = None;
        self.replace_editor_text(range.clone(), &marker, Some(attachment), cx);
        true
    }

    pub(crate) fn start_picker_search(
        &mut self,
        prefix: &'static str,
        fallback_models: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if self.command_models.is_empty() {
            self.set_command_models(fallback_models, cx);
        }
        self.picker_prefix = Some(prefix);
        self.command_scroll = model_picker::picker_list();
        self.model_stage = Default::default();
        self.picker_effort = None;
        self.suggestions_revision += 1;
        self.set_content(String::new(), cx);
        if prefix == "/effort"
            && let Some(index) = self
                .command_suggestions()
                .iter()
                .position(|row| row.active_choice)
        {
            self.command_selection = index;
            self.reveal_command(index);
        }
    }

    pub(crate) fn focus_next_picker_row_for_screenshot(&mut self, cx: &mut Context<Self>) {
        let rows = self.command_suggestions();
        let Some(index) = rows.iter().position(|row| row.active_choice) else {
            return;
        };
        if let Some(next) = rows.get(index + 1) {
            self.command_selection = index + 1;
            self.command_selection_explicit = Some(next.value.clone());
            self.reveal_command(self.command_selection);
            cx.notify();
        }
    }

    #[cfg(test)]
    pub(crate) fn open_model_menu(&mut self, fallback_models: Vec<String>, cx: &mut Context<Self>) {
        // Offline/reconnected inputs may not have received route metadata yet.
        // Never replace the canonical grouped routes once they are available.
        if self.command_models.is_empty() {
            self.set_command_models(fallback_models, cx);
        }
        if self.model_menu_draft.is_none()
            && !(self.content.trim() == "/model"
                || self.content.trim_start().starts_with("/model "))
        {
            let draft = self.draft_without_live_dictation().0.into_owned();
            self.model_menu_draft = Some((draft.into(), std::mem::take(&mut self.attachments)));
            self.attachment_preview = None;
        }
        self.model_stage = Default::default();
        self.suggestions_revision += 1;
        self.set_content("/model ".into(), cx);
    }

    /// Restore the stashed draft after the effort menu closes or submits.
    pub(crate) fn close_effort_menu(&mut self, cx: &mut Context<Self>) {
        if let Some((draft, attachments)) = self.model_menu_draft.take() {
            self.attachments = attachments;
            self.set_content(draft.to_string(), cx);
        } else if self.effort_menu_open() {
            self.set_content(String::new(), cx);
        }
    }

    pub(crate) fn close_model_menu(&mut self, cx: &mut Context<Self>) {
        if let Some((draft, attachments)) = self.model_menu_draft.take() {
            self.attachments = attachments;
            self.set_content(draft.to_string(), cx);
        } else if self.content.trim() == "/model"
            || self.content.trim_start().starts_with("/model ")
        {
            self.set_content(String::new(), cx);
        }
    }

    pub(crate) fn append_queued_files(
        &mut self,
        files: Vec<SubmittedFile>,
        cx: &mut Context<Self>,
    ) {
        let mut content = self.content.to_string();
        for file in files {
            let name = file.name.replace(['[', ']', '\n', '\r'], " ");
            let mut marker = format!("[#{name}, {} B]", file.size);
            let mut index = 2;
            while content.contains(&marker) {
                marker = format!("[#{name}, {} B #{index}]", file.size);
                index += 1;
            }
            let image = base64::engine::general_purpose::STANDARD
                .decode(&file.data)
                .ok()
                .and_then(|bytes| preview_image(&file.mime, bytes))
                .map(|preview| FileImage {
                    media_type: file.mime.clone(),
                    encoded: file.data.clone().into(),
                    preview,
                });
            if !content.is_empty() && !content.ends_with(char::is_whitespace) {
                content.push(' ');
            }
            content.push_str(&marker);
            // The recalled prompt already contains the original path.
            self.attachments.push(Attachment {
                label: marker.into(),
                payload: AttachmentPayload::File {
                    path: std::path::PathBuf::new(),
                    image,
                    file,
                },
            });
        }
        self.set_content(content, cx);
    }

    /// Put a recalled queued prompt back into the composer for editing. Any
    /// existing draft is kept ahead of it so nothing typed is lost.
    pub(crate) fn recall_prompt(
        &mut self,
        content: &str,
        images: Vec<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        let draft = self.content.trim_end().to_string();
        let ranges = image_marker_ranges(content);
        let mut recalled = content.to_string();
        let mut replacements = Vec::new();
        let mut used: HashSet<String> = image_marker_ranges(&draft)
            .into_iter()
            .map(|range| draft[range].to_string())
            .collect();
        for (index, (media_type, encoded)) in images.into_iter().enumerate() {
            let Some(bytes) = base64::engine::general_purpose::STANDARD
                .decode(&encoded)
                .ok()
            else {
                continue;
            };
            let Some(preview) = preview_image(&media_type, bytes) else {
                continue;
            };
            let original = ranges
                .get(index)
                .map(|range| content[range.clone()].to_string());
            let marker = original
                .clone()
                .filter(|marker| !used.contains(marker))
                .unwrap_or_else(|| {
                    (1..)
                        .map(|n| format!("[#image {n}]"))
                        .find(|marker| !used.contains(marker) && !content.contains(marker))
                        .unwrap()
                });
            used.insert(marker.clone());
            if let Some(range) = ranges.get(index) {
                if original.as_deref() != Some(marker.as_str()) {
                    replacements.push((range.clone(), marker.clone()));
                }
            } else {
                replacements.push((content.len()..content.len(), format!(" {marker}")));
            }
            self.attachments.push(Attachment {
                label: marker.into(),
                payload: AttachmentPayload::Image {
                    media_type,
                    encoded: encoded.into(),
                    preview,
                },
            });
        }
        for (range, marker) in replacements.into_iter().rev() {
            recalled.replace_range(range, &marker);
        }
        let merged = if draft.is_empty() {
            recalled
        } else {
            format!("{draft}\n\n{recalled}")
        };
        self.set_content(merged, cx);
    }

    #[cfg(test)]
    pub(crate) fn command_suggestion_values(&self) -> Vec<String> {
        self.command_suggestions()
            .iter()
            .map(|row| row.value.clone())
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn selected_command_value(&self) -> Option<String> {
        self.command_suggestions()
            .get(self.command_selection)
            .map(|row| row.value.clone())
    }

    pub(crate) fn mode_rows_for_control(&self) -> Vec<serde_json::Value> {
        self.command_suggestions()
            .iter()
            .filter_map(|row| {
                let name = row.value.strip_prefix("/mode ")?;
                Some(serde_json::json!({
                    "name": name, "section": row.header,
                    "active": self.current_modes.iter().any(|active| active == name),
                    "icon": crate::mode_catalog::icon(name, self.mode_catalog.get(name)).name(),
                }))
            })
            .collect()
    }

    pub(super) fn selected_mode_name(&self) -> Option<String> {
        self.command_suggestions()
            .get(self.command_selection)?
            .value
            .strip_prefix("/mode ")
            .map(str::to_owned)
    }
}

fn keep_in_history(history: &mut Vec<String>, text: &str) {
    if !text.trim().is_empty() && history.last().map(String::as_str) != Some(text) {
        history.push(text.to_owned());
    }
}
