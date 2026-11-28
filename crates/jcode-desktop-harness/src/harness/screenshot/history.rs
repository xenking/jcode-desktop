use crate::harness::protocol::{Command, Update};
use jcode_base::compaction::CompactionEvent;
use jcode_base::message::{ContentBlock, Role};
use jcode_base::session::{self, Session, StoredCompactionState, StoredMessage};
use jcode_sdk::ApiEvent;

const ID: &str = "screenshot-fixture";
const ARCHIVED: usize = 100;
const GUIDE: &str = "Resume prior conversation.\n\nArchived transcript scopes:\n\nReading HISTORY:\n\nHISTORY\n===================";

pub(super) fn enabled() -> bool {
    std::env::var("JCODE_DESKTOP_CONTROL_TEST").as_deref() == Ok("1")
        && std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref() == Ok("compaction")
}

pub(super) fn session() -> Session {
    let mut session = Session::create_with_id(ID.into(), None, None);
    let text = |text| {
        vec![ContentBlock::Text {
            text,
            cache_control: None,
        }]
    };
    for i in 0..ARCHIVED {
        session.add_message(Role::User, text(format!("Before compaction {i:03}")));
    }
    session.adopt_compaction(Some(StoredCompactionState {
        summary_text: GUIDE.into(),
        openai_encrypted_content: None,
        snapcompact: None,
        covers_up_to_turn: ARCHIVED,
        original_turn_count: ARCHIVED,
        compacted_count: ARCHIVED,
        plan_context: None,
    }));
    session.record_compaction_event(
        &CompactionEvent {
            trigger: "reactive".into(),
            method: Some("snapcompact".into()),
            pre_tokens: Some(524_000),
            post_tokens: Some(100_000),
            tokens_saved: Some(424_000),
            duration_ms: None,
            messages_dropped: None,
            messages_compacted: Some(ARCHIVED),
            summary_chars: Some(GUIDE.len()),
            active_messages: None,
        },
        Some("snapcompact"),
        Some(GUIDE),
    );
    session.append_stored_message(StoredMessage {
        id: "after-compaction".into(),
        role: Role::User,
        content: text("After compaction".into()).into(),
        attachment_data: None,
        display_role: None,
        timestamp: Some(chrono::Utc::now() + std::time::Duration::from_secs(1)),
        tool_duration_ms: None,
        token_usage: None,
        codemode: None,
        ask: None,
    });
    session
}

pub(super) fn reply(session: &Session, command: &Command) -> Option<Update> {
    let (messages, info) = match command {
        Command::Watch { session_id } if session_id == ID => {
            (session::render_messages(session), None)
        }
        Command::GetCompactedHistory {
            session_id,
            visible_messages,
        } if session_id == ID => {
            let (messages, _, info) = session::render_messages_and_images_with_compacted_history(
                session,
                *visible_messages,
            );
            (messages, info)
        }
        _ => return None,
    };
    let messages = messages
        .into_iter()
        .map(|row| jcode_sdk::HistoryMessage {
            role: row.role,
            content: row.content,
            timestamp_ms: row.timestamp_ms,
            attachments: Vec::new(),
            tool: None,
            response_stats: None,
        })
        .collect();
    let session_id = ID.to_owned();
    Some(match command {
        Command::Watch { .. } => Update::History {
            session_id,
            messages,
            images: Vec::new(),
        },
        _ => {
            let info = info.expect("the fixture has compacted history");
            Update::HistoryPage {
                session_id: session_id.clone(),
                result: Ok(ApiEvent::CompactedHistory {
                    session_id,
                    messages,
                    images: Vec::new(),
                    compacted_total: info.total_messages,
                    compacted_visible: info.visible_messages,
                    compacted_remaining: info.remaining_messages,
                    compacted_hidden_prompts: info.hidden_user_prompts,
                }),
            }
        }
    })
}
