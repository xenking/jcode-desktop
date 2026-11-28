//! Parsing markdown source into blocks and inline spans.
use super::*;

#[derive(Debug, PartialEq)]
pub(super) enum Block {
    Heading(u8, String),
    Paragraph(String),
    Reasoning(String),
    Bullet {
        depth: usize,
        text: String,
        task: Option<bool>,
    },
    Numbered {
        depth: usize,
        number: String,
        text: String,
    },
    Quote(Vec<String>),
    Code {
        lang: String,
        body: String,
    },
    Mermaid(String),
    HtmlPreview(String),
    Table {
        header: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Math(String),
    Rule,
}

pub(super) fn indent_depth(line: &str) -> usize {
    let spaces = line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum::<usize>();
    (spaces / 2).min(4)
}

pub(super) fn parse(source: &str) -> Vec<Block> {
    parse_with_line_breaks(source, false)
}

#[cfg(test)]
thread_local! {
    static PARSE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn parse_count() -> usize {
    PARSE_COUNT.get()
}

pub(super) fn parse_with_line_breaks(source: &str, preserve_line_breaks: bool) -> Vec<Block> {
    parse_document(source, preserve_line_breaks, |_, _| {})
}

pub(super) const PARSED_SOURCE_LIMIT: usize = 256;
pub(super) const PARSED_SOURCE_BYTES: usize = 4 * 1024 * 1024;

pub(super) struct ParsedSource {
    // Indexed by whether this render preserves soft breaks.
    pub(super) documents: [Option<Rc<ParsedMarkdown>>; 2],
    pub(super) last_used: u64,
}

#[derive(Default)]
pub(super) struct ParsedSources {
    pub(super) sources: HashMap<Rc<str>, ParsedSource>,
    pub(super) source_bytes: usize,
    pub(super) clock: u64,
}

impl ParsedSources {
    pub(super) fn get(&mut self, source: &str, preserve_line_breaks: bool) -> Rc<ParsedMarkdown> {
        let parse = || Rc::new(ParsedMarkdown::parse(source, preserve_line_breaks));
        if source.len() > PARSED_SOURCE_BYTES {
            return parse();
        }
        self.clock += 1;
        let mode = usize::from(preserve_line_breaks);
        if let Some(entry) = self.sources.get_mut(source) {
            entry.last_used = self.clock;
            return entry.documents[mode].get_or_insert_with(parse).clone();
        }
        while self.sources.len() == PARSED_SOURCE_LIMIT
            || self.source_bytes + source.len() > PARSED_SOURCE_BYTES
        {
            let oldest = self
                .sources
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .expect("cache is over budget")
                .0
                .clone();
            self.sources.remove(oldest.as_ref());
            self.source_bytes -= oldest.len();
        }
        let document = parse();
        let mut documents = [None, None];
        documents[mode] = Some(document.clone());
        self.source_bytes += source.len();
        self.sources.insert(
            source.into(),
            ParsedSource {
                documents,
                last_used: self.clock,
            },
        );
        document
    }
}

thread_local! {
    // Bound count and source bytes: each reveal prefix is a different source.
    static PARSED_SOURCES: RefCell<ParsedSources> = RefCell::default();
}

pub(super) fn cached_parse(source: &str, preserve_line_breaks: bool) -> Rc<ParsedMarkdown> {
    PARSED_SOURCES.with_borrow_mut(|cache| cache.get(source, preserve_line_breaks))
}

pub(super) struct ParsedMarkdown {
    pub(super) blocks: Vec<Block>,
    pub(super) public: Option<String>,
}

impl ParsedMarkdown {
    pub(super) fn parse(source: &str, preserve_line_breaks: bool) -> Self {
        let offset = |line: &str| line.as_ptr() as usize - source.as_ptr() as usize;
        let mut public = String::new();
        let mut cursor = 0;
        let blocks = parse_document(source, preserve_line_breaks, |first, next| {
            let start = offset(first);
            public.push_str(&source[cursor..start]);
            if !public.is_empty() && next.is_some() && !public.ends_with("\n\n") {
                public.push('\n');
            }
            cursor = next.map_or(source.len(), offset);
        });
        let public = (cursor != 0).then(|| {
            public.push_str(&source[cursor..]);
            public
        });
        Self { blocks, public }
    }
}

/// History encodes private reasoning as a Markdown block inside assistant text.
/// Use the renderer's own block boundaries so literal markers in code stay public.
pub(crate) fn public_transcript(source: &str) -> Cow<'_, str> {
    if !source.contains(jcode_render_core::REASONING_SENTINEL) {
        return Cow::Borrowed(source);
    }
    cached_parse(source, false)
        .public
        .clone()
        .map_or(Cow::Borrowed(source), Cow::Owned)
}

pub(super) fn parse_document(
    source: &str,
    preserve_line_breaks: bool,
    mut reasoning_block: impl FnMut(&str, Option<&str>),
) -> Vec<Block> {
    #[cfg(test)]
    PARSE_COUNT.set(PARSE_COUNT.get() + 1);
    let mut blocks = Vec::new();
    let mut lines = source.lines().peekable();
    let mut paragraph = String::new();
    // CommonMark hard line break: the previous paragraph line ended with two
    // or more spaces or a backslash.
    let mut hard_break = false;

    let flush = |paragraph: &mut String, blocks: &mut Vec<Block>| {
        // Each line was trim_start'ed before insertion, so only the end
        // needs trimming. Transfer its buffer instead of allocating and
        // copying every paragraph again during every visible-row render.
        paragraph.truncate(paragraph.trim_end().len());
        if !paragraph.is_empty() {
            blocks.push(Block::Paragraph(std::mem::take(paragraph)));
        }
    };

    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        let depth = indent_depth(line);

        if let Some(mut content) = jcode_render_core::reasoning_line_content(trimmed) {
            // Rendered history embeds the terminal's escaped reasoning lines.
            // Decode only that explicit format, before inline emphasis sees
            // its escaped asterisks. Fenced code is consumed separately below.
            flush(&mut paragraph, &mut blocks);
            loop {
                let mut ahead = lines.clone();
                let mut blank_lines = 0;
                while ahead.peek().is_some_and(|line| line.trim().is_empty()) {
                    ahead.next();
                    blank_lines += 1;
                }
                let Some(next) = ahead
                    .next()
                    .and_then(jcode_render_core::reasoning_line_content)
                else {
                    break;
                };
                for _ in 0..=blank_lines {
                    lines.next();
                    content.push('\n');
                }
                content.push_str(&next);
            }
            reasoning_block(line, lines.peek().copied());
            blocks.push(Block::Reasoning(content));
        } else if trimmed == "$$" || trimmed == "\\[" {
            flush(&mut paragraph, &mut blocks);
            let closing = if trimmed == "$$" { "$$" } else { "\\]" };
            let mut body = String::new();
            for math_line in lines.by_ref() {
                if math_line.trim() == closing {
                    break;
                }
                if !body.is_empty() {
                    body.push(' ');
                }
                body.push_str(math_line.trim());
            }
            blocks.push(Block::Math(body));
        } else if (trimmed.starts_with("$$") && trimmed.ends_with("$$")
            || trimmed.starts_with("\\[") && trimmed.ends_with("\\]"))
            && trimmed.len() > 4
        {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Math(
                trimmed[2..trimmed.len() - 2].trim().to_string(),
            ));
        } else if let Some(rest) = trimmed
            .strip_prefix("```")
            .or_else(|| trimmed.strip_prefix("~~~"))
        {
            flush(&mut paragraph, &mut blocks);
            let lang = rest.trim().to_string();
            let mut body = String::new();
            let marker = trimmed.chars().next().unwrap_or('`');
            let mut closed = false;
            for code_line in lines.by_ref() {
                let t = code_line.trim();
                if t.len() >= 3 && t.chars().all(|c| c == marker) {
                    closed = true;
                    break;
                }
                body.push_str(code_line);
                body.push('\n');
            }
            while body.ends_with('\n') {
                body.pop();
            }
            if lang.eq_ignore_ascii_case("html-preview") && closed {
                blocks.push(Block::HtmlPreview(body));
            } else if lang.eq_ignore_ascii_case("mermaid") || lang.eq_ignore_ascii_case("mmd") {
                blocks.push(Block::Mermaid(body));
            } else {
                blocks.push(Block::Code { lang, body });
            }
        } else if lines.peek().is_some_and(|next| is_setext_rule(next.trim())) {
            flush(&mut paragraph, &mut blocks);
            let level = if lines
                .next()
                .is_some_and(|rule| rule.trim().starts_with('='))
            {
                1
            } else {
                2
            };
            blocks.push(Block::Heading(level, trimmed.trim().to_string()));
        } else if is_heading(trimmed) {
            flush(&mut paragraph, &mut blocks);
            let level = trimmed.chars().take_while(|c| *c == '#').count().min(6) as u8;
            let text = trimmed[level as usize..].trim().to_string();
            blocks.push(Block::Heading(level, text));
        } else if is_rule(trimmed) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Rule);
        } else if is_table_row(trimmed)
            && lines.peek().map(|l| is_table_divider(l.trim())) == Some(true)
        {
            flush(&mut paragraph, &mut blocks);
            let header = table_cells(trimmed);
            lines.next();
            let mut rows = Vec::new();
            while let Some(next) = lines.peek() {
                let next = next.trim();
                if !is_table_row(next) {
                    break;
                }
                rows.push(table_cells(next));
                lines.next();
            }
            blocks.push(Block::Table { header, rows });
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
            .or_else(|| trimmed.strip_prefix("• "))
        {
            flush(&mut paragraph, &mut blocks);
            let (task, text) = split_task(item.trim());
            blocks.push(Block::Bullet { depth, text, task });
        } else if let Some((number, item)) = split_numbered(trimmed) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Numbered {
                depth,
                number,
                text: item,
            });
        } else if let Some(rest) = trimmed
            .strip_prefix("> ")
            .or_else(|| if trimmed == ">" { Some("") } else { None })
        {
            flush(&mut paragraph, &mut blocks);
            let mut quoted = vec![rest.to_string()];
            while let Some(next) = lines.peek() {
                let t = next.trim_start();
                if let Some(more) = t.strip_prefix("> ") {
                    quoted.push(more.to_string());
                } else if t == ">" {
                    quoted.push(String::new());
                } else {
                    break;
                }
                lines.next();
            }
            blocks.push(Block::Quote(quoted));
        } else if trimmed.is_empty() {
            flush(&mut paragraph, &mut blocks);
        } else {
            if !paragraph.is_empty() {
                if hard_break && !preserve_line_breaks && paragraph.ends_with('\\') {
                    paragraph.pop();
                }
                let text_end = paragraph.trim_end_matches(' ').len();
                paragraph.truncate(text_end);
                paragraph.push(if preserve_line_breaks || hard_break {
                    '\n'
                } else {
                    ' '
                });
            }
            paragraph.push_str(trimmed);
            let trailing_backslashes = trimmed.len() - trimmed.trim_end_matches('\\').len();
            hard_break = trimmed.ends_with("  ") || trailing_backslashes % 2 == 1;
        }
    }
    flush(&mut paragraph, &mut blocks);
    blocks
}

pub(super) fn is_heading(line: &str) -> bool {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes) && line[hashes..].starts_with(' ')
}

pub(super) fn is_rule(line: &str) -> bool {
    let line = line.trim();
    line.len() >= 3
        && (line.chars().all(|c| c == '-')
            || line.chars().all(|c| c == '*')
            || line.chars().all(|c| c == '_'))
}

pub(super) fn is_setext_rule(line: &str) -> bool {
    let line = line.trim();
    line.len() >= 3 && (line.chars().all(|c| c == '=') || line.chars().all(|c| c == '-'))
}

pub(super) fn is_table_row(line: &str) -> bool {
    let line = line.trim();
    line.starts_with('|') && line.len() > 1 && line.matches('|').count() >= 2
}

pub(super) fn is_table_divider(line: &str) -> bool {
    is_table_row(line)
        && table_cells(line)
            .iter()
            .all(|cell| !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':' || c == ' '))
}

pub(super) fn table_cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let inner = line
        .strip_prefix('|')
        .unwrap_or(line)
        .strip_suffix('|')
        .unwrap_or_else(|| line.strip_prefix('|').unwrap_or(line));
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

pub(super) fn split_task(item: &str) -> (Option<bool>, String) {
    for (prefix, state) in [
        ("[ ] ", false),
        ("[x] ", true),
        ("[X] ", true),
        ("[-] ", true),
    ] {
        if let Some(rest) = item.strip_prefix(prefix) {
            return (Some(state), rest.trim().to_string());
        }
    }
    (None, item.to_string())
}

pub(super) fn split_numbered(line: &str) -> Option<(String, String)> {
    let dot = line.find(". ").or_else(|| line.find(") "))?;
    if dot == 0 || dot > 3 || !line[..dot].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((line[..dot].to_string(), line[dot + 2..].trim().to_string()))
}

#[derive(Debug, PartialEq)]
pub(super) struct Inline {
    pub(super) plain: String,
    pub(super) code_ranges: Vec<std::ops::Range<usize>>,
    pub(super) highlights: Vec<(std::ops::Range<usize>, HighlightStyle)>,
    pub(super) links: Vec<(std::ops::Range<usize>, String)>,
}

pub(super) enum InlineDestination {
    Markdown(String),
    Mention(PathBuf),
}

pub(super) fn inline_file_mentions(inline: &Inline, base: &Path) -> Vec<(Range<usize>, PathBuf)> {
    let overlaps = |left: &Range<usize>, right: &Range<usize>| {
        left.start < right.end && right.start < left.end
    };
    let linked = |range: &Range<usize>| inline.links.iter().any(|(r, _)| overlaps(r, range));
    let mut mentions = Vec::new();
    FILE_REFERENCES.with_borrow_mut(|cache| {
        for range in &inline.code_ranges {
            if !linked(range)
                && let Some(path) = cache.resolve(base, &inline.plain[range.clone()])
            {
                mentions.push((range.clone(), path));
            }
        }
        let mut offset = 0;
        for token in inline.plain.split_inclusive(char::is_whitespace) {
            let right = token
                .trim_end_matches(|c: char| c.is_whitespace() || ".,;:!?)]}>\"'”’»".contains(c));
            let candidate = right.trim_start_matches(|c| "([{<\"'“‘«".contains(c));
            let range = offset + right.len() - candidate.len()..offset + right.len();
            offset += token.len();
            if candidate.contains(['.', '/', '\\'])
                && !linked(&range)
                && !inline.code_ranges.iter().any(|r| overlaps(r, &range))
                && let Some(path) = cache.resolve(base, candidate)
            {
                mentions.push((range, path));
            }
        }
    });
    mentions
}

/// Check unconfirmed mentions in the background. They render as plain text
/// until the answer lands, then the windows redraw once if any became links.
pub(super) fn probe_queued_file_mentions(cx: &gpui::App) {
    let queued = FILE_REFERENCES.with_borrow_mut(FileReferenceCache::take_queued);
    crate::background_probe::run(
        cx,
        queued,
        |path| FileReferenceCache::probe(path),
        |answers, cx| {
            let changed = FILE_REFERENCES.with_borrow_mut(|cache| {
                answers.into_iter().fold(false, |changed, (path, exists)| {
                    cache.fill(path, exists) | changed
                })
            });
            if changed {
                cx.refresh_windows();
            }
        },
    );
}

/// Inline spans: strip `code`, **bold**, *italic*, ~~strike~~ and link syntax,
/// returning plain text plus highlight ranges (byte offsets into the plain
/// text) and any link targets.
pub(super) fn inline_spans(source: &str) -> Inline {
    // Same-binary control is restricted to offline profiling, never live sessions.
    static SCALAR: LazyLock<bool> = LazyLock::new(|| {
        (crate::harness::screenshot_mode() || cfg!(test))
            && std::env::var("JCODE_DESKTOP_SCREENSHOT_SCALAR_INLINE").as_deref() == Ok("1")
    });
    if *SCALAR {
        inline_spans_impl::<false>(source)
    } else {
        inline_spans_impl::<true>(source)
    }
}

pub(super) fn inline_spans_impl<const BULK_TEXT: bool>(source: &str) -> Inline {
    let mut plain = String::with_capacity(source.len());
    let mut code_ranges = Vec::new();
    let mut highlights = Vec::new();
    let mut links = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;

    let code_style = HighlightStyle {
        color: Some(to_hsla(Theme::global().CODE_TEXT)),
        ..Default::default()
    };

    while i < bytes.len() {
        if BULK_TEXT {
            // Only these ASCII bytes can begin syntax handled below (h starts
            // a bare http(s) URL). Copy ordinary prose in one run instead of
            // constructing/checking every highlight style for every character.
            // ASCII delimiters cannot occur inside a UTF-8 continuation byte.
            let count = bytes[i..]
                .iter()
                .position(|byte| {
                    matches!(
                        byte,
                        b'\\' | b'`' | b'$' | b'[' | b'<' | b'h' | b'~' | b'*' | b'_'
                    )
                })
                .unwrap_or(bytes.len() - i);
            if count > 0 {
                plain.push_str(&source[i..i + count]);
                i += count;
                continue;
            }
        }
        // Inline math delimiters \( ... \) come before the generic escape
        // rule, which would otherwise eat the opening parenthesis.
        if source[i..].starts_with("\\(")
            && let Some(end) = source[i + 2..].find("\\)")
        {
            let content = latex_to_text(&source[i + 2..i + 2 + end]);
            let start = plain.len();
            plain.push_str(&content);
            highlights.push((
                start..plain.len(),
                HighlightStyle {
                    color: Some(to_hsla(Theme::global().CODE_TEXT)),
                    ..Default::default()
                },
            ));
            i += end + 4;
            continue;
        }
        // Escapes: \* keeps the literal character.
        if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_punctuation() {
            plain.push(bytes[i + 1] as char);
            i += 2;
            continue;
        }
        if bytes[i] == b'`' {
            let ticks = source[i..].chars().take_while(|c| *c == '`').count();
            let fence = "`".repeat(ticks);
            if let Some(end) = source[i + ticks..].find(&fence) {
                let content = source[i + ticks..i + ticks + end].trim();
                let start = plain.len();
                plain.push_str(content);
                code_ranges.push(start..plain.len());
                highlights.push((start..plain.len(), code_style));
                i += ticks + end + ticks;
                continue;
            }
        }
        if bytes[i] == b'$'
            && !bytes[i..].starts_with(b"$$")
            && let Some(end) = source[i + 1..].find('$')
        {
            let content = latex_to_text(&source[i + 1..i + 1 + end]);
            let start = plain.len();
            plain.push_str(&content);
            highlights.push((
                start..plain.len(),
                HighlightStyle {
                    color: Some(to_hsla(Theme::global().CODE_TEXT)),
                    ..Default::default()
                },
            ));
            i += end + 2;
            continue;
        }
        // An attachment marker is literal text, never a markdown link label,
        // even when prose in parentheses follows it directly: `[#image 1](see
        // above)` made the marker a link to "see above", hid its chip and sent
        // the OS an invalid URL on click (Finder error -50).
        if bytes[i] == b'['
            && let Some(marker) = crate::image_chip::image_marker_ranges(&source[i..])
                .into_iter()
                .find(|range| range.start == 0)
        {
            plain.push_str(&source[i..i + marker.end]);
            i += marker.end;
            continue;
        }
        // Links: [label](url) and bare <url>.
        if bytes[i] == b'['
            && let Some(close) = source[i..].find("](")
        {
            let label_end = i + close;
            if let Some(paren) = source[label_end + 2..].find(')') {
                let label = &source[i + 1..label_end];
                let url = source[label_end + 2..label_end + 2 + paren].trim();
                let nested = inline_spans_impl::<BULK_TEXT>(label);
                let start = plain.len();
                plain.push_str(&nested.plain);
                code_ranges.extend(
                    nested
                        .code_ranges
                        .into_iter()
                        .map(|range| start + range.start..start + range.end),
                );
                for (range, style) in nested.highlights {
                    highlights.push((start + range.start..start + range.end, style));
                }
                let range = start..plain.len();
                highlights.push((range.clone(), link_style()));
                links.push((range, url.to_string()));
                i = label_end + 3 + paren;
                continue;
            }
        }
        if bytes[i] == b'<'
            && let Some(close) = source[i..].find('>')
        {
            let inner = &source[i + 1..i + close];
            if inner.starts_with("http://") || inner.starts_with("https://") {
                let start = plain.len();
                plain.push_str(inner);
                let range = start..plain.len();
                highlights.push((range.clone(), link_style()));
                links.push((range, inner.to_string()));
                i += close + 1;
                continue;
            }
        }
        if source[i..].starts_with("http://") || source[i..].starts_with("https://") {
            let end = source[i..]
                .find(|c: char| c.is_whitespace())
                .map_or(source.len(), |p| i + p);
            let url = source[i..end].trim_end_matches(['.', ',', ')', ']', '!', '?', ';', ':']);
            let start = plain.len();
            plain.push_str(url);
            let range = start..plain.len();
            highlights.push((range.clone(), link_style()));
            links.push((range, url.to_string()));
            i += url.len();
            continue;
        }
        if bytes[i..].starts_with(b"~~")
            && let Some(end) = source[i + 2..].find("~~")
        {
            let nested = inline_spans_impl::<BULK_TEXT>(&source[i + 2..i + 2 + end]);
            let start = plain.len();
            plain.push_str(&nested.plain);
            code_ranges.extend(
                nested
                    .code_ranges
                    .into_iter()
                    .map(|range| start + range.start..start + range.end),
            );
            for (range, style) in nested.highlights {
                highlights.push((start + range.start..start + range.end, style));
            }
            for (range, url) in nested.links {
                links.push((start + range.start..start + range.end, url));
            }
            highlights.push((
                start..plain.len(),
                HighlightStyle {
                    strikethrough: Some(StrikethroughStyle {
                        thickness: px(1.0),
                        color: Some(to_hsla(Theme::global().TEXT_DIM)),
                    }),
                    color: Some(to_hsla(Theme::global().TEXT_DIM)),
                    ..Default::default()
                },
            ));
            i += end + 4;
            continue;
        }
        let strong_markers: [(&str, HighlightStyle); 3] = [
            (
                "***",
                HighlightStyle {
                    font_weight: Some(FontWeight::BOLD),
                    font_style: Some(gpui::FontStyle::Italic),
                    ..Default::default()
                },
            ),
            (
                "**",
                HighlightStyle {
                    font_weight: Some(FontWeight::BOLD),
                    ..Default::default()
                },
            ),
            (
                "__",
                HighlightStyle {
                    font_weight: Some(FontWeight::BOLD),
                    ..Default::default()
                },
            ),
        ];
        let mut matched_strong = false;
        for (marker, style) in strong_markers {
            if !source[i..].starts_with(marker) {
                continue;
            }
            // Unterminated emphasis falls through and prints literally, so a
            // response still reads correctly mid-stream.
            if let Some(end) = source[i + marker.len()..].find(marker) {
                let nested = inline_spans_impl::<BULK_TEXT>(
                    &source[i + marker.len()..i + marker.len() + end],
                );
                let start = plain.len();
                plain.push_str(&nested.plain);
                code_ranges.extend(
                    nested
                        .code_ranges
                        .into_iter()
                        .map(|range| start + range.start..start + range.end),
                );
                for (range, nested_style) in nested.highlights {
                    highlights.push((start + range.start..start + range.end, nested_style));
                }
                for (range, url) in nested.links {
                    links.push((start + range.start..start + range.end, url));
                }
                highlights.push((start..plain.len(), style));
                i += end + marker.len() * 2;
            } else {
                plain.push_str(marker);
                i += marker.len();
            }
            matched_strong = true;
            break;
        }
        if matched_strong {
            continue;
        }
        if bytes[i] == b'*' || bytes[i] == b'_' {
            let marker = bytes[i] as char;
            let boundary_ok = marker != '_'
                || i == 0
                || !source[..i]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric());
            if boundary_ok && let Some(end) = source[i + 1..].find(marker) {
                let inner = &source[i + 1..i + 1 + end];
                if !inner.is_empty() && !inner.starts_with(' ') {
                    let nested = inline_spans_impl::<BULK_TEXT>(inner);
                    let start = plain.len();
                    plain.push_str(&nested.plain);
                    code_ranges.extend(
                        nested
                            .code_ranges
                            .into_iter()
                            .map(|range| start + range.start..start + range.end),
                    );
                    for (range, style) in nested.highlights {
                        highlights.push((start + range.start..start + range.end, style));
                    }
                    for (range, url) in nested.links {
                        links.push((start + range.start..start + range.end, url));
                    }
                    highlights.push((
                        start..plain.len(),
                        HighlightStyle {
                            font_style: Some(gpui::FontStyle::Italic),
                            ..Default::default()
                        },
                    ));
                    i += end + 2;
                    continue;
                }
            }
        }
        // Advance one full UTF-8 character.
        let ch_len = source[i..].chars().next().map_or(1, char::len_utf8);
        plain.push_str(&source[i..i + ch_len]);
        i += ch_len;
    }
    Inline {
        plain,
        code_ranges,
        highlights,
        links,
    }
}

/// Only a URL with a scheme is handed to the OS. A link target such as
/// `сессия и TLDR видео` or `crates/foo.rs` is not openable, and macOS answers
/// it with a modal "The application can't be opened. -50".
pub(super) fn openable_url(url: &str) -> Option<&str> {
    let url = url.trim();
    let (scheme, rest) = url.split_once(':')?;
    let valid_scheme = scheme.len() > 1
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    (valid_scheme && !rest.is_empty() && !url.chars().any(char::is_whitespace)).then_some(url)
}

pub(super) fn link_style() -> HighlightStyle {
    HighlightStyle {
        color: Some(to_hsla(Theme::global().LINK)),
        underline: Some(UnderlineStyle {
            thickness: px(1.0),
            color: Some(to_hsla(Theme::global().LINK)),
            wavy: false,
        }),
        ..Default::default()
    }
}
