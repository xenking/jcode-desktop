//! The ask card's answer travels as a typed record end to end: card -> wire
//! -> transcript, with no prose parsed back, and who asked / who may answer
//! comes from the request itself.
use super::*;
use jcode_session_types::AskRecord;

fn request(id: &str, kind: &str, question: &str) -> ApiEvent {
    serde_json::from_value(serde_json::json!({
        "ev": "ask_request", "session_id": "session-a", "request_id": id,
        "tool_call_id": format!("call-{id}"), "kind": kind,
        "questions": [{"header": "Database", "question": question, "options": [
            {"label": "Postgres"}, {"label": "SQLite"}]}]
    }))
    .unwrap()
}

fn texts(panel: &Panel, call_id: &str) -> Vec<String> {
    let Some(Item::Tool {
        input,
        output,
        error,
        ask,
        ..
    }) = panel
        .items
        .iter()
        .find(|item| matches!(item, Item::Tool { call_id: id, .. } if id == call_id))
    else {
        panic!("no tool row for {call_id}");
    };
    super::decision::selection_segments(0, ask.as_deref(), input, output, error.as_deref())
        .into_iter()
        .map(|(_, text)| text)
        .collect()
}

#[gpui::test]
fn typed_answer_round_trips_from_the_card_to_the_transcript(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |w, _| w.test_panel(0)).unwrap();
    let event: ApiEvent = serde_json::from_value(serde_json::json!({
        "ev": "ask_request", "session_id": "session-a", "request_id": "ask-c1", "tool_call_id": "c1",
        "questions": [
            {"question": "Single?", "options": [{"label": "A"}, {"label": "B"}]},
            {"question": "Multi?", "multi_select": true, "options": [{"label": "X"}, {"label": "Y"}, {"label": "Z"}]},
            {"question": "Free?", "allow_other": true, "options": [{"label": "P"}, {"label": "Q"}]}
        ]
    }))
    .unwrap();
    let response = panel.update(vcx, |panel, cx| {
        panel.apply(&event, cx);
        let card = panel.ask_card.as_mut().unwrap();
        card.toggle(0, 1);
        card.toggle(1, 0);
        card.toggle(1, 2);
        card.comments[1][0] = "because".into();
        card.other[2] = Some("custom".into());
        let response = card.response_with_images(cx);
        panel.answer_ask(response, cx);
        commands
            .try_iter()
            .find_map(|command| match command {
                crate::harness::Command::AskResponse { response, .. } => Some(response),
                _ => None,
            })
            .expect("the answer is sent through the bridge")
    });
    let answers = response.answers.clone().unwrap();
    assert_eq!(answers[0].selected, ["B"]);
    assert_eq!(answers[1].selected, ["X", "Z"]);
    assert_eq!(
        answers[1].comments.get("X").map(String::as_str),
        Some("because")
    );
    assert_eq!(answers[2].other.as_deref(), Some("custom"));

    // The daemon stores that response and sends it back typed.
    let questions = match &event {
        ApiEvent::AskRequest { questions, .. } => questions.clone(),
        _ => unreachable!(),
    };
    let record = AskRecord::new(questions, response);
    panel.update(vcx, |panel, cx| {
        panel.apply(
            &ApiEvent::ToolDone {
                session_id: "session-a".into(),
                call_id: "c1".into(),
                name: "ask".into(),
                output: record.model_text(),
                error: None,
                codemode: None,
                ask: Some(Box::new(record.clone())),
            },
            cx,
        );
        assert!(panel.ask_card.is_none());
        assert_eq!(
            texts(panel, "c1"),
            [
                "Single?",
                "✓ B",
                "Multi?",
                "✓ X\nUser comment: because\n✓ Z",
                "Free?",
                "User answer: custom",
            ]
        );
    });
}

#[gpui::test]
fn history_renders_the_typed_record_not_the_prose_beside_it(cx: &mut gpui::TestAppContext) {
    let record = serde_json::json!({
        "questions": [{"question": "Which?", "options": [{"label": "A"}, {"label": "B"}]}],
        "response": {"answers": [{"selected": ["B"], "comments": {"B": "why"}}]}
    });
    let message: jcode_sdk::HistoryMessage = serde_json::from_value(serde_json::json!({
        "role": "tool", "content": "text the model read, never parsed by clients",
        "tool": {"id": "ask-1", "name": "ask", "input": {}, "ask": record}
    }))
    .unwrap();
    let (items, _) = crate::panel::history_items::build(vec![message], vec![]);
    let (panel, _) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "typed-history".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        *panel.items = items;
        panel
    });
    panel.read_with(cx, |panel, _| {
        assert_eq!(texts(panel, "ask-1"), ["Which?", "✓ B\nUser comment: why"]);
    });
}

#[gpui::test]
fn worker_ask_names_the_worker_and_approvals_stay_with_a_person(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |w, _| w.test_panel(0)).unwrap();
    panel.update(vcx, |panel, cx| {
        panel.apply(
            &request("r1", "question", "Worker \"builder\": Which database?"),
            cx,
        );
        let card = panel.ask_card.as_ref().unwrap();
        let shown = question_without_header(
            &card.questions[0].question,
            card.questions[0].header.as_deref(),
        );
        assert!(shown.starts_with("Worker \"builder\": "), "{shown}");
        assert!(
            !panel.ask_needs_human("r1:0:0"),
            "a plain question may be answered by automation"
        );

        panel.apply(
            &request(
                "r2",
                "worker_mcp_approval",
                "Worker \"builder\": Allow tool?",
            ),
            cx,
        );
        panel.answer_ask(AskResponse::declined(), cx);
        let attempt = panel.ask_card.as_ref().unwrap().attempt_id.unwrap();
        panel.ask_response_settled("r1", attempt, Ok(()), cx);
        let card = panel.ask_card.as_ref().unwrap();
        assert_eq!(
            (card.request_id.as_str(), card.kind),
            ("r2", jcode_session_types::AskKind::WorkerMcpApproval)
        );
        assert!(panel.ask_needs_human("r2:0:0"));
        assert!(
            !panel.ask_needs_human("r1:0:0"),
            "another request's selector is not this approval"
        );
    });
}
