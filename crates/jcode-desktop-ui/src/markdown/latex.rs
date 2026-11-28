//! Turning LaTeX notation into readable Unicode.

/// Turn common LaTeX notation into readable Unicode. GPUI does not currently
/// provide a TeX layout engine, so this keeps formulas clean instead of
/// exposing commands and delimiters. Unknown commands remain visible.
pub(super) fn latex_to_text(source: &str) -> String {
    const REPLACEMENTS: &[(&str, &str)] = &[
        ("\\rightarrow", "→"),
        ("\\leftarrow", "←"),
        ("\\infty", "∞"),
        ("\\notin", "∉"),
        ("\\partial", "∂"),
        ("\\approx", "≈"),
        ("\\theta", "θ"),
        ("\\alpha", "α"),
        ("\\gamma", "γ"),
        ("\\delta", "δ"),
        ("\\lambda", "λ"),
        ("\\sigma", "σ"),
        ("\\omega", "ω"),
        ("\\times", "×"),
        ("\\cdot", "·"),
        ("\\nabla", "∇"),
        ("\\forall", "∀"),
        ("\\exists", "∃"),
        ("\\ldots", "…"),
        ("\\cdots", "⋯"),
        ("\\beta", "β"),
        ("\\phi", "φ"),
        ("\\pi", "π"),
        ("\\mu", "μ"),
        ("\\pm", "±"),
        ("\\leq", "≤"),
        ("\\geq", "≥"),
        ("\\neq", "≠"),
        ("\\to", "→"),
        ("\\sum", "∑"),
        ("\\prod", "∏"),
        ("\\int", "∫"),
        ("\\in", "∈"),
        ("\\cup", "∪"),
        ("\\cap", "∩"),
        ("\\quad", "  "),
        ("\\,", " "),
        ("\\!", ""),
    ];
    // Preserve the structure of commands whose arguments matter before the
    // simple command replacement pass removes braces. In particular, merely
    // stripping `\frac` and braces produced mangled output such as
    // `x=\frac-b±√b²-4ac2a`.
    let mut text = expand_latex_groups(source.trim());
    // Layout-only commands have no useful textual representation. Models
    // commonly put `\displaystyle` at the start of inline formulas, and
    // leaving it intact is especially distracting in transcript output.
    // Most commands are absent in any one formula. String::replace still
    // allocates and copies the entire expression on a miss, so avoid dozens
    // of discarded buffers while preserving the ordered substitution rules.
    if text.contains('\\') {
        for (from, to) in [
            ("\\displaystyle", ""),
            ("\\textstyle", ""),
            ("\\scriptstyle", ""),
        ]
        .into_iter()
        .chain(REPLACEMENTS.iter().copied())
        .chain([("\\left", ""), ("\\right", "")])
        {
            if text.contains(from) {
                text = text.replace(from, to);
            }
        }
    }
    if text.contains('^') {
        text = convert_scripts(&text, '^');
    }
    if text.contains('_') {
        text = convert_scripts(&text, '_');
    }
    text.retain(|ch| ch != '{' && ch != '}');
    text.truncate(text.trim_end().len());
    let leading = text.len() - text.trim_start().len();
    if leading > 0 {
        text.drain(..leading);
    }
    text
}

/// Expand the small set of braced TeX commands that affect how an expression
/// reads in plain text. This is intentionally not a TeX engine, but it keeps
/// common model output mathematically unambiguous.
pub(super) fn expand_latex_groups(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut i = 0;
    while i < source.len() {
        let rest = &source[i..];
        // Group syntax can only start with an ASCII backslash. Copy ordinary
        // formula text in one run instead of probing six commands per glyph.
        if !rest.starts_with('\\') {
            let end = rest.find('\\').unwrap_or(rest.len());
            output.push_str(&rest[..end]);
            i += end;
            continue;
        }
        if let Some(after_command) = rest.strip_prefix("\\frac") {
            let offset = i + rest.len() - after_command.len();
            if let Some((numerator, after_numerator)) = braced_group(source, offset)
                && let Some((denominator, after_denominator)) =
                    braced_group(source, after_numerator)
            {
                output.push('(');
                output.push_str(&expand_latex_groups(numerator));
                output.push_str(")/(");
                output.push_str(&expand_latex_groups(denominator));
                output.push(')');
                i = after_denominator;
                continue;
            }
        }
        if let Some(after_command) = rest.strip_prefix("\\sqrt") {
            let offset = i + rest.len() - after_command.len();
            if let Some((radicand, after_radicand)) = braced_group(source, offset) {
                output.push_str("√(");
                output.push_str(&expand_latex_groups(radicand));
                output.push(')');
                i = after_radicand;
                continue;
            }
        }
        let wrapper = ["\\mathbf", "\\mathrm", "\\boxed", "\\text"]
            .iter()
            .find_map(|command| rest.strip_prefix(command));
        if let Some(after_command) = wrapper {
            let offset = i + rest.len() - after_command.len();
            if let Some((content, after_content)) = braced_group(source, offset) {
                output.push_str(&expand_latex_groups(content));
                i = after_content;
                continue;
            }
        }
        let ch = rest.chars().next().expect("non-empty remainder");
        output.push(ch);
        i += ch.len_utf8();
    }
    output
}

pub(super) fn braced_group(source: &str, mut start: usize) -> Option<(&str, usize)> {
    while source[start..]
        .chars()
        .next()
        .is_some_and(char::is_whitespace)
    {
        start += source[start..].chars().next()?.len_utf8();
    }
    if !source[start..].starts_with('{') {
        return None;
    }
    let content_start = start + 1;
    let mut depth = 1;
    for (offset, ch) in source[content_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let end = content_start + offset;
                    return Some((&source[content_start..end], end + 1));
                }
            }
            _ => {}
        }
    }
    None
}

pub(super) fn convert_scripts(source: &str, marker: char) -> String {
    let (keys, table) = if marker == '^' {
        ("0123456789+-=()niab", "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ⁿⁱᵃᵇ")
    } else {
        ("0123456789+-=()nia", "₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎ₙᵢₐ")
    };
    let map = |c: char| keys.find(c).and_then(|i| table.chars().nth(i));
    let chars: Vec<char> = source.chars().collect();
    let mut output = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != marker {
            output.push(chars[i]);
            i += 1;
            continue;
        }
        let (start, end) = if chars.get(i + 1) == Some(&'{') {
            let end = chars[i + 2..]
                .iter()
                .position(|c| *c == '}')
                .map(|p| i + 2 + p);
            match end {
                Some(end) => (i + 2, end),
                None => {
                    output.push(marker);
                    i += 1;
                    continue;
                }
            }
        } else if i + 1 < chars.len() {
            (i + 1, i + 2)
        } else {
            output.push(marker);
            break;
        };
        let converted: Option<String> = chars[start..end].iter().map(|c| map(*c)).collect();
        if let Some(converted) = converted {
            output.push_str(&converted);
        } else {
            output.push(marker);
            if end - start > 1 {
                output.push('(');
            }
            output.extend(chars[start..end].iter());
            if end - start > 1 {
                output.push(')');
            }
        }
        i = if chars.get(i + 1) == Some(&'{') {
            end + 1
        } else {
            end
        };
    }
    output
}
