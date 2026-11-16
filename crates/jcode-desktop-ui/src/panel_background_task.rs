//! Text accompanying a background task completion.

/// Keep the task identifier and status even with an older server that does not
/// send the completion notice. A new server also supplies the bounded preview.
pub(super) fn detail_text(task_id: &str, summary: &str, detail: Option<&str>) -> String {
    let mut text = format!("Task: {task_id}\n{summary}");
    if let Some(body) = detail.and_then(|text| text.split_once('\n').map(|(_, body)| body.trim()))
        && !body.is_empty()
    {
        text.push_str("\n\n");
        if let Some((before, preview)) = body.split_once("```text\n") {
            if let Some((preview, after)) = preview.rsplit_once("\n```") {
                text.push_str(before);
                text.push_str("Output preview:\n");
                text.push_str(preview);
                text.push_str(after);
            } else {
                text.push_str(body);
            }
        } else {
            text.push_str(body);
        }
    }
    text
}
