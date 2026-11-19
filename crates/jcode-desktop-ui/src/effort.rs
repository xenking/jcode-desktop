//! Reasoning effort: the selectable ladder for the serving model, stepping
//! for the fast effort keys, and those keys themselves.
//!
//! The ladder comes from `jcode-provider-core`, the same source the TUI uses,
//! so `/effort`, the effort pill menu, and Alt+Left/Right offer exactly the
//! levels the provider accepts. The keys come from the user's Jcode
//! `[keybindings] effort_increase / effort_decrease`, so one setting drives
//! both clients.

use crate::theme::Theme;
use gpui::{App, KeyBinding, Keystroke};

gpui::actions!(effort, [IncreaseEffort, DecreaseEffort]);

/// Offered before the serving model is known, e.g. while a session attaches.
const FALLBACK_LADDER: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// Levels the serving model accepts, weakest first. Empty when the provider
/// has no effort control. Swarm modes follow the real levels, as in the TUI.
pub(crate) fn ladder(provider: Option<&str>, model: Option<&str>) -> Vec<&'static str> {
    let model = model.map(str::trim).filter(|model| !model.is_empty());
    let Some(model) = model else {
        return FALLBACK_LADDER.to_vec();
    };
    // Desktop model ids can carry a route prefix (`openai:gpt-5.6`) or a
    // provider pin (`claude-opus-4-8@anthropic`); inference keys on the bare id.
    let (prefix, bare) = match model.split_once(':') {
        Some((prefix, bare)) if !prefix.contains('/') => (Some(prefix), bare),
        _ => (None, model),
    };
    let bare = bare.split('@').next().unwrap_or(bare);
    let provider = provider
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .or(prefix);
    jcode_provider_core::inferred_reasoning_efforts(provider, Some(bare))
}

/// The next level in `direction` (positive = stronger), or `None` at either
/// end. With no known current level, stepping starts from `medium`, the
/// providers' usual default, so one press lands on high or low.
pub(crate) fn step(
    ladder: &[&'static str],
    current: Option<&str>,
    direction: i8,
) -> Option<&'static str> {
    let current = current.map(str::trim);
    let index = match current.and_then(|current| ladder.iter().position(|level| *level == current))
    {
        Some(index) => index,
        // Unset or unknown: treat medium as current. A ladder without medium
        // starts from its weakest rung on the first press either way.
        None => match ladder.iter().position(|level| *level == "medium") {
            Some(medium) => medium,
            None => return ladder.first().copied(),
        },
    };
    let target = if direction > 0 {
        index + 1
    } else {
        index.checked_sub(1)?
    };
    ladder.get(target).copied()
}

/// Friendly name for status copy, e.g. `xhigh` reads `xHigh`.
pub(crate) fn label(effort: &str) -> &str {
    match effort {
        "none" => "None",
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "xHigh",
        "max" => "Max",
        "swarm" => "Swarm",
        "swarm-deep" => "Swarm Deep",
        other => other,
    }
}

/// Hover copy for the drawn level mark: the level name the shape replaces.
/// Swarm is not a rung, so its hover says what it actually does.
pub(crate) fn tooltip(effort: &str) -> String {
    if effort.trim().eq_ignore_ascii_case("swarm") {
        return format!("Reasoning effort: Swarm\n{SWARM_HELP}");
    }
    format!("Reasoning effort: {}", spoken(effort))
}

/// What the swarm rung means, unlike `/swarm`, which only toggles the tools.
pub(crate) const SWARM_HELP: &str =
    "Root thinks at low effort and every prompt asks it to fan work out to parallel workers";

/// The level name, for the places that must stay legible without a hover:
/// picker rows (beside the drawn mark), transcript notices. No text glyph:
/// the level's shape is drawn by `effort_icon`, never typed.
pub(crate) fn spoken(effort: &str) -> String {
    label(effort.trim()).to_string()
}

/// Ink for a level glyph.
///
/// The ladder is a heat scale, not a fade to the brand accent: the quiet rungs
/// stay cool and dim, medium is the chrome's neutral ink, high warms to amber,
/// and the top rungs are hot. A glance then says how hard the model is priced
/// to think without reading the glyph's name. The TUI's status line uses the
/// same scale (`jcode_tui_style::effort_ramp`).
///
/// The warm end is fixed rather than theme-keyed: every palette needs the same
/// warm/hot signal, and a monochrome palette would otherwise erase the top.
pub(crate) fn tint(effort: &str, theme: &Theme) -> gpui::Rgba {
    use gpui::Rgba;
    let rgb = |hex: u32| Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.,
        g: ((hex >> 8) & 0xff) as f32 / 255.,
        b: (hex & 0xff) as f32 / 255.,
        a: 1.,
    };
    match effort.trim().to_ascii_lowercase().as_str() {
        "none" | "minimal" => theme.TEXT_FAINT,
        // Cool: paid the least thinking, reads as the quiet end of the scale.
        "low" => rgb(0x6f9ed6),
        "medium" => theme.TEXT_DIM,
        // Warm, then hot.
        "high" => rgb(0xe0a458),
        "xhigh" => rgb(0xe8734a),
        "max" => rgb(0xe0453f),
        // Swarm modes are orchestration sentinels, not rungs.
        _ => theme.TEXT_FAINT,
    }
}

/// The configured effort chords in GPUI syntax, `(increase, decrease)`.
pub(crate) struct EffortKeys {
    pub increase: Vec<String>,
    pub decrease: Vec<String>,
}

impl EffortKeys {
    pub(crate) fn from_config() -> Self {
        let keys = &jcode_base::config::config().keybindings;
        Self::parse(&keys.effort_increase, &keys.effort_decrease)
    }

    pub(crate) fn parse(increase: &str, decrease: &str) -> Self {
        Self {
            increase: parse_chords(increase, "effort_increase"),
            decrease: parse_chords(decrease, "effort_decrease"),
        }
    }

    /// Display label such as `Alt+Left / Alt+Right`, or `None` when unbound.
    #[allow(dead_code)]
    pub(crate) fn label(&self) -> Option<String> {
        let decrease = self.decrease.first()?;
        let increase = self.increase.first()?;
        Some(format!(
            "{} / {}",
            display_chord(decrease),
            display_chord(increase)
        ))
    }
}

/// Jcode config chords (`alt+right`, comma-separated alternatives) become
/// GPUI keystrokes (`alt-right`). `none`/`off`/`disabled` unbind. An empty
/// value falls back to the platform default, matching the TUI.
fn parse_chords(raw: &str, id: &str) -> Vec<String> {
    let raw = raw.trim();
    if matches!(
        raw.to_ascii_lowercase().as_str(),
        "none" | "off" | "disabled"
    ) {
        return Vec::new();
    }
    let raw = if raw.is_empty() {
        default_chord(id)
    } else {
        raw
    };
    raw.split(',').filter_map(to_gpui_chord).collect()
}

/// Jcode's platform defaults (`jcode-config-types` keybinding table): macOS
/// uses Cmd so Option+arrows keep moving by word, elsewhere Alt+arrows.
fn default_chord(id: &str) -> &'static str {
    match (id, cfg!(target_os = "macos")) {
        ("effort_increase", true) => "cmd+right",
        ("effort_increase", false) => "alt+right",
        (_, true) => "cmd+left",
        (_, false) => "alt+left",
    }
}

fn to_gpui_chord(chord: &str) -> Option<String> {
    let chord = chord.trim().to_ascii_lowercase();
    if chord.is_empty() {
        return None;
    }
    let mut parts: Vec<&str> = chord.split('+').map(str::trim).collect();
    let key = parts.pop()?.to_string();
    let mut modifiers = Vec::new();
    for part in parts {
        let modifier = match part {
            "ctrl" | "control" => "ctrl",
            "alt" | "option" | "opt" | "meta" => "alt",
            "shift" => "shift",
            "cmd" | "command" | "super" | "win" | "platform" => "cmd",
            _ => return None,
        };
        if !modifiers.contains(&modifier) {
            modifiers.push(modifier);
        }
    }
    // GPUI's canonical order: ctrl, alt, shift, cmd.
    modifiers.sort_by_key(|modifier| match *modifier {
        "ctrl" => 0,
        "alt" => 1,
        "shift" => 2,
        _ => 3,
    });
    let key = match key.as_str() {
        "pgup" => "pageup".to_string(),
        "pgdn" | "pgdown" => "pagedown".to_string(),
        "esc" => "escape".to_string(),
        "return" => "enter".to_string(),
        "del" => "delete".to_string(),
        "arrowleft" => "left".to_string(),
        "arrowright" => "right".to_string(),
        "arrowup" => "up".to_string(),
        "arrowdown" => "down".to_string(),
        _ => key,
    };
    let mut gpui = modifiers.join("-");
    if !gpui.is_empty() {
        gpui.push('-');
    }
    gpui.push_str(&key);
    Keystroke::parse(&gpui).ok().map(|_| gpui)
}

fn display_chord(chord: &str) -> String {
    chord
        .split('-')
        .map(|part| match part {
            "ctrl" => "Ctrl".to_string(),
            "alt" if cfg!(target_os = "macos") => "⌥".to_string(),
            "alt" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            "cmd" if cfg!(target_os = "macos") => "Cmd".to_string(),
            "cmd" => "Super".to_string(),
            key => {
                let mut chars = key.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().chain(chars).collect())
                    .unwrap_or_default()
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Bind the fast effort keys in the chat panel and its composer.
///
/// Composer bindings share the `PromptInput` context with word motion and are
/// registered later, so they take precedence there. An input with no effort
/// handler (no chat panel above it) leaves the action unhandled and GPUI falls
/// through to the next binding, so Alt+Left keeps moving by word elsewhere.
/// A chord already claimed by a global workspace binding (Cmd+Left focuses
/// the left panel on macOS) is skipped rather than stolen.
pub(crate) fn bind_keys(cx: &mut App) {
    let keys = EffortKeys::from_config();
    let taken = |chord: &str, cx: &App| {
        let Ok(keystroke) = Keystroke::parse(chord) else {
            return true;
        };
        cx.key_bindings()
            .borrow()
            .all_bindings_for_input(&[keystroke])
            .iter()
            .any(|binding| binding.predicate().is_none())
    };
    let mut bindings = Vec::new();
    for (chords, increase) in [(&keys.increase, true), (&keys.decrease, false)] {
        for chord in chords {
            if taken(chord, cx) {
                continue;
            }
            for context in ["ChatPanel", "PromptInput"] {
                bindings.push(if increase {
                    KeyBinding::new(chord, IncreaseEffort, Some(context))
                } else {
                    KeyBinding::new(chord, DecreaseEffort, Some(context))
                });
            }
        }
    }
    cx.bind_keys(bindings);
}

/// The label of the effort chords that are actually bound, for hints.
pub(crate) fn bound_keys_label(cx: &App) -> Option<String> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let first = |action: &dyn gpui::Action| {
        keymap.bindings_for_action(action).next().map(|binding| {
            binding
                .keystrokes()
                .iter()
                .map(|keystroke| keystroke.inner().unparse())
                .collect::<Vec<_>>()
                .join(" ")
        })
    };
    let decrease = first(&DecreaseEffort)?;
    let increase = first(&IncreaseEffort)?;
    Some(format!(
        "{} / {}",
        display_chord(&decrease),
        display_chord(&increase)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spoken copy is the level name only: the shape is drawn, so no text
    /// circle leaks into notices, hovers or picker rows.
    #[test]
    fn spoken_copy_carries_no_text_glyph() {
        assert_eq!(tooltip("xhigh"), "Reasoning effort: xHigh");
        assert_eq!(
            tooltip("swarm"),
            format!("Reasoning effort: Swarm\n{SWARM_HELP}")
        );
        for level in ["none", "minimal", "low", "medium", "high", "xhigh", "max"] {
            let text = spoken(level);
            assert!(
                text.chars().all(|c| c.is_ascii_alphanumeric() || c == ' '),
                "{level} spoke {text:?}"
            );
        }
    }

    /// The ladder is a heat scale: cool low rungs, a neutral middle, warm high
    /// rungs and a hot top. Neighbouring levels never share an ink, so the
    /// glyph's color alone tells low from high.
    #[test]
    fn glyph_tint_is_a_heat_scale() {
        let theme = Theme::global();
        let levels = ["none", "low", "medium", "high", "xhigh", "max"];
        let inks = levels.map(|level| tint(level, theme));
        for (index, pair) in inks.windows(2).enumerate() {
            assert_ne!(
                pair[0],
                pair[1],
                "{} and {} share an ink",
                levels[index],
                levels[index + 1]
            );
        }
        assert_eq!(
            tint("none", theme),
            theme.TEXT_FAINT,
            "the weakest rung is chrome ink"
        );
        let low = inks[1];
        let high = inks[3];
        assert!(low.b > low.r, "low is cool (more blue than red): {low:?}");
        assert!(
            high.r > high.b,
            "high is warm (more red than blue): {high:?}"
        );
        let max = inks[5];
        assert!(max.r > max.g && max.r > max.b, "max is hot: {max:?}");
        assert_eq!(
            tint("swarm", theme),
            theme.TEXT_FAINT,
            "modes are not rungs"
        );
    }

    #[test]
    fn ladder_follows_the_serving_model() {
        assert_eq!(ladder(None, None), FALLBACK_LADDER);
        let openai = ladder(Some("openai"), Some("gpt-5.6-sol"));
        assert!(openai.contains(&"max") && openai.contains(&"minimal"));
        let claude = ladder(Some("anthropic"), Some("claude-sonnet-4-6"));
        assert!(!claude.contains(&"minimal"), "{claude:?}");
        assert_eq!(
            ladder(None, Some("openai:gpt-5.6-sol")),
            openai,
            "a route prefix names the provider"
        );
        assert_eq!(
            ladder(Some("anthropic"), Some("claude-sonnet-4-6@anthropic")),
            claude
        );
        assert!(ladder(Some("ollama"), Some("llama3")).is_empty());
    }

    #[test]
    fn step_moves_one_rung_and_stops_at_the_ends() {
        let ladder = ["low", "medium", "high"];
        assert_eq!(step(&ladder, Some("medium"), 1), Some("high"));
        assert_eq!(step(&ladder, Some("medium"), -1), Some("low"));
        assert_eq!(step(&ladder, Some("high"), 1), None);
        assert_eq!(step(&ladder, Some("low"), -1), None);
        // Unset or unknown effort steps from medium.
        assert_eq!(step(&ladder, None, 1), Some("high"));
        assert_eq!(step(&ladder, None, -1), Some("low"));
        assert_eq!(step(&ladder, Some("bogus"), 1), Some("high"));
        assert_eq!(step(&[], Some("low"), 1), None);
        // A ladder without medium starts from its first rung.
        assert_eq!(step(&["low", "high"], None, 1), Some("low"));
    }

    #[test]
    fn config_chords_translate_to_gpui() {
        let keys = EffortKeys::parse("alt+right", "Alt+Left, ctrl+shift+h");
        assert_eq!(keys.increase, ["alt-right"]);
        assert_eq!(keys.decrease, ["alt-left", "ctrl-shift-h"]);
        assert_eq!(EffortKeys::parse("cmd+right", "").increase, ["cmd-right"]);
        assert!(EffortKeys::parse("off", "none").increase.is_empty());
        assert!(EffortKeys::parse("hyper+x", "").increase.is_empty());
        assert!(
            !EffortKeys::parse("", "").decrease.is_empty(),
            "empty falls back to the platform default"
        );
        assert!(EffortKeys::parse("alt+right", "alt+left").label().is_some());
    }
}
