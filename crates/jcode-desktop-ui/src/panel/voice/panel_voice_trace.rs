//! What Jev saw and decided for one voice request, and the numbers derived
//! from it: ranked action pills and the estimated cost. Plain data and pure
//! functions, so the panel's views only draw what is computed here.
use jcode_base::voice_intent::{self, SessionCandidate};
use jcode_voice as voice;
use std::time::Duration;

/// Ephemeral routing evidence. Never included in persisted panel snapshots.
#[derive(Clone)]
pub(crate) struct VoiceTrace {
    pub(super) transcript: String,
    pub(super) candidates: Vec<SessionCandidate>,
    pub(super) questions: Vec<voice_intent::VoiceQuestion>,
    pub(super) answers: Vec<voice_intent::VoiceAnswer>,
    /// Streamed microphone audio, the Nari billing unit. Measured locally.
    pub(super) audio: Option<Duration>,
    /// Provider-reported Jev usage. `None` while routing or when unreported.
    pub(super) usage: Option<voice_intent::VoiceUsage>,
}

// Keep the shared routing argmax order, including numeric (not lexical) candidates.
fn concrete_action_rank(id: &str) -> Option<usize> {
    match id {
        "coding_agent" => Some(0),
        "new_session" => Some(1),
        "next_session" => Some(2),
        "previous_session" => Some(3),
        _ => id
            .strip_prefix("candidate_")?
            .parse::<usize>()
            .ok()?
            .checked_add(4),
    }
}

fn action_label(id: &str) -> String {
    match id {
        "coding_agent" => "Coding agent".into(),
        "new_session" => "New conversation".into(),
        "next_session" => "Next conversation".into(),
        "previous_session" => "Previous conversation".into(),
        _ => id.into(),
    }
}

pub(super) struct ActionPill {
    pub(super) id: String,
    pub(super) label: String,
    pub(super) score: String,
    pub(super) probability: Option<f64>,
    pub(super) winner: bool,
}

/// Concrete actions ranked by Jev's yes probability, highest first. Exact ties
/// and unanswered rows keep the shared argmax order, so rank 1 is always the
/// route that was actually selected.
pub(super) fn action_pills(trace: &VoiceTrace, waiting: bool, failed: bool) -> Vec<ActionPill> {
    let mut actions: Vec<_> = trace
        .questions
        .iter()
        .filter_map(|question| concrete_action_rank(&question.id).map(|rank| (rank, question)))
        .collect();
    actions.sort_by_key(|(rank, _)| *rank);
    let mut winner = None;
    let mut best = f64::NEG_INFINITY;
    if !waiting && !failed {
        for (_, question) in &actions {
            if let Some(answer) = trace.answers.iter().find(|answer| answer.id == question.id) {
                let probability = answer.probability;
                if probability.is_finite() && probability > best {
                    best = probability;
                    winner = Some(question.id.as_str());
                }
            }
        }
    }
    let mut pills: Vec<_> = actions
        .into_iter()
        .map(|(_, question)| {
            let probability = trace
                .answers
                .iter()
                .find(|answer| answer.id == question.id)
                .map(|answer| answer.probability)
                .filter(|probability| probability.is_finite());
            let score = probability
                .map(|probability| format!("{:.1}%", probability * 100.))
                .unwrap_or_else(|| {
                    if waiting {
                        "Waiting…"
                    } else {
                        "Not returned"
                    }
                    .into()
                });
            ActionPill {
                id: question.id.clone(),
                label: question
                    .id
                    .strip_prefix("candidate_")
                    .and_then(|index| index.parse::<usize>().ok())
                    .and_then(|index| trace.candidates.get(index))
                    .map(|candidate| format!("Open: {}", candidate.title))
                    .unwrap_or_else(|| action_label(&question.id)),
                score,
                probability,
                winner: winner == Some(question.id.as_str()),
            }
        })
        .collect();
    // Stable sort: equal and missing probabilities retain argmax order.
    pills.sort_by(|a, b| {
        b.probability
            .unwrap_or(f64::NEG_INFINITY)
            .total_cmp(&a.probability.unwrap_or(f64::NEG_INFINITY))
    });
    pills
}

/// Compact USD for sub-cent amounts, keeping two significant digits.
fn format_usd(amount: f64) -> String {
    if !amount.is_finite() || amount <= 0. {
        return "$0".into();
    }
    if amount >= 0.01 {
        return format!("${amount:.4}");
    }
    let decimals = ((-amount.log10()).ceil() as usize + 1).min(10);
    format!("${amount:.decimals$}")
}

fn format_tokens(tokens: u64) -> String {
    let digits = tokens.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Context fed to Jev and the estimated cost of this voice request.
/// Prices are published list rates, not a bill, so every value says "est.".
pub(super) fn cost_lines(trace: &VoiceTrace, waiting: bool) -> Vec<(&'static str, String, String)> {
    let transcription = trace.audio.map(voice::estimated_transcription_usd);
    let jev = trace.usage.map(|usage| usage.estimated_usd());
    let context = match trace.usage {
        Some(usage) => (
            format!(
                "{} tokens in · {} out{}",
                format_tokens(usage.input_tokens),
                format_tokens(usage.output_tokens),
                if usage.requests > 1 {
                    format!(" · {} requests", usage.requests)
                } else {
                    String::new()
                }
            ),
            jev.map(format_usd).unwrap_or_default(),
        ),
        None if waiting => ("Measuring…".into(), String::new()),
        None => ("Not reported".into(), String::new()),
    };
    let audio = match trace.audio {
        Some(audio) => (
            format!("{:.1}s audio · Nari", audio.as_secs_f64()),
            transcription.map(format_usd).unwrap_or_default(),
        ),
        None => ("Not measured".into(), String::new()),
    };
    let total = match (jev, transcription) {
        (Some(jev), Some(transcription)) => format_usd(jev + transcription),
        (None, Some(transcription)) => format!("{}+", format_usd(transcription)),
        (Some(jev), None) => format!("{}+", format_usd(jev)),
        (None, None) => "—".into(),
    };
    vec![
        ("Jev context", context.0, context.1),
        ("Transcription", audio.0, audio.1),
        ("Total (est.)", String::new(), total),
    ]
}

/// One-line cost summary: Jev context tokens and cost, audio and Nari cost, total.
pub(super) fn cost_summary(trace: &VoiceTrace, waiting: bool) -> String {
    let jev = match trace.usage {
        Some(usage) => format!(
            "{} tok {}",
            format_tokens(usage.input_tokens),
            format_usd(usage.estimated_usd())
        ),
        None if waiting => "Jev …".into(),
        None => "Jev n/a".into(),
    };
    let audio = trace.audio.map_or_else(
        || "audio n/a".into(),
        |audio| {
            format!(
                "{:.1}s {}",
                audio.as_secs_f64(),
                format_usd(voice::estimated_transcription_usd(audio))
            )
        },
    );
    let total = cost_lines(trace, waiting)[2].2.clone();
    format!("{jev} · {audio} · {total} est.")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn trace_fixture() -> VoiceTrace {
        let candidates = (0..20)
            .map(|index| SessionCandidate {
                id: format!("session-{index}"),
                title: format!("Conversation {index}"),
                working_dir: None,
            })
            .collect::<Vec<_>>();
        VoiceTrace {
            transcript: "Open a conversation".into(),
            questions: voice_intent::describe_questions("Open a conversation", &candidates)
                .unwrap(),
            answers: Vec::new(),
            candidates,
            audio: None,
            usage: None,
        }
    }

    #[test]
    fn costs_report_context_tokens_and_published_prices() {
        let mut trace = trace_fixture();
        let lines = cost_lines(&trace, true);
        assert_eq!(lines[0].1, "Measuring…");
        assert_eq!(lines[1].1, "Not measured");
        assert_eq!(lines[2].2, "—");
        trace.audio = Some(Duration::from_secs(30));
        trace.usage = Some(voice_intent::VoiceUsage {
            input_tokens: 12_345,
            output_tokens: 27,
            requests: 2,
        });
        let lines = cost_lines(&trace, false);
        assert_eq!(lines[0].1, "12,345 tokens in · 27 out · 2 requests");
        // 12,345 × $0.042 / 1M = $0.000518
        assert_eq!(lines[0].2, "$0.00052");
        assert_eq!(lines[1].1, "30.0s audio · Nari");
        // 30 s × $0.12 / h = $0.001
        assert_eq!(lines[1].2, "$0.0010");
        assert_eq!(lines[2].2, "$0.0015");
        trace.usage = None;
        assert_eq!(cost_lines(&trace, false)[0].1, "Not reported");
        assert_eq!(cost_lines(&trace, false)[2].2, "$0.0010+");
        assert_eq!(format_usd(0.25), "$0.2500");
        trace.usage = Some(voice_intent::VoiceUsage {
            input_tokens: 4_812,
            output_tokens: 9,
            requests: 1,
        });
        assert_eq!(
            cost_summary(&trace, false),
            "4,812 tok $0.00020 · 30.0s $0.0010 · $0.0012 est."
        );
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1_000_000), "1,000,000");
    }

    #[test]
    fn concrete_pills_show_actual_scores_and_exclude_diagnostic_families() {
        let mut trace = trace_fixture();
        trace.answers = vec![
            voice_intent::VoiceAnswer {
                id: "coding_agent".into(),
                probability: 0.876,
            },
            voice_intent::VoiceAnswer {
                id: "navigation".into(),
                probability: 0.99,
            },
            voice_intent::VoiceAnswer {
                id: "quick_action".into(),
                probability: 1.0,
            },
            voice_intent::VoiceAnswer {
                id: "candidate_19".into(),
                probability: 0.91,
            },
        ];
        let pills = action_pills(&trace, false, false);
        assert_eq!(pills.len(), 24);
        // Ranked by probability: the winner is first, then descending scores.
        assert_eq!(pills[0].id, "candidate_19");
        assert_eq!(pills[1].id, "coding_agent");
        assert!(pills[2..].iter().all(|p| p.probability.is_none()));
        assert!(
            !pills
                .iter()
                .any(|p| p.id == "navigation" || p.id == "quick_action")
        );
        let coding = pills.iter().find(|p| p.id == "coding_agent").unwrap();
        assert_eq!(coding.label, "Coding agent");
        assert_eq!(coding.score, "87.6%");
        assert!(!coding.winner);
        let winners: Vec<_> = pills.iter().filter(|p| p.winner).collect();
        assert_eq!(winners.len(), 1);
        assert_eq!(winners[0].id, "candidate_19");
        assert_eq!(winners[0].score, "91.0%");
        assert_eq!(winners[0].label, "Open: Conversation 19");
        // Unanswered rows retain the shared argmax order after answered rows.
        assert_eq!(pills[2].label, "New conversation");
        assert_eq!(pills[3].label, "Next conversation");
        assert_eq!(pills[4].label, "Previous conversation");
    }

    #[test]
    fn pill_ties_use_shared_argmax_order_not_question_or_answer_order() {
        let mut trace = trace_fixture();
        trace.questions.reverse();
        let ids = [
            "coding_agent",
            "new_session",
            "next_session",
            "previous_session",
            "candidate_2",
            "candidate_10",
        ];
        for start in 0..ids.len() {
            trace.answers = ids[start..]
                .iter()
                .rev()
                .map(|id| voice_intent::VoiceAnswer {
                    id: (*id).into(),
                    probability: 0.8,
                })
                .collect();
            let pills = action_pills(&trace, false, false);
            assert_eq!(pills.iter().find(|p| p.winner).unwrap().id, ids[start]);
            assert!(pills[0].winner, "the chosen route always ranks first");
            assert_eq!(pills.iter().filter(|p| p.winner).count(), 1);
        }
    }

    #[test]
    fn waiting_and_error_pills_never_invent_confidence_or_a_winner() {
        let trace = trace_fixture();
        for (waiting, failed, label) in [(true, false, "Waiting…"), (false, true, "Not returned")]
        {
            let pills = action_pills(&trace, waiting, failed);
            assert_eq!(pills.len(), 24);
            assert!(pills.iter().all(|p| p.score == label && !p.winner));
        }
        let mut trace = trace;
        trace.answers.push(voice_intent::VoiceAnswer {
            id: "coding_agent".into(),
            probability: 0.9,
        });
        assert!(action_pills(&trace, true, false).iter().all(|p| !p.winner));
        assert!(action_pills(&trace, false, true).iter().all(|p| !p.winner));
    }

}
