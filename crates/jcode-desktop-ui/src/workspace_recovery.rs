//! Short-lived crash checkpoints, separate from in-process hot reload snapshots.
//! A dead owner plus a recent heartbeat is required. Quitting normally disarms
//! recovery, and generation tokens prevent retired UI writers racing a reload.
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

const RECOVERY_WINDOW_MS: u64 = 30_000;
const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(1);

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

fn filename(mode: jcode_desktop_api::LaunchMode) -> Option<&'static str> {
    // Keep the primary single-panel window separate from spatial workspaces.
    match mode {
        jcode_desktop_api::LaunchMode::SinglePanel => Some("crash-recovery-single-panel.json"),
        jcode_desktop_api::LaunchMode::NoSidebar => Some("crash-recovery-no-sidebar.json"),
        jcode_desktop_api::LaunchMode::Workspace => Some("crash-recovery.json"),
    }
}

fn recovery_enabled(launch: &jcode_desktop_api::WindowLaunch, forwarded: bool) -> bool {
    use jcode_desktop_api::LaunchMode;
    let mode = LaunchMode::from_args(&launch.args);
    mode != LaunchMode::SinglePanel
        || (!forwarded
            && !LaunchMode::resume_requested(&launch.args)
            && LaunchMode::requested_session(&launch.args).is_none())
}

fn path(window: &Window, app: &App) -> Option<PathBuf> {
    let id = window.window_handle().window_id().as_u64();
    let launch = jcode_desktop_api::WindowLaunches::get(app, id);
    if !recovery_enabled(
        &launch,
        jcode_desktop_api::WindowLaunches::is_forwarded(app, id),
    ) {
        return None;
    }
    let name = filename(jcode_desktop_api::LaunchMode::from_args(&launch.args))?;
    Some(crate::learning::state_path()?.with_file_name(name))
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

fn eligible(checkpoint: &Checkpoint, now: u64, alive: bool) -> bool {
    !alive
        && (checkpoint.clean
            || now
                .checked_sub(checkpoint.updated_at_ms)
                .is_some_and(|age| age <= RECOVERY_WINDOW_MS))
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
    updated_at_ms: u64,
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

pub(crate) fn load(window: &Window, app: &App) -> Option<WorkspaceSnapshot> {
    let path = path(window, app)?;
    let _lock = lock(&path).ok()?;
    let checkpoint = read(&path)?;
    let now = now_ms();
    // A cleanly quit owner is gone by definition. Its pid may since belong
    // to an unrelated process, which must not block restoring the panels.
    let alive = !checkpoint.clean && process_alive(checkpoint.pid);
    let snapshot = restore(&checkpoint, now, alive)?;
    eprintln!(
        "restoring {} desktop panels from {} ({} ms since checkpoint)",
        snapshot.slots.len(),
        if checkpoint.clean {
            "last session"
        } else {
            "recent crash"
        },
        now.saturating_sub(checkpoint.updated_at_ms)
    );
    Some(snapshot)
}

fn restore(checkpoint: &Checkpoint, now: u64, alive: bool) -> Option<WorkspaceSnapshot> {
    if !eligible(checkpoint, now, alive) {
        return None;
    }
    // Validate the workspace schema as strictly as the hot-reload boundary.
    let mut snapshot = WorkspaceSnapshot::decode(&checkpoint.snapshot.encode().ok()?).ok()?;
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
    fn begin(path: PathBuf, snapshot: WorkspaceSnapshot) -> anyhow::Result<Self> {
        let _lock = lock(&path)?;
        let now = now_ms();
        if let Some(previous) = read_owner(&path) {
            let same_process = previous.pid == std::process::id();
            anyhow::ensure!(
                previous.clean || same_process || !process_alive(previous.pid),
                "another desktop process owns the crash checkpoint"
            );
            // An expired snapshot must not reopen at startup, but overwriting
            // it would also lose the list of sessions the user could reopen.
            if !previous.clean
                && !same_process
                && now
                    .checked_sub(previous.updated_at_ms)
                    .is_some_and(|age| age > RECOVERY_WINDOW_MS)
                && let Some(missed) = read(&path)
                && !missed.snapshot.slots.is_empty()
            {
                write(&path.with_extension("missed.json"), &missed)?;
            }
        }
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

/// Install at the plugin boundary so Ctrl+R enables recovery in existing hosts.
/// The loop retains only a weak workspace, never an old generation's panels.
pub(crate) fn install(workspace: &Entity<Workspace>, window: &Window, app: &mut App) {
    let Some(path) = path(window, app) else {
        return;
    };
    install_at_path(workspace, window, app, path);
}

fn install_at_path(workspace: &Entity<Workspace>, window: &Window, app: &mut App, path: PathBuf) {
    let writer = workspace
        .read(app)
        .snapshot(window, app)
        .and_then(|snapshot| Writer::begin(path, snapshot));
    let writer = match writer {
        Ok(writer) => writer,
        Err(error) => {
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
        loop {
            cx.background_executor().timer(CHECKPOINT_INTERVAL).await;
            let snapshot = window.update(cx, |_, window, app| {
                workspace
                    .upgrade()
                    .map(|workspace| workspace.read(app).snapshot(window, app))
            });
            let Ok(Some(Ok(snapshot))) = snapshot else {
                break;
            };
            let captured_at = now_ms();
            let writer = writer.clone();
            match cx
                .background_executor()
                .spawn(async move { writer.save(snapshot, captured_at) })
                .await
            {
                Ok(true) => {}
                Ok(false) => break,
                Err(error) => eprintln!("desktop crash checkpoint failed: {error:#}"),
            }
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::AppContext;

    fn relaunch_restores_tabs_and_pending_ask(cx: &mut gpui::TestAppContext, single: bool) {
        use jcode_desktop_api::LaunchMode;
        cx.update(crate::bind_workspace_keys);
        let mode = if single {
            LaunchMode::SinglePanel
        } else {
            LaunchMode::Workspace
        };
        let root = tempfile::tempdir().unwrap();
        let path = root
            .path()
            .join(filename(mode).expect("launch mode must persist its workspace"));
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
        let writer = Writer::begin(path.clone(), snapshot.clone()).unwrap();
        writer.clean_quit(Some(snapshot)).unwrap();
        let recovered = restore(&read(&path).unwrap(), now_ms(), false).unwrap();
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
        vcx.update(|window, app| install_at_path(&workspace, window, app, path.clone()));
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
        vcx.update(|window, app| install_at_path(&workspace, window, app, path.clone()));
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
        let restored = restore(&record, now_ms(), false).unwrap();
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
        vcx.update(|window, app| install_at_path(&workspace, window, app, path.clone()));
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
        let restored = restore(&record, now_ms(), false).unwrap();
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
    fn single_panel_recovery_is_separate_and_fresh_launches_are_excluded() {
        use jcode_desktop_api::LaunchMode;
        use jcode_desktop_api::WindowLaunch;
        assert_eq!(
            filename(LaunchMode::SinglePanel),
            Some("crash-recovery-single-panel.json")
        );
        assert_eq!(filename(LaunchMode::Workspace), Some("crash-recovery.json"));
        assert_eq!(
            filename(LaunchMode::NoSidebar),
            Some("crash-recovery-no-sidebar.json")
        );
        let launch = |args: &[&str]| WindowLaunch::from_parts(args.iter().copied(), []);
        assert!(recovery_enabled(&launch(&["--single-panel"]), false));
        assert!(!recovery_enabled(&launch(&["--single-panel"]), true));
        assert!(!recovery_enabled(
            &launch(&["--single-panel", "--session=other"]),
            false
        ));
        assert!(!recovery_enabled(&launch(&["--resume"]), false));
    }

    #[test]
    fn only_recent_dead_owners_are_recoverable() {
        let record = Checkpoint {
            owner: "old".into(),
            pid: 123,
            updated_at_ms: 100_000,
            clean: false,
            snapshot: snapshot(),
        };
        assert!(eligible(&record, 100_000, false));
        assert!(eligible(&record, 130_000, false));
        assert!(!eligible(&record, 130_001, false));
        assert!(!eligible(&record, 99_999, false));
        assert!(!eligible(&record, 100_001, true));
        assert!(process_alive(std::process::id()));
        assert!(process_alive(0));
    }

    #[cfg(unix)]
    #[gpui::test]
    fn delayed_relaunch_keeps_crashed_tabs_for_manual_recovery(cx: &mut gpui::TestAppContext) {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("crash-recovery.json");
        let missed = root.path().join("crash-recovery.missed.json");
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-keep", cx);
            workspace.push_test_panel("session-suspect", cx);
            workspace.active = 1;
            workspace.row_focus[0] = Some(workspace.slots[1].panel.entity_id());
            workspace
        });
        let crashed = vcx.update(|window, cx| workspace.read(cx).snapshot(window, cx).unwrap());
        let writer = Writer::begin(path.clone(), crashed.clone()).unwrap();
        let mut exited = std::process::Command::new("true").spawn().unwrap();
        let old_pid = exited.id();
        assert!(exited.wait().unwrap().success());
        let now = now_ms();
        let mut record = writer.checkpoint(crashed, now - 278_000);
        record.pid = old_pid;
        write(&path, &record).unwrap();

        assert!(restore(&record, now, process_alive(old_pid)).is_none());
        assert!(!missed.exists());
        let fresh = Writer::begin(path.clone(), snapshot()).unwrap();
        assert!(read(&path).unwrap().snapshot.slots.is_empty());
        let saved = read(&missed).expect("a delayed launch must not discard the crashed tabs");
        let ids: Vec<_> = saved
            .snapshot
            .slots
            .iter()
            .map(|slot| slot.panel.session_id.as_str())
            .collect();
        assert_eq!(ids, ["session-keep", "session-suspect"]);
        assert_eq!(saved.snapshot.active, 1);
        assert_eq!(saved.updated_at_ms, record.updated_at_ms);
        assert_eq!(
            fs::metadata(&missed).unwrap().permissions().mode() & 0o777,
            0o600
        );

        fresh.clean_quit(None).unwrap();
        Writer::begin(path, snapshot()).unwrap();
        assert_eq!(read(&missed).unwrap().pid, old_pid);
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
        let restored = restore(&record, 110_000, false).unwrap();
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
        assert!(restore(&record, 110_000, false).is_none());
        record.snapshot.format_version = 1;
        record.snapshot.slots[0].row = super::super::STRIP_COUNT;
        assert!(restore(&record, 110_000, false).is_none());
    }

    #[test]
    fn quit_disarms_and_late_save_cannot_recreate_checkpoint() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery.json");
        let writer = Writer::begin(path.clone(), snapshot()).unwrap();
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
        let writer = Writer::begin(path.clone(), snapshot()).unwrap();
        assert!(writer.save(snapshot(), 100_000).unwrap());
        writer.clean_quit(None).unwrap();
        let record = read(&path).unwrap();
        // Hours later, owner gone: relaunch must reopen the workspace.
        assert!(restore(&record, 100_000 + 6 * 3_600_000, false).is_some());
        // A still-running owner keeps its checkpoint to itself.
        assert!(restore(&record, 100_000 + 6 * 3_600_000, true).is_none());
    }

    #[test]
    fn reload_retires_old_writer_without_disarming_new_generation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery.json");
        let old = Writer::begin(path.clone(), snapshot()).unwrap();
        let new = Writer::begin(path.clone(), snapshot()).unwrap();
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
        let writer = Writer::begin(path.clone(), snapshot()).unwrap();
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
