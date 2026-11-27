//! The sidebar's workspace sections and their screenshot fixture. Leaf view
//! code: the workspace's data model lives in `workspace_spaces`.
use super::*;
use jcode_desktop_harness::paired;

impl Workspace {
    /// The sidebar's workspace sections: this machine first, then every paired
    /// machine by its label, each with its four workspaces and the sessions
    /// open in them. A machine that is not connected shows dimmed, with what it
    /// last held. Nothing is shown until a machine is paired.
    pub(super) fn render_space_sections(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.space_hosts.is_empty() {
            return None;
        }
        let theme = Theme::global();
        let titles: HashMap<_, _> = self
            .sidebar_panel_metadata
            .as_ref()?
            .read(cx)
            .iter()
            .map(|(id, panel)| (*id, panel.title.to_string()))
            .collect();
        let sections = std::iter::once(None)
            .chain(self.space_hosts.iter().map(|host| Some(host.as_str())))
            .enumerate()
            .map(|(space, host)| {
                let first = space * ROWS_PER_SPACE;
                let name = host.map_or_else(|| "Local".to_owned(), machines::machine_label);
                let status = host
                    .and_then(|host| self.remotes.machine_status.get(host).copied())
                    .map(|status| status.label(&name));
                let offline = host.is_some_and(|host| {
                    matches!(
                        self.remotes.machine_status.get(host),
                        Some(machines::MachineStatus::Lost | machines::MachineStatus::NeedsUpdate)
                    )
                });
                let pills = (0..ROWS_PER_SPACE).map(|offset| {
                    let row = first + offset;
                    let active = row == self.active_row;
                    let occupied = self
                        .row_indices(row)
                        .any(|index| !self.slots[index].closing);
                    div()
                        .id(("space-workspace", row))
                        .debug_selector(move || format!("space-workspace-{row}"))
                        .flex_none()
                        .w(px(22.0))
                        .h(px(20.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .cursor_pointer()
                        .text_size(px(11.0))
                        .text_color(if occupied || active { theme.TEXT } else { theme.TEXT_DIM })
                        .when(active, |el| el.bg(theme.HEADER_BG))
                        .hover(|el| el.bg(theme.HEADER_BG))
                        .child((offset + 1).to_string())
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.select_row(row, 0);
                                this.focus_active(window, cx);
                                cx.notify();
                            }),
                        )
                });
                let sessions = (first..first + ROWS_PER_SPACE)
                    .flat_map(|row| self.row_indices(row))
                    .filter(|&index| !self.slots[index].closing)
                    
                    .filter_map(|index| {
                        let title = titles.get(&self.slots[index].panel.entity_id())?.clone();
                        let active = index == self.active;
                        Some(
                            div()
                                .id(("space-session", index))
                                .debug_selector(move || format!("space-session-{index}"))
                                .w_full()
                                .px_2()
                                .py_0p5()
                                .rounded_full()
                                .cursor_pointer()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(11.0))
                                .text_color(if active { theme.TEXT } else { theme.TEXT_DIM })
                                .when(active, |el| el.bg(theme.HEADER_BG))
                                .hover(|el| el.bg(theme.HEADER_BG))
                                .child(title)
                                .on_mouse_down(
                                    gpui::MouseButton::Left,
                                    cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.set_active(index, cx);
                                        this.focus_active(window, cx);
                                    }),
                                ),
                        )
                    })
                    .collect::<Vec<_>>();
                div()
                    .id(("space-section", space))
                    .debug_selector(move || format!("space-section-{space}"))
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .when(offline, |el| el.opacity(0.55))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .debug_selector(move || format!("space-name-{space}"))
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(theme.TEXT)
                                    .child(name),
                            )
                            .children(status.map(|status| {
                                div()
                                    .debug_selector(move || format!("space-status-{space}"))
                                    .flex_none()
                                    .max_w(px(120.0))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(px(10.0))
                                    .text_color(theme.TEXT_DIM)
                                    .child(status)
                            })),
                    )
                    .child(div().flex().items_center().gap_0p5().children(pills))
                    .children(sessions)
            })
            .collect::<Vec<_>>();
        Some(
            div()
                .debug_selector(|| "space-sections".into())
                .flex_none()
                .px_3()
                .py_1p5()
                .flex()
                .flex_col()
                .gap_2()
                .border_b_1()
                .border_color(theme.PANEL_BORDER)
                .children(sections)
                .into_any_element(),
        )
    }

    /// Screenshot fixture: this machine and paired machines side by side, with
    /// sessions in each. Offline Desktop: no daemon, an inert bridge.
    /// Variants: `one`, `two` (two machines), `offline` (the work machine's
    /// link is lost), `update` (needs a newer Jcode), `migrated` (a version 1
    /// snapshot with `paired://` panels, restored through the real decode path).
    pub(super) fn show_spaces_fixture(
        &mut self,
        variant: &str,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let machine = |id: &str, label: &str| paired::Destination {
            id: id.repeat(32 / id.len()),
            endpoint: format!("https://{label}.example"),
            label: label.into(),
            working_dir: "/work".into(),
        };
        let remote = |id: &str, name: &str| format!("paired://{}/{name}", id.repeat(32 / id.len()));
        let two = matches!(variant, "two");
        let machines = if two {
            vec![machine("aa", "work"), machine("bb", "lab")]
        } else {
            vec![machine("aa", "work")]
        };
        let titles = [
            (0, None, "Refactor the parser"),
            (1, None, "Review the migration"),
            (0, Some("aa"), "Deploy checks"),
            (1, Some("aa"), "Fix flaky CI"),
            (1, Some("aa"), "Triage alerts"),
            (0, Some("bb"), "Benchmark run"),
        ];
        let open = |this: &mut Self, host: Option<&str>, row: usize, title: &str, cx: &mut Context<Self>| {
            let session_id = match host {
                Some(id) => remote(id, &format!("session_{}", title.replace(' ', "_").to_lowercase())),
                None => format!("session_{}", title.replace(' ', "_").to_lowercase()),
            };
            let first = host.map_or(0, |id| {
                let known = this.space_first_row(&format!("paired:{}", id.repeat(32 / id.len())));
                known.unwrap_or(0)
            });
            this.open_session_with_identity(session_id, Some(title.into()), None, first + row, cx);
        };
        if variant == "migrated" {
            // A flat window as version 1 saved it: every panel in rows 0..3.
            for (row, host, title) in titles.iter().filter(|(_, host, _)| *host != Some("bb")) {
                open(self, *host, *row, title, cx);
            }
            let mut v1 = serde_json::to_value(self.snapshot_inner(window, cx, true).unwrap()).unwrap();
            v1["format_version"] = 1.into();
            v1.as_object_mut().unwrap().remove("space_hosts");
            let snapshot = WorkspaceSnapshot::decode(&serde_json::to_vec(&v1).unwrap()).unwrap();
            self.remotes.paired = machines;
            paired::publish_labels(&self.remotes.paired);
            self.apply_snapshot(snapshot, cx);
        } else {
            self.remotes.paired = machines;
            paired::publish_labels(&self.remotes.paired);
            self.sync_spaces();
            for (row, host, title) in &titles {
                if *host != Some("bb") || two {
                    open(self, *host, *row, title, cx);
                }
            }
        }
        let status = |status| (format!("paired:{}", "aa".repeat(16)), status);
        let (host, status) = match variant {
            "offline" => status(machines::MachineStatus::Lost),
            "update" => status(machines::MachineStatus::NeedsUpdate),
            _ => status(machines::MachineStatus::Connected),
        };
        self.remotes.machine_status.insert(host, status);
        if two {
            self.remotes
                .machine_status
                .insert(format!("paired:{}", "bb".repeat(16)), machines::MachineStatus::Connecting);
        }
        self.active_row = 0;
        self.active = 0;
        self.sidebar_view = SidebarView::Sessions;
    }
}
