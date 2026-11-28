//! Worktree creation inside the unified project sidebar. There is no separate
//! worktree mode: every checkout of a project, and the threads (including
//! swarm agents) running in it, share one hierarchy of
//! project, then branch or worktree, then threads.
use super::sidebar_atoms::{BRANCH_ICON, icon};
use super::*;


#[derive(Default)]
pub(super) struct State {
    pub(super) creating: bool,
    pub(super) error: Option<String>,
    pub(super) input: Option<Entity<PromptInput>>,
    /// Checkout the new worktree branches from.
    pub(super) input_directory: Option<String>,
    creation: Option<gpui::Task<()>>,
}

impl Workspace {
    pub(super) fn init_worktree_fixture(&mut self, cx: &mut Context<Self>) {
        if !harness::screenshot_mode()
            || std::env::var_os("JCODE_DESKTOP_SCREENSHOT_WORKTREES").is_none()
        {
            return;
        }
        // Real `.git` markers so the sidebar exercises its filesystem probe.
        let root =
            std::env::temp_dir().join(format!("jcode-worktree-fixture-{}", std::process::id()));
        let main = root.join("example");
        let _ = std::fs::create_dir_all(main.join(".git/worktrees/search"));
        let _ = std::fs::write(main.join(".git/HEAD"), "ref: refs/heads/main\n");
        let search = root.join("example-search");
        let _ = std::fs::create_dir_all(&search);
        let gitdir = main.join(".git/worktrees/search");
        let _ = std::fs::write(
            search.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        );
        let _ = std::fs::write(gitdir.join("commondir"), "../..\n");
        let _ = std::fs::write(gitdir.join("HEAD"), "ref: refs/heads/feature/search\n");
        let main = main.to_string_lossy().into_owned();
        let search = search.to_string_lossy().into_owned();
        if let Some(first) = self.sessions.first_mut() {
            first.working_dir = Some(main.clone());
        }
        for slot in &self.slots {
            slot.panel
                .update_chat(cx, |panel, cx| panel.set_working_dir(Some(main.clone()), cx));
        }
        let mut session = self.sessions[0].clone();
        session.session_id = "worktree-fixture-search".into();
        session.title = Some("Build project search".into());
        session.working_dir = Some(search.clone());
        session.updated_at_ms = Some(unix_now_ms() - 120_000);
        self.sessions.push(session.clone());
        session.session_id = "worktree-fixture-index".into();
        session.title = Some("Index workspace files".into());
        session.updated_at_ms = Some(unix_now_ms() - 7_200_000);
        self.sessions.push(session);
    }

    pub(super) fn begin_worktree(
        &mut self,
        directory: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.worktrees.creating {
            return;
        }
        let weak = cx.weak_entity();
        let cancel = weak.clone();
        let input = cx.new(|cx| {
            PromptInput::new(
                cx,
                "New branch name, e.g. feature/search",
                move |branch, _, _, app| {
                    let _ = weak.update(app, |this, cx| this.create_worktree(&branch, cx));
                },
            )
            .with_on_overlay_cancel(move |app| {
                let _ = cancel.update(app, |this, cx| this.cancel_worktree(cx));
                true
            })
        });
        window.focus(&input.read(cx).editor_focus(), cx);
        self.focus_pending = false;
        self.worktrees.input = Some(input);
        self.worktrees.input_directory = Some(directory);
        self.worktrees.error = None;
        cx.notify();
    }

    fn cancel_worktree(&mut self, cx: &mut Context<Self>) {
        if !self.worktrees.creating {
            self.worktrees.input = None;
            self.worktrees.input_directory = None;
            self.worktrees.error = None;
            self.focus_pending = true;
            cx.notify();
        }
    }

    pub(super) fn create_worktree(&mut self, branch: &str, cx: &mut Context<Self>) {
        if self.worktrees.creating {
            return;
        }
        let Some(directory) = self.worktrees.input_directory.clone() else {
            return;
        };
        let branch = branch.trim().to_owned();
        if branch.is_empty() {
            self.worktrees.error = Some("Enter a new branch name.".into());
            cx.notify();
            return;
        }
        self.worktrees.creating = true;
        self.worktrees.error = None;
        let task = cx.background_executor().spawn(async move {
            jcode_sdk::worktrees::create(&directory, &branch).map_err(|error| format!("{error:#}"))
        });
        self.worktrees.creation = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.worktrees.creating = false;
                match result {
                    Ok(entry) => {
                        this.worktrees.input = None;
                        this.worktrees.input_directory = None;
                        // The new thread is what makes the worktree appear
                        // under its project, so always open one.
                        this.focus_pending = true;
                        this.open_local_draft(Some(entry.path), cx);
                    }
                    Err(error) => this.worktrees.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Inline form shown above the thread list while naming a new worktree.
    pub(super) fn render_worktree_form(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let input = self.worktrees.input.clone()?;
        let origin = self
            .worktrees
            .input_directory
            .as_deref()
            .map(|dir| {
                Path::new(dir)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| dir.to_owned())
            })
            .unwrap_or_default();
        Some(
            div()
                .id("worktree-create-form")
                .debug_selector(|| "worktree-create-form".into())
                .flex_none()
                .mx_2()
                .mb_2()
                .p_2()
                .rounded_xl()
                .bg(Theme::global().PANEL_BG)
                .flex()
                .flex_col()
                .gap_2()
                .capture_action(cx.listener(|this, _: &crate::input::Submit, _, cx| {
                    cx.stop_propagation();
                    if let Some(input) = &this.worktrees.input {
                        let branch = input.read(cx).text_owned();
                        this.create_worktree(&branch, cx);
                    }
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_size(px(11.0))
                        .text_color(Theme::global().TEXT_DIM)
                        .child(icon(BRANCH_ICON, 12.0))
                        .child(if self.worktrees.creating {
                            format!("Creating worktree in {origin}…")
                        } else {
                            format!("New worktree in {origin}")
                        }),
                )
                .child(input)
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(Theme::global().TEXT_DIM)
                        .child("Branches from HEAD. Uncommitted edits stay put. Enter to create, Esc to cancel."),
                )
                .children(self.worktrees.error.clone().map(|error| {
                    div()
                        .id("worktree-error")
                        .debug_selector(|| "worktree-error".into())
                        .text_size(px(11.0))
                        .text_color(Theme::global().ERROR)
                        .child(error)
                }))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
#[path = "sidebar_worktrees_tests.rs"]
mod tests;
