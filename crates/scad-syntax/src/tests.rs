use crate::ast::*;
use crate::parse;

const SAMPLE: &str = r#"
// Parametric bracket
include <lib/util.scad>
use <shapes.scad>

width = 60; // [20:120]
$fn = 64;

function sq(x) = x * x;
function f(a, b = 2) = let(c = a + b) c > 3 ? [for (i = [0:c]) if (i % 2 == 0) i else -i] : undef;
add = function (x, y) x + y;

module bracket(w = width, h = 10) {
    difference() {
        cube([w, w / 2, h], center = true);
        #translate([0, 0, -1]) cylinder(d = 5, h = h + 2);
    }
    for (x = [-1, 1]) translate([x * w / 3, 0, 0]) children();
}

!bracket() sphere(r = 2);
if (width > 50) cube(1); else { sphere(1); }
echo(str("w=", width), v = [1, 2, 3].x, m = [[1, 2], [3, 4]][1][0]);
xs = [each [1, 2], for (i = 0, j = 1; i < 3; i = i + 1, j = j * 2) [i, j], let (a = 3) a];
y = -2 ^ 2 + (1 < 2 && !false || 3 != 4) * 5 % 3;
"#;

#[test]
fn sample_parses_without_errors() {
    let parse = parse(SAMPLE);
    assert!(parse.errors().is_empty(), "{:#?}", parse.errors());
}

#[test]
fn tree_is_lossless() {
    for text in [
        SAMPLE,
        "cube(",
        "]]]",
        "module m( { }",
        "a = ;",
        "/* open",
        "x = \"open",
    ] {
        assert_eq!(parse(text).syntax().to_string(), text);
    }
}

#[test]
fn errors_are_reported_and_recovered() {
    let parse = parse("a = ;\ncube(1);\nb = 2 +;\nsphere(2);");
    assert_eq!(parse.errors().len(), 2, "{:#?}", parse.errors());
    let tree = parse.tree();
    let calls: Vec<_> = tree
        .statements
        .iter()
        .filter_map(|s| match &s.kind {
            StatementKind::Instantiation(Instantiation {
                kind: InstantiationKind::Call { name, .. },
                ..
            }) => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(calls, ["cube", "sphere"]);
}

fn single_expr(text: &str) -> ExprKind {
    let parse = parse(&format!("x = {text};"));
    assert!(parse.errors().is_empty(), "{text}: {:#?}", parse.errors());
    match parse.tree().statements.remove(0).kind {
        StatementKind::Assignment(a) => a.value.kind,
        other => panic!("not an assignment: {other:?}"),
    }
}

fn binary(kind: &ExprKind) -> (BinaryOp, &ExprKind, &ExprKind) {
    match kind {
        ExprKind::Binary { op, lhs, rhs } => (*op, &lhs.kind, &rhs.kind),
        other => panic!("not binary: {other:?}"),
    }
}

#[test]
fn precedence() {
    // 1 + 2 * 3 → 1 + (2 * 3)
    let e = single_expr("1 + 2 * 3");
    let (op, lhs, rhs) = binary(&e);
    assert_eq!(op, BinaryOp::Add);
    assert_eq!(*lhs, ExprKind::Number(1.0));
    assert_eq!(binary(rhs).0, BinaryOp::Mul);

    // -2 ^ 2 → -(2 ^ 2)
    match single_expr("-2 ^ 2") {
        ExprKind::Unary { op, operand } => {
            assert_eq!(op, UnaryOp::Neg);
            assert_eq!(binary(&operand.kind).0, BinaryOp::Pow);
        }
        other => panic!("{other:?}"),
    }

    // 2 ^ 3 ^ 2 → 2 ^ (3 ^ 2)
    let e = single_expr("2 ^ 3 ^ 2");
    let (_, lhs, rhs) = binary(&e);
    assert_eq!(*lhs, ExprKind::Number(2.0));
    assert_eq!(binary(rhs).0, BinaryOp::Pow);

    // a - b - c → (a - b) - c
    let e = single_expr("a - b - c");
    let (_, lhs, rhs) = binary(&e);
    assert_eq!(binary(lhs).0, BinaryOp::Sub);
    assert_eq!(*rhs, ExprKind::Variable("c".into()));

    // a || b && c → a || (b && c)
    let e = single_expr("a || b && c");
    assert_eq!(binary(&e).0, BinaryOp::Or);
    assert_eq!(binary(binary(&e).2).0, BinaryOp::And);
}

#[test]
fn ternary_is_right_nested() {
    match single_expr("a ? b : c ? d : e") {
        ExprKind::Ternary { else_expr, .. } => {
            assert!(matches!(else_expr.kind, ExprKind::Ternary { .. }));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn ranges_and_vectors() {
    assert!(matches!(
        single_expr("[0:10]"),
        ExprKind::Range { step: None, .. }
    ));
    assert!(matches!(
        single_expr("[0:2:10]"),
        ExprKind::Range { step: Some(_), .. }
    ));
    match single_expr("[1, 2, 3,]") {
        ExprKind::List(items) => assert_eq!(items.len(), 3),
        other => panic!("{other:?}"),
    }
    match single_expr("[a ? 1 : 2, 3]") {
        ExprKind::List(items) => assert_eq!(items.len(), 2),
        other => panic!("{other:?}"),
    }
    assert_eq!(single_expr("[]"), ExprKind::List(vec![]));
}

#[test]
fn comprehensions() {
    let ExprKind::List(items) = single_expr("[for (i = [0:3]) if (i > 1) i * 2 else 0]") else {
        panic!()
    };
    let ListElementKind::For { assignments, body } = &items[0].kind else {
        panic!("{:?}", items[0]);
    };
    assert_eq!(assignments[0].name.as_deref(), Some("i"));
    assert!(matches!(
        body.kind,
        ListElementKind::If {
            else_element: Some(_),
            ..
        }
    ));

    let ExprKind::List(items) = single_expr("[(for (i = x) i), each y]") else {
        panic!()
    };
    assert!(matches!(items[0].kind, ListElementKind::For { .. }));
    assert!(matches!(items[1].kind, ListElementKind::Each(_)));

    let ExprKind::List(items) = single_expr("[for (i = 0; i < 3; i = i + 1) i]") else {
        panic!()
    };
    assert!(matches!(items[0].kind, ListElementKind::ForC { .. }));
}

#[test]
fn postfix_chains() {
    match single_expr("f(1)(2)[0].x") {
        ExprKind::Member { target, member } => {
            assert_eq!(member, "x");
            assert!(matches!(target.kind, ExprKind::Index { .. }));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn modifiers_and_children() {
    let tree = parse("#!translate([1, 0, 0]) { cube(1); a = 2; sphere(a); }").tree();
    let StatementKind::Instantiation(inst) = &tree.statements[0].kind else {
        panic!()
    };
    assert!(inst.modifiers.highlight && inst.modifiers.root);
    let InstantiationKind::Call { name, children, .. } = &inst.kind else {
        panic!()
    };
    assert_eq!(name, "translate");
    assert_eq!(children.len(), 3);
}

#[test]
fn if_else_branches() {
    let tree = parse("if (a) ; else cube();").tree();
    let StatementKind::Instantiation(Instantiation {
        kind:
            InstantiationKind::If {
                then_branch,
                else_branch,
                ..
            },
        ..
    }) = &tree.statements[0].kind
    else {
        panic!()
    };
    assert!(then_branch.is_empty());
    assert_eq!(else_branch.as_ref().map(Vec::len), Some(1));
}

#[test]
fn definitions_and_includes() {
    let tree = parse(SAMPLE).tree();
    let mut modules = 0;
    let mut functions = 0;
    let mut paths = Vec::new();
    for s in &tree.statements {
        match &s.kind {
            StatementKind::ModuleDef(m) => {
                modules += 1;
                assert_eq!(m.params.len(), 2);
                assert_eq!(m.body.len(), 2);
            }
            StatementKind::FunctionDef(_) => functions += 1,
            StatementKind::Include(p) | StatementKind::Use(p) => paths.push(p.clone()),
            _ => {}
        }
    }
    assert_eq!((modules, functions), (1, 2));
    assert_eq!(paths, ["lib/util.scad", "shapes.scad"]);
}

#[test]
fn keyword_modules() {
    let tree = parse("for (i = [1:3]) let (r = i) echo(r) assert(r > 0) cube(r);").tree();
    let StatementKind::Instantiation(Instantiation {
        kind: InstantiationKind::Call { name, children, .. },
        ..
    }) = &tree.statements[0].kind
    else {
        panic!()
    };
    assert_eq!(name, "for");
    assert_eq!(children.len(), 1);
}

#[test]
fn lexer_warnings_and_overflow_losslessness() {
    use crate::{SyntaxWarningKind, UnterminatedKind};

    let src = "2b = 9007199254740993;\necho(1e400 + 1, \"\\q\\q\\q\\q\\q\\q\", 0XFF);";
    let p = parse(src);
    assert_eq!(p.syntax().to_string(), src);
    assert!(p.errors().is_empty());
    let kinds: Vec<_> = p.warnings().iter().map(|w| &w.kind).collect();
    assert_eq!(
        kinds,
        vec![
            &SyntaxWarningKind::DigitIdentifier("2b".into()),
            &SyntaxWarningKind::ImpreciseInteger("9007199254740993".into()),
            &SyntaxWarningKind::UndefinedEscape,
            &SyntaxWarningKind::UndefinedEscape,
            &SyntaxWarningKind::UndefinedEscape,
            &SyntaxWarningKind::UndefinedEscape,
            &SyntaxWarningKind::UndefinedEscape,
            &SyntaxWarningKind::DigitIdentifier("0XFF".into()),
        ]
    );

    let unterm = parse("echo(\"abc);");
    assert_eq!(unterm.syntax().to_string(), "echo(\"abc);");
    assert_eq!(
        unterm.errors().first().and_then(|e| e.unterminated),
        Some(UnterminatedKind::String)
    );
}
