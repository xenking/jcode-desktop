//! Protocol history to transcript items. A pure translation: it reads no
//! panel, so the paged-in history, the provisional prefix and the demo
//! fixtures all build rows the same way.
use super::*;

/// Causes older daemons wrote for settings the user changed himself, deferred
/// forms included ("you chose it while the agent was busy; …"). A current
/// daemon never stores or sends such rows; a prefix keeps older sessions quiet.
const LEGACY_USER_SETTING_CAUSES: [&str; 3] = [
    "you chose it",
    "you changed the mode",
    "chosen during the previous turn, applied at the next request",
];

fn is_user_setting_row(content: &str) -> bool {
    content
        .strip_suffix(')')
        .and_then(|row| row.rsplit_once(" ("))
        .is_some_and(|(_, cause)| {
            LEGACY_USER_SETTING_CAUSES
                .iter()
                .any(|user| cause.starts_with(user))
        })
}

/// Whether the notice rows ending the transcript already say `message`.
pub(super) fn notice_in_trailing_run(items: &[Item], message: &str) -> bool {
    items
        .iter()
        .rev()
        .take_while(|item| matches!(item, Item::SwarmWorkerNotice(_)))
        .any(|item| matches!(item, Item::SwarmWorkerNotice(held) if held == message))
}

/// Convert protocol history into transcript items. Shared by the
/// authoritative reply and the provisional on-disk prefix so both paint
/// identically.
pub(super) fn build(
    messages: Vec<jcode_sdk::HistoryMessage>,
    images: Vec<jcode_sdk::RenderedImage>,
) -> (Vec<Item>, HashMap<usize, i64>) {
    let mut user_times = HashMap::new();
    let mut images_by_prompt: HashMap<usize, Vec<jcode_sdk::RenderedImage>> = HashMap::new();
    let mut images_by_message: HashMap<usize, Vec<jcode_sdk::RenderedImage>> = HashMap::new();
    let mut trailing_images = Vec::new();
    for image in images {
        if matches!(image.source, jcode_sdk::RenderedImageSource::UserInput) {
            if let Some(jcode_sdk::RenderedImageAnchor::UserPrompt { ordinal }) = image.anchor {
                images_by_prompt.entry(ordinal).or_default().push(image);
            }
            continue;
        }
        if let Some(index) = image.history_message_index {
            // The boundary counts protocol messages, including hidden tool
            // rows, rather than visible desktop transcript items.
            images_by_message
                .entry(index.min(messages.len()))
                .or_default()
                .push(image);
            continue;
        }
        match &image.anchor {
            Some(jcode_sdk::RenderedImageAnchor::UserPrompt { ordinal }) => {
                images_by_prompt.entry(*ordinal).or_default().push(image);
            }
            _ => trailing_images.push(image),
        }
    }
    let mut items = Vec::with_capacity(messages.len() + trailing_images.len());
    let mut user_ordinal = 0;
    let mut assistant_before_tool = None;
    let message_count = messages.len();
    for (index, message) in messages.into_iter().enumerate() {
        if let Some(images) = images_by_message.remove(&index) {
            items.extend(
                images
                    .into_iter()
                    .map(|image| Item::Image(TranscriptImage::from_rendered(image))),
            );
        }
        match message.role.as_str() {
            "user" => {
                assistant_before_tool = None;
                let prompt = agent_message::history_user_item(
                    message.content,
                    !message.attachments.is_empty() || images_by_prompt.contains_key(&user_ordinal),
                    message.timestamp_ms,
                );
                if let Some(at) = message.timestamp_ms
                    && matches!(prompt, Item::User(_))
                {
                    user_times.insert(items.len(), at);
                }
                let is_prompt = classify::is_pinnable_prompt(&prompt);
                items.push(prompt);
                if let Some(images) = images_by_prompt.remove(&user_ordinal)
                    && is_prompt
                {
                    items.extend(
                        images
                            .into_iter()
                            .map(|image| Item::Image(TranscriptImage::from_rendered(image))),
                    );
                }
                user_ordinal += 1;
            }
            "assistant" => {
                assistant_before_tool = None;
                if !message.content.trim().is_empty() {
                    if message.response_stats.is_none() {
                        assistant_before_tool = Some(items.len());
                    }
                    items.push(Item::history_assistant(
                        message.content,
                        message.response_stats.is_some(),
                    ));
                }
                if let Some(stats) = message.response_stats {
                    let mut stats: response_stats::ResponseStats = stats.into();
                    stats.at_ms = message.timestamp_ms;
                    if !stats.is_empty() {
                        items.push(Item::ResponseStats(stats));
                    }
                }
            }
            "tool" => {
                if let Some(index) = assistant_before_tool.take()
                    && message.tool.is_some()
                    && let Item::Assistant(text) = &mut items[index]
                {
                    items[index] = Item::AssistantOutput {
                        text: std::mem::take(text),
                        phase: AssistantOutputPhase::Update,
                        stream_id: None,
                    };
                }
                if let Some(tool) = message.tool {
                    if tool.name == "ask"
                        && let Some(at) = message.timestamp_ms
                    {
                        user_times.insert(items.len(), at);
                    }
                    items.push(Item::Tool {
                        ask: ask::resolve_record(&tool.name, tool.ask.as_deref(), &message.content),
                        call_id: tool.id,
                        name: tool.name,
                        input: tool.input.to_string(),
                        output: message.content,
                        done: true,
                        error: None,
                    });
                }
            }
            // A stored compaction divider. Its body already carries the
            // label and the expandable detail, so it renders identically
            // before and after a reload.
            "compaction" => {
                if !message.content.trim().is_empty() {
                    items.push(Item::Compaction(compaction::CompactionRow::complete(
                        message.content,
                    )));
                }
            }
            // Setting changes share the compact notice row.
            "setting_change" if is_user_setting_row(&message.content) => {}
            "swarm_worker_notice" | "setting_change" if !message.content.trim().is_empty() => {
                items.push(Item::SwarmWorkerNotice(message.content));
            }
            // A stored automatic memory injection. Like the compaction
            // divider, its body already carries the label and the
            // expandable detail, so the reloaded row matches the live one.
            "memory" => {
                if !message.content.trim().is_empty() {
                    items.push(Item::Memory(compaction::CompactionRow::complete(
                        message.content,
                    )));
                }
            }
            _ => {}
        }
    }
    if let Some(images) = images_by_message.remove(&message_count) {
        trailing_images.splice(0..0, images);
    }
    items.extend(
        trailing_images
            .into_iter()
            .map(|image| Item::Image(TranscriptImage::from_rendered(image))),
    );
    (items, user_times)
}
