//! Cached suggestion metadata and viewport-only model/command row rendering.
use super::*;
use crate::speed_icon;

pub(super) struct SuggestionsCache {
    content: SharedString,
    cursor: usize,
    revision: u64,
    minute: u64,
    completion: bool,
    rows: Arc<Vec<CommandSuggestion>>,
}

/// Longest name the grid gives its own column, in characters.
const GRID_NAME_CHARS: usize = 28;
/// Share of the row the name column may take, so descriptions keep the rest.
const GRID_NAME_SHARE: f32 = 0.4;
const GRID_ICON: f32 = 18.0;
const GRID_GAP: f32 = 12.0;
const GRID_TEXT: f32 = 12.0;

/// What a grid row shows after its name: the kind tag and the description on
/// one line. `None` for rows that are not plain text commands, skills or modes
/// (model, effort and speed rows lay themselves out).
pub(super) fn grid_description(row: &CommandSuggestion) -> Option<String> {
    if row.toggle.is_some()
        || row.action.is_some()
        || row.value.starts_with("/model ")
        || row.value.starts_with("/effort ")
        || crate::speed_icon::command_tier(&row.value).is_some()
    {
        return None;
    }
    let one_line = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let detail = row.detail.as_deref().map(one_line).filter(|d| !d.is_empty());
    let help = one_line(&row.help);
    let text = match (detail, help.is_empty()) {
        (Some(detail), false) => format!("{detail} · {help}"),
        (Some(detail), true) => detail,
        (None, _) => help,
    };
    (!text.is_empty()).then_some(text)
}

/// The name a grid row shows in its name column.
fn grid_label(row: &CommandSuggestion) -> &str {
    match row.value.strip_prefix("/mode ") {
        Some("off") => "Clear all",
        Some(name) => name,
        None => &row.value,
    }
}

impl PromptInput {
    /// Rows for whichever step of the two-step model picker is showing.
    pub(super) fn model_stage_rows(&self, query: &str) -> Vec<CommandSuggestion> {
        match &self.model_stage {
            model_stage::ModelStage::Models => self.model_rows(query),
            model_stage::ModelStage::Efforts { model } => self.effort_rows(model, query),
            model_stage::ModelStage::Manage => self.manage_rows(query),
        }
    }

    /// The route the active session is on, which stays listed even if hidden.
    fn current_route(&self) -> Option<&str> {
        model_menu::current_spec(
            self.current_model.as_deref(),
            &self.command_models,
            &self.model_details,
        )
    }

    /// Step one: visible routes, grouped and ranked. A model with reasoning
    /// levels opens step two instead of switching.
    fn model_rows(&self, query: &str) -> Vec<CommandSuggestion> {
        let current = self.current_route();
        let hidden: Vec<String> = crate::config::hidden_models()
            .into_iter()
            .filter(|hidden| Some(hidden.as_str()) != current)
            .collect();
        let models = model_stage::visible_models(&self.command_models, &hidden);
        let searching = !query.trim().is_empty();
        let mut rows: Vec<CommandSuggestion> = model_menu::grouped_rows(
            &models,
            &self.model_details,
            self.current_model.as_deref(),
            &self.expanded_model_groups,
            query,
        )
        .into_iter()
        .map(|row| {
            let active_choice = row
                .value
                .strip_prefix("/model ")
                .is_some_and(|spec| Some(spec) == current);
            // Enter switches model and effort together: the row carries the
            // level picked with the effort keys (or the one in force).
            let action = row.value.strip_prefix("/model ").map(|spec| {
                model_stage::StageStep::Submit(match self.row_effort(spec) {
                    Some(effort) => {
                        format!("/model {spec}{}{effort}", model_stage::EFFORT_FLAG)
                    }
                    None => format!("/model {spec}"),
                })
            });
            CommandSuggestion {
                value: row.value,
                help: String::new(),
                active_choice,
                detail: None,
                header: row.header,
                toggle: row.toggle,
                action,
                context_action: None,
            }
        })
        .collect();
        if !searching {
            rows.push(CommandSuggestion {
                value: "Manage models".into(),
                active_choice: false,
                help: match hidden.len() {
                    0 => String::new(),
                    count => format!("{count} hidden"),
                },
                detail: None,
                header: None,
                toggle: None,
                context_action: None,
                action: Some(model_stage::StageStep::Goto(
                    model_stage::ModelStage::Manage,
                )),
            });
        }
        rows
    }

    /// Levels offered on a model row. `swarm` is a Jcode sentinel, not
    /// something the model accepts, so it is not a reasoning choice here.
    pub(super) fn picker_efforts(&self, spec: &str) -> Vec<&'static str> {
        model_stage::model_efforts(spec, &self.model_details)
            .into_iter()
            .filter(|effort| !effort.starts_with("swarm"))
            .collect()
    }

    /// The level a switch to `spec` would use: the one picked in the menu,
    /// else the one in force, clamped to what `spec` accepts. `None` when
    /// neither is known, so the switch keeps the model's own default.
    pub(super) fn row_effort(&self, spec: &str) -> Option<&'static str> {
        let wanted = self
            .picker_effort
            .as_deref()
            .or(self.current_effort.as_deref())?;
        model_stage::preferred_effort(&self.picker_efforts(spec), Some(wanted))
    }

    /// The model row under the highlight, if the menu is on its model step.
    fn selected_model_row(&self) -> Option<String> {
        if self.model_stage != model_stage::ModelStage::Models {
            return None;
        }
        self.command_suggestions()
            .get(self.command_selection)
            .and_then(|row| row.value.strip_prefix("/model "))
            .map(str::to_owned)
    }

    /// Step the highlighted model's level. True when the menu used the key.
    pub(super) fn step_picker_effort(&mut self, direction: i8, cx: &mut Context<Self>) -> bool {
        let Some(spec) = self.selected_model_row() else {
            return false;
        };
        let ladder = self.picker_efforts(&spec);
        if ladder.is_empty() {
            return false;
        }
        let current = self.row_effort(&spec);
        if let Some(next) = crate::effort::step(&ladder, current, direction) {
            self.set_picker_effort(next, cx);
        }
        true
    }

    pub(crate) fn set_picker_effort(&mut self, effort: &str, cx: &mut Context<Self>) {
        if self.picker_effort.as_deref() != Some(effort) {
            self.picker_effort = Some(effort.to_owned());
            self.suggestions_revision += 1;
            cx.notify();
        }
    }

    fn row_speed(&self, spec: &str) -> SpeedMode {
        if self.current_route() == Some(spec) {
            self.speed_picker
                .as_ref()
                .map_or(SpeedMode::Standard, |picker| picker.active)
        } else {
            SpeedMode::Standard
        }
    }

    fn row_speed_detail(&self, spec: &str, mode: SpeedMode) -> Option<String> {
        if self.current_route() == Some(spec) {
            return self
                .speed_picker
                .as_ref()?
                .options
                .iter()
                .find(|(tier, _)| *tier == mode)
                .map(|(_, detail)| detail.clone());
        }
        let options = &self.model_details.get(spec)?.speed_options;
        speed_icon::offered(options, mode).then(|| {
            let reason = options
                .iter()
                .find(|option| option.mode == mode)
                .and_then(|option| option.reason.as_deref())
                .unwrap_or_default();
            format!("{} {reason}", speed_icon::quota(mode))
        })
    }

    fn choose_row_speed(&self, spec: &str, mode: SpeedMode, cx: &mut Context<Self>) {
        if self.submission_enabled
            && !self.queue_editor
            && let Some(choose) = &self.on_model_speed
        {
            choose(
                ModelSpeedChoice {
                    spec,
                    effort: self.row_effort(spec),
                    current: self.current_route() == Some(spec),
                    mode,
                },
                cx,
            );
        }
    }

    pub(super) fn step_picker_speed(&self, direction: i8, cx: &mut Context<Self>) {
        let Some(spec) = self.selected_model_row() else {
            return;
        };
        let modes: Vec<_> = std::iter::once(SpeedMode::Standard)
            .chain(
                speed_icon::ACCELERATED
                    .into_iter()
                    .filter(|mode| self.row_speed_detail(&spec, *mode).is_some()),
            )
            .collect();
        let current = modes
            .iter()
            .position(|mode| *mode == self.row_speed(&spec))
            .unwrap_or(0);
        if let Some(next) = current
            .checked_add_signed(isize::from(direction))
            .and_then(|index| modes.get(index))
        {
            self.choose_row_speed(&spec, *next, cx);
        }
    }

    /// Step two: only the levels this model supports.
    fn effort_rows(&self, model: &str, query: &str) -> Vec<CommandSuggestion> {
        let query = query.trim().to_ascii_lowercase();
        let on_current_model = self.current_route() == Some(model);
        self.picker_efforts(model)
            .into_iter()
            .filter(|effort| query.is_empty() || effort.contains(&query))
            .enumerate()
            .map(|(index, effort)| CommandSuggestion {
                value: format!("/effort {effort}"),
                help: String::new(),
                active_choice: on_current_model && self.current_effort.as_deref() == Some(effort),
                detail: None,
                header: (index == 0)
                    .then(|| model_stage::effort_header(model, &self.model_details)),
                toggle: None,
                context_action: None,
                action: self.model_stage.pick_effort(effort),
            })
            .collect()
    }

    /// Every route, hidden or not, each flipping its visibility on Enter.
    fn manage_rows(&self, query: &str) -> Vec<CommandSuggestion> {
        let hidden = crate::config::hidden_models();
        let open: HashSet<String> = self
            .command_models
            .iter()
            .map(|model| model_menu::group_key(model, &self.model_details))
            .collect();
        model_menu::grouped_rows(
            &self.command_models,
            &self.model_details,
            self.current_model.as_deref(),
            &open,
            query,
        )
        .into_iter()
        .filter_map(|row| {
            let spec = row.value.strip_prefix("/model ")?.to_string();
            Some(CommandSuggestion {
                active_choice: false,
                help: if hidden.contains(&spec) {
                    "Hidden".into()
                } else {
                    "Shown".into()
                },
                action: Some(model_stage::StageStep::ToggleHidden(spec)),
                value: row.value,
                detail: None,
                header: row.header,
                toggle: None,
                context_action: None,
            })
        })
        .collect()
    }

    /// Move between picker steps, keeping the highlight sensible: step two
    /// opens on the level the model will use, and going back lands on the
    /// model that was open.
    pub(super) fn go_to_model_stage(
        &mut self,
        stage: model_stage::ModelStage,
        cx: &mut Context<Self>,
    ) {
        let from = std::mem::replace(&mut self.model_stage, stage);
        self.suggestions_revision += 1;
        self.set_content(
            if self.picker_prefix.is_some() {
                String::new()
            } else {
                "/model ".into()
            },
            cx,
        );
        let target = match &self.model_stage {
            model_stage::ModelStage::Efforts { model } => {
                let ladder = self.picker_efforts(model);
                let on_current = self.current_route() == Some(model.as_str());
                let current = on_current
                    .then_some(self.current_effort.as_deref())
                    .flatten();
                model_stage::preferred_effort(&ladder, current)
                    .map(|effort| format!("/effort {effort}"))
            }
            _ => match from {
                model_stage::ModelStage::Efforts { model } => Some(format!("/model {model}")),
                _ => None,
            },
        };
        if let Some(index) = target.and_then(|target| {
            self.command_suggestions()
                .iter()
                .position(|row| row.value == target)
        }) {
            self.command_selection = index;
            self.reveal_command(index);
        }
        cx.notify();
    }

    /// Esc inside the picker: one step back, or false when it should close.
    pub(super) fn model_stage_back(&mut self, cx: &mut Context<Self>) -> bool {
        match self.model_stage.back() {
            Some(previous) => {
                self.go_to_model_stage(previous, cx);
                true
            }
            None => false,
        }
    }

    /// Flip whether `spec` is hidden and persist it. The highlight stays put.
    pub(super) fn toggle_model_hidden(&mut self, spec: &str, cx: &mut Context<Self>) {
        let next = model_stage::toggled(&crate::config::hidden_models(), spec);
        if let Err(error) = crate::config::set_hidden_models(&next) {
            self.set_transient_notice(format!("Could not save hidden models: {error}"), cx);
            return;
        }
        self.suggestions_revision += 1;
        cx.notify();
    }

    pub(super) fn reveal_command(&self, index: usize) {
        if self.command_scroll.bounds_for_item(index).is_some() {
            self.command_scroll.scroll_to_reveal_item(index);
        } else {
            // A distant item's estimated height is not its measured height.
            // Anchor by identity instead of an estimated pixel bottom, which
            // can leave the selected row just outside the virtual viewport.
            self.command_scroll.scroll_to(gpui::ListOffset {
                item_ix: index,
                offset_in_item: px(0.),
            });
        }
    }

    /// Hover changes selection, not the catalog. Retain rows across repaints and
    /// refresh relative usage labels on the next interaction after a minute.
    pub(super) fn command_suggestions(&self) -> Arc<Vec<CommandSuggestion>> {
        if self.current_command_menu_close().is_some() {
            return Arc::new(Vec::new());
        }
        let minute = model_menu::now_unix_secs() / 60;
        let mut cache = self.suggestions_cache.borrow_mut();
        if let Some(cached) = cache.as_ref()
            && cached.content == self.content
            && cached.cursor == self.sel.cursor()
            && cached.revision == self.suggestions_revision
            && cached.minute == minute
            && cached.completion == self.command_completion
        {
            return cached.rows.clone();
        }
        let rows = Arc::new(self.build_command_suggestions());
        // Metadata updates must not reset an independently scrolled menu. Only
        // changed identities/order or row geometry require fresh measurements.
        let same_layout = cache.as_ref().is_some_and(|old| {
            old.rows.len() == rows.len()
                && old.rows.iter().zip(rows.iter()).all(|(a, b)| {
                    a.value == b.value
                        && a.header == b.header
                        && a.detail.is_some() == b.detail.is_some()
                })
        });
        if !same_layout {
            self.command_scroll
                .reset_with_uniform_height(rows.len(), px(48.));
        }
        *cache = Some(SuggestionsCache {
            content: self.content.clone(),
            cursor: self.sel.cursor(),
            revision: self.suggestions_revision,
            minute,
            completion: self.command_completion,
            rows: rows.clone(),
        });
        rows
    }

    /// Width of the name column every grid row shares: the widest name in the
    /// list (capped), plus the icon slot when any row has an icon. Descriptions
    /// start right after it, so they share one x whatever the row holds.
    fn grid_name_column(&self, window: &Window, row_width: Pixels) -> (Pixels, bool) {
        let rows = self.command_suggestions();
        let grid = || rows.iter().filter(|row| grid_description(row).is_some());
        let longest = grid()
            .map(|row| grid_label(row).chars().count())
            .max()
            .unwrap_or(0)
            .min(GRID_NAME_CHARS);
        let any_icon = grid().any(|row| {
            row.value.starts_with("/mode ")
                || matches!(
                    row.context_action,
                    Some(
                        context_picker::Action::File(_)
                            | context_picker::Action::Browse
                            | context_picker::Action::Attach
                    )
                )
        });
        let cell = window
            .text_system()
            .shape_line(
                "M".into(),
                px(GRID_TEXT),
                &[gpui::TextRun {
                    len: 1,
                    font: gpui::font(Theme::global().FONT_MONO),
                    ..Default::default()
                }],
                None,
            )
            .width;
        let icon = if any_icon { px(GRID_ICON + GRID_GAP) } else { px(0.) };
        let text = (cell * longest as f32 + px(2.)).min(row_width * GRID_NAME_SHARE);
        (icon + text, any_icon)
    }

    /// The focused grid row's whole description, shown under the list when the
    /// row cannot fit it on one line. Hover moves the focus, so the pointer
    /// reads it the same way the keyboard does.
    pub(super) fn picker_detail(
        &self,
        suggestions: &[CommandSuggestion],
        window: &Window,
    ) -> Option<gpui::Div> {
        let row = suggestions.get(self.command_selection)?;
        let text = grid_description(row)?;
        let row_width = self
            .composer_bounds
            .get()
            .map_or(window.viewport_size().width, |bounds| bounds.size.width);
        let (name_column, _) = self.grid_name_column(window, row_width);
        // Row padding, scrollbar gutter, the check mark, the column gaps and,
        // on the rows that offer one, the focused row's key hint.
        let hint = if row.context_action.is_some() && row_width >= px(340.) {
            GRID_GAP + 120.
        } else {
            0.
        };
        let reserved = name_column + px(2. * 12. + 8. + 8. + 14. + 2. * GRID_GAP + hint);
        let measured = window
            .text_system()
            .shape_line(
                text.clone().into(),
                px(GRID_TEXT),
                &[gpui::TextRun {
                    len: text.len(),
                    font: gpui::font(Theme::global().FONT_UI),
                    ..Default::default()
                }],
                None,
            )
            .width;
        (measured > row_width - reserved).then(|| {
            div()
                .debug_selector(|| "picker-detail".into())
                .flex_none()
                .px_3()
                .py_2()
                .border_t_1()
                .border_color(Theme::global().PANEL_BORDER)
                .text_size(px(11.0))
                .text_color(Theme::global().TEXT_DIM)
                .line_clamp(4)
                .child(text)
        })
    }

    pub(super) fn render_command_row(
        &self,
        index: usize,
        suggestion: CommandSuggestion,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let row_width = self
            .composer_bounds
            .get()
            .map_or(window.viewport_size().width, |bounds| bounds.size.width);
        let compact = row_width < px(430.);
        // Too narrow for the key hint to share a row with a description.
        let tiny = row_width < px(340.);
        let grid_help = grid_description(&suggestion);
        let grid = grid_help.is_some();
        let (name_column, icon_slot) = grid
            .then(|| self.grid_name_column(window, row_width))
            .map_or((None, false), |(width, icon)| (Some(width), icon));
        let is_toggle = suggestion.toggle.is_some();
        let selected = index == self.command_selection;
        let active_choice = suggestion.active_choice;
        let ink = if selected {
            Theme::global().BG
        } else {
            Theme::global().TEXT_DIM
        };
        let model = suggestion.value.strip_prefix("/model ");
        let mode = suggestion.value.strip_prefix("/mode ");
        let speed = crate::speed_icon::command_tier(&suggestion.value);
        let draft_target = crate::fixture_control_targets::draft_session().then(|| {
            model
                .map(|spec| self.model_details.get(spec).map_or(spec, |route| route.model.as_str()))
                .map(|model| format!("draft-model-row:{model}"))
                .or_else(|| (suggestion.value == "/effort high").then(|| "draft-effort-high".to_owned()))
        }).flatten();
        let active_mode =
            mode.is_some_and(|name| self.current_modes.iter().any(|active| active == name));
        let mode_icon = mode.map(|name| crate::mode_catalog::icon(name, self.mode_catalog.get(name)))
            .or_else(|| match suggestion.context_action.as_ref()? {
                context_picker::Action::File(_) => Some(crate::mode_catalog::ModeIcon::Document),
                context_picker::Action::Browse => Some(crate::mode_catalog::ModeIcon::Search),
                context_picker::Action::Attach => Some(crate::mode_catalog::ModeIcon::Document),
                context_picker::Action::Skill(_) => None,
            });
        let alternate_key = if cfg!(target_os = "macos") {
            "⌥↵"
        } else {
            "Alt+Enter"
        };
        let action_hint = match &suggestion.context_action {
            Some(context_picker::Action::File(_)) if selected && !compact => {
                Some(format!("{alternate_key} Include contents"))
            }
            Some(context_picker::Action::Skill(name)) if selected && !tiny && name != "off" => {
                let pin = match (active_mode, compact) {
                    (true, _) => "Unpin",
                    (false, true) => "Pin",
                    (false, false) => "Pin for session",
                };
                let once = if compact { "Use once" } else { "Next message only" };
                Some(format!("↵ {pin} · {alternate_key} {once}"))
            }
            _ => None,
        };
        let mode_target = mode.map(str::to_owned);
        let logo_svg = |provider: &str, fallback: &str, size: f32, color: gpui::Rgba| {
            match crate::accounts::logo(provider) {
                Some(bytes) => gpui::svg()
                    .data(bytes)
                    .size(px(size))
                    .flex_none()
                    .text_color(color)
                    .into_any_element(),
                None => div()
                    .size(px(size))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(size * 0.6))
                    .text_color(color)
                    .child(crate::accounts::lettermark(fallback))
                    .into_any_element(),
            }
        };
        // The row logo names the model's maker (OpenAI, Anthropic), never the
        // service it is routed through. The group header names that service.
        let logo = model.map(|model| {
            let title = model_menu::pretty_title(model, &self.model_details);
            div()
                .debug_selector(move || format!("model-picker-logo-{index}"))
                .child(logo_svg(
                    model_menu::model_logo(model, &self.model_details),
                    &title,
                    18.0,
                    ink,
                ))
        });
        let header = suggestion.header.clone().map(|header| {
            let faint = Theme::global().TEXT_FAINT;
            let base = div()
                .debug_selector(move || format!("model-picker-group-{index}"))
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .pt_3()
                .pb_1()
                .text_size(px(11.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(faint);
            let Some(model) = model else {
                return base.child(header);
            };
            let parts = model_menu::header_parts(model, &self.model_details);
            // Show the credential's live quota instead of how it signs in:
            // one bar per window with what is left and when it resets.
            let quota = self
                .model_details
                .get(model)
                .and_then(|detail| {
                    cx.try_global::<crate::panel::usage::StatusAccounts>()
                        .map(|accounts| {
                            model_menu::route_quota(
                                &accounts.0,
                                &detail.provider,
                                &detail.api_method,
                            )
                        })
                })
                .filter(|quota| !quota.is_empty());
            let label = header.clone();
            base.child(logo_svg(parts.logo, &parts.provider, 13.0, faint))
                .child(div().text_color(Theme::global().TEXT_DIM).child(label))
                .children(quota.map(|quota| header_quota(index, &quota)))
        });
        // Effort rows lead with the drawn level mark (never a text glyph),
        // the same shape the composer pill paints.
        let effort_mark = suggestion
            .value
            .strip_prefix("/effort ")
            .filter(|_| model.is_none())
            .and_then(|effort| {
                let effort = effort.to_string();
                crate::effort_icon::picker_icon(&effort, 14.0).map(|icon| {
                    div()
                        .debug_selector(move || format!("effort-row-icon-{index}"))
                        .flex_none()
                        .child(icon)
                })
            });
        let stage_row = suggestion.action.is_some() && model.is_none();
        let label = if let Some(speed) = speed {
            crate::speed_icon::label(speed).into()
        } else { match (model, suggestion.value.strip_prefix("/effort ")) {
            (Some(model), _) => model_menu::pretty_title(model, &self.model_details),
            // Effort rows keep the level name beside the drawn mark: a
            // picker has no hover to hide it in.
            (None, Some(effort)) => effort_row_label(effort),
            _ => mode.map_or_else(
                || suggestion.value.clone(),
                |name| {
                    if name == "off" {
                        "Clear all".into()
                    } else {
                        name.into()
                    }
                },
            ),
        }};
        let speed_tooltip = speed.and(suggestion.detail.clone());
        div()
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .when(active_choice, |row| {
                row.debug_selector(move || format!("active-picker-choice-{index}"))
            })
            .children(header)
            .child(
                div()
                    .id(("slash-command", index))
                    .relative()
                    .children(draft_target.and_then(|id| crate::fixture_control_targets::marker(|| id, || None)))
                    .debug_selector(move || {
                        if is_toggle {
                            format!("model-picker-toggle-{index}")
                        } else {
                            format!("slash-command-row-{index}")
                        }
                    })
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .when(compact && model.is_some(), |row| row.gap_1().px_2())
                    .when(!compact || model.is_none(), |row| row.gap_3().px_3())
                    .py_1p5()
                    .when(suggestion.context_action.is_some(), |row| row.rounded_full())
                    .when(suggestion.context_action.is_none(), |row| row.rounded_md())
                    .when(model.is_some() || speed.is_some() || suggestion.value.starts_with("/effort "), |row| {
                        row.border_1().border_color(match (active_choice, selected) {
                            (true, true) => crate::theme::to_hsla(Theme::global().BG),
                            (true, false) => crate::theme::to_hsla(Theme::global().TEXT),
                            (false, _) => gpui::transparent_black(),
                        })
                    })
                    .bg(if selected {
                        Theme::global().TEXT
                    } else {
                        Theme::global().HEADER_BG
                    })
                    .when(!selected, |row| {
                        row.hover(|style| style.bg(Theme::global().INPUT_BG))
                    })
                    .text_size(px(12.0))
                    .text_color(ink)
                    .cursor_pointer()
                    .when_some(speed_tooltip, |row, detail| {
                        row.tooltip(move |_, cx| cx.new(|_| crate::speed_icon::SpeedTooltip(detail.clone())).into())
                    })
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.command_selection != index {
                            this.command_selection = index;
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .flex()
                            .min_w_0()
                            .overflow_hidden()
                            .items_center()
                            .when(!grid, |row| row.flex_1())
                            .when_some(name_column, |row, width| row.flex_none().w(width))
                            .when(compact && model.is_some(), |row| row.gap_1())
                            .when(!compact || model.is_none(), |row| row.gap_3())
                            .when(icon_slot && mode_icon.is_none(), |row| {
                                row.child(div().w(px(GRID_ICON)).flex_none())
                            })
                            .children(logo)
                            .children(effort_mark)
                            .children(speed.map(|tier| div().flex_none()
                                .debug_selector(move || format!("speed-picker-icon-{index}"))
                                .child(crate::speed_icon::render(tier, 14., ink))))
                            .children(
                                mode_icon.map(|icon| {
                                    div()
                                        .debug_selector(move || format!("mode-picker-icon-{}-{index}", icon.name()))
                                        .size(px(18.0))
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon.render(16.0, ink))
                                }),
                            )
                            .font_family(if model.is_some() || speed.is_some() || stage_row {
                                Theme::global().FONT_UI
                            } else {
                                Theme::global().FONT_MONO
                            })
                            .text_color(ink)
                            .when(selected, |label| {
                                label
                                    .debug_selector(|| "picker-keyboard-focus".into())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .min_w_0()
                                    .when(grid, |column| column.flex_1())
                                    .gap_1()
                                    .child(
                                        div()
                                            .debug_selector(move || format!("picker-name-{index}"))
                                            .min_w_0()
                                            .truncate()
                                            .child(label),
                                    )
                                    .children(suggestion.detail.clone().filter(|_| !grid).map(|detail| {
                                        div()
                                            .debug_selector(move || {
                                                format!("model-picker-detail-{index}")
                                            })
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(11.0))
                                            .font_weight(gpui::FontWeight::NORMAL)
                                            .text_color(if selected {
                                                ink
                                            } else {
                                                Theme::global().TEXT_FAINT
                                            })
                                            .child(detail)
                                    })),
                            ),
                    )
                    .children(grid_help.map(|text| {
                        div()
                            .debug_selector(move || format!("picker-help-{index}"))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(if selected {
                                ink
                            } else {
                                Theme::global().TEXT_FAINT
                            })
                            .child(text)
                    }))
                    .children((!grid && !suggestion.help.is_empty()).then(|| {
                        div()
                            .debug_selector(move || format!("picker-help-{index}"))
                            .max_w(relative(0.4))
                            .min_w_0()
                            .when(mode.is_none(), |help| help.truncate())
                            .when(mode.is_some(), |help| help.whitespace_nowrap().overflow_hidden())
                            .text_color(if selected {
                                ink
                            } else {
                                Theme::global().TEXT_FAINT
                            })
                            .child(suggestion.help)
                    }))
                    .children(action_hint.map(|hint| {
                        div().flex_none().text_size(px(10.)).child(hint)
                    }))
                    .children(
                        model
                            .filter(|_| self.model_stage == model_stage::ModelStage::Models)
                            .and_then(|spec| self.render_row_speeds(spec, ink, cx)),
                    )
                    .children(
                        model.filter(|_| selected).and_then(|spec| {
                            self.render_row_efforts(index, spec, ink, compact, cx)
                        }),
                    )
                    .children(
                        (model.is_none() && !suggestion.value.starts_with("/effort ")).then(|| {
                            div()
                                .w(px(14.0))
                                .flex_none()
                                .text_color(ink)
                                .when(active_mode, |marker| {
                                    marker
                                        .debug_selector(move || {
                                            format!("mode-picker-active-{index}")
                                        })
                                        .child(
                                            crate::mode_catalog::ModeIcon::Check.render(12.0, ink),
                                        )
                                })
                                .when(mode.is_none() && selected, |marker| {
                                    marker
                                        .debug_selector(|| "slash-command-selected".into())
                                        .child("✓")
                                })
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            window.focus(&this.focus_handle, cx);
                            if let Some(action) = suggestion.context_action.clone() {
                                this.command_selection = index;
                                this.apply_context_action(action, event.modifiers.alt, window, cx);
                                return;
                            }
                            if this.embedded_command_token().is_some() {
                                this.command_selection = index;
                                this.command_selection_explicit = Some(suggestion.value.clone());
                                cx.notify();
                                return;
                            }
                            if let Some(name) = &mode_target {
                                this.command_selection = index;
                                this.toggle_mode_row(name, window, cx);
                                return;
                            }
                            if let Some(key) = suggestion.toggle.clone() {
                                this.toggle_model_group(key, cx);
                                return;
                            }
                            if let Some(step) = suggestion.action.clone() {
                                this.command_selection = index;
                                this.run_stage_step(step, false, window, cx);
                                return;
                            }
                            if matches!(suggestion.value.as_str(), "/model" | "/models") {
                                this.set_content("/model ".into(), cx);
                                if let Some(on_change) = &this.on_change {
                                    on_change(&this.content, cx);
                                }
                            } else {
                                this.submit_command(suggestion.value.clone(), false, window, cx);
                            }
                        }),
                    ),
            )
            .into_any_element()
    }

    fn render_row_speeds(
        &self,
        spec: &str,
        ink: gpui::Rgba,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        self.on_model_speed.as_ref()?;
        let active = self.row_speed(spec);
        let model = self
            .model_details
            .get(spec)
            .map_or(spec, |route| &route.model);
        let buttons: Vec<_> = speed_icon::ACCELERATED
            .into_iter()
            .filter_map(|mode| {
                let detail = self.row_speed_detail(spec, mode)?;
                let on = active == mode;
                let label = speed_icon::label(mode);
                let spec = spec.to_owned();
                let selector = format!(
                    "model-row-speed-{model}-{label}-{}",
                    if on { "on" } else { "off" }
                );
                let marker = format!("model-speed:{model}:{label}");
                let tooltip = format!(
                    "{label}. {detail} {} Option+Shift+←/→ changes speed.",
                    if on { "Click to use Standard." } else { "" }
                );
                Some(
                    div()
                        .id(format!("model-row-speed:{spec}:{label}"))
                        .relative()
                        .children(crate::fixture_control_targets::marker(|| marker, || None))
                        .debug_selector(move || selector.clone())
                        .size(px(22.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .when(on, |button| {
                            button
                                .bg(ink.opacity(0.18))
                                .border_1()
                                .border_color(ink.opacity(0.55))
                        })
                        .hover(|button| button.bg(ink.opacity(0.25)))
                        .cursor_pointer()
                        .child(speed_icon::render(
                            mode,
                            12.,
                            if on { ink } else { ink.opacity(0.75) },
                        ))
                        .tooltip(move |_, cx| {
                            cx.new(|_| speed_icon::SpeedTooltip(tooltip.clone())).into()
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                let next = if this.row_speed(&spec) == mode {
                                    SpeedMode::Standard
                                } else {
                                    mode
                                };
                                this.choose_row_speed(&spec, next, cx);
                            }),
                        ),
                )
            })
            .collect();
        (!buttons.is_empty()).then(|| {
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_0p5()
                .children(buttons)
        })
    }

    /// The highlighted model row's levels as a strip of drawn marks. The
    /// level the switch will use is filled; a click picks another. Enter or
    /// a click on the row then switches model and effort together.
    fn render_row_efforts(
        &self,
        index: usize,
        spec: &str,
        ink: gpui::Rgba,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let ladder = self.picker_efforts(spec);
        if ladder.is_empty() {
            return None;
        }
        let chosen = self.row_effort(spec);
        let commit_draft_choice = self.pending_session && self.picker_prefix == Some("/model");
        let marks = ladder.into_iter().enumerate().map(|(rung, effort)| {
            let on = chosen == Some(effort);
            let command = commit_draft_choice
                .then(|| format!("/model {spec}{}{effort}", model_stage::EFFORT_FLAG));
            div()
                .id(("model-row-effort", index * 16 + rung))
                .debug_selector(move || format!("model-row-effort-{index}-{effort}"))
                .flex()
                .items_center()
                .gap_1()
                .when(compact, |pill| pill.px_0p5())
                .when(!compact, |pill| pill.px_1p5())
                .h(px(20.))
                .rounded_full()
                .when(on, |pill| pill.bg(ink.opacity(0.14)))
                .text_size(px(10.5))
                .text_color(ink)
                .cursor_pointer()
                .children(crate::effort_icon::picker_icon(effort, 11.0))
                .when(on && !compact, |pill| pill.child(effort_row_label(effort)))
                .tooltip(move |_, cx| {
                    cx.new(|_| speed_icon::SpeedTooltip(crate::effort::tooltip(effort)))
                        .into()
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        if let Some(command) = &command {
                            this.submit_command(command.clone(), false, window, cx);
                        } else {
                            this.set_picker_effort(effort, cx);
                        }
                    }),
                )
        });
        Some(
            div()
                .debug_selector(move || format!("model-row-efforts-{index}"))
                .flex_none()
                .flex()
                .items_center()
                .gap_0p5()
                .children(marks)
                .into_any_element(),
        )
    }
}

/// The label of an `/effort <level>` row in the picker's effort step: the level
/// name, because a list row has no hover to hide it in. The level's shape is
/// drawn beside it by `effort_icon`, so the label carries no text glyph.
pub(super) fn effort_row_label(effort: &str) -> String {
    crate::effort::spoken(effort)
}

/// Quota beside a provider header: per window a short name, a thin bar, the
/// share left (or used, per `display.usage_display`), and the reset time.
fn header_quota(index: usize, quota: &model_menu::RouteQuota) -> gpui::Div {
    let theme = Theme::global();
    let show_used = jcode_base::config::config().display.usage_display_used();
    let windows = quota.windows.iter().enumerate().map(|(window, limit)| {
        let used = limit.used;
        let figure = if show_used {
            format!("{used:.0}% used")
        } else {
            format!("{:.0}% left", 100. - used)
        };
        let color = crate::panel::meter::usage_color(used, theme);
        div()
            .debug_selector(move || format!("model-picker-quota-{index}-{window}"))
            .flex()
            .items_center()
            .gap_1p5()
            .child(div().text_color(theme.TEXT_FAINT).child(limit.name.clone()))
            .child(
                div()
                    .w(px(44.))
                    .h(px(4.))
                    .rounded_full()
                    .overflow_hidden()
                    .bg(theme.INLINE_CODE_BG)
                    .child(
                        div()
                            .h_full()
                            .w(relative(used / 100.))
                            .rounded_full()
                            .bg(color),
                    ),
            )
            .child(div().text_color(color).child(figure))
            .children(limit.reset_in.clone().map(|reset| {
                div()
                    .text_color(theme.TEXT_FAINT)
                    .child(format!("· {reset}"))
            }))
    });
    div()
        .debug_selector(move || format!("model-picker-quota-{index}"))
        .ml_1()
        .flex()
        .items_center()
        .gap_3()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .font_weight(gpui::FontWeight::NORMAL)
        .font_family(theme.FONT_MONO)
        .text_size(px(10.5))
        .children(windows)
        .children((quota.banked_resets > 0).then(|| {
            let count = quota.banked_resets;
            div().text_color(theme.TEXT_DIM).child(if count == 1 {
                "↻ 1 reset".to_string()
            } else {
                format!("↻ {count} resets")
            })
        }))
        .children(
            quota
                .money
                .clone()
                .map(|money| div().text_color(theme.TEXT_DIM).child(money)),
        )
}
