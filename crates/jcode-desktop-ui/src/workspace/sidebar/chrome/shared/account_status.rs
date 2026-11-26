//! What the account meters show: the latest usage of every saved login, and
//! the color a fill level reads as.
//!
//! The sidebar, the composer's model picker and the panel footer all draw
//! quota figures from one snapshot, set by whoever last refreshed the accounts.
use crate::accounts::Account;
use crate::theme::Palette;

/// The accounts the meters draw from.
#[derive(Default)]
pub(crate) struct StatusAccounts(pub Vec<Account>);
impl gpui::Global for StatusAccounts {}

/// Green until 70% used, amber from 70%, red from 90%: the info widget's
/// ramp (amber at 30% left, red at 10% left), rounded on the used side first.
pub(crate) fn usage_color(used: f32, theme: &Palette) -> gpui::Rgba {
    let used = used.round();
    if used >= 90. {
        theme.ERROR
    } else if used >= 70. {
        theme.WARN
    } else {
        gpui::rgb(0x64c864).into()
    }
}

/// A quota reading the way the info widget words it: `N% used` or `M% left`.
pub(crate) fn usage_reading(used: f32, show_used: bool) -> String {
    let used = used.clamp(0., 100.).round() as u8;
    if show_used {
        format!("{used}% used")
    } else {
        format!("{}% left", 100 - used)
    }
}

/// [`usage_reading`] in the direction `display.usage_display` picks (`left` by default).
pub(crate) fn configured_usage_reading(used: f32) -> String {
    usage_reading(
        used,
        jcode_base::config::config().display.usage_display_used(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ten_percent_reading_says_what_the_info_widget_says() {
        assert_eq!(usage_reading(10., false), "90% left");
        assert_eq!(usage_reading(10., true), "10% used");
        assert_eq!(
            usage_reading(89.6, false),
            "10% left",
            "rounded on the used side"
        );
    }

    #[test]
    fn the_ramp_flips_at_the_info_widgets_rounded_thresholds() {
        let theme = crate::theme::Theme::global();
        assert_eq!(usage_color(69.4, theme), gpui::rgb(0x64c864).into());
        assert_eq!(usage_color(69.6, theme), theme.WARN, "70% used is 30% left");
        assert_eq!(
            usage_color(89.6, theme),
            theme.ERROR,
            "90% used is 10% left"
        );
    }
}
