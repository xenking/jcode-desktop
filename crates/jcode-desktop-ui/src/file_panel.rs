//! A source or Markdown file open as its own panel kind: an editor with an
//! optional rendered preview. It has no session, composer or transcript.
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Task, Window, div, prelude::FluentBuilder as _, px,
};
use jcode_desktop_model::markdown_file::{self, LinkTarget};

use crate::code_editor::CodeEditor;
use crate::markdown;
use crate::text_selection::{self, TextSelection};
use crate::theme::{Ink, Theme};

const MAX_MARKDOWN_PREVIEW_BYTES: usize = 2 * 1024 * 1024;

struct MarkdownFilePreview {
    visible: bool,
    scroll: ScrollHandle,
    document: markdown::FileDocument,
    revision: u64,
}

pub(crate) struct FilePanel {
    path: PathBuf,
    session_id: String,
    title: SharedString,
    working_dir: Option<String>,
    editor: Entity<CodeEditor>,
    loaded: bool,
    markdown: Option<MarkdownFilePreview>,
    pending_heading: Option<String>,
    preview_task: Option<Task<()>>,
    badge: Option<&'static str>,
    focus_handle: FocusHandle,
    selection: Entity<TextSelection>,
    surface_focused: bool,
}

impl FilePanel {
    pub(crate) fn session_id_for(path: &Path) -> String {
        format!("file://{}", path.display())
    }

    pub(crate) fn new(path: PathBuf, cx: &mut Context<Self>) -> Self {
        let title: SharedString = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
            .into();
        let working_dir = path.parent().map(|parent| parent.display().to_string());
        let is_markdown = markdown_file::is_markdown(&path);
        let supports_preview = is_markdown
            || path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("txt"));
        let editor = cx.new(|cx| CodeEditor::new(path.clone(), cx));
        cx.observe(&editor, |file: &mut Self, editor, cx| {
            let badge = editor.read(cx).badge();
            if file.badge != badge {
                file.badge = badge;
                cx.notify();
            }
        })
        .detach();
        let selection = cx.new(TextSelection::new_scoped);
        cx.observe(&selection, |_, _, cx| cx.notify()).detach();
        let requested = path.clone();
        cx.spawn(async move |file, cx| {
            let (loaded, document) = cx
                .background_executor()
                .spawn(async move {
                    let loaded = jcode_desktop_model::text_buffer::load(&requested);
                    let document = loaded
                        .as_ref()
                        .ok()
                        .filter(|_| supports_preview)
                        .and_then(|file| {
                            let snapshot = file.buffer.snapshot();
                            (snapshot.byte_len() <= MAX_MARKDOWN_PREVIEW_BYTES)
                                .then(|| markdown::FileDocument::parse(&snapshot.text()))
                        });
                    (loaded, document)
                })
                .await;
            let _ = file.update(cx, |file, cx| {
                file.editor
                    .update(cx, |editor, cx| editor.set_loaded(loaded, cx));
                file.loaded = true;
                file.markdown = document.map(|document| MarkdownFilePreview {
                    visible: is_markdown,
                    scroll: ScrollHandle::new(),
                    document,
                    revision: 0,
                });
                if let Some(fragment) = file.pending_heading.take() {
                    file.show_heading(&fragment, cx);
                }
                cx.notify();
            });
        })
        .detach();
        Self {
            session_id: Self::session_id_for(&path),
            path,
            title,
            working_dir,
            editor,
            loaded: false,
            markdown: None,
            pending_heading: None,
            preview_task: None,
            badge: None,
            focus_handle: cx.focus_handle(),
            selection,
            surface_focused: false,
        }
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn title(&self) -> &SharedString {
        &self.title
    }

    pub(crate) fn working_dir(&self) -> Option<&str> {
        self.working_dir.as_deref()
    }

    pub(crate) fn surface_focused(&self) -> bool {
        self.surface_focused
    }

    pub(crate) fn set_surface_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        if self.surface_focused != focused {
            self.surface_focused = focused;
            cx.notify();
        }
    }

    pub(crate) fn selection_focus_handle(&self, cx: &App) -> FocusHandle {
        self.selection.read(cx).focus_handle().clone()
    }

    pub(crate) fn panel_focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// Where keys typed into this panel go: the editor while the source shows,
    /// the panel itself while the rendered preview does.
    pub(crate) fn input_focus_handle(&self, cx: &App) -> FocusHandle {
        if self.markdown.as_ref().is_some_and(|preview| preview.visible) {
            self.focus_handle.clone()
        } else {
            self.editor.read(cx).focus_handle.clone()
        }
    }

    pub(crate) fn show_heading(&mut self, fragment: &str, cx: &mut Context<Self>) {
        if !self.loaded {
            self.pending_heading = Some(fragment.to_owned());
            return;
        }
        let Some(preview) = &mut self.markdown else {
            return;
        };
        let Some(index) = preview.document.heading_index(fragment) else {
            return;
        };
        preview.visible = true;
        preview.scroll.scroll_to_top_of_item(index);
        self.refresh_preview(cx);
        cx.notify();
    }

    /// Unsaved edits may veto closing.
    pub(crate) fn confirm_close(&self, cx: &mut App) -> bool {
        self.editor
            .update(cx, |editor, cx| editor.confirm_close(cx))
    }

    fn render_markdown(
        &self,
        preview: &MarkdownFilePreview,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let base = self.path.parent().unwrap_or(Path::new("/")).to_path_buf();
        let selection = self.selection.clone();
        let on_link = link_handler(
            base.clone(),
            Some((self.path.clone(), cx.entity().downgrade())),
        );
        let resolve_image = Rc::new(move |destination: &str| {
            let LinkTarget::Local { path, .. } = markdown_file::resolve(&base, destination)? else {
                return None;
            };
            Some(path)
        });
        div()
            .id("markdown-file")
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .on_mouse_up(gpui::MouseButton::Left, {
                let selection = selection.clone();
                move |_, _, cx| {
                    selection.update(cx, |selection, cx| {
                        selection.finish();
                        cx.notify();
                    })
                }
            })
            .child(markdown::render_file(
                &preview.document,
                &selection,
                window,
                cx,
                markdown::FileRenderOptions {
                    scroll: preview.scroll.clone(),
                    on_link,
                    resolve_image,
                },
            ))
            .child(crate::scrollbar::vertical(
                &preview.scroll,
                "markdown-file-scrollbar",
            ))
            .into_any_element()
    }

    fn toggle_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preview) = &mut self.markdown else {
            return;
        };
        if preview.visible {
            preview.visible = false;
            let focus = self.editor.read(cx).focus_handle.clone();
            window.focus(&focus, cx);
        } else {
            preview.visible = true;
            window.focus(&self.focus_handle, cx);
            self.refresh_preview(cx);
        }
        cx.notify();
    }

    fn refresh_preview(&mut self, cx: &mut Context<Self>) {
        let (Some(preview), Some((revision, snapshot))) =
            (&self.markdown, self.editor.read(cx).preview_snapshot())
        else {
            return;
        };
        if preview.revision == revision || self.preview_task.is_some() {
            return;
        }
        self.preview_task = Some(cx.spawn(async move |file, cx| {
            let document = cx
                .background_executor()
                .spawn(async move {
                    if snapshot.byte_len() > MAX_MARKDOWN_PREVIEW_BYTES {
                        markdown::FileDocument::parse(
                            "Preview unavailable for files larger than 2 MB.",
                        )
                    } else {
                        markdown::FileDocument::parse(&snapshot.text())
                    }
                })
                .await;
            let _ = file.update(cx, |file, cx| {
                file.preview_task = None;
                if let Some(preview) = &mut file.markdown {
                    preview.document = document;
                    preview.revision = revision;
                }
                if file
                    .markdown
                    .as_ref()
                    .is_some_and(|preview| preview.visible)
                {
                    file.refresh_preview(cx);
                }
                cx.notify();
            });
        }));
    }
}

impl Focusable for FilePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Opens links found in Markdown: external ones in the browser, local ones as
/// documents. `document` names the file being shown, so links back into it
/// scroll instead of opening a second panel.
pub(crate) fn link_handler(
    base: PathBuf,
    document: Option<(PathBuf, gpui::WeakEntity<FilePanel>)>,
) -> Rc<markdown::FileLinkHandler> {
    Rc::new(markdown::FileLinkHandler {
        base,
        open: Box::new(move |target, window, cx| match target {
            LinkTarget::External(url) => cx.open_url(&url),
            LinkTarget::Local { path, fragment } => {
                if let Some((current, panel)) = &document
                    && current == &path
                {
                    if let Some(fragment) = fragment {
                        let _ = panel.update(cx, |panel, cx| panel.show_heading(&fragment, cx));
                    }
                } else {
                    window.dispatch_action(
                        Box::new(crate::ui_actions::OpenDocumentFile { path, fragment }),
                        cx,
                    );
                }
            }
            LinkTarget::Anchor(fragment) => {
                if let Some((_, panel)) = &document {
                    let _ = panel.update(cx, |panel, cx| panel.show_heading(&fragment, cx));
                }
            }
        }),
    })
}

impl Render for FilePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("FilePanel");
        let path = self.path.display().to_string();
        let preview = self.markdown.as_ref().filter(|preview| preview.visible);
        let body = if let Some(preview) = preview {
            self.render_markdown(preview, window, cx)
        } else {
            div().id("code-file-contents").debug_selector(|| "code-file-contents".into())
                .flex_1().min_h_0().child(self.editor.clone()).into_any_element()
        };
        let contents = div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(Theme::global().CODE_BORDER)
                    .bg(Theme::global().CODE_HEADER_BG)
                    .font_family(Theme::global().FONT_MONO)
                    .text_size(px(11.0))
                    .text_color(Theme::global().TEXT_DIM)
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(text_selection::plain(
                                self.selection.clone(),
                                "code-file-path",
                                path,
                                window,
                                cx,
                            )),
                    )
                    .when_some(self.badge, |header, badge| {
                        header.child(div().id("code-file-badge").debug_selector(|| "code-file-badge".into())
                            .flex_none().text_color(Theme::global().ACCENT).child(badge))
                    })
                    .when(self.markdown.is_some(), |header| {
                        header.child(
                            div()
                                .id("toggle-file-preview")
                                .debug_selector(|| "toggle-file-preview".into())
                                .flex_none()
                                .rounded_full()
                                .px_3()
                                .py_1()
                                .font_family(Theme::global().FONT_UI)
                                .text_color(Theme::global().TEXT)
                                .bg(Theme::global().CODE_BG)
                                .hover(|style| style.bg(Theme::global().wash(Ink::Accent, 0.15)))
                                .cursor_pointer()
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.toggle_preview(window, cx)),
                                )
                                .child(if preview.is_some() {
                                    "Source"
                                } else {
                                    "Preview"
                                }),
                        )
                    }),
            )
            .child(body);
        div()
            .size_full()
            .track_focus(&self.focus_handle)
            .key_context("FileDocument")
            .on_action(cx.listener(|panel, _: &crate::code_editor::Save, _, cx| {
                panel.editor.update(cx, |editor, cx| editor.save_to_disk(cx));
                cx.stop_propagation();
            }))
            .child(TextSelection::focus_scope(
                contents,
                &self.selection,
                cx,
            ))
            .into_any_element()
    }
}
