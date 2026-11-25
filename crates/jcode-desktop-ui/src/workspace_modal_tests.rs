//! Workspace modes that float above the canvas: rename, folder picker, usage
//! reset and the beta notice. These tests describe how a mode opens, how it is
//! dismissed, where focus lands afterwards, and that typing aimed at the
//! workspace root is never redirected into the composer while one is open.
//! They talk to each mode through the four helpers below, so they hold across
//! changes to how a mode is stored.
use super::*;
use crate::accounts::{BankedReset, ResetProvider};

#[derive(Clone, Copy, Debug)]
enum Mode {
    Rename,
    FolderPicker,
    UsageReset,
    BetaNotice,
}

const ALL: [Mode; 4] = [
    Mode::Rename,
    Mode::FolderPicker,
    Mode::UsageReset,
    Mode::BetaNotice,
];

fn open(mode: Mode, w: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    match mode {
        Mode::Rename => w.rename_session(&RenameSession, window, cx),
        Mode::FolderPicker => w.open_folder(&OpenFolder, window, cx),
        Mode::UsageReset => w.open_usage_reset(
            &BankedReset {
                provider: ResetProvider::Claude,
                account_label: None,
                available_count: 1,
                usable_count: None,
                expires_at: None,
                cached_source: None,
                session_reset_available: None,
                limit_reached: true,
                next_available_at: None,
            },
            window,
            cx,
        ),
        Mode::BetaNotice => w.show_beta_notice(window, cx),
    }
    cx.notify();
}

fn is_open(mode: Mode, w: &Workspace, cx: &gpui::App) -> bool {
    match mode {
        Mode::Rename => w.active_modal::<super::rename::Rename>(cx).is_some(),
        Mode::FolderPicker => w.folder_picker(cx).is_some(),
        Mode::UsageReset => w.active_modal::<super::usage_reset::UsageReset>(cx).is_some(),
        Mode::BetaNotice => w.active_modal::<super::beta_notice::BetaNotice>(cx).is_some(),
    }
}

/// A selector inside the mode's dialog body, used to aim "outside" clicks.
fn dialog(mode: Mode) -> &'static str {
    match mode {
        Mode::Rename => "rename-session-dialog",
        Mode::FolderPicker => "folder-picker-overlay",
        Mode::UsageReset => "usage-reset-dialog",
        Mode::BetaNotice => "desktop-beta-notice",
    }
}

/// Where Escape lands, whether a click on the scrim dismisses, and whether the
/// mode takes keyboard focus while open.
struct Behavior {
    escape_dismisses: bool,
    outside_click_dismisses: bool,
    takes_focus: bool,
}

fn behavior(mode: Mode) -> Behavior {
    match mode {
        Mode::Rename => Behavior {
            escape_dismisses: true,
            outside_click_dismisses: true,
            takes_focus: true,
        },
        Mode::FolderPicker => Behavior {
            escape_dismisses: true,
            outside_click_dismisses: false,
            takes_focus: true,
        },
        Mode::UsageReset => Behavior {
            escape_dismisses: true,
            outside_click_dismisses: false,
            takes_focus: true,
        },
        Mode::BetaNotice => Behavior {
            escape_dismisses: false,
            outside_click_dismisses: false,
            takes_focus: false,
        },
    }
}

fn setup(
    cx: &mut gpui::TestAppContext,
) -> (Entity<Workspace>, &mut gpui::VisualTestContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("session_fox_original", cx);
        w.push_test_panel("session_owl_neighbor", cx);
        w.sessions.push(crate::workspace::tests::session_info(
            "session_fox_original",
            Some("Original"),
        ));
        w
    });
    vcx.update(|window, cx| workspace.update(cx, |w, cx| w.focus_active(window, cx)));
    vcx.run_until_parked();
    (workspace, vcx)
}

fn open_mode(
    mode: Mode,
    workspace: &Entity<Workspace>,
    vcx: &mut gpui::VisualTestContext,
) {
    vcx.update(|window, cx| workspace.update(cx, |w, cx| open(mode, w, window, cx)));
    vcx.run_until_parked();
    assert!(
        workspace.read_with(vcx, |w, cx| is_open(mode, w, cx)),
        "{mode:?} must open"
    );
}

fn composer_focus(
    workspace: &Entity<Workspace>,
    vcx: &mut gpui::VisualTestContext,
) -> gpui::FocusHandle {
    vcx.update(|_, cx| {
        let w = workspace.read(cx);
        w.slots[w.active].panel.input_focus_handle(cx)
    })
}

fn composer_text(workspace: &Entity<Workspace>, vcx: &mut gpui::VisualTestContext) -> String {
    workspace.read_with(vcx, |w, cx| {
        w.slots[w.active]
            .panel
            .read_chat(cx)
            .unwrap()
            .input
            .read(cx)
            .content
            .to_string()
    })
}

#[gpui::test]
fn escape_dismisses_modes_that_own_it_and_returns_focus(cx: &mut gpui::TestAppContext) {
    for mode in ALL {
        let (workspace, vcx) = setup(cx);
        let composer = composer_focus(&workspace, vcx);
        vcx.update(|window, _| assert!(composer.is_focused(window), "{mode:?} baseline"));
        open_mode(mode, &workspace, vcx);
        vcx.update(|window, _| {
            assert_eq!(
                !composer.is_focused(window),
                behavior(mode).takes_focus,
                "{mode:?} focus while open"
            )
        });
        vcx.simulate_keystrokes("escape");
        vcx.run_until_parked();
        assert_eq!(
            !workspace.read_with(vcx, |w, cx| is_open(mode, w, cx)),
            behavior(mode).escape_dismisses,
            "{mode:?} escape"
        );
        if behavior(mode).escape_dismisses {
            vcx.update(|window, _| {
                assert!(composer.is_focused(window), "{mode:?} must return focus")
            });
        }
    }
}

#[gpui::test]
fn outside_click_dismisses_only_modes_that_allow_it(cx: &mut gpui::TestAppContext) {
    for mode in ALL {
        let (workspace, vcx) = setup(cx);
        let composer = composer_focus(&workspace, vcx);
        open_mode(mode, &workspace, vcx);
        let shown = vcx.debug_bounds(dialog(mode)).expect("mode must render");
        // Left of every dialog and the beta card, clear of the title bar.
        let outside = gpui::point(px(4.0), px(400.0));
        if !matches!(mode, Mode::FolderPicker) {
            // The folder picker names its full-window scrim.
            assert!(!shown.contains(&outside), "{mode:?} dialog covers the click");
        }
        vcx.simulate_click(outside, gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(
            !workspace.read_with(vcx, |w, cx| is_open(mode, w, cx)),
            behavior(mode).outside_click_dismisses,
            "{mode:?} outside click"
        );
        if behavior(mode).outside_click_dismisses {
            vcx.update(|window, _| {
                assert!(composer.is_focused(window), "{mode:?} must return focus")
            });
        }
    }
}

#[gpui::test]
fn root_typing_is_not_redirected_to_the_composer_while_a_mode_is_open(
    cx: &mut gpui::TestAppContext,
) {
    // Control: with nothing open, typing at the root reaches the composer, so a
    // blocked redirect below is the mode's doing and not a dead test.
    let (workspace, vcx) = setup(cx);
    vcx.update(|window, cx| {
        let focus = workspace.read(cx).focus_handle.clone();
        window.focus(&focus, cx)
    });
    vcx.simulate_keystrokes("x");
    vcx.run_until_parked();
    assert_eq!(composer_text(&workspace, vcx), "x");

    for mode in ALL {
        let (workspace, vcx) = setup(cx);
        open_mode(mode, &workspace, vcx);
        vcx.update(|window, cx| {
            let focus = workspace.read(cx).focus_handle.clone();
            window.focus(&focus, cx)
        });
        vcx.simulate_keystrokes("x");
        vcx.run_until_parked();
        assert_eq!(composer_text(&workspace, vcx), "", "{mode:?} redirect");
        assert!(
            workspace.read_with(vcx, |w, cx| is_open(mode, w, cx)),
            "{mode:?} stays open"
        );
    }
}

#[gpui::test]
fn a_folder_picker_restored_after_reload_closes_back_to_the_composer(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = setup(cx);
    open_mode(Mode::FolderPicker, &workspace, vcx);
    let snapshot = vcx.update(|window, cx| workspace.read(cx).snapshot(window, cx).unwrap());
    assert!(snapshot.folder_picker_dir.is_some());
    assert!(!snapshot.folder_picker_sets_default);
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(!workspace.read_with(vcx, |w, cx| is_open(Mode::FolderPicker, w, cx)));

    vcx.update(|window, cx| {
        workspace.update(cx, |w, cx| {
            w.apply_snapshot(snapshot, cx);
            w.restore_focus(window, cx);
        })
    });
    vcx.run_until_parked();
    assert!(workspace.read_with(vcx, |w, cx| is_open(Mode::FolderPicker, w, cx)));
    assert!(vcx.debug_bounds("folder-picker-overlay").is_some());
    let search = workspace.read_with(vcx, |w, cx| w.folder_picker(cx).unwrap().focus_handle(cx));
    vcx.update(|window, _| assert!(search.is_focused(window), "restored focus"));

    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(!workspace.read_with(vcx, |w, cx| is_open(Mode::FolderPicker, w, cx)));
    let composer = composer_focus(&workspace, vcx);
    vcx.update(|window, _| assert!(composer.is_focused(window), "focus returns to the panel"));
}
