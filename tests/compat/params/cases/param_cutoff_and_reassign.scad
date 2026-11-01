// { inside line comment does not stop extraction
/* { inside block comment does not stop extraction } */
dup = 1; // [0:10]
keep = 2; // [0:5]
removed = 3; // [0:10]

function helper(x) = x * 2;
echo("header echo", keep);
cube(1);

dup = 4; // [0:2:8]
removed = helper(2);

module part() {
    after_brace_local = 99;
    sphere(1);
}

after_brace_top = 100; // [0:200]
