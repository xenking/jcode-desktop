//! The visible local default for Super+Enter, backed by the existing pinned preference.
use super::*;
use super::folder_picker::FolderPicker;
use gpui::DismissEvent;

impl Workspace {
    pub(super) fn open_default_directory_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.default_directory_panel_index(cx) {
            self.set_active(index, cx);
            self.overview = false;
            self.overview_progress.set(0.0, Instant::now());
            self.focus_active(window, cx);
            self.focus_pending = true;
            return;
        }
        let dir = self
            .pinned_working_dir
            .clone()
            .or_else(|| self.default_working_dir())
            .map(PathBuf::from)
            .unwrap_or_else(filesystem_root);
        let workspace = cx.weak_entity();
        let picker = cx.new(|cx| FolderPicker::new(workspace, true, dir, None, cx));
        let search = picker.read(cx).search.clone();
        self.adopt_default_directory_picker(picker, cx);
        let panel = cx.new(|cx| {
            let mut panel = Panel::new(
                Panel::DEFAULT_DIRECTORY_SESSION_ID.into(),
                Some("Default directory".into()),
                None,
                self.bridge.clone(),
                cx,
            );
            // Use the panel's actual input handle for every existing focus path,
            // including live tabs, overview, navigation diagnostics and reload.
            panel.input = search;
            panel
        });
        let width_fraction = 0.5;
        let insert_at = if self.slots.is_empty() {
            0
        } else {
            self.active + 1
        };
        self.slots.insert(
            insert_at,
            Slot {
                panel: PanelItem::Chat(panel),
                row: self.active_row,
                width_fraction,
                animated_width: AnimatedValue::new(
                    width_fraction,
                    transition::policy(Transition::PanelOpen).duration,
                ),
                order_offset: AnimatedValue::new(
                    0.0,
                    transition::policy(Transition::PanelOrder).duration,
                ),
                order_distance_fraction: width_fraction,
                close_progress: AnimatedValue::new(
                    1.0,
                    transition::policy(Transition::PanelClose).duration,
                ),
                closing: false,
                restore_fraction: None,
            },
        );
        self.set_active(insert_at, cx);
        crate::sounds::play(crate::sounds::Cue::PanelOpen, cx);
        self.overview = false;
        self.overview_progress.set(0.0, Instant::now());
        self.focus_pending = true;
        cx.notify();
    }

    /// Remove only the preference panel, preserving the identities and focus of
    /// every conversation, including when saving after moving it to another row.
    pub(super) fn remove_default_directory_panel(&mut self, cx: &mut Context<Self>) {
        self.default_directory_picker = None;
        let Some(index) = self.default_directory_panel_index(cx) else {
            return;
        };
        let active_id = self
            .slots
            .get(self.active)
            .map(|slot| slot.panel.entity_id());
        let removed = self.slots.remove(index);
        crate::sounds::play(crate::sounds::Cue::PanelClose, cx);
        let removed_id = removed.panel.entity_id();
        if self.previous == Some(removed_id) {
            self.previous = None;
        }
        for remembered in &mut self.row_focus {
            if *remembered == Some(removed_id) {
                *remembered = None;
            }
        }
        self.active = active_id
            .filter(|id| *id != removed_id)
            .and_then(|id| {
                self.slots
                    .iter()
                    .position(|slot| slot.panel.entity_id() == id)
            })
            .unwrap_or_else(|| {
                // Conversation surfaces can still be fading when a held close
                // reaches this immediately removed preference panel.
                let remaining: Vec<_> = self
                    .row_indices(self.active_row)
                    .filter(|&index| !self.slots[index].closing)
                    .collect();
                focus_after_close(index, &remaining)
            });
        if let Some(slot) = self
            .slots
            .get(self.active)
            .filter(|slot| slot.row == self.active_row && !slot.closing)
        {
            self.row_focus[self.active_row] = Some(slot.panel.entity_id());
        }
        self.retarget_camera();
    }

    /// Choosing the default directory shows the picker inline, in its panel.
    pub(super) fn adopt_default_directory_picker(
        &mut self,
        picker: Entity<FolderPicker>,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe(&picker, |this, _, _: &DismissEvent, cx| {
            this.close_default_directory_picker(cx)
        })
        .detach();
        self.default_directory_picker = Some(picker);
    }

    pub(super) fn close_default_directory_picker(&mut self, cx: &mut Context<Self>) {
        self.remove_default_directory_panel(cx);
        self.focus_pending = true;
        cx.notify();
    }

    pub(super) fn render_default_directory_picker(&self) -> gpui::AnyElement {
        match &self.default_directory_picker {
            Some(picker) => picker.clone().into_any_element(),
            None => div().into_any_element(),
        }
    }
}
