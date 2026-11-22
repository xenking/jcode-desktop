//! On-disk tab checkpoints, separate from in-process hot reload snapshots.
//! Only a workspace which restored the dead owner's state can replace it.
use super::{Workspace, WorkspaceSnapshot};
use gpui::{App, Entity, Window};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    fs::{self, File, OpenOptions},
    io::{self, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(1);
const MAX_CHECKPOINT_RETRY: Duration = Duration::from_secs(60);

fn checkpoint_retry_after(previous: Option<Duration>) -> Duration {
    previous.map_or(Duration::from_secs(5), |delay| {
        (delay * 2).min(MAX_CHECKPOINT_RETRY)
    })
}

#[derive(Deserialize, Serialize)]
struct Checkpoint {
    owner: String,
    pid: u32,
    updated_at_ms: u64,
    /// Written by a normal quit. The open panels then reopen on the next
    /// launch no matter how much later, like a browser restoring its tabs.
    #[serde(default)]
    clean: bool,
    snapshot: WorkspaceSnapshot,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn filename(mode: jcode_desktop_api::LaunchMode) -> &'static str {
    // Keep the primary single-panel window separate from spatial workspaces.
    match mode {
        jcode_desktop_api::LaunchMode::SinglePanel => "crash-recovery-single-panel.json",
        jcode_desktop_api::LaunchMode::NoSidebar => "crash-recovery-no-sidebar.json",
        jcode_desktop_api::LaunchMode::Workspace => "crash-recovery.json",
    }
}

pub(crate) struct Recovery {
    path: PathBuf,
    normally_enabled: bool,
}

pub(crate) struct Restored {
    pub(crate) snapshot: WorkspaceSnapshot,
    /// An unclean generation is replaced only after applying its snapshot.
    pub(crate) crash_owner: Option<String>,
}

#[derive(Clone, Copy)]
pub(crate) enum Claim<'a> {
    Fresh,
    Restored(&'a str),
    Reload,
    /// Launched from a restart/rollout restore manifest. The manifest is the
    /// replaced process's final state, so its unclean checkpoint is
    /// superseded once that process has exited.
    Handoff,
}

impl Recovery {
    fn new(state_dir: &Path, launch: &jcode_desktop_api::WindowLaunch) -> Self {
        use jcode_desktop_api::LaunchMode;
        let mode = LaunchMode::from_args(&launch.args);
        Self {
            path: state_dir.join(filename(mode)),
            normally_enabled: mode != LaunchMode::SinglePanel
                || (!LaunchMode::resume_requested(&launch.args)
                    && LaunchMode::requested_session(&launch.args).is_none()),
        }
    }

    pub(crate) fn for_window(window: &Window, app: &App) -> Option<Self> {
        use jcode_desktop_api::WindowLaunches;
        let id = window.window_handle().window_id().as_u64();
        if WindowLaunches::is_forwarded(app, id) {
            return None;
        }
        let state = crate::learning::state_path()?;
        Some(Self::new(state.parent()?, &WindowLaunches::get(app, id)))
    }
}

fn process_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return true;
    }
    #[cfg(unix)]
    {
        // EPERM is also a live process. Be conservative if process inspection fails.
        (unsafe { libc::kill(pid as i32, 0) == 0 })
            || io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
    #[cfg(not(unix))]
    {
        // Do not steal another independent Windows instance's checkpoint.
        true
    }
}

fn private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

// All generations use this same short-lived lock, including across cdylibs.
// Never remove the lock inode, which could let two writers hold different locks.
fn lock(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path.with_extension("lock"))?;
    file.lock()?;
    Ok(file)
}

fn read(path: &Path) -> Option<Checkpoint> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

// Ownership checks run every second. Do not allocate all the side-panel
// documents again just to compare the generation token.
#[derive(Deserialize)]
struct CheckpointOwner {
    owner: String,
    pid: u32,
    #[serde(default)]
    clean: bool,
}

fn read_owner(path: &Path) -> Option<CheckpointOwner> {
    serde_json::from_reader(BufReader::new(File::open(path).ok()?)).ok()
}

fn write(path: &Path, checkpoint: &Checkpoint) -> anyhow::Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = BufWriter::new(private_file(&temporary)?);
        serde_json::to_writer(&mut file, checkpoint)?;
        file.flush()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

impl Recovery {
    pub(crate) fn load(&self) -> Option<Restored> {
        let _lock = lock(&self.path).ok()?;
        let checkpoint = read(&self.path)?;
        if checkpoint.clean && !self.normally_enabled {
            return None;
        }
        let snapshot = restore(
            &checkpoint,
            !checkpoint.clean && process_alive(checkpoint.pid),
        )?;
        eprintln!(
            "restoring {} desktop panels from {} ({} ms since checkpoint)",
            snapshot.slots.len(),
            if checkpoint.clean {
                "last session"
            } else {
                "crash"
            },
            now_ms().saturating_sub(checkpoint.updated_at_ms),
        );
        Some(Restored {
            snapshot,
            crash_owner: (!checkpoint.clean).then_some(checkpoint.owner),
        })
    }
}

fn restore(checkpoint: &Checkpoint, alive: bool) -> Option<WorkspaceSnapshot> {
    if alive {
        return None;
    }
    // Validate the workspace schema as strictly as the hot-reload boundary.
    let mut snapshot = WorkspaceSnapshot::decode(&checkpoint.snapshot.encode().ok()?).ok()?;
    if !checkpoint.clean {
        snapshot.resume = super::resume::Snapshot::Closed;
    }
    for slot in &mut snapshot.slots {
        // PTY resource handles and output cursors belong to the dead host.
        // Reopen in its directory without suppressing the new PTY's replies.
        if slot.panel.terminal_resource_id.take().is_some() {
            slot.panel.session_id = "terminal".into();
        }
        slot.panel.terminal_output_cursor = None;
    }
    Some(snapshot)
}

#[derive(Clone)]
struct Writer {
    path: PathBuf,
    owner: String,
}

struct SuspendedSnapshot {
    window_id: gpui::WindowId,
    snapshot: Rc<RefCell<Option<WorkspaceSnapshot>>>,
    writer: Writer,
    workspace: gpui::WeakEntity<Workspace>,
}

impl gpui::Global for SuspendedSnapshot {}

/// Capture the last edits before a shared single-panel window is destroyed.
pub(crate) fn clean_window(window: &Window, app: &App) {
    let Some(state) = app.try_global::<SuspendedSnapshot>() else {
        return;
    };
    if state.window_id != window.window_handle().window_id() {
        return;
    }
    let snapshot = state
        .workspace
        .upgrade()
        .and_then(|workspace| workspace.read(app).snapshot(window, app).ok());
    if let Err(error) = state.writer.clean_quit(snapshot) {
        eprintln!("failed to save closing desktop workspace: {error:#}");
    }
}

pub(super) fn remember_reload_snapshot(snapshot: &WorkspaceSnapshot, window: &Window, app: &App) {
    if let Some(suspended) = app.try_global::<SuspendedSnapshot>()
        && suspended.window_id == window.window_handle().window_id()
    {
        *suspended.snapshot.borrow_mut() = Some(snapshot.clone());
    }
}

impl Writer {
    fn begin(path: PathBuf, snapshot: WorkspaceSnapshot, claim: Claim<'_>) -> anyhow::Result<Self> {
        let _lock = lock(&path)?;
        let now = now_ms();
        let previous = read_owner(&path);
        let can_claim = match claim {
            Claim::Fresh => previous.is_none_or(|previous| previous.clean),
            Claim::Restored(owner) => previous.is_some_and(|previous| {
                !previous.clean && previous.owner == owner && !process_alive(previous.pid)
            }),
            Claim::Reload => {
                previous.is_none_or(|previous| previous.clean || previous.pid == std::process::id())
            }
            Claim::Handoff => {
                previous.is_none_or(|previous| previous.clean || !process_alive(previous.pid))
            }
        };
        anyhow::ensure!(
            can_claim,
            "desktop checkpoint ownership changed or was not restored"
        );
        let owner = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        let writer = Self { path, owner };
        write(&writer.path, &writer.checkpoint(snapshot, now))?;
        Ok(writer)
    }

    fn checkpoint(&self, snapshot: WorkspaceSnapshot, updated_at_ms: u64) -> Checkpoint {
        Checkpoint {
            owner: self.owner.clone(),
            pid: std::process::id(),
            updated_at_ms,
            clean: false,
            snapshot,
        }
    }

    fn save(&self, snapshot: WorkspaceSnapshot, updated_at_ms: u64) -> anyhow::Result<bool> {
        let _lock = lock(&self.path)?;
        if read_owner(&self.path).is_none_or(|record| record.owner != self.owner) {
            return Ok(false);
        }
        write(&self.path, &self.checkpoint(snapshot, updated_at_ms))?;
        Ok(true)
    }

    fn clean_quit(&self, snapshot: Option<WorkspaceSnapshot>) -> anyhow::Result<()> {
        let _lock = lock(&self.path)?;
        if read_owner(&self.path).is_some_and(|record| record.owner == self.owner)
            && let Some(mut record) = read(&self.path)
        {
            // Retire the owner atomically with the last snapshot so an in-flight
            // background save cannot overwrite the final selections or draft.
            if let Some(snapshot) = snapshot {
                record.snapshot = snapshot;
                record.updated_at_ms = now_ms();
            }
            record.clean = true;
            record.owner = format!("{}-quit", self.owner);
            write(&self.path, &record)?;
        }
        Ok(())
    }
}

impl Recovery {
    pub(crate) fn install(
        self,
        workspace: &Entity<Workspace>,
        window: &Window,
        app: &mut App,
        claim: Claim<'_>,
    ) {
        let enabled = self.normally_enabled
            || match claim {
                Claim::Fresh => false,
                Claim::Restored(_) | Claim::Handoff => true,
                Claim::Reload => read_owner(&self.path)
                    .is_some_and(|owner| !owner.clean && owner.pid == std::process::id()),
            };
        if enabled {
            install_at_path(workspace, window, app, self.path, claim);
        }
    }
}

fn crash_snapshot(
    workspace: &Workspace,
    window: &Window,
    app: &App,
) -> anyhow::Result<WorkspaceSnapshot> {
    let mut snapshot = workspace.snapshot(window, app)?;
    // Picker edits and keyboard focus are transient. A crash returns to the
    // saved active panel, while a clean quit saves the full final UI state.
    snapshot.resume = super::resume::Snapshot::Closed;
    snapshot.focus = super::FocusSnapshot::Workspace;
    settle_motion(&mut snapshot);
    Ok(snapshot)
}

/// A crash restores where motion was heading, not a frame of it: the camera
/// lands on its target, and a panel stuck to the bottom ignores its offset on
/// restore. Recording mid-animation frames or streaming scroll offsets would
/// rewrite the checkpoint every tick without changing what a crash restores.
fn settle_motion(snapshot: &mut WorkspaceSnapshot) {
    snapshot.camera_x = snapshot.camera_target;
    for slot in &mut snapshot.slots {
        if slot.panel.stick_to_bottom {
            slot.panel.scroll_x = 0.0;
            slot.panel.scroll_y = 0.0;
        }
    }
}

/// How long a restore-manifest successor waits for the replaced process to
/// exit before giving up on crash recovery for this run.
const HANDOFF_WAIT: Duration = Duration::from_secs(120);

/// The replaced process still holds an unclean checkpoint.
fn handoff_pending(path: &Path) -> bool {
    read_owner(path).is_some_and(|owner| !owner.clean && process_alive(owner.pid))
}

/// Rollout and `/restart-all` start the successor before the replaced process
/// has exited, and SIGTERM leaves its checkpoint unclean. Claim it as soon as
/// that process is gone, so the new window is not left without crash recovery.
fn defer_handoff(workspace: &Entity<Workspace>, window: &Window, app: &mut App, path: PathBuf) {
    let workspace = workspace.downgrade();
    let window = window.window_handle();
    app.spawn(async move |cx| {
        let executor = cx.background_executor().clone();
        let deadline = executor.now() + HANDOFF_WAIT;
        while handoff_pending(&path) {
            if executor.now() >= deadline {
                eprintln!("desktop crash checkpoint unavailable: replaced process never exited");
                return;
            }
            executor.timer(Duration::from_secs(1)).await;
        }
        let _ = window.update(cx, |_, window, app| {
            if let Some(workspace) = workspace.upgrade() {
                install_at_path(&workspace, window, app, path, Claim::Handoff);
            }
        });
    })
    .detach();
}

fn install_at_path(
    workspace: &Entity<Workspace>,
    window: &Window,
    app: &mut App,
    path: PathBuf,
    claim: Claim<'_>,
) {
    let writer = crash_snapshot(workspace.read(app), window, app).and_then(|snapshot| {
        let key = serde_json::to_value(&snapshot)?;
        Ok((Writer::begin(path.clone(), snapshot, claim)?, key))
    });
    let (writer, initial_key) = match writer {
        Ok(writer_and_key) => writer_and_key,
        Err(error) => {
            if matches!(claim, Claim::Handoff) && handoff_pending(&path) {
                defer_handoff(workspace, window, app, path);
                return;
            }
            eprintln!("desktop crash checkpoint unavailable: {error:#}");
            return;
        }
    };
    let quit_writer = writer.clone();
    let quit_workspace = workspace.downgrade();
    let quit_window = window.window_handle();
    if let Some(previous) = app.try_global::<SuspendedSnapshot>() {
        previous.snapshot.borrow_mut().take();
    }
    let suspended_snapshot = Rc::new(RefCell::new(None));
    app.set_global(SuspendedSnapshot {
        window_id: quit_window.window_id(),
        snapshot: suspended_snapshot.clone(),
        writer: writer.clone(),
        workspace: workspace.downgrade(),
    });
    if jcode_desktop_api::WindowLaunches::shared(app) {
        // Unlike the spatial host, this host does not install a suspend
        // handler. Native close destroys the root before app quit can read it.
        window.on_window_should_close(app, |window, app| {
            clean_window(window, app);
            true
        });
    }
    app.on_app_quit(move |app| {
        let snapshot = quit_window
            .update(app, |_, window, app| {
                quit_workspace
                    .upgrade()
                    .map(|workspace| workspace.read(app).snapshot(window, app))
            })
            .ok()
            .flatten()
            .and_then(Result::ok)
            .or_else(|| suspended_snapshot.borrow_mut().take());
        if let Err(error) = quit_writer.clean_quit(snapshot) {
            eprintln!("failed to disarm desktop crash recovery: {error:#}");
        }
        std::future::ready(())
    })
    .detach();
    let workspace = workspace.downgrade();
    let window = window.window_handle();
    app.spawn(async move |cx| {
        let executor = cx.background_executor().clone();
        let mut last = (executor.now(), initial_key);
        let mut retry = None;
        loop {
            executor.timer(retry.unwrap_or(CHECKPOINT_INTERVAL)).await;
            let snapshot = window.update(cx, |_, window, app| {
                workspace
                    .upgrade()
                    .map(|workspace| crash_snapshot(workspace.read(app), window, app))
            });
            let Ok(Some(Ok(snapshot))) = snapshot else {
                break;
            };
            let captured_at = now_ms();
            let sample_time = executor.now();
            let writer = writer.clone();
            let (updated, result) = cx
                .background_executor()
                .spawn(async move {
                    let result = (|| {
                        let key = serde_json::to_value(&snapshot)?;
                        if super::housekeeping::should_checkpoint(
                            sample_time.duration_since(last.0),
                            key != last.1,
                        ) {
                            if !writer.save(snapshot, captured_at)? {
                                return Ok(false);
                            }
                            last = (sample_time, key);
                        }
                        anyhow::Ok(true)
                    })();
                    (last, result)
                })
                .await;
            last = updated;
            match result {
                Ok(true) => retry = None,
                Ok(false) => break,
                Err(error) => {
                    let delay = checkpoint_retry_after(retry);
                    if retry.is_none() || retry == Some(MAX_CHECKPOINT_RETRY) {
                        eprintln!(
                            "desktop crash checkpoint failed: {error:#}; retry in {}s",
                            delay.as_secs()
                        );
                    }
                    retry = Some(delay);
                }
            }
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::AppContext;

    #[test]
    fn failed_checkpoints_back_off_and_stop_increasing_at_one_minute() {
        let mut retry = None;
        for seconds in [5, 10, 20, 40, 60, 60] {
            let next = checkpoint_retry_after(retry);
            assert_eq!(next, Duration::from_secs(seconds));
            retry = Some(next);
        }
    }

    fn relaunch_restores_tabs_and_pending_ask(cx: &mut gpui::TestAppContext, single: bool) {
        use jcode_desktop_api::LaunchMode;
        cx.update(crate::bind_workspace_keys);
        let mode = if single {
            LaunchMode::SinglePanel
        } else {
            LaunchMode::Workspace
        };
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(filename(mode));
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.single_panel = single;
            workspace.push_test_panel("session-a", cx);
            workspace.push_test_panel("session-b", cx);
            workspace.active = 1;
            workspace.row_focus[0] = Some(workspace.slots[1].panel.entity_id());
            workspace
        });
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(1))
            .unwrap();
        panel.update(vcx, |panel, cx| panel.test_settled_conversation(1, cx));
        let event: jcode_sdk::ApiEvent = serde_json::from_str(
            r#"{
            "ev":"ask_request", "session_id":"session-b", "request_id":"ask-1",
            "tool_call_id":"tool-1", "questions":[{"header":"Database",
            "question":"Which database?", "options":[{"label":"Postgres"},{"label":"SQLite"}]}]
        }"#,
        )
        .unwrap();
        panel.update(vcx, |panel, cx| panel.apply(&event, cx));
        vcx.run_until_parked();
        vcx.update(|window, cx| {
            let focus = panel.read(cx).focus_handle.clone();
            window.focus(&focus, cx);
        });
        vcx.simulate_keystrokes("2 n");
        vcx.simulate_input("important note");
        vcx.run_until_parked();
        let snapshot = vcx.update(|window, cx| workspace.read(cx).snapshot(window, cx).unwrap());
        let draft = serde_json::to_value(&snapshot.slots[1].panel).unwrap();
        assert_eq!(draft["ask_draft"]["chosen"][0], serde_json::json!([1]));
        assert_eq!(draft["ask_draft"]["comments"][0][1], "important note");
        let writer = Writer::begin(path.clone(), snapshot.clone(), Claim::Fresh).unwrap();
        writer.clean_quit(Some(snapshot)).unwrap();
        let recovered = restore(&read(&path).unwrap(), false).unwrap();
        let (bridge, commands) = crate::harness::spawn_recording();
        let workspace = vcx.update(|_, cx| {
            cx.new(|cx| {
                let mut workspace = Workspace::for_test(crate::learning::Coach::new(), cx);
                workspace.single_panel = single;
                workspace
            })
        });
        workspace.update(vcx, |workspace, cx| {
            workspace.bridge = bridge;
            workspace.apply_snapshot(recovered, cx);
            assert_eq!(workspace.slots.len(), 2);
            assert_eq!(workspace.active, 1);
            assert_eq!(workspace.slots[0].panel.read(cx).session_id, "session-a");
            assert_eq!(workspace.slots[1].panel.read(cx).session_id, "session-b");
        });
        let watched: Vec<_> = std::iter::from_fn(|| commands.try_recv().ok()).collect();
        assert!(watched.iter().any(|cmd| matches!(cmd, crate::harness::Command::Watch { session_id } if session_id == "session-b")));
        let restored = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(1))
            .unwrap();
        workspace.update(vcx, |workspace, cx| {
            workspace.apply(
                crate::harness::Update::Event {
                    session_id: "session-b".into(),
                    event,
                },
                cx,
            );
        });
        let restored_draft = restored.read_with(vcx, |panel, cx| {
            serde_json::to_value(panel.snapshot(cx)).unwrap()
        });
        assert_eq!(
            restored_draft["ask_draft"]["chosen"][0],
            serde_json::json!([1])
        );
        assert_eq!(
            restored_draft["ask_draft"]["comments"][0][1],
            "important note"
        );
    }

    #[gpui::test]
    fn relaunch_restores_single_panel_tabs_and_ask(cx: &mut gpui::TestAppContext) {
        relaunch_restores_tabs_and_pending_ask(cx, true);
    }

    #[gpui::test]
    fn relaunch_restores_workspace_tabs_and_ask(cx: &mut gpui::TestAppContext) {
        relaunch_restores_tabs_and_pending_ask(cx, false);
    }

    #[gpui::test]
    fn single_panel_native_close_saves_latest_state(cx: &mut gpui::TestAppContext) {
        cx.update(jcode_desktop_api::WindowLaunches::install);
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("single.json");
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.single_panel = true;
            workspace.push_test_panel("first", cx);
            workspace
        });
        vcx.update(|window, app| {
            install_at_path(&workspace, window, app, path.clone(), Claim::Fresh)
        });
        workspace.update(vcx, |workspace, cx| {
            workspace.push_test_panel("second", cx);
            workspace.active = 1;
            workspace.row_focus[0] = Some(workspace.slots[1].panel.entity_id());
        });
        assert!(vcx.simulate_close());
        let record = read(&path).unwrap();
        assert!(record.clean);
        assert_eq!(record.snapshot.slots.len(), 2);
        assert_eq!(record.snapshot.active, 1);
        assert_eq!(record.snapshot.slots[1].panel.session_id, "second");
    }

    fn snapshot() -> WorkspaceSnapshot {
        WorkspaceSnapshot::decode(br#"{"format_version":1,"slots":[],"active":0,"active_row":0,"row_focus":[null,null,null,null],"previous":null,"camera_x":[0,0,0,0],"camera_target":[0,0,0,0],"overview":false,"hints_overlay":false,"folder_picker_dir":null,"folder_picker_error":null,"folder_search":null,"focus":"Workspace"}"#).unwrap()
    }

    #[gpui::test]
    fn clean_shutdown_captures_ask_selection_after_last_checkpoint(cx: &mut gpui::TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery.json");
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        vcx.update(|window, app| {
            install_at_path(&workspace, window, app, path.clone(), Claim::Fresh)
        });
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        panel.update(vcx, |panel, cx| panel.test_settled_conversation(1, cx));
        let event: jcode_sdk::ApiEvent = serde_json::from_str(
            r#"{
            "ev":"ask_request", "session_id":"session-a", "request_id":"ask-1",
            "tool_call_id":"tool-1", "questions":[{"header":"Database",
            "question":"Which database?", "options":[{"label":"Postgres"},{"label":"SQLite"}]}]
        }"#,
        )
        .unwrap();
        panel.update(vcx, |panel, cx| panel.apply(&event, cx));
        vcx.run_until_parked();
        for _ in 0..120 {
            if vcx.debug_bounds("ask-option-0-1").is_some() {
                break;
            }
            vcx.executor().advance_clock(Duration::from_millis(16));
            vcx.update(|window, cx| window.simulate_next_frame(cx));
            vcx.run_until_parked();
        }
        let option = vcx.debug_bounds("ask-option-0-1").unwrap();
        vcx.simulate_click(option.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        let current = panel.read_with(vcx, |panel, cx| {
            serde_json::to_value(panel.snapshot(cx)).unwrap()
        });
        assert_eq!(current["ask_draft"]["chosen"][0], serde_json::json!([1]));
        cx.quit();
        let record = read(&path).expect("clean shutdown leaves a checkpoint");
        assert!(record.clean);
        let restored = restore(&record, false).unwrap();
        let saved = serde_json::to_value(&restored.slots[0].panel).unwrap();
        assert_eq!(saved["ask_draft"]["chosen"][0], serde_json::json!([1]));
    }

    #[gpui::test]
    fn suspended_window_keeps_ask_selection_on_later_quit(cx: &mut gpui::TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery.json");
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        vcx.update(|window, app| {
            install_at_path(&workspace, window, app, path.clone(), Claim::Fresh)
        });
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        panel.update(vcx, |panel, cx| panel.test_settled_conversation(1, cx));
        let event: jcode_sdk::ApiEvent = serde_json::from_str(
            r#"{
            "ev":"ask_request", "session_id":"session-a", "request_id":"ask-1",
            "tool_call_id":"tool-1", "questions":[{"header":"Database",
            "question":"Which database?", "options":[{"label":"Postgres"},{"label":"SQLite"}]}]
        }"#,
        )
        .unwrap();
        panel.update(vcx, |panel, cx| panel.apply(&event, cx));
        vcx.run_until_parked();
        for _ in 0..120 {
            if vcx.debug_bounds("ask-option-0-1").is_some() {
                break;
            }
            vcx.executor().advance_clock(Duration::from_millis(16));
            vcx.update(|window, cx| window.simulate_next_frame(cx));
            vcx.run_until_parked();
        }
        let option = vcx.debug_bounds("ask-option-0-1").unwrap();
        vcx.simulate_click(option.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        vcx.update(|window, app| {
            let snapshot = workspace
                .read(app)
                .snapshot_for_reload(window, app)
                .unwrap();
            assert_eq!(
                serde_json::to_value(&snapshot.slots[0].panel).unwrap()["ask_draft"]["chosen"][0],
                serde_json::json!([1])
            );
            window.remove_window();
        });
        cx.quit();
        let record = read(&path).unwrap();
        let restored = restore(&record, false).unwrap();
        assert_eq!(
            serde_json::to_value(&restored.slots[0].panel).unwrap()["ask_draft"]["chosen"][0],
            serde_json::json!([1])
        );
    }

    #[test]
    fn ownership_checks_skip_document_payloads() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery.json");
        // Ownership does not need the workspace schema. In particular it must
        // not build a second copy of every saved document to read two fields.
        let mut file = BufWriter::new(File::create(&path).unwrap());
        write!(
            file,
            "{{\"owner\":\"generation\",\"pid\":42,\"updated_at_ms\":100,\"snapshot\":{{\"large_document\":\""
        )
        .unwrap();
        for _ in 0..1024 {
            file.write_all(&[b'x'; 8192]).unwrap();
        }
        file.write_all(b"\"}}").unwrap();
        file.flush().unwrap();
        let owner = read_owner(&path).unwrap();
        assert_eq!(owner.owner, "generation");
        assert_eq!(owner.pid, 42);
        assert!(
            read(&path).is_none(),
            "fixture deliberately is not a workspace"
        );
        fs::write(&path, b"{\"owner\":\"partial\",\"pid\":42,").unwrap();
        assert!(read_owner(&path).is_none(), "reject truncated checkpoints");
    }

    #[test]
    fn explicit_single_panel_launch_preserves_clean_checkpoint_for_normal_launch() {
        use jcode_desktop_api::WindowLaunch;
        let root = tempfile::tempdir().unwrap();
        let recovery = |args: &[&str]| {
            Recovery::new(
                root.path(),
                &WindowLaunch::from_parts(args.iter().copied(), []),
            )
        };
        let normal = recovery(&["--single-panel"]);
        assert!(normal.path.ends_with("crash-recovery-single-panel.json"));
        Writer::begin(normal.path.clone(), snapshot(), Claim::Fresh)
            .unwrap()
            .clean_quit(None)
            .unwrap();
        assert!(normal.load().is_some());
        assert!(
            recovery(&["--single-panel", "--session=other"])
                .load()
                .is_none()
        );
        assert!(recovery(&["--resume"]).load().is_none());
        assert!(recovery(&[]).load().is_none());
    }

    #[test]
    fn live_owner_is_not_restored_or_displaced_by_a_fresh_launch() {
        let root = tempfile::tempdir().unwrap();
        let recovery = Recovery::new(root.path(), &jcode_desktop_api::WindowLaunch::default());
        let writer = Writer::begin(recovery.path.clone(), snapshot(), Claim::Fresh).unwrap();
        assert!(recovery.load().is_none());
        assert!(
            Writer::begin(
                recovery.path.clone(),
                snapshot(),
                Claim::Restored(&writer.owner)
            )
            .is_err()
        );
        assert!(Writer::begin(recovery.path, snapshot(), Claim::Fresh).is_err());
    }

    #[cfg(unix)]
    #[gpui::test]
    fn restore_state_handoff_takes_over_once_the_replaced_process_exits(
        cx: &mut gpui::TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery.json");
        // The process being replaced still holds an unclean checkpoint when
        // its successor starts, then exits without a clean quit (SIGTERM).
        let mut replaced = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        write(
            &path,
            &Checkpoint {
                owner: "replaced".into(),
                pid: replaced.id(),
                updated_at_ms: now_ms(),
                clean: false,
                snapshot: snapshot(),
            },
        )
        .unwrap();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        vcx.run_until_parked();
        vcx.update(|window, app| {
            install_at_path(&workspace, window, app, path.clone(), Claim::Handoff)
        });
        vcx.run_until_parked();
        assert_eq!(
            read_owner(&path).unwrap().pid,
            replaced.id(),
            "a live predecessor keeps its checkpoint"
        );
        replaced.kill().unwrap();
        replaced.wait().unwrap();
        for _ in 0..3 {
            vcx.executor().advance_clock(Duration::from_secs(1));
            vcx.run_until_parked();
        }
        let owner = read_owner(&path).unwrap();
        assert_eq!(
            owner.pid,
            std::process::id(),
            "the successor owns crash recovery"
        );
        assert!(!owner.clean);
        assert_eq!(read(&path).unwrap().snapshot.slots.len(), 1);
    }

    #[cfg(unix)]
    #[gpui::test]
    fn idle_checkpoint_writes_only_at_heartbeat_even_when_picker_changes(
        cx: &mut gpui::TestAppContext,
    ) {
        use std::os::unix::fs::MetadataExt;

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery.json");
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        vcx.run_until_parked();
        vcx.update(|window, app| {
            install_at_path(&workspace, window, app, path.clone(), Claim::Fresh)
        });
        vcx.run_until_parked();
        let mut writes = 0;
        let mut inode = fs::metadata(&path).unwrap().ino();
        for second in 1..=60 {
            workspace.update_in(vcx, |workspace, window, cx| {
                if second % 2 == 0 {
                    workspace.close_session_search(window, cx);
                } else {
                    workspace.open_resume(&super::super::OpenResume, window, cx);
                }
            });
            vcx.run_until_parked();
            vcx.executor().advance_clock(Duration::from_secs(1));
            vcx.run_until_parked();
            let next = fs::metadata(&path).unwrap().ino();
            if next != inode {
                writes += 1;
                inode = next;
            }
        }
        assert_eq!(writes, 6, "checkpoint writes in one simulated minute");
        for second in 1..=60 {
            workspace.update(vcx, |workspace, _| {
                let row = workspace.active_row;
                workspace.camera_target[row] = 0.0;
                workspace.camera_x[row] = second as f32 * 3.5;
            });
            vcx.run_until_parked();
            vcx.executor().advance_clock(Duration::from_secs(1));
            vcx.run_until_parked();
            let next = fs::metadata(&path).unwrap().ino();
            if next != inode {
                writes += 1;
                inode = next;
            }
        }
        assert_eq!(
            writes, 12,
            "camera animation frames do not rewrite the checkpoint between heartbeats"
        );
        workspace.update(vcx, |workspace, cx| {
            workspace.push_test_panel("session-b", cx)
        });
        vcx.run_until_parked();
        vcx.executor().advance_clock(Duration::from_secs(1));
        vcx.run_until_parked();
        assert_eq!(
            read(&path).unwrap().snapshot.slots.len(),
            2,
            "a tab change saves without waiting for the heartbeat"
        );
    }

    #[cfg(unix)]
    #[gpui::test]
    fn delayed_crash_relaunch_with_session_adopts_tabs_before_replacing_checkpoint(
        cx: &mut gpui::TestAppContext,
    ) {
        use jcode_desktop_api::WindowLaunch;
        let root = tempfile::tempdir().unwrap();
        let launch = WindowLaunch::from_parts(["--single-panel", "--session=session-new"], []);
        let recovery = Recovery::new(root.path(), &launch);
        let path = recovery.path.clone();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.single_panel = true;
            workspace.push_test_panel("session-keep", cx);
            workspace.push_test_panel("session-suspect", cx);
            workspace.active = 1;
            workspace.row_focus[0] = Some(workspace.slots[1].panel.entity_id());
            workspace
        });
        let crashed = vcx.update(|window, cx| workspace.read(cx).snapshot(window, cx).unwrap());
        let mut exited = std::process::Command::new("true").spawn().unwrap();
        let old_pid = exited.id();
        assert!(exited.wait().unwrap().success());
        let record = Checkpoint {
            owner: "crashed".into(),
            pid: old_pid,
            updated_at_ms: now_ms() - 278_000,
            clean: false,
            snapshot: crashed,
        };
        write(&path, &record).unwrap();
        let before = fs::read(&path).unwrap();
        assert!(Writer::begin(path.clone(), snapshot(), Claim::Fresh).is_err());
        assert!(Writer::begin(path.clone(), snapshot(), Claim::Restored("stale")).is_err());
        let restored = recovery
            .load()
            .expect("even an explicit launch must adopt a delayed crash");
        assert_eq!(
            fs::read(&path).unwrap(),
            before,
            "reading must not consume the checkpoint"
        );
        let resume = restored.snapshot.resume.clone();
        workspace.update_in(vcx, |workspace, window, cx| {
            workspace.launch = launch;
            workspace.apply_snapshot(restored.snapshot, cx);
            assert_eq!(
                workspace.slots[workspace.active].panel.read(cx).session_id,
                "session-suspect"
            );
            workspace.finish_launch(Some(resume), true, window, cx);
            assert_eq!(
                workspace.slots[workspace.active].panel.read(cx).session_id,
                "session-new"
            );
        });
        let owner = restored.crash_owner.unwrap();
        assert_eq!(fs::read(&path).unwrap(), before);
        vcx.update(|window, app| {
            recovery.install(&workspace, window, app, Claim::Restored(&owner))
        });
        let saved = read(&path).unwrap();
        assert!(!saved.clean);
        assert_eq!(saved.pid, std::process::id());
        assert_eq!(
            saved
                .snapshot
                .slots
                .iter()
                .map(|slot| slot.panel.session_id.as_str())
                .collect::<Vec<_>>(),
            ["session-new", "session-suspect", "session-keep"]
        );
    }

    #[test]
    fn recovery_validates_schema_and_discards_dead_host_resources() {
        let mut state = serde_json::to_value(snapshot()).unwrap();
        state["slots"] = serde_json::json!([{
            "panel": {
                "session_id": "terminal", "title": "shell", "working_dir": "/project",
                "draft": {"content": "draft", "selection_start": 5, "selection_end": 5,
                    "selection_reversed": false, "history": [], "history_index": null,
                    "live_draft": "", "attachments": []},
                "scroll_x": 0, "scroll_y": 0, "stick_to_bottom": true,
                "terminal_resource_id": 42, "terminal_output_cursor": 1234
            },
            "row": 1, "width_fraction": 0.5, "restore_fraction": null
        }]);
        let mut record = Checkpoint {
            owner: "dead".into(),
            pid: 123,
            updated_at_ms: 100_000,
            clean: false,
            snapshot: serde_json::from_value(state).unwrap(),
        };
        let restored = restore(&record, false).unwrap();
        assert_eq!(restored.slots[0].panel.terminal_resource_id, None);
        assert_eq!(restored.slots[0].panel.terminal_output_cursor, None);
        assert_eq!(restored.slots[0].panel.session_id, "terminal");
        assert_eq!(restored.slots[0].row, 1);
        assert_eq!(
            restored.slots[0].panel.working_dir.as_deref(),
            Some("/project")
        );
        assert_eq!(
            record.snapshot.slots[0].panel.terminal_resource_id,
            Some(42),
            "hot-reload snapshots remain untouched"
        );
        assert_eq!(
            record.snapshot.slots[0].panel.terminal_output_cursor,
            Some(1234)
        );
        record.snapshot.format_version = 999;
        assert!(restore(&record, false).is_none());
        record.snapshot.format_version = 1;
        record.snapshot.slots[0].row = super::super::STRIP_COUNT;
        assert!(restore(&record, false).is_none());
    }

    #[test]
    fn quit_disarms_and_late_save_cannot_recreate_checkpoint() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery.json");
        let writer = Writer::begin(path.clone(), snapshot(), Claim::Fresh).unwrap();
        assert!(writer.save(snapshot(), 123).unwrap());
        assert_eq!(read(&path).unwrap().updated_at_ms, 123);
        writer.clean_quit(None).unwrap();
        assert!(!writer.save(snapshot(), 456).unwrap());
        let record = read(&path).expect("a clean quit keeps the open panels");
        assert!(record.clean);
        assert_eq!(record.updated_at_ms, 123, "late save is ignored");
    }

    #[test]
    fn a_clean_quit_restores_open_sessions_on_any_later_launch() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery.json");
        let writer = Writer::begin(path.clone(), snapshot(), Claim::Fresh).unwrap();
        assert!(writer.save(snapshot(), 100_000).unwrap());
        writer.clean_quit(None).unwrap();
        let record = read(&path).unwrap();
        // Hours later, owner gone: relaunch must reopen the workspace.
        assert!(restore(&record, false).is_some());
        // A still-running owner keeps its checkpoint to itself.
        assert!(restore(&record, true).is_none());
    }

    #[test]
    fn reload_retires_old_writer_without_disarming_new_generation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery.json");
        let old = Writer::begin(path.clone(), snapshot(), Claim::Fresh).unwrap();
        let new = Writer::begin(path.clone(), snapshot(), Claim::Reload).unwrap();
        assert_ne!(old.owner, new.owner);
        assert!(!old.save(snapshot(), 123).unwrap());
        old.clean_quit(None).unwrap();
        assert_eq!(read(&path).unwrap().owner, new.owner);
        assert!(new.save(snapshot(), 456).unwrap());
    }

    #[test]
    fn corrupt_checkpoint_is_replaced_and_files_are_private() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery.json");
        fs::write(&path, b"partial json").unwrap();
        assert!(read(&path).is_none());
        let writer = Writer::begin(path.clone(), snapshot(), Claim::Fresh).unwrap();
        assert_eq!(read(&path).unwrap().owner, writer.owner);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(path.with_extension("lock"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}
