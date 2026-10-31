//! Tokenizer. Every byte of the input ends up in exactly one token, so the
//! syntax tree can reproduce the source text exactly.

use logos::Logos;

use crate::SyntaxKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: SyntaxKind,
    pub text: &'a str,
}

pub fn lex(text: &str) -> Vec<Token<'_>> {
    let mut lexer = SyntaxKind::lexer(text);
    let mut tokens = Vec::new();
    while let Some(result) = lexer.next() {
        let kind = result.unwrap_or(SyntaxKind::ERROR_TOKEN);
        let slice = lexer.slice();
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
        assert_eq!(k, vec![STRING, WHITESPACE, LINE_COMMENT, WHITESPACE, BLOCK_COMMENT]);
    }

    #[test]
    fn lossless_with_errors() {
        let text = "cube(1) @@ \"unterminated";
        let rebuilt: String = lex(text).iter().map(|t| t.text).collect();
        assert_eq!(rebuilt, text);
    }
}
