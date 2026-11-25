//! Math and Mermaid diagrams rendered as images, with their caches.
use super::*;

#[derive(Clone)]
pub(super) struct RenderedMath {
    pub(super) svg: Arc<[u8]>,
    pub(super) width: f32,
    pub(super) height: f32,
}

pub(super) const MATH_CACHE_CAPACITY: usize = 128;
pub(super) static MATH_CACHE: LazyLock<Mutex<VecDeque<(u64, RenderedMath)>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

/// Typeset display math natively and embed the glyph outlines in a crisp SVG.
pub(super) fn render_math_svg(source: &str) -> Result<RenderedMath, String> {
    let key = hash(source);
    if let Ok(mut cache) = MATH_CACHE.lock()
        && let Some(index) = cache.iter().position(|(cached_key, _)| *cached_key == key)
    {
        let entry = cache.remove(index).expect("cached math entry exists");
        let rendered = entry.1.clone();
        cache.push_back(entry);
        return Ok(rendered);
    }

    let source = source.to_owned();
    let rendered = std::panic::catch_unwind(move || {
        let ast = ratex_parser::parse(&source).map_err(|error| error.to_string())?;
        let layout = ratex_layout::layout(&ast, &ratex_layout::LayoutOptions::default());
        let display_list = ratex_layout::to_display_list(&layout);
        let font_size = 20.0;
        let padding = 3.0;
        let options = ratex_svg::SvgOptions {
            font_size,
            padding,
            stroke_width: 1.0,
            embed_glyphs: true,
            font_dir: String::new(),
        };
        let width = (display_list.width * font_size + padding * 2.0) as f32;
        let height =
            ((display_list.height + display_list.depth) * font_size + padding * 2.0) as f32;
        // currentColor allows one cached SVG to follow live theme changes.
        let svg = ratex_svg::render_to_svg(&display_list, &options)
            .replace("rgba(0,0,0,1)", "currentColor");
        Ok::<_, String>(RenderedMath {
            svg: Arc::from(svg.into_bytes()),
            width: width.max(1.0),
            height: height.max(1.0),
        })
    })
    .map_err(|_| "math renderer panicked".to_string())??;

    if let Ok(mut cache) = MATH_CACHE.lock() {
        cache.push_back((key, rendered.clone()));
        while cache.len() > MATH_CACHE_CAPACITY {
            cache.pop_front();
        }
    }
    Ok(rendered)
}

pub(super) fn math_block(
    source: &str,
    selection: &gpui::Entity<TextSelection>,
    key: SharedString,
    window: &gpui::Window,
    cx: &gpui::App,
) -> gpui::AnyElement {
    let shell = div()
        .debug_selector(|| "md-math".into())
        .w_full()
        // Equations are transcript content, not code cards. Keep the SVG
        // transparent and let ordinary block spacing separate display math.
        .my_2()
        .text_color(Theme::global().TEXT);

    match render_math_svg(source) {
        Ok(rendered) => shell
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .child(
                gpui::svg()
                    .data(rendered.svg.as_ref())
                    .w(px(rendered.width))
                    .h(px(rendered.height))
                    .max_w_full()
                    .text_color(Theme::global().TEXT),
            )
            .into_any_element(),
        Err(_) => shell
            .text_center()
            .text_size(px(16.0))
            .child(text_selection::plain(
                selection.clone(),
                key,
                latex_to_text(source),
                window,
                cx,
            ))
            .into_any_element(),
    }
}

#[derive(Clone)]
pub(super) struct RenderedMermaid {
    pub(super) native: Arc<crate::native_mermaid::NativeMermaid>,
}

pub(super) const MERMAID_CACHE_CAPACITY: usize = 64;
pub(super) static MERMAID_CACHE: LazyLock<Mutex<VecDeque<(u64, RenderedMermaid)>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

pub(super) fn mermaid_theme(theme: &Palette) -> mermaid_rs_renderer::Theme {
    fn hex(color: gpui::Rgba) -> String {
        format!(
            "#{:02x}{:02x}{:02x}",
            (color.r * 255.0).round() as u8,
            (color.g * 255.0).round() as u8,
            (color.b * 255.0).round() as u8
        )
    }
    let mut diagram = if theme.PANEL_BG.r + theme.PANEL_BG.g + theme.PANEL_BG.b < 1.5 {
        mermaid_rs_renderer::Theme::dark()
    } else {
        mermaid_rs_renderer::Theme::modern()
    };
    diagram.font_family = theme.FONT_UI.to_owned();
    diagram.font_size = 14.0;
    diagram.background = hex(theme.PANEL_BG);
    diagram.primary_color = hex(theme.USER_BG);
    diagram.primary_text_color = hex(theme.TEXT);
    diagram.primary_border_color = hex(theme.TEXT_DIM);
    diagram.text_color = hex(theme.TEXT);
    diagram.line_color = hex(theme.TEXT_DIM);
    diagram.secondary_color = hex(theme.TOOL_BG);
    diagram.tertiary_color = hex(theme.PANEL_BG);
    diagram.edge_label_background = hex(theme.PANEL_BG);
    diagram.cluster_background = hex(theme.QUOTE_BG);
    diagram.cluster_border = hex(theme.PANEL_BORDER);
    diagram.sequence_actor_fill = diagram.primary_color.clone();
    diagram.sequence_actor_border = diagram.primary_border_color.clone();
    diagram.sequence_actor_line = diagram.line_color.clone();
    diagram.sequence_note_fill = diagram.secondary_color.clone();
    diagram.sequence_note_border = diagram.primary_border_color.clone();
    diagram.sequence_activation_fill = diagram.secondary_color.clone();
    diagram.sequence_activation_border = diagram.primary_border_color.clone();
    diagram.pie_title_text_color = diagram.text_color.clone();
    diagram.pie_section_text_color = diagram.text_color.clone();
    diagram.pie_legend_text_color = diagram.text_color.clone();
    diagram
}

pub(super) fn render_mermaid_scene(
    body: &str,
    theme: mermaid_rs_renderer::Theme,
) -> Result<RenderedMermaid, String> {
    // Include the effective palette, not just source. Switching desktop themes
    // must never reuse paths filled with colors from the previous theme.
    let key = hash(&format!("{body}\0{theme:?}"));
    if let Ok(mut cache) = MERMAID_CACHE.lock()
        && let Some(index) = cache.iter().position(|(cached_key, _)| *cached_key == key)
    {
        let entry = cache.remove(index).expect("cached Mermaid entry exists");
        let rendered = entry.1.clone();
        cache.push_back(entry);
        return Ok(rendered);
    }

    // Incomplete streamed model output must not take down the desktop.
    let source = body.to_owned();
    let rendered = std::panic::catch_unwind(move || {
        let options = mermaid_rs_renderer::RenderOptions {
            theme,
            ..Default::default()
        };
        let scene = mermaid_rs_renderer::render_scene(&source, options)
            .map_err(|error| error.to_string())?;
        let native = Arc::new(crate::native_mermaid::NativeMermaid::new(&scene)?);
        Ok::<_, String>(RenderedMermaid { native })
    })
    .map_err(|_| "Mermaid renderer panicked".to_string())??;

    if let Ok(mut cache) = MERMAID_CACHE.lock() {
        cache.push_back((key, rendered.clone()));
        while cache.len() > MERMAID_CACHE_CAPACITY {
            cache.pop_front();
        }
    }
    Ok(rendered)
}

#[cfg(test)]
pub(crate) fn mermaid_natural_size(body: &str) -> (f32, f32) {
    let rendered = render_mermaid_scene(body, mermaid_theme(Theme::selected()))
        .expect("valid regression diagram");
    (rendered.native.width(), rendered.native.height())
}

#[derive(Clone, Default)]
pub(crate) struct MediaPreviewHandler {
    pub(crate) fit_scroll: Option<crate::inline_image::FitScroll>,
    pub(crate) on_gesture: Option<crate::inline_image::GestureHandler>,
}

/// Paint mmdr vector primitives directly into the native transcript. Incomplete streamed
/// diagrams retain the lightweight text representation until they become
/// valid, rather than flashing an error into the transcript.
pub(super) fn mermaid_diagram(
    body: &str,
    key: SharedString,
    on_preview: Option<MediaPreviewHandler>,
) -> gpui::AnyElement {
    let theme = mermaid_theme(Theme::selected());
    if let Ok(rendered) = render_mermaid_scene(body, theme.clone()) {
        let navigation = on_preview.unwrap_or_default();
        // Source and palette identity keep gestures stable across cache eviction
        // and concurrent scene preparation, but reset state for streamed edits.
        let diagram_key = format!("{key}-{}", hash(&format!("{body}\0{theme:?}"))).into();
        return div()
            .id(key)
            .debug_selector(|| "md-mermaid".into())
            .my_1()
            .w_full()
            .min_w_0()
            .child(
                crate::inline_image::InlineImage::diagram(diagram_key, rendered.native.clone())
                    .navigation(navigation.fit_scroll, navigation.on_gesture),
            )
            .into_any_element();
    }

    let mut lines = body.lines().filter_map(mermaid_display_line).peekable();
    let rows: Vec<String> = lines.by_ref().collect();
    div()
        .debug_selector(|| "md-mermaid".into())
        .flex()
        .flex_col()
        .my_1()
        .w_full()
        .overflow_hidden()
        .rounded_md()
        .border_1()
        .border_color(Theme::global().PANEL_BORDER)
        .bg(Theme::global().QUOTE_BG)
        .child(
            div()
                .px_2p5()
                .py_1()
                .border_b_1()
                .border_color(Theme::global().PANEL_BORDER)
                .text_size(px(10.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(Theme::global().TEXT_DIM)
                .child("DIAGRAM"),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_1p5()
                .px_3()
                .py_2p5()
                .children(rows.into_iter().map(|line| {
                    let connector = line.trim_start().starts_with(['→', '←', '↔']);
                    div()
                        .max_w_full()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .when(!connector, |el| {
                            el.border_1()
                                .border_color(Theme::global().CODE_BORDER)
                                .bg(Theme::global().CODE_BG)
                        })
                        .font_family(Theme::global().FONT_MONO)
                        .text_size(px(12.0))
                        .text_color(if connector {
                            Theme::global().ACCENT_MUTED
                        } else {
                            Theme::global().TEXT
                        })
                        .child(line)
                })),
        )
        .into_any_element()
}

pub(super) fn mermaid_display_line(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with("%%") || line.starts_with("%%{") {
        return None;
    }
    let lower = line.to_ascii_lowercase();
    if [
        "graph ",
        "flowchart ",
        "sequencediagram",
        "classdiagram",
        "statediagram",
        "erdiagram",
        "gantt",
        "pie",
        "journey",
        "mindmap",
        "timeline",
    ]
    .iter()
    .any(|kind| lower.starts_with(kind))
    {
        return None;
    }
    let mut text = line
        .replace("<-->", " ↔ ")
        .replace("-->", " → ")
        .replace("==>", " ⇒ ")
        .replace("-.->", " ⇢ ")
        .replace("<--", " ← ")
        .replace("->>", " → ")
        .replace("-->>", " → ")
        .replace(":::", "  ");
    for (from, to) in [
        ("[", ""),
        ("]", ""),
        ("(", ""),
        (")", ""),
        ("{", ""),
        ("}", ""),
        ("\"", ""),
    ] {
        text = text.replace(from, to);
    }
    Some(text.split_whitespace().collect::<Vec<_>>().join(" "))
}

// --- Rendering -------------------------------------------------------------
