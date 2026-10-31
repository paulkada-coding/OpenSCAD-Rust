use std::path::Path;

use geometry::csg::{Node, NodeKind};

use crate::{EvalOptions, Evaluator, Level};

fn run(source: &str) -> (Node, Vec<String>) {
    let mut ev = Evaluator::new(EvalOptions::default());
    let (nodes, error) = ev.run(Path::new("test.scad"), source);
    if let Some(e) = error {
        panic!("evaluation failed: {e}");
    }
    let node = Node::group("", nodes);
    let lines = ev
        .messages()
        .iter()
        .map(|m| format!("{}: {}", m.level.label(), m.text))
        .collect();
    (node, lines)
}

fn echoes(source: &str) -> Vec<String> {
    run(source)
        .1
        .into_iter()
        .filter_map(|l| l.strip_prefix("ECHO: ").map(str::to_string))
        .collect()
}

fn echo1(expr: &str) -> String {
    echoes(&format!("echo({expr});")).remove(0)
}

#[test]
fn arithmetic_and_formatting() {
    assert_eq!(echo1("1 + 2 * 3"), "7");
    assert_eq!(echo1("-2 ^ 2"), "-4");
    assert_eq!(echo1("7 % 3, 1 / 4"), "1, 0.25");
    assert_eq!(echo1("[1, 2] + [3, 4]"), "[4, 6]");
    assert_eq!(
        echo1("\"a\", [\"b\"], true, undef"),
        "\"a\", [\"b\"], true, undef"
    );
    assert_eq!(echo1("x = 1"), "x = 1");
    assert_eq!(echo1("str(\"a\", 1, [2])"), "\"a1[2]\"");
}

#[test]
fn assignments_are_hoisted_and_last_wins() {
    let out = echoes("echo(a); a = 1; b = a + 1; a = 5; echo(b);");
    assert_eq!(out, ["5", "6"]);
}

#[test]
fn functions_and_recursion() {
    let out = echoes(
        "function fact(n) = n <= 1 ? 1 : n * fact(n - 1);
         function sum(n, acc = 0) = n == 0 ? acc : sum(n - 1, acc + n);
         echo(fact(10), sum(200000));",
    );
    assert_eq!(out, ["3.6288e+6, 2.00001e+10"]);
}

#[test]
fn function_literals_and_closures() {
    let out = echoes(
        "add = function(a) function(b) a + b;
         add3 = add(3);
         echo(add3(4), (function(x) x * 2)(5));",
    );
    assert_eq!(out, ["7, 10"]);
}

#[test]
fn list_comprehensions() {
    assert_eq!(echo1("[for (i = [0:4]) if (i % 2 == 0) i]"), "[0, 2, 4]");
    assert_eq!(
        echo1("[for (i = [1:2], j = [1:2]) i * 10 + j]"),
        "[11, 12, 21, 22]"
    );
    assert_eq!(echo1("[each [1, 2], each [3:4], 5]"), "[1, 2, 3, 4, 5]");
    assert_eq!(
        echo1("[for (i = 0, j = 1; i < 4; i = i + 1, j = j * 2) j]"),
        "[1, 2, 4, 8]"
    );
    assert_eq!(echo1("[let (a = 2, b = a * 3) b]"), "[6]");
    assert_eq!(echo1("[for (c = \"ab\") c]"), "[\"a\", \"b\"]");
}

#[test]
fn let_and_ternary_scopes() {
    assert_eq!(echo1("let (a = 1, b = a + 1) [a, b]"), "[1, 2]");
    assert_eq!(echo1("true ? 1 : 2, false ? 1 : 2"), "1, 2");
}

#[test]
fn special_variables_are_dynamic() {
    let out = echoes(
        "module show() echo($fn);
         module outer() { $fn = 7; children(); }
         outer() show();
         show($fn = 3);
         show();",
    );
    assert_eq!(out, ["7", "3", "0"]);
}

#[test]
fn modules_and_children() {
    let (root, _) = run(
        "module pair() { children(0); translate([5, 0, 0]) children(1); }
         pair() { cube(1); sphere(2); }",
    );
    let pair = &root.children[0];
    assert!(matches!(&pair.kind, NodeKind::Group { name } if name == "pair"));
    assert_eq!(pair.children.len(), 2);
}

#[test]
fn children_count() {
    let out = echoes("module m() echo($children); m() { cube(); sphere(); } m();");
    assert_eq!(out, ["2", "0"]);
}

#[test]
fn for_module_and_if() {
    let (root, _) =
        run("for (i = [0:2]) translate([i, 0, 0]) cube(1); if (false) cube(); else sphere();");
    assert_eq!(root.children[0].children.len(), 3);
    let if_node = &root.children[1];
    assert!(matches!(if_node.children[0].kind, NodeKind::Sphere { .. }));
}

#[test]
fn primitives_resolve_parameters() {
    let (root, _) = run("cylinder(h = 10, d = 4, center = true, $fn = 12); cube([1, 2, 3]);");
    match &root.children[0].kind {
        NodeKind::Cylinder {
            h,
            r1,
            r2,
            center,
            tessellation,
        } => {
            assert_eq!((*h, *r1, *r2, *center), (10.0, 2.0, 2.0, true));
            assert_eq!(tessellation.fn_, 12.0);
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(root.children[1].kind, NodeKind::Cube { .. }));
}

#[test]
fn unknown_names_warn() {
    let (_, lines) = run("echo(nope); foo(); x = bar(1);");
    assert!(
        lines
            .iter()
            .any(|l| l.contains("unknown variable \"nope\""))
    );
    assert!(lines.iter().any(|l| l.contains("unknown module 'foo'")));
    assert!(lines.iter().any(|l| l.contains("unknown function 'bar'")));
}

#[test]
fn assert_failure_stops() {
    let mut ev = Evaluator::new(EvalOptions::default());
    let (_, err) = ev.run(Path::new("t.scad"), "x = 1;\nassert(x > 2, \"too small\");");
    let err = err.expect("assert must fail");
    assert!(
        err.message.contains("Assertion '(x > 2)' failed"),
        "{}",
        err.message
    );
    assert!(ev.messages().iter().all(|m| m.level != Level::Error));
}

#[test]
fn deep_recursion_does_not_overflow() {
    // Not a tail call: exercises stack growth.
    let out = echoes("function count(n) = n == 0 ? 0 : 1 + count(n - 1); echo(count(9000));");
    assert_eq!(out, ["9000"]);
}

#[test]
fn modifiers_and_disable() {
    let (root, _) = run("*cube(); #sphere(); %cylinder();");
    assert_eq!(root.children.len(), 2);
    assert!(root.children[0].modifiers.highlight);
    assert!(root.children[1].modifiers.background);
}

#[test]
fn source_mapping_tracks_leaf_and_root_call() {
    let src = "module part(r) {\n    sphere(r);\n}\npart(5);\ncube(2);\n";
    let mut ev = Evaluator::new(EvalOptions::default());
    let (nodes, err) = ev.run(Path::new("main.scad"), src);
    assert!(err.is_none());
    assert_eq!(nodes.len(), 2);

    let part_group = &nodes[0];
    let sphere_leaf = &part_group.children[0];
    assert!(matches!(sphere_leaf.kind, NodeKind::Sphere { .. }));

    let leaf_src = ev
        .resolve_source(sphere_leaf.source.expect("leaf source"))
        .expect("resolved leaf");
    assert_eq!(leaf_src.start_line_col, (2, 5));
    assert_eq!(leaf_src.snippet, "sphere(r);");

    let root_src = ev
        .resolve_source(sphere_leaf.root_call.expect("root call"))
        .expect("resolved root");
    assert_eq!(root_src.start_line_col, (4, 1));
    assert_eq!(root_src.snippet, "part(5);");

    let root = Node::group("", nodes);
    let cursor_on_sphere = src.find("sphere(r)").unwrap() as u32;
    let hit = root
        .find_at_offset(0, cursor_on_sphere)
        .expect("node at cursor");
    assert!(matches!(hit.kind, NodeKind::Sphere { .. }));
}

#[test]
fn defines_override_before_dependent_assignments_without_warning() {
    let src = "a = 3;\nb = a * 10;\necho(a, b);\n";
    let mut ev = Evaluator::new(EvalOptions {
        defines: vec![("a".into(), "7".into()), ("extra".into(), "[1, 2]".into())],
        ..EvalOptions::default()
    });
    let (_, err) = ev.run(Path::new("defs.scad"), src);
    assert!(err.is_none());
    let msgs: Vec<String> = ev
        .messages()
        .iter()
        .map(|m| format!("{}: {}", m.level.label(), m.text))
        .collect();
    assert_eq!(msgs, ["ECHO: 7, 70"]);
}

#[test]
fn source_cache_reuses_parsed_library_and_invalidates_on_change() {
    let dir = std::env::temp_dir().join(format!("scadr_cache_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let lib_path = dir.join("mylib.scad");
    std::fs::write(&lib_path, "function answer() = 42;\n").unwrap();

    let cache = crate::SourceCache::new();
    let opts = EvalOptions {
        library_paths: vec![dir.clone()],
        cache: Some(cache.clone()),
        ..EvalOptions::default()
    };

    let main_src = "use <mylib.scad>\necho(answer());\n";
    let mut ev1 = Evaluator::new(opts.clone());
    let (_, err1) = ev1.run(Path::new("main.scad"), main_src);
    assert!(err1.is_none());
    assert_eq!(ev1.messages()[0].text, "42");
    assert_eq!(cache.len(), 1);

    // Second run hits the cache cleanly.
    let mut ev2 = Evaluator::new(opts.clone());
    let (_, err2) = ev2.run(Path::new("main.scad"), main_src);
    assert!(err2.is_none());
    assert_eq!(ev2.messages()[0].text, "42");

    // Modifying the file on disk invalidates the cached AST automatically.
    std::fs::write(&lib_path, "function answer() = 999;\n").unwrap();
    let mut ev3 = Evaluator::new(opts);
    let (_, err3) = ev3.run(Path::new("main.scad"), main_src);
    assert!(err3.is_none());
    assert_eq!(ev3.messages()[0].text, "999");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fuzz_live_editing_and_mutated_programs_never_panic() {
    let program = r##"
obj = object(a = 1, [["b", [10, 20, 30, 40]]]);
v = obj.b.zyx;
f = function(n) n <= 0 ? 1 : f(n - 1);
module shape(s = 2) {
    color("#ff00aa80")
    translate([1, 2, 3])
    rotate([90, 0, 180])
    scale([-0, 1, 2])
    mirror([1, 1, 0])
        linear_extrude(height = s)
            offset(r = 0.5)
                text("Hi", size = 6, halign = "center");
}
echo(f(4), v, has_key(obj, "a"), textmetrics("a"), fontmetrics(10));
shape();
"##;
    let opts = EvalOptions {
        object_function: true,
        vector_swizzle: true,
        import_function: true,
        ..EvalOptions::default()
    };

    for idx in 0..=program.len() {
        if !program.is_char_boundary(idx) {
            continue;
        }
        let prefix = &program[..idx];
        let mut ev = Evaluator::new(opts.clone());
        let _ = ev.run(Path::new("live.scad"), prefix);
    }

    const SNIPPETS: &[&str] = &[
        "echo(1/0, 0/0, -1/0, (1/0) % 2, pow(-2, 0.5));",
        "echo([1, 2].rgba, [1].w, object(123), has_key(1, 2));",
        "echo(search([1], \"abc\", 0, 99), rands(10, -10, -5, 1e50));",
        "echo(textmetrics(123, size = -5, direction = \"ttb\"), fontmetrics(-1, 99));",
        "color(\"#12345\") translate(\"bad\") rotate([1, \"x\"]) scale([]) mirror([0,0,0]) cube(\"x\");",
        "polygon(points = [[1], \"a\"], paths = [[-1, 99]]); polyhedron(points = 1, faces = [[0]]);",
    ];
    for &snippet in SNIPPETS {
        let mut ev = Evaluator::new(opts.clone());
        let _ = ev.run(Path::new("edge.scad"), snippet);
    }
}
