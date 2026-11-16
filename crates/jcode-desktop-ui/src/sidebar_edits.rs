//! Refresh session metadata after edits.

pub(super) fn refresh_after(event: &jcode_sdk::ApiEvent) -> bool {
    match event {
        jcode_sdk::ApiEvent::ToolDone { name, .. } => matches!(
            name.trim_start_matches("functions."),
            "write" | "edit" | "multiedit" | "patch" | "apply_patch" | "replace" | "batch"
        ),
        jcode_sdk::ApiEvent::TurnDone { .. } => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::workspace::{Workspace, tests::session_info};
    use crate::{harness::Update, learning};

    #[test]
    fn refresh_after_mutations_not_streaming_or_reads() {
        for name in ["edit", "write", "multiedit", "patch", "apply_patch", "batch", "functions.edit"] {
            let event = jcode_sdk::ApiEvent::ToolDone {
                session_id: "session".into(), call_id: "call".into(), name: name.into(),
                output: String::new(), error: Some("Partial patch failure".into()),
            };
            assert!(super::refresh_after(&event));
        }
        assert!(super::refresh_after(&jcode_sdk::ApiEvent::TurnDone {
            session_id: "session".into(), pending_soft_interrupts: None,
        }));
        assert!(!super::refresh_after(&jcode_sdk::ApiEvent::TextDelta {
            message_id: None, session_id: "session".into(), text: "writing".into(),
        }));
    }

    #[gpui::test]
    fn session_rows_never_render_edit_totals(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        let mut first = session_info("first", Some("First session"));
        first.edit_stats = Some(jcode_sdk::SessionEditStats {
            added: 350,
            removed: 109,
            approximate: false,
        });
        workspace.update(vcx, |w, cx| {
            w.sidebar_closed_sessions.insert("first".into());
            w.apply(
                Update::Sessions {
                    sessions: vec![first],
                },
                cx,
            );
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("sidebar-session-0").is_some());
        assert!(vcx.debug_bounds("sidebar-edits-first").is_none());
        assert!(vcx.debug_bounds("sidebar-edits-tooltip").is_none());
    }
}
