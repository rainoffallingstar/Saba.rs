//! A lightweight Markdown renderer for game-comment preview.
//!
//! The design prototype leaves the comment box as a plain textarea, but the
//! PRD (§4.3) calls for Markdown study notes with a live preview. GPUI has no
//! built-in Markdown widget available without `&mut Window`, so this module
//! renders a focused subset — the constructs actually used in Go commentary:
//! `#`–`###` headings, `**bold**`, `` `inline code` ``, fenced code blocks,
//! `-` / `1.` lists, `>` quotes and paragraphs. Everything else falls back to
//! plain text. It is a pure view: input string in, element tree out, no state.

use gpui::{Div, FontWeight, ParentElement, Styled, div, px, rgb};

use crate::theme::UiPalette;

/// A parsed inline run: plain, bold or inline-code text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InlineKind {
    Plain,
    Bold,
    Code,
}

/// Renders a Markdown string into a GPUI element tree for the comment preview.
pub fn render_markdown(source: &str, palette: UiPalette) -> Div {
    let mut container = div().flex().flex_col().gap_1().text_xs();
    let lines = source.lines();
    let mut in_code_block = false;
    let mut code_lines: Vec<String> = Vec::new();

    for line in lines {
        let trimmed = line.trim_end();
        if trimmed.trim_start().starts_with("```") {
            if in_code_block {
                container = container.child(code_block(&code_lines.join("\n"), palette));
                code_lines.clear();
                in_code_block = false;
            } else {
                in_code_block = true;
            }
            continue;
        }
        if in_code_block {
            code_lines.push(line.to_owned());
            continue;
        }

        if let Some(heading) = parse_heading(trimmed) {
            container = container.child(render_heading(heading, palette));
        } else if let Some(quote) = trimmed.strip_prefix("> ") {
            container = container.child(render_quote(quote, palette));
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            container = container.child(render_list_item(item, palette, false, 0));
        } else if let Some((index, item)) = parse_ordered_item(trimmed) {
            container = container.child(render_list_item(item, palette, true, index));
        } else if trimmed.trim().is_empty() {
            container = container.child(div().h(px(4.0)));
        } else {
            container = container.child(render_paragraph(trimmed, palette));
        }
    }
    // Unterminated code block still renders its collected lines.
    if in_code_block && !code_lines.is_empty() {
        container = container.child(code_block(&code_lines.join("\n"), palette));
    }
    container
}

fn parse_heading(line: &str) -> Option<(usize, &str)> {
    for level in (1..=3).rev() {
        let prefix = "#".repeat(level) + " ";
        if let Some(text) = line.strip_prefix(prefix.as_str()) {
            return Some((level, text));
        }
    }
    None
}

fn parse_ordered_item(line: &str) -> Option<(usize, &str)> {
    let (num, rest) = line.split_once(". ")?;
    let index: usize = num.trim().parse().ok()?;
    Some((index, rest))
}

fn render_heading((level, text): (usize, &str), palette: UiPalette) -> Div {
    let size = match level {
        1 => crate::theme::type_scale::LG,
        2 => crate::theme::type_scale::BASE,
        _ => crate::theme::type_scale::SM,
    };
    div()
        .pt_1()
        .text_size(px(size))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(palette.text))
        .child(inline_runs(text, palette, size))
}

fn render_quote(text: &str, palette: UiPalette) -> Div {
    div()
        .pl_2()
        .border_l_2()
        .border_color(rgb(palette.border))
        .text_color(rgb(palette.muted))
        .child(inline_runs(text, palette, crate::theme::type_scale::XS))
}

fn render_list_item(text: &str, palette: UiPalette, ordered: bool, index: usize) -> Div {
    let marker = if ordered {
        format!("{index}.")
    } else {
        "•".to_owned()
    };
    div()
        .flex()
        .gap_1p5()
        .child(
            div()
                .flex_none()
                .text_color(rgb(palette.muted))
                .child(marker),
        )
        .child(
            div()
                .flex_1()
                .text_color(rgb(palette.text))
                .child(inline_runs(text, palette, crate::theme::type_scale::XS)),
        )
}

fn render_paragraph(text: &str, palette: UiPalette) -> Div {
    div().text_color(rgb(palette.text)).child(inline_runs(
        text,
        palette,
        crate::theme::type_scale::XS,
    ))
}

fn code_block(code: &str, palette: UiPalette) -> Div {
    div()
        .w_full()
        .px_2()
        .py_1p5()
        .rounded_sm()
        .bg(rgb(palette.input))
        .border_1()
        .border_color(rgb(palette.border_soft))
        .text_color(rgb(palette.text_secondary))
        .child(div().font_family("monospace").child(code.to_owned()))
}

/// Builds the inline (bold/code) run sequence for one line of text.
fn inline_runs(text: &str, palette: UiPalette, _size: f32) -> Div {
    let mut row = div().flex().flex_wrap();
    for (kind, segment) in parse_inline(text) {
        if segment.is_empty() {
            continue;
        }
        let segment = segment.to_owned();
        row = row.child(match kind {
            InlineKind::Plain => div().child(segment),
            InlineKind::Bold => div()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(palette.text))
                .child(segment),
            InlineKind::Code => div()
                .px_1()
                .rounded_sm()
                .bg(rgb(palette.input))
                .text_color(rgb(palette.accent))
                .font_family("monospace")
                .child(segment),
        });
    }
    row
}

/// Splits a line into plain/bold/code runs for `**bold**` and `` `code` ``.
fn parse_inline(text: &str) -> Vec<(InlineKind, &str)> {
    let mut runs = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let bold_pos = rest.find("**");
        let code_pos = rest.find('`');
        let (pos, marker_len, kind) = match (bold_pos, code_pos) {
            (Some(b), Some(c)) if b <= c => (b, 2, InlineKind::Bold),
            (Some(_), Some(c)) => (c, 1, InlineKind::Code),
            (Some(b), None) => (b, 2, InlineKind::Bold),
            (None, Some(c)) => (c, 1, InlineKind::Code),
            (None, None) => {
                runs.push((InlineKind::Plain, rest));
                break;
            }
        };
        if pos > 0 {
            runs.push((InlineKind::Plain, &rest[..pos]));
        }
        let after = &rest[pos + marker_len..];
        let close = match kind {
            InlineKind::Bold => after.find("**"),
            _ => after.find('`'),
        };
        match close {
            Some(end) => {
                runs.push((kind, &after[..end]));
                rest = &after[end + marker_len..];
            }
            None => {
                // Unterminated marker: render the marker literally as plain text.
                runs.push((InlineKind::Plain, &rest[pos..]));
                break;
            }
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::parse_inline;

    #[test]
    fn splits_bold_and_code() {
        let runs = parse_inline("normal **bold** and `code` end");
        assert!(runs.contains(&(super::InlineKind::Bold, "bold")));
        assert!(runs.contains(&(super::InlineKind::Code, "code")));
    }

    #[test]
    fn unterminated_marker_stays_literal() {
        let runs = parse_inline("an **open bold");
        assert_eq!(
            runs.last(),
            Some(&(super::InlineKind::Plain, "**open bold"))
        );
    }
}
