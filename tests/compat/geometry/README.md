# Corpus di geometria (render 3D)

Casi che confrontano la **geometria** prodotta dal nostro motore (`scadr` + `mesher`) con quella di
**OpenSCAD Nightly 2026.10.22** (backend Manifold). Ogni caso viene eseguito con

```
<motore> -o <nome>.stl --export-format asciistl [compat-args] <nome>.scad
```

e dall'STL ASCII si calcolano delle metriche, confrontate con quelle registrate da OpenSCAD in
`expected/<nome>.json`. Si usa l'STL ASCII perché OpenSCAD lo scrive con precisione piena
(numeri più corti che si rileggono identici); l'OFF di OpenSCAD ha solo 6 cifre significative e l'STL
binario è in `float` a 32 bit. Il comportamento osservato dell'export è descritto in
[`docs/openscad-behavior.md`](../../../docs/openscad-behavior.md) §16.

Gli expected sono nel repository: per l'uso normale **OpenSCAD non serve**.

## Struttura

```
tests/compat/geometry/
  cases/<argomento>_<nome>.scad   un caso per file (solo i .scad al primo livello)
  cases/data/                      octahedron.stl (per import), heights.dat (per surface)
  expected/<nome>.json             metriche, log ed exit code registrati da OpenSCAD
```

La direttiva `// compat-args:` (nelle prime 5 righe) funziona come nel corpus testuale: i modelli
reali la usano per variare i parametri con `-D`, ad esempio
`// compat-args: -D FINESTRA="ruota" -D TESTO="BLU" -D N_POS=2 -D TAB_POS=1`
(gli argomenti sono separati da spazi e passati senza shell: niente spazi dentro i valori).

## Metriche (`expected/<nome>.json`)

```json
{
  "format": 1,
  "exit_code": 0,
  "top_level": null,
  "log": "WARNING: ...\n",
  "geometry": {
    "triangles": 12, "vertices": 8,
    "volume": 1000, "area": 600,
    "bbox_min": [0, 0, 0], "bbox_max": [10, 10, 10],
    "manifold": true, "open_edges": 0, "nonmanifold_edges": 0, "degenerate_triangles": 0,
    "components": 1, "euler": 2, "genus": 0,
    "vertex_hash": "fnv1a64:…", "triangle_hash": "fnv1a64:…",
    "vertex_list": [[0, 0, 0], …]
  }
}
```

| Campo | Significato |
|---|---|
| `exit_code` | codice di uscita del motore (OpenSCAD: 0, oppure 1 senza file) |
| `top_level` | riga `Current top level object is empty.` / `… is not a 3D object.` se stampata, altrimenti `null` |
| `log` | stderr normalizzato come nel corpus testuale (solo i messaggi: `ECHO:`, `WARNING:`, `ERROR:`…; le statistiche sono scartate) |
| `geometry` | `null` se non è stato scritto nessun file |
| `triangles` | faccette dell'STL (anche degeneri) |
| `vertices` | vertici distinti dopo la saldatura delle coordinate identiche bit a bit |
| `volume`, `area` | volume con segno (teorema della divergenza, rispetto al centro del box) e area |
| `bbox_min`, `bbox_max` | box di ingombro |
| `manifold` | ogni spigolo è usato da esattamente due triangoli con verso opposto |
| `open_edges`, `nonmanifold_edges`, `degenerate_triangles` | spigoli con un solo triangolo, spigoli con più di due triangoli (o due nello stesso verso), triangoli con due vertici coincidenti |
| `components`, `euler`, `genus` | pezzi connessi, χ = V − E + F, genere con la convenzione di Manifold stampata da OpenSCAD: `1 − χ/2` (due cubi separati → −1) |
| `vertex_hash` | FNV-1a 64 dell'elenco ordinato dei vertici distinti arrotondati a 1e-6 |
| `triangle_hash` | FNV-1a 64 dei triangoli (vertici arrotondati a 1e-6, rotazione canonica che conserva il verso, elenco ordinato) |
| `vertex_list` | i vertici arrotondati ordinati, solo se sono al massimo 64 |

Tutte le metriche sono indipendenti dall'ordine dei triangoli nel file (i triangoli vengono messi in
ordine canonico prima delle somme, quindi anche volume e area sono riproducibili all'ultimo bit).

## Esiti

| Esito | Significato |
|---|---|
| `EXACT` | stessi vertici e stessi triangoli (a 1e-6), stesso log, stesso exit code; per i casi senza geometria: stesso esito (`top_level`) |
| `EQUIVALENT` | volume, area e box entro le tolleranze, stessa topologia (`manifold`, `components`, `genus`), stesso log ed exit code, ma vertici o triangolazione diversi |
| `FAIL` | metrica fuori tolleranza, geometria mancante o in più, log o exit code diversi |
| `ERROR` / `TIMEOUT` / `SKIP` | motore non avviabile o STL illeggibile / tempo scaduto / expected mancante |

Tolleranze predefinite: volume e area **relative 1e-6**, box **assoluta 1e-6** (opzioni
`--tol-volume`, `--tol-area`, `--tol-bbox`). Il numero di triangoli e di vertici e gli spigoli aperti
sono informativi (non decidono l'esito da soli). Exit code 0 se tutti i casi sono `EXACT` o
`EQUIVALENT` (`--strict`: solo `EXACT`).

## Come eseguire

```powershell
cargo build -p scadr
cargo run -p compat-runner -- --geometry                       # tutti i casi con target\debug\scadr.exe
cargo run -p compat-runner -- --geometry --filter bool_ -v     # solo alcuni casi, con tabella delle metriche
cargo run -p compat-runner -- --geometry --tol-volume 1e-9 --strict
cargo run -p compat-runner -- --geometry --ignore-log          # ignora i messaggi su stderr
```

Output (una riga per caso, poi il riepilogo; con `-v` per ogni caso non `EXACT` la tabella
metrica/atteso/ottenuto/scarto/tolleranza e il diff del log). Esempio illustrativo:

```
CASE                 RESULT        TIME  DETAIL
prim_cube_basic      EXACT        0.05s  12 tri, 8 vert
prim_sphere_default  EQUIVALENT   0.06s  vertices differ; tri 250 vs 252, vert 127 vs 128; vol rel 2.1e-9, area rel 1.0e-9
bool_cross_holes     FAIL         0.02s  exit code 1 (expected 0); no geometry written (Current top level object is empty.)

Total: 100 cases, 1 exact, 1 equivalent, 98 failed, 0 errors, 0 timeouts, 0 skipped (1.0% exact, 2.0% exact or equivalent)
```

Gli STL grezzi e le metriche ottenute finiscono in `target\compat-out\geometry\engine\<nome>.stl|json`
(quelli di OpenSCAD in `…\geometry\openscad\` con `--update-expected`).

Controllo di sanità: con OpenSCAD come motore tutti i casi devono risultare `EXACT`:

```powershell
cargo run -p compat-runner -- --geometry --engine 'C:\Program Files\OpenSCAD (Nightly)\openscad.exe'
```

## Come rigenerare gli expected

```powershell
cargo run -p compat-runner -- --geometry --update-expected                 # tutti
cargo run -p compat-runner -- --geometry --update-expected --filter rex_   # solo alcuni
```

Riporta `new` / `updated` / `unchanged`; rieseguirlo senza modifiche deve dare `0 changed`.
Per aggiungere un caso: creare `cases/<argomento>_<nome>.scad`, rigenerare con `--filter <nome>`,
controllare il JSON e verificare che il caso resti `EXACT` contro OpenSCAD stesso su più esecuzioni
(vedi sotto).

Regole per i casi: veloci (< 1 s in OpenSCAD; oggi il più lento, `model_porta_carte_taratura`,
impiega 0.5 s), deterministici, senza librerie esterne. **Attenzione al non determinismo di
OpenSCAD**: `minkowski()` con un operando non convesso e un secondo operando arrotondato (sfera,
cilindro) produce a ogni esecuzione triangolazioni e vertici collineari diversi (stesso volume a
1e-16), perché i pezzi convessi vengono uniti in parallelo. `mink_nonconvex` usa quindi un cubo come
secondo operando, che risulta stabile (15 esecuzioni su 15).

## Casi (100)

| Prefisso | Casi | Contenuto |
|---|---|---|
| `prim_` | 23 | `cube` (scalare, vettore, centrato, minuscolo, dimensione zero → vuoto), `sphere` (default, `$fn` dispari e < 3, `$fa`/`$fs`, raggio minuscolo e grande, `$fs` fine), `cylinder` (default, `$fn` 6 e 500, `$fa` 1 su r = 500, minuscolo, cono, cono rovesciato centrato, tronco con diametri e `$fs`), `polyhedron` (tetraedro, facce quadrilatere, piramide con base quadrata, facce con verso sbagliato) |
| `twod_` | 6 | primitive 2D estruse: quadrato + cerchio, cerchio con `$fa`/`$fs`, poligono con foro (`paths`), stella concava, percorsi sovrapposti (la sovrapposizione diventa un foro), isole separate |
| `xf_` | 8 | `translate`/`rotate` sui tre assi, rotazione attorno a un asse, `scale` non uniforme + `mirror`, scala negativa, `multmatrix` (taglio, matrice 3×4), `resize`, `resize` con `auto`, `color` |
| `bool_` | 11 | unione, differenza, intersezione, facce complanari (differenza e unione), cubi che si toccano su uno spigolo, tre fori ortogonali, risultato vuoto, unione implicita di un `for`, `intersection_for`, booleane annidate |
| `hull_` / `mink_` | 2 / 3 | `hull` 3D e 2D; `minkowski` convesso, non convesso, 2D |
| `lex_` | 7 | `linear_extrude`: `center`, `twist` + `slices`, `twist` negativo con fette calcolate, `scale` vettoriale, `scale = 0`, tutto insieme, direzione `v` |
| `rex_` | 6 | `rotate_extrude`: toro, frammenti di default, `angle` 90 e −135, `start`, profilo che tocca l'asse |
| `off_` / `proj_` / `fill_` / `text_` | 4 / 2 / 1 / 2 | `offset` `r` positivo e negativo, `delta`, `chamfer`; `projection` completa e `cut`; `fill`; `text` semplice e centrato con `$fn` |
| `import_` / `surface_` | 2 / 2 | `import` di un STL ASCII (anche scalato e in una differenza); `surface` da `.dat`, anche centrato |
| `mod_` | 4 | modificatori `#` (incluso), `%` (escluso), `!` (solo quel sotto-albero), `*` (disabilitato) |
| `cli_` | 5 | livello superiore vuoto, oggetto 2D esportato in STL (nessun file, exit 1), 2D misto a 3D (warning del render), errore di valutazione (geometria parziale, exit 0), `$preview` falso nel render |
| `model_` | 12 | copie di `examples/porta_carte.scad` (stampa, verticale con 2 file, coperchio, dima di taratura) e `examples/separatore.scad` (default, finestra a ruota con testo, verticale senza piedino), varianti via `-D`; copie dal corpus testuale di `model_bracket`, `model_vase`, `model_gear_polygon`, `model_parametric_box`, `model_spiral_tower` |
