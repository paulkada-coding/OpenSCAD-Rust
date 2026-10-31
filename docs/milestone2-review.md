# Revisione Milestone 2 — Branch `milestone-2-complete`

Documento di handoff e tracciabilità pensato per agevolare la revisione da parte di **Claude** prima del merge su `main`.

---

## 1. Riepilogo esecutivo

- **Branch di lavoro**: `milestone-2-complete` (partito da `main` al commit `34dc2e0`; `main` non è stato toccato).
- **Compatibilità con OpenSCAD Nightly 2026.10.22** (`tools/compat-runner` su `tests/compat`):
  - Stato iniziale su `main`: **181 / 215 casi passati (84,2%)** — 34 casi falliti.
  - Stato finale su `milestone-2-complete`: **215 / 215 casi passati (100,0%)** — 0 falliti, 0 errori, 0 timeout.
- **Qualità del codice e vincoli**:
  - `cargo test --workspace`: tutti i test unitari e di integrazione verdi.
  - `cargo clippy --workspace --all-targets`: **0 warning** (risolti anche i 3 warning `clippy` preesistenti in `crates/scad-eval/src/value.rs`).
  - **Zero `unsafe`** aggiunto (`unsafe_code = "deny"` rispettato in tutti i crate del linguaggio/interprete/geometria).
  - **Clean-room**: nessun file sorgente C++ di OpenSCAD (GPL) è stato letto o copiato; il comportamento è stato derivato esclusivamente da `docs/openscad-behavior.md`, dai casi attesi in `tests/compat/expected/` e dall'uso black-box dell'eseguibile `openscad.com` come oracolo su piccoli snippet `.scad` di test.
  - Formattazione (`rustfmt --edition 2024`) applicata solo ai file modificati, evitando rumore di diff su file non toccati.

---

## 2. Comandi rapidi per la revisione

```powershell
# Elenco dei commit atomici rispetto a main
git log --oneline --reverse main..milestone-2-complete

# Statistiche dei file toccati per singolo commit
git log --stat --reverse main..milestone-2-complete

# Diff completo rispetto a main
git diff main..milestone-2-complete

# Verifica completa (clippy, unit test, suite di compatibilità)
cargo clippy --workspace --all-targets
cargo test --workspace
cargo build -p scadr
cargo run -p compat-runner
```

---

## 3. Dettaglio commit per commit

### Commit 1 — `eb6f001`
**`feat(scad-syntax,scad-eval): add lexer/parser diagnostics and escape rules`**
*(+8 casi compat: da 181/215 a 189/215)*

- **File modificati**:
  - `crates/scad-syntax/src/{syntax_kind.rs, lexer.rs, lower.rs, parser.rs, lib.rs, tests.rs}`
  - `crates/scad-eval/src/{message.rs, eval.rs}`
- **Cosa è stato implementato**:
  1. **Letterali esadecimali e identificatori che iniziano con cifre**:
     - Regex esadecimale ristretta a `0x[0-9a-fA-F]+` (`0x` minuscolo); `0XFF` viene ora tokenizzato come `IDENT` (come in OpenSCAD), generando la diagnostica `DEPRECATED` e poi `WARNING: Ignoring unknown variable "0XFF"`.
     - Introdotto `LexWarning` in `scad-syntax` e `Level::Deprecated` (`DEPRECATED: ...`) in `scad-eval`.
     - Ogni token `IDENT` che inizia con cifra ASCII emette `DEPRECATED: Variable names starting with digits ("...") will be removed in future releases.`.
  2. **Interi non rappresentabili esattamente in IEEE-754 `f64`**:
     - Per letterali interi decimali che superano la precisione a 53 bit (`(n as f64) as u128 != n` o overflow `u128`), emesso `WARNING: Integer "..." cannot be represented precisely`.
  3. **Overflow di letterali float (`1e400`) e CST lossless**:
     - Introdotto il `SyntaxKind::OVERFLOW_NUMBER` classificato come trivia (`is_trivia() == true`).
     - In questo modo il CST `rowan` **conserva tutti i caratteri del sorgente** (proprietà lossless intatta per refactoring e AI), mentre il parser salta trasparentemente il numero fuori range come fa Flex/Bison in OpenSCAD (`echo(1e400);` → `ECHO: `, `1e400 ? 1 : 2` → errore di sintassi).
  4. **Sequenze di escape nelle stringhe (`lower.rs::unescape`)**:
     - Escape validi: `\"`, `\\`, `\n`, `\t`, `\r`, `\x01`..`\x7f`, `\uXXXX` (4 hex), `\UXXXXXXXX` (6 hex).
     - Codepoint nullo (`\x00`, `\u0000`, `\U000000`), surrogati UTF-16 (`0xD800..=0xDFFF`) o valori `> 0x10FFFF` convertiti in spazio `' '` senza warning.
     - `\x` con meno di 2 cifre hex o valore `>= 0x80` (`\x80`..`\xff`) trattato come escape non definito: rimosso solo `\` e mantenuto il resto letterale, emettendo `WARNING: Undefined escape sequence` (con limite massimo di 5 warning per singolo letterale stringa, come OpenSCAD).
  5. **Stringhe e commenti a blocco non terminati**:
     - Introdotti `UNTERMINATED_STRING` e `UNTERMINATED_COMMENT`; il numero di riga riportato segue la convenzione del buffer Flex di OpenSCAD (`newline_count + 3`), emettendo la coppia `ERROR: Parser error: Unterminated string/comment ...` seguita da `ERROR: Parser error: syntax error ...`.
- **Casi risolti**: `echo_numbers_edge`, `err_parse_number_overflow`, `err_parse_unterminated_comment`, `err_parse_unterminated_string`, `lex_identifiers`, `num_integers_large`, `str_bad_escapes`, `str_escapes`.

---

### Commit 2 — `7c1fc28`
**`fix(scad-eval): warn on missing include files and unsupported child modules`**
*(+3 casi compat: da 189/215 a 192/215)*

- **File modificati**:
  - `crates/scad-eval/src/{eval.rs, builtins/modules.rs}`
- **Cosa è stato implementato**:
  1. **`include <missing.scad>` non fatale**:
     - Sostituito l'errore fatale con `WARNING: Can't find include file '{path}'.` (con span sull'istruzione `include`) proseguendo la valutazione del file chiamante, in linea con `use <missing.scad>`.
  2. **Moduli foglia che non accettano figli**:
     - Aggiunta `builtins::modules::accepts_children(name)` (`false` per `cube`, `sphere`, `cylinder`, `polyhedron`, `square`, `circle`, `polygon`, `text`, `import`, `surface`).
     - Quando uno di questi moduli viene invocato con un blocco non vuoto (`cube() echo("x");` o `cube() sphere();`), viene emesso `WARNING: module {name}() does not support child modules` e il corpo figlio non viene valutato (ma gli argomenti del modulo padre vengono comunque validati).
- **Casi risolti**: `include_missing`, `module_with_child_not_supported`, `err_parse_missing_semicolon`.

---

### Commit 3 — `09e34a3`
**`fix(scad-eval,scadr): validate builtin module parameters and match matrix signed zeros`**
*(+12 casi compat: da 192/215 a 204/215)*

- **File modificati**:
  - `crates/scad-eval/src/{message.rs, eval.rs, builtins/modules.rs}`
  - `crates/scadr/src/main.rs`
- **Cosa è stato implementato**:
  1. **Conversione stretta dei booleani nei parametri builtin (`Params::bool`)**:
     - Sostituito `Value::is_truthy()` con `Value::as_bool().unwrap_or(default)`: in OpenSCAD `cube(center = "yes")` o `cube(center = 1)` usa il default `false` invece di convertire per truthiness.
  2. **Warning di conversione parametri dei moduli builtin**:
     - `square`: `Unable to convert square(size=..., ...) parameter to a number or a vec2 of numbers`.
     - `polygon` / `polyhedron`: `Unable to convert points = ... to a vector of coordinates`.
     - `translate`: `Unable to convert translate(...) parameter to a vec3 or vec2 of numbers`.
     - `scale`: `Unable to convert scale(...) parameter to a number or a vec3 or vec2 of numbers`.
     - `mirror`: `Unable to convert mirror(...) parameter to a vec3 or vec2 of numbers`.
     - `rotate`: `Problem converting rotate(a=...) parameter` (con distinzione tra `rotate(x)` posizionale e `rotate(a=x)` nominale).
     - `color`: `Unable to parse color "..."` seguito dalla riga di continuazione `For a list of valid color names, see the <a href="open-window://colorlist"><b>Color List</b></a> window.` (tramite `Message::continuation` senza prefisso `WARNING:` né suffisso `in file ...`), più il controllo range `[0.0, 1.0]` (`color() expects numbers between 0.0 and 1.0. Value of {:.1} is out of range`).
  3. **Segni esatti di `-0.0` nelle matrici 4×4 (`DMat4`)**:
     - `scale_matrix`: costruita con moltiplicazione riga per componente come `Eigen::Scaling` in modo che `scale([-0, 1, 1])` produca `[-0, -0, -0]` nella prima colonna.
     - `mirror`: eliminata la normalizzazione intermedia via `sqrt()` (`1.0 - 2.0 * a * b / len_sq`, dove `len_sq = x*x + y*y + z*z`) evitando residui `2.22045e-16` su `mirror([1, 1, 0])` e preservando i `-0` fuori diagonale su `mirror([1, 0, 0])`.
     - `rotation` su asse singolo: normalizzazione `+ 0.0` sui termini fuori diagonale per evitare `-0` spuri su `rotate(-90, [0, 0, 1])`.
     - `euler_rotation_matrix([x, y, z])`: formula chiusa diretta identica alla composizione Eigen di OpenSCAD (preserva `[-1, 0, 0, 0], [-0, -1, 0, 0], [-0, 0, 1, 0]` per `rotate([0, 0, 180])`).
- **Casi risolti**: `csg_2d_invalid`, `csg_color_hex`, `csg_color_invalid`, `csg_cube_invalid`, `csg_mirror`, `csg_polygon`, `csg_polyhedron`, `csg_rotate_axis`, `csg_rotate_invalid`, `csg_rotate_vector`, `csg_scale`, `csg_translate_invalid`.

---

### Commit 4 — `0585ed1`
**`feat(geometry,scad-eval): add structured CSG nodes and export for text, import, and surface`**
*(+5 casi compat: da 204/215 a 209/215)*

- **File modificati**:
  - `crates/geometry/src/{csg.rs, csg_text.rs}`
  - `crates/scad-eval/src/{eval.rs, builtins/modules.rs}`
- **Cosa è stato implementato**:
  1. **Varianti tipizzate nell'albero CSG (`NodeKind::{Text, Import, Surface}`)**:
     - Scelta architetturale pensata per **Milestone 3 e Milestone 5**: invece di formattare stringhe opache dentro `NodeKind::Other`, i parametri risolti (inclusi `Tessellation`, `dpi`, `origin`, `timestamp`, `script`, `direction`) sono salvati come campi tipizzati in `geometry::csg::NodeKind`, pronti per essere consumati direttamente dal mesher 2D/3D.
  2. **Rilevamento script Unicode ISO 15924 e direzione automatica RTL in `text()`**:
     - Se `script` è omesso: scansione dei caratteri di `text` per individuare il primo script non-Common/Inherited (`Latn`, `Cyrl`, `Arab`, `Hani`, `Grek`, `Hebr`, `Deva`, `Kana`, `Hang`, ecc.); se `text` contiene solo cifre, spazi o è vuota, `script` viene omesso nell'export `.csg` esattamente come fa OpenSCAD.
     - Se `script` è passato esplicitamente (es. `"latin"`, `"arabic"`): normalizzazione a 4 caratteri TitleCase (`"Lati"`, `"Arab"`), come `hb_script_from_string`.
     - Se `direction` è omesso e lo script risolto è RTL (`Arab`, `Hebr`, `Syrc`, `Thaa`), `direction` diventa `"rtl"`, altrimenti `"ltr"`.
     - Validazione del tipo di `text`: `WARNING: text(..., text=...) Invalid type: expected string, found ...`.
  3. **`import()` e `surface()`**:
     - Risoluzione del `timestamp` (mtime Unix in secondi del file risolto rispetto alla cartella del sorgente `.scad`, `0` se assente), `dpi` emesso solo per `.svg` (default `72`), e formattazione `.csg` identica a OpenSCAD.
- **Casi risolti**: `csg_import_surface`, `csg_text`, `csg_text_invalid`, `csg_text_scripts`, `model_text_label`.

---

### Commit 5 — `334c215`
**`feat(scad-eval,scadr): add mt19937 rands, search checks, object(), and vector-swizzle`**
*(+5 casi compat: da 209/215 a 214/215)*

- **File modificati**:
  - `crates/scad-eval/src/{value.rs, eval.rs, builtins/functions.rs}`
  - `crates/scadr/src/main.rs`
- **Cosa è stato implementato**:
  1. **`rands()` con Mersenne Twister `Mt19937` a 32 bit**:
     - Sostituito lo stub `splitmix64` con l'implementazione esatta di `std::mt19937` + `std::generate_canonical<double, 53>` (due estrazioni `u32` per ogni `double`: `(r0 + r1 * 2^32) / 2^64`).
     - Aggiunti il controllo di arità (`expected 3 or 4`), lo scambio automatico quando `min > max`, e la troncatura `i64 as u32` del seed.
  2. **`search()`**:
     - Controllo di arità (`expected between 2 and 4`) e warning `Invalid entry in search vector at index ..., required number of values in the entry: ...`.
  3. **Flag CLI `--enable` e funzioni sperimentali (`object-function`, `vector-swizzle`, `import-function`)**:
     - Supporto a `--enable <feature>` / `--enable=<feature>` (incluso `all`) in `scadr` collegato a `EvalOptions`.
     - Se una funzione sperimentale (`object`, `has_key`, `import`) viene chiamata senza `--enable`, emesso `WARNING: Experimental builtin function '...' is not enabled` prima di `WARNING: Ignoring unknown function '...'`.
     - Introdotto `Value::Object(Rc<Vec<(String, Value)>>)` con preservazione dell'ordine di inserimento, formattazione `{ a = 1; b = "x"; }`, accesso `obj.key` e `obj["key"]`, iterazione delle chiavi in `for (k = obj)`, `len(obj)`, `is_object(v)`, `has_key(obj, k)`, e costruzione/copia/override tramite `object(...)` (argomenti nominati e coppie `[key, value]`, inclusa rimozione chiave con valore `undef`).
     - Supporto allo swizzling vettoriale (`.xyzw` / `.rgba`, da 1 a 4 componenti; 1 componente restituisce scalare, 2..4 restituiscono lista) quando `vector_swizzle` è abilitato.
  4. **Pulizia warning `clippy`**:
     - Risolti i 3 warning `clippy` preesistenti in `crates/scad-eval/src/value.rs`.
- **Casi risolti**: `builtin_object_disabled`, `builtin_object_enabled`, `builtin_rands_seeded`, `builtin_search_bad`, `builtin_swizzle_enabled`.

---

### Commit 6 — `6c3e058`
**`feat(scad-eval): implement fontmetrics() and textmetrics() with skrifa and rustybuzz`**
*(+1 caso compat: da 214/215 a **215/215 — 100,0%**)*

- **File aggiunti / modificati**:
  - `crates/scad-eval/Cargo.toml`, `Cargo.lock` (`fontdb` 0.24, `rustybuzz` 0.20, `skrifa` 0.49)
  - `crates/scad-eval/fonts/Liberation-2.00.1/{LICENSE, AUTHORS, ttf/LiberationSans-*.ttf}`
  - `crates/scad-eval/src/builtins/{fonts.rs, functions.rs, mod.rs}`
- **Cosa è stato implementato**:
  1. **Font Liberation Sans 2.00.1 integrati (SIL Open Font License 1.1)**:
     - Inclusi i 4 stili (`Regular`, `Bold`, `Italic`, `BoldItalic`) insieme ai file `LICENSE` e `AUTHORS` richiesti dalla SIL OFL 1.1.
     - Database `fontdb` condiviso (`LazyLock`) che carica prima i font Liberation Sans integrati (disponibili ovunque senza dipendere dalla directory di lavoro) e poi i font di sistema come fallback (es. `"Arial"`).
     - Parser delle specifiche font in stile fontconfig (`"Liberation Sans:style=Bold"`, `"Family1,Family2:Bold Italic"`, pesi, stili e larghezze).
  2. ** Matematica di scaling FreeType 26.6 + HarfBuzz**:
     - Riprodotto esattamente il fattore di scala FreeType `FT_Set_Char_Size(face, 100_000, 100_000, 100, 100)` (`dim_26_6 = 138_889`, `ppem = 2170`, `FT_MulFix` con arrotondamento signed-magnitude a 16.16 bit) moltiplicato per `size / 100_000.0`.
     - Hinting TrueType tramite `skrifa::outline::HintingInstance` a `Size::new(2170.0)` e control-box grid-fitted (`FT_GLYPH_BBOX_GRIDFIT`: `floor` sui minimi, `ceil` sui massimi in pixel interi).
     - Shaping completo `rustybuzz` (direzioni `ltr`, `rtl`, `ttb`, `btt`, lingua, script ISO 15924, `spacing`, `halign`, `valign`) inclusa la sintesi delle metriche verticali dai campi `OS/2` (`sTypoAscender - sTypoDescender`) quando il font non possiede la tabella `vmtx` (come Liberation Sans).
     - Validazione dei tipi di tutti i parametri di `fontmetrics()` e `textmetrics()` con i relativi warning.
- **Caso risolto**: `builtin_textmetrics` (+ test unitari aggiuntivi in `builtins::fonts::tests` anche per `"ab"`, `"gp"`, spazi, Bold `size=20` e layout verticale `ttb`).

---

## 4. Punti di attenzione consigliati per la revisione di Claude

1. **`crates/geometry/src/csg.rs` (`NodeKind::{Text, Import, Surface}`)**:
   - Verificare che la forma delle nuove varianti `NodeKind` sia comoda anche per **Milestone 5** (dove `Text` userà lo stesso database `fontdb` + `skrifa` + `rustybuzz` per estrarre i contorni 2D dei glifi e passarli a Clipper2 / `CrossSection`). Eventualmente, in M5, la logica di scoperta font in `crates/scad-eval/src/builtins/fonts.rs` potrà essere promossa in un crate condiviso (es. `crates/scad-font` o dentro `geometry`) così sia `textmetrics()` sia il mesher `text()` condivideranno un'unica istanza `FONT_DB` e lo stesso pipeline di shaping.
2. **`crates/scad-syntax/src/syntax_kind.rs` (`OVERFLOW_NUMBER` come trivia)**:
   - Verificare che nessun futuro tool di formattazione/refactoring sul CST assuma che i token trivia siano solo spazi e commenti: `OVERFLOW_NUMBER` si presenta solo su letterali patologici fuori scala IEEE-754 (`> 1.797e308`), preservando il testo originale nel CST.
3. **`crates/scad-eval/src/value.rs` (`Value::Object`)**:
   - L'oggetto usa `Rc<Vec<(String, Value)>>` per preservare l'ordine di inserimento delle chiavi (identico all'output di OpenSCAD) con costo minimo sul numero tipico di campi. Se in futuro servissero oggetti con migliaia di chiavi, si potrà affiancare `IndexMap`.
