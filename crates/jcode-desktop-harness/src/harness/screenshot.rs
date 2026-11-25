//! Offline fixture bridge for screenshots and UI acceptance runs.

mod history;

use std::sync::mpsc::Receiver;

use jcode_sdk::{ApiEvent, SessionInfo};

use crate::harness::catalog::persisted_todo_snapshot;
use crate::harness::protocol::{Command, SESSION_IMAGE_LIST_KEY, SessionOperation, Update};
use crate::harness::updates::UpdateSender;

pub const GALLERY_FIXTURE_IMAGES: usize = 96;

pub fn gallery_fixture_image_id(index: usize) -> String {
    format!("img_{index:032x}")
}

pub fn composer_picker_fixture_runtime(
    session_id: &str,
    selection: Option<(&str, Option<String>, jcode_sdk::api::SpeedMode)>,
) -> ApiEvent {
    use jcode_sdk::api::{SpeedAvailability, SpeedMode, SpeedOption};

    let (model, reasoning_effort, speed_mode) =
        selection.unwrap_or(("gpt-6.2-astra", None, SpeedMode::Standard));
    let routes: Vec<_> = ["gpt-6.2-astra", "gpt-6.2-sol"]
        .into_iter()
        .map(|model| jcode_sdk::ModelRouteInfo {
            model: model.into(),
            reasoning_efforts: None,
            provider: "openai".into(),
            api_method: "openai-oauth".into(),
            available: true,
            detail: String::new(),
            usage: None,
            speed_options: [SpeedMode::Fast, SpeedMode::Ultrafast]
                .into_iter()
                .filter(|mode| model == "gpt-6.2-astra" || *mode == SpeedMode::Ultrafast)
                .map(|mode| SpeedOption {
                    mode,
                    availability: SpeedAvailability::Available,
                    reason: None,
                })
                .collect(),
        })
        .collect();
    let speed_options = routes
        .iter()
        .find(|route| route.model == model)
        .map_or_else(Vec::new, |route| route.speed_options.clone());
    ApiEvent::RuntimeInfo {
        session_id: session_id.into(),
        provider: Some("openai".into()),
        model: Some(model.into()),
        auth_method: Some("openai-oauth".into()),
        account_label: None,
        reasoning_effort,
        routes,
        speed_mode,
        speed_options,
    }
}

/// A bridge that answers from fixtures, so the screenshot harness can
/// render the results screen without a daemon, an index, or a network.
pub(super) fn run(update_tx: UpdateSender, command_rx: Receiver<Command>) {
    let history = history::enabled().then(history::session);
    let picker = std::env::var("JCODE_DESKTOP_CONTROL_TEST").as_deref() == Ok("1")
        && std::env::var("JCODE_DESKTOP_SCREENSHOT_COMPOSER_PICKER").as_deref() == Ok("model");
    let mut picker_model = "gpt-6.2-astra".to_owned();
    let mut picker_effort = None;
    let mut picker_speed = jcode_sdk::api::SpeedMode::Standard;
    let gallery = std::env::var("JCODE_DESKTOP_CONTROL_TEST").as_deref() == Ok("1")
        && std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref() == Ok("gallery-preload");
    let draft = std::env::var("JCODE_DESKTOP_CONTROL_TEST").as_deref() == Ok("1")
        && std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref() == Ok("draft-session");
    if draft {
        let routes = ["gpt-5.6-terra", "gpt-5.6-sol"]
            .into_iter()
            .map(|model| jcode_sdk::ModelRouteInfo {
                speed_options: Vec::new(),
                reasoning_efforts: None,
                model: model.into(),
                provider: "openai".into(),
                api_method: "openai-api".into(),
                available: true,
                detail: String::new(),
                usage: None,
            })
            .collect();
        let _ = update_tx.send(Update::DraftModels {
            current: Some("gpt-5.6-terra".into()),
            routes,
        });
    }
    while let Ok(command) = command_rx.recv() {
        if picker {
            let mut speed_serial = None;
            let session_id = match &command {
                Command::SessionOperation {
                    session_id,
                    operation,
                } => match operation {
                    SessionOperation::SetModelWithSpeed {
                        model,
                        effort,
                        serial,
                        mode,
                    } => {
                        picker_model = model
                            .split_once(':')
                            .map_or(model.as_str(), |(_, bare)| bare)
                            .into();
                        picker_effort = effort.clone();
                        picker_speed = *mode;
                        speed_serial = Some(*serial);
                        Some(session_id)
                    }
                    SessionOperation::SetSpeed { serial, mode } => {
                        picker_speed = *mode;
                        speed_serial = Some(*serial);
                        Some(session_id)
                    }
                    SessionOperation::SetEffort(effort) => {
                        picker_effort = Some(effort.clone());
                        Some(session_id)
                    }
                    _ => None,
                },
                Command::SetModel { session_id, model } => {
                    picker_model = model
                        .split_once(':')
                        .map_or(model.as_str(), |(_, bare)| bare)
                        .into();
                    picker_speed = jcode_sdk::api::SpeedMode::Standard;
                    Some(session_id)
                }
                _ => None,
            };
            if let Some(session_id) = session_id {
                let event = composer_picker_fixture_runtime(
                    session_id,
                    Some((&picker_model, picker_effort.clone(), picker_speed)),
                );
                let _ = update_tx.send(Update::Event {
                    session_id: session_id.clone(),
                    event,
                });
                if let Some(serial) = speed_serial {
                    let _ = update_tx.send(Update::SpeedSettled {
                        session_id: session_id.clone(),
                        serial,
                        result: Ok(picker_speed),
                    });
                }
                continue;
            }
        }
        if gallery && let Some(update) = gallery_fixture_reply(&command) {
            let _ = update_tx.send(update);
            continue;
        }
        if draft
            && let Command::CreateSession {
                working_dir,
                request_id,
            } = &command
        {
            let session: SessionInfo = serde_json::from_value(serde_json::json!({
                "session_id": "session_verifier_draft", "working_dir": working_dir,
                "title": "Native draft verification", "status": "idle"
            }))
            .expect("draft fixture session");
            let session_id = session.session_id.clone();
            let _ = update_tx.send(Update::SessionCreated {
                session,
                request_id: request_id.clone(),
            });
            let _ = update_tx.send(Update::History {
                session_id,
                messages: vec![],
                images: vec![],
            });
            continue;
        }
        if draft {
            match &command {
                Command::Send { session_id, .. } => {
                    let _ = update_tx.send(Update::Event {
                        session_id: session_id.clone(),
                        event: ApiEvent::SessionStatus {
                            session_id: session_id.clone(),
                            status: "running_tools".into(),
                            pending_soft_interrupts: None,
                        },
                    });
                    continue;
                }
                Command::SideQuestion {
                    session_id,
                    question,
                } => {
                    let child = "session_verifier_side_question";
                    let session = serde_json::from_value(serde_json::json!({
                        "session_id": child, "status": "idle"
                    }))
                    .expect("side question fixture session");
                    let _ = update_tx.send(Update::SideQuestionStarted {
                        parent_session_id: session_id.clone(),
                        session,
                        question: question.clone(),
                    });
                    for event in [
                        ApiEvent::TextDelta {
                            session_id: child.into(),
                            message_id: None,
                            text: "The retry policy uses a short backoff.".into(),
                        },
                        ApiEvent::TurnDone {
                            session_id: child.into(),
                            pending_soft_interrupts: None,
                        },
                    ] {
                        let _ = update_tx.send(Update::Event {
                            session_id: child.into(),
                            event,
                        });
                    }
                    continue;
                }
                Command::ListSideQuestions {
                    parent_session_id,
                    request_id,
                } => {
                    let _ = update_tx.send(Update::SideQuestionHistory {
                        parent_session_id: parent_session_id.clone(),
                        request_id: *request_id,
                        result: Ok(Vec::new()),
                    });
                    continue;
                }
                _ => {}
            }
        }
        if let Command::ReadTodoSnapshot { session_id, reply } = &command {
            let _ = reply.try_send(persisted_todo_snapshot(session_id));
            continue;
        }
        if let Some(update) = history
            .as_ref()
            .and_then(|session| history::reply(session, &command))
        {
            let _ = update_tx.send(update);
            continue;
        }
        let Command::SearchSessions { query, generation } = command else {
            continue;
        };
        let started = std::time::Instant::now();
        let _ = update_tx.send(Update::SessionSearch {
            query: query.clone(),
            generation,
            result: Ok(screenshot_search_results(&query)),
            elapsed_ms: started.elapsed().as_secs_f64() * 1_000.0,
        });
    }
}

pub(super) fn gallery_fixture_reply(command: &Command) -> Option<Update> {
    use base64::Engine as _;

    let (session_id, key, result) = match command {
        Command::ListSessionImages { session_id } => {
            let metadata: Vec<_> = (0..GALLERY_FIXTURE_IMAGES)
                .map(|index| {
                    serde_json::json!({
                        "image_id": gallery_fixture_image_id(index),
                        "message_index": index,
                        "media_type": "image/png"
                    })
                })
                .collect();
            (
                session_id,
                SESSION_IMAGE_LIST_KEY.into(),
                Ok(serde_json::to_string(&metadata).unwrap()),
            )
        }
        Command::GetHistoryImage { session_id, key } => {
            let index = key
                .strip_prefix("img_")
                .and_then(|id| usize::from_str_radix(id, 16).ok());
            let index = index.filter(|&index| index < GALLERY_FIXTURE_IMAGES)?;
            eprintln!("desktop-gallery-fixture read index={index}");
            let result = if index == 47 {
                Err("fixture image unavailable".into())
            } else {
                Ok(
                    base64::engine::general_purpose::STANDARD.encode(include_bytes!(
                        "../../../../assets/previews/image-preview.png"
                    )),
                )
            };
            (session_id, key.clone(), result)
        }
        Command::Send { .. } => {
            eprintln!("desktop-gallery-fixture unexpected prompt submitted");
            return None;
        }
        _ => return None,
    };
    Some(Update::HistoryImage {
        session_id: session_id.clone(),
        key,
        result,
    })
}

/// Hit rows for the screenshot: several sessions, a user prompt whose raw
/// snippet is full of newlines, assistant answers with their models, and a
/// title match.
pub(super) fn screenshot_search_results(query: &str) -> jcode_sdk::SessionSearchResults {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default();
    let minute = 60_000i64;
    let highlight = |start: u32, end: u32| jcode_sdk::SessionSearchHighlight { start, end };
    let prompt = "Can you make the global search results readable?\n\nThe rows currently spread over four lines, and the match lands far to the right of the text that introduces it, which looks broken.";
    let prompt_start = prompt.find("readable").unwrap_or_default() as u32;
    let answer = "Done. Each hit is now one line: the snippet is whitespace-collapsed and windowed about forty characters before the match, with ellipses on both sides.";
    let answer_start = answer.find("whitespace-collapsed").unwrap_or_default() as u32;
    jcode_sdk::SessionSearchResults {
        query: query.to_string(),
        titles: vec![jcode_sdk::SessionSearchHit {
            session_id: "session_swarm_1794742000000_aaaaaaaaaaaa".into(),
            title: "Global search: one line per hit".into(),
            role: jcode_sdk::SessionSearchRole::Title,
            model: None,
            snippet: "Global search: one line per hit".into(),
            highlights: vec![highlight(0, 6)],
            timestamp_ms: now - 3 * minute,
            message_id: None,
        }],
        messages: vec![
            jcode_sdk::SessionSearchHit {
                session_id: "session_swarm_1794742000000_aaaaaaaaaaaa".into(),
                title: "Global search: one line per hit".into(),
                role: jcode_sdk::SessionSearchRole::User,
                model: None,
                snippet: prompt.into(),
                highlights: vec![highlight(prompt_start, prompt_start + 8)],
                timestamp_ms: now - 3 * 60 * minute,
                message_id: Some("message_1".into()),
            },
            jcode_sdk::SessionSearchHit {
                session_id: "session_swarm_1794742000000_aaaaaaaaaaaa".into(),
                title: "Global search: one line per hit".into(),
                role: jcode_sdk::SessionSearchRole::Assistant,
                model: Some("claude-opus-5-5".into()),
                snippet: answer.into(),
                highlights: vec![highlight(answer_start, answer_start + 20)],
                timestamp_ms: now - 2 * 60 * minute,
                message_id: Some("message_2".into()),
            },
            jcode_sdk::SessionSearchHit {
                session_id: "session_otter_1794720000000_bbbbbbbbbbbb".into(),
                title: "Indexer: classify by message source".into(),
                role: jcode_sdk::SessionSearchRole::Assistant,
                model: Some("deepseek-v4.1-flash".into()),
                snippet: "The indexer now classifies a user-role message by its source, so an auto-poke or a swarm report is no longer attributed to you.".into(),
                highlights: vec![highlight(26, 34)],
                timestamp_ms: now - 5 * 3_600_000,
                message_id: Some("message_3".into()),
            },
            jcode_sdk::SessionSearchHit {
                session_id: "session_heron_1794700000000_cccccccccccc".into(),
                title: "Titlebar inset for full-height screens".into(),
                role: jcode_sdk::SessionSearchRole::User,
                model: None,
                snippet: "The search screen draws under the traffic lights, so the heading collides with the red and yellow buttons.".into(),
                highlights: vec![highlight(4, 10)],
                timestamp_ms: now - 27 * 3_600_000,
                message_id: Some("message_4".into()),
            },
        ],
        indexed_sessions: 214,
        truncated: false,
    }
}
