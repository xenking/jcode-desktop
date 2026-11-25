//! Prompt input with full IME support, adapted from gpui's input example.
//! Long prompts soft-wrap in a bounded, scrollable editor. Enter submits via a callback.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, GlobalElementId, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, SharedString, Style,
    StyledImage, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div, img, point,
    prelude::*, px, relative, size,
};
use jcode_desktop_model::text_field::{
    FieldSelection, byte_to_utf16, clamp_range, clicked_word_range, next_grapheme_boundary,
    next_word_boundary, previous_grapheme_boundary, previous_word_boundary, range_from_utf16,
    range_to_utf16,
};
use jcode_desktop_ui_core::ime_geometry::ImeGeometry;
use jcode_provider_core::SpeedMode;
use serde::{Deserialize, Serialize};
use unicode_segmentation::GraphemeCursor;

pub(crate) mod popup;

mod layout;
mod live_dictation;

mod motion;
use layout::{PromptLayout, selection_quads};

#[path = "input/context/file_drop.rs"]
pub(crate) mod file_drop;
#[path = "input/context/picker.rs"]
mod context_picker;
#[path = "input/context/image_paths.rs"]
mod image_paths;
#[path = "input/model/menu.rs"]
pub(crate) mod model_menu;
#[path = "input/model/picker.rs"]
mod model_picker;
#[path = "input/model/search.rs"]
mod model_search;
#[path = "input/model/stage.rs"]
mod model_stage;
pub(crate) use model_stage::parse_model_pick;
#[path = "input/context/paste_preview.rs"]
mod paste_preview;
mod resize;
pub(crate) use resize::VerticalResize;

mod editing;
mod keys;
mod primitives;
mod session;
mod submit;
mod suggestions;
mod types;

pub(crate) use keys::*;
pub(crate) use suggestions::*;
pub(crate) use types::*;

use crate::commands::registered_command_entries;
use crate::image_chip::image_marker_ranges;
use crate::theme::{Theme, to_hsla};

const PENDING_COMMAND_NOTICE: &str =
    "Commands are available once connected. Your command is still in the editor.";



pub struct PromptInput {
    session_image_base: usize,
    pub(crate) focus_handle: FocusHandle,
    #[cfg(target_os = "macos")]
    ime_log_focus_subscriptions: Option<Vec<gpui::Subscription>>,
    pub(crate) content: SharedString,
    placeholder: SharedString,
    pub(crate) external_picker_open: bool,
    /// Byte-offset selection and IME composition in `content`.
    sel: FieldSelection,
    last_layout: Option<PromptLayout>,
    ime_geometry: Option<ImeGeometry>,
    last_layout_revision: u64,
    vertical_caret: Option<VerticalCaret>,
    last_bounds: Option<Bounds<Pixels>>,
    chip_positions: Vec<(usize, Bounds<Pixels>)>,
    /// Row height the layout was shaped and painted with. Event handlers run
    /// outside the element's text style, where `Window::line_height` differs.
    last_line_height: Option<Pixels>,
    voice_preview: Option<String>,
    live_dictation: Option<Range<usize>>,
    /// Says what stopping the recording will do to the draft. Comes from
    /// `[dictation] mode`, so the dim preview never promises a send the mode
    /// will not perform.
    voice_preview_hint: Option<SharedString>,
    voice_preview_scroll: gpui::ScrollHandle,
    composer_bounds: paste_preview::MeasuredBounds,
    visual_line_count: usize,
    is_selecting: bool,
    undo: Vec<EditSnapshot>,
    redo: Vec<EditSnapshot>,
    history: Vec<String>,
    history_index: Option<usize>,
    live_draft: Option<HistoryDraft>,
    attachments: Vec<Attachment>,
    stashed_prompt: Option<(String, Range<usize>, Vec<Attachment>)>,
    model_menu_draft: Option<(SharedString, Vec<Attachment>)>,
    attachment_notice: Option<SharedString>,
    /// Short-lived feedback such as `Effort: High`, cleared by a timer.
    transient_notice: Option<SharedString>,
    transient_notice_generation: u64,
    /// The newest paste briefly appears at reading size, then flies into its
    /// thumbnail. The index keeps simultaneous attachments independent.
    attachment_preview: Option<paste_preview::Preview>,
    preview_close_focus: FocusHandle,
    preview_remove_focus: FocusHandle,
    preview_panel_bounds: paste_preview::MeasuredBounds,
    file_attachments_enabled: bool,
    paste_mode: PasteMode,
    queue_editor: bool,
    #[allow(clippy::type_complexity)]
    on_submit: Box<
        dyn Fn(
            SubmissionKind,
            String,
            Vec<(String, String)>,
            Vec<SubmittedFile>,
            bool,
            &mut Window,
            &mut App,
        ),
    >,
    #[allow(clippy::type_complexity)]
    on_change: Option<Box<dyn Fn(&str, &mut App)>>,
    #[allow(clippy::type_complexity)]
    on_overlay_cancel: Option<Box<dyn Fn(&mut App) -> bool>>,
    on_picker_outside: Option<popup::Dismiss>,
    command_models: Vec<String>,
    command_skills: Vec<(String, String)>,
    context_picker: Option<context_picker::Picker>,
    /// A standalone picker owns plain query text rather than a slash command.
    picker_prefix: Option<&'static str>,
    command_completion: bool,
    standalone_btw: bool,
    command_menu_closed: Option<(SharedString, CommandMenuClose)>,
    command_selection_explicit: Option<String>,
    command_selection: usize,
    model_details: HashMap<String, model_menu::ModelDetails>,
    /// Inputs of the last applied `set_model_routes`. RuntimeInfo is
    /// rebroadcast on every usage tick with an unchanged catalog, and
    /// rebuilding plus re-ranking hundreds of routes each time showed up as a
    /// steady share of main-thread time in live profiles.
    applied_model_routes: Option<(Vec<String>, Vec<jcode_sdk::ModelRouteInfo>)>,
    expanded_model_groups: HashSet<String>,
    /// Which step of the two-step model picker is showing.
    model_stage: model_stage::ModelStage,
    current_model: Option<String>,
    /// Levels the serving model accepts, for the `/effort` menu.
    effort_ladder: Vec<&'static str>,
    current_effort: Option<String>,
    speed_picker: Option<crate::speed_icon::SpeedPicker>,
    on_model_speed: Option<OnModelSpeed>,
    current_modes: Vec<String>,
    mode_catalog: crate::mode_catalog::ModeCatalog,
    /// Level picked on a model row with the effort keys, sent with the
    /// switch. Cleared when the model menu closes.
    picker_effort: Option<String>,
    command_scroll: gpui::ListState,
    suggestions_revision: u64,
    control_revision: u64,
    suggestions_cache: RefCell<Option<model_picker::SuggestionsCache>>,
    command_layout_key: Option<(SharedString, usize, gpui::Size<Pixels>)>,
    command_entrance: jcode_desktop_motion::MenuEntrance,
    editor_scroll: gpui::ScrollHandle,
    editor_wheel_gesture: std::cell::Cell<bool>,
    editor_resize: VerticalResize,
    revealed_caret: Option<(usize, SharedString, gpui::Size<Pixels>, bool)>,
    submission_enabled: bool,
    host_handles_submission: bool,
    empty_submit: bool,
    pending_session: bool,
    spacious: bool,
    compact_padding: bool,
    /// Right padding reserved for a control the host floats inside the box
    /// (the chat composer's voice button).
    trailing_inset: f32,
    /// False when a host container draws the box (onboarding email tab).
    chrome: bool,
    /// Cycle example prompts behind an empty chat composer.
    example_prompts: bool,
    /// Project this composer works in. Scopes personalized example prompts.
    working_dir: Option<String>,
    /// Shared with paint and the ticker, outside the entity, so that caret
    /// and placeholder frames do not count as changes to this input: GPUI
    /// retains views between frames and builds again a view whose entities
    /// were updated.
    motion: std::rc::Rc<std::cell::RefCell<MotionState>>,
    /// Inline autocomplete. `None` for composers that never suggest.
    ghost: Option<GhostState>,
}




pub(crate) struct PickerSettings {
    models: Vec<String>,
    details: HashMap<String, model_menu::ModelDetails>,
    current_model: Option<String>,
    effort_ladder: Vec<&'static str>,
    current_effort: Option<String>,
    skills: Vec<(String, String)>,
    current_modes: Vec<String>,
    mode_catalog: crate::mode_catalog::ModeCatalog,
}

pub(crate) struct ModelSpeedChoice<'a> {
    pub spec: &'a str,
    pub effort: Option<&'static str>,
    pub current: bool,
    pub mode: SpeedMode,
}

type OnModelSpeed = Box<dyn Fn(ModelSpeedChoice<'_>, &mut App)>;

impl PromptInput {








    fn remove_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.attachments.len() {
            return;
        }
        self.undo.push(self.edit_snapshot());
        self.redo.clear();
        let attachment = self.attachments.remove(index);
        if let Some(range) = self.chip_range(&attachment) {
            let mut content = self.content.to_string();
            content.replace_range(range.clone(), "");
            self.rebase_live_dictation_after_edit(&range, 0);
            self.control_revision = self.control_revision.wrapping_add(1);
            self.content = content.into();
            self.sel.range = range.start..range.start;
        }
        self.attachment_preview = None;
        self.attachment_notice = None;
        self.ghost_on_edit(cx);
        if let Some(on_change) = &self.on_change {
            on_change(&self.content, cx);
        }
        cx.notify();
    }

    fn edit_snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            content: self.draft_without_live_dictation().0.into_owned(),
            attachments: self.attachments.clone(),
        }
    }






    fn expanded_chip_range(&self, mut edit: Range<usize>) -> Range<usize> {
        for (_, chip) in self.chip_ranges() {
            if (edit.start < chip.end && edit.end > chip.start)
                || (edit.is_empty() && edit.start > chip.start && edit.start < chip.end)
            {
                edit.start = edit.start.min(chip.start);
                edit.end = edit.end.max(chip.end);
            }
        }
        edit
    }



































































    fn submit_command(
        &mut self,
        command: String,
        queued: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.submission_enabled || self.queue_editor {
            return;
        }
        if let Some(crate::workspace_command_parse::WorkspaceCommand::Invalid(usage)) =
            crate::workspace_command_parse::parse(&command)
        {
            self.set_transient_notice(usage.into(), cx);
            return;
        }
        if self.pending_session
            && !(crate::commands::is_resume_command(&command)
                || crate::commands::is_accounts_command(&command)
                || crate::commands::reset_command(&command).is_some()
                || command == "/btw"
                || crate::commands::is_draft_config_command(&command))
        {
            self.attachment_notice = Some(PENDING_COMMAND_NOTICE.into());
            cx.notify();
            return;
        }
        self.dispatch_submission(SubmissionKind::Command, command, queued, window, cx);
    }


    /// Clear the composer and hand `content` to the host.
    fn dispatch_submission(
        &mut self,
        mut kind: SubmissionKind,
        content: String,
        queued: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let content = if content.is_empty() {
            "[image]".to_string()
        } else {
            content
        };
        let remaining_draft = (kind == SubmissionKind::Command)
            .then(|| self.embedded_command_token())
            .flatten()
            .map(|(token, _)| {
                let mut draft = self.content.to_string();
                let caret = token.start;
                draft.replace_range(token, "");
                (draft, caret)
            });
        let mut attachments = if remaining_draft.is_none() {
            std::mem::take(&mut self.attachments)
        } else {
            Vec::new()
        };
        attachments.sort_by_key(|a| content.find(a.label.as_ref()).unwrap_or(usize::MAX));
        let mut images = Vec::new();
        let mut files = Vec::new();
        let mut replacements = Vec::new();
        let mut history_content = content.clone();
        for attachment in attachments {
            let range = content
                .find(attachment.label.as_ref())
                .map(|start| start..start + attachment.label.len());
            match attachment.payload {
                AttachmentPayload::Image {
                    media_type,
                    encoded,
                    ..
                } => {
                    images.push((media_type, encoded.to_string()));
                    if let Some(range) = range {
                        // Draft markers are editor keys. The wire marker indexes
                        // the reordered payload, not the draft's insertion order.
                        let output = format!("[#image {}]", images.len());
                        replacements.push((range, Some(output), String::new()));
                    }
                }
                // Only queue editors hold these, and they never submit.
                AttachmentPayload::Queued { .. } => {}
                AttachmentPayload::Text(text) => {
                    if let Some(range) = range {
                        replacements.push((range, Some(text.to_string()), text.to_string()));
                    }
                }
                AttachmentPayload::Context(context) => {
                    if let Some(range) = range {
                        let history = match &context {
                            DraftContext::File { path, .. } => file_drop::path_text(std::slice::from_ref(path)),
                            DraftContext::Skill(name) => {
                                kind = SubmissionKind::Skill(name.clone());
                                String::new()
                            }
                        };
                        replacements.push((range, Some(context.prompt_text().unwrap_or_default()), history));
                    }
                }
                AttachmentPayload::File { path, image, file } => {
                    let (reference, history) = if let Some(image) = image {
                        images.push((image.media_type, image.encoded.to_string()));
                        (format!("[#image {}]", images.len()), String::new())
                    } else {
                        if self.file_attachments_enabled {
                            files.push(file);
                        }
                        let path = file_drop::path_text(&[path]);
                        (path.clone(), path)
                    };
                    if let Some(range) = range {
                        replacements.push((range, Some(reference), history));
                    }
                }
            }
        }
        let mut content = content;
        for (range, output, history) in replacements.into_iter().rev() {
            if let Some(output) = output {
                content.replace_range(range.clone(), &output);
            }
            history_content.replace_range(range, &history);
        }
        self.attachment_notice = None;
        self.transient_notice = None;
        self.attachment_preview = None;
        self.control_revision = self.control_revision.wrapping_add(1);
        self.content = "".into();
        self.command_menu_closed = None;
        self.command_selection_explicit = None;
        self.expanded_model_groups.clear();
        self.model_stage = Default::default();
        self.picker_effort = None;
        self.suggestions_revision += 1;
        // The cleared editor is one line immediately. Waiting for its next
        // paint leaves a tall, empty composer after a wrapped prompt submits.
        self.visual_line_count = 1;
        self.sel.range = 0..0;
        self.sel.marked = None;
        if !history_content.trim().is_empty() {
            self.history.push(history_content);
        }
        if remaining_draft.is_none() {
            self.history_index = None;
        }
        self.command_selection = 0;
        (self.on_submit)(kind, content, images, files, queued, window, cx);
        if let Some((text, caret)) = remaining_draft {
            self.set_content(text, cx);
            self.sel.range = caret..caret;
        } else if let Some(draft) = self.live_draft.take() {
            self.restore_history_draft(draft, cx);
        } else if let Some((text, selection, attachments)) = self.stashed_prompt.take() {
            self.set_content(text, cx);
            self.sel.range = selection;
            self.attachments = attachments;
        }
        cx.notify();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        // On a model row, ←/→ pick the level the switch will use.
        if self.step_picker_effort(-1, cx) {
            return;
        }
        if self.sel.range.is_empty() {
            self.move_to(self.previous_boundary(self.sel.cursor()), cx);
        } else {
            self.move_to(self.sel.range.start, cx)
        }
    }









    pub(crate) fn attach_image(
        &mut self,
        image: crate::clipboard_image::ClipboardImage,
        cx: &mut Context<Self>,
    ) {
        let Some(preview) = preview_image(&image.media_type, image.bytes.clone()) else {
            self.attachment_notice = Some("could not preview pasted image".into());
            cx.notify();
            return;
        };
        let mut number = 1;
        let marker = loop {
            let marker = format!("[#image {number}]");
            if !self.content.contains(&marker)
                && !self.attachments.iter().any(|a| a.label.as_ref() == marker)
            {
                break marker;
            }
            number += 1;
        };
        self.undo.push(self.edit_snapshot());
        self.redo.clear();
        let range = self.expanded_chip_range(clamp_range(&self.content, self.sel.range.clone()));
        self.remove_chips_in_range(&range);
        let mut content = self.content.to_string();
        content.replace_range(range.clone(), &marker);
        self.rebase_live_dictation_after_edit(&range, marker.len());
        self.control_revision = self.control_revision.wrapping_add(1);
        self.content = content.into();
        let cursor = range.start + marker.len();
        self.sel.range = cursor..cursor;
        self.attachments.push(Attachment {
            label: marker.into(),
            payload: AttachmentPayload::Image {
                media_type: image.media_type,
                encoded: base64::engine::general_purpose::STANDARD
                    .encode(image.bytes)
                    .into(),
                preview,
            },
        });
        self.attachment_preview = None;
        self.attachment_notice = None;
        self.sel.marked = None;
        self.ghost_on_edit(cx);
        if let Some(on_change) = &self.on_change {
            on_change(&self.content, cx);
        }
        cx.notify();
    }
























    fn restore_history_draft(&mut self, draft: HistoryDraft, cx: &mut Context<Self>) {
        self.attachments = draft.attachments;
        self.set_content(draft.content, cx);
        self.sel.range = draft.selection;
        self.sel.reversed = draft.selection_reversed;
    }

    fn clear(&mut self, _: &Clear, window: &mut Window, cx: &mut Context<Self>) {
        if self.attachment_preview.is_some() {
            self.close_attachment_preview(window, cx);
            return;
        }
        if self.ghost_text().is_some() {
            self.dismiss_ghost();
            cx.notify();
            return;
        }
        // Inside the picker, Esc steps back before it closes.
        let in_model_menu = self.picker_prefix == Some("/model")
            || self.content.trim() == "/model"
            || self.content.starts_with("/model ");
        if in_model_menu && self.model_stage_back(cx) {
            return;
        }
        if self.picker_prefix.is_none()
            && self.current_command_menu_close() != Some(CommandMenuClose::Literal)
            && (in_model_menu
                || self.mention_token().is_some()
                || self.current_command_menu_close() == Some(CommandMenuClose::Completed)
                || !self.command_suggestions().is_empty())
            && !self.standalone_context()
        {
            self.close_command_menu(CommandMenuClose::Literal);
            cx.notify();
            return;
        }
        if let Some(cancel) = &self.on_overlay_cancel
            && cancel(cx)
        {
            return;
        }
        if !self.content.is_empty() {
            self.undo.push(self.edit_snapshot());
            self.redo.clear();
            self.attachments.clear();
            self.set_content(String::new(), cx);
        }
    }






















    pub(crate) fn set_content(&mut self, content: String, cx: &mut Context<Self>) {
        self.live_dictation = None;
        self.dismiss_ghost();
        self.command_menu_closed = None;
        self.command_selection_explicit = None;
        self.control_revision = self.control_revision.wrapping_add(1);
        self.content = content.into();
        self.reset_model_groups_after_exit();
        self.command_selection = 0;
        self.reveal_command(0);
        self.sel.range = self.content.len()..self.content.len();
        self.sel.reversed = false;
        cx.notify();
    }










    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The editor owns this gesture, including repeated clicks. Do not let
        // it reach a caption-background listener behind a header-area composer.
        // Focus explicitly because stopping propagation also skips track_focus.
        window.focus(&self.focus_handle, cx);
        cx.stop_propagation();
        self.is_selecting = true;
        let offset = self.index_for_mouse_position(event.position, window);
        let clicked_chip = self.last_bounds.as_ref().and_then(|text_bounds| {
            let relative = event.position - text_bounds.origin;
            self.chip_positions
                .iter()
                .find_map(|(index, bounds)| bounds.contains(&relative).then_some(*index))
        });
        if !event.modifiers.shift
            && let Some(index) = clicked_chip.or_else(|| {
                self.chip_ranges()
                    .into_iter()
                    .find_map(|(index, range)| range.contains(&offset).then_some(index))
            })
        {
            self.is_selecting = false;
            self.activate_attachment(index, window, cx);
            return;
        }
        if event.modifiers.shift {
            self.select_to(offset, cx);
        } else if event.click_count == 2 {
            self.sel.range = clicked_word_range(&self.content, offset);
            self.sel.reversed = false;
            cx.notify();
        } else {
            self.move_to(offset, cx)
        }
    }

    fn activate_attachment(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(attachment) = self.attachments.get(index) else {
            return;
        };
        match &attachment.payload {
            AttachmentPayload::Text(text) => {
                if let Some(range) = self.chip_range(attachment) {
                    let text = text.clone();
                    window.focus(&self.focus_handle, cx);
                    self.sel.range = range;
                    self.sel.marked = None;
                    self.replace_editor_text(self.replacement_range(None), &text, None, cx);
                }
            }
            AttachmentPayload::Image { .. } | AttachmentPayload::File { image: Some(_), .. } => {
                self.attachment_preview = Some(paste_preview::Preview::new(index));
                window.focus(&self.preview_close_focus, cx);
            }
            AttachmentPayload::Context(DraftContext::File { path, .. }) => {
                let action = crate::ui_actions::OpenDocumentFile { path: path.clone(), fragment: None };
                window.dispatch_action(Box::new(action), cx);
            }
            _ => return,
        }
        cx.notify();
    }


    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position, window), cx);
        }
    }






    fn index_for_mouse_position(&self, position: Point<Pixels>, window: &Window) -> usize {
        let line_height = self.painted_line_height(window);
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_position(position - bounds.origin, line_height)
            .unwrap_or_else(|index| index)
            // The layout includes ghost text past the draft.
            .min(self.content.len())
    }



}

impl PromptInput {
    fn replacement_range(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        self.expanded_chip_range(
            self.sel
                .replacement_range(&self.content, range_utf16.as_ref()),
        )
    }

    fn replace_editor_text(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        attachment: Option<Attachment>,
        cx: &mut Context<Self>,
    ) {
        self.undo.push(self.edit_snapshot());
        self.redo.clear();
        self.remove_chips_in_range(&range);
        self.control_revision = self.control_revision.wrapping_add(1);
        self.command_menu_closed = None;
        self.command_selection_explicit = None;
        self.rebase_live_dictation_after_edit(&range, new_text.len());
        let mut content = self.content.to_string();
        content.replace_range(range.clone(), new_text);
        self.content = content.into();
        self.attachments.extend(attachment);
        self.reset_model_groups_after_exit();
        self.sel.commit(range.start, new_text.len());
        self.command_selection = 0;
        self.reveal_command(0);
        self.ghost_on_edit(cx);
        if let Some(on_change) = &self.on_change {
            on_change(&self.content, cx);
        }
        cx.notify();
    }
}

impl EntityInputHandler for PromptInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = range_from_utf16(&self.content, &range_utf16);
        actual_range.replace(range_to_utf16(&self.content, &range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: range_to_utf16(&self.content, &self.sel.range),
            reversed: self.sel.reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.sel
            .marked
            .as_ref()
            .map(|range| range_to_utf16(&self.content, range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.sel.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.replacement_range(range_utf16);
        // The platform also sends unmarked bulk insertions through insertText.
        // A marked IME candidate remains editable text even when it is long.
        if self.sel.marked.is_none() && self.try_attach_pasted_text(&range, new_text, cx) {
            self.finish_paste();
        } else {
            self.replace_editor_text(range, new_text, None, cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.replacement_range(range_utf16);
        self.remove_chips_in_range(&range);
        self.control_revision = self.control_revision.wrapping_add(1);
        self.command_menu_closed = None;
        self.command_selection_explicit = None;
        self.rebase_live_dictation_after_edit(&range, new_text.len());
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.reset_model_groups_after_exit();
        // The IME's selection is UTF-16 offsets into `new_text`, not into the
        // whole document (#6: converting it against the document sliced inside
        // a multi-byte character after CJK text and aborted the app from
        // `setMarkedText:`). `mark` converts it against `new_text`.
        self.sel
            .mark(range.start, new_text, new_selected_range_utf16.as_ref());
        self.dismiss_ghost();

        self.command_selection = 0;
        self.reveal_command(0);
        if let Some(on_change) = &self.on_change {
            on_change(&self.content, cx);
        }

        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let geometry = self.ime_geometry.as_ref()?;
        let chips = self.chip_ranges();
        let display_text = if chips.is_empty() {
            self.content.clone()
        } else {
            crate::image_chip::mask_markers(
                &self.content,
                chips.into_iter().map(|(_, range)| range),
            )
            .into()
        };
        geometry.bounds_for_range(
            display_text,
            range_from_utf16(&self.content, &range_utf16),
            bounds.origin,
            window,
        )
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let local = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        let utf8_index = last_layout
            .closest_index_for_position(local, self.painted_line_height(_window))
            .unwrap_or_else(|index| index)
            .min(self.content.len());
        Some(byte_to_utf16(&self.content, utf8_index))
    }
}

struct TextElement {
    input: Entity<PromptInput>,
}

struct PrepaintState {
    ime_geometry: ImeGeometry,
    line: Option<PromptLayout>,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
    text_bounds: Bounds<Pixels>,
    /// Horizontal gap between the caret and placeholder text, so the caret
    /// never overlaps the first placeholder glyph.
    text_indent: Pixels,
    visual_line_count: usize,
}

/// Space kept between the caret and the example placeholder.
const PLACEHOLDER_INDENT: Pixels = px(6.);
/// Fresh-session composer height: one 16px line plus its 16px padding.
pub(crate) const SPACIOUS_MIN_HEIGHT: f32 = 58.;

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let line_count = self.input.read(cx).visual_line_count.max(1);
        style.size.height = (window.line_height() * line_count).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // The executor clock, so motion and its sleeping ticker share one
        // time source (real time in production, simulated in tests).
        let now = cx.background_executor().now();
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.sel.range.clone();
        let cursor = input.sel.cursor();
        let style = window.text_style();

        let mut placeholder_full = None;
        let mut placeholder_live = false;
        let text_indent = if content.is_empty() {
            PLACEHOLDER_INDENT
        } else {
            px(0.)
        };
        let wrap_width = (bounds.size.width - text_indent).max(px(0.));
        let (display_text, text_color) = if content.is_empty() {
            let (mut text, full, live) = input.placeholder_text(now, cx);
            placeholder_full = full.clone();
            // Only an active window animates. Background windows show the
            // whole example rather than freezing mid-word.
            placeholder_live = live && window.is_window_active();
            if live
                && !placeholder_live
                && let Some(full) = &full
            {
                text = full.clone();
            }
            (text, to_hsla(Theme::global().composer_secondary()))
        } else {
            (content, style.color)
        };
        let ghost = (!input.content.is_empty())
            .then(|| input.ghost_text())
            .flatten()
            .map(str::to_owned);
        let draft_len = display_text.len();
        let display_text: SharedString = match &ghost {
            Some(ghost) => format!("{display_text}{ghost}").into(),
            None => display_text,
        };

        let run = TextRun {
            len: draft_len,
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        // Only underline a composition that actually lies in the shown text.
        // A placeholder or stale range would otherwise underflow the run math.
        let marked_range = input.sel.marked.as_ref().filter(|marked| {
            !input.content.is_empty()
                && marked.start <= marked.end
                && marked.end <= display_text.len()
                && display_text.is_char_boundary(marked.start)
                && display_text.is_char_boundary(marked.end)
        });
        let ghost_run = run.clone();
        let runs = if let Some(marked_range) = marked_range {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: draft_len - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };
        let mut runs: Vec<TextRun> = runs;
        // Preserve source byte offsets for editing and submission, but never
        // paint marker glyphs. The positioned pill paints the whole marker.
        if !input.content.is_empty() {
            let hidden: Vec<_> = input
                .chip_ranges()
                .into_iter()
                .map(|(_, range)| range)
                .collect();
            let mut offset = 0;
            runs = runs
                .into_iter()
                .flat_map(|run| {
                    let start = offset;
                    let end = start + run.len;
                    offset = end;
                    let mut cuts = vec![start, end];
                    for range in &hidden {
                        if range.start > start && range.start < end {
                            cuts.push(range.start);
                        }
                        if range.end > start && range.end < end {
                            cuts.push(range.end);
                        }
                    }
                    cuts.sort_unstable();
                    cuts.dedup();
                    cuts.windows(2)
                        .map(|pair| {
                            let mut part = run.clone();
                            part.len = pair[1] - pair[0];
                            if hidden
                                .iter()
                                .any(|range| range.start <= pair[0] && pair[1] <= range.end)
                            {
                                part.color.a = 0.;
                            }
                            part
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
        }
        if let Some(ghost) = &ghost {
            runs.push(TextRun {
                len: ghost.len(),
                color: to_hsla(Theme::global().composer_secondary()),
                ..ghost_run
            });
        }

        // Shape a same-length word in place of each ASCII marker. GPUI's
        // wrapper breaks on punctuation and even non-breaking spaces, but not
        // inside this word. Byte offsets stay identical to the stored text.
        let shaped = crate::image_chip::mask_markers(
            &display_text,
            input.chip_ranges().into_iter().map(|(_, range)| range),
        );

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let ime_geometry = ImeGeometry::new(
            style.font(),
            font_size,
            line_height,
            Some(bounds.size.width),
        );
        let line = PromptLayout::new(
            window
                .text_system()
                .shape_text(shaped.into(), font_size, &runs, Some(wrap_width), None)
                .expect("prompt text should shape"),
        );
        let target = input.caret_position(&line, line_height).unwrap_or_default();
        let mut visual_line_count = line.visual_line_count();
        if let Some(full) = placeholder_full.filter(|full| !full.is_empty()) {
            // Reserve the fully typed prompt's height so typing never resizes
            // the composer mid-word at narrow widths.
            let full_lines = window
                .text_system()
                .shape_text(
                    full.clone(),
                    font_size,
                    &[TextRun {
                        len: full.len(),
                        font: style.font(),
                        color: text_color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    Some(wrap_width),
                    None,
                )
                .map(|lines| PromptLayout::new(lines).visual_line_count())
                .unwrap_or(1);
            visual_line_count = visual_line_count.max(full_lines);
        }
        let text_bounds = Bounds::new(
            bounds.origin,
            size(bounds.size.width, line_height * visual_line_count),
        );
        let focused = self.input.read(cx).focus_handle.is_focused(window);
        let reduced = motion_reduced(cx);
        let (cursor_pos, caret_alpha, caret_next, caret_gliding) = {
            let input = self.input.read(cx);
            let mut state = input.motion.borrow_mut();
            let state = &mut *state;
            let key = (input.content.clone(), cursor);
            let mut edited = false;
            if state.key.as_ref() != Some(&key) {
                edited = state.key.as_ref().is_some_and(|(old, _)| *old != key.0);
                if key.0.is_empty() && state.key.as_ref().is_some_and(|(old, _)| !old.is_empty()) {
                    // Emptying the composer starts a fresh example.
                    state.seed = state.seed.wrapping_add(1);
                }
                state.key = Some(key);
                state.epoch = now;
            }
            let epoch = state.epoch;
            let glide = state
                .glide
                .get_or_insert_with(|| motion::Glide::new(target, now));
            if edited {
                // Typed text lands this frame, so the caret must too. A
                // glide here trails the new character and forces extra
                // full-panel frames between keystrokes.
                glide.snap(target, now);
            } else {
                glide.retarget(target, line_height, now, reduced);
            }
            let (position, gliding) = glide.position(now);
            let (alpha, next) = motion::caret_alpha(now.saturating_duration_since(epoch), reduced);
            (
                position,
                alpha,
                next.filter(|_| focused),
                focused && gliding,
            )
        };
        if caret_gliding {
            // The 33ms ticker is too coarse for a 55ms glide. Draw every frame.
            window.request_animation_frame();
        }
        let motion_next = if placeholder_live {
            Some(Duration::ZERO)
        } else {
            caret_next
        };
        self.input.read(cx).motion.borrow_mut().next = motion_next;
        let (selection, cursor) = if selected_range.is_empty() {
            let mut color = to_hsla(Theme::global().CURSOR);
            color.a *= caret_alpha;
            (
                Vec::new(),
                Some(gpui::quad(
                    Bounds::new(text_bounds.origin + cursor_pos, size(px(2.), line_height)),
                    px(1.),
                    color,
                    px(0.),
                    gpui::transparent_black(),
                    gpui::BorderStyle::default(),
                )),
            )
        } else {
            (
                selection_quads(&line, selected_range, text_bounds, line_height),
                None,
            )
        };
        PrepaintState {
            ime_geometry,
            line: Some(line),
            cursor,
            selection,
            text_bounds,
            text_indent,
            visual_line_count,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        #[cfg(target_os = "macos")]
        if focus_handle.is_focused(window) {
            crate::ime_log::record(format_args!(
                "composer-handler-register entity={:?}",
                self.input.entity_id()
            ));
        }
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        for selection in prepaint.selection.drain(..) {
            window.paint_quad(selection)
        }
        let line = prepaint.line.take().unwrap();
        line.paint(
            prepaint.text_bounds.origin + point(prepaint.text_indent, px(0.)),
            window.line_height(),
            window,
            cx,
        )
        .unwrap();

        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        self.input.update(cx, |input, _cx| {
            let positions: Vec<_> = input
                .chip_ranges()
                .into_iter()
                .filter_map(|(index, range)| {
                    let mut start = line.position_for_index(range.start, window.line_height())?;
                    let end = line.position_for_index(range.end, window.line_height())?;
                    // GPUI assigns a boundary index to the preceding row even
                    // when its first glyph is on the next visual row.
                    if start.y != end.y {
                        let next =
                            line.position_for_index(range.start + 1, window.line_height())?;
                        if next.y == end.y {
                            start = point(px(0.), next.y);
                        }
                    }
                    (start.y == end.y).then_some((
                        index,
                        Bounds::new(start, size(end.x - start.x, window.line_height())),
                    ))
                })
                .collect();
            if input.chip_positions != positions {
                input.chip_positions = positions;
                let entity = _cx.entity();
                _cx.defer(move |cx| entity.update(cx, |_, cx| cx.notify()));
            }
            // Reveal edits and keyboard navigation, but do not undo wheel scrolling
            // on every paint. Include viewport size so resizing reveals the caret.
            let viewport = input.editor_scroll.bounds();
            let caret_key = (
                input.sel.cursor(),
                input.content.clone(),
                viewport.size,
                input.vertical_caret().is_some_and(|caret| caret.downstream),
            );
            if input.revealed_caret.as_ref() != Some(&caret_key) {
                if let Some(caret) = input.caret_position(&line, window.line_height()) {
                    let top = prepaint.text_bounds.top() + caret.y;
                    let bottom = top + window.line_height();
                    let adjustment = if top < viewport.top() {
                        viewport.top() - top
                    } else if bottom > viewport.bottom() {
                        viewport.bottom() - bottom
                    } else {
                        px(0.)
                    };
                    if adjustment != px(0.) {
                        let offset = input.editor_scroll.offset();
                        input
                            .editor_scroll
                            .set_offset(point(px(0.), (offset.y + adjustment).min(px(0.))));
                        let entity = _cx.entity();
                        _cx.defer(move |cx| entity.update(cx, |_, cx| cx.notify()));
                    }
                }
                input.revealed_caret = Some(caret_key);
            }
            input.last_layout = Some(line);
            input.ime_geometry = Some(prepaint.ime_geometry.clone());
            input.last_layout_revision = input.control_revision;
            input.last_bounds = Some(prepaint.text_bounds);
            input.last_line_height = Some(window.line_height());
            let needs_ticker = {
                let state = input.motion.borrow();
                state.next.is_some() && state.ticker.is_none()
            };
            if needs_ticker {
                // One bounded ticker drives placeholder typing and caret
                // breathing. It sleeps through the solid caret phase and exits
                // as soon as motion settles. It notifies this input by id and
                // never updates it, so the panel reading the input is not
                // built again for a caret frame.
                let shared = input.motion.clone();
                let id = _cx.entity_id();
                let weak = _cx.entity().downgrade();
                let ticker = _cx.spawn(async move |_, cx| {
                    loop {
                        let idle = shared.borrow().next.unwrap_or(Duration::ZERO);
                        if idle > motion::TICK {
                            cx.background_executor().timer(idle - motion::TICK).await;
                        }
                        crate::animation_clock::next_tick(cx.background_executor(), motion::TICK)
                            .await;
                        if weak.upgrade().is_none() {
                            break;
                        }
                        let keep = {
                            let mut state = shared.borrow_mut();
                            if state.next.take().is_none() {
                                state.ticker = None;
                                false
                            } else {
                                true
                            }
                        };
                        if !keep {
                            break;
                        }
                        cx.update(|cx| cx.notify(id));
                    }
                });
                input.motion.borrow_mut().ticker = Some(ticker);
            }
            if input.visual_line_count != prepaint.visual_line_count {
                // Layout used the previous height this frame. Retry revealing the
                // caret after the new scroll extent exists, even at the height cap.
                input.revealed_caret = None;
                input.visual_line_count = prepaint.visual_line_count;
                let entity = _cx.entity();
                _cx.defer(move |cx| entity.update(cx, |_, cx| cx.notify()));
            }
        });
    }
}

impl Render for PromptInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("PromptInput");
        let image_chip_numbers = self.image_chip_numbers();
        #[cfg(test)]
        crate::render_stats::test_renders::record(cx.entity_id());
        let focused = self.focus_handle.is_focused(window);
        #[cfg(target_os = "macos")]
        if crate::ime_log::enabled() && self.ime_log_focus_subscriptions.is_none() {
            let id = cx.entity_id();
            crate::ime_log::record(format_args!(
                "composer-focus-initial entity={id:?} focused={focused}"
            ));
            let focus = cx.on_focus(&self.focus_handle, window, move |_, _, _| {
                crate::ime_log::record(format_args!("composer-focus entity={id:?}"));
            });
            let blur = cx.on_blur(&self.focus_handle, window, move |_, _, _| {
                crate::ime_log::record(format_args!("composer-blur entity={id:?}"));
            });
            self.ime_log_focus_subscriptions = Some(vec![focus, blur]);
        }
        let editor_height = self.editor_height(window);
        let show_resize = self.can_resize_editor(window);
        let spacious = self.spacious && window.viewport_size().height >= px(400.);
        self.update_context_index(cx);
        let context_menu = self.standalone_context() || self.mention_token().is_some();
        let suggestions = self.command_suggestions();
        if self.command_selection >= suggestions.len() {
            self.command_selection = 0;
        }
        let command_visible = !self.external_picker_open
            && self.command_completion
            && self.current_command_menu_close().is_none()
            && (!suggestions.is_empty()
                || self.picker_prefix.is_some()
                || context_menu
                || self.content.trim_start().starts_with("/model "));
        let menu_duration = jcode_desktop_motion::policy(
            crate::transition::Transition::Menu,
            context_menu || crate::config::get().appearance.reduce_motion || cx.reduce_motion(),
        )
        .duration;
        let menu_progress =
            self.command_entrance
                .update(command_visible, Instant::now(), menu_duration);
        if self.command_entrance.is_animating() {
            window.request_animation_frame();
        }
        let command_layout_key = (!suggestions.is_empty()).then(|| {
            (
                self.content.clone(),
                suggestions.len(),
                window.viewport_size(),
            )
        });
        if self.command_layout_key != command_layout_key {
            self.command_layout_key = command_layout_key;
            // List geometry is updated during layout, after render.
            // One follow-up frame keeps the thumb correct without idle polling.
            let input = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = input.update(cx, |_, cx| cx.notify());
            });
        }
        let picker_detail = self.picker_detail(&suggestions, window);
        let attachment_overlay = self.render_paste_preview(window, cx);
        div()
            .debug_selector(|| "prompt-input".into())
            .children(attachment_overlay)
            .flex()
            .flex_col()
            .key_context(if self.queue_editor {
                "PromptInput QueueEditor"
            } else if context_menu {
                "PromptInput ContextPicker"
            } else if self.on_model_speed.is_some()
                && self.model_stage == model_stage::ModelStage::Models
                && self.command_content().split_whitespace().next() == Some("/model")
            {
                "PromptInput ModelPicker"
            } else {
                "PromptInput"
            })
            .track_focus(&self.focus_handle(cx).tab_index(0).tab_stop(true))
            .relative()
            .child({
                let measured = self.composer_bounds.clone();
                gpui::canvas(
                    |_, _, _| (),
                    move |bounds, _, _, _| {
                        // Absolute children fill the padding box. Include the root's
                        // one-pixel border so reparenting preserves its exact footprint.
                        measured.set(Some(Bounds::new(
                            bounds.origin - point(px(1.0), px(1.0)),
                            size(bounds.size.width + px(2.0), bounds.size.height + px(2.0)),
                        )));
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full()
            })
            .when(command_visible, |el| {
                el.child(
                    gpui::deferred(
                        popup::Popup::new(
                            div()
                                .id("slash-command-suggestions")
                                .debug_selector(|| "slash-command-overlay".into())
                                .w_full()
                                .max_h_full()
                                .opacity(if context_menu { 1.0 } else { 0.65 + 0.35 * menu_progress })
                                .flex()
                                .flex_col()
                                .rounded_lg()
                                .border_1()
                                .border_color(Theme::global().PANEL_BORDER_FOCUS)
                                .bg(Theme::global().HEADER_BG)
                                .shadow_lg()
                                .occlude()
                                .child(
                                    div()
                                        .relative()
                                        .flex()
                                        .flex_col()
                                        .min_h_0()
                                        .p_1()
                                        .child(
                                            div()
                                                .id("slash-command-scroll")
                                                .debug_selector(|| "slash-command-scroll".into())
                                                .min_h_0()
                                                .flex()
                                                .flex_col()
                                                .max_h(popup::scroll_height(
                                                    window.viewport_size().height,
                                                    context_menu || self.picker_prefix == Some("/model")
                                                        || self
                                                            .content
                                                            .trim_start()
                                                            .starts_with("/model"),
                                                ))
                                                .on_scroll_wheel(cx.listener(|_, _, _, cx| {
                                                    cx.stop_propagation();
                                                    cx.notify();
                                                }))
                                                .pr(px(crate::scrollbar::GUTTER - 4.0))
                                                .when(suggestions.is_empty(), |el| {
                                                    el.child(
                                                        div()
                                                            .px_3()
                                                            .py_2()
                                                            .text_size(px(12.0))
                                                            .text_color(
                                                                Theme::global()
                                                                    .composer_secondary(),
                                                            )
                                                            .child(match self.picker_prefix {
                                                                _ if context_menu => self.context_empty_label(),
                                                                Some("/effort") => {
                                                                    "No effort levels match"
                                                                }
                                                                Some("/mode") => {
                                                                    "No modes or skills match"
                                                                }
                                                                _ => "No models match",
                                                            }),
                                                    )
                                                })
                                                .when(!suggestions.is_empty(), |el| {
                                                    let input = cx.entity();
                                                    el.child(
                                                        gpui::list(
                                                            self.command_scroll.clone(),
                                                            move |index, window, cx| {
                                                                input.update(cx, |input, cx| {
                                                                    input.render_command_row(
                                                                        index,
                                                                        suggestions[index].clone(),
                                                                        window,
                                                                        cx,
                                                                    )
                                                                })
                                                            },
                                                        )
                                                        .with_sizing_behavior(
                                                            gpui::ListSizingBehavior::Infer,
                                                        )
                                                        .min_h_0()
                                                        .w_full(),
                                                    )
                                                }),
                                        )
                                        .child(crate::scrollbar::vertical_list(
                                            &self.command_scroll,
                                            "slash-command-scrollbar",
                                        )),
                                )
                                .children(picker_detail),
                            px(4.0 + 6.0 * (1.0 - menu_progress)),
                        )
                        .dismiss_outside(self.on_picker_outside.clone()),
                    )
                    .with_priority(1),
                )
            })
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::insert_newline))
            .on_action(cx.listener(Self::shift_space))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(|this, _: &DecreasePickerSpeed, _, cx| {
                this.step_picker_speed(-1, cx);
            }))
            .on_action(cx.listener(|this, _: &IncreasePickerSpeed, _, cx| {
                this.step_picker_speed(1, cx);
            }))
            // Inside the model menu the effort keys set the highlighted
            // row's level instead of the running session's.
            .capture_action(
                cx.listener(|this, _: &crate::effort::IncreaseEffort, _, cx| {
                    if this.step_picker_effort(1, cx) {
                        cx.stop_propagation();
                    } else {
                        cx.propagate();
                    }
                }),
            )
            .capture_action(
                cx.listener(|this, _: &crate::effort::DecreaseEffort, _, cx| {
                    if this.step_picker_effort(-1, cx) {
                        cx.stop_propagation();
                    } else {
                        cx.propagate();
                    }
                }),
            )
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::paste_plain_text))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::delete_word_back))
            .on_action(cx.listener(Self::delete_word_forward))
            .on_action(cx.listener(Self::kill_to_start))
            .on_action(cx.listener(Self::kill_to_end))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::history_prev))
            .on_action(cx.listener(Self::history_next))
            .on_action(cx.listener(Self::grow_editor))
            .on_action(cx.listener(Self::shrink_editor))
            .on_action(cx.listener(Self::clear))
            .on_action(cx.listener(Self::stash))
            .on_action(cx.listener(Self::accept_completion))
            .on_action(cx.listener(Self::alternate_context))
            .when(!self.host_handles_submission, |el| {
                el.on_action(cx.listener(Self::submit))
                    .on_action(cx.listener(Self::queue))
            })
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .min_w_0()
            .flex_none()
            .when(self.chrome, |el| el.bg(Theme::global().INPUT_BG))
            .border_1()
            .border_color(if !self.chrome {
                gpui::transparent_black().into()
            } else if focused {
                Theme::global().PANEL_BORDER_FOCUS
            } else {
                Theme::global().INPUT_BORDER
            })
            // Near-pill corners. A single-line composer is ~38px tall, so an
            // 18px radius reads as fully round while multi-line drafts keep
            // straight sides instead of turning into an ellipse.
            .rounded(px(18.0))
            .children(
                self.attachment_notice
                    .clone()
                    .or_else(|| self.transient_notice.clone())
                    .map(|notice| {
                        div()
                            .debug_selector(|| "composer-notice".into())
                            .px_3()
                            .pt_1()
                            .text_size(px(10.0))
                            .text_color(Theme::global().composer_secondary())
                            .child(notice)
                    }),
            )
            .when(self.stashed_prompt.is_some(), |el| {
                el.child(
                    div()
                        .debug_selector(|| "prompt-stashed".into())
                        .px_3()
                        .pt_1()
                        .text_size(px(10.0))
                        .text_color(Theme::global().composer_secondary())
                        .child("Prompt stashed · Ctrl+S to restore"),
                )
            })
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_start()
                    .px_4()
                    .py_2()
                    .when(self.compact_padding, |el| el.py(px(3.)))
                    .text_size(px(14.0))
                    .when(spacious, |el| {
                        // One line of 16px text plus padding.
                        el.min_h(px(SPACIOUS_MIN_HEIGHT))
                            .px_4()
                            .py_4()
                            .text_size(px(16.0))
                    })
                    .when(self.trailing_inset > 0., |el| {
                        el.pr(px(self.trailing_inset))
                    })
                    .child(
                        div()
                            .id("prompt-editor")
                            .debug_selector(|| "prompt-editor".into())
                            .flex_1()
                            .min_w_0()
                            .max_h(editor_height)
                            .overflow_y_scroll()
                            .track_scroll(&self.editor_scroll)
                            .on_scroll_wheel(cx.listener(
                                |this, event: &gpui::ScrollWheelEvent, window, cx| {
                                    if this.compact_padding {
                                        let delta = event.delta.pixel_delta(window.line_height()).y;
                                        let before = this.editor_scroll.offset().y - delta;
                                        let max = this.editor_scroll.max_offset().y;
                                        if (delta < px(0.) && before > -max)
                                            || (delta > px(0.) && before < px(0.))
                                        {
                                            cx.stop_propagation();
                                        }
                                    } else {
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .text_color(Theme::global().TEXT)
                            .child(
                                div()
                                    .relative()
                                    .child(TextElement { input: cx.entity() })
                                    .children(self.chip_positions.iter().filter_map(
                                        |(index, bounds)| {
                                            let attachment = self.attachments.get(*index)?;
                                            let index = *index;
                                            let chip = crate::image_chip::chip_with_label(
                                                match &attachment.payload {
                                                    AttachmentPayload::Image {
                                                        preview, ..
                                                    } => Some(preview.clone()),
                                                    AttachmentPayload::Text(_) | AttachmentPayload::Context(_) => None,
                                                    AttachmentPayload::File { image, .. }
                                                    | AttachmentPayload::Queued {
                                                        preview: image,
                                                        ..
                                                    } => image
                                                        .as_ref()
                                                        .map(|image| image.preview.clone()),
                                                },
                                                image_chip_numbers[index].map_or_else(
                                                    || crate::image_chip::label(&attachment.label),
                                                    crate::image_chip::numbered_label,
                                                ),
                                                "composer-image-chip",
                                                "composer-image-thumbnail",
                                                "composer-image-label",
                                            )
                                            .absolute()
                                            .left(bounds.origin.x)
                                            .top(bounds.origin.y)
                                            .w(bounds.size.width)
                                            .h(bounds.size.height);
                                            let chip = if matches!(
                                                attachment.payload,
                                                AttachmentPayload::Image { .. }
                                                    | AttachmentPayload::File {
                                                        image: Some(_),
                                                        ..
                                                    }
                                                    | AttachmentPayload::Text(_)
                                                    | AttachmentPayload::Context(DraftContext::File { .. })
                                            ) {
                                                crate::keyboard::button(
                                                    chip.id(("composer-attachment", index)),
                                                )
                                                .on_click(cx.listener(
                                                    move |this, event, window, cx| {
                                                        if matches!(
                                                            event,
                                                            gpui::ClickEvent::Keyboard(_)
                                                        ) {
                                                            this.activate_attachment(
                                                                index, window, cx,
                                                            );
                                                        }
                                                    },
                                                ))
                                                .into_any_element()
                                            } else {
                                                chip.into_any_element()
                                            };
                                            Some(chip)
                                        },
                                    )),
                            ),
                    ),
            )
            .when(show_resize, |el| {
                el.child(self.render_editor_resize(window, cx))
            })
    }
}

impl Focusable for PromptInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn is_large_paste(text: &str) -> bool {
    text.split('\n').nth(10).is_some() || text.encode_utf16().nth(1000).is_some()
}

#[cfg(test)]
#[path = "input/model/order_tests.rs"]
mod model_order_tests;
#[cfg(test)]
#[path = "input/model/picker_tests.rs"]
mod model_picker_tests;
#[cfg(test)]
#[path = "input/model/profile_tests.rs"]
mod model_profile_tests;
#[cfg(test)]
#[path = "input/model/stage_tests.rs"]
mod model_stage_tests;
#[cfg(test)]
#[path = "input/tests/pending_session.rs"]
mod pending_session_tests;
#[cfg(test)]
#[path = "input/model/grid_tests.rs"]
mod picker_grid_tests;
#[cfg(test)]
#[path = "input/tests/responsive.rs"]
mod responsive_tests;
#[cfg(test)]
#[path = "input/tests/base.rs"]
mod tests;

