//! Recursive-descent parser producing a lossless rowan tree.
//!
//! Trivia (whitespace, comments) is attached to whichever node is open when
//! the next significant token is consumed, so the tree text always equals
//! the input text.

use rowan::{Checkpoint, GreenNode, GreenNodeBuilder, TextRange, TextSize};

use crate::SyntaxKind::{self, *};
use crate::lexer::{Token, lex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub message: String,
    pub range: TextRange,
}

pub(crate) fn parse_text(text: &str) -> (GreenNode, Vec<SyntaxError>) {
    let tokens = lex(text);
    let mut parser = Parser {
        tokens,
        pos: 0,
        offset: 0,
        builder: GreenNodeBuilder::new(),
        errors: Vec::new(),
    };
    parser.source_file();
    (parser.builder.finish(), parser.errors)
}

/// Tokens that can begin a module instantiation.
const INSTANTIATION_START: &[SyntaxKind] = &[
    IDENT, IF_KW, FOR_KW, LET_KW, ECHO_KW, ASSERT_KW, BANG, HASH, PERCENT, STAR,
];
const MODIFIERS: &[SyntaxKind] = &[BANG, HASH, PERCENT, STAR];

struct Parser<'t> {
    tokens: Vec<Token<'t>>,
    pos: usize,
    /// Byte offset of `tokens[pos]`.
    offset: usize,
    builder: GreenNodeBuilder<'static>,
    errors: Vec<SyntaxError>,
}

impl Parser<'_> {
    // ------------------------------------------------------------ cursor

    /// Kind of the n-th significant token ahead (EOF is `__LAST`).
    fn nth(&self, n: usize) -> SyntaxKind {
        self.tokens[self.pos..]
            .iter()
            .filter(|t| !t.kind.is_trivia())
            .nth(n)
            .map_or(SyntaxKind::__LAST, |t| t.kind)
    }

    fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    fn at_any(&self, kinds: &[SyntaxKind]) -> bool {
        kinds.contains(&self.current())
    }

    fn at_eof(&self) -> bool {
        self.current() == SyntaxKind::__LAST
    }

    fn eat_trivia(&mut self) {
        while let Some(token) = self.tokens.get(self.pos) {
            if !token.kind.is_trivia() {
                break;
            }
            self.push_token();
        }
    }

    fn push_token(&mut self) {
        let token = self.tokens[self.pos];
        self.builder.token(token.kind.into(), token.text);
        self.offset += token.text.len();
        self.pos += 1;
    }

    /// Consumes the current significant token (and the trivia before it).
    fn bump(&mut self) {
        self.eat_trivia();
        if self.pos < self.tokens.len() {
            self.push_token();
        }
    }

    fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: SyntaxKind, what: &str) -> bool {
        if self.eat(kind) {
            true
        } else {
            self.error(format!("atteso {what}"));
            false
        }
    }

    // ------------------------------------------------------------- nodes

    fn start(&mut self, kind: SyntaxKind) {
        self.eat_trivia();
        self.builder.start_node(kind.into());
    }

    fn finish(&mut self) {
        self.builder.finish_node();
    }

    fn checkpoint(&mut self) -> Checkpoint {
        self.eat_trivia();
        self.builder.checkpoint()
    }

    fn start_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        self.builder.start_node_at(checkpoint, kind.into());
    }

    // ------------------------------------------------------------ errors

    /// Range of the next significant token (or an empty range at EOF).
    fn current_range(&self) -> TextRange {
        let mut offset = self.offset;
        for token in &self.tokens[self.pos..] {
            if !token.kind.is_trivia() {
                let start = TextSize::from(offset as u32);
                return TextRange::at(start, TextSize::from(token.text.len() as u32));
            }
            offset += token.text.len();
        }
        TextRange::empty(TextSize::from(offset as u32))
    }

    fn error(&mut self, message: impl Into<String>) {
        let found = self.tokens[self.pos..]
            .iter()
            .find(|t| !t.kind.is_trivia())
            .map_or_else(|| "fine del file".to_string(), |t| format!("'{}'", t.text));
        self.errors.push(SyntaxError {
            message: format!("{}, trovato {found}", message.into()),
            range: self.current_range(),
        });
    }

    /// Wraps the current token in an ERROR node (never at EOF).
    fn error_bump(&mut self, message: &str) {
        self.error(message);
        if !self.at_eof() {
            self.start(ERROR);
            self.bump();
            self.finish();
        }
    }

    // -------------------------------------------------------- statements

    fn source_file(&mut self) {
        self.builder.start_node(SOURCE_FILE.into());
        while !self.at_eof() {
            if self.at(R_BRACE) {
                self.error_bump("'}' senza '{' corrispondente");
                continue;
            }
            self.statement();
        }
        self.eat_trivia();
        self.finish();
    }

    fn statement(&mut self) {
        match self.current() {
            SEMI => {
                self.start(EMPTY_STMT);
                self.bump();
                self.finish();
            }
            L_BRACE => self.block(),
            MODULE_KW => self.module_def(),
            FUNCTION_KW => self.function_def(),
            INCLUDE => {
                self.start(INCLUDE_STMT);
                self.bump();
                self.finish();
            }
            USE => {
                self.start(USE_STMT);
                self.bump();
                self.finish();
            }
            IDENT if self.nth(1) == EQ => self.assignment(),
            kind if INSTANTIATION_START.contains(&kind) => self.instantiation(),
            _ => self.recover_statement(),
        }
    }

    /// Skips tokens until something that can start or end a statement.
    fn recover_statement(&mut self) {
        self.error("attesa un'istruzione");
        self.start(ERROR);
        loop {
            if self.at_eof() || self.at(R_BRACE) {
                break;
            }
            if self.eat(SEMI) {
                break;
            }
            self.bump();
            if self.at_any(&[MODULE_KW, FUNCTION_KW, INCLUDE, USE, L_BRACE]) {
                break;
            }
        }
        self.finish();
    }

    fn block(&mut self) {
        self.start(BLOCK);
        self.bump(); // '{'
        while !self.at_eof() && !self.at(R_BRACE) {
            self.statement();
        }
        self.expect(R_BRACE, "'}'");
        self.finish();
    }

    fn assignment(&mut self) {
        self.start(ASSIGNMENT);
        self.bump(); // name
        self.bump(); // '='
        self.expr();
        self.expect(SEMI, "';'");
        self.finish();
    }

    fn module_def(&mut self) {
        self.start(MODULE_DEF);
        self.bump(); // 'module'
        self.expect(IDENT, "il nome del modulo");
        self.param_list();
        if self.at_eof() {
            self.error("atteso il corpo del modulo");
        } else {
            self.statement();
        }
        self.finish();
    }

    fn function_def(&mut self) {
        self.start(FUNCTION_DEF);
        self.bump(); // 'function'
        self.expect(IDENT, "il nome della funzione");
        self.param_list();
        self.expect(EQ, "'='");
        self.expr();
        self.expect(SEMI, "';'");
        self.finish();
    }

    fn param_list(&mut self) {
        self.start(PARAM_LIST);
        if !self.expect(L_PAREN, "'('") {
            self.finish();
            return;
        }
        while !self.at_eof() && !self.at(R_PAREN) {
            if self.at(IDENT) {
                self.start(PARAM);
                self.bump();
                if self.eat(EQ) {
                    self.expr();
                }
                self.finish();
            } else {
                self.error_bump("atteso il nome di un parametro");
                if self.at_any(&[SEMI, L_BRACE, EQ]) {
                    break;
                }
                continue;
            }
            if !self.eat(COMMA) {
                break;
            }
        }
        self.expect(R_PAREN, "')'");
        self.finish();
    }

    /// `[modifiers] name(args) child` or `[modifiers] if (cond) child [else child]`.
    fn instantiation(&mut self) {
        let checkpoint = self.checkpoint();
        while self.at_any(MODIFIERS) {
            self.bump();
        }
        if self.at(IF_KW) {
            self.bump();
            self.expect(L_PAREN, "'('");
            self.expr();
            self.expect(R_PAREN, "')'");
            self.child_statement();
            if self.eat(ELSE_KW) {
                self.child_statement();
            }
            self.start_at(checkpoint, IF_STMT);
            self.finish();
        } else if self.at_any(&[IDENT, FOR_KW, LET_KW, ECHO_KW, ASSERT_KW]) {
            self.bump();
            self.arg_list();
            self.child_statement();
            self.start_at(checkpoint, MODULE_CALL);
            self.finish();
        } else {
            self.error("atteso il nome di un modulo");
            self.start_at(checkpoint, ERROR);
            self.finish();
        }
    }

    fn child_statement(&mut self) {
        match self.current() {
            SEMI => self.bump(),
            L_BRACE => self.block(),
            kind if INSTANTIATION_START.contains(&kind) => self.instantiation(),
            _ => self.error("atteso ';', '{' o un modulo"),
        }
    }

    fn arg_list(&mut self) {
        self.start(ARG_LIST);
        if !self.expect(L_PAREN, "'('") {
            self.finish();
            return;
        }
        self.args_until(R_PAREN);
        self.expect(R_PAREN, "')'");
        self.finish();
    }

    /// Comma-separated `[name =] expr` items up to (not including) `end`.
    fn args_until(&mut self, end: SyntaxKind) {
        while !self.at_eof() && !self.at(end) {
            if self.at_any(&[SEMI, R_BRACE, R_PAREN, R_BRACK]) {
                break;
            }
            self.start(ARG);
            if self.at(IDENT) && self.nth(1) == EQ {
                self.bump();
                self.bump();
            }
            self.expr();
            self.finish();
            if !self.eat(COMMA) {
                break;
            }
        }
    }

    // ------------------------------------------------------- expressions

    fn expr(&mut self) {
        match self.current() {
            FUNCTION_KW => {
                self.start(FUNCTION_LIT);
                self.bump();
                self.param_list();
                self.expr();
                self.finish();
            }
            LET_KW => {
                self.start(LET_EXPR);
                self.bump();
                self.arg_list();
                self.expr();
                self.finish();
            }
            ASSERT_KW | ECHO_KW => {
                let kind = if self.at(ASSERT_KW) {
                    ASSERT_EXPR
                } else {
                    ECHO_EXPR
                };
                self.start(kind);
                self.bump();
                self.arg_list();
                if self.at_expr_start() {
                    self.expr();
                }
                self.finish();
            }
            _ => self.ternary(),
        }
    }

    fn at_expr_start(&self) -> bool {
        self.at_any(&[
            NUMBER, STRING, IDENT, TRUE_KW, FALSE_KW, UNDEF_KW, L_PAREN, L_BRACK, PLUS, MINUS,
            BANG, TILDE, FUNCTION_KW, LET_KW, ASSERT_KW, ECHO_KW,
        ])
    }

    fn ternary(&mut self) {
        let checkpoint = self.checkpoint();
        self.logic_or();
        if self.at(QUESTION) {
            self.start_at(checkpoint, TERNARY_EXPR);
            self.bump();
            self.expr();
            self.expect(COLON, "':'");
            self.expr();
            self.finish();
        }
    }

    fn binary_level(&mut self, ops: &[SyntaxKind], operand: fn(&mut Self)) {
        let checkpoint = self.checkpoint();
        operand(self);
        while self.at_any(ops) {
            self.start_at(checkpoint, BINARY_EXPR);
            self.bump();
            operand(self);
            self.finish();
        }
    }

    fn logic_or(&mut self) {
        self.binary_level(&[PIPE2], Self::logic_and);
    }

    fn logic_and(&mut self) {
        self.binary_level(&[AMP2], Self::equality);
    }

    fn equality(&mut self) {
        self.binary_level(&[EQ2, NEQ], Self::comparison);
    }

    fn comparison(&mut self) {
        self.binary_level(&[LT, LTEQ, GT, GTEQ], Self::bit_or);
    }

    // Bitwise operators sit between comparisons and arithmetic.
    fn bit_or(&mut self) {
        self.binary_level(&[PIPE], Self::bit_and);
    }

    fn bit_and(&mut self) {
        self.binary_level(&[AMP], Self::shift);
    }

    fn shift(&mut self) {
        self.binary_level(&[SHL, SHR], Self::additive);
    }

    fn additive(&mut self) {
        self.binary_level(&[PLUS, MINUS], Self::multiplicative);
    }

    fn multiplicative(&mut self) {
        self.binary_level(&[STAR, SLASH, PERCENT], Self::unary);
    }

    fn unary(&mut self) {
        if self.at_any(&[PLUS, MINUS, BANG, TILDE]) {
            self.start(UNARY_EXPR);
            self.bump();
            self.unary();
            self.finish();
        } else {
            self.exponent();
        }
    }

    /// `^` binds tighter than unary minus and is right-associative.
    fn exponent(&mut self) {
        let checkpoint = self.checkpoint();
        self.postfix();
        if self.at(CARET) {
            self.start_at(checkpoint, BINARY_EXPR);
            self.bump();
            self.unary();
            self.finish();
        }
    }

    fn postfix(&mut self) {
        let checkpoint = self.checkpoint();
        self.primary();
        loop {
            match self.current() {
                L_PAREN => {
                    self.start_at(checkpoint, CALL_EXPR);
                    self.arg_list();
                    self.finish();
                }
                L_BRACK => {
                    self.start_at(checkpoint, INDEX_EXPR);
                    self.bump();
                    self.expr();
                    self.expect(R_BRACK, "']'");
                    self.finish();
                }
                DOT => {
                    self.start_at(checkpoint, MEMBER_EXPR);
                    self.bump();
                    self.expect(IDENT, "il nome di un membro");
                    self.finish();
                }
                _ => break,
            }
        }
    }

    fn primary(&mut self) {
        match self.current() {
            NUMBER | STRING | TRUE_KW | FALSE_KW | UNDEF_KW => {
                self.start(LITERAL);
                self.bump();
                self.finish();
            }
            IDENT => {
                self.start(NAME_REF);
                self.bump();
                self.finish();
            }
            L_PAREN => {
                self.start(PAREN_EXPR);
                self.bump();
                self.expr();
                self.expect(R_PAREN, "')'");
                self.finish();
            }
            L_BRACK => self.vector_or_range(),
            SEMI | COMMA | R_PAREN | R_BRACK | R_BRACE => {
                self.error("attesa un'espressione");
            }
            _ => self.error_bump("attesa un'espressione"),
        }
    }

    fn vector_or_range(&mut self) {
        let checkpoint = self.checkpoint();
        self.bump(); // '['
        if self.at(R_BRACK) {
            self.bump();
            self.start_at(checkpoint, VECTOR_EXPR);
            self.finish();
            return;
        }
        if self.at_list_generator() {
            self.list_element();
        } else {
            self.expr();
            if self.at(COLON) {
                self.bump();
                self.expr();
                if self.eat(COLON) {
                    self.expr();
                }
                self.expect(R_BRACK, "']'");
                self.start_at(checkpoint, RANGE_EXPR);
                self.finish();
                return;
            }
        }
        while self.eat(COMMA) {
            if self.at(R_BRACK) {
                break; // trailing comma
            }
            self.list_element();
        }
        self.expect(R_BRACK, "']'");
        self.start_at(checkpoint, VECTOR_EXPR);
        self.finish();
    }

    fn at_list_generator(&self) -> bool {
        match self.current() {
            FOR_KW | EACH_KW | IF_KW | LET_KW => true,
            L_PAREN => matches!(self.nth(1), FOR_KW | EACH_KW | IF_KW),
            _ => false,
        }
    }

    fn list_element(&mut self) {
        match self.current() {
            EACH_KW => {
                self.start(LC_EACH);
                self.bump();
                self.list_element();
                self.finish();
            }
            LET_KW => {
                self.start(LC_LET);
                self.bump();
                self.arg_list();
                self.list_element();
                self.finish();
            }
            IF_KW => {
                self.start(LC_IF);
                self.bump();
                self.expect(L_PAREN, "'('");
                self.expr();
                self.expect(R_PAREN, "')'");
                self.list_element();
                if self.eat(ELSE_KW) {
                    self.list_element();
                }
                self.finish();
            }
            FOR_KW => self.list_for(),
            L_PAREN if matches!(self.nth(1), FOR_KW | EACH_KW | IF_KW) => {
                self.start(LC_PAREN);
                self.bump();
                self.list_element();
                self.expect(R_PAREN, "')'");
                self.finish();
            }
            _ => self.expr(),
        }
    }

    /// `for (a = x, b = y) elem` or the C-style `for (init; cond; update) elem`.
    fn list_for(&mut self) {
        let checkpoint = self.checkpoint();
        self.bump(); // 'for'
        self.expect(L_PAREN, "'('");
        self.start(ARG_LIST);
        self.args_until(R_PAREN);
        self.finish();
        let kind = if self.eat(SEMI) {
            self.expr();
            self.expect(SEMI, "';'");
            self.start(ARG_LIST);
            self.args_until(R_PAREN);
            self.finish();
            LC_FOR_C
        } else {
            LC_FOR
        };
        self.expect(R_PAREN, "')'");
        self.list_element();
        self.start_at(checkpoint, kind);
        self.finish();
    }
}
