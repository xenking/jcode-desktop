//! The model picker is two small steps, not one flat model x effort list.
//!
//! Step one picks a model. If that model has reasoning effort levels, step two
//! lists only those levels. Esc steps back before it closes. A third view,
//! "Manage models", toggles which routes the picker shows at all.
//!
//! The transitions are pure so the keyboard flow can be pinned without a window.

use std::collections::HashMap;

use super::model_menu::{self, ModelDetails};

/// Separates a model spec from its chosen effort in the submitted command, as
/// in `/model claude-oauth:claude-opus-5-5 --effort high`.
pub(crate) const EFFORT_FLAG: &str = " --effort ";

/// Split the argument of `/model` into the spec and an optional effort.
pub(crate) fn parse_model_pick(argument: &str) -> (&str, Option<&str>) {
    match argument.split_once(EFFORT_FLAG) {
        Some((model, effort)) => (
            model.trim(),
            Some(effort.trim()).filter(|effort| !effort.is_empty()),
        ),
        None => (argument.trim(), None),
    }
}

/// Where the model picker currently is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) enum ModelStage {
    /// Choosing a model.
    #[default]
    Models,
    /// Choosing the reasoning effort for `model`.
    Efforts { model: String },
    /// Choosing which routes the picker lists.
    Manage,
}

/// What a keypress on the highlighted row does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum StageStep {
    /// Show another step of the picker.
    Goto(ModelStage),
    /// Send this command and close the picker.
    Submit(String),
    /// Flip whether this route is hidden.
    ToggleHidden(String),
}

impl ModelStage {
    /// Enter on a model row. `efforts` are the levels that model supports.
    #[allow(dead_code)]
    pub(super) fn pick_model(&self, spec: &str, efforts: &[&str]) -> StageStep {
        match self {
            Self::Models if !efforts.is_empty() => StageStep::Goto(Self::Efforts {
                model: spec.to_string(),
            }),
            _ => StageStep::Submit(format!("/model {spec}")),
        }
    }

    /// Enter on an effort row.
    pub(super) fn pick_effort(&self, effort: &str) -> Option<StageStep> {
        match self {
            Self::Efforts { model, .. } => Some(StageStep::Submit(format!(
                "/model {model}{EFFORT_FLAG}{effort}"
            ))),
            _ => None,
        }
    }

    /// Esc: the step to go back to, or `None` when the picker should close.
    pub(super) fn back(&self) -> Option<Self> {
        match self {
            Self::Models => None,
            Self::Efforts { .. } | Self::Manage => Some(Self::Models),
        }
    }
}

/// The reasoning levels a route accepts, weakest first. Keyed on the route's
/// own provider and model id, the same inference the effort pill uses.
/// Routes without catalog metadata get no effort step.
pub(super) fn model_efforts(
    spec: &str,
    details: &HashMap<String, ModelDetails>,
) -> Vec<&'static str> {
    if !details.contains_key(spec) {
        return Vec::new();
    }
    // The same inference the session applies when the pair is submitted.
    crate::effort::ladder(None, Some(spec))
}

/// The level step two opens on: the current one if this model takes it,
/// else medium, else the first.
pub(super) fn preferred_effort<'a>(ladder: &[&'a str], current: Option<&str>) -> Option<&'a str> {
    current
        .and_then(|current| ladder.iter().copied().find(|level| *level == current))
        .or_else(|| ladder.iter().copied().find(|level| *level == "medium"))
        .or_else(|| ladder.first().copied())
}

/// Routes the picker lists: everything not hidden, in the given order.
pub(super) fn visible_models(models: &[String], hidden: &[String]) -> Vec<String> {
    if hidden.is_empty() {
        return models.to_vec();
    }
    models
        .iter()
        .filter(|model| !hidden.contains(model))
        .cloned()
        .collect()
}

/// The hidden list after flipping `spec`.
pub(super) fn toggled(hidden: &[String], spec: &str) -> Vec<String> {
    let mut next: Vec<String> = hidden.iter().filter(|h| *h != spec).cloned().collect();
    if next.len() == hidden.len() {
        next.push(spec.to_string());
    }
    next
}

/// Title above the effort list, e.g. `Reasoning effort · Claude Opus 5.5`.
pub(super) fn effort_header(spec: &str, details: &HashMap<String, ModelDetails>) -> String {
    format!(
        "Reasoning effort · {}",
        model_menu::pretty_title(spec, details)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn efforts(model: &str) -> ModelStage {
        ModelStage::Efforts {
            model: model.into(),
        }
    }

    #[test]
    fn model_with_efforts_opens_the_effort_step_and_without_submits() {
        let stage = ModelStage::Models;
        assert_eq!(
            stage.pick_model("openai-oauth:gpt-5.6-sol", &["low", "high"]),
            StageStep::Goto(ModelStage::Efforts {
                model: "openai-oauth:gpt-5.6-sol".into(),
            })
        );
        assert_eq!(
            stage.pick_model("ollama:llama3", &[]),
            StageStep::Submit("/model ollama:llama3".into())
        );
    }

    #[test]
    fn effort_step_submits_the_pair_and_esc_walks_back_one_step() {
        let stage = efforts("a:m");
        assert_eq!(
            stage.pick_effort("high"),
            Some(StageStep::Submit("/model a:m --effort high".into()))
        );
        assert_eq!(ModelStage::Models.pick_effort("high"), None);
        assert_eq!(stage.back(), Some(ModelStage::Models));
        assert_eq!(ModelStage::Manage.back(), Some(ModelStage::Models));
        assert_eq!(ModelStage::Models.back(), None);
    }

    #[test]
    fn model_pick_round_trips_through_the_command() {
        assert_eq!(parse_model_pick("a:m"), ("a:m", None));
        assert_eq!(parse_model_pick("a:m --effort high"), ("a:m", Some("high")));
        assert_eq!(parse_model_pick("a:m --effort "), ("a:m", None));
    }

    #[test]
    fn hiding_filters_exactly_and_toggling_is_symmetric() {
        let models: Vec<String> = ["a:1", "a:2", "b:1"].map(String::from).into();
        assert_eq!(visible_models(&models, &[]), models);
        assert_eq!(
            visible_models(&models, &["a:2".into(), "gone:x".into()]),
            ["a:1", "b:1"]
        );
        let hidden = toggled(&[], "a:2");
        assert_eq!(hidden, ["a:2"]);
        assert!(toggled(&hidden, "a:2").is_empty());
    }

    #[test]
    fn step_two_opens_on_current_then_medium_then_first() {
        let ladder = ["low", "medium", "high"];
        assert_eq!(preferred_effort(&ladder, Some("high")), Some("high"));
        assert_eq!(preferred_effort(&ladder, Some("max")), Some("medium"));
        assert_eq!(preferred_effort(&["low", "high"], None), Some("low"));
        assert_eq!(preferred_effort(&[], None), None);
    }
}
