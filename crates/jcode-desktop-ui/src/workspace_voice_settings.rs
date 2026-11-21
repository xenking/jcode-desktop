//! Settings > Voice: pick the shared `[dictation] backend` and store provider
//! keys exactly where jcode-voice reads them (`~/.config/jcode/<name>.env`,
//! owner-only). Applies to every Jcode client, not just this window.
use super::*;
use crate::login_input::LoginInput;
use jcode_voice::{DictationBackend, StreamingBackend};

/// Where the SuperWhisper bridge script is installed.
const SUPERWHISPER_SCRIPT: &str = include_str!("../assets/superwhisper-dictate.sh");

#[derive(Default)]
pub(super) struct VoiceSettings {
    /// Masked key entry for the selected streaming backend.
    key_input: Option<(StreamingBackend, Entity<LoginInput>)>,
    /// Plain text entry for `base_url` (Nari/Soniox) or `command`.
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

/// Install the SuperWhisper bridge next to the jcode config and return the
/// command line to store in `[dictation] command`.
fn install_superwhisper_bridge() -> std::io::Result<String> {
    let path = crate::panel::voice::superwhisper_bridge_path()
        .ok_or_else(|| std::io::Error::other("no jcode home"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, SUPERWHISPER_SCRIPT)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(crate::panel::voice::bridge_command(&path))
}

fn superwhisper_installed() -> bool {
    cfg!(target_os = "macos") && Path::new("/Applications/superwhisper.app").exists()
}

fn pill(id: &'static str, selected: bool) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::global();
    div()
        .id(id)
        .debug_selector(move || id.into())
        .flex_none()
        .px_3()
        .py_1()
        .rounded_full()
        .cursor_pointer()
        .bg(if selected {
            theme.ACCENT
        } else {
            theme.PANEL_BG
        })
        .text_color(if selected { theme.BG } else { theme.TEXT })
        .hover(|el| el.bg(theme.TOOL_BG))
}

impl Workspace {
    fn save_dictation(&mut self, key: &str, value: &str, cx: &mut Context<Self>) -> bool {
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
        if self.save_dictation("backend", backend_slug(backend), cx) {
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
        let dictation = &jcode_base::config::config().dictation;
        let current = if field == "command" {
            dictation.command.clone()
        } else {
            dictation.base_url.clone()
        };
        let weak = cx.weak_entity();
        let input = cx.new(|cx| {
            let mut input = PromptInput::new(
                cx,
                if field == "command" {
                    "shell command that prints the transcript, then Enter"
                } else {
                    "wss:// endpoint override (empty = provider default), then Enter"
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
        let dictation = jcode_base::config::config().dictation.clone();
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
            backends = backends.child(
                pill(id, backend == selected)
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.select_voice_backend(backend, cx)),
                    )
                    .child(label),
            );
        }
        let detail: String = match selected {
            DictationBackend::Auto => "Uses a Nari key if present, else a Soniox key.".into(),
            DictationBackend::ElevenLabs => {
                if StreamingBackend::ElevenLabs.api_key().is_some() {
                    "Uses your own ElevenLabs key. SuperWhisper is not needed.".into()
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
                format!("{key}. Read from {var} or ~/.config/jcode/{file}.")
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
                pill(id, mode == dictation.mode)
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.select_voice_mode(mode, cx)),
                    )
                    .child(label),
            );
        }
        if let Some(b) = streaming(selected) {
            actions = actions.child(
                pill("settings-voice-key", false)
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.open_voice_key_input(b, cx)),
                    )
                    .child("Set API key"),
            );
            if b != StreamingBackend::ElevenLabs {
                actions = actions.child(
                    pill("settings-voice-base-url", false)
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.open_voice_text_input("base_url", cx)
                            }),
                        )
                        .child(if dictation.base_url.trim().is_empty() {
                            "Custom endpoint".to_string()
                        } else {
                            format!("Endpoint: {}", dictation.base_url.trim())
                        }),
                );
            }
        }
        if selected == DictationBackend::Command {
            actions = actions.child(
                pill("settings-voice-command-edit", false)
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.open_voice_text_input("command", cx)),
                    )
                    .child("Edit command"),
            );
        }
        if superwhisper_installed() {
            actions = actions.child(
                pill("settings-voice-superwhisper", false)
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.use_superwhisper(cx)),
                    )
                    .child("Use SuperWhisper"),
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
                    pill("settings-voice-key-save", true)
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.save_voice_key(cx)),
                        )
                        .child("Save"),
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
}
