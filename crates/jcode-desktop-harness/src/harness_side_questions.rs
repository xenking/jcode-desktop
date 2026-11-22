use super::{
    JcodeClient, SessionInfo, SideQuestionThread, SideQuestionTopic, SideQuestionTurn, connect,
    remote, transport,
};

pub(super) fn read<T>(
    transports: &transport::RemoteTransports,
    parent_session_id: &str,
    read: impl FnOnce(&JcodeClient, &remote::SessionAddress) -> Result<T, String>,
) -> Result<T, String> {
    let parent = remote::SessionAddress::parse(parent_session_id)?;
    let client = match parent.host.as_deref() {
        Some(host) => transports.connect(host),
        None => connect("side-question-history"),
    }
    .map_err(|error| error.to_string())?;
    if !client.supports(jcode_sdk::SIDE_QUESTIONS_CAPABILITY) {
        return Err("Side question history requires an updated runtime and bridge.".into());
    }
    read(&client, &parent)
}

pub(super) fn list(
    client: &JcodeClient,
    parent: &remote::SessionAddress,
) -> Result<Vec<SideQuestionTopic>, String> {
    let topics = client
        .list_side_questions(&parent.session_id)
        .map_err(|error| error.to_string())?;
    Ok(topics
        .into_iter()
        .filter(|topic| topic.message_start.is_some())
        .map(|topic| {
            let question = client
                .get_side_question_history(&topic.session_id)
                .ok()
                .and_then(|messages| messages.into_iter().find(|message| message.role == "user"))
                .map(|message| one_line_preview(&message.content))
                .unwrap_or_else(|| "Untitled side question".into());
            SideQuestionTopic {
                session_id: parent.ui_id(&topic.session_id),
                question,
            }
        })
        .collect())
}

fn one_line_preview(question: &str) -> String {
    question
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect()
}

pub(super) fn open(
    client: &JcodeClient,
    parent: &remote::SessionAddress,
    session_id: &str,
) -> Result<SideQuestionThread, String> {
    let child = remote::SessionAddress::parse(session_id)?;
    let topic = client
        .list_side_questions(&parent.session_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|topic| parent.host == child.host && topic.session_id == child.session_id)
        .ok_or("This side question no longer belongs to this conversation.")?;
    let history = client
        .get_side_question_history(&child.session_id)
        .map_err(|error| error.to_string())?;
    let mut turns: Vec<SideQuestionTurn> = Vec::new();
    for message in history {
        match message.role.as_str() {
            "user" if !message.content.trim().is_empty() => turns.push(SideQuestionTurn {
                question: message.content,
                answer: String::new(),
            }),
            "assistant" if !message.content.trim().is_empty() => {
                if let Some(turn) = turns.last_mut() {
                    if !turn.answer.is_empty() {
                        turn.answer.push_str("\n\n");
                    }
                    turn.answer.push_str(&message.content);
                }
            }
            _ => {}
        }
    }
    let session = SessionInfo {
        session_id: session_id.to_string(),
        working_dir: None,
        title: turns.first().map(|turn| one_line_preview(&turn.question)),
        status: "idle".into(),
        parent_session_id: None,
        agent_label: None,
        swarm_status: None,
        context: topic.context,
        transcript_bytes: None,
        user_connected: false,
        edit_stats: None,
        saved: false,
        save_label: None,
        updated_at_ms: None,
        last_active_at_ms: None,
        archived: false,
        archived_at_ms: None,
    };
    Ok(SideQuestionThread { session, turns })
}
