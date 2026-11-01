# Comportamento osservato di OpenSCAD Nightly 2026.10.22

Riferimento per implementare in modo clean-room un interprete compatibile byte per byte.
Tutto ciò che segue è stato **osservato eseguendo il binario**
`C:\Program Files\OpenSCAD (Nightly)\openscad.exe` (versione `2026.10.22`, Windows 11) con
`openscad -o out.csg file.scad` e catturando stderr; nessun sorgente di OpenSCAD è stato letto.
Molti esempi sono anche casi del corpus `tests/compat/cases` (nome indicato tra parentesi).

Convenzioni: `input` → `output esatto`. Le righe di stderr sono mostrate dopo la normalizzazione
del corpus (CRLF → LF, percorso assoluto della cartella dei casi rimosso; vedi
`tests/compat/README.md`).

---

## 0. Esecuzione da riga di comando

- `openscad -o x.csg file.scad`: valuta il file, scrive l'albero CSG, stampa i messaggi su **stderr**.
  Su stdout non scrive nulla. Nessuna riga di statistiche con l'export `.csg`, `.echo`, `.ast`.
- `-o x.echo`: i messaggi (ECHO/WARNING/ERROR…) vanno **nel file** e non su stderr. Il contenuto è
  identico alle righe che con `.csg` vanno su stderr. Anche gli errori di parsing finiscono nel file.
- `-o x.stl` (render completo) aggiunge righe di statistiche su stderr, da filtrare (export della
  geometria, messaggi ed exit code in dettaglio: §16):
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
- Su Windows sia stderr sia i file scritti usano **CRLF**. Un `\n` dentro una stringa echeggiata
  diventa anch'esso CRLF.
- Alcune funzioni sui font stampano righe `Fontconfig error: Cannot load default config file: File not found`
  (due volte, alla prima inizializzazione dei font: `textmetrics`, `fontmetrics`; non `text()` in export CSG).
- `$preview` vale `true` con export `.csg`, `.echo`, `.ast`; vale `false` con export `.stl`.
- **Exit code**: `1` per errore di parsing (nessun file di output scritto); `0` in tutti gli altri casi,
  **anche dopo un `ERROR:` di valutazione** (assert fallito, ricorsione).
- Feature sperimentali (`--enable=<nome>` o `--enable=all`): `roof`, `input-driver-dbus`, `lazy-union`,
  `vertex-object-renderers-indexing`, `import-function`, `object-function`, `predictible-output`,
  `vector-swizzle`, `discretization-by-error`, `ai-features`, `unicode-identifiers`.

## 1. Formato dei messaggi (stderr)

### 1.1 Prefissi e posizione

| Tipo | Formato |
|---|---|
| echo | `ECHO: <argomenti>` |
| warning | `WARNING: <testo> in file <file>, line <n>` |
| errore | `ERROR: <testo> in file <file>, line <n>` |
| traccia | `TRACE: called by '<nome>' in file <file>, line <n>` / `TRACE: call of '<firma>' in file <file>, line <n>` |
| deprecazione | `DEPRECATED: <testo> in file <file>, line <n>` |
| parsing | `ERROR: Parser error: <testo> in file <file>, line <n>` seguito da `Can't parse file '<percorso assoluto>'!` |

- `<file>` è il percorso **relativo alla cartella del file principale**, con `/` come separatore
  (`in file lib/sub/subdefs.scad, line 3`), indipendente dalla cartella corrente. Solo
  `Can't parse file '...'!` usa il percorso assoluto con `\`.
- Le righe sono numerate da 1. La riga è quella dell'espressione/istruzione che genera il messaggio
  (per gli argomenti di un modulo: la riga dell'istanza; per i warning in una funzione: la riga della
  sotto-espressione).
- Alcuni messaggi hanno una **seconda riga** senza prefisso. Colore non valido:
  ```
  WARNING: Unable to parse color "nosuchcolor" in file csg_color_invalid.scad, line 1
  For a list of valid color names, see the <a href="open-window://colorlist"><b>Color List</b></a> window.
  ```
- Non c'è deduplica: lo stesso warning in un ciclo viene stampato a ogni iterazione.
- I warning non hanno righe TRACE; solo gli `ERROR:` di valutazione sono seguiti da TRACE.

### 1.2 Ordine dei messaggi

1. **Prima di tutto** (fase di parsing, per l'intero file e per gli include, anche per moduli mai chiamati):
   - `WARNING: Undefined escape sequence in file f.scad, line N` (al massimo **5 per letterale stringa**);
   - `DEPRECATED: Variable names starting with digits ("2b") will be removed in future releases. in file f.scad, line N` (uno per occorrenza);
   - `WARNING: "a" was assigned on line 2 but was overwritten in file f.scad, line 4` (riassegnazione nello
     stesso scope; emesso anche per i corpi dei moduli mai istanziati, in ordine di file).
2. Poi la valutazione. In ogni scope **tutte le assegnazioni vengono valutate per prime**, nell'ordine
   del file, poi le istanze di modulo (`echo`, `cube`, `for`, …) nell'ordine del file (vedi §9).
3. I warning prodotti valutando gli argomenti di un `echo(...)` precedono la riga `ECHO:` corrispondente.

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

### 1.3 Errori di valutazione

Un `ERROR:` (assert fallito, ricorsione) **interrompe tutta la valutazione**: nessun ECHO successivo.
Il CSG viene comunque scritto con i nodi top-level già completati prima dell'errore, exit code 0.

```openscad
echo("before"); cube(); assert(1 == 2); echo("after"); sphere();
```
stderr:
```
ECHO: "before"
ERROR: Assertion '(1 == 2)' failed in file err_assert_toplevel.scad, line 3
TRACE: called by 'assert' in file err_assert_toplevel.scad, line 3
```
CSG: `cube(size = [1, 1, 1], center = false);` (solo il cubo).

Traccia: dalla chiamata più interna verso l'esterno. Le istanze di moduli utente aggiungono
`call of 'm(x = -1)'` (argomenti con nome del parametro, valori formattati come echo) seguita da
`called by 'm'` alla riga dell'istanza:

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

Tracce lunghe: vengono mostrati i primi 12 frame, `TRACE:   *** Excluding N frames ***` (tre spazi
dopo `TRACE:`), e gli ultimi 12 frame (vedi `module_infinite_recursion`).

Messaggi di assert:

| input | prima riga |
|---|---|
| `assert(1 == 2);` | `ERROR: Assertion '(1 == 2)' failed in file …` |
| `assert(false, 5);` | `ERROR: Assertion 'false' failed: 5 in file …` |
| `assert(false, [1, "a"]);` | `ERROR: Assertion 'false' failed: [1, "a"] in file …` |
| `assert(x > 0, "x must be positive")` | `ERROR: Assertion '(x > 0)' failed: "x must be positive" in file …` |
| `assert(cond, undef)` | `ERROR: Assertion '<expr>' failed: undef in file …` |
| `assert();` | `ERROR: Assertion failed in file …` |

La condizione è stampata con lo stampatore di espressioni (§4); il messaggio è formattato come un
valore di echo (le stringhe tra virgolette).

### 1.4 Errori di parsing

| input (file completo) | stderr |
|---|---|
| `x = 1 +;` | `ERROR: Parser error: syntax error in file f.scad, line 1` + `Can't parse file '<abs>'!` |
| `echo(1);\n\n  a = @;` | `... syntax error in file f.scad, line 3` |
| `echo("abc);\n` | `ERROR: Parser error: Unterminated string in file f.scad, line 4` poi `ERROR: Parser error: syntax error in file f.scad, line 4` poi `Can't parse file ...` |
| `echo(1);\n/* unterminated\n` | `Unterminated comment ... line 5`, `syntax error ... line 5`, `Can't parse ...` |
| errore in un file incluso con `include` | `ERROR: Parser error: syntax error in file lib/badlib.scad, line 2` + `Can't parse file '<abs del file principale>'!` |
| errore in un file `use` | `ERROR: Parser error: syntax error in file lib/badlib.scad, line 2`, poi la valutazione **continua** (exit 0) |

Per stringhe/commenti non terminati la riga riportata è `(numero di '\n' nel file) + 3`, cioè
due oltre l'ultima riga.

Con errore di parsing: nessun file di output, nessun ECHO, exit code 1.

`cube()\nsphere();` (manca `;`) **non** è un errore: `sphere()` diventa figlio di `cube()` →
`WARNING: module cube() does not support child modules in file f.scad, line 2`.

Letterale numerico che va in overflow (`1e400`): il token viene **scartato in silenzio**:
`echo(1e400);` → `ECHO: ` ; `echo(2e400 + 1);` → `ECHO: 1` ; `echo(1e400, 5);` → syntax error.
Underflow: `1e-400` → `0`.

---

## 2. Formattazione dei numeri

Esistono **due formattatori diversi**.

### 2.1 Echo / `str()` / stampa espressioni / messaggi

Algoritmo osservato (coerente con tutti gli esempi):

1. `nan` → `nan` (anche `-(0/0)`), `+inf` → `inf`, `-inf` → `-inf`.
2. Lo zero (anche **-0**) → `0`.
3. Altrimenti si arrotonda il **valore binario esatto** a **6 cifre significative**, con i casi di
   pareggio esatto arrotondati **lontano da zero** (`1234565` → `1.23457e+6`, `123456.5` → `123457`).
   Se l'arrotondamento fa salire l'esponente, si usa l'esponente nuovo (`999999.5` → `1e+6`, `9.999995` → `10`).
4. Sia `e` l'esponente decimale (valore = d.ddddd × 10^e). Notazione **fissa** se `-5 <= e < 6`,
   altrimenti **esponenziale**.
5. Si tolgono gli zeri finali della mantissa e il punto se resta vuoto.
6. Esponente scritto come `e+N` / `e-N` **senza zeri di riempimento** (`1e+6`, `1e-7`, `1.5e+300`).

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
| `999999.5`, `9.999995`, `99999.95` | `1e+6`, `10`, `99999.9` (99999.95 in binario è 99999.94999…) |
| `1.0000005`, `0.1234565`, `0.9999995`, `1.000005` | `1`, `0.123456`, `1`, `1.00001` |
| `12.3456789e10`, `0.000001234565` | `1.23457e+11`, `1.23457e-6` |
| `5e-324`, `1.7976931348623157e308` | `4.94066e-324`, `1.79769e+308` |
| `-0`, `0 * -1`, `-0.0` | `0`, `0`, `0` |
| `1/0`, `-1/0`, `0/0`, `-(0/0)` | `inf`, `-inf`, `nan`, `nan` |
| `version_num()` | `2.0261e+7` |

Lo stesso formattatore è usato dentro vettori, `str()`, messaggi di assert, stampa di espressioni
(`function() 123456789` → `function() 1.23457e+8`), argomenti nelle TRACE.
Eccezioni con formato C: il warning `$fs too small - clamping to 0.010000` (`%f`) e
`color() ... Value of 2.0 is out of range` (`%.1f`).

### 2.2 Export CSG

I numeri nei parametri del CSG seguono **printf `%g`** (6 cifre significative), cioè:

- notazione fissa se `-4 <= e < 6`, altrimenti esponenziale;
- esponente con **almeno 2 cifre** (`1e+06`, `1e-05`, `1.5e-05`, `1e+100`);
- arrotondamento corretto del valore binario con pareggi **a pari** (`1234565` → `1.23456e+06`,
  `1234585` → `1.23458e+06`, `123456.5` → `123456`);
- **lo zero negativo si stampa `-0`**; `inf`, `-inf`, `nan`.

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

Differenza chiave rispetto all'echo: `echo([1e-5, 1e6])` → `ECHO: [0.00001, 1e+6]`, ma nel CSG
`1e-05`, `1e+06`; `echo(0 * -1)` → `0`, nel CSG `-0`. Le stringhe nel CSG (es. `text = "1e+6"` da
`text(str(1e6))`) restano come prodotte da `str()`.

---

## 3. Formattazione dei valori (echo / str)

| input | output |
|---|---|
| `echo(true, false, undef)` | `ECHO: true, false, undef` |
| `echo("abc")` | `ECHO: "abc"` |
| `echo("q\"q", "b\\s", "t\tt")` | `ECHO: "q"q", "b\s", "t	t"` (contenuto **grezzo**, nessun escape; tab reale) |
| `echo("line1\nline2")` | due righe: `ECHO: "line1` e `line2"` |
| `echo(["abc", "q\"q", "b\\s"])` | `ECHO: ["abc", "q"q", "b\s"]` (grezzo anche nei vettori) |
| `echo([])`, `echo([[]])` | `ECHO: []`, `ECHO: [[]]` |
| `echo([1, [2, [3]]])` | `ECHO: [1, [2, [3]]]` (separatore `", "`) |
| `echo([0:5])` | `ECHO: [0 : 1 : 5]` (il range mostra sempre il passo) |
| `echo([0:2:10], [0.5:1.5:3], [0:-1:-3])` | `ECHO: [0 : 2 : 10], [0.5 : 1.5 : 3], [0 : -1 : -3]` |
| `echo(function(x) x + 1)` | `ECHO: function(x) (x + 1)` |
| `echo(function(x, y = 2) x * y)` | `ECHO: function(x, y = 2) (x * y)` |
| `echo(a = 1, b = "s")` | `ECHO: a = 1, b = "s"` |
| `echo(1, b = 2, 3)` | `ECHO: 1, b = 2, 3` |
| `echo($fn = 3)` | `ECHO: $fn = 3` |
| `echo()` | `ECHO: ` (spazio finale) |
| oggetto (`textmetrics`, `object()`) | `{ a = 1; b = "x"; c = [1, 2]; }`, annidati `{ nominal = { ascent = 12.5733; descent = -2.9433; }; ... }` |

`str()` concatena le rappresentazioni **senza virgolette al primo livello** (dentro i vettori sì):

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

## 4. Stampa delle espressioni (funzioni letterali, assert)

Ogni operatore binario e il ternario sono racchiusi tra parentesi; gli unari no. I letterali stringa
**sono** con escape (`\"`, `\\`, `\n`, `\t`, `\r`; gli altri caratteri grezzi). I numeri usano il
formattatore echo. `+x` stampa `x`; `-<letterale>` è piegato in un letterale (`-(1)` → `-1`, `-0` → `0`).

| espressione in `function() <expr>` | stampa |
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
| `[1:3]`, `[1:2:3]` | `[1 : 3]`, `[1 : 2 : 3]` (il passo omesso resta omesso) |
| `[if (1) 2, 3, if (0) 4 else 5]` | `[if(1) (2), 3, if(0) (4) else (5)]` |
| `[each [1, 2], 3]` | `[each ([1, 2]), 3]` |
| `-x + !y - -1 + +3` | `(((-x + !y) - -1) + 3)` |
| `-(-x)`, `!!x` | `--x`, `!!x` |
| `c = "s\"q\n"` (default) | `function(c = "s\"q\n") c` |
| `1e6 + 0.1 + 1/3 + 1e-7 + 123456789 + 0x10 + 1.5e300` | `((((((1e+6 + 0.1) + (1 / 3)) + 1e-7) + 1.23457e+8) + 16) + 1.5e+300)` |

### 4.1 Precedenza (dalla più bassa alla più alta), ricavata dalle parentesi stampate

1. `? :` (associativo a destra)
2. `||`
3. `&&`
4. `==` `!=`
5. `<` `<=` `>` `>=`
6. `|`
7. `&`
8. `<<` `>>`
9. `+` `-`
10. `*` `/` `%`
11. unari prefissi `!` `-` `+` `~`
12. `^` (associativo a **destra**: `2 ^ 3 ^ 2` = 512; `-2 ^ 2` = `-(2 ^ 2)` = -4)
13. postfissi: indice `[]`, membro `.x`, chiamata `()`

Gli operatori binari diversi da `^` sono associativi a sinistra.
Esempi: `1 < 2 | 3` → `(1 < (2 | 3))`; `1 | 2 == 3` → `((1 | 2) == 3)`; `!a == b` → `(!a == b)`;
`-a ^ b` → `-(a ^ b)`; `1 | 2 & 3 << 4 >> 5 + ~6` → `(1 | (2 & ((3 << 4) >> (5 + ~6))))`.

---

## 5. Semantica numerica e operatori

### 5.1 Aritmetica

| input | echo |
|---|---|
| `7 / 2`, `2 ^ 10`, `2 ^ 0.5`, `2 ^ -1`, `0 ^ 0` | `3.5`, `1024`, `1.41421`, `0.5`, `1` |
| `2 ^ 3 ^ 2`, `-2 ^ 2`, `(-2) ^ 2`, `(-8) ^ (1/3)` | `512`, `-4`, `4`, `nan` |
| `1 + 2 * 3 ^ 2` | `19` |
| `5 % 3`, `-5 % 3`, `5 % -3`, `-5 % -3` | `2`, `-2`, `2`, `-2` (segno del dividendo, come `fmod`) |
| `5.5 % 2`, `-5.5 % 2`, `5 % 0`, `0 % 5` | `1.5`, `-1.5`, `nan`, `0` |
| `1/0`, `-1/0`, `0/0`, `1/(-0)` | `inf`, `-inf`, `nan`, `-inf` (lo zero negativo esiste) |
| `0/0 == 0/0`, `1/0 == 1/0` | `false`, `true` |
| `2 ^ 1000`, `fact(171)` | `1.07151e+301`, `inf` |

Tutti i numeri sono `f64`. Letterali: `.5`, `1.`, `1.e3`, `1e+3`, `1E3`, `00012` (=12), `0x10` (=16),
`0xff`; **`0XFF` non è esadecimale** ma un identificatore (DEPRECATED + variabile sconosciuta).
`9007199254740993` → `WARNING: Integer "9007199254740993" cannot be represented precisely in file …`.

### 5.2 Funzioni matematiche

| input | echo |
|---|---|
| `round(0.5), round(1.5), round(2.5), round(-0.5), round(-1.5), round(-2.5)` | `1, 2, 3, -1, -2, -3` (metà lontano da zero) |
| `round(-0.4)`, `round(0.49999999999999994)` | `0`, `1` (!) |
| `floor(-2.7), ceil(-2.1), floor(-0.5), ceil(-0.5)` | `-3, -2, -1, 0` |
| `abs(-0), sign(-2), sign(0), sign(-0)` | `0, -1, 0, 0` |
| `sqrt(-1), ln(0), ln(-1), log(100), log(2, 8)` | `nan, -inf, nan, 2, 3` (`log(b, x)` = log in base b) |
| `exp(1), pow(-8, 1/3), pow(0, 0)` | `2.71828, nan, 1` |
| `min(3, 1, 2), max([3, 1, 2]), min(1)` | `1, 3, 1` |
| `min(1, 1/0), max(-1/0, 2), min([1, 0/0]), max(0/0, 1)` | `1, 2, 1, nan` |

`round(0.49999999999999994)` = 1 indica un'implementazione tipo `floor(x + 0.5)` per x ≥ 0 e
`-floor(-x + 0.5)` per x < 0.

### 5.3 Trigonometria (gradi)

Valori **esatti** ai multipli di 30°/45°/90° (tutte le uguaglianze sono `true`):
`sin(30)==0.5`, `sin(150)==0.5`, `sin(-30)==-0.5`, `sin(210)==-0.5`, `sin(330)==-0.5`,
`cos(60)==0.5`, `cos(120)==-0.5`, `cos(240)==-0.5`, `cos(300)==0.5`, `cos(90)==0`, `sin(180)==0`,
`cos(270)==0`, `sin(360)==0`, `sin(390)==0.5`, `sin(720)==0`, `sin(45)==sqrt(2)/2`, `cos(45)==sqrt(0.5)`,
`tan(45)==1`, `tan(135)==-1`, `tan(60)==sqrt(3)`.
`tan(90)` → `inf`, `tan(-90)` → `-inf`, `tan(270)` → `-inf`, `tan(30)` → `0.57735`.
`asin(0.5)==30`, `acos(0.5)==60`, `atan(1)==45`, `atan2(1,1)==45`, `asin(1)==90`, `acos(0)==90`.

Segno degli zeri (osservato con `1/x`):

| x | 1/x | x | 1/x |
|---|---|---|---|
| `sin(0)` | `inf` | `cos(90)` | `inf` |
| `sin(180)` | `-inf` | `cos(270)` | `-inf` |
| `sin(-180)` | `-inf` | `cos(-90)` | `-inf` |
| `sin(360)` | `inf` | `cos(450)` | `inf` |
| `sin(-360)` | `inf` | `sin(0 * -1)` | `-inf` |
| `sin(540)` | `-inf` | `tan(180)`, `tan(-180)` | `-inf`, `-inf` |

Altri: `atan2(0, -1)` → `180`, `atan2(-0, -1)` → `-180`, `atan2(0, 0)` → `0`, `atan(1/0)` → `90`,
`asin(2)` → `nan`, `sin(1e10)` → `-0.984808`, `sin(89.9999)` → `1`.

### 5.4 Confronti e uguaglianza

| input | echo |
|---|---|
| `1 == 1.0, 1 == true, 0 == false, 1 == "1"` | `true, false, false, false` (tipi diversi: `==` è `false`, nessun warning) |
| `undef == undef, undef == 0, undef == [], [] == []` | `true, false, false, true` |
| `[1, [2]] == [1, [2]], [1, 2] != [1, 2, 3], [0/0] == [0/0]` | `true, true, false` |
| `"a" < "b", "abc" < "abd", "B" < "a", "a" < "ab", "é" < "z"` | `true, true, true, true, false` (confronto byte UTF-8) |
| `[1, 2] < [1, 3], [1] < [1, 2], [2] > [1, 5]` | `true, true, true` (lessicografico) |
| `true < false, false < true` | `false, true` |
| `true < 2` | `undef` + `WARNING: undefined operation (bool < number) in file …` |
| `1 < "a"`, `undef < 1`, `2 > true`, `[1] < 1` | `undef` + `undefined operation (number < string)` / `(undefined < number)` / `(number > bool)` / `(vector < number)` |

### 5.5 Verità (`!`, `if`, `? :`, `&&`, `||`)

Falsi: `false`, `0` (anche -0), `""`, `[]`, `undef`. **Veri**: `nan`, ogni numero ≠ 0, `"0"`, `[0]`,
`[[]]`, ogni range (anche vuoto), le funzioni.
`!(0/0)` → `false`; `0/0 ? 1 : 2` → `1`. `&&`/`||` restituiscono sempre un booleano:
`1 && 2` → `true`, `0 || ""` → `false`.
Nota: un range come `[0:-1:1]` emette il warning di range anche dentro `!`.

### 5.6 Vettori e matrici

| input | echo |
|---|---|
| `[1, 2] + [3, 4]`, `[1, 2] - [3, 4]` | `[4, 6]`, `[-2, -2]` |
| `[1, 2, 3] + [1]`, `[1, 2] - [1, 2, 3]` | `[2]`, `[0, 0]` (**troncato alla lunghezza minore**, nessun warning) |
| `[1, "a"] + [1, 2]` | `[2, undef]` (nessun warning per l'elemento) |
| `[] + []` | `[]` |
| `2 * [1, 2]`, `[1, 2] * 2`, `[2, 4] / 2`, `2 / [2, 4]` | `[2, 4]`, `[2, 4]`, `[1, 2]`, `[1, 0.5]` |
| `[1, [2, 3]] * 2`, `-[1, [2, 3]]` | `[2, [4, 6]]`, `[-1, [-2, -3]]` (ricorsivo) |
| `[1, 2] * [3, 4]` | `11` (prodotto scalare) |
| `[[1, 2], [3, 4]] * [1, 1]`, `[1, 1] * [[1, 2], [3, 4]]` | `[3, 7]`, `[4, 6]` |
| `[[1, 2], [3, 4]] * [[5, 6], [7, 8]]` | `[[19, 22], [43, 50]]` |
| `[[1, 2, 3]] * [[1], [2], [3]]` | `[[14]]` |
| `[1, 2] * [3]` | `undef` + `WARNING: vector*vector requires matching lengths (2 != 1) in file …` |
| `[] * []` | `undef` + `WARNING: Multiplication is undefined on empty vectors in file …` |
| `[[1, 2], [3, 4]] * [1, 2, 3]` | `undef` + `WARNING: matrix*vector requires matrix column count to match vector length (2 != 3) in file …` |
| `["a", 1] * [1, 1]` | `undef` + `WARNING: undefined vector*vector multiplication where first elements are types string and number in file …` |

### 5.7 Operazioni non definite

Risultato `undef` e `WARNING: undefined operation (<tipo1> <op> <tipo2>) in file …`, con tipi
`number`, `string`, `bool`, `vector`, `undefined`, `range`, `function`:

`"a" + "b"` → `(string + string)` (**niente concatenazione con +**); `"a" * 2` → `(string * number)`;
`[1, 2] + 1` → `(vector + number)`; `1 + [1, 2]` → `(number + vector)`; `[1] + undef` →
`(vector + undefined)`; `undef + 1` → `(undefined + number)`; `true + 1` → `(bool + number)`;
`true + true` → `(bool + bool)`; `[1, 2] % 2` → `(vector % number)`; `[1, 2] / [1, 2]` →
`(vector / vector)`. Unari: `-"a"` → `(-string)`, `-true` → `(-bool)`, `-undef` → `(-undefined)`.

### 5.8 Operatori bitwise (supportati)

Operandi convertiti a intero 64 bit con segno (troncamento; fuori range/`nan` → `-9223372036854775808`).

| input | echo |
|---|---|
| `1 \| 2, 6 & 3, ~0, ~1, 1 << 4, 256 >> 2, -1 >> 1, 12 & 10` | `3, 2, -1, -2, 16, 64, -1, 8` |
| `5.7 \| 0, -5.7 \| 0, ~1.5` | `5, -5, -2` |
| `1 << 63, 2^63 \| 0, 1e20 \| 0, (0/0) \| 0, ~(1/0)` | `-9.22337e+18, -9.22337e+18, -9.22337e+18, -9.22337e+18, 9.22337e+18` |
| `1 << 64` | `undef` + `WARNING: shift too large in file …` |
| `1 << -1` | `undef` + `WARNING: negative shift in file …` |
| `true \| 1`, `"a" \| 1` | `undef` + `undefined operation (bool \| number)` / `(string \| number)` |

---

## 6. Stringhe

- Le stringhe sono UTF-8; `len` e l'indicizzazione lavorano per **code point**:
  `len("é😀a")` → `3`, `"é😀a"[1]` → `"😀"`, `"abc"[2.9]` → `"c"`, `"abc"[5]`, `"abc"[-1]` → `undef`.
- Escape validi: `\n` `\t` `\r` `\"` `\\` `\xHH` (solo `\x01`–`\x7f`, esattamente 2 cifre),
  `\uHHHH` (4 cifre), `\UHHHHHH` (6 cifre). `\x00` e `\u0000` producono uno **spazio** (0x20).
- Escape non validi (`\q`, `\a`, `\0`, `\'`, `\v`, `\b`, `\f`, `\x7`, `\x80`, `\xff`, `\u12`, `\u{41}`,
  `\U00004`): la barra viene tolta e il resto rimane letterale (`"\x80"` → `"x80"`), con
  `WARNING: Undefined escape sequence in file f.scad, line N` (fase di parsing, max 5 per letterale).
- `chr(65)` → `"A"`; `chr(65, 66)` → `"AB"`; `chr([65, 66, [67]])` → `"ABC"` (appiattisce);
  `chr(65.7)` → `"A"` (tronca); `chr(0)`, `chr(-1)`, `chr(1114112)`, `chr("a")`, `chr()` → `""` senza warning.
- `ord("A")` → `65`; `ord("😀")` → `128512`; `ord("BA")` → `66` (primo carattere); `ord("")` → `undef`;
  `ord(65)` → `undef` + `WARNING: ord() parameter could not be converted: argument 0: expected string, found number (65) in file …`
  (`found undefined (undef)`, `found vector (["A"])`).
- `for (c = "ab")` itera sui caratteri; `[each "ab"]` → `["a", "b"]`; `concat("ab", "cd")` → `["ab", "cd"]`.

---

## 7. Indicizzazione e range

### 7.1 Indici

| input (`v = [10, 20, 30, 40]`) | echo |
|---|---|
| `v[0], v[3], v[1.5], v[1.9999], v[-0.5]` | `10, 40, 20, 20, 10` (indice **troncato verso zero**) |
| `v[-1], v[4], v[100]` | `undef` (nessun warning) |
| `v["a"], v[undef], v[true], v[[1]], v[0/0], v[1/0]` | `undef` (nessun warning) |
| `v.x, v.y, v.z, v.w` | `10, 20, 30, undef` (**`.w` non supportato** senza `vector-swizzle`) |
| `[1, 2].z, "abc".x, [1, 2, 3].xy, (5).x` | `undef` (tutti) |
| `5[0], undef[0], true[0], ""[0]` | `undef` |
| `[0:2:9][0], [0:2:9][1], [0:2:9][2], [0:2:9][3]` | `0, 2, 9, undef` (indice su range = inizio, passo, fine) |

Con `--enable=vector-swizzle`: `v.xy` → `[1, 2]`, `v.zyx` → `[3, 2, 1]`, `v.xxx` → `[1, 1, 1]`, `v.w` → `4`.

### 7.2 Range

- `[a:b]` = passo 1; i range vengono stampati come `[a : passo : b]`.
- **`[5:0]` non viene invertito**: è un range vuoto e quando viene creato emette
  `WARNING: begin is greater than the end, but step is positive in file …`;
  `[0:-1:5]` → `WARNING: begin is smaller than the end, but step is negative in file …`.
- Numero di elementi con passo decimale (con tolleranza): `[0:0.1:1]` → 11 elementi
  `[0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1]`; `[0:0.1:0.3]` → 4; `[0:1/3:1]` → 4;
  `[0:0.2:1]` → 6; `[1:1.5:5]` → `[1, 2.5, 4]`.
- `[3:3]` → `[3]`; `[2:-1:0]` → 2, 1, 0.
- Limite: un range con **≥ 1 000 000 elementi** non viene iterato (`for` o list comprehension):
  `WARNING: Bad range parameter in for statement: too many elements (1000000) in file …`, zero
  iterazioni. `[0:1:999998]` (999999 elementi) funziona. Passo 0: `[0:0:5]` →
  `too many elements (4294967295)`. `[0:1e-9:1]` → `(1000000001)`. Creare e stampare il range è
  sempre possibile (`echo([0:1:1e7])` → `ECHO: [0 : 1 : 1e+7]`).
- `len([0:5])` → `undef` + `WARNING: len() parameter could not be converted: argument 0: expected string, found range ([0 : 1 : 5]) in file …`.

### 7.3 `for` su valori non vettoriali

`for (i = 3)` → una iterazione con 3; `for (i = true)` → una iterazione con `true`;
`for (i = "ab")` → `"a"`, `"b"`; `for (i = undef)` → nessuna iterazione; `for (i = [1, [2, 3]])` → `1`, `[2, 3]`.

---

## 8. List comprehension

| input | echo |
|---|---|
| `[for (i = [0:3]) if (i % 2) i]` | `[1, 3]` |
| `[for (i = [0:3]) if (i % 2) i else -i]` | `[0, 1, -2, 3]` |
| `[for (i = [0:2]) let (j = i * i) j]` | `[0, 1, 4]` |
| `[each [1, 2], each [3:5], each "ab", each 7]` | `[1, 2, 3, 4, 5, "a", "b", 7]` |
| `[each undef], [each []]` | `[], []` |
| `[for (i = 0, j = 10; i < 3; i = i + 1, j = j - 1) [i, j]]` | `[[0, 10], [1, 9], [2, 8]]` |
| `[for (i = [1, 2], j = [3, 4]) [i, j]]` | `[[1, 3], [1, 4], [2, 3], [2, 4]]` (prima variabile esterna) |
| `[for (i = [0:1], j = [0:i]) [i, j]]` | `[[0, 0], [1, 0], [1, 1]]` (le variabili successive vedono le precedenti) |
| `[for (a = [1:2]) for (b = [1:a]) b]` | `[1, 1, 2]` |
| `[if (true) 1, if (false) 2, 3]` | `[1, 3]` |
| `[for (i = [0:5]) if (i < 2) "a" else if (i < 4) "b" else "c"]` | `["a", "a", "b", "b", "c", "c"]` |
| `[(1), (each [1])]` | `[1, 1]` |

`let` (espressione e modulo) è **sequenziale**: `let (a = 1, b = a + 1)` → b = 2.
Nome duplicato in `let`: vince il **primo**, con
`WARNING: Ignoring duplicate variable assignment "a" = 2 in file …` (il valore stampato è l'espressione).

---

## 9. Scope e ordine di valutazione

- In ogni scope (file, corpo di modulo, blocco di `if`/`for`/`let`, figli di un'istanza) le
  **assegnazioni vengono valutate tutte prima** delle istanze di modulo, nell'ordine del file.
- **Riassegnazione nello stesso scope**: rimane una sola variabile, nella **posizione della prima
  assegnazione** ma con l'**espressione dell'ultima**; warning in fase di parsing
  `WARNING: "a" was assigned on line 2 but was overwritten in file f.scad, line 4`.
  ```openscad
  a = 1; echo("a1", a); a = 2; echo("a2", a);     // ECHO: "a1", 2 / ECHO: "a2", 2
  z = 1; z = z + 1; echo(z);                       // z = z + 1 valutato dove c'era z = 1:
  // WARNING: "z" was assigned on line 1 but was overwritten in file f.scad, line 2
  // WARNING: Ignoring unknown variable "z" in file f.scad, line 2
  // WARNING: undefined operation (undefined + number) in file f.scad, line 2
  // ECHO: undef
  ```
  Una variabile definita in un file `include`-uto e riassegnata nel file principale **non** dà il warning.
- Uso prima dell'assegnazione: `b = c + 1; c = 5;` → `Ignoring unknown variable "c"` e b = `undef`.
- Variabile sconosciuta: `WARNING: Ignoring unknown variable "x" in file …` (valore `undef`).
  Funzione: `WARNING: Ignoring unknown function 'f' in file …`. Modulo:
  `WARNING: Ignoring unknown module 'm' in file …` (nessun nodo nel CSG).
- Le variabili definite in un blocco `if`/`for` non sono visibili fuori.
- Un'assegnazione in un modulo oscura la variabile esterna per **tutto** il corpo del modulo
  (anche prima della riga dell'assegnazione): `g = 1; module h() { echo(g); g = 10; }` → `10`.
- Le funzioni sono a **scope lessicale** per le variabili normali (`function addk(x) = x + k;`
  usa il `k` del punto di definizione anche se il chiamante ha un altro `k`).
- Le funzioni letterali catturano lo scope: `a = 1; f = function() a; a = 2; echo(f());` → `2`
  (perché la riassegnazione sostituisce l'espressione).
- Namespace separati per variabili, funzioni e moduli: `foo = 5; function foo() = 10; module foo() …`
  coesistono (`echo(foo, foo())` → `5, 10`).
- Funzioni e moduli sono visibili in tutto lo scope in cui sono definiti (hoisting): si possono
  chiamare prima della definizione. Definizioni annidate in un modulo non sono visibili fuori.
- **Variabili `$`** (scope dinamico): visibili nei moduli e nelle funzioni chiamati, e nei figli
  (`children()`) che vedono il valore del modulo che esegue `children()`:
  ```openscad
  module ch() { $y = "from ch"; children(); }
  ch() echo($y);                                   // "from ch"
  module setter() { $w = "setter"; children(); }
  $w = "top"; setter() echo($w);                   // "setter"
  function getfn() = $fn;
  module m() { $fn = 24; echo(getfn()); }          // 24
  echo(let ($fn = 6) getfn());                     // 6
  ```
- Valori predefiniti a livello di file: `$fn = 0`, `$fa = 12`, `$fs = 2`, `$t = 0`, `$preview = true`
  (export CSG), `$vpr = [55, 0, 25]`, `$vpt = [0, 0, 0]`, `$vpd = 140`, `$vpf = 22.5`, `PI = 3.14159…`.
  `$children` e `$parent_modules` a livello di file non esistono (`Ignoring unknown variable "$children"`).

---

## 10. `include` e `use`

| | `include <f>` | `use <f>` |
|---|---|---|
| esegue le istruzioni top-level (echo, geometria) | sì, come testo incollato | no |
| variabili top-level del file | visibili | **non** visibili (`Ignoring unknown variable`) |
| funzioni e moduli | visibili | visibili |
| le funzioni del file vedono le sue variabili | sì (ridefinibili dal file principale) | sì (`uses_private()` → 5) |
| variabili `$` del chiamante | sì | sì (`show_q()` vede `$q = "caller"`) |
| file mancante | `WARNING: Can't find include file 'lib/x.scad'. in file f.scad, line 1` | `WARNING: Can't open library 'lib/x.scad'. in file f.scad, line 2` |
| errore di parsing | errore fatale per tutto il file | errore stampato, il resto continua |

Con `include`, ridefinire `libvar = 1;` nel file principale cambia il valore usato anche dalle
funzioni della libreria (`libf(2)` → 2), senza warning. Il percorso è relativo al file che include.

---

## 11. Funzioni, moduli, argomenti

- Argomenti: posizionali e nominati si possono mescolare, anche **posizionali dopo nominati**
  (`g(b = 2, 1)` → `[1, 2]`). Parametri mancanti → `undef` (nessun warning).
- Troppi posizionali: `WARNING: Too many unnamed arguments supplied in file …` (gli extra ignorati).
- Nome sconosciuto: `WARNING: variable "c" not specified as parameter in file …`.
- Nome ripetuto: `WARNING: argument "a" supplied more than once in file …` (vince l'**ultimo**).
- Valori di default: valutati nello **scope di definizione**, **non** vedono gli altri parametri
  (`function f(a = 1, b = a)` → `b = undef` con `Ignoring unknown variable "a"`), ma vedono le `$`
  del chiamante. `x = 10; function g(x = x) = x;` → `g()` = 10.
- Builtin con arità sbagliata: `WARNING: abs() number of parameters does not match: expected 1, found 0 in file …`
  (`atan2()`/`pow()`/`lookup()`/`cross()`: `expected 2`; **`log()`: `expected 2`** anche se accetta 1;
  `rands()`: `expected 3 or 4`; `search()`: `expected between 2 and 4`; `min()`/`max()`: `expected at least 1`).
- Funzioni letterali: `f = function(x) x * 3; f(2)` → 6; chiusure annidate ok
  (`adder(5)(10)` → 15); `(function(x) x + 1)(1)` → 2. I builtin non sono valori
  (`is_function(len)` → `Ignoring unknown variable "len"`, `false`).
- `echo(...) expr`, `assert(...) expr`, `let(...) expr` sono espressioni; `echo` stampa prima di
  valutare `expr`.
- Moduli e funzioni rimossi: `assign`, `child`, `import_stl` → `Ignoring unknown module`.
- `children()`:
  - `$children` conta tutte le istruzioni figlie (anche `if`, `for`, `echo`): `e() { cube(); if (false) sphere(); for (…) cube(); echo("x"); }` → 4.
  - indici fuori range: `WARNING: Children index (5) out of bounds (1 children) in file …`; indice
    decimale troncato (`1.7` → 1); tipo errato: `WARNING: Bad parameter type ("a") for children, only accept: empty, number, vector, range in file …`.
- `parent_module(n)` / `$parent_modules`: contano solo i **moduli utente** (non `translate` ecc.).
  `parent_module(0)` = modulo corrente; oltre lo stack:
  `WARNING: Parent module index (2) greater than the number of modules on the stack in file …` → `undef`.
  `parent_module()` = `parent_module(1)`.

---

## 12. Funzioni builtin

Esistenti senza feature flag: `abs sign sin cos tan acos asin atan atan2 floor round ceil ln log pow
sqrt exp rands min max norm cross concat lookup str chr ord search version version_num parent_module
len is_undef is_bool is_num is_string is_list is_function is_object textmetrics fontmetrics dxf_dim
dxf_cross`.
Sperimentali (senza flag: `WARNING: Experimental builtin function 'object' is not enabled in file …`
seguito da `WARNING: Ignoring unknown function 'object' in file …`): `object`, `has_key`
(`object-function`), `import` come funzione (`import-function`).
Non esistenti: `keys`, `values`, `typeof`, `is_range`, `is_vector`, `sort`, `sum`, `reverse`…

| input | echo |
|---|---|
| `version(), version_num()` | `[2026, 10, 22], 2.0261e+7` |
| `is_num(0/0), is_num(1/0), is_list([0:1]), is_list("ab"), is_object(1)` | `false, true, false, false, false` |
| `len(undef)` / `len(5)` / `len(true)` | `undef` + `len() parameter could not be converted: argument 0: expected string, found undefined (undef)` / `number (5)` / `bool (true)` |
| `concat([1], [2, 3], 4, "ab", [[5]]), concat(), concat([0:2]), concat(undef, [1])` | `[1, 2, 3, 4, "ab", [5]], [], [[0 : 1 : 2]], [undef, 1]` |
| `lookup(1.5, t), lookup(-1, t), lookup(6, t), lookup(3, t)` con `t = [[0,0],[1,10],[2,20],[5,50]]` | `15, 0, 50, 30` (clamp agli estremi, interpolazione lineare) |
| `lookup(0.5, [[1, 10], [0, 0]]), lookup(1, [[1, 5]]), lookup(1, [])` | `5, 5, undef` |
| `norm([3, 4]), norm([]), norm([5])` | `5, 0, 5` |
| `norm([[1, 2]])` | `undef` + `WARNING: Incorrect arguments to norm() in file …` |
| `norm(5)` | `undef` + `norm() parameter could not be converted: argument 0: expected vector, found number (5)` |
| `cross([1, 2, 3], [4, 5, 6]), cross([1, 0], [0, 1])` | `[-3, 6, -3], 1` (2D → scalare) |
| `cross([1, 2], [3])` | `undef` + `WARNING: Invalid vector size of parameter for cross() in file …` |
| `min()` | `undef` + `min() number of parameters does not match: expected at least 1, found 0` |
| `min([])` | `undef` + `min() number of parameters does not match: expected at least 1 vector element, found 0` |
| `min([3, 1], [2])` | `undef` + `min() parameter could not be converted: argument 0: expected number, found vector ([3, 1])` |
| `max(1, "a")` | `undef` + `max() parameter could not be converted: argument 1: expected number, found string ("a")` |

### 12.1 `rands`

Con seme è deterministico e compatibile con `std::mt19937(seed)` + `generate_canonical<double, 53>`
(due estrazioni a 32 bit per numero, la seconda è la più significativa):
`rands(0, 1, 3, 42)` → `[0.796543, 0.183435, 0.779691]`; `rands(0, 1, 2, 0)` → `[0.592845, 0.844266]`;
`rands(0, 10, 4, 1)` → `[9.97185, 9.32557, 1.28124, 9.99041]`; `rands(-5, 5, 3, 123)` →
`[2.12955, -0.715291, 1.90885]`; `rands(5, 5, 2, 1)` → `[5, 5]`; `rands(0, 1, 0, 1)` → `[]`.
Se min > max vengono scambiati: `rands(1, 0, 1, 1)` → `[0.997185]`.
Semi non interi/negativi (`rands(0,1,1,1.5)` → `[0.7768]`, `rands(0,1,1,-1)` → `[0.111607]`):
conversione del seme non determinata.

### 12.2 `search`

| input | echo |
|---|---|
| `search("a", "banana")` | `[1]` |
| `search("an", "banana")` | `[1, 2]` (ogni carattere cercato separatamente, prima occorrenza) |
| `search("a", "banana", 0)` | `[[1, 3, 5]]` |
| `search("abc", "aabbcc", 0)` | `[[0, 1], [2, 3], [4, 5]]` |
| `search("na", "banana", 2)` | `[[2, 4], [1, 3]]` |
| `search("q", "abc")`, `search("", "abc")` | `[]`, `[]` |
| `search(3, [1, 2, 3, 3])` | `[2]` |
| `search([3], [1, 2, 3, 3], 0)` | `[[2, 3]]` |
| `search([1, 9], [1, 2, 3])` | `[0, []]` |
| `search([[1, "x"]], [[1, "x"], [2, "y"]])` | `[0]` |
| `search("x", [["a", 1], ["x", 2]])` | `[1]` (confronta la colonna 0) |
| `search(2, [["a", 1], ["x", 2]], 0, 1)` | `[1]` (colonna 1) |
| `search([1, 2], [[1, "a"], [2, "b"], [1, "c"]], 0)` | `[[0, 2], [1]]` |
| `search(["b"], ["abc", "b", "xb"], 0)` | `[[1]]` |
| `search("b", ["abc", "b", "xb"])` | `[]` + `WARNING: Invalid entry in search vector at index 0, required number of values in the entry: 1. Invalid entry: "abc" in file …` |

### 12.3 Oggetti (`--enable=object-function`) e metriche dei font

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
(non richiedono flag; font predefinito Liberation Sans, size 10).

---

## 13. Ricorsione e limiti

| situazione | risultato |
|---|---|
| ricorsione in coda (`n == 0 ? acc : f(n - 1, acc + 1)`) | iterativa: `tail(999999)` ok; con **1 000 000** iterazioni → errore |
| `function f(x) = f(x + 1);` | `ERROR: Recursion detected calling function 'f' in file f.scad, line 1` + `TRACE: called by 'f' … line 1` + `TRACE: called by 'echo' … line 2` |
| ricorsione non in coda `1 + nt(n - 1)` | `nt(9999)` ok, `nt(10000)` → `Recursion detected calling function 'nt'` (dipende dallo stack: con corpi più complessi il limite è più basso, es. `ERROR: Stack exhausted while trying to convert a vector to EchoString`) |
| moduli ricorsivi | limite dipendente dallo stack: ~4563 livelli con `if`, ~3000 con `translate` in mezzo; `ERROR: Recursion detected calling module 'mm' in file …` |
| `module m() m(); m();` | `ERROR: Recursion detected calling module 'm' …` + 12 TRACE + `TRACE:   *** Excluding N frames ***` + 12 TRACE |

Il messaggio di ricorsione è lo stesso sia per il limite delle iterazioni in coda sia per
l'esaurimento dello stack. Dopo l'errore la valutazione si ferma.

---

## 14. Export CSG (`-o file.csg`)

### 14.1 Formato generale

- Un nodo per riga; i nodi con figli: `nome(params) {` … `}`; senza figli: `nome(params);`.
- Indentazione con **un TAB per livello**.
- I nodi top-level sono scritti uno dopo l'altro **senza `group()` radice**. Il file termina con una
  riga vuota extra (`...;\r\n\r\n`); un albero vuoto produce il solo `\r\n`.
- Modificatori: il carattere (`%` o `#`) è scritto **all'inizio della riga, prima dei TAB**:
  ```
  multmatrix([[1, 0, 0, 1], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]) {
  %	sphere($fn = 0, $fa = 12, $fs = 2, r = 1, style = "orig");
  }
  ```
  `*` (disable): il nodo non compare. `!` (root): l'output contiene **solo** quel sottoalbero, senza
  modificatore (anche se è dentro un modulo); con più `!` vince il primo +
  `WARNING: More than one Root Modifier (!) in file f.scad, line 2`.
- Parametri: `nome = valore`, separati da `", "`; numeri in formato `%g` (§2.2), stringhe tra virgolette
  **senza escape**, vettori `[a, b]`, booleani `true`/`false`.
- `$fn`, `$fa`, `$fs` sono stampati per i nodi che li usano, con i valori effettivi (anche ereditati).

### 14.2 Struttura dell'albero

| sorgente | CSG |
|---|---|
| blocco `{ … }` (anche annidato, a qualunque livello) | **appiattito**, nessun nodo |
| `group() { cube(); }` | `group() {` … `}` |
| `union() {}` | `union();` |
| istanza di modulo utente `m();` | `group() { <figli> }`; senza figli `group();` |
| `for (…) …` | `group() { … }` (una lista piatta di tutte le iterazioni); vuoto: `group();` |
| `intersection_for (…) …` | `intersection() { … }`; vuoto: `intersection();` |
| `if (true) x;` / `if (c) … else …` | `group() { <ramo eseguito> }` |
| `if (false) x;` (senza else) | **nessun nodo** (anche dentro un modulo: `module m() { if (false) cube(); }` → `group();`) |
| `let (…) x;`, `echo(…) x;`, `assert(…) x;` | `group() { … }` |
| `children();` | `group() { <tutti i figli> }`; senza figli `group();` |
| `children(i)` valido | `group() { <figlio i> }`; fuori range: nessun nodo |
| istruzioni senza geometria (`echo(…);` da solo, assegnazioni) | nessun nodo |
| modulo builtin con figli vuoti | `multmatrix(...);` |
| modulo sconosciuto | nessun nodo |

Esempi (`csg_children_nesting`):
```
module c() children();      c();           →  group() {\n\tgroup();\n}
                            c() cube();    →  group() {\n\tgroup() {\n\t\tcube(...);\n\t}\n}
```

### 14.3 Primitive 3D

| sorgente | CSG |
|---|---|
| `cube();` | `cube(size = [1, 1, 1], center = false);` |
| `cube(2);` | `cube(size = [2, 2, 2], center = false);` |
| `cube([1, 2, 3], center = true);` | `cube(size = [1, 2, 3], center = true);` |
| `cube(-1);` | `cube(size = [-1, -1, -1], center = false);` (nessun warning) |
| `cube([1, 2]);` | `cube(size = [1, 1, 1], …)` + `WARNING: Unable to convert cube(size=[1, 2], ...) parameter to a number or a vec3 of numbers in file …` |
| `cube("a");` | default + `Unable to convert cube(size="a", ...) parameter …` |
| `cube(undef);`, `cube(center = "yes");` | default, nessun warning (`center` non booleano → `false`) |
| `cube(1, 2, 3);` | `cube(size = [1, 1, 1], center = false);` + `Too many unnamed arguments supplied` |
| `sphere();` | `sphere($fn = 0, $fa = 12, $fs = 2, r = 1, style = "orig");` |
| `sphere(d = 3);` | `… r = 1.5, style = "orig");` |
| `sphere(r = 1, d = 4);` | `… r = 2 …` + `WARNING: Ignoring radius variable "r" as diameter "d" is defined too. in file …` |
| `sphere(-1);` / `sphere("a");` | `r = -1` / `r = 1` (nessun warning) |
| `sphere(style = "octa");` | `… style = "octa");` |
| `sphere(style = "ico");` | `"orig"` + `WARNING: sphere(style="ico") is not "orig" or "octa", using "orig" in file …` |
| `sphere(style = 1);` | `"orig"` + `WARNING: (..., style=1) Invalid type: expected string, found number in file …` |
| `cylinder();` | `cylinder($fn = 0, $fa = 12, $fs = 2, h = 1, r1 = 1, r2 = 1, center = false);` |
| `cylinder(h = 2, d = 3, center = true);` | `… h = 2, r1 = 1.5, r2 = 1.5, center = true);` |
| `cylinder(5, 1, 2, true);` | posizionali `h, r1, r2, center`: `h = 5, r1 = 1, r2 = 2, center = true` |
| `cylinder(h = 2, d1 = 1, d2 = 2, $fn = 6);` | `cylinder($fn = 6, $fa = 12, $fs = 2, h = 2, r1 = 0.5, r2 = 1, center = false);` |
| `cylinder(h = 1, r = 1, r1 = 2);` | `r1 = 2, r2 = 1` + `WARNING: Cylinder parameters ambiguous in file …` (anche `h = 3, r = 1, r2 = 0`) |
| `cylinder(r = 1, d = 4, h = 1);` | `r1 = 2, r2 = 2` + `Ignoring radius variable "r" as diameter "d" is defined too.` (`"r1"`/`"d1"` analogo) |
| `cylinder(r = -1, h = 2);`, `cylinder(h = -2);` | valori negativi stampati così come sono |
| `polyhedron(points = P, faces = F);` | `polyhedron(points = P, faces = F, convexity = 1);` |
| `polyhedron();` | `polyhedron(points = [], faces = [], convexity = 1);` + `WARNING: Unable to convert points = undef to a vector of coordinates in file …` |

### 14.4 Primitive 2D

| sorgente | CSG |
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
- ordine fisso `text, size, spacing, font, direction, language, [script], halign, valign, $fn, $fa, $fs`;
- `script` è il codice ISO 15924 rilevato dal testo (`"Latn"`, `"Cyrl"`, `"Hani"`, `"Arab"`); se il
  testo contiene solo caratteri comuni (`"123"`, `" "`, `""`) il parametro **è omesso**;
  `script = "latin"` → `"Lati"`, `"arabic"` → `"Arab"` (prime 4 lettere, iniziale maiuscola);
- `direction` predefinita `"ltr"`, ma `"rtl"` per testo arabo; `language` predefinita `"en"`;
- `text(5)` → `text = ""` + `WARNING: text(..., text=5) Invalid type: expected string, found number in file …`;
- `text("a\"b\\c")` → `text = "a"b\c"` (nessun escape).

### 14.5 Trasformazioni → `multmatrix`

`translate`, `rotate`, `scale`, `mirror`, `multmatrix` diventano tutte
`multmatrix([[a, b, c, d], [e, f, g, h], [i, j, k, l], [0, 0, 0, 1]]) {` (4×4, righe).

| sorgente | matrice |
|---|---|
| `translate([1, 2, 3])` | `[[1, 0, 0, 1], [0, 1, 0, 2], [0, 0, 1, 3], [0, 0, 0, 1]]` |
| `translate([1, 2])` | z = 0 |
| `translate(5)`, `translate("a")`, `translate([1,2,3,4])`, `translate()` | identità + `WARNING: Unable to convert translate(5) parameter to a vec3 or vec2 of numbers in file …` (`translate(undef)` per l'ultimo) |
| `translate([0 * -1, 0, 0])` | elemento di traslazione stampato `0` (non `-0`) |
| `rotate(90)` | `[[0, -1, 0, 0], [1, 0, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]` (esatta) |
| `rotate(30)` | `[[0.866025, -0.5, 0, 0], [0.5, 0.866025, 0, 0], …]` |
| `rotate(1e-9)` | `[[1, -1.74533e-11, 0, 0], [1.74533e-11, 1, 0, 0], …]` |
| `rotate([90, 0, 0])` | `[[1, 0, 0, 0], [0, 0, -1, 0], [0, 1, 0, 0], [0, 0, 0, 1]]` |
| `rotate([0, 0, 180])` | `[[-1, 0, 0, 0], [-0, -1, 0, 0], [-0, 0, 1, 0], [0, 0, 0, 1]]` (**zeri negativi**) |
| `rotate([180, 0, 0])` | `[[1, 0, 0, 0], [0, -1, 0, 0], [0, 0, -1, 0], [0, 0, 0, 1]]` |
| `rotate([10, 20, 30])` | `[[0.813798, -0.44097, 0.378522, 0], [0.469846, 0.882564, 0.0180283, 0], [-0.34202, 0.163176, 0.925417, 0], [0, 0, 0, 1]]` (Rz·Ry·Rx) |
| `rotate([1, 2])` | come `[1, 2, 0]` |
| `rotate(a = 45, v = [1, 1, 0])` | `[[0.853553, 0.146447, 0.5, 0], [0.146447, 0.853553, -0.5, 0], [-0.5, 0.5, 0.707107, 0], …]` (asse normalizzato) |
| `rotate(a = 90, v = [0, 0, -1])` | `[[0, 1, 0, 0], [-1, 0, 0, 0], [0, 0, 1, 0], …]` |
| `rotate(a = 90, v = [0, 0, 0])` | identità |
| `rotate("a")` / `rotate()` | identità + `WARNING: Problem converting rotate(a="a") parameter` / `rotate(a=undef)` |
| `scale(2)` | `[[2, 0, 0, 0], [0, 2, 0, 0], [0, 0, 2, 0], [0, 0, 0, 1]]` |
| `scale([1, 2])` | z = 1 |
| `scale([0 * -1, 1, 1])` | `[[-0, 0, 0, 0], [-0, 1, 0, 0], [-0, 0, 1, 0], [0, 0, 0, 1]]` (colonna 0 tutta `-0`) |
| `scale("x")` | identità + `Unable to convert scale("x") parameter to a number or a vec3 or vec2 of numbers` |
| `mirror([1, 0, 0])` | `[[-1, -0, -0, 0], [-0, 1, -0, 0], [-0, -0, 1, 0], [0, 0, 0, 1]]` |
| `mirror([1, 1, 0])` | `[[0, -1, -0, 0], [-1, 0, -0, 0], [-0, -0, 1, 0], [0, 0, 0, 1]]` |
| `mirror([1, 2, 3])` | `[[0.857143, -0.285714, -0.428571, 0], [-0.285714, 0.428571, -0.857143, 0], [-0.428571, -0.857143, -0.285714, 0], …]` |
| `mirror([0, 0, 0])` | identità; `mirror()` → come `[1, 0, 0]` + `Unable to convert mirror(undef) parameter to a vec3 or vec2 of numbers` |
| `multmatrix([[1,0,0,1],[0,1,0,2],[0,0,1,3]])` (3×4) | completata con `[0, 0, 0, 1]` |
| `multmatrix([[2, 0], [0, 2]])` | `[[2, 0, 0, 0], [0, 2, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]` (completata con l'identità) |
| `multmatrix()` | identità |

Mirror: gli elementi fuori diagonale si comportano come `-2·nᵢ·nⱼ/|n|²` (quindi `-0` quando il
prodotto è 0); diagonale `1 - 2·nᵢ²/|n|²`. Rotazione con asse: formula di Rodrigues con asse
normalizzato. `rotate` usa seno/coseno esatti a multipli di 90° (vedi §5.3), da cui i `-0`.

`resize` resta un nodo proprio, con vettori **senza spazi** e `auto` come interi:
```
resize([2, 3, 4]) cube();                               → resize(newsize = [2,3,4], auto = [0,0,0], convexity = 0) {
resize([2, 0, 0], auto = true)                          → resize(newsize = [2,0,0], auto = [1,1,1], convexity = 0) {
resize(newsize = [2, 0, 0], auto = [true, false, true]) → resize(newsize = [2,0,0], auto = [1,0,1], convexity = 0) {
resize([2, 3]) square();                                → resize(newsize = [2,3,0], auto = [0,0,0], convexity = 0) {
resize([1.5, 2.5, 1/3], convexity = 2)                  → resize(newsize = [1.5,2.5,0.333333], auto = [0,0,0], convexity = 2) {
```

### 14.6 Colore

`color(...)` → `color([r, g, b, a]) {` con componenti `%g`.

| sorgente | CSG |
|---|---|
| `color("red")`, `color("#ff0000")`, `color("#f00")`, `color([1, 0, 0])` | `color([1, 0, 0, 1])` |
| `color("Red", 0.5)` (nomi case-insensitive) | `color([1, 0, 0, 0.5])` |
| `color("DarkOliveGreen")` | `color([0.333333, 0.419608, 0.184314, 1])` (componenti/255) |
| `color("#ff000080")`, `color("#f008")` | `color([1, 0, 0, 0.501961])`, `color([1, 0, 0, 0.533333])` |
| `color("transparent")` | `color([0, 0, 0, 0])` |
| `color([1, 0, 0], 0.25)`, `color("red", alpha = 0.25)` | `color([1, 0, 0, 0.25])` |
| `color([0.5, 0.5])` | `color([0.5, 0.5, 1, 1])` (componenti mancanti = 1) |
| `color([1e-5, 0.5, 1/3, 2])` | `color([1e-05, 0.5, 0.333333, 2])` + `WARNING: color() expects numbers between 0.0 and 1.0. Value of 2.0 is out of range in file …` |
| `color("nosuchcolor")`, `color("#12345")` | `color([-1, -1, -1, -1])` + warning su due righe (§1.1) |
| `color()`, `color(5)` | `color([-1, -1, -1, -1])`, nessun warning |
| `color("red") {}` | `color([1, 0, 0, 1]);` |

### 14.7 Booleane e operazioni

```
union() {…}                difference() {…}             intersection() {…}
hull() {…}                 hull();                      fill() {…}
render(convexity = 1) {…}  (default 1; render(convexity = 0) → convexity = 0)
minkowski(convexity = 0) {…}   (default 0)
projection(cut = false, convexity = 0) {…}   projection(cut = true, convexity = 0)   projection(convexity = 3) → cut = false, convexity = 3
```
`roof()` richiede `--enable=roof` (altrimenti `Experimental builtin module 'roof' is not enabled` +
`Ignoring unknown module 'roof'`).

### 14.8 Estrusioni

`linear_extrude`: ordine `height, [v], [center], [twist], [slices], [segments], [scale], $fn, $fa, $fs, [convexity]`.

| sorgente | CSG |
|---|---|
| `linear_extrude(10)` | `linear_extrude(height = 10, $fn = 0, $fa = 12, $fs = 2) {` |
| `linear_extrude()` | `height = 100` |
| `linear_extrude(height = 5, center = true, convexity = 10, twist = 90, slices = 20, scale = 0.5, $fn = 16)` | `linear_extrude(height = 5, center = true, twist = 90, slices = 20, scale = 0.5, $fn = 16, $fa = 12, $fs = 2, convexity = 10) {` |
| `linear_extrude(height = 5, scale = [1, 2], twist = -45)` | `linear_extrude(height = 5, twist = -45, scale = [1, 2], $fn = 0, $fa = 12, $fs = 2) {` |
| `linear_extrude(height = 2, center = false, twist = 0, scale = 1, convexity = 1, slices = 1)` | `linear_extrude(height = 2, slices = 1, $fn = 0, $fa = 12, $fs = 2) {` |
| `scale = [1, 1]` / `scale = [2, 2]` | omesso / `scale = 2` |
| `linear_extrude(5, segments = 3)` | `linear_extrude(height = 5, segments = 3, $fn = 0, $fa = 12, $fs = 2) {` |
| `linear_extrude(v = [0, 1, 1])` | `linear_extrude(height = 1.41421, v = [ 0, 0.707107, 0.707107], $fn = 0, $fa = 12, $fs = 2) {` |
| `linear_extrude(height = 2, v = [1, 0, 1])` | `linear_extrude(height = 2, v = [ 0.707107, 0, 0.707107], …` |
| `linear_extrude(height = 2, v = [0, 0, 2])` | `v` omesso |
| `linear_extrude(-1)` | `height = 0` (nessun warning) |

Regole: `center` solo se `true`; `twist` solo se ≠ 0; `slices`/`segments` solo se specificati;
`scale` solo se ≠ [1, 1] (scalare se uniforme); `convexity` solo se > 1 (0, 1, -2 omessi); `v` (normalizzato, formato
`[ x, y, z]` con uno spazio dopo `[`) solo se non parallelo a Z; senza `height` l'altezza è `|v|`.

`rotate_extrude`: `rotate_extrude(angle = A, start = S, convexity = C, $fn, $fa, $fs) {`

| sorgente | CSG |
|---|---|
| `rotate_extrude()` | `angle = 360, start = 180, convexity = 2` |
| `rotate_extrude(convexity = 1)` | `angle = 360, start = 180, convexity = 2` (convexity = max(2, valore): 0, 1, -5 → 2; 3 → 3) |
| `rotate_extrude(angle = 360)`, `rotate_extrude(start = 0)` | `angle = 360, start = 0` |
| `rotate_extrude(angle = 180, convexity = 4, $fn = 30)` | `angle = 180, start = 0, convexity = 4, $fn = 30, …` |
| `rotate_extrude(start = 90, angle = -90)` | `angle = -90, start = 90` |
| `rotate_extrude(angle = 400)` | `angle = 360, start = 0` (limitato a 360) |

`start` predefinito: 180 se né `angle` né `start` sono dati, altrimenti 0.

### 14.9 `offset`

| sorgente | CSG |
|---|---|
| `offset(r = 1)`, `offset(1)`, `offset()` | `offset(r = 1, $fn = 0, $fa = 12, $fs = 2) {` (default r = 1) |
| `offset(delta = 1)` | `offset(delta = 1, chamfer = false, $fn = 0, $fa = 12, $fs = 2) {` |
| `offset(delta = 1, chamfer = true)` | `offset(delta = 1, chamfer = true, …` |
| `offset(r = 1, chamfer = true)` | `offset(r = 1, …` (chamfer ignorato) |
| `offset(r = 1, delta = 2)` | `offset(r = 1, …` + `WARNING: Ignoring "delta" argument as "r" is defined too. in file …` |
| `offset(-1)` | `offset(r = -1, …` |

### 14.10 `import` e `surface`

```
import("data/tri.stl");  → import(file = "data/tri.stl", origin = [0, 0], scale = 1, center = false, convexity = 1, $fn = 0, $fa = 12, $fs = 2, timestamp = <mtime>);
import("x.svg", center = true, dpi = 96) → import(file = "x.svg", origin = [0, 0], dpi = 96, scale = 1, center = true, convexity = 1, $fn = 0, $fa = 12, $fs = 2, timestamp = 0);
import("x.dxf", layer = "a", origin = [1, 2], scale = 2) → import(file = "x.dxf", layer = "a", origin = [1, 2], scale = 2, center = false, convexity = 1, $fn = 0, $fa = 12, $fs = 2, timestamp = 0);
surface("data/surf.dat") → surface(file = "data/surf.dat", center = false, invert = false, timestamp = <mtime>);
surface(file = "data/surf.dat", center = true, invert = true, convexity = 2) → surface(file = "data/surf.dat", center = true, invert = true, timestamp = <mtime>);
```
`file` è il percorso come scritto. `timestamp` = data di modifica del file (valore negativo strano,
es. `-4644214685`), `0` se il file non esiste; nessun warning per file mancanti in export CSG.
`dpi` compare solo per `.svg` (se dato), `layer` solo se dato. `convexity` di `surface` non è stampata.

### 14.11 Variabili speciali nelle primitive

- `$fn = 10; circle();` → `circle($fn = 10, …)`; in un modulo `$fn = 3; circle();` → `$fn = 3`.
- `sphere($fn = -3)` → `$fn = 0` + `WARNING: $fn negative - setting to 0 in file …`
- `sphere($fn = 2.5)` → `$fn = 2.5` (non arrotondato).
- `sphere($fa = 0.001, $fs = -1)` → `$fa = 0.01, $fs = 0.01` + `WARNING: $fs too small - clamping to 0.010000 in file …`
  e `WARNING: $fa too small - clamping to 0.010000 in file …` (in quest'ordine).
- `cylinder($fn = "x")` → `$fn = 0`, nessun warning.

---

## 15. Cose non determinate / note aperte

- L'algoritmo interno esatto dei limiti di ricorsione non in coda e dei moduli: dipende dallo stack
  del processo (es. 10000 chiamate per `1 + nt(n - 1)`, molte meno con espressioni più complesse).
  Il corpus evita questi casi tranne quelli deterministici.
- Conversione dei semi non interi o negativi di `rands` (`1.5` → `[0.7768]`, `-1` → `[0.111607]`).
- Senza seme `rands` non è riproducibile.
- La regola esatta di tolleranza nel conteggio degli elementi di un range con passo decimale
  (osservato: `[0:0.1:1]` → 11, `[0:1/3:1]` → 4, `[0:0.2:1]` → 6).
- Il valore `timestamp` di `import`/`surface` (sembra una conversione errata della data del file).
- I valori numerici di `textmetrics`/`fontmetrics` dipendono dai font inclusi in OpenSCAD.
- L'export `.stl`/`.off` è descritto in §16; `.3mf`, `.obj`, `.wrl`, `.svg`, `.dxf` non sono coperti.
- La regola esatta della griglia su cui vengono arrotondati i risultati 2D (§16.7).
- Il testo esatto dei messaggi quando il file di output esiste ma non è scrivibile.

---

## 16. Export della geometria (`-o file.stl`, `-o file.off`)

Osservato con `openscad -o out.stl in.scad` (il backend predefinito è Manifold; `--backend=manifold`
non cambia nulla). Implementato in `scadr` (`crates/scadr`), verificato dal corpus
`tests/compat/geometry` (vedi il suo README).

### 16.1 Scelta del formato

- Il formato viene dal suffisso del file di output, **senza distinzione tra maiuscole e minuscole**
  (`CUBE.STL` va bene), oppure da `--export-format <nome>` (o `--export-format=<nome>`), che vince sul
  suffisso ed è **sensibile alle maiuscole** (`STL` non è valido). `--export-format asciistl -o x.off`
  scrive un STL ASCII nel file `x.off`.
- Nomi validi (suffissi e `--export-format`): `stl`, `asciistl`, `binstl`, `off`, `wrl`, `3mf`, `csg`,
  `dxf`, `svg`, `pdf`, `png`, `echo`, `ast`, `term`, `nef3`, `nefdbg`, `param`, `pov`, `obj`.
- `stl` in OpenSCAD è **STL ASCII** (l'help dice che il binario diventerà il default in futuro).
  `scadr` fa lo stesso, per poter sostituire `openscad` negli script esistenti; `asciistl` e
  `binstl` sono espliciti in entrambi. L'app esporta in binario o ASCII a scelta.
- `-o -` scrive su stdout (serve `--export-format`, il suffisso vuoto non è valido).
- Questi controlli avvengono **prima** di leggere e valutare il file (nessun `ECHO`), exit code 1:

| situazione | stderr |
|---|---|
| suffisso sconosciuto (`-o a.xyz`) | `Invalid suffix xyz. Either add a valid suffix or specify one using the --export-format option.` |
| nessun suffisso (`-o a`, `-o -`) | `Invalid suffix . Either add a valid suffix or specify one using the --export-format option.` |
| `--export-format foo` | `Unknown --export-format option 'foo'.  Use -h to list available options.` (due spazi dopo il punto) |
| cartella di output inesistente | riga vuota, `'nodir' is not a directory for output file nodir\cube.stl - Skipping`, riga vuota |
| file di input inesistente | `Can't open input file 'nosuch.scad'!` seguito da una riga vuota |

In `scadr` un formato valido per OpenSCAD ma non ancora supportato (`3mf`, `svg`…) dà
`ERROR: export format '3mf' is not supported by scadr yet.` ed exit code 1.

### 16.2 Messaggi ed exit code

- `$preview` vale `false` (con `.csg`/`.echo` vale `true`).
- Ordine su stderr: messaggi della valutazione (`ECHO`, `WARNING`, `ERROR` + `TRACE`), poi i warning
  del render, poi la riga di esito oppure il riepilogo (§16.3). Esempio di warning del render
  (`cube(5); square(3);`):
  ```
  WARNING: Mixing 2D and 3D objects is not supported in file cli_mixed_2d_3d.scad, line 3
  WARNING: Ignoring 2D child object for 3D operation in file cli_mixed_2d_3d.scad, line 3
  ```
  In `scadr` sono gli elementi di `RenderOutput::warnings`, stampati come `WARNING: <testo>`.
- Niente da esportare → `Current top level object is empty.`, **nessun file**, exit code **1**. Succede
  con un file vuoto o solo `echo`, solo oggetti `%`, `cube(0)`/`cube([0, 1, 1])`, una differenza che
  toglie tutto, un errore di valutazione prima di qualunque oggetto.
- Risultato 2D esportato in STL/OFF → `Current top level object is not a 3D object.`, nessun file,
  exit code 1. (Al contrario, un oggetto 3D esportato in `.svg`/`.dxf` dà
  `Current top level object is not a 2D object.`)
- Errore di valutazione (`assert`, ricorsione) → si renderizzano gli oggetti top-level completati prima
  dell'errore, il file viene scritto, exit code **0**.
- Errore di parsing → come per `.csg`: nessun file, exit code 1.
- Su successo: riepilogo su stderr, niente su stdout, exit code 0.

### 16.3 Riepilogo dopo il render

OpenSCAD stampa due forme. Quando il risultato è un singolo oggetto che non passa da una booleana tra
più oggetti (primitiva, trasformazioni, `union()` con un figlio, `render()`, `linear_extrude`,
`rotate_extrude`, `polyhedron`) è un **PolySet**:

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

`Facets:` conta i poligoni (6 per un cubo); per i PolySet già triangolati (`linear_extrude`,
`rotate_extrude`, `polyhedron`) la riga è `Triangles:` (`   Triangles:     12`). `Convex:` vale `yes`/`no`.
Le colonne: `Convex:`/`Facets:`/`Triangles:` più il valore allineato a destra su 17 caratteri.

Con unioni di più oggetti (anche quella implicita del livello superiore), `difference`,
`intersection`, `hull`, `minkowski` il risultato è un **manifold** (notare i 3 spazi anche prima di
`Top level`):

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

`Facets` = triangoli, `Vertices` = vertici del manifold (non saldati: due cubi che si toccano su uno
spigolo hanno vertici duplicati), `Genus` = `1 − χ/2` con χ = V − E + F, senza tenere conto dei pezzi
(due cubi separati o un cubo con una cavità → `-1`). Formato: `Status:`/`Genus:` riempiti a 12
caratteri, `Vertices:`/`Facets:` riempiti a 10 + numero allineato a destra su 8.
`Total rendering time` è `H:MM:SS.mmm`.

`scadr` stampa un sottoinsieme compatibile: `Total rendering time` e sempre la forma **manifold**
(Status `NoError`, Genus, Vertices = vertici della mesh, Facets = triangoli), senza le righe della
cache. Il compat-runner scarta comunque queste righe.

### 16.4 STL ASCII

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

- Numeri: le cifre più corte che si rileggono identiche come `double`, in notazione normale se
  l'esponente decimale è tra −7 e 21 esclusi, altrimenti esponenziale **senza `+`**: `0.000001`,
  `1e-7`, `-9e-9`, `110000000000000000000`, `1e21`, `1.1e22`, `0.000012345678901234568`,
  `-0.8137976813493738`. `-0` diventa `0`.
- Normale = `(b − a) × (c − a)` **diviso** per la sua lunghezza (moltiplicare per il reciproco cambia
  l'ultima cifra nel 25–50% delle faccette).
- Su Windows le righe terminano con CRLF; `scadr` scrive LF (per il resto l'output è identico byte per
  byte a parità di triangoli, verificato su 5636 faccette di `separatore.scad`).

### 16.5 STL binario

Intestazione di 80 byte `OpenSCAD Model\n` seguita da zeri, numero di triangoli `u32`, poi per ogni
triangolo normale e 3 vertici in `f32` little endian e attributo `0`.

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

- Coordinate con **6 cifre significative** (`0.975452`), seguite da uno spazio finale.
- Un PolySet mantiene i poligoni (un cubo ha 6 facce `4 …`); un manifold ha triangoli seguiti dal
  colore della faccia (`3 0 2 6 249 215 44`). I vertici non sono saldati.
- `scadr` scrive lo stesso schema con coordinate a precisione piena (formato di §16.4), solo triangoli,
  senza colori. Il lettore (`mesh_io::read_off`) accetta anche i file di OpenSCAD (poligoni triangolati a
  ventaglio, colori ignorati).

### 16.7 Osservazioni sulla geometria (dal corpus)

- Un `polyhedron` con tutte le facce nel verso sbagliato viene esportato così com'è (volume negativo,
  `prim_polyhedron_reversed`).
- I risultati 2D vengono arrotondati su una griglia binaria fine: in `twod_square_circle` il vertice
  teorico `6.299038105676658` diventa `6.299038104712963` (multiplo esatto di 2^-30) e il volume
  `37.499999989984985` invece di `37.5`.
- `minkowski()` con un operando non convesso e un secondo operando arrotondato **non è
  deterministico** in OpenSCAD: a ogni esecuzione cambiano triangolazione e vertici collineari (stesso
  volume a 1e-16). Con un cubo come secondo operando il risultato è stabile.
- Due cubi che si toccano su uno spigolo restano due gusci con vertici duplicati (`bool_touching_edge`);
  due cubi con una faccia in comune vengono fusi (`bool_coplanar_union`, 20 triangoli).
