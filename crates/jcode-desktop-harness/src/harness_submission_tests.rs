//! Submission regressions use the real session worker and SDK over socket pairs.

use super::*;
use jcode_sdk::api::ErrorCode;

fn busy_error() -> ApiEvent {
    ApiEvent::Error {
        code: ErrorCode::Internal,
        message: "Already processing a message".into(),
    }
}

#[test]
fn busy_recovery_retains_images_and_never_replays_twice() {
    let images = vec![("image/png".into(), "original image bytes".into())];
    let attachments = vec![jcode_sdk::api::Attachment::PastedText {
        text: "original snippet".into(),
    }];
    let mut unaccepted = VecDeque::from([SessionCommand::Send {
        content: "look at this".into(),
        images: images.clone(),
        attachments: attachments.clone(),
    }]);
    let mut pending = VecDeque::from([SessionCommand::Cancel]);
    assert!(recover_async_busy(
        &busy_error(),
        &mut unaccepted,
        &mut pending
    ));
    assert!(!recover_async_busy(
        &busy_error(),
        &mut unaccepted,
        &mut pending
    ));
    assert!(
        matches!(pending.pop_front(), Some(SessionCommand::Send { content, images: actual, attachments: actual_attachments })
        if content == "look at this" && actual == images && actual_attachments == attachments)
    );
    assert!(matches!(pending.pop_front(), Some(SessionCommand::Cancel)));
}

#[test]
fn ordinary_errors_do_not_trigger_busy_recovery() {
    let mut unaccepted = VecDeque::from([SessionCommand::Send {
        content: "prompt".into(),
        images: vec![],
        attachments: vec![],
    }]);
    let mut pending = VecDeque::new();
    let event = ApiEvent::Error {
        code: ErrorCode::Internal,
        message: "provider rejected image".into(),
    };
    assert!(!recover_async_busy(&event, &mut unaccepted, &mut pending));
    assert_eq!(unaccepted.len(), 1);
    assert!(pending.is_empty());
}

#[cfg(unix)]
mod socket_tests {
    use super::*;
    include!("harness_recovery_tests.rs");
    use jcode_sdk::api::{ApiRequest, ClientFrame, ServerFrame, read_frame, write_frame};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;
    use std::time::Instant;

    struct Transport(UnixStream);
    impl jcode_sdk::Transport for Transport {
        fn split(
            self: Box<Self>,
        ) -> jcode_sdk::Result<(Box<dyn BufRead + Send>, Box<dyn Write + Send>)> {
            let writer = self.0.try_clone().unwrap();
            Ok((Box::new(BufReader::new(self.0)), Box::new(writer)))
        }
    }

    struct Worker {
        commands: Sender<SessionCommand>,
        updates: UpdateReceiver,
        requests: Receiver<ApiRequest>,
        writer: Arc<Mutex<UnixStream>>,
        thread: Option<JoinHandle<()>>,
    }

    impl Worker {
        fn new(on_send: impl FnMut(&ApiRequest) -> Vec<ApiEvent> + Send + 'static) -> Self {
            Self::with_attachments(true, on_send)
        }

        fn with_attachments(
            supports_attachments: bool,
            mut on_send: impl FnMut(&ApiRequest) -> Vec<ApiEvent> + Send + 'static,
        ) -> Self {
            let (client_socket, server_socket) = UnixStream::pair().unwrap();
            let mut reader = BufReader::new(server_socket.try_clone().unwrap());
            let writer = Arc::new(Mutex::new(server_socket));
            let server_writer = writer.clone();
            let (request_tx, requests) = channel();
            std::thread::spawn(move || {
                while let Ok(frame) = read_frame::<_, ClientFrame>(&mut reader) {
                    let event = match &frame.request {
                        ApiRequest::Hello { .. } => ApiEvent::HelloOk {
                            runtime_identity: None,
                            version: jcode_sdk::api::API_VERSION_MAJOR,
                            server: "submission-regression".into(),
                            capabilities: ["queued_prompt_editing", "skill_message_v1"]
                                .into_iter()
                                .chain(
                                    supports_attachments
                                        .then_some(jcode_sdk::api::FILE_ATTACHMENTS_CAPABILITY),
                                )
                                .map(str::to_owned)
                                .collect(),
                        },
                        ApiRequest::GetHistory { .. } => ApiEvent::History {
                            session_id: "s1".into(),
                            messages: vec![],
                            images: vec![],
                        },
                        ApiRequest::GetRuntimeInfo { .. } => ApiEvent::RuntimeInfo {
                            session_id: "s1".into(),
                            provider: None,
                            model: None,
                            routes: vec![],
                            reasoning_effort: None,
                            auth_method: None,
                            account_label: None,
                        },
                        ApiRequest::SendMessage { .. } | ApiRequest::SendSkillMessage { .. } => {
                            let events = on_send(&frame.request);
                            request_tx.send(frame.request).unwrap();
                            for event in events {
                                write_frame(
                                    &mut *server_writer.lock().unwrap(),
                                    &ServerFrame::event(event),
                                )
                                .unwrap();
                            }
                            continue;
                        }
                        ApiRequest::AskResponse { .. } => {
                            request_tx.send(frame.request.clone()).unwrap();
                            ApiEvent::Ok
                        }
                        ApiRequest::SoftInterrupt { .. } => {
                            request_tx.send(frame.request.clone()).unwrap();
                            ApiEvent::Ok
                        }
                        ApiRequest::QueuePrompt { .. } => {
                            request_tx.send(frame.request.clone()).unwrap();
                            on_send(&frame.request)
                                .into_iter()
                                .next()
                                .unwrap_or(ApiEvent::Ok)
                        }
                        ApiRequest::EditQueuedPrompt { .. } => {
                            request_tx.send(frame.request.clone()).unwrap();
                            ApiEvent::Ok
                        }
                        _ => ApiEvent::Ok,
                    };
                    if write_frame(
                        &mut *server_writer.lock().unwrap(),
                        &ServerFrame::reply(frame.id, event),
                    )
                    .is_err()
                    {
                        break;
                    }
                }
            });
            let client = JcodeClient::connect_with(
                Box::new(Transport(client_socket)),
                ConnectOptions {
                    ensure_runtime: false,
                    request_timeout: Some(Duration::from_secs(2)),
                    ..Default::default()
                },
            )
            .unwrap();
            let (commands, command_rx) = channel();
            let (update_tx, updates) = update_channel();
            let thread = std::thread::spawn(move || {
                session_worker("s1".into(), command_rx, update_tx, Some(client))
            });
            let worker = Self {
                commands,
                updates,
                requests,
                writer,
                thread: Some(thread),
            };
            worker.wait_for(|update| {
                matches!(
                    update,
                    Update::Event {
                        event: ApiEvent::RuntimeInfo { .. },
                        ..
                    }
                )
            });
            worker
        }

        fn send(&self, content: &str, images: Vec<(String, String)>) {
            self.commands
                .send(SessionCommand::Send {
                    content: content.into(),
                    images,
                    attachments: vec![],
                })
                .unwrap();
        }

        fn request(&self) -> ApiRequest {
            self.requests
                .recv_timeout(Duration::from_secs(3))
                .expect("worker must submit promptly")
        }

        fn event(&self, event: ApiEvent) {
            write_frame(
                &mut *self.writer.lock().unwrap(),
                &ServerFrame::event(event),
            )
            .unwrap();
        }

        fn wait_for(&self, matches: impl Fn(&Update) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                assert!(
                    Instant::now() < deadline,
                    "worker did not emit expected update"
                );
                if let Ok(update) = self.updates.try_recv() {
                    if matches(&update) {
                        return;
                    }
                    assert!(
                        !matches!(
                            update,
                            Update::SendFailed { .. }
                                | Update::Event {
                                    event: ApiEvent::Error { .. },
                                    ..
                                }
                        ),
                        "unexpected error: {update:?}"
                    );
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }

    #[test]
    fn queued_prompt_rejection_identifies_input_and_reason_over_socket() {
        let worker = Worker::new(|request| {
            if matches!(request, ApiRequest::QueuePrompt { .. }) {
                vec![ApiEvent::Error {
                    code: ErrorCode::Internal,
                    message: "queue storage unavailable".into(),
                }]
            } else {
                vec![]
            }
        });
        worker
            .commands
            .send(SessionCommand::QueuePrompt(jcode_sdk::api::QueuedPrompt {
                active_skill: None,
                client_id: "failed-id".into(),
                content: "keep me".into(),
                images: vec![],
                attachments: vec![],
                mode: jcode_sdk::api::QueuedPromptMode::Steer,
                editing: false,
            }))
            .unwrap();
        assert!(
            matches!(worker.request(), ApiRequest::QueuePrompt { client_id, .. } if client_id == "failed-id")
        );
        worker.wait_for(|update| {
            matches!(update, Update::QueuePromptFailed { client_id, reason, .. }
            if client_id == "failed-id" && reason.contains("queue storage unavailable"))
        });
    }

    impl Drop for Worker {
        fn drop(&mut self) {
            let _ = self.commands.send(SessionCommand::Stop);
            self.thread.take().unwrap().join().unwrap();
            let _ = self
                .writer
                .lock()
                .unwrap()
                .shutdown(std::net::Shutdown::Both);
        }
    }

    fn images() -> Vec<(String, String)> {
        vec![("image/png".into(), "same-image-payload".into())]
    }

    #[test]
    fn old_bridge_does_not_receive_a_prompt_it_would_strip() {
        let worker = Worker::with_attachments(false, |_| vec![]);
        worker
            .commands
            .send(SessionCommand::Send {
                content: "do not send without the attachment".into(),
                images: vec![],
                attachments: vec![jcode_sdk::api::Attachment::PastedText {
                    text: "payload".into(),
                }],
            })
            .unwrap();
        worker.wait_for(|update| matches!(update, Update::SendFailed { reason, .. } if reason.contains("cannot receive file attachments")));
        worker.send("plain prompt", vec![]);
        assert!(
            matches!(worker.request(), ApiRequest::SendMessage { content, .. } if content == "plain prompt")
        );
    }

    #[test]
    fn send_preserves_file_and_pasted_text_attachments_over_socket() {
        let worker = Worker::new(|_| vec![]);
        let attachments = vec![
            jcode_sdk::api::Attachment::File {
                name: "x.rs".into(),
                mime: "text/plain".into(),
                size: 3,
                data: Some("YWJj".into()),
                path: None,
            },
            jcode_sdk::api::Attachment::PastedText {
                text: "snippet".into(),
            },
        ];
        worker
            .commands
            .send(SessionCommand::Send {
                content: "review".into(),
                images: vec![],
                attachments: attachments.clone(),
            })
            .unwrap();
        assert!(
            matches!(worker.request(), ApiRequest::SendMessage { attachments: actual, .. }
            if actual == attachments)
        );
    }

    #[test]
    fn queued_and_edited_prompts_preserve_attachments_over_socket() {
        let worker = Worker::new(|_| vec![]);
        let attachments = vec![jcode_sdk::api::Attachment::PastedText {
            text: "queued snippet".into(),
        }];
        worker
            .commands
            .send(SessionCommand::QueuePrompt(jcode_sdk::api::QueuedPrompt {
                active_skill: None,
                client_id: "q1".into(),
                content: "queued".into(),
                images: vec![],
                attachments: attachments.clone(),
                mode: jcode_sdk::api::QueuedPromptMode::Steer,
                editing: false,
            }))
            .unwrap();
        assert!(
            matches!(worker.request(), ApiRequest::QueuePrompt { attachments: actual, .. }
            if actual == attachments)
        );
        worker
            .commands
            .send(SessionCommand::EditQueuedPrompt(
                "q1".into(),
                "edited".into(),
                vec![],
                attachments.clone(),
            ))
            .unwrap();
        assert!(
            matches!(worker.request(), ApiRequest::EditQueuedPrompt { attachments: actual, .. }
            if actual == attachments)
        );
    }

    #[test]
    fn history_replay_preserves_attachment_metadata_over_socket() {
        let worker = Worker::new(|_| vec![]);
        let attachments = serde_json::json!([
            {"type":"file", "name":"x.rs", "mime":"text/plain", "size":3},
            {"type":"pasted_text", "text":"snippet"}
        ]);
        let event: ApiEvent = serde_json::from_value(serde_json::json!({
            "ev":"history", "session_id":"s1", "messages":[{
                "role":"user", "content":"review", "attachments":attachments
            }]
        }))
        .unwrap();
        worker.event(event);
        worker.wait_for(|update| match update {
            Update::Event {
                event: ApiEvent::History { messages, .. },
                ..
            } => messages.iter().any(|message| {
                message.content == "review"
                    && serde_json::to_value(&message.attachments).unwrap() == attachments
            }),
            _ => false,
        });
    }

    #[test]
    fn busy_send_queues_attachments_as_steering() {
        let worker = Worker::new(|_| vec![]);
        worker.send("first", vec![]);
        assert!(matches!(worker.request(), ApiRequest::SendMessage { .. }));
        let attachments = vec![jcode_sdk::api::Attachment::PastedText {
            text: "follow-up".into(),
        }];
        worker
            .commands
            .send(SessionCommand::Send {
                content: "steer".into(),
                images: vec![],
                attachments: attachments.clone(),
            })
            .unwrap();
        assert!(matches!(worker.request(), ApiRequest::QueuePrompt {
            content, attachments: actual, mode: jcode_sdk::api::QueuedPromptMode::Steer, ..
        } if content == "steer" && actual == attachments));
    }

    #[test]
    fn skill_send_preserves_attachments_over_socket() {
        let worker = Worker::new(|_| vec![]);
        let attachments = vec![jcode_sdk::api::Attachment::PastedText {
            text: "skill snippet".into(),
        }];
        worker
            .commands
            .send(SessionCommand::SendSkill {
                content: "review".into(),
                images: vec![],
                attachments: attachments.clone(),
                skill: "review".into(),
            })
            .unwrap();
        assert!(
            matches!(worker.request(), ApiRequest::SendSkillMessage { attachments: actual, .. }
            if actual == attachments)
        );
    }

    fn assert_steering(
        request: ApiRequest,
        expected: &str,
        expected_images: Vec<(String, String)>,
    ) {
        assert!(
            matches!(request, ApiRequest::SoftInterrupt { session_id, content, images, urgent, .. }
            if session_id == "s1" && content == expected && images == expected_images && urgent)
        );
    }

    #[test]
    fn ask_response_reaches_socket_with_all_fields_and_waits_for_ack() {
        let worker = Worker::new(|_| vec![]);
        let response = jcode_sdk::AskResponse {
            answers: Some(vec![jcode_sdk::AskAnswer {
                selected: vec!["B".into()],
                other: Some("own answer".into()),
                comments: [("B".into(), "why".into())].into_iter().collect(),
                ..Default::default()
            }]),
        };
        worker
            .commands
            .send(SessionCommand::AskResponse {
                request_id: "ask-1".into(),
                response,
            })
            .unwrap();
        assert!(
            matches!(worker.request(), ApiRequest::AskResponse { session_id, request_id, response }
            if session_id == "s1" && request_id == "ask-1" && response.answers.as_ref().is_some_and(|answers| answers[0].comments.get("B").map(String::as_str) == Some("why") && answers[0].other.as_deref() == Some("own answer")))
        );
        worker.wait_for(|update| matches!(update, Update::AskResponseSettled { request_id, error: None, .. } if request_id == "ask-1"));
    }

    #[test]
    fn repeated_image_followups_use_steering_without_duplicate_normal_sends() {
        let worker = Worker::new(|_| {
            vec![ApiEvent::MessageAccepted {
                session_id: "s1".into(),
            }]
        });
        worker.send("first image", images());
        worker.send("same image again", images());
        worker.send("text followup", vec![]);
        assert!(
            matches!(worker.request(), ApiRequest::SendMessage { content, images: actual, .. }
            if content == "first image" && actual == images())
        );
        assert_steering(worker.request(), "same image again", images());
        assert_steering(worker.request(), "text followup", vec![]);
        worker.wait_for(|update| {
            matches!(
                update,
                Update::Event {
                    event: ApiEvent::MessageAccepted { .. },
                    ..
                }
            )
        });
        assert!(worker.requests.try_recv().is_err());
    }

    #[test]
    fn async_busy_rejection_retries_image_and_text_through_steering() {
        for payload in [images(), vec![]] {
            let worker = Worker::new(|_| vec![busy_error()]);
            worker.send("race followup", payload.clone());
            assert!(matches!(worker.request(), ApiRequest::SendMessage { .. }));
            assert_steering(worker.request(), "race followup", payload);
            // A fence confirms the event loop handled the rejection without
            // forwarding it to the transcript or issuing another normal send.
            worker.event(ApiEvent::TextDelta {
                message_id: None,
                session_id: "s1".into(),
                text: "continued".into(),
            });
            worker.wait_for(|update| {
                matches!(
                    update,
                    Update::Event {
                        event: ApiEvent::TextDelta { .. },
                        ..
                    }
                )
            });
            assert!(worker.requests.try_recv().is_err());
        }
    }

    #[test]
    fn acknowledged_steering_is_not_replayed_after_a_later_busy_rejection() {
        let mut sends = 0;
        let worker = Worker::new(move |_| {
            sends += 1;
            if sends == 1 {
                vec![ApiEvent::MessageAccepted {
                    session_id: "s1".into(),
                }]
            } else {
                vec![busy_error()]
            }
        });
        worker.send("initial", vec![]);
        assert!(matches!(worker.request(), ApiRequest::SendMessage { .. }));
        worker.wait_for(|update| {
            matches!(
                update,
                Update::Event {
                    event: ApiEvent::MessageAccepted { .. },
                    ..
                }
            )
        });
        worker.send("already delivered", images());
        assert_steering(worker.request(), "already delivered", images());
        worker.event(ApiEvent::TurnDone {
            session_id: "s1".into(),
            pending_soft_interrupts: None,
        });
        worker.wait_for(|update| {
            matches!(
                update,
                Update::Event {
                    event: ApiEvent::TurnDone { .. },
                    ..
                }
            )
        });
        worker.send("new followup", images());
        assert!(
            matches!(worker.request(), ApiRequest::SendMessage { content, .. } if content == "new followup")
        );
        assert_steering(worker.request(), "new followup", images());
        worker.event(ApiEvent::TextDelta {
            message_id: None,
            session_id: "s1".into(),
            text: "continued".into(),
        });
        worker.wait_for(|update| {
            matches!(
                update,
                Update::Event {
                    event: ApiEvent::TextDelta { .. },
                    ..
                }
            )
        });
        assert!(worker.requests.try_recv().is_err());
    }
}
