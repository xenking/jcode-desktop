//! Cloud and paired-machine routing through the machines panel.
use super::super::remote_tests::{click, session};
use super::*;

const WORK_ID: &str = "0123456789abcdef0123456789abcdef";
const WORK_HOST: &str = "paired:0123456789abcdef0123456789abcdef";

fn paired_workspace(
    cx: &mut gpui::TestAppContext,
) -> (
    Entity<Workspace>,
    &mut gpui::VisualTestContext,
    std::sync::mpsc::Receiver<Command>,
) {
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.bridge = bridge;
        w.remotes = Machines::default();
        w.remotes.paired.push(paired::Destination {
            id: WORK_ID.into(),
            endpoint: "https://work.example".into(),
            label: "WORK".into(),
            working_dir: "/work/default".into(),
        });
        w.show_machines(cx);
        w
    });
    (workspace, vcx, commands)
}

fn fail(
    workspace: &Entity<Workspace>,
    vcx: &mut gpui::VisualTestContext,
    kind: jcode_sdk::PairedFailure,
) {
    workspace.update(vcx, |w, cx| {
        w.apply(
            Update::RemoteStatus {
                host: WORK_HOST.into(),
                message: format!("Remote session failed: {kind:?}"),
                request_id: None,
                failed: true,
                failure: Some(crate::connect_failure::ConnectFailure::from_error(
                    &jcode_sdk::Error::new(jcode_sdk::ErrorKind::Paired(kind), "cause"),
                )),
            },
            cx,
        );
    });
    vcx.run_until_parked();
    // Machines slides into view. Settle it so the row has painted bounds.
    vcx.executor().advance_clock(Duration::from_secs(2));
    vcx.run_until_parked();
}

#[test]
fn labels_published_by_another_thread_never_rename_this_threads_machine() {
    let destination = |label: &str| paired::Destination {
        id: WORK_ID.into(),
        endpoint: "https://work.example".into(),
        label: label.into(),
        working_dir: "/work/default".into(),
    };
    jcode_desktop_harness::paired::publish_labels(&[destination("WORK")]);
    // A parallel test building its own workspace publishes its own machines.
    std::thread::spawn(|| jcode_desktop_harness::paired::publish_labels(&[]))
        .join()
        .unwrap();
    assert_eq!(machines::machine_label(WORK_HOST), "WORK");
}

#[gpui::test]
fn paired_machine_creates_sessions_in_multiple_remote_projects(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx, commands) = paired_workspace(cx);
    for path in ["/work/one", "/work/two"] {
        workspace.update(vcx, |w, cx| {
            w.remotes.path_inputs[WORK_HOST].update(cx, |input, cx| input.set_content(path.into(), cx));
        });
        click(vcx, "machine-connect-2");
        assert!(matches!(commands.try_recv(), Ok(Command::CreateRemoteSession { host, working_dir: Some(dir), .. })
            if host == WORK_HOST && dir == path));
    }
}

#[gpui::test]
fn machine_rows_keep_their_actions_inside_the_row_at_narrow_and_wide_widths(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx, _commands) = paired_workspace(cx);
    fail(&workspace, vcx, jcode_sdk::PairedFailure::Unresolved);
    for width in [420.0, 700.0, 1600.0] {
        vcx.simulate_resize(gpui::size(px(width), px(900.0)));
        vcx.executor().advance_clock(Duration::from_secs(2));
        vcx.run_until_parked();
        let row = vcx.debug_bounds("machine-row-2").unwrap();
        let mut previous_right = None;
        for selector in ["machine-connect-2", "machine-sessions-2", "machine-default-2"] {
            let pill = vcx.debug_bounds(selector).unwrap();
            assert!(
                pill.left() >= row.left() && pill.right() <= row.right(),
                "{selector} escapes its row at {width}: {pill:?} vs {row:?}"
            );
            if let Some(right) = previous_right {
                assert!(pill.left() >= right, "{selector} overlaps its neighbour at {width}");
            }
            previous_right = Some(pill.right());
        }
        let status = vcx.debug_bounds("machine-status-2").unwrap();
        let connect = vcx.debug_bounds("machine-connect-2").unwrap();
        let same_line = status.top() < connect.bottom() && connect.top() < status.bottom();
        assert!(
            !same_line || status.right() <= connect.left(),
            "status runs under the actions at {width}: {status:?} vs {connect:?}"
        );
    }
}

#[gpui::test]
fn paired_machine_path_is_editable_and_must_be_absolute(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx, commands) = paired_workspace(cx);
    assert!(vcx.debug_bounds("machine-path-save-2").is_none());
    workspace.update(vcx, |w, cx| {
        w.remotes.path_inputs[WORK_HOST].update(cx, |input, cx| input.set_content("relative".into(), cx));
    });
    click(vcx, "machine-connect-2");
    assert!(commands.try_recv().is_err());
    assert!(vcx.debug_bounds("machine-notice-2").is_some());
    assert!(vcx.debug_bounds("machine-path-save-2").is_some());
}

#[gpui::test]
fn unreachable_paired_machine_shows_its_cause_inside_its_row_and_retry_reconnects(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx, commands) = paired_workspace(cx);
    fail(&workspace, vcx, jcode_sdk::PairedFailure::Unresolved);
    workspace.read_with(vcx, |w, _| {
        let notice = &w.remotes.row_notices[WORK_HOST];
        assert_eq!(
            notice.summary,
            "Can't reach WORK (name did not resolve). Is Tailscale or the VPN on?"
        );
        assert!(!w.remotes.status.clone().unwrap().contains("paired:"));
    });
    assert!(vcx.debug_bounds("machine-notice-2").is_some());
    assert!(vcx.debug_bounds("machine-connection-status").is_none());
    click(vcx, "machine-notice-action-2");
    assert!(matches!(commands.try_recv(), Ok(Command::CreateRemoteSession { host, .. }) if host == WORK_HOST));
    assert!(vcx.debug_bounds("machines-picker").is_some());
    // The next attempt's progress clears the old error from the row.
    workspace.update(vcx, |w, cx| {
        w.apply(
            Update::RemoteStatus {
                host: WORK_HOST.into(),
                message: "Connected to WORK".into(),
                request_id: None,
                failed: false,
                failure: None,
            },
            cx,
        );
    });
    vcx.run_until_parked();
    vcx.executor().advance_clock(Duration::from_secs(2));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("machine-notice-2").is_none());
    workspace.read_with(vcx, |w, _| assert!(w.remotes.reachable.contains(WORK_HOST)));
}

#[gpui::test]
fn revoked_paired_machine_offers_repair_inside_its_row(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx, commands) = paired_workspace(cx);
    fail(&workspace, vcx, jcode_sdk::PairedFailure::Revoked);
    workspace.read_with(vcx, |w, _| {
        assert_eq!(w.remotes.row_notices[WORK_HOST].summary, "WORK no longer accepts this device. Pair it again.");
    });
    click(vcx, "machine-notice-action-2");
    assert!(commands.try_recv().is_err());
    workspace.read_with(vcx, |w, cx| {
        let form = w.remotes.pairing.as_ref().unwrap();
        assert_eq!(form.repair.as_deref(), Some(WORK_HOST));
        assert_eq!(form.endpoint.read(cx).snapshot().content, "https://work.example");
        assert_eq!(form.label.read(cx).snapshot().content, "WORK");
    });
    assert!(vcx.debug_bounds("machine-pairing-form").is_some());
}

#[gpui::test]
fn machines_picker_routes_named_cloud_connect_default_and_session_open(
    cx: &mut gpui::TestAppContext,
) {
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.bridge = bridge;
        workspace.remotes = Machines::default();
        workspace.show_machines(cx);
        workspace
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("machine-connect-1").is_some());
    assert!(vcx.debug_bounds("machine-connect-2").is_none());

    workspace.update(vcx, |w, cx| {
        w.remotes.registered = vec![
            RegisteredMachine {
                name: "ai-core".into(),
                is_default: true,
                last_seen_at: Some(1_795_236_600_000),
            },
            RegisteredMachine {
                name: "studio".into(),
                is_default: false,
                last_seen_at: None,
            },
        ];
        cx.notify();
    });
    click(vcx, "machine-default-2");
    workspace.read_with(vcx, |w, _| {
        assert_eq!(w.remotes.default_host.as_deref(), Some("cloud:ai-core"))
    });
    assert!(commands.try_recv().is_err());
    click(vcx, "machine-sessions-2");
    let first = match commands.try_recv().unwrap() {
        Command::ListRemoteSessions { host, request_id } if host == "cloud:ai-core" => {
            request_id
        }
        _ => panic!("expected ai-core catalog request"),
    };
    click(vcx, "machine-sessions-3");
    let second = match commands.try_recv().unwrap() {
        Command::ListRemoteSessions { host, request_id } if host == "cloud:studio" => {
            request_id
        }
        _ => panic!("expected studio catalog request"),
    };
    workspace.update(vcx, |w, cx| {
        w.apply_machine_sessions(first, Ok(vec![session("cloud://ai-core/not-current")]));
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("cloud-open-session-0").is_none());
    workspace.update(vcx, |w, cx| {
        w.apply_machine_sessions(second, Ok(vec![session("cloud://studio/session-existing")]));
        cx.notify();
    });
    click(vcx, "cloud-open-session-0");
    assert!(
        matches!(commands.try_recv(), Ok(Command::Watch { session_id }) if session_id == "cloud://studio/session-existing")
    );

    click(vcx, "machines-picker-button");
    click(vcx, "machine-connect-3");
    assert!(
        matches!(commands.try_recv(), Ok(Command::CreateRemoteSession { host, working_dir: None, .. }) if host == "cloud:studio")
    );
    workspace.update(vcx, |w, cx| w.open_default_draft(None, cx));
    assert!(
        matches!(commands.try_recv(), Ok(Command::CreateRemoteSession { host, working_dir: None, .. }) if host == "cloud:ai-core")
    );
}

#[test]
fn cloud_active_detector_requires_connected_nonclosing_exact_namespace() {
    let id = "ssh://jcode-cloud-alpha/session";
    assert!(live_cloud_panel(false, id, true, false, true));
    assert!(!live_cloud_panel(false, id, true, false, false));
    assert!(live_cloud_panel(false, id, true, false, true));
    // A lost old panel must not keep checks alive once a new live one closes.
    assert!(
        ![(false, false), (true, true)]
            .into_iter()
            .any(|(closing, connected)| live_cloud_panel(closing, id, true, false, connected))
    );
    assert!(!live_cloud_panel(true, id, true, false, true));
    assert!(!live_cloud_panel(false, id, false, false, true));
    assert!(!live_cloud_panel(false, id, true, true, true));
    for id in [
        "local",
        "startup://draft",
        "ssh://other/session",
        "ssh://jcode-cloud-alpha-evil/session",
        "ssh://jcode-cloud-alpha/",
    ] {
        assert!(!live_cloud_panel(false, id, true, false, true));
    }
}

#[gpui::test]
fn managed_and_legacy_cloud_defaults_connect_only_to_managed_cloud(
    cx: &mut gpui::TestAppContext,
) {
    let (bridge, commands) = harness::spawn_recording();
    let workspace = cx.new(|cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.bridge = bridge;
        workspace
    });
    workspace.update(cx, |workspace, cx| {
        for host in [MANAGED_CLOUD_HOST, cloud_alpha::HOST] {
            workspace.remotes.default_host = Some(host.into());
            workspace.create_default_session(
                Some("/local-only".into()),
                Some(Panel::STARTUP_SESSION_ID.into()),
                cx,
            );
            assert!(
                matches!(
                    commands.try_recv(),
                    Ok(Command::CreateRemoteSession { host, working_dir: None, request_id: Some(id) })
                        if host == MANAGED_CLOUD_HOST && id == Panel::STARTUP_SESSION_ID
                ),
                "cloud must never fall back to SSH aliases, the AWS alpha, or local"
            );
            assert!(commands.try_recv().is_err());
            assert!(
                workspace.remotes.cloud.take_updates().is_empty(),
                "must not invoke the personal AWS helper"
            );
            assert!(!workspace.remotes.startup_failed);
            assert_eq!(
                workspace.remotes.default_host.as_deref(),
                Some(MANAGED_CLOUD_HOST)
            );
        }
    });
}

#[gpui::test]
fn managed_and_legacy_cloud_connect_open_a_managed_draft(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = harness::spawn_recording();
    let workspace = cx.new(|cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.bridge = bridge;
        workspace
    });
    workspace.update(cx, |workspace, cx| {
        for host in [MANAGED_CLOUD_HOST, cloud_alpha::HOST] {
            workspace.connect_machine(Some(host.into()), cx);
            let panel = workspace.slots[workspace.active].panel.read(cx);
            let id = panel.session_id.clone();
            assert_eq!(pending::remote_draft_host(&id), Some(MANAGED_CLOUD_HOST));
            assert!(panel.status.contains(MANAGED_CLOUD_STARTING));
            assert!(matches!(
                commands.try_recv(),
                Ok(Command::CreateRemoteSession { host, request_id: Some(request), .. })
                    if host == MANAGED_CLOUD_HOST && request == id
            ));
            assert!(workspace.remotes.cloud.take_updates().is_empty());
        }
    });
}

#[test]
fn managed_names_never_rewrite_existing_ssh_session_ids() {
    assert_eq!(new_session_host(cloud_alpha::HOST), MANAGED_CLOUD_HOST);
    assert_eq!(new_session_host("my-server"), "my-server");
    assert!(!managed_destination("jcode-cloud-evil"));
    assert!(!managed_destination("ssh://jcode-cloud-alpha/session"));
}
