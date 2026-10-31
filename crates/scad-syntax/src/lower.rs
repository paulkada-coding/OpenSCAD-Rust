//! Conversion from the lossless syntax tree to the owned AST.
//!
//! Lowering is total: malformed parts (already reported as syntax errors)
//! become `undef` expressions or are skipped.

use std::rc::Rc;

use crate::SyntaxKind::{self, *};
use crate::ast::*;
use crate::{SyntaxElement, SyntaxNode, SyntaxToken};

pub(crate) fn lower_file(root: &SyntaxNode) -> SourceFile {
    let mut statements = Vec::new();
    for node in root.children() {
        lower_statement(&node, &mut statements);
    }
    SourceFile { statements }
}

// ---------------------------------------------------------------- helpers

fn significant(node: &SyntaxNode) -> impl Iterator<Item = SyntaxElement> + use<> {
    node.children_with_tokens()
        .filter(|e| e.as_token().is_none_or(|t| !t.kind().is_trivia()))
}

fn token(node: &SyntaxNode, kinds: &[SyntaxKind]) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(SyntaxElement::into_token)
        .find(|t| kinds.contains(&t.kind()))
}

fn child(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    node.children().find(|n| n.kind() == kind)
}

fn is_expr(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        LITERAL
            | NAME_REF
            | PAREN_EXPR
            | UNARY_EXPR
            | BINARY_EXPR
            | TERNARY_EXPR
            | CALL_EXPR
            | INDEX_EXPR
            | MEMBER_EXPR
            | VECTOR_EXPR
            | RANGE_EXPR
            | LET_EXPR
            | ASSERT_EXPR
            | ECHO_EXPR
            | FUNCTION_LIT
            | ERROR
    )
}

fn expr_children(node: &SyntaxNode) -> Vec<SyntaxNode> {
    node.children().filter(|n| is_expr(n.kind())).collect()
}

fn undef(node: &SyntaxNode) -> Expr {
    Expr {
        kind: ExprKind::Undef,
        span: node.text_range(),
    }
}

fn lower_opt_expr(node: Option<&SyntaxNode>, parent: &SyntaxNode) -> Expr {
    node.map_or_else(|| undef(parent), lower_expr)
}

// ------------------------------------------------------------- statements

fn lower_statement(node: &SyntaxNode, out: &mut Vec<Statement>) {
    let span = node.text_range();
    let kind = match node.kind() {
        // A bare block only groups statements: they belong to the
        // enclosing scope.
        BLOCK => {
            for child in node.children() {
                lower_statement(&child, out);
            }
            return;
        }
        ASSIGNMENT => {
            let Some(name) = token(node, &[IDENT]) else {
                return;
            };
            let value = lower_opt_expr(expr_children(node).first(), node);
            StatementKind::Assignment(Assignment {
                name: name.text().into(),
                value,
            })
        }
        MODULE_DEF => {
            let Some(name) = token(node, &[IDENT]) else {
                return;
            };
            let params = child(node, PARAM_LIST)
                .map(|p| lower_params(&p))
                .unwrap_or_default();
            let body = node
                .children()
                .skip_while(|n| n.kind() != PARAM_LIST)
                .nth(1)
                .map(|n| child_statements(&n))
                .unwrap_or_default();
            StatementKind::ModuleDef(Rc::new(ModuleDef {
                name: name.text().into(),
                params,
                body,
                span,
            }))
        }
        FUNCTION_DEF => {
            let Some(name) = token(node, &[IDENT]) else {
                return;
            };
            let params = child(node, PARAM_LIST)
                .map(|p| lower_params(&p))
                .unwrap_or_default();
            let body = lower_opt_expr(expr_children(node).first(), node);
            StatementKind::FunctionDef(Rc::new(FunctionDef {
                name: name.text().into(),
                params,
                body,
                span,
            }))
        }
        INCLUDE_STMT | USE_STMT => {
            let Some(token) = token(node, &[INCLUDE, USE]) else {
                return;
            };
            let text = token.text();
            let path = text
                .find('<')
                .and_then(|start| text[start + 1..].strip_suffix('>'))
                .unwrap_or_default()
                .to_string();
            if node.kind() == INCLUDE_STMT {
                StatementKind::Include(path)
            } else {
                StatementKind::Use(path)
            }
        }
        MODULE_CALL | IF_STMT => match lower_instantiation(node) {
            Some(inst) => StatementKind::Instantiation(inst),
            None => return,
        },
        _ => return, // EMPTY_STMT, ERROR
    };
    out.push(Statement { kind, span });
}

/// Statements of a child position: a block, a single statement or `;`.
fn child_statements(node: &SyntaxNode) -> Vec<Statement> {
    let mut out = Vec::new();
    lower_statement(node, &mut out);
    out
}

fn lower_instantiation(node: &SyntaxNode) -> Option<Instantiation> {
    let mut modifiers = Modifiers::default();
    let mut elements = significant(node).peekable();
    while let Some(SyntaxElement::Token(t)) = elements.peek() {
        match t.kind() {
            BANG => modifiers.root = true,
            HASH => modifiers.highlight = true,
            PERCENT => modifiers.background = true,
            STAR => modifiers.disable = true,
            _ => break,
        }
        elements.next();
    }

    let kind = if node.kind() == IF_STMT {
        let mut condition = None;
        let mut then_branch = Vec::new();
        let mut else_branch = None;
        let mut after_else = false;
        for element in elements {
            match element {
                SyntaxElement::Token(t) if t.kind() == ELSE_KW => after_else = true,
                SyntaxElement::Token(t) if t.kind() == SEMI && after_else => {
                    else_branch = Some(Vec::new());
                }
                SyntaxElement::Node(n) if condition.is_none() && is_expr(n.kind()) => {
                    condition = Some(lower_expr(&n));
                }
                SyntaxElement::Node(n) if after_else => else_branch = Some(child_statements(&n)),
                SyntaxElement::Node(n) => then_branch = child_statements(&n),
                SyntaxElement::Token(_) => {}
            }
        }
        if after_else && else_branch.is_none() {
            else_branch = Some(Vec::new());
        }
        InstantiationKind::If {
            condition: condition.unwrap_or_else(|| undef(node)),
            then_branch,
            else_branch,
        }
    } else {
        let name = token(node, &[IDENT, FOR_KW, LET_KW, ECHO_KW, ASSERT_KW])?;
        let args = child(node, ARG_LIST)
            .map(|a| lower_args(&a))
            .unwrap_or_default();
        let children = node
            .children()
            .find(|n| matches!(n.kind(), BLOCK | MODULE_CALL | IF_STMT))
            .map(|n| child_statements(&n))
            .unwrap_or_default();
        InstantiationKind::Call {
            name: name.text().into(),
            name_span: name.text_range(),
            args,
            children,
        }
    };
    Some(Instantiation { modifiers, kind })
}

fn lower_params(node: &SyntaxNode) -> Vec<Param> {
    node.children()
        .filter(|n| n.kind() == PARAM)
        .filter_map(|p| {
            let name = token(&p, &[IDENT])?;
            Some(Param {
                name: name.text().into(),
                default: expr_children(&p).first().map(lower_expr),
                span: p.text_range(),
            })
        })
        .collect()
}

fn lower_args(node: &SyntaxNode) -> Vec<Argument> {
    node.children()
        .filter(|n| n.kind() == ARG)
        .map(|a| {
            // `name = value`: the name is a direct IDENT token before '='.
            let name = token(&a, &[EQ])
                .and_then(|_| token(&a, &[IDENT]))
                .map(|t| Name::from(t.text()));
            Argument {
                name,
                value: lower_opt_expr(expr_children(&a).first(), &a),
                span: a.text_range(),
            }
        })
        .collect()
}

// ------------------------------------------------------------ expressions

fn lower_expr(node: &SyntaxNode) -> Expr {
    let span = node.text_range();
    let exprs = expr_children(node);
    let sub = |i: usize| Box::new(lower_opt_expr(exprs.get(i), node));

    let kind = match node.kind() {
        LITERAL => match token(node, &[NUMBER, STRING, TRUE_KW, FALSE_KW, UNDEF_KW]) {
            Some(t) => match t.kind() {
                NUMBER => ExprKind::Number(parse_number(t.text())),
                STRING => ExprKind::String(unescape(t.text()).into()),
                TRUE_KW => ExprKind::Bool(true),
                FALSE_KW => ExprKind::Bool(false),
                _ => ExprKind::Undef,
            },
            None => ExprKind::Undef,
        },
        NAME_REF => match token(node, &[IDENT]) {
            Some(t) => ExprKind::Variable(t.text().into()),
            None => ExprKind::Undef,
        },
        PAREN_EXPR => return lower_opt_expr(exprs.first(), node),
        UNARY_EXPR => {
            let op = match token(node, &[MINUS, PLUS, BANG, TILDE]).map(|t| t.kind()) {
                Some(MINUS) => UnaryOp::Neg,
                Some(PLUS) => UnaryOp::Plus,
                Some(TILDE) => UnaryOp::BitNot,
                _ => UnaryOp::Not,
            };
            ExprKind::Unary {
                op,
                operand: sub(0),
            }
        }
        BINARY_EXPR => {
            let op = token(
                node,
                &[
                    PLUS, MINUS, STAR, SLASH, PERCENT, CARET, LT, LTEQ, GT, GTEQ, EQ2, NEQ, AMP2,
                    PIPE2, AMP, PIPE, SHL, SHR,
                ],
            )
            .map_or(BinaryOp::Add, |t| binary_op(t.kind()));
            ExprKind::Binary {
                op,
                lhs: sub(0),
                rhs: sub(1),
            }
        }
        TERNARY_EXPR => ExprKind::Ternary {
            condition: sub(0),
            then_expr: sub(1),
            else_expr: sub(2),
        },
        CALL_EXPR => ExprKind::Call {
            callee: sub(0),
            args: child(node, ARG_LIST)
                .map(|a| lower_args(&a))
                .unwrap_or_default(),
        },
        INDEX_EXPR => ExprKind::Index {
            target: sub(0),
            index: sub(1),
        },
        MEMBER_EXPR => match token(node, &[IDENT]) {
            Some(member) => ExprKind::Member {
                target: sub(0),
                member: member.text().into(),
            },
            None => ExprKind::Undef,
        },
        RANGE_EXPR => {
            if exprs.len() >= 3 {
                ExprKind::Range {
                    start: sub(0),
                    step: Some(sub(1)),
                    end: sub(2),
                }
            } else {
                ExprKind::Range {
                    start: sub(0),
                    step: None,
                    end: sub(1),
                }
            }
        }
        VECTOR_EXPR => ExprKind::List(
            node.children()
                .filter(|n| is_expr(n.kind()) || is_generator(n.kind()))
                .map(|n| lower_list_element(&n))
                .collect(),
        ),
        LET_EXPR => ExprKind::Let {
            assignments: child(node, ARG_LIST)
                .map(|a| lower_args(&a))
                .unwrap_or_default(),
            body: sub(0),
        },
        ASSERT_EXPR | ECHO_EXPR => {
            let args = child(node, ARG_LIST)
                .map(|a| lower_args(&a))
                .unwrap_or_default();
            let body = exprs.first().map(|n| Box::new(lower_expr(n)));
            if node.kind() == ASSERT_EXPR {
                ExprKind::Assert { args, body }
            } else {
                ExprKind::Echo { args, body }
            }
        }
        FUNCTION_LIT => ExprKind::FunctionLiteral(Rc::new(FunctionLiteral {
            params: child(node, PARAM_LIST)
                .map(|p| lower_params(&p))
                .unwrap_or_default(),
            body: lower_opt_expr(exprs.first(), node),
        })),
        _ => ExprKind::Undef,
    };
    Expr { kind, span }
}

fn binary_op(kind: SyntaxKind) -> BinaryOp {
    match kind {
        PLUS => BinaryOp::Add,
        MINUS => BinaryOp::Sub,
        STAR => BinaryOp::Mul,
        SLASH => BinaryOp::Div,
        PERCENT => BinaryOp::Mod,
        CARET => BinaryOp::Pow,
        LT => BinaryOp::Lt,
        LTEQ => BinaryOp::Le,
        GT => BinaryOp::Gt,
        GTEQ => BinaryOp::Ge,
        EQ2 => BinaryOp::Eq,
        NEQ => BinaryOp::Ne,
        AMP2 => BinaryOp::And,
        AMP => BinaryOp::BitAnd,
        PIPE => BinaryOp::BitOr,
        SHL => BinaryOp::Shl,
        SHR => BinaryOp::Shr,
        _ => BinaryOp::Or,
    }
}

fn is_generator(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        LC_EACH | LC_FOR | LC_FOR_C | LC_IF | LC_LET | LC_PAREN
    )
}

fn lower_list_element(node: &SyntaxNode) -> ListElement {
    let span = node.text_range();
    let elements: Vec<SyntaxNode> = node
        .children()
        .filter(|n| is_expr(n.kind()) || is_generator(n.kind()))
        .collect();
    let element = |i: usize| -> Box<ListElement> {
        Box::new(match elements.get(i) {
            Some(n) => lower_list_element(n),
            None => ListElement {
                kind: ListElementKind::Expr(undef(node)),
                span,
            },
        })
    };
    let arg_lists: Vec<SyntaxNode> = node.children().filter(|n| n.kind() == ARG_LIST).collect();
    let args = |i: usize| arg_lists.get(i).map(lower_args).unwrap_or_default();

    let kind = match node.kind() {
        LC_EACH => ListElementKind::Each(element(0)),
        LC_LET => ListElementKind::Let {
            assignments: args(0),
            body: element(0),
        },
        LC_IF => ListElementKind::If {
            condition: lower_opt_expr(elements.first(), node),
            then_element: element(1),
            else_element: (elements.len() > 2).then(|| element(2)),
        },
        LC_FOR => ListElementKind::For {
            assignments: args(0),
            body: element(0),
        },
        LC_FOR_C => ListElementKind::ForC {
            init: args(0),
            condition: lower_opt_expr(elements.first(), node),
            update: args(1),
            body: element(1),
        },
        LC_PAREN => return *element(0),
        _ => ListElementKind::Expr(lower_expr(node)),
    };
    ListElement { kind, span }
}

fn parse_number(text: &str) -> f64 {
    match text.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).map_or_else(
            |_| {
                hex.bytes().fold(0.0_f64, |acc, b| {
                    let digit = match b {
                        b'0'..=b'9' => b - b'0',
                        b'a'..=b'f' => b - b'a' + 10,
                        b'A'..=b'F' => b - b'A' + 10,
                        _ => 0,
                    };
                    acc * 16.0 + f64::from(digit)
                })
            },
            |n| n as f64,
        ),
        None => text.parse().unwrap_or(f64::NAN),
    }
}

/// Decodes a quoted string literal.
fn unescape(quoted: &str) -> String {
    crate::lexer::decode_string_literal(quoted, 0, |_| {})
}

#[cfg(test)]
mod tests {
    use super::unescape;

    #[test]
    fn escapes() {
        assert_eq!(unescape(r#""a\nb""#), "a\nb");
        assert_eq!(unescape(r#""\"q\" \\""#), "\"q\" \\");
        assert_eq!(unescape(r#""\x41€""#), "A€");
        assert_eq!(unescape(r#""\x00|\u0000|\U000000""#), " | | ");
        assert_eq!(unescape(r#""\q|\x80|\u12""#), "q|x80|u12");
    }
}
