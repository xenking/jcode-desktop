use super::*;
use crate::code_block::{flatten_overlapping_highlights, highlight_code};

#[test]
fn clean_chat_keeps_public_markdown_and_literal_reasoning_markup() {
    let literal = jcode_render_core::reasoning_line_markup("## Private marker in a code sample");
    let private = jcode_render_core::reasoning_line_markup("## 私的な推論");
    assert!(public_transcript(&private).trim().is_empty());
    let source = format!("## Public heading\n\n```md\n{literal}```\n\n{private}Visible answer");
    assert_eq!(
        public_transcript(&source),
        format!("## Public heading\n\n```md\n{literal}```\n\nVisible answer")
    );
}

#[gpui::test]
fn pinned_prompt_selection_namespace_is_independent(cx: &mut gpui::TestAppContext) {
    struct PromptCopies {
        selection: gpui::Entity<TextSelection>,
    }
    impl gpui::Render for PromptCopies {
        fn render(
            &mut self,
            window: &mut gpui::Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            let preview = MediaPreviewHandler::default();
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(render_prompt(
                    "Full prompt",
                    4,
                    &self.selection,
                    window,
                    cx,
                    preview.clone(),
                    Theme::global().QUOTE_BG,
                ))
                .child(render_prompt_with_prefix(
                    "Pinned preview",
                    4,
                    "pinned-4",
                    &self.selection,
                    window,
                    cx,
                    preview,
                    Theme::global().QUOTE_BG,
                ))
        }
    }
    let (view, vcx) = cx.add_window_view(|_, cx| PromptCopies {
        selection: cx.new(TextSelection::new),
    });
    vcx.run_until_parked();
    for (selector, expected) in [
        ("selectable-text-4-0", "Full prompt"),
        ("selectable-text-pinned-4-0", "Pinned preview"),
    ] {
        let bounds = vcx.debug_bounds(selector).expect("independent prompt leaf");
        vcx.simulate_event(gpui::MouseDownEvent {
            button: gpui::MouseButton::Left,
            position: bounds.center(),
            modifiers: gpui::Modifiers::default(),
            click_count: 4,
            first_mouse: false,
        });
        let selection = view.read_with(vcx, |view, _| view.selection.clone());
        vcx.update(|_, cx| selection.update(cx, |selection, cx| selection.copy(cx)));
        let copied = vcx
            .update(|_, cx| cx.read_from_clipboard())
            .and_then(|item| item.text());
        assert_eq!(copied.as_deref(), Some(expected));
    }
    assert_eq!(
        selection_segments("Full prompt", 4, false, true)[0]
            .0
            .as_ref(),
        "4-0"
    );
}

fn extracted(source: &str, row: usize, reasoning: bool, prompt: bool) -> Vec<(String, String)> {
    selection_segments(source, row, reasoning, prompt)
        .into_iter()
        .map(|(key, text)| (key.to_string(), text.to_string()))
        .collect()
}

#[test]
fn selection_segments_preserve_leaf_order_and_plain_text() {
    let source = "# **Heading**\n\nProse [link](https://example.com)\ncontinued.\n\n- [x] `done`\n2. *next*\n\n> **quote**\n>\n> > nested\n\n***\n\n```rust\n  let café = 1;\n\t// text\n```";
    let expected = [
        ("9-0", "Heading"),
        ("9-1", "Prose link continued."),
        ("9-2", "done"),
        ("9-3", "next"),
        ("9-4-0", "quote"),
        ("9-4-1", "> nested"),
        ("9-6", "  let café = 1;\n\t// text"),
    ];
    assert_eq!(
        extracted(source, 9, false, false),
        expected.map(|(k, t)| (k.into(), t.into()))
    );
    assert_eq!(
        extracted("a\nb", 0, false, true),
        [("0-0".into(), "a\nb".into())]
    );
    assert_eq!(
        extracted("**`Title`**", 0, true, false),
        [("0-0".into(), "Title".into())]
    );
    assert!(selection_segments("\n***\n", 0, false, false).is_empty());
}

#[test]
fn selection_segments_tables_are_positional_unique_and_padded() {
    let source = "| same | same |\n| --- | --- |\n| same | same | same |\n| same |";
    let segments = extracted(source, 3, false, false);
    assert_eq!(segments.len(), 9);
    let keys: std::collections::HashSet<_> = segments.iter().map(|(key, _)| key).collect();
    assert_eq!(keys.len(), segments.len());
    for (index, (key, _)) in segments.iter().enumerate() {
        assert_eq!(key, &format!("3-0-{}-{}", index / 3, index % 3));
    }
    assert_eq!(segments[2].1, "");
    assert_eq!(segments[7].1, "");
    assert_eq!(segments[8].1, "");
}

#[test]
fn selection_segments_nested_reasoning_quotes_keep_their_namespace() {
    let inner: String = "> **nested**\n>\n> last"
        .lines()
        .map(jcode_render_core::reasoning_line_markup)
        .collect();
    let source: String = inner
        .lines()
        .map(jcode_render_core::reasoning_line_markup)
        .collect();
    assert_eq!(
        extracted(&source, 7, false, false),
        [
            ("7-0-0-0-0".into(), "nested".into()),
            ("7-0-0-0-1".into(), "last".into()),
        ]
    );
}

#[test]
fn selection_segments_code_and_media_use_readable_bodies() {
    for lang in [
        "rust",
        "text",
        "unknown",
        "diff",
        "patch",
        "html-preview",
        "mermaid",
        "mmd",
    ] {
        let body = "  α <b> & `literal`\n\tsecond";
        let source = format!("```{lang}\n{body}\n```");
        assert_eq!(
            extracted(&source, 4, false, false),
            [("4-0".into(), body.into())]
        );
        if !matches!(lang, "diff" | "patch" | "html-preview" | "mermaid" | "mmd") {
            assert_eq!(highlight_code(body, lang).0, body);
        }
    }
    assert_eq!(
        extracted("$$x^2$$", 4, false, false),
        [("4-0".into(), "x^2".into())]
    );
}

#[gpui::test]
fn selection_segments_match_rendered_leaf_keys_and_copy(cx: &mut gpui::TestAppContext) {
    let nested: String = ["> **quoted**", ">", "> second"]
        .into_iter()
        .map(jcode_render_core::reasoning_line_markup)
        .collect();
    // Separate small documents keep every tested leaf inside the test viewport.
    for source in [
        "# **Heading**\n\nProse `code`\n\n- [x] done\n2. next".to_owned(),
        "```rust\n  let x = 1;\n\t// café\n```".to_owned(),
        nested,
        "| same | same |\n| --- | --- |\n| same | same |".to_owned(),
    ] {
        let expected = selection_segments(&source, 0, false, false);
        let (view, vcx) = cx.add_window_view(|_, cx| RestoredReasoningView {
            selection: cx.new(TextSelection::new),
            source,
        });
        vcx.run_until_parked();
        for (key, text) in expected {
            let bounds = vcx
                .debug_bounds(Box::leak(format!("selectable-text-{key}").into_boxed_str()))
                .unwrap_or_else(|| panic!("missing rendered leaf {key}"));
            vcx.simulate_event(gpui::MouseDownEvent {
                button: gpui::MouseButton::Left,
                position: bounds.center(),
                modifiers: gpui::Modifiers::default(),
                click_count: 4,
                first_mouse: false,
            });
            let selection = view.read_with(vcx, |view, _| view.selection.clone());
            vcx.update(|_, cx| selection.update(cx, |selection, cx| selection.copy(cx)));
            let copied = vcx
                .update(|_, cx| cx.read_from_clipboard())
                .and_then(|item| item.text());
            assert_eq!(copied.as_deref(), Some(text.as_ref()), "leaf {key}");
        }
    }
}

#[test]
fn commonmark_hard_line_breaks_are_rendered() {
    assert_eq!(
        parse("Two spaces  \nBackslash\\\nsoft\nwrap\\\\\nend"),
        vec![Block::Paragraph(
            "Two spaces\nBackslash\nsoft wrap\\\\ end".into()
        )]
    );
}

#[test]
fn prompt_line_breaks_are_preserved_without_changing_assistant_markdown() {
    let source = "A longer first line\nShort.\n\nAnother paragraph.";
    assert_eq!(
        parse_with_line_breaks(source, true),
        vec![
            Block::Paragraph("A longer first line\nShort.".into()),
            Block::Paragraph("Another paragraph.".into()),
        ]
    );
    assert_eq!(
        parse(source),
        vec![
            Block::Paragraph("A longer first line Short.".into()),
            Block::Paragraph("Another paragraph.".into()),
        ]
    );
}

#[test]
fn parsed_markdown_keeps_hot_sources_and_distinguishes_line_break_modes() {
    let mut cache = ParsedSources::default();
    let cold = cache.get("old reply", false);
    let source = "First line\nSecond line";
    let assistant = cache.get(source, false);
    let prompt = cache.get(source, true);
    assert_eq!(
        assistant.blocks,
        [Block::Paragraph("First line Second line".into())]
    );
    assert_eq!(prompt.blocks, [Block::Paragraph(source.into())]);

    for index in 0..PARSED_SOURCE_LIMIT {
        cache.get(&format!("Reply {index}"), false);
        assert!(Rc::ptr_eq(&cache.get(source, true), &prompt));
    }
    assert!(Rc::ptr_eq(&cache.get(source, false), &assistant));
    assert!(!Rc::ptr_eq(&cache.get("old reply", false), &cold));
    assert_eq!(cache.sources.len(), PARSED_SOURCE_LIMIT);
}

#[test]
fn bulk_inline_matches_scalar_at_every_streaming_boundary() {
    let examples = [
        "ordinary prose without markup. café 日本語 😀 hello",
        "**bold** *italic* __strong__ snake_case ~~gone~~ ***both***",
        r"escaped \*literal\* and `code` ``a ` b`` and trailing ",
        "[**nested** `code`](https://example.com) <https://example.org> http://a.b.",
        r"math $x_2$ and \(n \to \infty\) plus [broken]( and **unfinished",
        "h http https hhttp://example.com [link] ~ _ * ` \\ $ < 🦀",
    ];
    for source in examples {
        for end in source.char_indices().map(|(i, _)| i).chain([source.len()]) {
            let prefix = &source[..end];
            assert_eq!(
                inline_spans_impl::<true>(prefix),
                inline_spans_impl::<false>(prefix),
                "{prefix:?}"
            );
        }
    }
    // Deterministic mixed syntax also exercises malformed nested delimiters.
    let atoms = [
        "a", "é", "😀", "h", "*", "_", "~", "`", "\\", "$", "[", "]", "(", ")", "<", ">", " ", ":",
        "/",
    ];
    let mut seed = 7u64;
    for _ in 0..1000 {
        let mut source = String::new();
        for _ in 0..48 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            source.push_str(atoms[(seed >> 32) as usize % atoms.len()]);
        }
        assert_eq!(
            inline_spans_impl::<true>(&source),
            inline_spans_impl::<false>(&source),
            "{source:?}"
        );
    }
}

#[test]
#[ignore = "manual paired same-binary inline parsing profiler"]
fn inline_prose_profile() {
    use std::{hint::black_box, time::Instant};
    let source = "The desktop renders a streaming response with **important details**, `code`, and [a link](https://example.com). Unicode café 日本語 🦀 remains intact. ".repeat(128);
    for trial in 0..6 {
        let run = |bulk: bool| {
            let start = Instant::now();
            for _ in 0..100 {
                if bulk {
                    black_box(inline_spans_impl::<true>(black_box(&source)));
                } else {
                    black_box(inline_spans_impl::<false>(black_box(&source)));
                }
            }
            start.elapsed().as_secs_f64() * 1000.0
        };
        let (bulk, scalar) = if trial % 2 == 0 {
            let scalar = run(false);
            (run(true), scalar)
        } else {
            let bulk = run(true);
            (bulk, run(false))
        };
        eprintln!(
            "inline trial={trial} bytes={} iterations=100 scalar_ms={scalar:.3} bulk_ms={bulk:.3} speedup={:.2}",
            source.len(),
            scalar / bulk
        );
    }
}

#[test]
fn restored_reasoning_decodes_before_markdown_emphasis() {
    let source = format!(
        "{}{}\nNormal **answer**.",
        jcode_render_core::reasoning_line_markup("**Checking top live tabs**"),
        jcode_render_core::reasoning_line_markup("Use `code` and [docs](https://example.com)."),
    );
    let blocks = parse(&source);
    assert_eq!(blocks.len(), 2);
    let Block::Reasoning(heading) = &blocks[0] else {
        panic!("missing reasoning")
    };
    let inner = parse(heading);
    let Block::Paragraph(text) = &inner[0] else {
        panic!("missing paragraph")
    };
    let inline = inline_spans(text.split_once(" Use ").unwrap().0);
    assert_eq!(inline.plain, "Checking top live tabs");
    assert_eq!(inline.highlights[0].1.font_weight, Some(FontWeight::BOLD));
    let detail = inline_spans(heading.lines().nth(1).unwrap());
    assert_eq!(detail.plain, "Use code and docs.");
    assert_eq!(&detail.plain[detail.code_ranges[0].clone()], "code");
    assert_eq!(detail.links[0].1, "https://example.com");
    assert_eq!(blocks[1], Block::Paragraph("Normal **answer**.".into()));
}

#[test]
fn restored_reasoning_preserves_intentional_escapes_and_fenced_examples() {
    let source = jcode_render_core::reasoning_line_markup(r"Keep \*literal\* and C:\work.");
    let Block::Reasoning(content) = &parse(&source)[0] else {
        panic!("missing reasoning")
    };
    let inline = inline_spans(content);
    assert_eq!(inline.plain, r"Keep *literal* and C:\work.");
    assert!(inline.highlights.is_empty());
    assert_eq!(
        parse(r"\*literal\*"),
        vec![Block::Paragraph(r"\*literal\*".into())]
    );
    let fenced = format!("```text\n{source}```");
    assert_eq!(
        parse(&fenced),
        vec![Block::Code {
            lang: "text".into(),
            body: source.trim_end_matches('\n').into(),
        }]
    );
}

#[test]
fn restored_reasoning_preserves_multiline_markdown_blocks() {
    let original = "## Plan\n\n- Read `code`\n- Test\n\n```rust\nlet x = 1;\n```";
    let encoded: String = original
        .lines()
        .map(jcode_render_core::reasoning_line_markup)
        .collect();
    let blocks = parse(&format!("{encoded}\nDone."));
    assert_eq!(blocks.len(), 2);
    let Block::Reasoning(decoded) = &blocks[0] else {
        panic!("missing reasoning")
    };
    assert_eq!(decoded, original);
    assert_eq!(parse(decoded), parse(original));
    assert!(matches!(&parse(decoded)[3], Block::Code { lang, .. } if lang == "rust"));
    assert_eq!(blocks[1], Block::Paragraph("Done.".into()));
}

struct AvatarTextView {
    selection: gpui::Entity<TextSelection>,
    source: String,
    width: f32,
    layout: gpui::TextLayout,
    continuation: gpui::TextLayout,
}

impl gpui::Render for AvatarTextView {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let (first, layout) = styled_line_layout(
            &self.source,
            &self.selection,
            "first".into(),
            window,
            cx,
            true,
        );
        let (next, continuation) = styled_line_layout(
            "Subsequent streaming segment",
            &self.selection,
            "next".into(),
            window,
            cx,
            false,
        );
        self.layout = layout;
        self.continuation = continuation;
        div()
            .w(px(self.width))
            .text_size(px(14.0))
            .line_height(px(22.0))
            .flex()
            .flex_col()
            .child(first)
            .child(next)
    }
}

#[gpui::test]
fn avatar_indents_only_first_visual_line_and_preserves_copy_and_links(
    cx: &mut gpui::TestAppContext,
) {
    let source = "[Linked](https://example.com) **bold βeta** and `code` then enough words to wrap over several visual lines without indenting the remainder of the paragraph.";
    let plain = inline_spans(source).plain;
    let (view, vcx) = cx.add_window_view(|_, cx| AvatarTextView {
        selection: cx.new(TextSelection::new),
        source: source.into(),
        width: 180.0,
        layout: gpui::TextLayout::default(),
        continuation: gpui::TextLayout::default(),
    });
    vcx.run_until_parked();
    let (layout, continuation, selection) = view.read_with(vcx, |view, _| {
        (
            view.layout.clone(),
            view.continuation.clone(),
            view.selection.clone(),
        )
    });
    let first = layout.position_for_index(AVATAR_PREFIX.len()).unwrap();
    let avatar = vcx.debug_bounds("assistant-avatar").unwrap();
    assert!(
        first.x >= avatar.right(),
        "first text must clear the icon: {first:?} {avatar:?}"
    );
    assert_eq!(
        first.y,
        layout.bounds().top(),
        "spacer must not wrap onto its own line"
    );
    assert!(
        first.x - layout.bounds().left() <= px(50.0),
        "first-line spacer must stay compact"
    );
    // GPUI gives a byte at a wrap boundary upstream affinity (the end
    // of the preceding line). Hit-test each actual following line's left
    // edge instead, and require its first glyph, not a padded gutter.
    let lines = layout.line_layouts();
    let wrapped = &lines[0];
    assert!(wrapped.wrap_boundaries.len() >= 2);
    for (line_index, boundary) in wrapped.wrap_boundaries.iter().enumerate() {
        let expected =
            wrapped.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index;
        let position = gpui::point(
            layout.bounds().left() + px(0.1),
            first.y + layout.line_height() * (line_index + 1) as f32 + px(5.0),
        );
        assert_eq!(
            layout.index_for_position(position),
            Ok(expected),
            "wrapped line must begin at normal margin"
        );
        assert!(
            position.y - px(5.0) >= avatar.bottom(),
            "icon must not overlap second line"
        );
    }
    assert_eq!(
        continuation.position_for_index(0).unwrap().x,
        layout.bounds().left()
    );
    let click = first + gpui::point(px(2.0), px(5.0));
    vcx.simulate_click(click, gpui::Modifiers::default());
    assert_eq!(vcx.opened_url().as_deref(), Some("https://example.com"));
    for (click_count, expected) in [(2, "Linked"), (4, plain.as_str())] {
        vcx.simulate_event(gpui::MouseDownEvent {
            button: gpui::MouseButton::Left,
            position: click,
            modifiers: gpui::Modifiers::default(),
            click_count,
            first_mouse: false,
        });
        vcx.update(|_, cx| selection.update(cx, |selection, cx| selection.copy(cx)));
        assert_eq!(
            vcx.update(|_, cx| cx.read_from_clipboard())
                .and_then(|item| item.text())
                .as_deref(),
            Some(expected)
        );
    }
    // Shift-select across a soft wrap and mixed UTF-8/style runs. The
    // selected bytes must be source bytes, not the display-only prefix.
    vcx.simulate_click(click, gpui::Modifiers::default());
    let end = plain.find("code").unwrap() + "code".len();
    let end_position = layout
        .position_for_index(AVATAR_PREFIX.len() + end)
        .unwrap();
    vcx.simulate_click(
        end_position + gpui::point(px(0.1), px(5.0)),
        gpui::Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    vcx.update(|_, cx| selection.update(cx, |selection, cx| selection.copy(cx)));
    assert_eq!(
        vcx.update(|_, cx| cx.read_from_clipboard())
            .and_then(|item| item.text())
            .as_deref(),
        Some(&plain[..end])
    );
    // Repaint selection highlights, including UTF-8, link, bold and code
    // ranges shifted by the layout prefix, then resize and append live text.
    view.update(vcx, |view, cx| {
        view.width = 260.0;
        view.source.push_str(" Streamed **tail**.");
        cx.notify();
    });
    vcx.run_until_parked();
    let layout = view.read_with(vcx, |view, _| view.layout.clone());
    assert_eq!(
        layout.position_for_index(AVATAR_PREFIX.len()).unwrap().y,
        layout.bounds().top()
    );
}

#[gpui::test]
fn markdown_leaf_without_extra_wrapper_preserves_geometry(cx: &mut gpui::TestAppContext) {
    struct Fixture {
        selection: gpui::Entity<TextSelection>,
        width: f32,
        layouts: Vec<gpui::TextLayout>,
    }
    impl gpui::Render for Fixture {
        fn render(
            &mut self,
            window: &mut gpui::Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            let source = "Unicode café🙂 **bold** with `inline code` and [a link](https://example.com), wrapping across several lines.";
            let (direct, a) =
                styled_line_layout(source, &self.selection, "direct".into(), window, cx, false);
            let (wrapped, b) =
                styled_line_layout(source, &self.selection, "wrapped".into(), window, cx, false);
            self.layouts = vec![a, b];
            div()
                .w(px(self.width))
                .flex()
                .flex_col()
                .text_size(px(14.))
                .line_height(px(22.))
                .child(direct)
                .child(div().relative().child(wrapped))
        }
    }
    let (view, vcx) = cx.add_window_view(|_, cx| Fixture {
        selection: cx.new(TextSelection::new),
        width: 150.,
        layouts: Vec::new(),
    });
    for width in [150., 280., 520.] {
        view.update(vcx, |view, cx| {
            view.width = width;
            cx.notify();
        });
        vcx.run_until_parked();
        view.read_with(vcx, |view, _| {
            let a = &view.layouts[0];
            let b = &view.layouts[1];
            assert_eq!(a.bounds().size, b.bounds().size);
            for index in 0..a.len() {
                if let (Some(a_pos), Some(b_pos)) =
                    (a.position_for_index(index), b.position_for_index(index))
                {
                    assert_eq!(a_pos - a.bounds().origin, b_pos - b.bounds().origin);
                }
            }
        });
        assert_eq!(
            vcx.debug_bounds("selectable-text-direct").unwrap().size,
            vcx.debug_bounds("selectable-text-wrapped").unwrap().size
        );
    }
}

#[gpui::test]
fn avatar_markdown_blocks_keep_their_normal_left_margin(cx: &mut gpui::TestAppContext) {
    struct Document {
        selection: gpui::Entity<TextSelection>,
    }
    impl gpui::Render for Document {
        fn render(
            &mut self,
            window: &mut gpui::Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            div().w(px(190.0)).child(render_document(
                    "# A heading that wraps across multiple visual lines\n\nThe next paragraph stays at its normal margin.\n\n- A list item", 0, "avatar-document", &self.selection, window, cx, false, None, true,
                ))
        }
    }
    let (_, vcx) = cx.add_window_view(|_, cx| Document {
        selection: cx.new(TextSelection::new),
    });
    vcx.run_until_parked();
    let first = vcx
        .debug_bounds("selectable-text-avatar-document-0")
        .unwrap();
    let next = vcx
        .debug_bounds("selectable-text-avatar-document-1")
        .unwrap();
    let avatar = vcx.debug_bounds("assistant-avatar").unwrap();
    assert_eq!(first.left(), next.left());
    assert_eq!(avatar.left(), first.left());
    assert!(next.top() >= first.bottom());
}

struct RestoredReasoningView {
    selection: gpui::Entity<TextSelection>,
    source: String,
}

#[gpui::test]
fn fenced_code_selection_copies_source_without_gutters(cx: &mut gpui::TestAppContext) {
    cx.update(crate::text_selection::bind_keys);
    struct CodeDocument {
        selection: gpui::Entity<TextSelection>,
        source: String,
    }
    impl gpui::Render for CodeDocument {
        fn render(
            &mut self,
            window: &mut gpui::Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            let selection = self.selection.clone();
            div()
                .w(px(500.))
                .key_context(TextSelection::key_context())
                .track_focus(&selection.read(cx).focus_handle())
                .on_action(move |_: &text_selection::Copy, _, cx| {
                    selection.update(cx, |selection, cx| selection.copy(cx));
                })
                .child(render(&self.source, 0, &self.selection, window, cx))
        }
    }
    let (view, vcx) = cx.add_window_view(|_, cx| {
        let selection = cx.new(TextSelection::new);
        cx.observe(&selection, |_, _, cx| cx.notify()).detach();
        CodeDocument {
            selection,
            source: "```rust\nlet βeta = 1;\nprintln!(\"hello\");\n```".into(),
        }
    });
    for (source, expected, header) in [
        (
            "```rust\nlet βeta = 1;\nprintln!(\"hello\");\n```",
            "let βeta = 1;\nprintln!(\"hello\");",
            true,
        ),
        ("```\nβeta\n```", "βeta", false),
    ] {
        view.update(vcx, |view, cx| {
            view.source = source.into();
            cx.notify();
        });
        vcx.run_until_parked();
        assert_eq!(vcx.debug_bounds("code-copy").is_some(), header);
        let bounds = vcx.debug_bounds("selectable-text-0-0").unwrap();
        let start = gpui::point(bounds.left() + px(0.1), bounds.top() + px(5.));
        let end = gpui::point(bounds.right() + px(10.), bounds.bottom() - px(2.));
        vcx.simulate_event(gpui::MouseDownEvent {
            button: gpui::MouseButton::Left,
            position: start,
            modifiers: Default::default(),
            click_count: 1,
            first_mouse: false,
        });
        vcx.simulate_event(gpui::MouseMoveEvent {
            position: end,
            pressed_button: Some(gpui::MouseButton::Left),
            modifiers: Default::default(),
        });
        vcx.simulate_event(gpui::MouseUpEvent {
            button: gpui::MouseButton::Left,
            position: end,
            modifiers: Default::default(),
            click_count: 1,
        });
        vcx.run_until_parked();
        view.read_with(vcx, |view, cx| {
            assert!(
                view.selection
                    .read(cx)
                    .highlight("0-0", expected.len())
                    .is_some()
            );
        });
        vcx.simulate_keystrokes("ctrl-c");
        assert_eq!(
            vcx.update(|_, cx| cx.read_from_clipboard())
                .and_then(|item| item.text())
                .as_deref(),
            Some(expected),
        );
        if let Some(copy) = vcx.debug_bounds("code-copy") {
            vcx.update(|_, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string("old".into()))
            });
            vcx.simulate_event(gpui::MouseDownEvent {
                button: gpui::MouseButton::Left,
                position: copy.center(),
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            });
            assert_eq!(
                vcx.update(|_, cx| cx.read_from_clipboard())
                    .and_then(|item| item.text())
                    .as_deref(),
                Some(expected),
            );
        }
    }
}

impl gpui::Render for RestoredReasoningView {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        div().size_full().child(render_with_style(
            &self.source,
            0,
            &self.selection,
            window,
            cx,
            false,
            None,
        ))
    }
}

#[gpui::test]
fn restored_reasoning_paints_and_copies_without_transport_markup(cx: &mut gpui::TestAppContext) {
    let styles = capture_painted_font_styles();
    let (view, vcx) = cx.add_window_view(|_, cx| RestoredReasoningView {
        selection: cx.new(TextSelection::new),
        source: format!(
            "{}\n{}\n{}\nThe **answer**.",
            jcode_render_core::reasoning_line_markup("**Checking top live tabs**"),
            jcode_render_core::reasoning_line_markup(""),
            jcode_render_core::reasoning_line_markup("Quiet reasoning body.")
        ),
    });
    vcx.run_until_parked();
    assert_eq!(
        styles.borrow()["Quiet reasoning body."],
        gpui::FontStyle::Italic
    );
    assert_eq!(styles.borrow()["The answer."], gpui::FontStyle::Normal);
    assert!(vcx.debug_bounds("restored-reasoning").is_some());
    let heading = vcx.debug_bounds("selectable-text-0-0-0").unwrap();
    let body = vcx.debug_bounds("selectable-text-0-0-1").unwrap();
    assert_eq!(
        heading.size.height, body.size.height,
        "reasoning titles must use body-sized text"
    );
    let answer = vcx.debug_bounds("selectable-text-0-1").unwrap();
    assert!(answer.top() >= heading.bottom());
    for (bounds, expected) in [(heading, "Checking top live tabs"), (answer, "The answer.")] {
        vcx.simulate_event(gpui::MouseDownEvent {
            button: gpui::MouseButton::Left,
            position: bounds.center(),
            modifiers: gpui::Modifiers::default(),
            click_count: 4,
            first_mouse: false,
        });
        let selection = view.read_with(vcx, |view, _| view.selection.clone());
        vcx.update(|_, cx| selection.update(cx, |selection, cx| selection.copy(cx)));
        let copied = vcx
            .update(|_, cx| cx.read_from_clipboard())
            .and_then(|item| item.text());
        assert_eq!(copied.as_deref(), Some(expected));
    }
}

/// Nested inline spans produce overlapping highlight ranges. GPUI aborts
/// the whole process in debug builds when highlight ranges overlap or run
/// past the text, so the exact invariant `compute_runs` needs is checked
/// here: sorted, disjoint, in-bounds segments that cover every styled
/// byte on char boundaries.
#[test]
fn flattened_highlights_are_sorted_disjoint_and_in_bounds() {
    let sources = [
        "**bold with `code` inside**",
        "*italic [link](https://example.com) and `code`*",
        "~~strike **bold `code`** tail~~ plus **more *nesting* here**",
        "prefix **`n the`** suffix",
        "a **b *c `d` e* f** g [h **i**](https://example.com/x) j",
    ];
    for source in sources {
        let inline = inline_spans(source);
        let flattened = flatten_highlights(&inline.highlights);
        let mut previous_end = 0;
        for (range, _) in &flattened {
            assert!(range.start < range.end, "empty segment in {source:?}");
            assert!(
                range.start >= previous_end,
                "overlapping segments in {source:?}"
            );
            assert!(
                range.end <= inline.plain.len(),
                "segment out of bounds in {source:?}"
            );
            assert!(
                inline.plain.is_char_boundary(range.start)
                    && inline.plain.is_char_boundary(range.end),
                "segment splits a character in {source:?}"
            );
            previous_end = range.end;
        }
        // Every originally styled byte stays styled after flattening.
        for (range, _) in &inline.highlights {
            for offset in range.clone() {
                assert!(
                    flattened.iter().any(|(r, _)| r.contains(&offset)),
                    "byte {offset} lost its style in {source:?}"
                );
            }
        }
    }
}

#[test]
fn parses_blocks() {
    let blocks = parse("# Title\n\nBody text\n\n- item\n\n```rust\nfn main() {}\n```");
    assert_eq!(blocks[0], Block::Heading(1, "Title".into()));
    assert_eq!(blocks[1], Block::Paragraph("Body text".into()));
    assert_eq!(
        blocks[2],
        Block::Bullet {
            depth: 0,
            text: "item".into(),
            task: None
        }
    );
    assert_eq!(
        blocks[3],
        Block::Code {
            lang: "rust".into(),
            body: "fn main() {}".into()
        }
    );
}

#[test]
fn parses_mermaid_fences_as_diagrams() {
    let blocks = parse("```mermaid\nflowchart LR\nA[Start] --> B[Done]\n```");
    assert_eq!(
        blocks,
        vec![Block::Mermaid("flowchart LR\nA[Start] --> B[Done]".into())]
    );
    assert_eq!(
        mermaid_display_line("A[Start] --> B[Done]").as_deref(),
        Some("AStart → BDone")
    );
}

#[test]
fn html_previews_require_an_explicit_complete_fence() {
    assert_eq!(
        parse("```html-preview\n<h1>Hi</h1>\n```"),
        vec![Block::HtmlPreview("<h1>Hi</h1>".into())]
    );
    assert!(matches!(
        &parse("```html\n<h1>Hi</h1>\n```")[0],
        Block::Code { .. }
    ));
    assert!(matches!(
        &parse("```html-preview\n<h1>Streaming")[0],
        Block::Code { .. }
    ));
    assert!(matches!(
        &parse("```html-preview\n<h1>Hi</h1>\n~~~")[0],
        Block::Code { .. }
    ));
    assert!(matches!(
        &parse("```html-preview\n<h1>Hi</h1>\n```not-a-close")[0],
        Block::Code { .. }
    ));
    assert_eq!(
        parse("~~~html-preview\nhello\n~~~"),
        vec![Block::HtmlPreview("hello".into())]
    );
    assert!(matches!(
        &parse("<div>ordinary HTML</div>")[0],
        Block::Paragraph(_)
    ));
}

#[test]
fn renders_mermaid_as_cached_native_scene() {
    let source = "flowchart TD\nA[Start] --> B[Done]";
    let light = mermaid_rs_renderer::Theme::modern();
    let first = render_mermaid_scene(source, light.clone()).expect("valid diagram");
    let second = render_mermaid_scene(source, light).expect("cached diagram");
    assert!(Arc::ptr_eq(&first.native, &second.native));
    assert!(first.native.width() > 0.0 && first.native.height() > 0.0);
    let dark =
        render_mermaid_scene(source, mermaid_rs_renderer::Theme::dark()).expect("dark diagram");
    assert!(
        !Arc::ptr_eq(&first.native, &dark.native),
        "palette is part of cache identity"
    );
}

#[test]
fn mermaid_uses_semantic_desktop_palette() {
    let desktop = Theme::global();
    let diagram = mermaid_theme(desktop);
    assert_eq!(diagram.font_family, desktop.FONT_UI);
    assert_eq!(diagram.font_size, 14.0);
    assert_eq!(diagram.text_color, diagram.primary_text_color);
    assert_eq!(diagram.background, diagram.edge_label_background);
    assert_ne!(diagram.text_color, diagram.background);
}

#[test]
fn parses_setext_headings() {
    assert_eq!(
        parse("Primary\n=======\n\nSecondary\n---"),
        vec![
            Block::Heading(1, "Primary".into()),
            Block::Heading(2, "Secondary".into())
        ]
    );
}

#[test]
fn inline_code_font_ranges_survive_nested_styles_links_and_unicode() {
    let inline =
        inline_spans("é `one` **`two`** *`three`* ~~`four`~~ [`five`](https://example.com)");
    let code: Vec<_> = inline
        .code_ranges
        .iter()
        .map(|range| &inline.plain[range.clone()])
        .collect();
    assert_eq!(code, ["one", "two", "three", "four", "five"]);
    // GPUI overrides whole runs. Every code boundary must remain a run
    // boundary even when selection or outer emphasis overlaps it.
    let mut highlights = inline.highlights.clone();
    highlights.push((
        0..inline.plain.len(),
        HighlightStyle {
            background_color: Some(to_hsla(Theme::global().SELECTION)),
            ..Default::default()
        },
    ));
    let flattened = flatten_highlights(&highlights);
    for range in &inline.code_ranges {
        assert!(flattened.iter().any(|(run, _)| run.start == range.start));
        assert!(flattened.iter().any(|(run, _)| run.end == range.end));
    }
    assert!(
        inline_spans("plain prose and \\`escaped\\`")
            .code_ranges
            .is_empty()
    );
}

#[test]
fn highlight_fast_path_matches_overlap_algorithm() {
    let styles = [
        HighlightStyle::default(),
        HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
    ];
    // Empty, reversed, nested, disjoint and adjacent ranges in either order.
    let ranges: Vec<_> = (0..=4)
        .flat_map(|start| (0..=4).map(move |end| start..end))
        .collect();
    for first in &ranges {
        for second in &ranges {
            for style in styles {
                let input = [(first.clone(), styles[0]), (second.clone(), style)];
                assert_eq!(
                    flatten_highlights(&input),
                    flatten_overlapping_highlights(&input),
                    "{input:?}"
                );
            }
        }
    }
    let input = [(0..2, styles[0]), (2..2, styles[1]), (2..4, styles[0])];
    assert_eq!(flatten_highlights(&input), vec![(0..4, styles[0])]);
    assert!(flatten_highlights(&[]).is_empty());
}

#[test]
#[ignore = "manual highlight throughput benchmark"]
fn highlight_flatten_profile() {
    use std::hint::black_box;
    let style = HighlightStyle::default();
    for count in [8, 64, 256] {
        let input: Vec<_> = (0..count).map(|i| (i * 4..i * 4 + 2, style)).collect();
        let iterations = 10_000;
        let started = std::time::Instant::now();
        for _ in 0..iterations {
            black_box(flatten_overlapping_highlights(black_box(&input)));
        }
        let original = started.elapsed();
        let started = std::time::Instant::now();
        for _ in 0..iterations {
            black_box(flatten_highlights(black_box(&input)));
        }
        println!(
            "HIGHLIGHTS spans={count} iterations={iterations} original={original:?} fast={:?}",
            started.elapsed()
        );
    }
}

#[test]
fn inline_code_and_bold() {
    let inline = inline_spans("use `cargo` and **run** it");
    assert_eq!(inline.plain, "use cargo and run it");
    assert_eq!(inline.highlights.len(), 2);
    assert_eq!(inline.highlights[0].0, 4..9);
    assert_eq!(inline.highlights[1].0, 14..17);
}

#[test]
fn parses_display_math_and_does_not_misread_hashes() {
    let blocks = parse("\\[\ne^{i\\pi}+1=0\n\\]\n\n#hashtag");
    assert_eq!(blocks[0], Block::Math(r"e^{i\pi}+1=0".into()));
    assert_eq!(blocks[1], Block::Paragraph("#hashtag".into()));
}

#[test]
fn renders_common_inline_math_as_unicode() {
    let inline = inline_spans("Euler: $e^{i\\pi}+1=0$ and $x_2$");
    assert_eq!(inline.plain, "Euler: e^(iπ)+1=0 and x₂");
    assert_eq!(inline.highlights.len(), 2);
}

#[test]
fn renders_paren_delimited_inline_math() {
    let inline = inline_spans(r"limit \(n \to \infty\) holds");
    assert_eq!(inline.plain, "limit n → ∞ holds");
    assert_eq!(inline.highlights.len(), 1);
}

#[test]
fn parses_single_line_bracket_math() {
    let blocks = parse(r"\[ e^{i\pi}+1=0 \]");
    assert_eq!(blocks[0], Block::Math(r"e^{i\pi}+1=0".into()));
}

#[test]
fn preserves_unknown_latex_readably() {
    assert_eq!(latex_to_text(r"\\unknown{x}^2"), r"\\unknownx²");
    assert_eq!(latex_to_text(r"a \rightarrow \infty"), "a → ∞");
}

#[test]
fn preserves_fraction_and_root_structure() {
    assert_eq!(
        latex_to_text(r"x=\frac{-b\pm\sqrt{b^2-4ac}}{2a}"),
        "x=(-b±√(b²-4ac))/(2a)"
    );
    assert_eq!(latex_to_text(r"\frac{1}{\frac{x}{2}}"), "(1)/((x)/(2))");
}

#[test]
fn transcript_equation_examples_remain_readable() {
    let source = concat!(
        r"\[E=mc^2\]",
        "\n\n",
        r"\[a^2+b^2=c^2\]",
        "\n\n",
        r"\[x=\frac{-b\pm\sqrt{b^2-4ac}}{2a}\]",
        "\n\n",
        r"\[\int_a^b f(x)\,dx=F(b)-F(a)\]",
        "\n\n",
        r"\[e^{i\pi}+1=0\]",
    );
    assert_eq!(
        parse(source),
        vec![
            Block::Math("E=mc^2".into()),
            Block::Math("a^2+b^2=c^2".into()),
            Block::Math(r"x=\frac{-b\pm\sqrt{b^2-4ac}}{2a}".into()),
            Block::Math(r"\int_a^b f(x)\,dx=F(b)-F(a)".into()),
            Block::Math(r"e^{i\pi}+1=0".into()),
        ]
    );
}

#[test]
fn typesets_display_math_as_embedded_svg() {
    let rendered =
        render_math_svg(r"x=\frac{-b\pm\sqrt{b^2-4ac}}{2a}").expect("common LaTeX should render");
    let svg = std::str::from_utf8(rendered.svg.as_ref()).expect("SVG is UTF-8");
    assert!(svg.starts_with("<svg "));
    assert!(svg.contains("<path"), "glyphs should be embedded as paths");
    assert!(!svg.contains("<text"), "SVG must not depend on KaTeX fonts");
    assert!(
        svg.contains("currentColor"),
        "equations should follow the theme"
    );
    assert!(rendered.width > rendered.height);
}

#[test]
fn strips_layout_commands_from_common_model_equations() {
    let quadratic =
        inline_spans(r"Quadratic: \(\displaystyle x = \frac{-b \pm \sqrt{b^2-4ac}}{2a}\)");
    assert_eq!(quadratic.plain, "Quadratic: x = (-b ± √(b²-4ac))/(2a)");

    let derivative = inline_spans(r"Derivative: \(\displaystyle \frac{d}{dx}x^n=nx^{n-1}\)");
    assert_eq!(derivative.plain, "Derivative: (d)/(dx)xⁿ=nxⁿ⁻¹");
}

#[test]
fn unwraps_text_and_style_commands() {
    assert_eq!(latex_to_text(r"\boxed{\mathbf{x^2}}"), "x²");
    assert_eq!(latex_to_text(r"\text{area} = \pi r^2"), "area = π r²");
}

#[test]
fn parses_nested_and_task_lists() {
    let blocks = parse("- top\n  - nested\n- [x] done\n- [ ] todo");
    assert_eq!(
        blocks[1],
        Block::Bullet {
            depth: 1,
            text: "nested".into(),
            task: None
        }
    );
    assert_eq!(
        blocks[2],
        Block::Bullet {
            depth: 0,
            text: "done".into(),
            task: Some(true)
        }
    );
    assert_eq!(
        blocks[3],
        Block::Bullet {
            depth: 0,
            text: "todo".into(),
            task: Some(false)
        }
    );
}

#[test]
fn parses_tables() {
    let blocks = parse("| a | b |\n| --- | --- |\n| 1 | 2 |");
    assert_eq!(
        blocks[0],
        Block::Table {
            header: vec!["a".into(), "b".into()],
            rows: vec![vec!["1".into(), "2".into()]],
        }
    );
}

#[test]
fn extracts_links() {
    let inline = inline_spans("see [docs](https://example.com) now");
    assert_eq!(inline.plain, "see docs now");
    assert_eq!(inline.links.len(), 1);
    assert_eq!(inline.links[0].1, "https://example.com");
    assert_eq!(inline.links[0].0, 4..8);
}

#[test]
fn image_marker_before_parenthesis_stays_a_marker() {
    for marker in ["[#image 1]", "[#img_64ab0e4a8c467a9f70900b407050b140]"] {
        let source = format!("see {marker}(сессия и TLDR видео) now");
        let inline = inline_spans(&source);
        assert_eq!(inline.plain, source);
        assert!(inline.links.is_empty(), "{marker} must not start a link");
    }
}

#[test]
fn only_urls_with_a_scheme_are_opened() {
    assert_eq!(
        openable_url("https://example.com"),
        Some("https://example.com")
    );
    assert_eq!(openable_url("mailto:a@b.c"), Some("mailto:a@b.c"));
    assert_eq!(openable_url("сессия транскрипция и TLDR видео"), None);
    assert_eq!(openable_url("crates/foo.rs"), None);
    assert_eq!(openable_url(""), None);
}

#[test]
fn autolinks_bare_urls() {
    let inline = inline_spans("go to https://example.com.");
    assert_eq!(inline.plain, "go to https://example.com.");
    assert_eq!(inline.links[0].1, "https://example.com");
}

#[test]
fn strikethrough_and_multiline_quote() {
    let inline = inline_spans("~~gone~~ here");
    assert_eq!(inline.plain, "gone here");
    let blocks = parse("> one\n> two\n\ntail");
    assert_eq!(blocks[0], Block::Quote(vec!["one".into(), "two".into()]));
}

#[test]
fn highlights_code_keywords() {
    let (plain, highlights) = highlight_code("fn main() { let x = 1; }", "rust");
    assert_eq!(plain, "fn main() { let x = 1; }");
    assert!(!highlights.is_empty());
}

#[test]
fn escapes_are_literal() {
    let inline = inline_spans(r"a \* b");
    assert_eq!(inline.plain, "a * b");
    assert!(inline.highlights.is_empty());
}
