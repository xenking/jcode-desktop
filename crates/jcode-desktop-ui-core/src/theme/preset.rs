//! The built-in themes a user can pick, by id and label. Which palette each
//! one means is [`super::presets`]' business, and which one is in use is
//! [`super::Theme`]'s.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemePreset {
    WarmNeutral,
    MonokaiProRistretto,
    WarmStudio,
    NeutralDark,
    NeutralLight,
    Midnight,
    Ocean,
    Forest,
    Plum,
    RoseDawn,
    Parchment,
    Graphite,
    Slate,
    Paper,
    Silver,
    LightNeutral,
    DarkNeutral,
    #[default]
    PureBlack,
    Glass,
}

impl ThemePreset {
    pub const ALL: [Self; 19] = [
        Self::WarmNeutral,
        Self::WarmStudio,
        Self::NeutralDark,
        Self::NeutralLight,
        Self::Midnight,
        Self::Ocean,
        Self::Forest,
        Self::Plum,
        Self::RoseDawn,
        Self::Parchment,
        Self::Graphite,
        Self::Slate,
        Self::Paper,
        Self::Silver,
        Self::LightNeutral,
        Self::DarkNeutral,
        Self::PureBlack,
        Self::Glass,
        Self::MonokaiProRistretto,
    ];
    pub const fn id(self) -> &'static str {
        match self {
            Self::WarmNeutral => "warm-neutral",
            Self::MonokaiProRistretto => "monokai-pro-ristretto",
            Self::WarmStudio => "warm-studio",
            Self::NeutralDark => "neutral-dark",
            Self::NeutralLight => "neutral-light",
            Self::Midnight => "midnight",
            Self::Ocean => "ocean",
            Self::Forest => "forest",
            Self::Plum => "plum",
            Self::RoseDawn => "rose-dawn",
            Self::Parchment => "parchment",
            Self::Graphite => "graphite",
            Self::Slate => "slate",
            Self::Paper => "paper",
            Self::Silver => "silver",
            Self::LightNeutral => "light-neutral",
            Self::DarkNeutral => "dark-neutral",
            Self::PureBlack => "pure-black",
            Self::Glass => "glass",
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::WarmNeutral => "Warm neutral",
            Self::MonokaiProRistretto => "Monokai Pro Ristretto",
            Self::WarmStudio => "Warm studio",
            Self::NeutralDark => "Neutral dark",
            Self::NeutralLight => "Neutral light",
            Self::Midnight => "Midnight",
            Self::Ocean => "Ocean",
            Self::Forest => "Forest",
            Self::Plum => "Plum",
            Self::RoseDawn => "Rose dawn",
            Self::Parchment => "Parchment",
            Self::Graphite => "Graphite",
            Self::Slate => "Slate",
            Self::Paper => "Paper",
            Self::Silver => "Silver",
            Self::LightNeutral => "Light Neutral",
            Self::DarkNeutral => "Dark Neutral",
            Self::PureBlack => "Pure Black",
            Self::Glass => "Glass",
        }
    }
    /// Position in [`Self::ALL`], which is also where the preset's palette
    /// sits in every per-preset table.
    pub(super) const fn index(self) -> usize {
        match self {
            Self::WarmNeutral => 0,
            Self::WarmStudio => 1,
            Self::NeutralDark => 2,
            Self::NeutralLight => 3,
            Self::Midnight => 4,
            Self::Ocean => 5,
            Self::Forest => 6,
            Self::Plum => 7,
            Self::RoseDawn => 8,
            Self::Parchment => 9,
            Self::Graphite => 10,
            Self::Slate => 11,
            Self::Paper => 12,
            Self::Silver => 13,
            Self::LightNeutral => 14,
            Self::DarkNeutral => 15,
            Self::PureBlack => 16,
            Self::Glass => 17,
            Self::MonokaiProRistretto => 18,
        }
    }
    pub fn from_id(value: &str) -> Self {
        // Keep existing selections when upgrading from the original theme name.
        let value = if value == "chatgpt-light" {
            "light-neutral"
        } else {
            value
        };
        Self::ALL
            .into_iter()
            .find(|preset| preset.id() == value)
            .unwrap_or_default()
    }
    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }
}
