use super::palette::{parse_color, rgb_c, rgba_c};
use super::presets::{apply_code_colors, builtin as raw_themes};
use super::*;
use gpui::{Rgba, rgb};

fn interpolate(from: &Palette, to: &Palette, amount: f32) -> Palette {
    from.mixed(to, amount)
}

#[test]
fn config_default_theme_is_the_default_preset() {
    assert_eq!(crate::config::DEFAULT_THEME, ThemePreset::default().id());
}

/// Same-process microbenchmark, not a render/frame-time measurement.
/// Run with `justrust test -p jcode-desktop-ui-core --lib
/// stable_lookup_benchmark -- --ignored --nocapture`.
#[test]
#[ignore = "manual timing comparison, not a performance assertion"]
fn stable_lookup_benchmark() {
    use std::hint::black_box;
    use std::time::Instant;

    // Keep the original implementation here, including its transition
    // branch, so both paths are measured with the same compiler/profile.
    #[inline(never)]
    fn original_global() -> &'static Palette {
        let target = ACTIVE_THEME.load(Ordering::Relaxed);
        let mut transition = transition_state().lock().unwrap();
        if let Some((from, started)) = *transition {
            let progress = started.elapsed().as_secs_f32() / 0.18;
            if progress < 1.0 {
                let step = (progress * 16.0).round().clamp(0.0, 16.0) as usize;
                return &transition_frames(from, target)[step];
            }
            *transition = None;
        }
        &themes()[target]
    }

    #[inline(never)]
    fn stable_global() -> &'static Palette {
        Theme::global()
    }

    fn measure(lookup: fn() -> &'static Palette, count: usize) -> f64 {
        let started = Instant::now();
        for _ in 0..count {
            black_box(black_box(lookup)());
        }
        started.elapsed().as_nanos() as f64 / count as f64
    }

    let _guard = test_theme_lock();
    let original = Theme::active_preset();
    Theme::select(ThemePreset::ALL[0]);
    assert!(std::ptr::eq(original_global(), stable_global()));
    const COUNT: usize = 1_000_000;
    const ROUNDS: usize = 9;
    measure(original_global, 10_000);
    measure(stable_global, 10_000);
    let mut old = [0.0; ROUNDS];
    let mut new = [0.0; ROUNDS];
    for round in 0..ROUNDS {
        // Alternate order to reduce warmup/thermal/order bias.
        if round % 2 == 0 {
            old[round] = measure(original_global, COUNT);
            new[round] = measure(stable_global, COUNT);
        } else {
            new[round] = measure(stable_global, COUNT);
            old[round] = measure(original_global, COUNT);
        }
    }
    eprintln!("theme lookup ns/call original={old:?} stable={new:?}");
    old.sort_by(f64::total_cmp);
    new.sort_by(f64::total_cmp);
    let before = old[ROUNDS / 2];
    let after = new[ROUNDS / 2];
    eprintln!(
        "theme lookup median: original={before:.2} ns/call stable={after:.2} ns/call speedup={:.2}x reduction={:.1}% ({ROUNDS} alternating rounds x {COUNT} lookups)",
        before / after,
        (1.0 - after / before) * 100.0,
    );
    Theme::select(original);
}

#[test]
fn stable_lookup_does_not_take_transition_lock() {
    let _guard = test_theme_lock();
    let original = Theme::active_preset();
    Theme::select(ThemePreset::ALL[0]);
    {
        let _transition = transition_state().lock().unwrap();
        assert!(std::ptr::eq(
            Theme::global(),
            Theme::preview(ThemePreset::ALL[0])
        ));
        assert!(!Theme::is_transitioning());
    }
    Theme::select(original);
}

#[test]
fn animated_selection_preserves_frames_and_returns_to_fast_path() {
    let _guard = test_theme_lock();
    let original = Theme::active_preset();
    select_theme(0, false);
    select_theme(1, true);
    assert!(Theme::is_transitioning());
    assert_eq!(Theme::active_preset(), ThemePreset::ALL[1]);
    assert!(std::ptr::eq(Theme::selected(), &themes()[1]));
    let frames = transition_frames(0, 1);
    {
        let mut transition = transition_state().lock().unwrap();
        assert_eq!(transition.as_ref().unwrap().0, 0);
        // Place the fade safely in its interior without sleeping.
        transition.as_mut().unwrap().1 =
            std::time::Instant::now() - std::time::Duration::from_millis(90);
    }
    let frame = Theme::global();
    assert!(
        frames
            .iter()
            .any(|candidate| std::ptr::eq(candidate, frame))
    );
    {
        let mut transition = transition_state().lock().unwrap();
        transition.as_mut().unwrap().1 =
            std::time::Instant::now() - std::time::Duration::from_secs(1);
    }
    assert!(std::ptr::eq(Theme::global(), &themes()[1]));
    assert!(!Theme::is_transitioning());
    assert!(transition_state().lock().unwrap().is_none());
    Theme::select(original);
}

#[test]
fn concurrent_selection_publishes_matching_target_and_transition() {
    let _guard = test_theme_lock();
    let original = Theme::active_preset();
    std::thread::scope(|scope| {
        for target in 0..4 {
            scope.spawn(move || {
                for iteration in 0..100 {
                    select_theme(target, iteration % 2 == 0);
                    let transition = transition_state().lock().unwrap();
                    let active = ACTIVE_THEME.load(Ordering::Acquire);
                    assert_eq!(active & TRANSITIONING != 0, transition.is_some());
                    assert!((active & !TRANSITIONING) < ThemePreset::ALL.len());
                    drop(transition);
                    let _ = Theme::global();
                }
            });
        }
    });
    Theme::select(original);
}

fn syntax_colors(theme: &Palette) -> [Rgba; 13] {
    [
        theme.CODE_TEXT,
        theme.CODE_KEYWORD,
        theme.CODE_STRING,
        theme.CODE_COMMENT,
        theme.CODE_NUMBER,
        theme.CODE_TYPE,
        theme.CODE_PUNCT,
        theme.CODE_FUNCTION,
        theme.CODE_VARIABLE,
        theme.CODE_CONTROL,
        theme.CODE_CONSTANT,
        theme.CODE_TAG,
        theme.CODE_ATTRIBUTE,
    ]
}

#[test]
fn vscode_syntax_colors_match_every_raw_preset_and_remain_readable() {
    for (preset, theme) in ThemePreset::ALL.into_iter().zip(raw_themes()) {
        let light = luminance(theme.CODE_BG) > 0.179;
        let expected = if light {
            [
                0x000000, 0x0000ff, 0xa31515, 0x008000, 0x098658, 0x267f99, 0x000000, 0x795e26,
                0x001080, 0xaf00db, 0x0070c1, 0x800000, 0xe50000,
            ]
        } else {
            [
                0xd4d4d4, 0x569cd6, 0xce9178, 0x6a9955, 0xb5cea8, 0x4ec9b0, 0xd4d4d4, 0xdcdcaa,
                0x9cdcfe, 0xc586c0, 0x4fc1ff, 0x569cd6, 0x9cdcfe,
            ]
        };
        for (color, original) in syntax_colors(&theme).into_iter().zip(expected.map(rgb_c)) {
            if contrast(original, theme.CODE_BG) >= 4.5 {
                assert_eq!(
                    color, original,
                    "{preset:?}: unnecessary palette adjustment"
                );
            } else {
                let direction = if light { -1.0 } else { 1.0 };
                assert!((color.r - original.r) * direction >= 0.0);
                assert!((color.g - original.g) * direction >= 0.0);
                assert!((color.b - original.b) * direction >= 0.0);
            }
            assert!(
                contrast(color, theme.CODE_BG) >= 4.5,
                "{preset:?}: syntax contrast {}",
                contrast(color, theme.CODE_BG)
            );
        }
        assert!(contrast(theme.CODE_TEXT, theme.CODE_BG) >= 4.5);
    }
}

#[test]
fn syntax_selection_uses_code_background_and_preserves_surfaces() {
    let original = Palette::defaults();
    let mut theme = original.clone();
    theme.CODE_BG = rgb_c(0xffffff);
    apply_code_colors(&mut theme);
    assert_eq!(theme.CODE_FUNCTION, rgb_c(0x795e26));
    assert_eq!(theme.BG, original.BG);
    assert_eq!(theme.PANEL_BG, original.PANEL_BG);
    assert_eq!(theme.TEXT, original.TEXT);
    assert_eq!(theme.ACCENT, original.ACCENT);
    assert_eq!(theme.CODE_BG, rgb_c(0xffffff));
    assert_eq!(theme.CODE_BORDER, original.CODE_BORDER);
    assert_eq!(theme.CODE_HEADER_BG, original.CODE_HEADER_BG);
    assert_eq!(theme.CODE_GUTTER, original.CODE_GUTTER);
    assert_eq!(theme.INLINE_CODE_BG, original.INLINE_CODE_BG);
    theme.BG = rgb_c(0xffffff);
    theme.CODE_BG = rgb_c(0x000000);
    apply_code_colors(&mut theme);
    assert_eq!(theme.CODE_FUNCTION, rgb_c(0xdcdcaa));
}

#[test]
fn new_code_roles_accept_overrides_and_interpolate() {
    let mut from = Palette::defaults();
    apply_code_colors(&mut from);
    let mut to = from.clone();
    let custom = rgba_c(0x12345678);
    for role in [
        "CODE_FUNCTION",
        "code-variable",
        "code_control",
        "CODE_CONSTANT",
        "code-tag",
        "code_attribute",
    ] {
        to.set_color(role, custom);
    }
    let mixed = interpolate(&from, &to, 0.5);
    for ((start, end), mid) in syntax_colors(&from)[7..]
        .iter()
        .zip(&syntax_colors(&to)[7..])
        .zip(&syntax_colors(&mixed)[7..])
    {
        assert_eq!(*end, custom);
        assert!((mid.r - (start.r + end.r) / 2.0).abs() < 0.00001);
        assert!((mid.g - (start.g + end.g) / 2.0).abs() < 0.00001);
        assert!((mid.b - (start.b + end.b) / 2.0).abs() < 0.00001);
        assert!((mid.a - (start.a + end.a) / 2.0).abs() < 0.00001);
    }
    assert_eq!(
        syntax_colors(&interpolate(&from, &to, 0.0)),
        syntax_colors(&from)
    );
    assert_eq!(
        syntax_colors(&interpolate(&from, &to, 1.0)),
        syntax_colors(&to)
    );
}

#[test]
fn ai_font_is_independent_and_defaults_to_ui_font() {
    let mut theme = Palette::defaults();
    let original_ui = theme.FONT_UI;
    let original_mono = theme.FONT_MONO;
    let appearance = crate::config::AppearanceConfig {
        ai_font: Some("Urbanist".into()),
        ..Default::default()
    };
    theme.apply_fonts(&appearance);
    assert_eq!(theme.FONT_AI, "Urbanist");
    assert_eq!(theme.FONT_UI, original_ui);
    assert_eq!(theme.FONT_MONO, original_mono);
    let mut fallback = Palette::defaults();
    fallback.apply_fonts(&crate::config::AppearanceConfig {
        ui_font: Some("Inter".into()),
        mono_font: Some("Test Mono".into()),
        ..Default::default()
    });
    assert_eq!(fallback.FONT_AI, "Inter");
    assert_eq!(fallback.FONT_UI, "Inter");
    assert_eq!(fallback.FONT_MONO, "Test Mono");
    assert_eq!(interpolate(&theme, &theme, 0.5).FONT_AI, "Urbanist");
}

#[test]
fn colors_accept_rgb_and_rgba_hex() {
    assert_eq!(parse_color("#ff0080").unwrap().a, 1.0);
    assert!((parse_color("10203040").unwrap().a - 64.0 / 255.0).abs() < 0.001);
    assert!(parse_color("purple").is_none());
}

fn luminance(color: Rgba) -> f32 {
    let channel = |value: f32| {
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}

fn over(top: Rgba, bottom: Rgba) -> Rgba {
    Rgba {
        r: top.r * top.a + bottom.r * (1.0 - top.a),
        g: top.g * top.a + bottom.g * (1.0 - top.a),
        b: top.b * top.a + bottom.b * (1.0 - top.a),
        a: 1.0,
    }
}

/// Opaque color a surface role paints on screen. Translucent palettes
/// stack roles over the canvas, and the canvas over the blurred wallpaper.
/// Measure against a white wallpaper, the worst case for light text.
fn painted(theme: &Palette, role: Rgba) -> Rgba {
    let canvas = over(theme.BG, rgb_c(0xffffff));
    if role == theme.BG {
        canvas
    } else if role == theme.PANEL_BG || role == theme.HEADER_BG {
        over(role, canvas)
    } else {
        over(role, over(theme.PANEL_BG, canvas))
    }
}

fn contrast(a: Rgba, b: Rgba) -> f32 {
    let (bright, dark) = if luminance(a) > luminance(b) {
        (luminance(a), luminance(b))
    } else {
        (luminance(b), luminance(a))
    };
    (bright + 0.05) / (dark + 0.05)
}

#[test]
fn workspace_identities_are_neutral_and_readable_in_every_palette() {
    for (preset, theme) in ThemePreset::ALL.iter().zip(themes()) {
        for row in 0..4 {
            let accent = theme.workspace_accent(row);
            assert_eq!(accent, theme.TEXT_DIM);
            for other in 0..row {
                assert_eq!(accent, theme.workspace_accent(other));
            }
            for background in [
                theme.PANEL_BG,
                theme.MINIMAP_BG,
                theme.HEADER_BG.blend(accent.opacity(0.05)),
            ] {
                assert!(
                    contrast(accent, background) >= 4.5,
                    "{preset:?} workspace {row}: contrast {} against {background:?}",
                    contrast(accent, background)
                );
            }
        }
    }
}

#[test]
fn light_neutral_matches_sampled_reference_surfaces() {
    let theme = &raw_themes()[ThemePreset::LightNeutral.index()];
    assert_eq!(theme.PANEL_BG, rgb_c(0xffffff));
    assert_eq!(theme.BG, rgb_c(0xf6f6f6));
    assert_eq!(theme.HEADER_BG, rgb_c(0xf4f4f4));
    assert_eq!(theme.USER_BG, rgb_c(0xf4f4f4));
    assert_eq!(theme.INPUT_BG, rgb_c(0xffffff));
    assert_eq!(theme.TEXT, rgb_c(0x3b3d3f));
    assert_eq!(theme.PANEL_BORDER_FOCUS, rgb_c(0x858585));
    assert_eq!(theme.ACCENT_DIM, rgb_c(0xe9eaea));
    assert_eq!(theme.HEADING, rgb_c(0x1a1c1f));
    assert!(contrast(theme.LINK, theme.PANEL_BG) >= 4.5);
    assert!(contrast(theme.TEXT_DIM, theme.HEADER_BG) >= 4.5);
}

#[test]
fn light_neutral_preserves_old_ids_and_untinted_prompt_paper() {
    let preset = ThemePreset::LightNeutral;
    assert_eq!(ThemePreset::from_id("chatgpt-light"), preset);
    assert_eq!(ThemePreset::from_id("light-neutral"), preset);
    assert_eq!(preset.id(), "light-neutral");
    assert_eq!(preset.label(), "Light Neutral");
    let theme = &raw_themes()[preset.index()];
    for age in [0, 1, 6, 40, usize::MAX] {
        assert_eq!(theme.prompt_background(age), theme.USER_BG);
    }
    for color in [
        theme.ACCENT,
        theme.ACCENT_DIM,
        theme.PANEL_BORDER_FOCUS,
        theme.USER_BG,
        theme.TOOL_BG,
        theme.CODE_BG,
    ] {
        assert!((color.r - color.b).abs() <= 4.0 / 255.0 + f32::EPSILON);
    }
    assert!(contrast(theme.PANEL_BORDER_FOCUS, theme.INPUT_BG) >= 3.0);
    assert!(contrast(theme.LINK, theme.PANEL_BG) >= 4.5);
    let warm = Palette::defaults();
    assert_eq!(warm.PROMPT_TINT_STRENGTH, 0.05);
    let mixed = interpolate(&warm, theme, 0.5);
    assert!((mixed.PROMPT_TINT_STRENGTH - 0.025).abs() < f32::EPSILON);
    assert_eq!(interpolate(&warm, theme, 1.0).PROMPT_TINT_STRENGTH, 0.0);
}

#[test]
fn dark_neutral_is_untinted_gray_with_blue_only_for_links() {
    let preset = ThemePreset::DarkNeutral;
    assert_eq!(ThemePreset::from_id("dark-neutral"), preset);
    assert_eq!(preset.label(), "Dark Neutral");
    let theme = &raw_themes()[preset.index()];
    assert_eq!(theme.PANEL_BG, rgb_c(0x212121));
    assert_eq!(theme.USER_BG, rgb_c(0x303030));
    assert!(luminance(theme.PANEL_BG) < 0.05);
    for age in [0, 1, 6, 40, usize::MAX] {
        assert_eq!(theme.prompt_background(age), theme.USER_BG);
    }
    for color in [
        theme.BG,
        theme.PANEL_BG,
        theme.HEADER_BG,
        theme.ACCENT,
        theme.ACCENT_DIM,
        theme.PANEL_BORDER_FOCUS,
        theme.USER_BG,
        theme.TOOL_BG,
        theme.CODE_BG,
        theme.INPUT_BG,
    ] {
        assert!((color.r - color.b).abs() <= 4.0 / 255.0 + f32::EPSILON);
    }
    assert!(contrast(theme.PANEL_BORDER_FOCUS, theme.INPUT_BG) >= 3.0);
    assert!(contrast(theme.LINK, theme.PANEL_BG) >= 4.5);
    assert!(contrast(theme.TEXT_DIM, theme.HEADER_BG) >= 4.5);
    assert!(contrast(theme.HEADING, theme.USER_BG) >= 7.0);
}

#[test]
fn pure_black_uses_true_black_canvas_and_focused_panes() {
    let preset = ThemePreset::PureBlack;
    assert_eq!(ThemePreset::from_id("pure-black"), preset);
    assert_eq!(preset.label(), "Pure Black");
    let theme = &raw_themes()[preset.index()];
    assert_eq!(theme.BG, rgb_c(0x000000));
    assert_eq!(theme.PANEL_BG, rgb_c(0x000000));
    assert_eq!(theme.INPUT_BG, rgb_c(0x000000));
    for age in [0, 1, 6, 40, usize::MAX] {
        assert_eq!(theme.prompt_background(age), theme.USER_BG);
    }
    assert!(contrast(theme.PANEL_BORDER_FOCUS, theme.INPUT_BG) >= 3.0);
    assert!(contrast(theme.LINK, theme.PANEL_BG) >= 4.5);
    assert!(contrast(theme.TEXT_DIM, theme.HEADER_BG) >= 4.5);
    assert!(contrast(theme.HEADING, theme.USER_BG) >= 7.0);
}

#[test]
fn glass_is_the_only_translucent_palette_and_keeps_text_readable() {
    let preset = ThemePreset::Glass;
    assert_eq!(ThemePreset::from_id("glass"), preset);
    assert_eq!(preset.label(), "Glass");
    for (other, theme) in ThemePreset::ALL.into_iter().zip(raw_themes()) {
        assert_eq!(theme.is_translucent(), other == preset, "{other:?}");
    }
    let theme = &raw_themes()[preset.index()];
    assert!(theme.BG.a < 0.75 && theme.PANEL_BG.a < 1.0);
    for age in [0, 1, 6, 40, usize::MAX] {
        assert_eq!(theme.prompt_background(age), theme.USER_BG);
        assert_eq!(theme.prompt_background(age).a, 1.0);
    }
    // Worst case for light text: a bright wallpaper blurred behind every
    // translucent layer.
    for role in [
        theme.PANEL_BG,
        theme.HEADER_BG,
        theme.INPUT_BG,
        theme.CODE_BG,
    ] {
        let painted = painted(theme, role);
        assert!(
            contrast(theme.TEXT, painted) >= 4.5,
            "{}",
            contrast(theme.TEXT, painted)
        );
    }
    assert!(contrast(theme.HEADING, theme.USER_BG) >= 7.0);
}

#[test]
fn ristretto_preset_has_soft_semantic_colors() {
    let preset = ThemePreset::MonokaiProRistretto;
    assert_eq!(ThemePreset::from_id("monokai-pro-ristretto"), preset);
    let theme = &raw_themes()[preset.index()];
    assert_eq!(theme.PANEL_BG, rgb_c(0x2c2525));
    assert_eq!(theme.TEXT, rgb_c(0xfff1f3));
    assert_eq!(theme.ACCENT, rgb_c(0xf66f88));
    assert_eq!(theme.OK, rgb_c(0xadd57d));
}

#[test]
fn preset_ids_indices_and_cycle_cover_every_palette() {
    let mut visited = std::collections::HashSet::new();
    let mut current = ThemePreset::ALL[0];
    for (index, preset) in ThemePreset::ALL.into_iter().enumerate() {
        assert_eq!(preset.index(), index);
        assert_eq!(ThemePreset::from_id(preset.id()), preset);
        assert_eq!(current, preset);
        assert!(visited.insert(preset.id()));
        current = current.next();
    }
    assert_eq!(current, ThemePreset::WarmNeutral);
    assert_eq!(ThemePreset::default(), ThemePreset::PureBlack);
    assert_eq!(ThemePreset::from_id("unknown"), ThemePreset::PureBlack);
    assert_eq!(ThemePreset::from_id(""), ThemePreset::PureBlack);
}

#[test]
fn new_palettes_keep_status_colors_legible() {
    for preset in &ThemePreset::ALL[4..] {
        let theme = &themes()[preset.index()];
        for (background, foregrounds) in [
            (
                theme.PANEL_BG,
                vec![
                    theme.TEXT_FAINT,
                    theme.LINK,
                    theme.ACCENT,
                    theme.AI_ACCENT,
                    theme.ERROR,
                    theme.OK,
                    theme.WARN,
                ],
            ),
            (theme.TOOL_BG, vec![theme.TOOL_TEXT]),
            (theme.HEADER_BG, vec![theme.TEXT_DIM]),
            (theme.INLINE_CODE_BG, vec![theme.CODE_TEXT]),
        ] {
            let background = painted(theme, background);
            for foreground in foregrounds {
                assert!(
                    contrast(foreground, background) >= 4.5,
                    "{} contrast was {} for {foreground:?}",
                    preset.id(),
                    contrast(foreground, background)
                );
            }
        }
    }
}

#[test]
fn every_preset_keeps_semantic_text_legible() {
    let themes = themes();
    for (preset, theme) in ThemePreset::ALL.into_iter().zip(themes) {
        for (foreground, background, role) in [
            (theme.TEXT, theme.PANEL_BG, "panel text"),
            (theme.TEXT_DIM, theme.PANEL_BG, "secondary text"),
            (theme.REASONING, theme.PANEL_BG, "thinking text"),
            (theme.OK, theme.PANEL_BG, "normal token badge"),
            (theme.WARN, theme.PANEL_BG, "warning token badge"),
            (theme.ERROR, theme.PANEL_BG, "large token badge"),
            (theme.CODE_TEXT, theme.CODE_BG, "code text"),
            (theme.TEXT_USER, theme.USER_BG, "user text"),
        ] {
            assert!(
                contrast(foreground, background) >= 4.5,
                "{} {role} contrast was {}",
                preset.id(),
                contrast(foreground, background)
            );
        }
    }
}

#[test]
fn prompt_age_tints_reverse_cli_hues_and_fade_back_to_card_paper() {
    let mut theme = Palette::defaults();
    theme.USER_BG = rgb(0x808080);
    let colors: Vec<_> = (0..7).map(|age| theme.prompt_background(age)).collect();
    assert!(colors[0].b > colors[0].r && colors[0].r > colors[0].g);
    assert!(colors[1].b > colors[1].g && colors[1].g > colors[1].r);
    assert!(colors[2].b > colors[2].g && colors[2].g > colors[2].r);
    assert!(colors[3].g > colors[3].b && colors[3].b > colors[3].r);
    assert!(colors[4].r > colors[4].g && colors[4].g > colors[4].b);
    assert!(colors[5].r > colors[5].g && colors[5].g > colors[5].b);
    assert!(colors[6].r > colors[6].g && colors[6].g == colors[6].b);
    let old = theme.prompt_background(32);
    assert!((old.r - theme.USER_BG.r).abs() < 0.00001);
    assert!((old.g - theme.USER_BG.g).abs() < 0.00001);
    assert!((old.b - theme.USER_BG.b).abs() < 0.00001);
    assert_eq!(theme.prompt_background(usize::MAX), theme.USER_BG);
}

#[test]
fn popover_paper_is_opaque_and_preserves_palette_rgb() {
    for mut theme in themes().iter().cloned() {
        for alpha in [0.0, 0.25, 0.6, 0.78, 1.0] {
            theme.PANEL_BG.a = alpha;
            let paper = theme.popover_background();
            assert_eq!(paper.a, 1.0);
            assert_eq!(
                (paper.r, paper.g, paper.b),
                (theme.PANEL_BG.r, theme.PANEL_BG.g, theme.PANEL_BG.b)
            );
            assert_eq!(theme.PANEL_BG.a, alpha, "window paper stays translucent");
        }
    }
}

/// Composer pills are chips on the pane surface, and that surface is
/// PANEL_BG while the pane owns focus but the brighter HEADER_BG while it
/// does not. The pill used prompt paper, which sits only a few steps from
/// PANEL_BG in several palettes (ristretto: 4/255), so one pill read fine
/// on an idle pane and vanished on the focused one. The chip keeps its
/// step on both surfaces.
#[test]
fn composer_chips_lift_off_focused_and_idle_panes() {
    for (preset, theme) in ThemePreset::ALL.iter().zip(themes()) {
        let chip = theme.chip_background();
        for (surface, label) in [
            (painted(theme, theme.PANEL_BG), "focused pane"),
            (painted(theme, theme.HEADER_BG), "idle pane"),
        ] {
            let fill = over(chip, surface);
            assert!(
                contrast(fill, surface) >= 1.15,
                "{} chip on the {label} is only {}:1",
                preset.id(),
                contrast(fill, surface)
            );
        }
    }
}

#[test]
fn composer_secondary_labels_meet_aa_in_light_and_dark() {
    for preset in [
        ThemePreset::NeutralLight,
        ThemePreset::NeutralDark,
        ThemePreset::LightNeutral,
        ThemePreset::DarkNeutral,
        ThemePreset::PureBlack,
    ] {
        let theme = &themes()[preset.index()];
        let overlay = painted(theme, theme.HEADER_BG);
        let panel = painted(theme, theme.PANEL_BG);
        let ink = theme.composer_secondary();
        for (background, label) in [
            (overlay, "work overlay / idle status"),
            (over(theme.USER_BG, overlay), "hovered and selected rows"),
            (panel, "focused status"),
            (painted(theme, theme.INPUT_BG), "prompt input"),
            (over(theme.chip_background(), panel), "focused pill"),
            (over(theme.chip_background(), overlay), "idle pill"),
        ] {
            let ratio = contrast(ink, background);
            println!("{} {label}: {ratio:.2}:1", preset.id());
            assert!(ratio >= 4.5, "{} {label}: {ratio:.2}:1", preset.id());
        }
    }
}

#[test]
fn prompt_cards_are_opaque_even_with_translucent_custom_paper() {
    let mut theme = themes()[0].clone();
    for alpha in [0.0, 0.25, 0.78, 1.0] {
        theme.USER_BG.a = alpha;
        for distance in [0, 1, 12, usize::MAX] {
            assert_eq!(theme.prompt_background(distance).a, 1.0);
        }
    }
}

#[test]
fn prompt_age_tints_stay_subtle_and_readable_in_every_palette() {
    for (preset, theme) in ThemePreset::ALL.into_iter().zip(themes()) {
        for age in 0..40 {
            let background = theme.prompt_background(age);
            assert_eq!(background.a, 1.0);
            let difference = (background.r - theme.USER_BG.r)
                .abs()
                .max((background.g - theme.USER_BG.g).abs())
                .max((background.b - theme.USER_BG.b).abs());
            assert!(
                difference <= 0.05 + f32::EPSILON,
                "{} age {age}: excessive tint",
                preset.id()
            );
            assert!(
                contrast(theme.TEXT_DIM, background) >= 4.5,
                "{} age {age}: prompt number contrast {}",
                preset.id(),
                contrast(theme.TEXT_DIM, background)
            );
            assert!(
                contrast(theme.TEXT_USER, background) >= 4.5,
                "{} age {age}: prompt contrast {}",
                preset.id(),
                contrast(theme.TEXT_USER, background)
            );
        }
    }
}

#[test]
fn every_preset_distinguishes_pane_focus_without_dimming_text() {
    for (preset, theme) in ThemePreset::ALL.into_iter().zip(themes()) {
        assert_eq!(theme.panel_background(true), theme.PANEL_BG);
        assert_eq!(
            theme.panel_background(false).a == 1.0,
            !theme.is_translucent()
        );
        let active = painted(theme, theme.panel_background(true));
        let inactive = painted(theme, theme.panel_background(false));
        let difference = (active.r - inactive.r)
            .abs()
            .max((active.g - inactive.g).abs())
            .max((active.b - inactive.b).abs());
        assert!(
            difference >= 10.0 / 255.0,
            "{} lacks pane contrast",
            preset.id()
        );
        for foreground in [theme.TEXT, theme.TEXT_DIM, theme.REASONING] {
            assert!(
                contrast(foreground, inactive) >= 4.5,
                "{} inactive text contrast was {}",
                preset.id(),
                contrast(foreground, inactive)
            );
        }
    }
}

#[test]
fn transition_frames_begin_and_end_at_the_selected_palettes() {
    for from in 0..ThemePreset::ALL.len() {
        for to in 0..ThemePreset::ALL.len() {
            let frames = transition_frames(from, to);
            assert_eq!(frames[0].BG, themes()[from].BG);
            assert_eq!(frames[16].BG, themes()[to].BG);
            assert_eq!(frames.len(), 17);
            assert!(std::ptr::eq(frames, transition_frames(from, to)));
        }
    }
}
