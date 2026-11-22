use gpui::{Context, IntoElement, ParentElement, Render, Rgba, Styled, Window, div, px, svg};

use jcode_provider_core::{SpeedAvailability, SpeedMode, SpeedOption};

const MODES: [SpeedMode; 3] = [SpeedMode::Standard, SpeedMode::Fast, SpeedMode::Ultrafast];

pub(crate) fn label(tier: SpeedMode) -> &'static str {
    match tier {
        SpeedMode::Standard => "Standard",
        SpeedMode::Fast => "Fast",
        SpeedMode::Ultrafast => "Ultrafast",
    }
}

pub(crate) fn command_value(tier: SpeedMode) -> String {
    format!("/speed {}", label(tier).to_ascii_lowercase())
}

pub(crate) fn command_tier(value: &str) -> Option<SpeedMode> {
    let speed = value.strip_prefix("/speed ")?;
    MODES
        .into_iter()
        .find(|tier| label(*tier).eq_ignore_ascii_case(speed))
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SpeedPicker {
    pub active: SpeedMode,
    pub options: Vec<(SpeedMode, String)>,
    pub tooltip: String,
}

pub(crate) fn offered(options: &[SpeedOption], mode: SpeedMode) -> bool {
    mode == SpeedMode::Standard
        || options.iter().any(|option| {
            option.mode == mode && option.availability != SpeedAvailability::Unavailable
        })
}

pub(crate) fn visible_mode(options: &[SpeedOption], requested: SpeedMode) -> SpeedMode {
    if offered(options, requested) {
        requested
    } else {
        SpeedMode::Standard
    }
}

impl SpeedPicker {
    pub(crate) fn new(
        options: &[SpeedOption],
        requested: SpeedMode,
        applying: bool,
        last_response: Option<&str>,
        rejection: Option<(SpeedMode, &str)>,
    ) -> Self {
        let selected = options.iter().find(|option| option.mode == requested);
        let active = visible_mode(options, requested);
        let available = active == requested;
        let mut tooltip = format!("Speed for this chat: {}. {}", label(active), quota(active));
        if !available {
            tooltip.push_str(&format!(" Requested {} is unavailable.", label(requested)));
            if let Some(reason) = selected.and_then(|option| option.reason.as_deref()) {
                tooltip.push(' ');
                tooltip.push_str(reason);
            }
        } else if applying {
            tooltip.push_str(" Setting requested; waiting for the runtime.");
        } else if active != SpeedMode::Standard {
            tooltip.push_str(" Requested for the next turn, not a guarantee of the serving tier.");
            if let Some(reason) = selected.and_then(|option| option.reason.as_deref()) {
                tooltip.push(' ');
                tooltip.push_str(reason);
            }
        } else if !MODES[1..].iter().any(|mode| offered(options, *mode)) {
            tooltip.push_str(" This route does not advertise an accelerated speed.");
        }
        if let Some(response) = last_response {
            tooltip.push(' ');
            tooltip.push_str(response);
        }
        if let Some((mode, cause)) = rejection {
            tooltip.push_str(&format!(" {} was not applied: {cause}", label(mode)));
        }

        let options = MODES
            .into_iter()
            .filter_map(|mode| {
                let advertised = options.iter().find(|option| option.mode == mode);
                if !offered(options, mode) {
                    return None;
                }
                let mut detail = if mode == active && !available {
                    format!(
                        "Requested {} unavailable. {}",
                        label(requested),
                        quota(mode)
                    )
                } else if let Some((rejected, _)) = rejection.filter(|_| mode == active) {
                    format!("{} was not applied. {}", label(rejected), quota(mode))
                } else if let Some(response) = last_response.filter(|_| mode == active) {
                    format!("{response} {}", quota(mode))
                } else {
                    quota(mode).to_owned()
                };
                if let Some(reason) = advertised.and_then(|option| option.reason.as_deref()) {
                    detail.push(' ');
                    detail.push_str(reason);
                }
                Some((mode, detail))
            })
            .collect();
        Self {
            active,
            options,
            tooltip,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_standard_preserves_unavailable_ultrafast_intent() {
        let picker = SpeedPicker::new(&[], SpeedMode::Ultrafast, false, None, None);
        assert_eq!(picker.active, SpeedMode::Standard);
        assert_eq!(picker.options.len(), 1);
        assert!(
            picker
                .tooltip
                .contains("Requested Ultrafast is unavailable")
        );
    }

    #[test]
    fn only_advertised_acceleration_is_offered_with_quota_caveat() {
        let options = [
            SpeedOption {
                mode: SpeedMode::Fast,
                availability: SpeedAvailability::Unknown,
                reason: Some("Account entitlement is not verified.".into()),
            },
            SpeedOption {
                mode: SpeedMode::Ultrafast,
                availability: SpeedAvailability::Unavailable,
                reason: None,
            },
        ];
        let picker = SpeedPicker::new(&options, SpeedMode::Standard, false, None, None);
        let modes: Vec<_> = picker.options.iter().map(|(mode, _)| *mode).collect();
        assert_eq!(modes, [SpeedMode::Standard, SpeedMode::Fast]);
        assert_eq!(picker.active, SpeedMode::Standard);
        assert!(picker.options[1].1.contains("more quota or cost more"));
        assert!(picker.options[1].1.contains("not verified"));
    }
}

fn quota(tier: SpeedMode) -> &'static str {
    match tier {
        SpeedMode::Standard => "Normal quota use.",
        SpeedMode::Fast => "Faster inference. May use more quota or cost more.",
        SpeedMode::Ultrafast => "Fastest advertised inference. May use more quota or cost more.",
    }
}

pub(crate) struct SpeedTooltip(pub String);

impl Render for SpeedTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = crate::theme::Theme::global();
        div()
            .max_w(px(300.))
            .p_2()
            .rounded_md()
            .bg(theme.HEADER_BG)
            .text_size(px(12.))
            .text_color(theme.TEXT)
            .child(self.0.clone())
    }
}

pub(crate) fn render(tier: SpeedMode, size: f32, ink: Rgba) -> impl IntoElement {
    let data = match tier {
        SpeedMode::Standard => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path fill="none" stroke="black" stroke-width="1.25" stroke-linejoin="round" d="M9 1 3 9h4l-.5 6L13 7H9z"/></svg>"#.as_slice(),
        SpeedMode::Fast => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M9 1 3 9h4l-.5 6L13 7H9z"/></svg>"#.as_slice(),
        SpeedMode::Ultrafast => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M6.5 1 1 8.5h3.25L3.5 15 9 7.5H5.75z M12.5 1 7 8.5h3.25L9.5 15 15 7.5h-3.25z"/></svg>"#.as_slice(),
    };
    svg().data(data).size(px(size)).flex_none().text_color(ink)
}
