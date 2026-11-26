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

/// Green until 70% used, amber from 70%, red from 90%.
pub(crate) fn usage_color(used: f32, theme: &Palette) -> gpui::Rgba {
    if used >= 90. {
        theme.ERROR
    } else if used >= 70. {
        theme.WARN
    } else {
        gpui::rgb(0x64c864).into()
    }
}
