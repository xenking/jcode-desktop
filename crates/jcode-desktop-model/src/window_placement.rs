use std::{fs, io::Write as _, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    fn is_valid(self) -> bool {
        self.width > 0.
            && self.height > 0.
            && [self.x, self.y, self.right(), self.bottom()]
                .into_iter()
                .all(f32::is_finite)
    }

    fn right(self) -> f32 {
        self.x + self.width
    }

    fn bottom(self) -> f32 {
        self.y + self.height
    }

    fn reachable_titlebar_area(self, visible: Self) -> Option<f32> {
        let title_height = self.height.min(32.);
        let width = self.right().min(visible.right()) - self.x.max(visible.x);
        let height = (self.y + title_height).min(visible.bottom()) - self.y.max(visible.y);
        (width >= self.width.min(64.) && height >= title_height.min(16.)).then_some(width * height)
    }

    fn clamped_to(self, visible: Self) -> Self {
        let width = self.width.min(visible.width);
        let height = self.height.min(visible.height);
        Self {
            x: self
                .x
                .clamp(visible.x, (visible.right() - width).max(visible.x)),
            y: self
                .y
                .clamp(visible.y, (visible.bottom() - height).max(visible.y)),
            width,
            height,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Presentation {
    Windowed,
    Maximized,
    Fullscreen,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub rect: Rect,
    pub presentation: Presentation,
    pub display_uuid: Option<String>,
}

pub struct DisplayArea {
    pub uuid: Option<String>,
    pub visible: Rect,
}

impl Placement {
    /// Resolve a placement against the current usable areas. When GPUI reports
    /// display-local coordinates, only the matching display can interpret them.
    pub fn restore(&self, displays: &[DisplayArea]) -> Option<(usize, Self)> {
        if !self.rect.is_valid() {
            return None;
        }
        let (index, display, _) = displays
            .iter()
            .enumerate()
            .filter(|(_, display)| {
                display.visible.is_valid()
                    && (self.display_uuid.is_none() || self.display_uuid == display.uuid)
            })
            .filter_map(|(index, display)| {
                let area = self.rect.reachable_titlebar_area(display.visible)?;
                Some((index, display, area))
            })
            .max_by(|(_, _, a), (_, _, b)| a.total_cmp(b))?;
        Some((
            index,
            Self {
                rect: self.rect.clamped_to(display.visible),
                presentation: self.presentation,
                display_uuid: display.uuid.clone(),
            },
        ))
    }

    pub fn load(path: &Path) -> Option<Self> {
        serde_json::from_slice(&fs::read(path).ok()?).ok()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if !self.rect.is_valid() || Self::load(path).as_ref() == Some(self) {
            return Ok(());
        }
        let dir = path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(dir)?;
        let mut file = tempfile::NamedTempFile::new_in(dir)?;
        file.write_all(&serde_json::to_vec(self)?)?;
        file.persist(path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn area(uuid: &str, visible: Rect) -> DisplayArea {
        DisplayArea {
            uuid: Some(uuid.into()),
            visible,
        }
    }

    fn sample() -> Placement {
        Placement {
            rect: rect(210., 120., 800., 600.),
            presentation: Presentation::Windowed,
            display_uuid: Some("external".into()),
        }
    }

    #[test]
    fn picks_the_same_physical_display_even_when_displays_share_a_local_origin() {
        let visible = rect(0., 30., 1440., 870.);
        let saved = sample();
        assert_eq!(
            saved.restore(&[area("laptop", visible), area("external", visible)]),
            Some((1, saved))
        );
    }

    #[test]
    fn a_disconnected_display_or_unreachable_titlebar_requires_default_placement() {
        let visible = rect(0., 30., 1440., 870.);
        let mut saved = sample();
        assert!(saved.restore(&[area("laptop", visible)]).is_none());
        saved.rect.y = -100.;
        assert!(saved.restore(&[area("external", visible)]).is_none());
    }

    #[test]
    fn clamps_a_partially_offscreen_window_to_the_visible_area() {
        let mut saved = sample();
        saved.rect = rect(-100., 24., 1500., 950.);
        let visible = rect(0., 30., 1440., 870.);
        assert_eq!(
            saved.restore(&[area("external", visible)]).unwrap().1.rect,
            rect(0., 30., 1440., 870.)
        );
    }

    #[test]
    fn picks_a_display_by_titlebar_when_no_identity_was_available() {
        let mut saved = sample();
        saved.display_uuid = None;
        saved.rect.x = -1000.;
        let displays = [
            area("laptop", rect(0., 0., 1440., 900.)),
            area("external", rect(-1440., 0., 1440., 900.)),
        ];
        assert_eq!(saved.restore(&displays).unwrap().0, 1);
    }

    #[test]
    fn fullscreen_and_maximized_survive_disk_replacement_and_clamping() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window.json");
        fs::write(&path, "{corrupt").unwrap();
        assert_eq!(Placement::load(&path), None);
        let mut saved = sample();
        saved.rect.x = 1300.;
        for presentation in [Presentation::Fullscreen, Presentation::Maximized] {
            saved.presentation = presentation;
            saved.save(&path).unwrap();
            let (_, restored) = Placement::load(&path)
                .unwrap()
                .restore(&[area("external", rect(0., 30., 1440., 870.))])
                .unwrap();
            assert_eq!(restored.presentation, presentation);
            assert_eq!(restored.rect.x, 640.);
        }
    }
}
