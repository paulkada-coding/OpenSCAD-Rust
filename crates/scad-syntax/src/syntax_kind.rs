//! Token and node kinds of the lossless syntax tree.

use logos::Logos;

#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
pub enum SyntaxKind {
    // ------------------------------------------------------------ trivia
    #[regex(r"[ \t\r\n\f]+")]
    WHITESPACE = 0,
    #[regex(r"//[^\n]*", allow_greedy = true)]
    LINE_COMMENT,
    #[regex(r"/\*([^*]|\*+[^*/])*\*+/")]
    BLOCK_COMMENT,
    /// Floating-point literal that overflows `f64`: skipped like trivia by the
    /// parser (matching OpenSCAD's lexer) while keeping the CST lossless.
    OVERFLOW_NUMBER,

    // ---------------------------------------------------------- literals
    #[regex(r"[0-9]+(\.[0-9]*)?([eE][+-]?[0-9]+)?", priority = 10)]
    #[regex(r"\.[0-9]+([eE][+-]?[0-9]+)?", priority = 10)]
    #[regex(r"0x[0-9a-fA-F]+", priority = 10)]
    NUMBER,
    #[regex(r#""([^"\\]|\\[\s\S])*""#)]
    STRING,
    #[regex(r#""([^"\\]|\\[\s\S])*\\?"#, priority = 1)]
    UNTERMINATED_STRING,
    #[regex(r"/\*([^*]|\*+[^*/])*\**", priority = 1)]
    UNTERMINATED_COMMENT,
    /// Identifiers may start with a digit (`2d_shape`) or `$` (`$fn`).
    /// The priority must stay below every keyword's (logos gives a literal
    /// token twice its length: "if" gets 4).
    #[regex(r"\$?[a-zA-Z0-9_]+", priority = 3)]
    IDENT,
    /// `include <path>` as a single token, like OpenSCAD's lexer.
    #[regex(r"include[ \t\r\n]*<[^>\n]*>")]
    INCLUDE,
    #[regex(r"use[ \t\r\n]*<[^>\n]*>")]
    USE,

    // ---------------------------------------------------------- keywords
    #[token("module")]
    MODULE_KW,
    #[token("function")]
    FUNCTION_KW,
    #[token("if")]
    IF_KW,
    #[token("else")]
    ELSE_KW,
    #[token("for")]
    FOR_KW,
    #[token("let")]
    LET_KW,
    #[token("each")]
    EACH_KW,
    #[token("assert")]
    ASSERT_KW,
    #[token("echo")]
    ECHO_KW,
    #[token("true")]
    TRUE_KW,
    #[token("false")]
    FALSE_KW,
    #[token("undef")]
    UNDEF_KW,

    // ------------------------------------------------------- punctuation
    #[token("(")]
    L_PAREN,
    #[token(")")]
    R_PAREN,
    #[token("[")]
    L_BRACK,
    #[token("]")]
    R_BRACK,
    #[token("{")]
    L_BRACE,
    #[token("}")]
    R_BRACE,
    #[token(";")]
    SEMI,
    #[token(",")]
    COMMA,
    #[token(".")]
    DOT,
    #[token("=")]
    EQ,
    #[token("==")]
    EQ2,
    #[token("!=")]
    NEQ,
    #[token("<")]
    LT,
    #[token("<=")]
    LTEQ,
    #[token(">")]
    GT,
    #[token(">=")]
    GTEQ,
    #[token("&&")]
    AMP2,
    #[token("||")]
    PIPE2,
    #[token("&")]
    AMP,
    #[token("|")]
    PIPE,
    #[token("~")]
    TILDE,
    #[token("<<")]
    SHL,
    #[token(">>")]
    SHR,
    #[token("!")]
    BANG,
    #[token("+")]
    PLUS,
    #[token("-")]
    MINUS,
    #[token("*")]
    STAR,
    #[token("/")]
    SLASH,
    #[token("%")]
    PERCENT,
    #[token("^")]
    CARET,
    #[token("?")]
    QUESTION,
    #[token(":")]
    COLON,
    #[token("#")]
    HASH,

    /// Any character the lexer does not understand.
    ERROR_TOKEN,

    // ------------------------------------------------------------- nodes
    SOURCE_FILE,
    ERROR,

    EMPTY_STMT,
    BLOCK,
    ASSIGNMENT,
    MODULE_DEF,
    FUNCTION_DEF,
    PARAM_LIST,
    PARAM,
    INCLUDE_STMT,
    USE_STMT,
    MODULE_CALL,
    IF_STMT,
    ARG_LIST,
    ARG,

    LITERAL,
    NAME_REF,
    PAREN_EXPR,
    UNARY_EXPR,
    BINARY_EXPR,
    TERNARY_EXPR,
    CALL_EXPR,
    INDEX_EXPR,
    MEMBER_EXPR,
    VECTOR_EXPR,
    RANGE_EXPR,
    LET_EXPR,
    ASSERT_EXPR,
    ECHO_EXPR,
    FUNCTION_LIT,

    LC_EACH,
    LC_FOR,
    LC_FOR_C,
    LC_IF,
    LC_LET,
    LC_PAREN,

    #[doc(hidden)]
    __LAST,
}

impl SyntaxKind {
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::WHITESPACE | Self::LINE_COMMENT | Self::BLOCK_COMMENT | Self::OVERFLOW_NUMBER
        )
    }
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScadLanguage {}

impl rowan::Language for ScadLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        assert!(raw.0 < SyntaxKind::__LAST as u16);
        // SAFETY-free conversion: the enum is `repr(u16)` with contiguous
        // discriminants starting at zero, checked by the assert above.
        KINDS[raw.0 as usize]
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub type SyntaxNode = rowan::SyntaxNode<ScadLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<ScadLanguage>;
pub type SyntaxElement = rowan::SyntaxElement<ScadLanguage>;

/// Lookup table from raw value to kind (avoids `unsafe` transmutes).
static KINDS: std::sync::LazyLock<Vec<SyntaxKind>> = std::sync::LazyLock::new(|| {
    use SyntaxKind::*;
    let all = [
        WHITESPACE,
        LINE_COMMENT,
        BLOCK_COMMENT,
        OVERFLOW_NUMBER,
        NUMBER,
        STRING,
        UNTERMINATED_STRING,
        UNTERMINATED_COMMENT,
        IDENT,
        INCLUDE,
        USE,
        MODULE_KW,
        FUNCTION_KW,
        IF_KW,
        ELSE_KW,
        FOR_KW,
        LET_KW,
        EACH_KW,
        ASSERT_KW,
        ECHO_KW,
        TRUE_KW,
        FALSE_KW,
        UNDEF_KW,
        L_PAREN,
        R_PAREN,
        L_BRACK,
        R_BRACK,
        L_BRACE,
        R_BRACE,
        SEMI,
        COMMA,
        DOT,
        EQ,
        EQ2,
        NEQ,
        LT,
        LTEQ,
        GT,
        GTEQ,
        AMP2,
        PIPE2,
        AMP,
        PIPE,
        TILDE,
        SHL,
        SHR,
        BANG,
        PLUS,
        MINUS,
        STAR,
        SLASH,
        PERCENT,
        CARET,
        QUESTION,
        COLON,
        HASH,
        ERROR_TOKEN,
        SOURCE_FILE,
        ERROR,
        EMPTY_STMT,
        BLOCK,
        ASSIGNMENT,
        MODULE_DEF,
        FUNCTION_DEF,
        PARAM_LIST,
        PARAM,
        INCLUDE_STMT,
        USE_STMT,
        MODULE_CALL,
        IF_STMT,
        ARG_LIST,
        ARG,
        LITERAL,
        NAME_REF,
        PAREN_EXPR,
        UNARY_EXPR,
        BINARY_EXPR,
        TERNARY_EXPR,
        CALL_EXPR,
        INDEX_EXPR,
        MEMBER_EXPR,
        VECTOR_EXPR,
        RANGE_EXPR,
        LET_EXPR,
        ASSERT_EXPR,
        ECHO_EXPR,
        FUNCTION_LIT,
        LC_EACH,
        LC_FOR,
        LC_FOR_C,
        LC_IF,
        LC_LET,
        LC_PAREN,
    ];
    for (i, kind) in all.iter().enumerate() {
        assert_eq!(*kind as usize, i, "KINDS table out of sync at {kind:?}");
    }
    assert_eq!(all.len(), __LAST as usize, "KINDS table is missing entries");
    all.to_vec()
});
