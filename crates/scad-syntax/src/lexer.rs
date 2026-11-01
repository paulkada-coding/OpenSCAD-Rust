//! Tokenizer. Every byte of the input ends up in exactly one token, so the
//! syntax tree can reproduce the source text exactly.

use logos::Logos;
use rowan::{TextRange, TextSize};

use crate::SyntaxKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: SyntaxKind,
    pub text: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxWarningKind {
    /// Identifier starting with an ASCII digit (`2b`, `0XFF`).
    DigitIdentifier(String),
    /// Unknown escape sequence in a string literal (`\q`, `\x80`, `\u12`…).
    UndefinedEscape,
    /// Integer literal that cannot round-trip losslessly through `f64`.
    ImpreciseInteger(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxWarning {
    pub kind: SyntaxWarningKind,
    pub range: TextRange,
}

/// Maximum number of `Undefined escape sequence` warnings emitted per string
/// literal, matching OpenSCAD's lexer cap.
const MAX_ESCAPE_WARNINGS_PER_STRING: usize = 5;

pub fn lex(text: &str) -> Vec<Token<'_>> {
    let mut lexer = SyntaxKind::lexer(text);
    let mut tokens = Vec::new();
    while let Some(result) = lexer.next() {
        let mut kind = result.unwrap_or(SyntaxKind::ERROR_TOKEN);
        let slice = lexer.slice();
        if kind == SyntaxKind::NUMBER
            && slice.contains(['.', 'e', 'E'])
            && !slice.parse::<f64>().is_ok_and(f64::is_finite)
        {
            kind = SyntaxKind::OVERFLOW_NUMBER;
        }
        // Merge consecutive unknown characters into a single error token.
        if kind == SyntaxKind::ERROR_TOKEN
            && let Some(last) = tokens.last_mut()
            && let Token {
                kind: SyntaxKind::ERROR_TOKEN,
                text: prev,
            } = last
        {
            let start = prev.as_ptr() as usize - text.as_ptr() as usize;
            *prev = &text[start..lexer.span().end];
            continue;
        }
        tokens.push(Token { kind, text: slice });
    }
    tokens
}

pub(crate) fn collect_warnings(tokens: &[Token<'_>]) -> Vec<SyntaxWarning> {
    let mut warnings = Vec::new();
    let mut offset = 0usize;
    for token in tokens {
        let range = TextRange::at(
            TextSize::from(offset as u32),
            TextSize::from(token.text.len() as u32),
        );
        match token.kind {
            SyntaxKind::IDENT
                if token
                    .text
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_digit) =>
            {
                warnings.push(SyntaxWarning {
                    kind: SyntaxWarningKind::DigitIdentifier(token.text.to_string()),
                    range,
                });
            }
            SyntaxKind::NUMBER if !token.text.contains(['.', 'e', 'E']) => {
                let parsed = match token.text.strip_prefix("0x") {
                    Some(hex) => u64::from_str_radix(hex, 16),
                    None => token.text.parse::<u64>(),
                };
                let precise = parsed.is_ok_and(|n| (n as f64) as u128 == u128::from(n));
                if !precise {
                    warnings.push(SyntaxWarning {
                        kind: SyntaxWarningKind::ImpreciseInteger(token.text.to_string()),
                        range,
                    });
                }
            }
            SyntaxKind::STRING => {
                let _ = decode_string_literal(token.text, offset, |esc_range| {
                    warnings.push(SyntaxWarning {
                        kind: SyntaxWarningKind::UndefinedEscape,
                        range: esc_range,
                    });
                });
            }
            _ => {}
        }
        offset += token.text.len();
    }
    warnings
}

/// Decodes an OpenSCAD string literal (`"..."`), matching the lexer's escape
/// rules (`\n`, `\t`, `\r`, `\\`, `\"`, `\xNN`, `\uNNNN`, `\UNNNNNN`, NUL→space).
pub fn unescape_string_literal(quoted: &str) -> String {
    decode_string_literal(quoted, 0, |_| {})
}

/// Decodes an OpenSCAD string literal (`"..."`), invoking `on_bad_escape` for
/// up to [`MAX_ESCAPE_WARNINGS_PER_STRING`] invalid escapes.
pub(crate) fn decode_string_literal(
    quoted: &str,
    base_offset: usize,
    mut on_bad_escape: impl FnMut(TextRange),
) -> String {
    let (inner, inner_offset) = quoted
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .map_or((quoted, base_offset), |s| (s, base_offset + 1));
    let mut out = String::with_capacity(inner.len());
    let mut bad_count = 0usize;
    let mut iter = inner.char_indices();
    while let Some((slash_rel, c)) = iter.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some((esc_rel, kind)) = iter.next() else {
            out.push('\\');
            break;
        };
        match kind {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            'x' | 'u' | 'U' => {
                let digits = match kind {
                    'x' => 2,
                    'u' => 4,
                    _ => 6,
                };
                let after = &inner[esc_rel + kind.len_utf8()..];
                let valid_hex = after.len() >= digits
                    && after.as_bytes()[..digits].iter().all(u8::is_ascii_hexdigit);
                let parsed = valid_hex
                    .then(|| u32::from_str_radix(&after[..digits], 16).ok())
                    .flatten()
                    .filter(|&v| kind != 'x' || v <= 0x7f);
                if let Some(val) = parsed {
                    for _ in 0..digits {
                        iter.next();
                    }
                    let decoded = if val == 0 {
                        ' '
                    } else {
                        char::from_u32(val).unwrap_or(' ')
                    };
                    out.push(decoded);
                } else {
                    if bad_count < MAX_ESCAPE_WARNINGS_PER_STRING {
                        bad_count += 1;
                        let start = TextSize::from((inner_offset + slash_rel) as u32);
                        let len = TextSize::from((1 + kind.len_utf8()) as u32);
                        on_bad_escape(TextRange::at(start, len));
                    }
                    out.push(kind);
                }
            }
            other => {
                if bad_count < MAX_ESCAPE_WARNINGS_PER_STRING {
                    bad_count += 1;
                    let start = TextSize::from((inner_offset + slash_rel) as u32);
                    let len = TextSize::from((1 + other.len_utf8()) as u32);
                    on_bad_escape(TextRange::at(start, len));
                }
                out.push(other);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use SyntaxKind::*;

    fn kinds(text: &str) -> Vec<SyntaxKind> {
        lex(text)
            .into_iter()
            .map(|t| t.kind)
            .filter(|k| !k.is_trivia())
            .collect()
    }

    #[test]
    fn numbers_and_identifiers() {
        assert_eq!(kinds("1 1.5 .5 1e3 2.5E-2 1."), vec![NUMBER; 6]);
        assert_eq!(kinds("$fn foo 2d_part _x"), vec![IDENT; 4]);
        assert_eq!(kinds("12abc"), vec![IDENT]);
    }

    #[test]
    fn keywords_win_over_identifiers() {
        assert_eq!(
            kinds("module modules for fortune if iff"),
            vec![MODULE_KW, IDENT, FOR_KW, IDENT, IF_KW, IDENT]
        );
    }

    #[test]
    fn include_and_use() {
        assert_eq!(kinds("include <a/b.scad>\nuse<c.scad>"), vec![INCLUDE, USE]);
        assert_eq!(kinds("use = 3;"), vec![IDENT, EQ, NUMBER, SEMI]);
    }

    #[test]
    fn operators() {
        assert_eq!(
            kinds("a<=b==c!=d&&e||!f^2%3"),
            vec![
                IDENT, LTEQ, IDENT, EQ2, IDENT, NEQ, IDENT, AMP2, IDENT, PIPE2, BANG, IDENT, CARET,
                NUMBER, PERCENT, NUMBER
            ]
        );
    }

    #[test]
    fn strings_and_comments() {
        let tokens = lex("\"a\\\"b\" // c\n/* d\n*/");
        let k: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            k,
            vec![STRING, WHITESPACE, LINE_COMMENT, WHITESPACE, BLOCK_COMMENT]
        );
    }

    #[test]
    fn lossless_with_errors() {
        let text = "cube(1) @@ \"unterminated";
        let rebuilt: String = lex(text).iter().map(|t| t.text).collect();
        assert_eq!(rebuilt, text);
    }
}
