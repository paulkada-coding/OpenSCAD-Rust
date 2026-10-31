# Corpus di compatibilità OpenSCAD

Casi di test piccoli e deterministici che confrontano il nostro motore (`scadr`) con
**OpenSCAD Nightly 2026.10.22** (l'oracolo). Ogni caso viene eseguito con
`<motore> -o <out>.csg <caso>.scad` e si confrontano due cose:

1. i messaggi su stderr (`ECHO:`, `WARNING:`, `ERROR:`, `TRACE:`, `DEPRECATED:` …);
2. il file CSG esportato (albero geometrico in forma testuale).

Il comportamento osservato è documentato in dettaglio in
[`docs/openscad-behavior.md`](../../docs/openscad-behavior.md).

## Struttura

```
tests/compat/
  cases/<argomento>_<nome>.scad   un caso per file (solo i .scad al primo livello sono casi)
  cases/lib/                       librerie di supporto per include/use (non sono casi)
  cases/data/                      file dati per import()/surface() (non sono casi)
  expected/<nome>.log              stderr normalizzato prodotto da OpenSCAD
  expected/<nome>.csg              export CSG normalizzato prodotto da OpenSCAD
                                   (ASSENTE se OpenSCAD non scrive il file, es. errori di parsing)
```

Prefissi degli argomenti (215 casi):

| Prefisso | Casi | Contenuto |
|---|---|---|
| `echo_` | 18 | formattazione di numeri, stringhe, vettori, range, funzioni, argomenti nominati, stampa delle espressioni |
| `str_` | 8 | `str`, `chr`, `ord`, indicizzazione stringhe, escape |
| `num_` | 9 | aritmetica, modulo, divisione per zero, `round`, `min`/`max`, interi grandi |
| `trig_` | 4 | esattezza di sin/cos/tan, segno dello zero, funzioni inverse |
| `op_` | 10 | operazioni su vettori/matrici, confronti, verità, bitwise, operazioni non definite |
| `index_` / `range_` | 4 / 7 | indicizzazione, `.x/.y/.z`, range con passo decimale, range inversi, limiti |
| `lc_` / `let_` / `expr_` | 5 / 2 / 1 | list comprehension, `let`, `echo`/`assert` come espressioni |
| `scope_` / `special_` | 10 / 9 | riassegnazione, ordine di valutazione, variabili `$`, `$children`, `parent_module` |
| `include_` / `use_` | 4 / 2 | semantica di `include` e `use` |
| `func_` / `module_` | 13 / 9 | funzioni, letterali funzione, argomenti, ricorsione, moduli, `children()` |
| `builtin_` | 15 | funzioni builtin (`len`, `concat`, `lookup`, `search`, `rands`, `is_*`, oggetti…) |
| `err_` / `lex_` | 16 / 3 | `assert`, errori di parsing, commenti, identificatori, moduli deprecati |
| `csg_` | 56 | export CSG di ogni modulo builtin, trasformazioni, modificatori, formattazione numeri |
| `model_` | 10 | modelli realistici multi-funzione (scatola parametrica, ingranaggio, staffa, vaso…) |

### Direttiva `compat-args`

Un caso può chiedere argomenti extra da riga di comando (passati sia a OpenSCAD sia al nostro
motore) con un commento in una delle prime 5 righe:

```openscad
// compat-args: --enable=object-function
```

## Regole di normalizzazione

Applicate in modo identico all'output di OpenSCAD (quando si generano gli expected) e a quello
del nostro motore (implementazione: `tools/compat-runner/src/normalize.rs`).

**Log (stderr):**

1. decodifica UTF-8 (lossy); `\r\n` → `\n` (OpenSCAD su Windows scrive CRLF);
2. il percorso assoluto della cartella `cases` viene rimosso (con `\` o `/` finale), quindi
   `Can't parse file 'C:\...\cases\x.scad'!` diventa `Can't parse file 'x.scad'!`;
   i percorsi in `in file x.scad, line N` sono già relativi alla cartella del file principale;
3. si scartano le righe "rumore" (dopo aver tolto gli spazi iniziali iniziano con):
   `Fontconfig `, `Geometries in cache:`, `Geometry cache size in bytes:`, `CGAL Polyhedrons in cache:`,
   `CGAL cache size in bytes:`, `Total rendering time:`, `Top level object is`, `Convex:`, `Facets:`,
   `Vertices:`, `Volumes:`, `Halfedges:`, `Edges:`, `Halffacets:`, `Contours:`, `Rendering Polygon Mesh`,
   `Simple:`, `Bounding box:`, `Current top level object is`, `Parsing design`, `Compiling design`,
   `Rendering finished`, `Normalized tree`, `Compile and preview finished`
   (con export `.csg` OpenSCAD non stampa statistiche; in pratica si vedono solo le righe `Fontconfig error`);
4. si tengono solo i messaggi: righe che iniziano con `ECHO:`, `WARNING:`, `ERROR:`, `TRACE:`,
   `DEPRECATED:`, `FONT-WARNING:`, `EXPORT-WARNING:`, `EXPORT-ERROR:`, `UI-WARNING:`,
   `Can't parse file`, **più le loro righe di continuazione** (es. `echo("a\nb")` produce due
   righe; il warning dei colori ha una seconda riga `For a list of valid color names…`).
   Le righe prima del primo messaggio vengono scartate;
5. nelle righe `TRACE:` il numero in `*** Excluding 15373 frames ***` diventa `N`
   (dipende dalla dimensione dello stack);
6. si tolgono le righe vuote finali; il file termina con `\n` (vuoto se non ci sono messaggi).

Non si tolgono spazi finali: `echo()` produce `ECHO: ` con uno spazio.

**CSG:**

1. UTF-8 (lossy), `\r\n` → `\n`; percorso assoluto della cartella `cases` rimosso;
2. `timestamp = <intero>` (in `import()`/`surface()`, è la data di modifica del file) → `timestamp = 0`;
3. si tolgono le righe vuote finali (OpenSCAD termina sempre con una riga vuota extra) e il file
   termina con `\n`; un albero vuoto produce un file vuoto.

Se OpenSCAD non produce il file CSG (errore di parsing: exit code 1), non esiste `expected/<nome>.csg`
e il caso fallisce se il nostro motore ne scrive uno. Il codice di uscita in sé non viene confrontato.

## Come eseguire

```powershell
$env:CARGO_TARGET_DIR = 'C:\Software\3D Openscad\target\agent-compat'   # opzionale
cargo run -p compat-runner                                  # tutti i casi con target\debug\scadr.exe
cargo run -p compat-runner -- --filter csg_ --verbose       # solo i casi che contengono "csg_", con diff
cargo run -p compat-runner -- --engine path\to\scadr.exe    # motore in un altro percorso
cargo run -p compat-runner -- --help
```

Opzioni: `--engine <path>`, `--openscad <path>`, `--update-expected`, `--filter <substr>` (ripetibile),
`-v/--verbose` (diff unificati dei fallimenti), `-j/--jobs <N>`, `--timeout <secondi>` (default 30),
`--cases <dir>`, `--expected <dir>`, `--out-dir <dir>` (default `target\compat-out`: lì finiscono
l'output grezzo e il log normalizzato del nostro motore, utile per il debug).

Esito per caso: `PASS`, `FAIL` (log e/o CSG diversi), `ERROR` (impossibile avviare il motore),
`TIMEOUT`, `SKIP` (motore mancante o expected assente). Exit code 0 solo se tutti i casi passano.
Se `scadr` non esiste ancora, tutti i casi risultano `SKIP` con un messaggio chiaro (exit code 1).

Controllo di sanità del runner: usando OpenSCAD stesso come motore si deve ottenere il 100%:

```powershell
cargo run -p compat-runner -- --engine 'C:\Program Files\OpenSCAD (Nightly)\openscad.exe'
```

## Come rigenerare gli expected

```powershell
cargo run -p compat-runner -- --update-expected
# oppure con un altro binario:
cargo run -p compat-runner -- --update-expected --openscad 'C:\path\to\openscad.exe'
```

Il comando riesegue OpenSCAD su ogni caso, applica la normalizzazione e riscrive solo i file
cambiati (riporta `new` / `updated` / `unchanged`). Rieseguirlo senza modifiche deve dare
`0 changed`. Per aggiungere un caso: creare `cases/<argomento>_<nome>.scad`, poi
`--update-expected --filter <nome>` e controllare a mano gli expected prima del commit.

Regole per i casi: niente librerie esterne, niente `rands()` senza seme, niente dipendenze
dall'ora o dalla dimensione dello stack (es. limite di ricorsione non in coda), file piccoli e veloci.
