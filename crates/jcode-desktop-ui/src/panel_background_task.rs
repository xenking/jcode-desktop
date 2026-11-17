//! Presentation of background task completions.

pub(super) fn summary_parts(summary: &str) -> (String, Option<i32>) {
    let mut exit_code = None;
    let parts = summary
        .split('·')
        .map(str::trim)
        .filter(|part| {
            if let Some(code) = part
                .strip_prefix("exit ")
                .and_then(|n| n.parse::<i32>().ok())
            {
                exit_code = (code != 0).then_some(code);
                false
            } else {
                !part.is_empty() && *part != "exit n/a"
            }
        })
        .collect::<Vec<_>>();
    (parts.join(" · "), exit_code)
}

pub(super) fn detail_text(_task_id: &str, _summary: &str, detail: Option<&str>) -> String {
    let Some(detail) = detail else {
        return String::new();
    };
    if let Some((_, rest)) = detail.split_once("```text\n")
        && let Some((output, _)) = rest.rsplit_once("\n```")
    {
        return super::strip_ansi(output);
    }
    String::new()
}
