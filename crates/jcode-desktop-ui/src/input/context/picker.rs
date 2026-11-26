use super::*;
use jcode_desktop_model::context_files::FileIndex;
use std::path::{Path, PathBuf};

impl DraftContext {
    pub(super) fn prompt_text(&self) -> Option<String> {
        match self {
            Self::Skill(_) => None,
            Self::File { path, contents } => {
                let reference = file_drop::path_text(std::slice::from_ref(path));
                Some(match contents {
                    Some(text) => format!("{reference}\n\nFile contents:\n{text}\n\n"),
                    None => reference,
                })
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Picker {
    root: Option<PathBuf>,
    index: Option<FileIndex>,
    indexing: Option<gpui::Task<()>>,
    open: bool,
    target: Option<Target>,
    /// The configured MCP servers, once read. `None` until the first read lands.
    mcp: Option<Vec<mcp_servers::Server>>,
    /// The MCP submenu replaces the list while it is open.
    mcp_open: bool,
}

#[derive(Clone)]
struct Target {
    /// The prompt the chosen context lands in, once the standalone picker
    /// closes. Held type-erased: the picker is a part of the prompt, not
    /// something the prompt's own editor type can be named from.
    draft: gpui::AnyWeakEntity,
    accept: std::rc::Rc<dyn Fn(&mut App)>,
}

fn mention(content: &str, cursor: usize) -> Option<(Range<usize>, &str)> {
    let before = content.get(..cursor)?;
    let start = before.rfind(char::is_whitespace).map_or(0, |i| {
        i + before[i..].chars().next().expect("whitespace").len_utf8()
    });
    let query = before[start..].strip_prefix('@')?;
    if query.contains('@') {
        return None;
    }
    let end = content[cursor..]
        .find(char::is_whitespace)
        .map_or(content.len(), |i| cursor + i);
    Some((start..end, query))
}

fn row(value: String, help: String, header: Option<&str>, action: Action) -> CommandSuggestion {
    CommandSuggestion {
        value,
        help,
        header: header.map(str::to_owned),
        active_choice: false,
        detail: None,
        toggle: None,
        action: None,
        context_action: Some(action),
    }
}

impl PromptInput {
    pub(crate) fn enable_context_picker(&mut self) {
        self.context_picker = Some(Picker::default());
    }

    pub(crate) fn context_root(&self) -> Option<&Path> {
        self.context_picker.as_ref()?.root.as_deref()
    }

    pub(crate) fn set_context_root(&mut self, root: Option<&Path>, cx: &mut Context<Self>) {
        if let Some(picker) = &mut self.context_picker
            && picker.root.as_deref() != root
        {
            picker.root = root.map(Path::to_path_buf);
            picker.index = None;
            picker.indexing = None;
            picker.open = false;
            self.suggestions_revision += 1;
            cx.notify();
        }
    }

    pub(crate) fn start_context_picker(
        &mut self,
        root: Option<&Path>,
        draft: gpui::WeakEntity<Self>,
        accept: impl Fn(&mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.enable_context_picker();
        self.set_context_root(root, cx);
        self.context_picker.as_mut().expect("enabled").target = Some(Target {
            draft: draft.into(),
            accept: std::rc::Rc::new(accept),
        });
    }

    /// The configured servers, read off the UI thread by the owner.
    pub(crate) fn set_mcp_servers(
        &mut self,
        servers: Option<Vec<mcp_servers::Server>>,
        cx: &mut Context<Self>,
    ) {
        if let Some(picker) = &mut self.context_picker {
            picker.mcp = servers;
            self.suggestions_revision += 1;
            cx.notify();
        }
    }

    /// The submenu: a way back, then one row per server with its tools.
    fn mcp_rows(&self, query: &str) -> Vec<CommandSuggestion> {
        let servers = self.context_picker.as_ref().and_then(|picker| picker.mcp.as_ref());
        let query = query.trim().to_lowercase();
        let mut rows = vec![row("Back".into(), String::new(), None, Action::McpBack)];
        let listed: Vec<_> = servers
            .into_iter()
            .flatten()
            .filter(|server| server.name.to_lowercase().contains(&query))
            .map(|server| {
                row(
                    server.name.clone(),
                    server.summary.clone(),
                    None,
                    Action::McpServer(server.name.clone()),
                )
            })
            .collect();
        if listed.is_empty() {
            let note = if servers.is_some_and(|servers| !servers.is_empty()) {
                "No MCP server matches"
            } else {
                "No MCP servers configured"
            };
            rows.push(row(note.into(), String::new(), None, Action::McpServer(String::new())));
        }
        rows.extend(listed);
        rows
    }

    /// Open or leave the MCP submenu. False when it was already so.
    pub(super) fn set_mcp_open(&mut self, open: bool, cx: &mut Context<Self>) -> bool {
        let Some(picker) = self.context_picker.as_mut().filter(|p| p.mcp_open != open) else {
            return false;
        };
        picker.mcp_open = open;
        self.suggestions_revision += 1;
        self.set_content(String::new(), cx);
        true
    }

    /// Esc and ← inside the submenu step back to the list.
    pub(super) fn leave_mcp(&mut self, cx: &mut Context<Self>) -> bool {
        self.set_mcp_open(false, cx)
    }

    pub(super) fn standalone_context(&self) -> bool {
        self.context_picker
            .as_ref()
            .is_some_and(|picker| picker.target.is_some())
    }

    pub(super) fn mention_token(&self) -> Option<(Range<usize>, &str)> {
        let picker = self.context_picker.as_ref()?;
        if picker.target.is_some()
            || self.picker_prefix.is_some()
            || !self.sel.range.is_empty()
            || self.sel.marked.is_some()
            || self.queue_editor
            || !self.submission_enabled
        {
            return None;
        }
        mention(&self.content, self.sel.cursor())
    }

    pub(super) fn update_context_index(&mut self, cx: &mut Context<Self>) {
        let open = self.current_command_menu_close().is_none()
            && (self.standalone_context() || self.mention_token().is_some());
        let Some(picker) = &mut self.context_picker else {
            return;
        };
        if std::mem::replace(&mut picker.open, open) || !open {
            return;
        }
        let Some(root) = picker.root.clone() else {
            return;
        };
        if picker.indexing.is_some() {
            return;
        }
        let scan = cx
            .background_executor()
            .spawn(async move { FileIndex::scan(&root) });
        picker.indexing = Some(cx.spawn(async move |input, cx| {
            let index = scan.await;
            let _ = input.update(cx, |input, cx| {
                if let Some(picker) = &mut input.context_picker {
                    picker.index = Some(index);
                    picker.indexing = None;
                    input.suggestions_revision += 1;
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn context_empty_label(&self) -> &'static str {
        match self.context_picker.as_ref() {
            Some(picker) if picker.root.is_none() => "File search needs a local workspace",
            Some(picker) if picker.index.is_none() => "Looking for files…",
            _ => "No matches",
        }
    }

    pub(super) fn context_rows(&self) -> Option<Vec<CommandSuggestion>> {
        let picker = self.context_picker.as_ref()?;
        let (query, files_only) = if picker.target.is_some() {
            let query = self.content.trim_start();
            query
                .strip_prefix('@')
                .map_or((query, false), |query| (query, true))
        } else {
            (self.mention_token()?.1, true)
        };
        let mut rows = Vec::new();
        if !files_only && picker.mcp_open {
            return Some(self.mcp_rows(query));
        }
        if !files_only {
            let query = query.trim_start_matches('/').trim().to_lowercase();
            let mut in_skills = false;
            let (mut skills, mut head): (Vec<_>, Vec<_>) = mode_suggestions(
                &format!("/mode {query}"),
                &self.command_skills,
                &self.current_modes,
                &self.mode_catalog,
            )
            .into_iter()
            .map(|mut row| {
                row.context_action = row
                    .value
                    .strip_prefix("/mode ")
                    .map(|name| Action::Skill(name.to_owned()));
                row
            })
            .partition(|row| {
                if let Some(header) = &row.header {
                    in_skills = header == "Skills";
                }
                in_skills
            });
            let mut tools = Vec::new();
            if "attach images".contains(&query) {
                tools.push(row("Attach…".into(), String::new(), None, Action::Attach));
            }
            if picker.root.is_some() && "files workspace project".contains(&query) {
                tools.push(row("Files".into(), String::new(), None, Action::Browse));
            }
            // Only a machine whose MCP files name a server has anything to open.
            if picker.mcp.as_ref().is_some_and(|servers| !servers.is_empty())
                && "mcp servers".contains(&query)
            {
                tools.push(row("MCP".into(), String::new(), None, Action::Mcp));
            }
            // Groups carry no titles: modes, then tools, then the long skill
            // list, each after a separator.
            for row in head.iter_mut().chain(skills.iter_mut()) {
                row.header = None;
            }
            if let Some(first) = tools.first_mut().filter(|_| !head.is_empty()) {
                first.header = Some(String::new());
            }
            if let Some(first) = skills
                .first_mut()
                .filter(|_| !head.is_empty() || !tools.is_empty())
            {
                first.header = Some(String::new());
            }
            head.extend(tools);
            rows.extend(head);
            rows.extend(skills);
        }
        if (files_only || !query.is_empty())
            && let (Some(root), Some(index)) = (&picker.root, &picker.index)
        {
            rows.extend(
                index
                    .search(query.trim(), 35)
                    .into_iter()
                    .enumerate()
                    .map(|(i, path)| {
                        let label = path.to_string_lossy().into_owned();
                        row(
                            label,
                            String::new(),
                            (i == 0).then_some("Files"),
                            Action::File(root.join(path)),
                        )
                    }),
            );
        }
        Some(rows)
    }

    pub(super) fn apply_context_action(
        &mut self,
        action: Action,
        alternate: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let draft_action = match action {
            Action::Browse => {
                self.set_content("@".into(), cx);
                return;
            }
            Action::Mcp => {
                self.set_mcp_open(true, cx);
                return;
            }
            Action::McpBack => {
                self.set_mcp_open(false, cx);
                return;
            }
            Action::McpServer(_) => return,
            Action::Skill(name) if !alternate || name == "off" => {
                self.toggle_mode_row(&name, window, cx);
                None
            }
            action => Some(action),
        };
        if let Some(target) = self
            .context_picker
            .as_ref()
            .and_then(|picker| picker.target.clone())
        {
            (target.accept)(cx);
            if let Some(draft) = target
                .draft
                .upgrade()
                .and_then(|draft| draft.downcast::<Self>().ok())
            {
                draft.update(cx, |input, cx| {
                    window.focus(&input.focus_handle, cx);
                    if let Some(action) = draft_action {
                        input.apply_to_draft(action, alternate, false, window, cx);
                    }
                });
            }
        } else if let Some(action) = draft_action {
            self.apply_to_draft(action, alternate, true, window, cx);
        }
    }

    fn apply_to_draft(
        &mut self,
        action: Action,
        alternate: bool,
        from_mention: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            Action::Attach => self.prompt_for_attachments(window, cx),
            Action::Skill(name) => self.insert_context(DraftContext::Skill(name), None, window, cx),
            Action::File(path) => {
                let range = from_mention
                    .then(|| self.mention_token().map(|(range, _)| range))
                    .flatten();
                let contents = if alternate {
                    match read_file_context(&path) {
                        Ok(contents) => Some(contents),
                        Err(err) => {
                            self.set_transient_notice(
                                format!("Cannot include file: {err}").into(),
                                cx,
                            );
                            return;
                        }
                    }
                } else {
                    None
                };
                self.insert_context(DraftContext::File { path, contents }, range, window, cx);
            }
            Action::Browse | Action::Mcp | Action::McpBack | Action::McpServer(_) => {
                unreachable!("these stay in the picker")
            }
        }
    }

    fn insert_context(
        &mut self,
        context: DraftContext,
        range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = match &context {
            DraftContext::Skill(name) => format!("Skill: {name}"),
            DraftContext::File { path, contents } => {
                let name = path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy();
                if contents.is_some() {
                    format!("{name} (included)")
                } else {
                    name.into_owned()
                }
            }
        }
        .replace(['[', ']', '\n', '\r'], " ");
        let mut marker = format!("[#{name}]");
        let mut ordinal = 2;
        while self.content.contains(&marker) {
            marker = format!("[#{name} #{ordinal}]");
            ordinal += 1;
        }
        let replacing_skill = if matches!(context, DraftContext::Skill(_)) {
            self.attachments.iter().find_map(|attachment| {
                matches!(
                    attachment.payload,
                    AttachmentPayload::Context(DraftContext::Skill(_))
                )
                .then(|| self.chip_range(attachment))
                .flatten()
            })
        } else {
            None
        };
        let range = range
            .or(replacing_skill)
            .unwrap_or_else(|| self.sel.range.clone());
        let prefix = if self.content[..range.start].ends_with(|c: char| !c.is_whitespace()) {
            " "
        } else {
            ""
        };
        self.replace_text_in_range(
            Some(range_to_utf16(&self.content, &range)),
            &format!("{prefix}{marker} "),
            window,
            cx,
        );
        self.attachments.push(Attachment {
            label: marker.into(),
            payload: AttachmentPayload::Context(context),
        });
        self.dismiss_ghost();
        cx.notify();
    }

    fn prompt_for_attachments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let response = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach to prompt".into()),
        });
        cx.spawn_in(window, async move |input, cx| {
            let Ok(Ok(Some(paths))) = response.await else {
                return;
            };
            let _ = cx.update(|window, cx| {
                let _ = input.update(cx, |editor, cx| editor.drop_files(&paths, window, cx));
            });
        })
        .detach();
    }
}

fn read_file_context(path: &Path) -> std::io::Result<String> {
    use std::io::{Error, ErrorKind, Read};
    const MAX: u64 = 64 * 1024;
    if !path.metadata()?.is_file() {
        return Err(Error::new(ErrorKind::InvalidInput, "not a regular file"));
    }
    let file = std::fs::File::open(path)?;
    let mut contents = String::new();
    file.take(MAX + 1).read_to_string(&mut contents)?;
    if contents.len() > MAX as usize || contents.contains('\0') {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "choose a UTF-8 text file no larger than 64 KiB",
        ));
    }
    Ok(contents)
}

impl PromptInput {
    pub(super) fn cursor_at_end(&self) -> bool {
        self.sel.range.is_empty() && self.sel.cursor() == self.content.len()
    }

    /// Suggested remainder to paint after the caret.
    pub(crate) fn ghost_text(&self) -> Option<&str> {
        if self.sel.marked.is_some() || self.content.trim_start().starts_with('/') {
            return None;
        }
        self.ghost
            .as_ref()?
            .machine
            .visible(&self.content, self.cursor_at_end())
    }

    /// The draft changed: drop stale work and schedule a debounced request.
    pub(super) fn ghost_on_edit(&mut self, cx: &mut Context<Self>) {
        if self.standalone_context() || self.mention_token().is_some() {
            self.dismiss_ghost();
            return;
        }
        let at_end = self.cursor_at_end();
        let draft = self.content.to_string();
        let working_dir = self.working_dir.clone();
        let Some(ghost) = self.ghost.as_mut() else {
            return;
        };
        ghost.task = None;
        let Some(generation) = ghost.machine.on_edit(&draft, at_end) else {
            return;
        };
        if ghost.disabled {
            return;
        }
        // GPUI's deterministic scheduler cannot be woken by Tokio's real
        // network thread. Tests explicitly opt into a fake request counter.
        #[cfg(test)]
        if ghost.request_counter.is_none() {
            return;
        }
        let context = (ghost.context)(cx);
        let settings = crate::config::completions();
        let debounce = Duration::from_millis(settings.debounce_ms.max(400));
        let model = settings.model.clone();
        #[cfg(test)]
        let request_counter = ghost.request_counter.clone();
        let project = working_dir.as_deref().and_then(|dir| {
            std::path::Path::new(dir)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        });
        let request = crate::prompt_completion::Request {
            draft,
            context,
            cwd: working_dir,
            project,
            model,
        };
        let cache_key = serde_json::to_string(&request).unwrap_or_default();
        if let Some((_, completion)) = ghost.cache.iter().find(|(key, _)| key == &cache_key) {
            if ghost.machine.on_response(generation, completion) {
                cx.notify();
            }
            return;
        }
        ghost.task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(debounce).await;
            #[cfg(test)]
            if let Some(counter) = request_counter {
                counter.set(counter.get() + 1);
                return;
            }
            let Some(key) = jcode_base::subscription_catalog::configured_api_key()
                .map(|key| key.trim().to_owned())
                .filter(|key| !key.is_empty())
            else {
                return;
            };
            let base = jcode_base::subscription_api::configured_api_base();
            let reserved = cx
                .background_executor()
                .spawn(async { crate::prompt_completion::reserve_daily_request() })
                .await;
            if !reserved {
                return;
            }
            let result = match crate::prompt_completion::fetch(base, key, request) {
                Ok(mut task) => task.result().await,
                Err(error) => Err(error),
            };
            let _ = this.update(cx, |input, cx| {
                let Some(ghost) = input.ghost.as_mut() else {
                    return;
                };
                match result {
                    Ok(completion) => {
                        if ghost.machine.is_current(generation) {
                            if ghost.cache.len() >= 32 {
                                ghost.cache.pop_front();
                            }
                            ghost.cache.push_back((cache_key, completion.clone()));
                        }
                        if ghost.machine.on_response(generation, &completion) {
                            cx.notify();
                        }
                    }
                    Err(crate::prompt_completion::FetchError::Unsupported) => {
                        ghost.disabled = true;
                    }
                    Err(crate::prompt_completion::FetchError::Other(error)) => {
                        eprintln!("jcode desktop: prompt completion failed: {error}");
                    }
                }
            });
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_tokens_do_not_capture_email_or_draft_text() {
        assert!(mention("mail me@example.org", 19).is_none());
        assert_eq!(mention("prépare @résumé.md now", 15), Some((9..21, "résu")));
        assert_eq!(mention("look\u{2003}@", 8), Some((7..8, "")));
    }

    #[gpui::test]
    fn context_mention_inserts_one_file_chip_and_includes_contents_only_on_request(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::input::bind_keys);
        let directory = tempfile::tempdir().unwrap();
        let report = directory.path().join("build report.md");
        std::fs::write(&report, "# The report body").unwrap();
        let sent = std::rc::Rc::new(RefCell::new(Vec::new()));
        let sink = sent.clone();
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let mut input = PromptInput::new(cx, "test", move |text, _, _, _| {
                sink.borrow_mut().push(text)
            });
            input.enable_context_picker();
            input.set_context_root(Some(directory.path()), cx);
            window.focus(&input.focus_handle, cx);
            input
        });
        for (key, included) in [("enter", false), ("alt-enter", true)] {
            vcx.simulate_input("Read @report");
            vcx.run_until_parked();
            assert_eq!(
                input.read_with(vcx, |input, _| input.command_suggestion_values()),
                ["build report.md"]
            );
            vcx.simulate_keystrokes(key);
            vcx.run_until_parked();
            input.update(vcx, |input, cx| {
                assert!(input.content.starts_with("Read [#build report.md"));
                assert!(!input.content.contains("@report"));
                assert_eq!(input.attachments.len(), 1);
                let snapshot = input.snapshot();
                input.restore(snapshot, cx);
            });
            assert!(vcx.debug_bounds("composer-image-chip").is_some());
            vcx.simulate_keystrokes("enter");
            let sent = sent.borrow();
            let prompt = sent.last().unwrap();
            assert!(prompt.contains(report.to_str().unwrap()));
            assert_eq!(prompt.contains("# The report body"), included);
        }
    }

    #[gpui::test]
    fn context_paste_converts_plain_image_path_lists_but_preserves_prose(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::input::bind_keys);
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first photo.png");
        let second = directory.path().join("second.png");
        let bytes = include_bytes!("../../../../../assets/previews/image-preview.png");
        for path in [&first, &second] {
            std::fs::write(path, bytes).unwrap();
        }
        let list = format!("\"{}\" {}", first.display(), second.display());
        let sent = std::rc::Rc::new(RefCell::new(None));
        let sink = sent.clone();
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let input = PromptInput::new(cx, "test", move |text, images, _, _| {
                *sink.borrow_mut() = Some((text, images))
            });
            window.focus(&input.focus_handle, cx);
            input
        });
        input.update_in(vcx, |input, window, cx| {
            let prose = format!("Please read {list}");
            input.paste_text_or_image_paths(&prose, window, cx);
            assert_eq!(input.content.as_ref(), prose);
            assert!(input.attachments.is_empty());
            input.set_content(String::new(), cx);
            input.paste_text_or_image_paths(&list, window, cx);
        });
        vcx.simulate_keystrokes("enter");
        let sent = sent.borrow();
        let (text, images) = sent.as_ref().unwrap();
        assert_eq!(text.trim(), "[#image 1][#image 2]");
        assert_eq!(images.len(), 2);
    }
}
