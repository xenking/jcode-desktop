//! Settings > Voice: pick the shared `[dictation] backend` and store provider
//! keys exactly where jcode-voice reads them (`~/.config/jcode/<name>.env`,
//! owner-only). Applies to every Jcode client, not just this window.
use super::*;
use crate::components::{Pill, PillVariant};
use crate::login_input::LoginInput;
use jcode_desktop_model::{fs_probe, probe_cache::ProbeCache};
use jcode_voice::{DictationBackend, StreamingBackend};

/// Where the SuperWhisper bridge script is installed.
const SUPERWHISPER_SCRIPT: &str = include_str!("../../../assets/superwhisper-dictate.sh");

#[derive(Default)]
pub(super) struct VoiceSettings {
    /// Masked key entry for the selected streaming backend.
    key_input: Option<(StreamingBackend, Entity<LoginInput>)>,
    /// Plain text entry for `model`, `base_url` (Nari/Soniox) or `command`.
    text_input: Option<(&'static str, Entity<PromptInput>)>,
}

pub(super) const BACKENDS: [(DictationBackend, &str, &str); 6] = [
    (DictationBackend::Auto, "auto", "Auto"),
    (DictationBackend::Nari, "nari", "Nari"),
    (DictationBackend::Soniox, "soniox", "Soniox"),
    (
        DictationBackend::ElevenLabs,
        "elevenlabs",
        "ElevenLabs Scribe",
    ),
    (
        DictationBackend::Subscription,
        "subscription",
        "Jcode subscription",
    ),
    (DictationBackend::Command, "command", "External command"),
];

pub(super) fn backend_slug(backend: DictationBackend) -> &'static str {
    BACKENDS
        .iter()
        .find(|(b, ..)| *b == backend)
        .map_or("auto", |(_, slug, _)| slug)
}

/// `[dictation] mode` choices, in the order the Settings row shows them. The
/// default comes first: voice is an input method, so the words land in the
/// composer for editing unless the user asks for an immediate send.
pub(super) const MODES: [(jcode_base::protocol::TranscriptMode, &str, &str); 4] = [
    (
        jcode_base::protocol::TranscriptMode::Insert,
        "insert",
        "Insert at cursor",
    ),
    (
        jcode_base::protocol::TranscriptMode::Append,
        "append",
        "Append to draft",
    ),
    (
        jcode_base::protocol::TranscriptMode::Replace,
        "replace",
        "Replace draft",
    ),
    (
        jcode_base::protocol::TranscriptMode::Send,
        "send",
        "Send immediately",
    ),
];

pub(super) fn mode_slug(mode: jcode_base::protocol::TranscriptMode) -> &'static str {
    MODES
        .iter()
        .find(|(m, ..)| *m == mode)
        .map_or("insert", |(_, slug, _)| slug)
}

fn streaming(backend: DictationBackend) -> Option<StreamingBackend> {
    match backend {
        DictationBackend::Nari => Some(StreamingBackend::Nari),
        DictationBackend::Soniox => Some(StreamingBackend::Soniox),
        DictationBackend::ElevenLabs => Some(StreamingBackend::ElevenLabs),
        _ => None,
    }
}

#[cfg(test)]
thread_local! {
    /// Tests pick the `[dictation]` snapshot Settings renders, and read back what
    /// Settings asked to save, without touching the shared config file.
    static TEST_DICTATION: std::cell::RefCell<Option<jcode_base::config::DictationConfig>> =
        const { std::cell::RefCell::new(None) };
    static TEST_SAVED: std::cell::RefCell<Vec<(String, String)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The `[dictation]` settings Settings shows and edits.
fn dictation_snapshot() -> jcode_base::config::DictationConfig {
    #[cfg(test)]
    if let Some(config) = TEST_DICTATION.with(|c| c.borrow().clone()) {
        return config;
    }
    jcode_base::config::config().dictation.clone()
}

/// The model a backend will request: the saved one, else its default.
fn model_label(backend: StreamingBackend, saved: &str) -> String {
    let saved = saved.trim();
    if saved.is_empty() {
        format!("{} (default)", backend.default_model())
    } else {
        saved.to_owned()
    }
}

/// Install the SuperWhisper bridge next to the jcode config and return the
/// command line to store in `[dictation] command`.
fn install_superwhisper_bridge() -> std::io::Result<String> {
    jcode_desktop_voice::superwhisper::install(SUPERWHISPER_SCRIPT)
}

thread_local! {
    static SUPERWHISPER: std::cell::RefCell<ProbeCache<(), bool>> =
        std::cell::RefCell::new(ProbeCache::new(std::time::Duration::from_secs(30), 1));
}

/// Whether the app is installed, as of the last background check. The button
/// appears once the answer lands. Render never touches the disk.
fn superwhisper_installed(cx: &gpui::App) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let installed = SUPERWHISPER.with_borrow_mut(|cache| cache.get(&()).copied().unwrap_or(false));
    crate::background_probe::drain_local(cx, &SUPERWHISPER, |_| {
        fs_probe::off_main(|| fs_probe::exists(Path::new("/Applications/superwhisper.app")))
    });
    installed
}

/// A voice setting choice: the selected one is the filled accent pill.
fn pill(id: &'static str, label: impl Into<gpui::SharedString>, selected: bool) -> Pill {
    let variant = if selected {
        PillVariant::Primary
    } else {
        PillVariant::Secondary
    };
    Pill::new(variant, id, label)
        .debug_selector(id)
        .px_3()
        .py_1()
        .text_size(px(11.0))
}

impl Workspace {
    fn save_dictation(&mut self, key: &str, value: &str, cx: &mut Context<Self>) -> bool {
        #[cfg(test)]
        TEST_SAVED.with(|saved| saved.borrow_mut().push((key.to_owned(), value.to_owned())));
        match crate::config::persist_dictation_value(key, value) {
            Ok(()) => {
                jcode_base::config::Config::invalidate_cache();
                true
            }
            Err(error) => {
                self.status = format!("Could not save voice setting: {error}");
                cx.notify();
                false
            }
        }
    }

    fn select_voice_backend(&mut self, backend: DictationBackend, cx: &mut Context<Self>) {
        let changed = dictation_snapshot().backend != backend;
        if self.save_dictation("backend", backend_slug(backend), cx) {
            // A model name belongs to one provider. Carrying it to another
            // backend would request a model that provider does not have.
            if changed && !self.save_dictation("model", "", cx) {
                return;
            }
            self.voice_settings = VoiceSettings::default();
            self.status = format!("Voice backend: {}", backend_slug(backend));
        }
        cx.notify();
    }

    fn select_voice_mode(
        &mut self,
        mode: jcode_base::protocol::TranscriptMode,
        cx: &mut Context<Self>,
    ) {
        if self.save_dictation("mode", mode_slug(mode), cx) {
            self.status = format!("Voice mode: {}", mode_slug(mode));
        }
        cx.notify();
    }

    fn open_voice_key_input(&mut self, backend: StreamingBackend, cx: &mut Context<Self>) {
        let input = cx.new(|cx| LoginInput::new(cx, "Paste API key, then Save"));
        self.voice_settings.key_input = Some((backend, input));
        cx.notify();
    }

    fn save_voice_key(&mut self, cx: &mut Context<Self>) {
        let Some((backend, input)) = self.voice_settings.key_input.take() else {
            return;
        };
        let key = input.update(cx, |input, cx| input.take(cx));
        self.status = match backend.save_api_key(&key) {
            Ok(()) => format!("Saved key to ~/.config/jcode/{}", backend.key_location().1),
            Err(error) => {
                self.voice_settings.key_input = Some((backend, input));
                format!("Could not save key: {error}")
            }
        };
        cx.notify();
    }

    fn open_voice_text_input(&mut self, field: &'static str, cx: &mut Context<Self>) {
        let dictation = dictation_snapshot();
        let current = match field {
            "command" => dictation.command.clone(),
            "model" => dictation.model.clone(),
            _ => dictation.base_url.clone(),
        };
        let weak = cx.weak_entity();
        let input = cx.new(|cx| {
            let mut input = PromptInput::new(
                cx,
                match field {
                    "command" => "shell command that prints the transcript, then Enter",
                    "model" => "model name (empty = backend default), then Enter",
                    _ => "wss:// endpoint override (empty = provider default), then Enter",
                },
                move |text, _, _, app| {
                    let _ = weak.update(app, |this, cx| {
                        if this.save_dictation(field, text.trim(), cx) {
                            this.voice_settings.text_input = None;
                            this.status = format!("Saved dictation {field}");
                        }
                        cx.notify();
                    });
                },
            )
            .without_chrome()
            .without_command_completion();
            input.set_content(current, cx);
            input
        });
        self.voice_settings.text_input = Some((field, input));
        cx.notify();
    }

    fn use_superwhisper(&mut self, cx: &mut Context<Self>) {
        match install_superwhisper_bridge() {
            Ok(command) => {
                if self.save_dictation("command", &command, cx)
                    && self.save_dictation("backend", "command", cx)
                {
                    self.voice_settings = VoiceSettings::default();
                    self.status =
                        "Voice uses SuperWhisper. Use Stop voice to finish recording.".into();
                }
            }
            Err(error) => self.status = format!("Could not install SuperWhisper bridge: {error}"),
        }
        cx.notify();
    }

    pub(super) fn render_voice_settings(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::global();
        let dictation = dictation_snapshot();
        let selected = dictation.backend;
        let mut backends = div().flex().flex_wrap().gap_1();
        for (backend, slug, label) in BACKENDS {
            let id: &'static str = match slug {
                "auto" => "settings-voice-auto",
                "nari" => "settings-voice-nari",
                "soniox" => "settings-voice-soniox",
                "elevenlabs" => "settings-voice-elevenlabs",
                "subscription" => "settings-voice-subscription",
                _ => "settings-voice-command",
            };
            backends = backends.child(pill(id, label, backend == selected).on_click(
                cx.listener(move |this, _, _, cx| this.select_voice_backend(backend, cx)),
            ));
        }
        let detail: String = match selected {
            DictationBackend::Auto => {
                "Uses a Nari key if present, else a Soniox key. Pick one to choose its model.".into()
            }
            DictationBackend::ElevenLabs => {
                if StreamingBackend::ElevenLabs.api_key().is_some() {
                    format!(
                        "Uses your own ElevenLabs key. SuperWhisper is not needed. Model: {}.",
                        model_label(StreamingBackend::ElevenLabs, &dictation.model)
                    )
                } else {
                    "Set an ElevenLabs API key. A SuperWhisper Pro license does not grant API access.".into()
                }
            }
            DictationBackend::Nari | DictationBackend::Soniox => {
                let b = streaming(selected).expect("streaming");
                let (var, file) = b.key_location();
                let key = if b.api_key().is_some() {
                    "Key saved"
                } else {
                    "No key yet"
                };
                format!(
                    "{key}. Read from {var} or ~/.config/jcode/{file}. Model: {}.",
                    model_label(b, &dictation.model)
                )
            }
            DictationBackend::Subscription => {
                let base = jcode_base::subscription_api::configured_api_base();
                format!(
                    "Jcode account voice via {base} (JCODE_API_BASE). Desktop streaming support is pending."
                )
            }
            DictationBackend::Command => {
                if dictation.command.trim().is_empty() {
                    "No command set. It must print the transcript to stdout.".into()
                } else {
                    format!("Runs: {}", dictation.command.trim())
                }
            }
        };
        let mut actions = div().flex().flex_wrap().gap_1();
        let mut modes = div().flex().flex_wrap().gap_1();
        for (mode, slug, label) in MODES {
            let id: &'static str = match slug {
                "insert" => "settings-voice-mode-insert",
                "append" => "settings-voice-mode-append",
                "replace" => "settings-voice-mode-replace",
                _ => "settings-voice-mode-send",
            };
            modes = modes.child(
                pill(id, label, mode == dictation.mode)
                    .on_click(cx.listener(move |this, _, _, cx| this.select_voice_mode(mode, cx))),
            );
        }
        if let Some(b) = streaming(selected) {
            actions = actions.child(
                pill("settings-voice-key", "Set API key", false)
                    .on_click(cx.listener(move |this, _, _, cx| this.open_voice_key_input(b, cx))),
            );
            actions = actions.child(
                pill(
                    "settings-voice-model",
                    format!("Model: {}", model_label(b, &dictation.model)),
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| this.open_voice_text_input("model", cx))),
            );
            if b != StreamingBackend::ElevenLabs {
                actions = actions.child(
                    pill(
                        "settings-voice-base-url",
                        if dictation.base_url.trim().is_empty() {
                            "Custom endpoint".to_string()
                        } else {
                            format!("Endpoint: {}", dictation.base_url.trim())
                        },
                        false,
                    )
                    .on_click(
                        cx.listener(|this, _, _, cx| this.open_voice_text_input("base_url", cx)),
                    ),
                );
            }
        }
        if selected == DictationBackend::Command {
            actions = actions.child(
                pill("settings-voice-command-edit", "Edit command", false).on_click(
                    cx.listener(|this, _, _, cx| this.open_voice_text_input("command", cx)),
                ),
            );
        }
        if superwhisper_installed(cx) {
            actions = actions.child(
                pill("settings-voice-superwhisper", "Use SuperWhisper", false)
                    .on_click(cx.listener(|this, _, _, cx| this.use_superwhisper(cx))),
            );
        }
        let key_editor = self.voice_settings.key_input.as_ref().map(|(_, input)| {
            div()
                .flex()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .rounded_full()
                        .overflow_hidden()
                        .border_1()
                        .border_color(theme.INPUT_BORDER)
                        .child(input.clone()),
                )
                .child(
                    pill("settings-voice-key-save", "Save", true)
                        .on_click(cx.listener(|this, _, _, cx| this.save_voice_key(cx))),
                )
        });
        let text_editor = self.voice_settings.text_input.as_ref().map(|(_, input)| {
            div()
                .id("settings-voice-text")
                .debug_selector(|| "settings-voice-text".into())
                .px_3()
                .py_1()
                .rounded_full()
                .border_1()
                .border_color(theme.INPUT_BORDER)
                .bg(theme.INPUT_BG)
                .child(input.clone())
        });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .mt_2()
            .child(
                div()
                    .text_color(theme.TEXT_DIM)
                    .child("Voice backend · all Jcode clients"),
            )
            .child(backends)
            .child(div().text_color(theme.TEXT_DIM).child(detail))
            .child(actions)
            .children(key_editor)
            .children(text_editor)
            .child(
                div()
                    .text_color(theme.TEXT_DIM)
                    .child("After speaking · where the words go"),
            )
            .child(modes)
            .child(
                div()
                    .text_color(theme.TEXT_DIM)
                    .child("Insert at cursor leaves the words in the composer for you to edit; only Send immediately submits them."),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(dictation: jcode_base::config::DictationConfig) {
        TEST_DICTATION.with(|c| *c.borrow_mut() = Some(dictation));
        TEST_SAVED.with(|s| s.borrow_mut().clear());
    }

    fn soniox_with_model(model: &str) -> jcode_base::config::DictationConfig {
        jcode_base::config::DictationConfig {
            backend: DictationBackend::Soniox,
            model: model.into(),
            ..Default::default()
        }
    }

    fn settings_window(
        cx: &mut gpui::TestAppContext,
    ) -> (gpui::Entity<Workspace>, &mut gpui::VisualTestContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.sidebar_view = SidebarView::Settings;
            workspace
        });
        vcx.run_until_parked();
        (workspace, vcx)
    }

    /// The Model pill shows the model that will be requested, only for a backend
    /// that has one, and opens an editor prefilled with the saved value.
    #[gpui::test]
    fn settings_show_and_edit_the_model_of_the_selected_backend(cx: &mut gpui::TestAppContext) {
        show(jcode_base::config::DictationConfig::default());
        let (workspace, vcx) = settings_window(cx);
        assert!(vcx.debug_bounds("settings-voice-soniox").is_some());
        assert!(
            vcx.debug_bounds("settings-voice-model").is_none(),
            "auto has no single model to pick"
        );

        show(soniox_with_model("stt-rt-chosen"));
        workspace.update(vcx, |_, cx| cx.notify());
        vcx.run_until_parked();
        let pill = vcx
            .debug_bounds("settings-voice-model")
            .expect("a streaming backend shows its model");
        vcx.simulate_click(pill.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        workspace.read_with(vcx, |workspace, cx| {
            let (field, input) = workspace.voice_settings.text_input.as_ref().unwrap();
            assert_eq!(*field, "model");
            assert_eq!(input.read(cx).content.as_ref(), "stt-rt-chosen");
        });

        // An empty model is shown as the backend default, not as a blank.
        assert_eq!(model_label(StreamingBackend::Soniox, ""), "stt-rt-v6 (default)");
        assert_eq!(model_label(StreamingBackend::Soniox, " stt-x "), "stt-x");
        TEST_DICTATION.with(|c| *c.borrow_mut() = None);
    }

    /// A model name belongs to one provider: switching backend saves the
    /// backend and clears the model, picking the same backend again does not.
    #[gpui::test]
    fn switching_backend_clears_the_model_and_reselecting_keeps_it(
        cx: &mut gpui::TestAppContext,
    ) {
        show(soniox_with_model("stt-rt-chosen"));
        let (_workspace, vcx) = settings_window(cx);
        let nari = vcx.debug_bounds("settings-voice-nari").unwrap();
        vcx.simulate_click(nari.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(
            TEST_SAVED.with(|s| s.borrow().clone()),
            [("backend".into(), "nari".into()), ("model".into(), "".into())]
        );

        show(soniox_with_model("stt-rt-chosen"));
        let soniox = vcx.debug_bounds("settings-voice-soniox").unwrap();
        vcx.simulate_click(soniox.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(
            TEST_SAVED.with(|s| s.borrow().clone()),
            [("backend".into(), "soniox".into())]
        );
        TEST_DICTATION.with(|c| *c.borrow_mut() = None);
    }

    #[test]
    fn backend_slugs_match_the_shared_config_names() {
        for (backend, slug, _) in BACKENDS {
            let parsed: jcode_base::config::Config =
                toml::from_str(&format!("[dictation]\nbackend = \"{slug}\"\n")).unwrap();
            assert_eq!(parsed.dictation.backend, backend);
            assert_eq!(backend_slug(backend), slug);
        }
    }

    #[test]
    fn mode_slugs_match_the_shared_config_names() {
        for (mode, slug, _) in MODES {
            let parsed: jcode_base::config::Config =
                toml::from_str(&format!("[dictation]\nmode = \"{slug}\"\n")).unwrap();
            assert_eq!(parsed.dictation.mode, mode);
            assert_eq!(mode_slug(mode), slug);
        }
        // The row leads with the mode a fresh install uses.
        assert_eq!(
            MODES[0].0,
            jcode_base::config::Config::default().dictation.mode
        );
    }

    #[test]
    fn superwhisper_bridge_prints_only_the_result_field() {
        assert!(SUPERWHISPER_SCRIPT.starts_with("#!/bin/sh"));
        assert!(SUPERWHISPER_SCRIPT.contains("superwhisper://record"));
        assert!(SUPERWHISPER_SCRIPT.contains("-extract result raw"));
    }

    /// A long dictation reaches the bridge's recording limit, or is interrupted
    /// by the caller's own limit. Either way the bridge stops SuperWhisper and
    /// still collects the transcript instead of abandoning the recording.
    #[test]
    fn superwhisper_bridge_stops_and_collects_instead_of_abandoning() {
        assert!(SUPERWHISPER_SCRIPT.contains("trap finish INT TERM"));
        assert!(SUPERWHISPER_SCRIPT.contains("superwhisper://record/stop"));
        let limit_branch = SUPERWHISPER_SCRIPT.rsplit_once("done\n").unwrap().1;
        assert_eq!(limit_branch.trim(), "finish");
    }

    /// The owner's choice, as written by Settings, is the backend and model
    /// every dictation start uses. Keys exist for all providers, so any
    /// fallback would show up as the wrong backend.
    #[test]
    fn saved_backend_and_model_are_the_ones_started() {
        use jcode_voice::{StreamingBackend, plan_streaming, trigger_route, TriggerRoute};
        let dir = tempfile::tempdir().unwrap();
        for (backend, slug, _) in BACKENDS {
            let path = dir.path().join(format!("{slug}.toml"));
            crate::config::persist_dictation_value_at(&path, "command", "my-whisper").unwrap();
            crate::config::persist_dictation_value_at(&path, "backend", slug).unwrap();
            crate::config::persist_dictation_value_at(&path, "model", "chosen-model").unwrap();
            let saved: jcode_base::config::Config =
                toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(saved.dictation.backend, backend, "{slug} persisted");
            assert_eq!(saved.dictation.model, "chosen-model");
            let plan = plan_streaming(&saved.dictation, |b| Some(format!("key-{}", b.name())));
            match backend {
                DictationBackend::Auto => {
                    assert_eq!(plan.unwrap().backend, StreamingBackend::Nari)
                }
                DictationBackend::Nari
                | DictationBackend::Soniox
                | DictationBackend::ElevenLabs => {
                    let plan = plan.unwrap();
                    assert_eq!(Some(plan.backend), streaming(backend), "{slug} started");
                    assert_eq!(plan.effective_model(), "chosen-model");
                }
                DictationBackend::Subscription | DictationBackend::Command => {
                    assert!(plan.is_err(), "{slug} has no live capture")
                }
            }
            // A saved command must never start for a streaming backend.
            let expected = match backend {
                DictationBackend::Command | DictationBackend::Auto => TriggerRoute::Command,
                _ => TriggerRoute::Streaming,
            };
            assert_eq!(trigger_route(&saved.dictation), expected, "{slug} trigger");
        }
    }
}
