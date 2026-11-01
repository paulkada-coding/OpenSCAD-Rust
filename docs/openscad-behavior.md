# Observed Behavior of OpenSCAD Nightly 2026.10.22

Clean-room specification for implementing a byte-compatible interpreter and geometry exporter.
Everything below was **observed by running the binary**
`C:\Program Files\OpenSCAD (Nightly)\openscad.exe` (version `2026.10.22`, Windows 11) with
`openscad -o out.csg file.scad` and capturing stderr; no OpenSCAD source code was read.
Many examples are also cases in `tests/compat/cases` (case name given in parentheses).

Conventions: `input` → `exact output`. Stderr lines are shown after corpus normalization
(CRLF → LF, absolute path of the cases directory stripped; see `tests/compat/README.md`).

---

## 0. Command-Line Execution

- `openscad -o x.csg file.scad`: evaluates the file, writes the CSG tree, prints messages to **stderr**.
  Writes nothing to stdout. No render statistics lines are printed for `.csg`, `.echo`, or `.ast` exports.
- `-o x.echo`: messages (ECHO/WARNING/ERROR…) go **into the file** instead of stderr. The content is
  identical to the lines sent to stderr with `.csg`. Parse errors also go into the file.
- `-o x.stl` (full render) appends statistics lines to stderr, which must be filtered out when comparing
  logs (geometry export, messages, and exit codes in detail: §16):
  ```
  Geometries in cache: 1
  Geometry cache size in bytes: 856
  CGAL Polyhedrons in cache: 0
  CGAL cache size in bytes: 0
  Total rendering time: 0:00:00.000
  Top level object is a 3D object (PolySet):
     Convex:       yes
     Facets:         6
  ```
- On Windows both stderr and written files use **CRLF**. A `\n` inside an echoed string also becomes CRLF.
- Font functions print `Fontconfig error: Cannot load default config file: File not found`
  (twice, on first font initialization: `textmetrics`, `fontmetrics`; not `text()` in CSG export).
- `$preview` is `true` for `.csg`, `.echo`, `.ast` exports; `false` for `.stl` export.
- **Exit code**: `1` on parse error (no output file written); `0` in all other cases,
  **even after an evaluation `ERROR:`** (failed assert, recursion).
- Experimental features (`--enable=<name>` or `--enable=all`): `roof`, `input-driver-dbus`, `lazy-union`,
  `vertex-object-renderers-indexing`, `import-function`, `object-function`, `predictible-output`,
  `vector-swizzle`, `discretization-by-error`, `ai-features`, `unicode-identifiers`.

## 1. Message Format (stderr)

### 1.1 Prefixes and Locations

| Type | Format |
|---|---|
| echo | `ECHO: <arguments>` |
| warning | `WARNING: <text> in file <file>, line <n>` |
| error | `ERROR: <text> in file <file>, line <n>` |
| trace | `TRACE: called by '<name>' in file <file>, line <n>` / `TRACE: call of '<signature>' in file <file>, line <n>` |
| deprecation | `DEPRECATED: <text> in file <file>, line <n>` |
| parse error | `ERROR: Parser error: <text> in file <file>, line <n>` followed by `Can't parse file '<absolute path>'!` |

- `<file>` is the path **relative to the main file's directory**, using `/` as separator
  (`in file lib/sub/subdefs.scad, line 3`), regardless of the current working directory. Only
  `Can't parse file '...'!` uses the absolute path with `\`.
- Lines are 1-based. The line is that of the expression/statement triggering the message
  (for module arguments: the instantiation line; for warnings inside a function: the sub-expression line).
- Some messages have an unprefixed **second line**. Invalid color:
  ```
  WARNING: Unable to parse color "nosuchcolor" in file csg_color_invalid.scad, line 1
  For a list of valid color names, see the <a href="open-window://colorlist"><b>Color List</b></a> window.
  ```
- There is no deduplication: the same warning inside a loop is printed on every iteration.
- Warnings have no TRACE lines; only evaluation `ERROR:` messages are followed by TRACE lines.

### 1.2 Message Ordering

1. **First of all** (parse phase, across the entire file and includes, even inside never-called modules):
   - `WARNING: Undefined escape sequence in file f.scad, line N` (at most **5 per string literal**);
   - `DEPRECATED: Variable names starting with digits ("2b") will be removed in future releases. in file f.scad, line N` (one per occurrence);
   - `WARNING: "a" was assigned on line 2 but was overwritten in file f.scad, line 4` (reassignment in
     the same scope; emitted even inside module bodies that are never instantiated, in file order).
2. Then evaluation. In every scope **all assignments are evaluated first**, in file order, followed by
   module instantiations (`echo`, `cube`, `for`, …) in file order (see §9).
3. Warnings produced while evaluating arguments to `echo(...)` precede the corresponding `ECHO:` line.

```openscad
echo("statement 1");
x = echo("assign x") 5;
echo("statement 2", x);
y = echo("assign y", x) x + 1;
```
```
ECHO: "assign x"
ECHO: "assign y", 5
ECHO: "statement 1"
ECHO: "statement 2", 5
```

### 1.3 Evaluation Errors

An `ERROR:` (failed assert, recursion) **aborts all further evaluation**: no subsequent ECHO runs.
The CSG file is still written with top-level nodes completed before the error, exit code 0.

```openscad
echo("before"); cube(); assert(1 == 2); echo("after"); sphere();
```
stderr:
```
ECHO: "before"
ERROR: Assertion '(1 == 2)' failed in file err_assert_toplevel.scad, line 3
TRACE: called by 'assert' in file err_assert_toplevel.scad, line 3
```
CSG: `cube(size = [1, 1, 1], center = false);` (only the cube).

Stack trace: innermost call first, outward. User module instantiations add
`call of 'm(x = -1)'` (arguments with parameter names, values formatted like echo) followed by
`called by 'm'` at the instantiation line:

```openscad
function f(x) = assert(x > 0) x;
function g(y) = f(y - 1) + 1;
module m(a, b = 2) { echo(g(a)); }
echo(f(2));
m(0, b = [1, 2]);
```
```
ECHO: 2
ERROR: Assertion '(x > 0)' failed in file err_assert_in_function.scad, line 1
TRACE: called by 'f' in file err_assert_in_function.scad, line 2
TRACE: called by 'g' in file err_assert_in_function.scad, line 3
TRACE: called by 'echo' in file err_assert_in_function.scad, line 3
TRACE: call of 'm(a = 0, b = [1, 2])' in file err_assert_in_function.scad, line 3
TRACE: called by 'm' in file err_assert_in_function.scad, line 5
```

Long traces: first 12 frames are shown, then `TRACE:   *** Excluding N frames ***` (three spaces
after `TRACE:`), then the last 12 frames (see `module_infinite_recursion`).

Assert messages:

| input | first line |
|---|---|
| `assert(1 == 2);` | `ERROR: Assertion '(1 == 2)' failed in file …` |
| `assert(false, 5);` | `ERROR: Assertion 'false' failed: 5 in file …` |
| `assert(false, [1, "a"]);` | `ERROR: Assertion 'false' failed: [1, "a"] in file …` |
| `assert(x > 0, "x must be positive")` | `ERROR: Assertion '(x > 0)' failed: "x must be positive" in file …` |
| `assert(cond, undef)` | `ERROR: Assertion '<expr>' failed: undef in file …` |
| `assert();` | `ERROR: Assertion failed in file …` |

The condition is printed with the expression printer (§4); the message is formatted as an echo value
(strings quoted).

### 1.4 Parse Errors

| input (full file) | stderr |
|---|---|
| `x = 1 +;` | `ERROR: Parser error: syntax error in file f.scad, line 1` + `Can't parse file '<abs>'!` |
| `echo(1);\n\n  a = @;` | `... syntax error in file f.scad, line 3` |
| `echo("abc);\n` | `ERROR: Parser error: Unterminated string in file f.scad, line 4` then `ERROR: Parser error: syntax error in file f.scad, line 4` then `Can't parse file ...` |
| `echo(1);\n/* unterminated\n` | `Unterminated comment ... line 5`, `syntax error ... line 5`, `Can't parse ...` |
| error inside an `include`d file | `ERROR: Parser error: syntax error in file lib/badlib.scad, line 2` + `Can't parse file '<abs of main file>'!` |
| error inside a `use`d file | `ERROR: Parser error: syntax error in file lib/badlib.scad, line 2`, then evaluation **continues** (exit 0) |

For unterminated strings/comments the reported line is `(count of '\n' in file) + 3`, i.e. two past
the last line.

On parse error: no output file, no ECHO, exit code 1.

`cube()\nsphere();` (missing `;`) is **not** a parse error: `sphere()` becomes a child of `cube()` →
`WARNING: module cube() does not support child modules in file f.scad, line 2`.

Overflowing numeric literal (`1e400`): the token is **silently discarded**:
`echo(1e400);` → `ECHO: ` ; `echo(2e400 + 1);` → `ECHO: 1` ; `echo(1e400, 5);` → syntax error.
Underflow: `1e-400` → `0`.

---

## 2. Number Formatting

There are **two distinct number formatters**.

### 2.1 Echo / `str()` / Expression Printing / Messages

Observed algorithm (consistent with all examples):

1. `nan` → `nan` (including `-(0/0)`), `+inf` → `inf`, `-inf` → `-inf`.
2. Zero (including **-0**) → `0`.
3. Otherwise round the **exact binary value** to **6 significant digits**, with exact ties rounded
   **away from zero** (`1234565` → `1.23457e+6`, `123456.5` → `123457`).
   If rounding carries into the next power of ten, use the new exponent (`999999.5` → `1e+6`, `9.999995` → `10`).
4. Let `e` be the decimal exponent (value = d.ddddd × 10^e). Use **fixed** notation if `-5 <= e < 6`,
   otherwise **scientific**.
5. Strip trailing zeros from the mantissa and strip the decimal point if nothing follows.
6. Write the exponent as `e+N` / `e-N` **without zero-padding** (`1e+6`, `1e-7`, `1.5e+300`).

| input | echo |
|---|---|
| `1/3`, `2/3`, `-1/3` | `0.333333`, `0.666667`, `-0.333333` |
| `0.1 + 0.2` | `0.3` |
| `PI` | `3.14159` |
| `123456`, `999999`, `1e6`, `1234567` | `123456`, `999999`, `1e+6`, `1.23457e+6` |
| `1e15`, `1e20`, `1e21`, `2^53`, `2^64` | `1e+15`, `1e+20`, `1e+21`, `9.0072e+15`, `1.84467e+19` |
| `0.0001`, `0.00001`, `0.000015`, `0.0000123456` | `0.0001`, `0.00001`, `0.000015`, `0.0000123456` |
| `0.000099999`, `0.0000099999` | `0.000099999`, `9.9999e-6` |
| `1e-6`, `1.1e-6`, `1e-7`, `1e-300` | `1e-6`, `1.1e-6`, `1e-7`, `1e-300` |
| `123456.789`, `-123456.789` | `123457`, `-123457` |
| `1234565`, `1234585`, `-1234565`, `123456.5` | `1.23457e+6`, `1.23459e+6`, `-1.23457e+6`, `123457` |
| `999999.5`, `9.999995`, `99999.95` | `1e+6`, `10`, `99999.9` (99999.95 in binary is 99999.94999…) |
| `1.0000005`, `0.1234565`, `0.9999995`, `1.000005` | `1`, `0.123456`, `1`, `1.00001` |
| `12.3456789e10`, `0.000001234565` | `1.23457e+11`, `1.23457e-6` |
| `5e-324`, `1.7976931348623157e308` | `4.94066e-324`, `1.79769e+308` |
| `-0`, `0 * -1`, `-0.0` | `0`, `0`, `0` |
| `1/0`, `-1/0`, `0/0`, `-(0/0)` | `inf`, `-inf`, `nan`, `nan` |
| `version_num()` | `2.0261e+7` |

The same formatter is used inside vectors, `str()`, assert messages, expression printing
(`function() 123456789` → `function() 1.23457e+8`), and arguments in TRACE lines.
Exceptions using C `printf` formatting: `$fs too small - clamping to 0.010000` (`%f`) and
`color() ... Value of 2.0 is out of range` (`%.1f`).

### 2.2 CSG Export

Numbers in CSG parameters follow **printf `%g`** (6 significant digits):

- fixed notation if `-4 <= e < 6`, otherwise scientific;
- exponent has **at least 2 digits** (`1e+06`, `1e-05`, `1.5e-05`, `1e+100`);
- exact binary value rounded with ties **to even** (`1234565` → `1.23456e+06`,
  `1234585` → `1.23458e+06`, `123456.5` → `123456`);
- **negative zero prints as `-0`**; `inf`, `-inf`, `nan`.

| input | CSG |
|---|---|
| `cube([0.00001, 0.0001, 0.000015])` | `cube(size = [1e-05, 0.0001, 1.5e-05], center = false);` |
| `cube([1e6, 123456, 1234567])` | `cube(size = [1e+06, 123456, 1.23457e+06], center = false);` |
| `cube([1234565, 1234585, 123456.5])` | `cube(size = [1.23456e+06, 1.23458e+06, 123456], center = false);` |
| `cube([1/0, -1/0, 0/0])` | `cube(size = [inf, -inf, nan], center = false);` |
| `cube([1e20, 1e100, 1e-100])` | `cube(size = [1e+20, 1e+100, 1e-100], center = false);` |
| `cube([999999.5, 0.1 + 0.2, 2^53])` | `cube(size = [1e+06, 0.3, 9.0072e+15], center = false);` |
| `cube([1e-7, 1e7, 123456789])` | `cube(size = [1e-07, 1e+07, 1.23457e+08], center = false);` |
| `cube([0.000099999, 0.0000099999, 99999.95])` | `cube(size = [9.9999e-05, 9.9999e-06, 99999.9], center = false);` |
| `cube([1, 0 * -1, 1])` | `cube(size = [1, -0, 1], center = false);` |

Key differences from echo: `echo([1e-5, 1e6])` → `ECHO: [0.00001, 1e+6]`, whereas CSG prints
`1e-05`, `1e+06`; `echo(0 * -1)` → `0`, CSG `-0`. Strings in CSG (e.g. `text = "1e+6"` from
`text(str(1e6))`) remain as produced by `str()`.

---

## 3. Value Formatting (echo / str)

| input | output |
|---|---|
| `echo(true, false, undef)` | `ECHO: true, false, undef` |
| `echo("abc")` | `ECHO: "abc"` |
| `echo("q\"q", "b\\s", "t\tt")` | `ECHO: "q"q", "b\s", "t	t"` (**raw** content, no escaping; literal tab) |
| `echo("line1\nline2")` | two lines: `ECHO: "line1` and `line2"` |
| `echo(["abc", "q\"q", "b\\s"])` | `ECHO: ["abc", "q"q", "b\s"]` (raw inside vectors too) |
| `echo([])`, `echo([[]])` | `ECHO: []`, `ECHO: [[]]` |
| `echo([1, [2, [3]]])` | `ECHO: [1, [2, [3]]]` (separator `", "`) |
| `echo([0:5])` | `ECHO: [0 : 1 : 5]` (ranges always display the step) |
| `echo([0:2:10], [0.5:1.5:3], [0:-1:-3])` | `ECHO: [0 : 2 : 10], [0.5 : 1.5 : 3], [0 : -1 : -3]` |
| `echo(function(x) x + 1)` | `ECHO: function(x) (x + 1)` |
| `echo(function(x, y = 2) x * y)` | `ECHO: function(x, y = 2) (x * y)` |
| `echo(a = 1, b = "s")` | `ECHO: a = 1, b = "s"` |
| `echo(1, b = 2, 3)` | `ECHO: 1, b = 2, 3` |
| `echo($fn = 3)` | `ECHO: $fn = 3` |
| `echo()` | `ECHO: ` (trailing space) |
| object (`textmetrics`, `object()`) | `{ a = 1; b = "x"; c = [1, 2]; }`, nested `{ nominal = { ascent = 12.5733; descent = -2.9433; }; ... }` |

`str()` concatenates representations **without quotes at the top level** (quoted inside vectors):

| input | output |
|---|---|
| `str(1/3)`, `str(-0)`, `str(1e6)` | `"0.333333"`, `"0"`, `"1e+6"` |
| `str(true)`, `str(undef)` | `"true"`, `"undef"` |
| `str("a\"b")` | `"a"b"` |
| `str([1, "a\"b", [2]])` | `"[1, "a"b", [2]]"` |
| `str([0:2:4])`, `str(function(x) x)` | `"[0 : 2 : 4]"`, `"function(x) x"` |
| `str(1, "a", [2], true, undef)` | `"1a[2]trueundef"` |
| `str()` | `""` |

---

## 4. Expression Printing (Function Literals, Assert)

Every binary operator and ternary is parenthesized; unary operators are not. String literals **are**
escaped (`\"`, `\\`, `\n`, `\t`, `\r`; other characters raw). Numbers use the echo formatter.
`+x` prints `x`; `-<literal>` folds into a literal (`-(1)` → `-1`, `-0` → `0`).

| expression in `function() <expr>` | printed form |
|---|---|
| `let(x = 1, y = 2) x + y` | `let(x = 1, y = 2) (x + y)` |
| `[for (i = [0:2:10]) if (i > 2) i else -i]` | `[for(i = [0 : 2 : 10]) (if((i > 2)) (i) else (-i))]` |
| `[for (i = [0:2], j = [1]) let(k = i) each [k, j]]` | `[for(i = [0 : 2], j = [1]) (let(k = i) (each ([k, j])))]` |
| `[for (i = 0; i < 3; i = i + 1) i]` | `[for(i = 0;(i < 3);i = (i + 1)) i]` |
| `[for (a = [1]) for (b = [2]) a + b]` | `[for(a = [1]) (for(b = [2]) ((a + b)))]` |
| `v[0].x + v.y[1]` | `(v[0].x + v.y[1])` |
| `f(1, b = 2)(3)` | `(f(1, b = 2))(3)` |
| `function(y) y * 2` | `function(y) (y * 2)` |
| `echo("e") assert(true, "m") 1` | `echo("e") assert(true, "m") 1` |
| `a ? b ? 1 : 2 : c ? 3 : 4` | `(a ? (b ? 1 : 2) : (c ? 3 : 4))` |
| `[1:3]`, `[1:2:3]` | `[1 : 3]`, `[1 : 2 : 3]` (omitted step stays omitted) |
| `[if (1) 2, 3, if (0) 4 else 5]` | `[if(1) (2), 3, if(0) (4) else (5)]` |
| `[each [1, 2], 3]` | `[each ([1, 2]), 3]` |
| `-x + !y - -1 + +3` | `(((-x + !y) - -1) + 3)` |
| `-(-x)`, `!!x` | `--x`, `!!x` |
| `c = "s\"q\n"` (default) | `function(c = "s\"q\n") c` |
| `1e6 + 0.1 + 1/3 + 1e-7 + 123456789 + 0x10 + 1.5e300` | `((((((1e+6 + 0.1) + (1 / 3)) + 1e-7) + 1.23457e+8) + 16) + 1.5e+300)` |

### 4.1 Operator Precedence (Lowest to Highest, Derived from Printed Parentheses)

1. `? :` (right-associative)
2. `||`
3. `&&`
4. `==` `!=`
5. `<` `<=` `>` `>=`
6. `|`
7. `&`
8. `<<` `>>`
9. `+` `-`
10. `*` `/` `%`
11. prefix unary `!` `-` `+` `~`
12. `^` (**right**-associative: `2 ^ 3 ^ 2` = 512; `-2 ^ 2` = `-(2 ^ 2)` = -4)
13. postfix: index `[]`, member `.x`, call `()`

All binary operators other than `^` are left-associative.
Examples: `1 < 2 | 3` → `(1 < (2 | 3))`; `1 | 2 == 3` → `((1 | 2) == 3)`; `!a == b` → `(!a == b)`;
`-a ^ b` → `-(a ^ b)`; `1 | 2 & 3 << 4 >> 5 + ~6` → `(1 | (2 & ((3 << 4) >> (5 + ~6))))`.

---

## 5. Numeric Semantics and Operators

### 5.1 Arithmetic

| input | echo |
|---|---|
| `7 / 2`, `2 ^ 10`, `2 ^ 0.5`, `2 ^ -1`, `0 ^ 0` | `3.5`, `1024`, `1.41421`, `0.5`, `1` |
| `2 ^ 3 ^ 2`, `-2 ^ 2`, `(-2) ^ 2`, `(-8) ^ (1/3)` | `512`, `-4`, `4`, `nan` |
| `1 + 2 * 3 ^ 2` | `19` |
| `5 % 3`, `-5 % 3`, `5 % -3`, `-5 % -3` | `2`, `-2`, `2`, `-2` (sign of dividend, like `fmod`) |
| `5.5 % 2`, `-5.5 % 2`, `5 % 0`, `0 % 5` | `1.5`, `-1.5`, `nan`, `0` |
| `1/0`, `-1/0`, `0/0`, `1/(-0)` | `inf`, `-inf`, `nan`, `-inf` (negative zero exists) |
| `0/0 == 0/0`, `1/0 == 1/0` | `false`, `true` |
| `2 ^ 1000`, `fact(171)` | `1.07151e+301`, `inf` |

All numbers are `f64`. Literals: `.5`, `1.`, `1.e3`, `1e+3`, `1E3`, `00012` (=12), `0x10` (=16),
`0xff`; **`0XFF` is not hex** — it is an identifier (DEPRECATED + unknown variable).
`9007199254740993` → `WARNING: Integer "9007199254740993" cannot be represented precisely in file …`.

### 5.2 Math Functions

| input | echo |
|---|---|
| `round(0.5), round(1.5), round(2.5), round(-0.5), round(-1.5), round(-2.5)` | `1, 2, 3, -1, -2, -3` (half away from zero) |
| `round(-0.4)`, `round(0.49999999999999994)` | `0`, `1` (!) |
| `floor(-2.7), ceil(-2.1), floor(-0.5), ceil(-0.5)` | `-3, -2, -1, 0` |
| `abs(-0), sign(-2), sign(0), sign(-0)` | `0, -1, 0, 0` |
| `sqrt(-1), ln(0), ln(-1), log(100), log(2, 8)` | `nan, -inf, nan, 2, 3` (`log(b, x)` = base-b logarithm) |
| `exp(1), pow(-8, 1/3), pow(0, 0)` | `2.71828, nan, 1` |
| `min(3, 1, 2), max([3, 1, 2]), min(1)` | `1, 3, 1` |
| `min(1, 1/0), max(-1/0, 2), min([1, 0/0]), max(0/0, 1)` | `1, 2, 1, nan` |

`round(0.49999999999999994)` = 1 indicates an implementation like `floor(x + 0.5)` for x ≥ 0 and
`-floor(-x + 0.5)` for x < 0.

### 5.3 Trigonometry (Degrees)

**Exact** values at multiples of 30°/45°/90° (all equalities evaluate to `true`):
`sin(30)==0.5`, `sin(150)==0.5`, `sin(-30)==-0.5`, `sin(210)==-0.5`, `sin(330)==-0.5`,
`cos(60)==0.5`, `cos(120)==-0.5`, `cos(240)==-0.5`, `cos(300)==0.5`, `cos(90)==0`, `sin(180)==0`,
`cos(270)==0`, `sin(360)==0`, `sin(390)==0.5`, `sin(720)==0`, `sin(45)==sqrt(2)/2`, `cos(45)==sqrt(0.5)`,
`tan(45)==1`, `tan(135)==-1`, `tan(60)==sqrt(3)`.
`tan(90)` → `inf`, `tan(-90)` → `-inf`, `tan(270)` → `-inf`, `tan(30)` → `0.57735`.
`asin(0.5)==30`, `acos(0.5)==60`, `atan(1)==45`, `atan2(1,1)==45`, `asin(1)==90`, `acos(0)==90`.

Signed zeros (observed via `1/x`):

| x | 1/x | x | 1/x |
|---|---|---|---|
| `sin(0)` | `inf` | `cos(90)` | `inf` |
| `sin(180)` | `-inf` | `cos(270)` | `-inf` |
| `sin(-180)` | `-inf` | `cos(-90)` | `-inf` |
| `sin(360)` | `inf` | `cos(450)` | `inf` |
| `sin(-360)` | `inf` | `sin(0 * -1)` | `-inf` |
| `sin(540)` | `-inf` | `tan(180)`, `tan(-180)` | `-inf`, `-inf` |

Others: `atan2(0, -1)` → `180`, `atan2(-0, -1)` → `-180`, `atan2(0, 0)` → `0`, `atan(1/0)` → `90`,
`asin(2)` → `nan`, `sin(1e10)` → `-0.984808`, `sin(89.9999)` → `1`.

### 5.4 Comparisons and Equality

| input | echo |
|---|---|
| `1 == 1.0, 1 == true, 0 == false, 1 == "1"` | `true, false, false, false` (different types: `==` is `false`, no warning) |
| `undef == undef, undef == 0, undef == [], [] == []` | `true, false, false, true` |
| `[1, [2]] == [1, [2]], [1, 2] != [1, 2, 3], [0/0] == [0/0]` | `true, true, false` |
| `"a" < "b", "abc" < "abd", "B" < "a", "a" < "ab", "é" < "z"` | `true, true, true, true, false` (UTF-8 byte comparison) |
| `[1, 2] < [1, 3], [1] < [1, 2], [2] > [1, 5]` | `true, true, true` (lexicographic) |
| `true < false, false < true` | `false, true` |
| `true < 2` | `undef` + `WARNING: undefined operation (bool < number) in file …` |
| `1 < "a"`, `undef < 1`, `2 > true`, `[1] < 1` | `undef` + `undefined operation (number < string)` / `(undefined < number)` / `(number > bool)` / `(vector < number)` |

### 5.5 Truthiness (`!`, `if`, `? :`, `&&`, `||`)

Falsy: `false`, `0` (including -0), `""`, `[]`, `undef`. **Truthy**: `nan`, any number ≠ 0, `"0"`, `[0]`,
`[[]]`, every range (even empty), functions.
`!(0/0)` → `false`; `0/0 ? 1 : 2` → `1`. `&&`/`||` always return a boolean:
`1 && 2` → `true`, `0 || ""` → `false`.
Note: a range like `[0:-1:1]` still emits the reversed-range warning inside `!`.

### 5.6 Vectors and Matrices

| input | echo |
|---|---|
| `[1, 2] + [3, 4]`, `[1, 2] - [3, 4]` | `[4, 6]`, `[-2, -2]` |
| `[1, 2, 3] + [1]`, `[1, 2] - [1, 2, 3]` | `[2]`, `[0, 0]` (**truncated to shorter length**, no warning) |
| `[1, "a"] + [1, 2]` | `[2, undef]` (no warning for element mismatch) |
| `[] + []` | `[]` |
| `2 * [1, 2]`, `[1, 2] * 2`, `[2, 4] / 2`, `2 / [2, 4]` | `[2, 4]`, `[2, 4]`, `[1, 2]`, `[1, 0.5]` |
| `[1, [2, 3]] * 2`, `-[1, [2, 3]]` | `[2, [4, 6]]`, `[-1, [-2, -3]]` (recursive) |
| `[1, 2] * [3, 4]` | `11` (dot product) |
| `[[1, 2], [3, 4]] * [1, 1]`, `[1, 1] * [[1, 2], [3, 4]]` | `[3, 7]`, `[4, 6]` |
| `[[1, 2], [3, 4]] * [[5, 6], [7, 8]]` | `[[19, 22], [43, 50]]` |
| `[[1, 2, 3]] * [[1], [2], [3]]` | `[[14]]` |
| `[1, 2] * [3]` | `undef` + `WARNING: vector*vector requires matching lengths (2 != 1) in file …` |
| `[] * []` | `undef` + `WARNING: Multiplication is undefined on empty vectors in file …` |
| `[[1, 2], [3, 4]] * [1, 2, 3]` | `undef` + `WARNING: matrix*vector requires matrix column count to match vector length (2 != 3) in file …` |
| `["a", 1] * [1, 1]` | `undef` + `WARNING: undefined vector*vector multiplication where first elements are types string and number in file …` |

### 5.7 Undefined Operations

Result `undef` and `WARNING: undefined operation (<type1> <op> <type2>) in file …`, with type names
`number`, `string`, `bool`, `vector`, `undefined`, `range`, `function`:

`"a" + "b"` → `(string + string)` (**no string concatenation with +**); `"a" * 2` → `(string * number)`;
`[1, 2] + 1` → `(vector + number)`; `1 + [1, 2]` → `(number + vector)`; `[1] + undef` →
`(vector + undefined)`; `undef + 1` → `(undefined + number)`; `true + 1` → `(bool + number)`;
`true + true` → `(bool + bool)`; `[1, 2] % 2` → `(vector % number)`; `[1, 2] / [1, 2]` →
`(vector / vector)`. Unary: `-"a"` → `(-string)`, `-true` → `(-bool)`, `-undef` → `(-undefined)`.

### 5.8 Bitwise Operators (Supported)

Operands converted to signed 64-bit integer (truncation; out-of-range/`nan` → `-9223372036854775808`).

| input | echo |
|---|---|
| `1 \| 2, 6 & 3, ~0, ~1, 1 << 4, 256 >> 2, -1 >> 1, 12 & 10` | `3, 2, -1, -2, 16, 64, -1, 8` |
| `5.7 \| 0, -5.7 \| 0, ~1.5` | `5, -5, -2` |
| `1 << 63, 2^63 \| 0, 1e20 \| 0, (0/0) \| 0, ~(1/0)` | `-9.22337e+18, -9.22337e+18, -9.22337e+18, -9.22337e+18, 9.22337e+18` |
| `1 << 64` | `undef` + `WARNING: shift too large in file …` |
| `1 << -1` | `undef` + `WARNING: negative shift in file …` |
| `true \| 1`, `"a" \| 1` | `undef` + `undefined operation (bool \| number)` / `(string \| number)` |

---

## 6. Strings

- Strings are UTF-8; `len` and indexing operate on **Unicode code points**:
  `len("é😀a")` → `3`, `"é😀a"[1]` → `"😀"`, `"abc"[2.9]` → `"c"`, `"abc"[5]`, `"abc"[-1]` → `undef`.
- Valid escapes: `\n` `\t` `\r` `\"` `\\` `\xHH` (`\x01`–`\x7f` only, exactly 2 hex digits),
  `\uHHHH` (4 hex digits), `\UHHHHHH` (6 hex digits). `\x00` and `\u0000` produce a **space** (0x20).
- Invalid escapes (`\q`, `\a`, `\0`, `\'`, `\v`, `\b`, `\f`, `\x7`, `\x80`, `\xff`, `\u12`, `\u{41}`,
  `\U00004`): the backslash is dropped and the rest stays literal (`"\x80"` → `"x80"`), emitting
  `WARNING: Undefined escape sequence in file f.scad, line N` (parse phase, max 5 per literal).
- `chr(65)` → `"A"`; `chr(65, 66)` → `"AB"`; `chr([65, 66, [67]])` → `"ABC"` (flattens);
  `chr(65.7)` → `"A"` (truncates); `chr(0)`, `chr(-1)`, `chr(1114112)`, `chr("a")`, `chr()` → `""` with no warning.
- `ord("A")` → `65`; `ord("😀")` → `128512`; `ord("BA")` → `66` (first character); `ord("")` → `undef`;
  `ord(65)` → `undef` + `WARNING: ord() parameter could not be converted: argument 0: expected string, found number (65) in file …`
  (`found undefined (undef)`, `found vector (["A"])`).
- `for (c = "ab")` iterates over characters; `[each "ab"]` → `["a", "b"]`; `concat("ab", "cd")` → `["ab", "cd"]`.

---

## 7. Indexing and Ranges

### 7.1 Indexing

| input (`v = [10, 20, 30, 40]`) | echo |
|---|---|
| `v[0], v[3], v[1.5], v[1.9999], v[-0.5]` | `10, 40, 20, 20, 10` (index **truncated toward zero**) |
| `v[-1], v[4], v[100]` | `undef` (no warning) |
| `v["a"], v[undef], v[true], v[[1]], v[0/0], v[1/0]` | `undef` (no warning) |
| `v.x, v.y, v.z, v.w` | `10, 20, 30, undef` (**`.w` not supported** without `vector-swizzle`) |
| `[1, 2].z, "abc".x, [1, 2, 3].xy, (5).x` | `undef` (all) |
| `5[0], undef[0], true[0], ""[0]` | `undef` |
| `[0:2:9][0], [0:2:9][1], [0:2:9][2], [0:2:9][3]` | `0, 2, 9, undef` (indexing a range returns begin, step, end) |

With `--enable=vector-swizzle`: `v.xy` → `[1, 2]`, `v.zyx` → `[3, 2, 1]`, `v.xxx` → `[1, 1, 1]`, `v.w` → `4`.

### 7.2 Ranges

- `[a:b]` = step 1; ranges print as `[a : step : b]`.
- **`[5:0]` does not auto-reverse**: it is an empty range and emits on creation
  `WARNING: begin is greater than the end, but step is positive in file …`;
  `[0:-1:5]` → `WARNING: begin is smaller than the end, but step is negative in file …`.
- Element count with decimal steps (with tolerance): `[0:0.1:1]` → 11 elements
  `[0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1]`; `[0:0.1:0.3]` → 4; `[0:1/3:1]` → 4;
  `[0:0.2:1]` → 6; `[1:1.5:5]` → `[1, 2.5, 4]`.
- `[3:3]` → `[3]`; `[2:-1:0]` → 2, 1, 0.
- Limit: a range with **≥ 1,000,000 elements** is not iterated (`for` or list comprehension):
  `WARNING: Bad range parameter in for statement: too many elements (1000000) in file …`, zero
  iterations. `[0:1:999998]` (999,999 elements) works. Step 0: `[0:0:5]` →
  `too many elements (4294967295)`. `[0:1e-9:1]` → `(1000000001)`. Creating and printing the range is
  always allowed (`echo([0:1:1e7])` → `ECHO: [0 : 1 : 1e+7]`).
- `len([0:5])` → `undef` + `WARNING: len() parameter could not be converted: argument 0: expected string, found range ([0 : 1 : 5]) in file …`.

### 7.3 `for` Over Non-Vector Values

`for (i = 3)` → one iteration with 3; `for (i = true)` → one iteration with `true`;
`for (i = "ab")` → `"a"`, `"b"`; `for (i = undef)` → zero iterations; `for (i = [1, [2, 3]])` → `1`, `[2, 3]`.

---

## 8. List Comprehensions

| input | echo |
|---|---|
| `[for (i = [0:3]) if (i % 2) i]` | `[1, 3]` |
| `[for (i = [0:3]) if (i % 2) i else -i]` | `[0, 1, -2, 3]` |
| `[for (i = [0:2]) let (j = i * i) j]` | `[0, 1, 4]` |
| `[each [1, 2], each [3:5], each "ab", each 7]` | `[1, 2, 3, 4, 5, "a", "b", 7]` |
| `[each undef], [each []]` | `[], []` |
| `[for (i = 0, j = 10; i < 3; i = i + 1, j = j - 1) [i, j]]` | `[[0, 10], [1, 9], [2, 8]]` |
| `[for (i = [1, 2], j = [3, 4]) [i, j]]` | `[[1, 3], [1, 4], [2, 3], [2, 4]]` (outer variable first) |
| `[for (i = [0:1], j = [0:i]) [i, j]]` | `[[0, 0], [1, 0], [1, 1]]` (later bindings see earlier ones) |
| `[for (a = [1:2]) for (b = [1:a]) b]` | `[1, 1, 2]` |
| `[if (true) 1, if (false) 2, 3]` | `[1, 3]` |
| `[for (i = [0:5]) if (i < 2) "a" else if (i < 4) "b" else "c"]` | `["a", "a", "b", "b", "c", "c"]` |
| `[(1), (each [1])]` | `[1, 1]` |

`let` (expression and module) is **sequential**: `let (a = 1, b = a + 1)` → b = 2.
Duplicate name in `let`: **first** binding wins, emitting
`WARNING: Ignoring duplicate variable assignment "a" = 2 in file …` (the printed value is the expression).

---

## 9. Scope and Evaluation Order

- In every scope (file, module body, `if`/`for`/`let` block, instantiation children) **all assignments
  are evaluated first** before module instantiations, in file order.
- **Reassignment in the same scope**: a single variable remains, at the **position of the first
  assignment** but with the **expression of the last**; parse-phase warning
  `WARNING: "a" was assigned on line 2 but was overwritten in file f.scad, line 4`.
  ```openscad
  a = 1; echo("a1", a); a = 2; echo("a2", a);     // ECHO: "a1", 2 / ECHO: "a2", 2
  z = 1; z = z + 1; echo(z);                       // z = z + 1 evaluated where z = 1 stood:
  // WARNING: "z" was assigned on line 1 but was overwritten in file f.scad, line 2
  // WARNING: Ignoring unknown variable "z" in file f.scad, line 2
  // WARNING: undefined operation (undefined + number) in file f.scad, line 2
  // ECHO: undef
  ```
  A variable defined in an `include`d file and reassigned in the main file does **not** emit the warning.
- Use before assignment: `b = c + 1; c = 5;` → `Ignoring unknown variable "c"` and b = `undef`.
- Unknown variable: `WARNING: Ignoring unknown variable "x" in file …` (value `undef`).
  Function: `WARNING: Ignoring unknown function 'f' in file …`. Module:
  `WARNING: Ignoring unknown module 'm' in file …` (no CSG node).
- Variables defined inside an `if`/`for` block are not visible outside it.
- An assignment inside a module shadows the outer variable across the **entire** module body
  (even before the assignment line): `g = 1; module h() { echo(g); g = 10; }` → `10`.
- Functions are **lexically scoped** for normal variables (`function addk(x) = x + k;` uses the `k`
  at the definition site even if the caller defines another `k`).
- Function literals capture their scope: `a = 1; f = function() a; a = 2; echo(f());` → `2`
  (because reassignment replaces the expression at `a`'s binding).
- Separate namespaces for variables, functions, and modules: `foo = 5; function foo() = 10; module foo() …`
  coexist (`echo(foo, foo())` → `5, 10`).
- Functions and modules are visible throughout the scope in which they are defined (hoisted): they can
  be called before their definition. Definitions nested inside a module are not visible outside it.
- **`$` variables** (dynamically scoped): visible inside called modules and functions, and inside
  children (`children()`), which see the value from the module executing `children()`:
  ```openscad
  module ch() { $y = "from ch"; children(); }
  ch() echo($y);                                   // "from ch"
  module setter() { $w = "setter"; children(); }
  $w = "top"; setter() echo($w);                   // "setter"
  function getfn() = $fn;
  module m() { $fn = 24; echo(getfn()); }          // 24
  echo(let ($fn = 6) getfn());                     // 6
  ```
- Top-level defaults: `$fn = 0`, `$fa = 12`, `$fs = 2`, `$t = 0`, `$preview = true`
  (CSG export), `$vpr = [55, 0, 25]`, `$vpt = [0, 0, 0]`, `$vpd = 140`, `$vpf = 22.5`, `PI = 3.14159…`.
  `$children` and `$parent_modules` do not exist at file scope (`Ignoring unknown variable "$children"`).

---

## 10. `include` and `use`

| | `include <f>` | `use <f>` |
|---|---|---|
| executes top-level statements (echo, geometry) | yes, as if pasted inline | no |
| file's top-level variables | visible | **not** visible (`Ignoring unknown variable`) |
| functions and modules | visible | visible |
| file's functions see its own variables | yes (overridable by main file) | yes (`uses_private()` → 5) |
| caller's `$` variables | yes | yes (`show_q()` sees `$q = "caller"`) |
| missing file | `WARNING: Can't find include file 'lib/x.scad'. in file f.scad, line 1` | `WARNING: Can't open library 'lib/x.scad'. in file f.scad, line 2` |
| parse error | fatal error for the whole file | error printed, rest continues |

With `include`, redefining `libvar = 1;` in the main file updates the value seen by the library's own
functions (`libf(2)` → 2), without warning. Paths resolve relative to the including file.

---

## 11. Functions, Modules, Arguments

- Arguments: positional and named arguments can be mixed, including **positional after named**
  (`g(b = 2, 1)` → `[1, 2]`). Omitted parameters → `undef` (no warning).
- Too many positional arguments: `WARNING: Too many unnamed arguments supplied in file …` (extras ignored).
- Unknown parameter name: `WARNING: variable "c" not specified as parameter in file …`.
- Duplicate parameter name: `WARNING: argument "a" supplied more than once in file …` (**last** wins).
- Default values: evaluated in the **definition scope**, do **not** see sibling parameters
  (`function f(a = 1, b = a)` → `b = undef` with `Ignoring unknown variable "a"`), but do see the
  caller's `$` variables. `x = 10; function g(x = x) = x;` → `g()` = 10.
- Built-in wrong arity: `WARNING: abs() number of parameters does not match: expected 1, found 0 in file …`
  (`atan2()`/`pow()`/`lookup()`/`cross()`: `expected 2`; **`log()`: `expected 2`** even though it accepts 1;
  `rands()`: `expected 3 or 4`; `search()`: `expected between 2 and 4`; `min()`/`max()`: `expected at least 1`).
- Function literals: `f = function(x) x * 3; f(2)` → 6; nested closures work
  (`adder(5)(10)` → 15); `(function(x) x + 1)(1)` → 2. Built-ins are not first-class values
  (`is_function(len)` → `Ignoring unknown variable "len"`, `false`).
- `echo(...) expr`, `assert(...) expr`, `let(...) expr` are expressions; `echo` prints before
  evaluating `expr`.
- Removed modules/functions: `assign`, `child`, `import_stl` → `Ignoring unknown module`.
- `children()`:
  - `$children` counts all child statements (including `if`, `for`, `echo`): `e() { cube(); if (false) sphere(); for (…) cube(); echo("x"); }` → 4.
  - out-of-bounds indices: `WARNING: Children index (5) out of bounds (1 children) in file …`; decimal
    index truncated (`1.7` → 1); wrong type: `WARNING: Bad parameter type ("a") for children, only accept: empty, number, vector, range in file …`.
- `parent_module(n)` / `$parent_modules`: count **user modules only** (not `translate`, etc.).
  `parent_module(0)` = current module; past the stack:
  `WARNING: Parent module index (2) greater than the number of modules on the stack in file …` → `undef`.
  `parent_module()` = `parent_module(1)`.

---

## 12. Built-in Functions

Available without feature flags: `abs sign sin cos tan acos asin atan atan2 floor round ceil ln log pow
sqrt exp rands min max norm cross concat lookup str chr ord search version version_num parent_module
len is_undef is_bool is_num is_string is_list is_function is_object textmetrics fontmetrics dxf_dim
dxf_cross`.
Experimental (without flag: `WARNING: Experimental builtin function 'object' is not enabled in file …`
followed by `WARNING: Ignoring unknown function 'object' in file …`): `object`, `has_key`
(`object-function`), `import` as a function (`import-function`).
Non-existent: `keys`, `values`, `typeof`, `is_range`, `is_vector`, `sort`, `sum`, `reverse`…

| input | echo |
|---|---|
| `version(), version_num()` | `[2026, 10, 22], 2.0261e+7` |
| `is_num(0/0), is_num(1/0), is_list([0:1]), is_list("ab"), is_object(1)` | `false, true, false, false, false` |
| `len(undef)` / `len(5)` / `len(true)` | `undef` + `len() parameter could not be converted: argument 0: expected string, found undefined (undef)` / `number (5)` / `bool (true)` |
| `concat([1], [2, 3], 4, "ab", [[5]]), concat(), concat([0:2]), concat(undef, [1])` | `[1, 2, 3, 4, "ab", [5]], [], [[0 : 1 : 2]], [undef, 1]` |
| `lookup(1.5, t), lookup(-1, t), lookup(6, t), lookup(3, t)` with `t = [[0,0],[1,10],[2,20],[5,50]]` | `15, 0, 50, 30` (clamped at endpoints, linear interpolation) |
| `lookup(0.5, [[1, 10], [0, 0]]), lookup(1, [[1, 5]]), lookup(1, [])` | `5, 5, undef` |
| `norm([3, 4]), norm([]), norm([5])` | `5, 0, 5` |
| `norm([[1, 2]])` | `undef` + `WARNING: Incorrect arguments to norm() in file …` |
| `norm(5)` | `undef` + `norm() parameter could not be converted: argument 0: expected vector, found number (5)` |
| `cross([1, 2, 3], [4, 5, 6]), cross([1, 0], [0, 1])` | `[-3, 6, -3], 1` (2D → scalar) |
| `cross([1, 2], [3])` | `undef` + `WARNING: Invalid vector size of parameter for cross() in file …` |
| `min()` | `undef` + `min() number of parameters does not match: expected at least 1, found 0` |
| `min([])` | `undef` + `min() number of parameters does not match: expected at least 1 vector element, found 0` |
| `min([3, 1], [2])` | `undef` + `min() parameter could not be converted: argument 0: expected number, found vector ([3, 1])` |
| `max(1, "a")` | `undef` + `max() parameter could not be converted: argument 1: expected number, found string ("a")` |

### 12.1 `rands`

With a seed it is deterministic and matches `std::mt19937(seed)` + `generate_canonical<double, 53>`
(two 32-bit draws per number, the second draw is the more significant part):
`rands(0, 1, 3, 42)` → `[0.796543, 0.183435, 0.779691]`; `rands(0, 1, 2, 0)` → `[0.592845, 0.844266]`;
`rands(0, 10, 4, 1)` → `[9.97185, 9.32557, 1.28124, 9.99041]`; `rands(-5, 5, 3, 123)` →
`[2.12955, -0.715291, 1.90885]`; `rands(5, 5, 2, 1)` → `[5, 5]`; `rands(0, 1, 0, 1)` → `[]`.
If min > max they are swapped: `rands(1, 0, 1, 1)` → `[0.997185]`.
Non-integer/negative seeds (`rands(0,1,1,1.5)` → `[0.7768]`, `rands(0,1,1,-1)` → `[0.111607]`):
seed conversion not specified.

### 12.2 `search`

| input | echo |
|---|---|
| `search("a", "banana")` | `[1]` |
| `search("an", "banana")` | `[1, 2]` (each character searched separately, first occurrence) |
| `search("a", "banana", 0)` | `[[1, 3, 5]]` |
| `search("abc", "aabbcc", 0)` | `[[0, 1], [2, 3], [4, 5]]` |
| `search("na", "banana", 2)` | `[[2, 4], [1, 3]]` |
| `search("q", "abc")`, `search("", "abc")` | `[]`, `[]` |
| `search(3, [1, 2, 3, 3])` | `[2]` |
| `search([3], [1, 2, 3, 3], 0)` | `[[2, 3]]` |
| `search([1, 9], [1, 2, 3])` | `[0, []]` |
| `search([[1, "x"]], [[1, "x"], [2, "y"]])` | `[0]` |
| `search("x", [["a", 1], ["x", 2]])` | `[1]` (matches column 0) |
| `search(2, [["a", 1], ["x", 2]], 0, 1)` | `[1]` (column 1) |
| `search([1, 2], [[1, "a"], [2, "b"], [1, "c"]], 0)` | `[[0, 2], [1]]` |
| `search(["b"], ["abc", "b", "xb"], 0)` | `[[1]]` |
| `search("b", ["abc", "b", "xb"])` | `[]` + `WARNING: Invalid entry in search vector at index 0, required number of values in the entry: 1. Invalid entry: "abc" in file …` |

### 12.3 Objects (`--enable=object-function`) and Font Metrics

```
o = object(a = 1, b = "x", c = [1, 2]);
echo(o, o.a, o["b"], has_key(o, "a"), has_key(o, "z"), is_object(o), len(o), o.z);
→ ECHO: { a = 1; b = "x"; c = [1, 2]; }, 1, "x", true, false, true, 3, undef
object(o, d = 4) → { a = 1; b = "x"; c = [1, 2]; d = 4; }
object([["k", 1], ["j", 2]]) → { k = 1; j = 2; }
for (k = o) echo(k); → "a", "b", "c"
```
`textmetrics("a")` → `{ position = [0.5888, -0.1408]; size = [7.136, 7.616]; ascent = 7.4752; descent = -0.1408; offset = [0, 0]; advance = [7.7243, 0]; }`
`fontmetrics()` → `{ nominal = { ascent = 12.5733; descent = -2.9433; }; max = { ascent = 13.6109; descent = -4.2114; }; interline = 15.9709; font = { family = "Liberation Sans"; style = "Regular"; }; }`
(do not require feature flags; default font Liberation Sans, size 10).

---

## 13. Recursion and Limits

| situation | outcome |
|---|---|
| tail recursion (`n == 0 ? acc : f(n - 1, acc + 1)`) | iterative: `tail(999999)` ok; at **1,000,000** iterations → error |
| `function f(x) = f(x + 1);` | `ERROR: Recursion detected calling function 'f' in file f.scad, line 1` + `TRACE: called by 'f' … line 1` + `TRACE: called by 'echo' … line 2` |
| non-tail recursion `1 + nt(n - 1)` | `nt(9999)` ok, `nt(10000)` → `Recursion detected calling function 'nt'` (stack-dependent: with heavier bodies the limit is lower, e.g. `ERROR: Stack exhausted while trying to convert a vector to EchoString`) |
| recursive modules | stack-dependent limit: ~4563 levels with `if`, ~3000 with `translate` in between; `ERROR: Recursion detected calling module 'mm' in file …` |
| `module m() m(); m();` | `ERROR: Recursion detected calling module 'm' …` + 12 TRACE + `TRACE:   *** Excluding N frames ***` + 12 TRACE |

The recursion message is identical for both the tail-iteration cap and stack exhaustion. Evaluation
halts immediately after the error.

---

## 14. CSG Export (`-o file.csg`)

### 14.1 General Format

- One node per line; nodes with children: `name(params) {` … `}`; childless nodes: `name(params);`.
- Indentation uses **one TAB per level**.
- Top-level nodes are written sequentially **with no root `group()`**. The file ends with an extra blank
  line (`...;\r\n\r\n`); an empty tree produces just `\r\n`.
- Modifiers: the character (`%` or `#`) is written **at the start of the line, before any TABs**:
  ```
  multmatrix([[1, 0, 0, 1], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]) {
  %	sphere($fn = 0, $fa = 12, $fs = 2, r = 1, style = "orig");
  }
  ```
  `*` (disable): node is omitted. `!` (root): output contains **only** that subtree, stripped of its `!`
  modifier (even when nested inside a module); with multiple `!` the first one wins +
  `WARNING: More than one Root Modifier (!) in file f.scad, line 2`.
- Parameters: `name = value`, separated by `", "`; numbers in `%g` format (§2.2), strings in quotes
  **without escaping**, vectors `[a, b]`, booleans `true`/`false`.
- `$fn`, `$fa`, `$fs` are printed on nodes that use them, showing their effective (including inherited) values.

### 14.2 Tree Structure

| source | CSG |
|---|---|
| block `{ … }` (even nested, at any level) | **flattened**, no node |
| `group() { cube(); }` | `group() {` … `}` |
| `union() {}` | `union();` |
| user module instantiation `m();` | `group() { <children> }`; childless `group();` |
| `for (…) …` | `group() { … }` (flat list of all iterations); empty: `group();` |
| `intersection_for (…) …` | `intersection() { … }`; empty: `intersection();` |
| `if (true) x;` / `if (c) … else …` | `group() { <taken branch> }` |
| `if (false) x;` (no else) | **no node** (even inside a module: `module m() { if (false) cube(); }` → `group();`) |
| `let (…) x;`, `echo(…) x;`, `assert(…) x;` | `group() { … }` |
| `children();` | `group() { <all children> }`; childless `group();` |
| valid `children(i)` | `group() { <child i> }`; out-of-bounds: no node |
| non-geometry statements (standalone `echo(…);`, assignments) | no node |
| built-in transform module with empty children | `multmatrix(...);` |
| unknown module | no node |

Examples (`csg_children_nesting`):
```
module c() children();      c();           →  group() {\n\tgroup();\n}
                            c() cube();    →  group() {\n\tgroup() {\n\t\tcube(...);\n\t}\n}
```

### 14.3 3D Primitives

| source | CSG |
|---|---|
| `cube();` | `cube(size = [1, 1, 1], center = false);` |
| `cube(2);` | `cube(size = [2, 2, 2], center = false);` |
| `cube([1, 2, 3], center = true);` | `cube(size = [1, 2, 3], center = true);` |
| `cube(-1);` | `cube(size = [-1, -1, -1], center = false);` (no warning) |
| `cube([1, 2]);` | `cube(size = [1, 1, 1], …)` + `WARNING: Unable to convert cube(size=[1, 2], ...) parameter to a number or a vec3 of numbers in file …` |
| `cube("a");` | default + `Unable to convert cube(size="a", ...) parameter …` |
| `cube(undef);`, `cube(center = "yes");` | default, no warning (non-boolean `center` → `false`) |
| `cube(1, 2, 3);` | `cube(size = [1, 1, 1], center = false);` + `Too many unnamed arguments supplied` |
| `sphere();` | `sphere($fn = 0, $fa = 12, $fs = 2, r = 1, style = "orig");` |
| `sphere(d = 3);` | `… r = 1.5, style = "orig");` |
| `sphere(r = 1, d = 4);` | `… r = 2 …` + `WARNING: Ignoring radius variable "r" as diameter "d" is defined too. in file …` |
| `sphere(-1);` / `sphere("a");` | `r = -1` / `r = 1` (no warning) |
| `sphere(style = "octa");` | `… style = "octa");` |
| `sphere(style = "ico");` | `"orig"` + `WARNING: sphere(style="ico") is not "orig" or "octa", using "orig" in file …` |
| `sphere(style = 1);` | `"orig"` + `WARNING: (..., style=1) Invalid type: expected string, found number in file …` |
| `cylinder();` | `cylinder($fn = 0, $fa = 12, $fs = 2, h = 1, r1 = 1, r2 = 1, center = false);` |
| `cylinder(h = 2, d = 3, center = true);` | `… h = 2, r1 = 1.5, r2 = 1.5, center = true);` |
| `cylinder(5, 1, 2, true);` | positional `h, r1, r2, center`: `h = 5, r1 = 1, r2 = 2, center = true` |
| `cylinder(h = 2, d1 = 1, d2 = 2, $fn = 6);` | `cylinder($fn = 6, $fa = 12, $fs = 2, h = 2, r1 = 0.5, r2 = 1, center = false);` |
| `cylinder(h = 1, r = 1, r1 = 2);` | `r1 = 2, r2 = 1` + `WARNING: Cylinder parameters ambiguous in file …` (also `h = 3, r = 1, r2 = 0`) |
| `cylinder(r = 1, d = 4, h = 1);` | `r1 = 2, r2 = 2` + `Ignoring radius variable "r" as diameter "d" is defined too.` (`"r1"`/`"d1"` analogously) |
| `cylinder(r = -1, h = 2);`, `cylinder(h = -2);` | negative values printed as-is |
| `polyhedron(points = P, faces = F);` | `polyhedron(points = P, faces = F, convexity = 1);` |
| `polyhedron();` | `polyhedron(points = [], faces = [], convexity = 1);` + `WARNING: Unable to convert points = undef to a vector of coordinates in file …` |

### 14.4 2D Primitives

| source | CSG |
|---|---|
| `square();` / `square(2);` | `square(size = [1, 1], center = false);` / `square(size = [2, 2], center = false);` |
| `square([1, 2, 3]);` | default + `Unable to convert square(size=[1, 2, 3], ...) parameter to a number or a vec2 of numbers` |
| `circle();` | `circle($fn = 0, $fa = 12, $fs = 2, r = 1);` |
| `circle(d = 4, $fn = 8);` | `circle($fn = 8, $fa = 12, $fs = 2, r = 2);` |
| `polygon([[0, 0], [1, 0], [0, 1]]);` | `polygon(points = [[0, 0], [1, 0], [0, 1]], paths = undef, convexity = 1);` |
| `polygon(points = P, paths = [[0, 1, 2], [3, 4, 5]], convexity = 3);` | `polygon(points = P, paths = [[0, 1, 2], [3, 4, 5]], convexity = 3);` |
| `polygon();` / `polygon("x");` | `polygon(points = [], paths = undef, convexity = 1);` + `Unable to convert points = undef` / `points = "x"` `to a vector of coordinates` |
| `text("Hi");` | `text(text = "Hi", size = 10, spacing = 1, font = "", direction = "ltr", language = "en", script = "Latn", halign = "default", valign = "default", $fn = 0, $fa = 12, $fs = 2);` |

`text()`:
- fixed order `text, size, spacing, font, direction, language, [script], halign, valign, $fn, $fa, $fs`;
- `script` is the ISO 15924 code detected from the text (`"Latn"`, `"Cyrl"`, `"Hani"`, `"Arab"`); if the
  text contains only Common characters (`"123"`, `" "`, `""`), the parameter **is omitted**;
  `script = "latin"` → `"Lati"`, `"arabic"` → `"Arab"` (first 4 letters, TitleCase);
- `direction` defaults to `"ltr"`, or `"rtl"` for Arabic text; `language` defaults to `"en"`;
- `text(5)` → `text = ""` + `WARNING: text(..., text=5) Invalid type: expected string, found number in file …`;
- `text("a\"b\\c")` → `text = "a"b\c"` (no escaping).

### 14.5 Transforms → `multmatrix`

`translate`, `rotate`, `scale`, `mirror`, `multmatrix` all lower to
`multmatrix([[a, b, c, d], [e, f, g, h], [i, j, k, l], [0, 0, 0, 1]]) {` (row-major 4×4).

| source | matrix |
|---|---|
| `translate([1, 2, 3])` | `[[1, 0, 0, 1], [0, 1, 0, 2], [0, 0, 1, 3], [0, 0, 0, 1]]` |
| `translate([1, 2])` | z = 0 |
| `translate(5)`, `translate("a")`, `translate([1,2,3,4])`, `translate()` | identity + `WARNING: Unable to convert translate(5) parameter to a vec3 or vec2 of numbers in file …` (`translate(undef)` for the last) |
| `translate([0 * -1, 0, 0])` | translation entry printed as `0` (not `-0`) |
| `rotate(90)` | `[[0, -1, 0, 0], [1, 0, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]` (exact) |
| `rotate(30)` | `[[0.866025, -0.5, 0, 0], [0.5, 0.866025, 0, 0], …]` |
| `rotate(1e-9)` | `[[1, -1.74533e-11, 0, 0], [1.74533e-11, 1, 0, 0], …]` |
| `rotate([90, 0, 0])` | `[[1, 0, 0, 0], [0, 0, -1, 0], [0, 1, 0, 0], [0, 0, 0, 1]]` |
| `rotate([0, 0, 180])` | `[[-1, 0, 0, 0], [-0, -1, 0, 0], [-0, 0, 1, 0], [0, 0, 0, 1]]` (**negative zeros**) |
| `rotate([180, 0, 0])` | `[[1, 0, 0, 0], [0, -1, 0, 0], [0, 0, -1, 0], [0, 0, 0, 1]]` |
| `rotate([10, 20, 30])` | `[[0.813798, -0.44097, 0.378522, 0], [0.469846, 0.882564, 0.0180283, 0], [-0.34202, 0.163176, 0.925417, 0], [0, 0, 0, 1]]` (Rz·Ry·Rx) |
| `rotate([1, 2])` | same as `[1, 2, 0]` |
| `rotate(a = 45, v = [1, 1, 0])` | `[[0.853553, 0.146447, 0.5, 0], [0.146447, 0.853553, -0.5, 0], [-0.5, 0.5, 0.707107, 0], …]` (normalized axis) |
| `rotate(a = 90, v = [0, 0, -1])` | `[[0, 1, 0, 0], [-1, 0, 0, 0], [0, 0, 1, 0], …]` |
| `rotate(a = 90, v = [0, 0, 0])` | identity |
| `rotate("a")` / `rotate()` | identity + `WARNING: Problem converting rotate(a="a") parameter` / `rotate(a=undef)` |
| `scale(2)` | `[[2, 0, 0, 0], [0, 2, 0, 0], [0, 0, 2, 0], [0, 0, 0, 1]]` |
| `scale([1, 2])` | z = 1 |
| `scale([0 * -1, 1, 1])` | `[[-0, 0, 0, 0], [-0, 1, 0, 0], [-0, 0, 1, 0], [0, 0, 0, 1]]` (column 0 all `-0`) |
| `scale("x")` | identity + `Unable to convert scale("x") parameter to a number or a vec3 or vec2 of numbers` |
| `mirror([1, 0, 0])` | `[[-1, -0, -0, 0], [-0, 1, -0, 0], [-0, -0, 1, 0], [0, 0, 0, 1]]` |
| `mirror([1, 1, 0])` | `[[0, -1, -0, 0], [-1, 0, -0, 0], [-0, -0, 1, 0], [0, 0, 0, 1]]` |
| `mirror([1, 2, 3])` | `[[0.857143, -0.285714, -0.428571, 0], [-0.285714, 0.428571, -0.857143, 0], [-0.428571, -0.857143, -0.285714, 0], …]` |
| `mirror([0, 0, 0])` | identity; `mirror()` → same as `[1, 0, 0]` + `Unable to convert mirror(undef) parameter to a vec3 or vec2 of numbers` |
| `multmatrix([[1,0,0,1],[0,1,0,2],[0,0,1,3]])` (3×4) | padded with `[0, 0, 0, 1]` |
| `multmatrix([[2, 0], [0, 2]])` | `[[2, 0, 0, 0], [0, 2, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]` (padded with identity) |
| `multmatrix()` | identity |

Mirror: off-diagonal entries behave like `-2·nᵢ·nⱼ/|n|²` (yielding `-0` when the product is 0);
diagonal entries `1 - 2·nᵢ²/|n|²`. Axis-angle rotation: Rodrigues' formula with normalized axis.
`rotate` uses exact sine/cosine at multiples of 90° (see §5.3), hence the `-0` entries.

`resize` stays a dedicated node, with **spaceless** vectors and integer `auto` flags:
```
resize([2, 3, 4]) cube();                               → resize(newsize = [2,3,4], auto = [0,0,0], convexity = 0) {
resize([2, 0, 0], auto = true)                          → resize(newsize = [2,0,0], auto = [1,1,1], convexity = 0) {
resize(newsize = [2, 0, 0], auto = [true, false, true]) → resize(newsize = [2,0,0], auto = [1,0,1], convexity = 0) {
resize([2, 3]) square();                                → resize(newsize = [2,3,0], auto = [0,0,0], convexity = 0) {
resize([1.5, 2.5, 1/3], convexity = 2)                  → resize(newsize = [1.5,2.5,0.333333], auto = [0,0,0], convexity = 2) {
```

### 14.6 Color

`color(...)` → `color([r, g, b, a]) {` with `%g` components.

| source | CSG |
|---|---|
| `color("red")`, `color("#ff0000")`, `color("#f00")`, `color([1, 0, 0])` | `color([1, 0, 0, 1])` |
| `color("Red", 0.5)` (case-insensitive names) | `color([1, 0, 0, 0.5])` |
| `color("DarkOliveGreen")` | `color([0.333333, 0.419608, 0.184314, 1])` (components/255) |
| `color("#ff000080")`, `color("#f008")` | `color([1, 0, 0, 0.501961])`, `color([1, 0, 0, 0.533333])` |
| `color("transparent")` | `color([0, 0, 0, 0])` |
| `color([1, 0, 0], 0.25)`, `color("red", alpha = 0.25)` | `color([1, 0, 0, 0.25])` |
| `color([0.5, 0.5])` | `color([0.5, 0.5, 1, 1])` (missing components default to 1) |
| `color([1e-5, 0.5, 1/3, 2])` | `color([1e-05, 0.5, 0.333333, 2])` + `WARNING: color() expects numbers between 0.0 and 1.0. Value of 2.0 is out of range in file …` |
| `color("nosuchcolor")`, `color("#12345")` | `color([-1, -1, -1, -1])` + two-line warning (§1.1) |
| `color()`, `color(5)` | `color([-1, -1, -1, -1])`, no warning |
| `color("red") {}` | `color([1, 0, 0, 1]);` |

### 14.7 Booleans and Operations

```
union() {…}                difference() {…}             intersection() {…}
hull() {…}                 hull();                      fill() {…}
render(convexity = 1) {…}  (default 1; render(convexity = 0) → convexity = 0)
minkowski(convexity = 0) {…}   (default 0)
projection(cut = false, convexity = 0) {…}   projection(cut = true, convexity = 0)   projection(convexity = 3) → cut = false, convexity = 3
```
`roof()` requires `--enable=roof` (otherwise `Experimental builtin module 'roof' is not enabled` +
`Ignoring unknown module 'roof'`).

### 14.8 Extrusions

`linear_extrude`: parameter order `height, [v], [center], [twist], [slices], [segments], [scale], $fn, $fa, $fs, [convexity]`.

| source | CSG |
|---|---|
| `linear_extrude(10)` | `linear_extrude(height = 10, $fn = 0, $fa = 12, $fs = 2) {` |
| `linear_extrude()` | `height = 100` |
| `linear_extrude(height = 5, center = true, convexity = 10, twist = 90, slices = 20, scale = 0.5, $fn = 16)` | `linear_extrude(height = 5, center = true, twist = 90, slices = 20, scale = 0.5, $fn = 16, $fa = 12, $fs = 2, convexity = 10) {` |
| `linear_extrude(height = 5, scale = [1, 2], twist = -45)` | `linear_extrude(height = 5, twist = -45, scale = [1, 2], $fn = 0, $fa = 12, $fs = 2) {` |
| `linear_extrude(height = 2, center = false, twist = 0, scale = 1, convexity = 1, slices = 1)` | `linear_extrude(height = 2, slices = 1, $fn = 0, $fa = 12, $fs = 2) {` |
| `scale = [1, 1]` / `scale = [2, 2]` | omitted / `scale = 2` |
| `linear_extrude(5, segments = 3)` | `linear_extrude(height = 5, segments = 3, $fn = 0, $fa = 12, $fs = 2) {` |
| `linear_extrude(v = [0, 1, 1])` | `linear_extrude(height = 1.41421, v = [ 0, 0.707107, 0.707107], $fn = 0, $fa = 12, $fs = 2) {` |
| `linear_extrude(height = 2, v = [1, 0, 1])` | `linear_extrude(height = 2, v = [ 0.707107, 0, 0.707107], …` |
| `linear_extrude(height = 2, v = [0, 0, 2])` | `v` omitted |
| `linear_extrude(-1)` | `height = 0` (no warning) |

Rules: `center` only if `true`; `twist` only if ≠ 0; `slices`/`segments` only if explicitly passed;
`scale` only if ≠ [1, 1] (scalar if uniform); `convexity` only if > 1 (0, 1, -2 omitted); `v` (normalized, formatted
`[ x, y, z]` with a leading space after `[`) only if not parallel to Z; when `height` is omitted, height is `|v|`.

`rotate_extrude`: `rotate_extrude(angle = A, start = S, convexity = C, $fn, $fa, $fs) {`

| source | CSG |
|---|---|
| `rotate_extrude()` | `angle = 360, start = 180, convexity = 2` |
| `rotate_extrude(convexity = 1)` | `angle = 360, start = 180, convexity = 2` (convexity = max(2, value): 0, 1, -5 → 2; 3 → 3) |
| `rotate_extrude(angle = 360)`, `rotate_extrude(start = 0)` | `angle = 360, start = 0` |
| `rotate_extrude(angle = 180, convexity = 4, $fn = 30)` | `angle = 180, start = 0, convexity = 4, $fn = 30, …` |
| `rotate_extrude(start = 90, angle = -90)` | `angle = -90, start = 90` |
| `rotate_extrude(angle = 400)` | `angle = 360, start = 0` (clamped to 360) |

Default `start`: 180 when neither `angle` nor `start` is given, otherwise 0.

### 14.9 `offset`

| source | CSG |
|---|---|
| `offset(r = 1)`, `offset(1)`, `offset()` | `offset(r = 1, $fn = 0, $fa = 12, $fs = 2) {` (default r = 1) |
| `offset(delta = 1)` | `offset(delta = 1, chamfer = false, $fn = 0, $fa = 12, $fs = 2) {` |
| `offset(delta = 1, chamfer = true)` | `offset(delta = 1, chamfer = true, …` |
| `offset(r = 1, chamfer = true)` | `offset(r = 1, …` (chamfer ignored) |
| `offset(r = 1, delta = 2)` | `offset(r = 1, …` + `WARNING: Ignoring "delta" argument as "r" is defined too. in file …` |
| `offset(-1)` | `offset(r = -1, …` |

### 14.10 `import` and `surface`

```
import("data/tri.stl");  → import(file = "data/tri.stl", origin = [0, 0], scale = 1, center = false, convexity = 1, $fn = 0, $fa = 12, $fs = 2, timestamp = <mtime>);
import("x.svg", center = true, dpi = 96) → import(file = "x.svg", origin = [0, 0], dpi = 96, scale = 1, center = true, convexity = 1, $fn = 0, $fa = 12, $fs = 2, timestamp = 0);
import("x.dxf", layer = "a", origin = [1, 2], scale = 2) → import(file = "x.dxf", layer = "a", origin = [1, 2], scale = 2, center = false, convexity = 1, $fn = 0, $fa = 12, $fs = 2, timestamp = 0);
surface("data/surf.dat") → surface(file = "data/surf.dat", center = false, invert = false, timestamp = <mtime>);
surface(file = "data/surf.dat", center = true, invert = true, convexity = 2) → surface(file = "data/surf.dat", center = true, invert = true, timestamp = <mtime>);
```
`file` is the path as written. `timestamp` = file modification timestamp (quirky negative value on
Windows, e.g. `-4644214685`), `0` if the file does not exist; no warning for missing files in CSG export.
`dpi` appears only for `.svg` (when given), `layer` only when given. `surface`'s `convexity` is not printed.

### 14.11 Special Variables in Primitives

- `$fn = 10; circle();` → `circle($fn = 10, …)`; inside a module `$fn = 3; circle();` → `$fn = 3`.
- `sphere($fn = -3)` → `$fn = 0` + `WARNING: $fn negative - setting to 0 in file …`
- `sphere($fn = 2.5)` → `$fn = 2.5` (not rounded).
- `sphere($fa = 0.001, $fs = -1)` → `$fa = 0.01, $fs = 0.01` + `WARNING: $fs too small - clamping to 0.010000 in file …`
  and `WARNING: $fa too small - clamping to 0.010000 in file …` (in that order).
- `cylinder($fn = "x")` → `$fn = 0`, no warning.

---

## 15. Unspecified Details / Open Notes

- Exact internal algorithm for non-tail function and module recursion limits: depends on process stack
  size (e.g. 10,000 calls for `1 + nt(n - 1)`, far fewer for heavier expressions).
  The corpus avoids those cases except deterministic infinite-recursion checks.
- Seed conversion for non-integer or negative seeds in `rands` (`1.5` → `[0.7768]`, `-1` → `[0.111607]`).
- Unseeded `rands` is non-reproducible.
- Exact tolerance rule when counting elements of a decimal-step range
  (observed: `[0:0.1:1]` → 11, `[0:1/3:1]` → 4, `[0:0.2:1]` → 6).
- Exact `timestamp` value in `import`/`surface` (appears to be a quirky Windows file-time conversion).
- Exact numeric values of `textmetrics`/`fontmetrics` depend on the bundled fonts.
- `.stl`/`.off` export is covered in §16; `.3mf`, `.obj`, `.wrl`, `.svg`, `.dxf` are not covered yet.
- Exact binary snap grid used on 2D boolean results (§16.7).
- Exact message text when an existing output file is not writable.

---

## 16. Geometry Export (`-o file.stl`, `-o file.off`)

Observed via `openscad -o out.stl in.scad` (default backend is Manifold; `--backend=manifold` changes
nothing). Implemented in `scadr` (`crates/scadr`), verified by the `tests/compat/geometry` corpus
(see its README).

### 16.1 Format Selection

- Format is determined from the output file suffix, **case-insensitively** (`CUBE.STL` works), or from
  `--export-format <name>` (or `--export-format=<name>`), which overrides the suffix and is
  **case-sensitive** (`STL` is invalid). `--export-format asciistl -o x.off` writes ASCII STL into `x.off`.
- Valid names (suffixes and `--export-format`): `stl`, `asciistl`, `binstl`, `off`, `wrl`, `3mf`, `csg`,
  `dxf`, `svg`, `pdf`, `png`, `echo`, `ast`, `term`, `nef3`, `nefdbg`, `param`, `pov`, `obj`.
- Plain `stl` in OpenSCAD is **ASCII STL** (its help notes binary will become the default in the future).
  `scadr` does the same so it works as a drop-in replacement for `openscad` in existing scripts;
  `asciistl` and `binstl` are explicit in both. The GUI app exports binary or ASCII on choice.
- `-o -` writes to stdout (`--export-format` is required since an empty suffix is invalid).
- These checks run **before** reading and evaluating the input file (no `ECHO`), exit code 1:

| situation | stderr |
|---|---|
| unknown suffix (`-o a.xyz`) | `Invalid suffix xyz. Either add a valid suffix or specify one using the --export-format option.` |
| no suffix (`-o a`, `-o -`) | `Invalid suffix . Either add a valid suffix or specify one using the --export-format option.` |
| `--export-format foo` | `Unknown --export-format option 'foo'.  Use -h to list available options.` (two spaces after the period) |
| missing output directory | blank line, `'nodir' is not a directory for output file nodir\cube.stl - Skipping`, blank line |
| missing input file | `Can't open input file 'nosuch.scad'!` followed by a blank line |

In `scadr`, an OpenSCAD-valid format that is not supported yet (`3mf`, `svg`…) prints
`ERROR: export format '3mf' is not supported by scadr yet.` and exits with code 1.

### 16.2 Messages and Exit Codes

- `$preview` is `false` (whereas `.csg`/`.echo` set `true`).
- Stderr order: evaluation messages (`ECHO`, `WARNING`, `ERROR` + `TRACE`), then render warnings,
  then either the outcome line or the render summary (§16.3). Render warning example
  (`cube(5); square(3);`):
  ```
  WARNING: Mixing 2D and 3D objects is not supported in file cli_mixed_2d_3d.scad, line 3
  WARNING: Ignoring 2D child object for 3D operation in file cli_mixed_2d_3d.scad, line 3
  ```
  In `scadr` these come from `RenderOutput::diagnostics`.
- Nothing to export → `Current top level object is empty.`, **no file written**, exit code **1**. Happens
  on an empty file or `echo`-only file, `%`-only objects, `cube(0)`/`cube([0, 1, 1])`, a difference that
  subtracts everything, or an evaluation error before any object is produced.
- 2D result exported to STL/OFF → `Current top level object is not a 3D object.`, no file written,
  exit code 1. (Conversely, exporting a 3D object to `.svg`/`.dxf` prints
  `Current top level object is not a 2D object.`)
- Evaluation error (`assert`, recursion) → top-level objects completed before the error are rendered,
  the output file is written, exit code **0**.
- Parse error → same as `.csg`: no file written, exit code 1.
- On success: render summary on stderr, nothing on stdout, exit code 0.

### 16.3 Post-Render Summary

OpenSCAD prints two variants. When the result is a single object that did not pass through a multi-object
boolean (primitive, transforms, single-child `union()`, `render()`, `linear_extrude`, `rotate_extrude`,
`polyhedron`), it is a **PolySet**:

```
Geometries in cache: 1
Geometry cache size in bytes: 856
CGAL Polyhedrons in cache: 0
CGAL cache size in bytes: 0
Total rendering time: 0:00:00.002
Top level object is a 3D object (PolySet):
   Convex:       yes
   Facets:         6
```

`Facets:` counts polygons (6 for a cube); for already-triangulated PolySets (`linear_extrude`,
`rotate_extrude`, `polyhedron`) the line is `Triangles:` (`   Triangles:     12`). `Convex:` is `yes`/`no`.
Column alignment: `Convex:`/`Facets:`/`Triangles:` plus value right-aligned to 17 characters.

With multi-object unions (including the implicit top-level union), `difference`, `intersection`, `hull`,
or `minkowski`, the result is a **manifold** (note the 3 leading spaces before `Top level` as well):

```
Geometries in cache: 2
Geometry cache size in bytes: 1712
CGAL Polyhedrons in cache: 1
CGAL cache size in bytes: 4000
Total rendering time: 0:00:00.009
   Top level object is a 3D object (manifold):
   Status:     NoError
   Genus:      -1
   Vertices:       16
   Facets:         24
```

`Facets` = triangles, `Vertices` = manifold vertices (unwelded across non-manifold touches: two cubes
sharing an edge have duplicate vertices), `Genus` = `1 − χ/2` with χ = V − E + F without splitting
components (two disjoint cubes or a hollow cube → `-1`). Layout: `Status:`/`Genus:` padded to 12 chars,
`Vertices:`/`Facets:` padded to 10 chars + number right-aligned in 8 chars.
`Total rendering time` is `H:MM:SS.mmm`.

`scadr` prints a compatible subset: `Total rendering time` followed by the **manifold** summary
(Status `NoError`, Genus, Vertices = mesh vertices, Facets = triangles), omitting cache lines.
`compat-runner` strips these summary lines anyway.

### 16.4 ASCII STL

```
solid OpenSCAD_Model
  facet normal 0 0 1
    outer loop
      vertex 0 10 10
      vertex 10 0 10
      vertex 10 10 10
    endloop
  endfacet
  ...
endsolid OpenSCAD_Model
```

- Numbers: shortest decimal representation that round-trips identically as `double`, using fixed
  notation when the decimal exponent is strictly between −7 and 21, otherwise scientific **without `+`**:
  `0.000001`, `1e-7`, `-9e-9`, `110000000000000000000`, `1e21`, `1.1e22`, `0.000012345678901234568`,
  `-0.8137976813493738`. `-0` becomes `0`.
- Normal = `(b − a) × (c − a)` **divided** by its length (multiplying by the reciprocal changes the
  last digit in 25–50% of facets).
- On Windows lines end with CRLF; `scadr` writes LF (otherwise byte-identical for identical triangles,
  verified across 5,636 facets of `separatore.scad`).

### 16.5 Binary STL

80-byte header `OpenSCAD Model\n` zero-padded, `u32` triangle count, then for each triangle its normal
and 3 vertices as little-endian `f32` plus `0` attribute byte count.

### 16.6 OFF

```
OFF
8 6 0
0 0 0 
10 0 0 
...
4 4 5 7 6
4 2 3 1 0
```

- Coordinates with **6 significant digits** (`0.975452`), followed by a trailing space.
- A PolySet preserves polygons (a cube has 6 `4 …` faces); a manifold has triangles followed by face
  color (`3 0 2 6 249 215 44`). Vertices are not welded.
- `scadr` writes the same layout using full-precision coordinates (§16.4 format), triangles only, no
  colors. The reader (`mesh_io::read_off`) also accepts OpenSCAD's files (fan-triangulated polygons,
  colors ignored).

### 16.7 Geometry Observations (From the Corpus)

- A `polyhedron` with all faces wound inside-out is exported as-is (negative volume,
  `prim_polyhedron_reversed`).
- 2D results are snapped onto a fine binary grid: in `twod_square_circle` the theoretical vertex
  `6.299038105676658` becomes `6.299038104712963` (exact multiple of 2^-30) and the volume is
  `37.499999989984985` instead of `37.5`.
- `minkowski()` with a non-convex operand and a curved second operand is **non-deterministic** in
  OpenSCAD: triangulation and collinear vertices vary across runs (same volume to 1e-16). Using a cube
  as the second operand makes the result deterministic.
- Two cubes touching along an edge remain two shells with duplicate vertices (`bool_touching_edge`);
  two cubes sharing a face are merged (`bool_coplanar_union`, 20 triangles).
