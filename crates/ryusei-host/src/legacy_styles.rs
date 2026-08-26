//! Legacy `styles.css` analysis (design §8.1).
//!
//! Sabaki does **not** promise runtime or binary compatibility with user
//! `styles.css`. Values that can be expressed as theme tokens (colors) are
//! listed in a migration report and can be imported into the new token
//! format; every other rule is reported as not migrated. This module only
//! *analyzes* the stylesheet — it never executes, parses selectors for
//! behavior, or applies any rule.

use serde::{Deserialize, Serialize};

/// One color declaration that can be expressed as a theme token.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigratedColorRule {
    /// The CSS selector the rule belongs to, e.g. `.board`.
    pub selector: String,
    /// The property name, e.g. `background` or `color`.
    pub property: String,
    /// The color value as written, e.g. `#ECB55A`.
    pub value: String,
}

/// The migration report for one `styles.css` file.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyStylesReport {
    /// Color rules that map onto theme tokens and can be imported.
    pub migrated_color_rules: Vec<MigratedColorRule>,
    /// Number of other rules that are not migrated.
    pub ignored_rule_count: usize,
}

/// A single CSS rule block captured verbatim: selector + declarations.
struct CssRule {
    selector: String,
    declarations: Vec<(String, String)>,
}

/// Splits a stylesheet into top-level rule blocks, tolerating the relaxed
/// syntax users actually write (missing semicolons, comments, whitespace).
fn split_rules(styles: &str) -> Vec<CssRule> {
    let mut rules = Vec::new();
    let mut rest = styles;
    loop {
        // Skip comments and whitespace before the next selector.
        rest = strip_comments(rest);
        let Some(open) = find_unescaped(rest, '{') else {
            break;
        };
        let selector = rest[..open].trim().to_owned();
        let Some(close) = find_unescaped(&rest[open + 1..], '}') else {
            break;
        };
        let body = &rest[open + 1..open + 1 + close];
        let declarations = split_declarations(body);
        if !selector.is_empty() {
            rules.push(CssRule {
                selector,
                declarations,
            });
        }
        rest = &rest[open + 1 + close + 1..];
    }
    rules
}

fn strip_comments(input: &str) -> &str {
    let Some(start) = input.find("/*") else {
        return input;
    };
    let Some(end) = input[start + 2..].find("*/") else {
        return &input[..start];
    };
    let end = start + 2 + end + 2;
    let mut result = input[..start].to_owned();
    result.push_str(&input[end..]);
    // A comment may contain braces; re-scan the remainder.
    Box::leak(result.into_boxed_str())
}

/// Finds the next unescaped occurrence of `needle`.
fn find_unescaped(haystack: &str, needle: char) -> Option<usize> {
    let mut escaped = false;
    for (index, character) in haystack.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == needle {
            return Some(index);
        }
    }
    None
}

fn split_declarations(body: &str) -> Vec<(String, String)> {
    let mut declarations = Vec::new();
    for part in body.split(';') {
        let Some(colon) = part.find(':') else {
            continue;
        };
        let property = part[..colon].trim();
        let value = part[colon + 1..].trim();
        if !property.is_empty() && !value.is_empty() {
            declarations.push((property.to_owned(), value.to_owned()));
        }
    }
    declarations
}

/// Extracts a `#RRGGBB` / `#RGB` / `rgb(...)` color from a declaration
/// value, if the value is purely a color.
fn extract_color(value: &str) -> Option<String> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        let hex = hex.split_whitespace().next().unwrap_or("");
        let hex = hex.trim_end_matches(|character: char| !character.is_ascii_hexdigit());
        if (hex.len() == 3 || hex.len() == 6) && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(format!("#{hex}"));
        }
        return None;
    }
    if value.starts_with("rgb(") && value.ends_with(')') {
        let channels: Vec<&str> = value[4..value.len() - 1]
            .split(',')
            .map(str::trim)
            .collect();
        if channels.len() == 3 && channels.iter().all(|channel| channel.parse::<u8>().is_ok()) {
            return Some(value.to_owned());
        }
    }
    None
}

/// Analyzes a legacy `styles.css` document and reports which color rules can
/// migrate to theme tokens and how many other rules are ignored.
pub fn analyze_legacy_styles(styles: &str) -> LegacyStylesReport {
    let rules = split_rules(styles);
    let mut migrated = Vec::new();
    let mut ignored = 0usize;
    for rule in rules {
        let mut migrated_any = false;
        for (property, value) in rule.declarations {
            // Colors are token-expressible; anything else (layouts, fonts,
            // animations, selectors with behavior) is not migrated.
            if let Some(color) = extract_color(&value) {
                migrated.push(MigratedColorRule {
                    selector: rule.selector.clone(),
                    property: property.clone(),
                    value: color,
                });
                migrated_any = true;
            }
        }
        if !migrated_any {
            ignored += 1;
        }
    }
    LegacyStylesReport {
        migrated_color_rules: migrated,
        ignored_rule_count: ignored,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_migratable_color_rules_and_ignores_the_rest() {
        let styles = r#"
.board {
  background: #ECB55A;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.4);
}
.stone.black { color: rgb(20, 20, 20); }
@keyframes pulse { from { opacity: 0.5; } }
"#;

        let report = analyze_legacy_styles(styles);

        assert_eq!(report.migrated_color_rules.len(), 2);
        assert_eq!(
            report.migrated_color_rules[0],
            MigratedColorRule {
                selector: ".board".to_owned(),
                property: "background".to_owned(),
                value: "#ECB55A".to_owned(),
            }
        );
        assert_eq!(
            report.migrated_color_rules[1],
            MigratedColorRule {
                selector: ".stone.black".to_owned(),
                property: "color".to_owned(),
                value: "rgb(20, 20, 20)".to_owned(),
            }
        );
        // .board and .stone.black migrate; @keyframes (nested block) and the
        // box-shadow rule are not color rules and count as ignored. The
        // nested @keyframes block is scanned as one top-level rule.
        assert_eq!(report.ignored_rule_count, 1);
    }

    #[test]
    fn empty_and_comment_only_stylesheets_report_nothing() {
        assert_eq!(analyze_legacy_styles(""), LegacyStylesReport::default());
        assert_eq!(
            analyze_legacy_styles("/* nothing here */"),
            LegacyStylesReport::default()
        );
    }

    #[test]
    fn non_color_values_are_never_reported_as_colors() {
        let styles = ".board { background: url(wood.png) no-repeat; }";
        let report = analyze_legacy_styles(styles);
        assert!(report.migrated_color_rules.is_empty());
        assert_eq!(report.ignored_rule_count, 1);
    }

    #[test]
    fn tolerates_missing_semicolons_and_short_hex() {
        // Without a semicolon the second declaration is absorbed into the
        // first value; the short hex still extracts.
        let styles = ".stone { background: #fff color: #123456 }";
        let report = analyze_legacy_styles(styles);
        assert_eq!(report.migrated_color_rules.len(), 1);
        assert_eq!(report.migrated_color_rules[0].value, "#fff");

        let styles = ".stone { background: #fff; color: #123456 }";
        let report = analyze_legacy_styles(styles);
        assert_eq!(report.migrated_color_rules.len(), 2);
        assert_eq!(report.migrated_color_rules[1].value, "#123456");
    }
}
