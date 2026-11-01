# Progetto: motore e app OpenSCAD nativi in Rust (nome da definire)

App desktop per aprire, parametrizzare, visualizzare (GPU) ed esportare in STL/3MF progetti OpenSCAD,
con un **interprete OpenSCAD scritto da zero in Rust** (non usa il binario OpenSCAD a runtime).
Predisposta per: modellazione 3D diretta e assistente AI (Claude / Gemini) integrato.

> Questo file è mantenuto da Claude: va aggiornato a ogni decisione o milestone
> (sezioni "Decisioni", "Stato" e "Roadmap").

## Utente e modo di lavorare

- L'utente parla italiano: rispondere e scrivere la documentazione in italiano.
  Codice, identificatori e commenti nel codice in inglese.
- L'utente non conosce Rust: Claude è responsabile di tutto il sorgente. Spiegare le scelte
  in termini comprensibili, chiedere conferma solo sulle decisioni di prodotto.
- Usare agenti paralleli quando ci sono parti indipendenti (es. crate separati, ricerche, test).
- Ad ogni milestone: l'utente prova l'app e fornisce file `.scad` reali come casi di test.

## Decisioni

| Data | Decisione | Motivo |
|---|---|---|
| 2026-10-31 | Opzione B: interprete OpenSCAD nativo in Rust | Source mapping geometria→codice, render incrementale, editing lossless del codice (modellazione e AI), un solo eseguibile |
| 2026-10-31 | Solo Windows per ora; codice portabile per macOS/Linux | Evitare API specifiche di Windows fuori da moduli isolati |
| 2026-10-31 | Visual Studio Build Tools **2022** (non 2026) | Massima compatibilità con l'ecosistema Rust/CMake; la 2026 non porta vantaggi per noi |
| 2026-10-31 | OpenSCAD ufficiale (Nightly) usato **solo come oracolo di test** | Confronto automatico dei risultati per misurare la compatibilità |
| 2026-10-31 | Frequenza fotogrammi **costante** di default (Impostazioni → Schermo), "su richiesta" opzionale | Con G-Sync/VRR i fotogrammi irregolari fanno variare la frequenza del monitor e gli OLED dell'utente sfarfallano |

## Stack

| Area | Tecnologia |
|---|---|
| Lexer / parser | `logos` + parser ricorsivo a mano + `rowan` (CST lossless, error recovery) |
| Interprete | tree-walking su AST tipizzato (bytecode in futuro), `stacker` per ricorsione profonda |
| Kernel CSG 3D | Manifold (C++, Apache-2.0) via binding Rust |
| 2D | Manifold CrossSection (Clipper2) |
| Font (`text()`) | `skrifa` + `rustybuzz` + `fontdb`, Liberation Sans incluso (crate `scad-font`) |
| Rendering | `wgpu` (DX12/Vulkan/Metal) + shader WGSL; adapter `HighPerformance` (PC con RTX 5080 + iGPU AMD) |
| UI | `egui` / `eframe` + `egui_dock` |
| Export | STL (bin/ASCII), 3MF, OBJ; SVG/DXF per il 2D |
| AI | `reqwest` + `tokio`, trait `LlmProvider` (Claude, Gemini), tool use + screenshot del viewport |
| Test | `insta`, `cargo-fuzz`, `compat-runner` contro OpenSCAD |

Vincoli di licenza: non copiare codice sorgente di OpenSCAD (GPL). Implementazione clean-room
basata su documentazione e comportamento osservato.

## Architettura (workspace Cargo)

```
crates/
  scad-syntax/   lexer logos, parser ricorsivo, CST rowan lossless, AST, diagnostica lessicale
  scad-font/     scoperta font (`fontdb` + Liberation Sans SIL OFL), shaping `rustybuzz`, metriche e contorni 2D `skrifa`
  scad-eval/     interprete, scope lessicali + `$` dinamiche, `SourceCache` cross-run, `Value::Object`, builtin
  scadr/         CLI compatibile con OpenSCAD (`-o x.csg|x.echo`, `-L`, `-D`, `--enable=...`, exit code e diagnostica)
  manifold/      Manifold v3.5.4 vendorizzato + shim C++ anti-eccezioni; triangolazione, ID originali, Clipper2 su interi
  geometry/      Mesh indicizzata f64, Aabb, volume/area, spigoli vivi, albero CSG (`Node` con `source` + `root_call`) ed export `.csg`
  mesher/        albero CSG → mesh con Manifold: tassellazione identica a OpenSCAD, 2D su griglia 2^-27, estrusioni, text, import, provenienza triangolo→sorgente
  mesh-io/       STL (binario/ASCII, formato OpenSCAD) e OFF, lettura e scrittura; poi 3MF/OBJ, import SVG/DXF
  render/        Scene wgpu offscreen (MSAA 4x, sRGB), OrbitCamera Z-up, rendering headless → PNG
  ai/            (da fare) provider Claude/Gemini, strumenti
  app/           UI egui: menu, pannello modello, viewport, barra di stato, impostazioni persistenti
tools/
  compat-runner/ confronto automatico parallelo con gli output attesi di OpenSCAD Nightly
examples/        file .scad di prova (+ STL generati con OpenSCAD)
tests/compat/    suite testuale (217 casi `.csg`/`.log`) + `geometry/` (100 casi con metriche delle mesh di OpenSCAD)
```

Note tecniche del rendering:
- La scena viene disegnata in una texture offscreen (`Rgba8Unorm`, scritta tramite una vista
  `Rgba8UnormSrgb`), poi mostrata da egui come texture nativa. Lo stesso percorso produce i PNG
  headless, che serviranno anche come "occhi" dell'AI.
- Si ridisegna solo quando cambia qualcosa (camera, dimensione, stile, mesh).
- Shading piatto (normale per faccia), facce posteriori in verde (come OpenSCAD) per evidenziare
  difetti, spigoli vivi > 25° disegnati come linee.
- glam 0.34: le proiezioni sono in `glam::camera::rh::{view, proj::directx}` (non più su `Mat4`).
- egui 0.37: `App::ui(&mut self, ui, frame)`, pannelli con `egui::Panel::{top,left,…}`.

Principi:
- Il motore (`scad-*`, `geometry`, `io`) non dipende dalla UI.
- Ogni nodo geometrico porta lo span del codice sorgente che lo ha generato.
- Le modifiche automatiche al codice (modellazione, AI) passano dal CST lossless.

## Roadmap

1. [x] **Fondamenta** — toolchain, workspace, finestra con viewer wgpu che carica/ruota un STL
2. [x] **Linguaggio** — parser, espressioni, funzioni, moduli, `echo`, test vs OpenSCAD (**215/215 — 100%**)
3. [x] **Primo render** — primitive 3D, trasformazioni, booleane Manifold, export STL (**99/100 casi geometrici**)
4. [ ] **Customizer** — parametri da commenti `// [min:step:max]`, slider, preset JSON, render incrementale
5. [ ] **2D e avanzate** — fatti in M3: extrude, offset, hull, minkowski, text, import STL/OFF, surface .dat. Mancano: import SVG/DXF/3MF/OBJ, surface PNG, regola di suddivisione di `linear_extrude` con scala non uniforme, sfera "octa" con `$fn > 12`
6. [ ] **Compatibilità** — MCAD, BOSL2, corpus di modelli reali
7. [ ] **AI** — pannello Claude/Gemini con tool use
8. [ ] **Modellazione diretta**

## Stato

- 2026-10-31: prerequisiti installati e verificati.
- 2026-10-31: **milestone 1 completata.** App egui con viewport wgpu (RTX 5080 via Vulkan),
  apertura STL (menu, drag & drop, argomento da riga di comando), export STL binario/ASCII,
  viste predefinite, prospettiva/ortogonale, griglia/assi/spigoli, info modello.
  Verificato: render headless e screenshot della GUI. Da provare a mano: interazione mouse.
  Prossimo passo: milestone 2 (linguaggio).
- 2026-10-31: **milestone 2 completata sul branch `milestone-2-complete` (in attesa di revisione).**
  Crate attivi: `scad-syntax` (lexer logos, parser rowan lossless, AST, diagnostica lessicale),
  `scad-eval` (interprete completo: scope lessicali + `$` dinamiche, tail call, list comprehension,
  include/use, children, builtin, `Mt19937`, `Value::Object`, swizzle `.xyzw`/`.rgba`,
  `fontmetrics()`/`textmetrics()` con `skrifa` + `rustybuzz` + `fontdb` e font Liberation Sans SIL OFL),
  `scadr` (CLI `scadr -o x.csg f.scad`, `--enable=...`, `-D`), `manifold` (Manifold v3.5.4 + shim C++,
  24 test; da integrare in M3), `tools/compat-runner` + `tests/compat`.
  Compatibilità attuale: **215/215 casi (100.0%)**, zero warning `clippy`, zero `unsafe` nei crate
  del linguaggio/interprete. Resoconto dettagliato per la revisione in `docs/milestone2-review.md`.
- 2026-10-31: **miglioramenti architetturali sul branch `milestone-2-improvements`** (partito da
  `milestone-2-complete`): estratto il crate condiviso `scad-font` (con estrazione di contorni 2D
  appiattiti pronta per M5), source mapping bidirezionale su `geometry::csg::Node` (`source` +
  `root_call` + `find_at_offset` + `Evaluator::resolve_source`), `SourceCache` riutilizzabile tra
  esecuzioni con invalidazione automatica su modifica disco, percorsi libreria standard
  (`OPENSCADPATH` + `Documents/OpenSCAD/libraries`), flag CLI `-L`, `-D var=expr` ed export `.echo`
  in `scadr`, property/fuzz test su digitazione live e `cargo fmt --all` uniforme su tutto il
  workspace. Guida alla revisione in `docs/milestone2-improvements-review.md`.
- 2026-10-31: **revisione di Claude completata, entrambi i branch uniti in `main`** (fast-forward).
  Verificato in modo indipendente: 215/215 casi, test/clippy/fmt puliti, nessun `unsafe` nuovo,
  `tests/compat` e la normalizzazione del runner non toccati (risultato genuino). Correzione
  applicata: gli override `-D` vengono analizzati una sola volta all'avvio come sorgenti virtuali
  (`-D nome`), un `-D` non valido produce `WARNING: Ignoring invalid definition` invece di essere
  ignorato in silenzio, e i messaggi generati dentro un `-D` puntano al `-D` e non al file principale.
  Prossimo passo: milestone 3.
- 2026-10-31: test dell'utente. I suoi `examples/porta_carte.scad` e `examples/separatore.scad`
  (progetto reale: offset, hull, linear_extrude, text+resize) danno output identico a OpenSCAD;
  aggiunti al corpus come `model_porta_carte` / `model_separatore` (**217/217**).
  Segnalato flicker su monitor OLED con G-Sync: aggiunta finestra **File → Impostazioni** con
  "Frequenza dei fotogrammi" costante (default) / su richiesta, salvata con la persistenza di
  eframe in `%APPDATA%\openscad-rust\data\app.ron` (insieme a finestra e pannelli). Verificato:
  a riposo "costante" ridisegna di continuo, "su richiesta" consuma 0 CPU; caricamento e
  salvataggio funzionano. Il flicker sul monitor dell'utente va confermato da lui.
- 2026-11-01: **milestone 3 completata** (tre filoni in parallelo, branch `m3-mesher` e `m3-export`
  in worktree `C:\Software\wt-*`, uniti in `main`).
  - `mesher`: tassellazione vertice per vertice identica a OpenSCAD (primitive, 2D, estrusioni,
    text, surface; triangolazione delle facce come il tessellatore di OpenSCAD), booleane batch
    Manifold, `!`/`#`/`%`, provenienza triangolo→nodo, annullamento, diagnostiche con posizione.
    Regole ricavate dall'oracolo documentate nei commenti.
  - `scadr -o x.stl|x.off` con messaggi e exit code di OpenSCAD; `stl` = ASCII come OpenSCAD.
  - Banco geometrico `compat-runner --geometry`: **100 casi, 39 identici, 60 equivalenti, 1 fallito**
    (`lex_scale_vector`: area +0,01%, regola di suddivisione con scala non uniforme non trovata).
  - App: apre `.scad` (anche drag & drop), calcolo in un thread di lavoro con annullamento, console
    colorata, ricarica automatica quando cambiano il file o le sue dipendenze, F5, inquadratura solo
    al primo caricamento, ultima geometria valida tenuta in caso di errore, `#` rosso e `%` grigio
    trasparenti, risultati 2D mostrati ma non esportabili.
  - Verificato nell'app con `porta_carte` (anche in modalità assieme con `%`) e `separatore`:
    geometria corretta, 5636 triangoli come OpenSCAD, 40–80 ms.
- 2026-11-01: pulizia. Rimossi i worktree `C:\Software\wt-*`, i branch già uniti
  (`m3-*`, `milestone-2-*`), le cartelle di build degli agenti e il contenuto di `out/`.
  **Prossimi passi dettagliati in `docs/next-steps.md`** (brief autosufficiente per altri agenti,
  priorità: milestone 4 Customizer).

## Note per le prossime milestone

- `SourceCache` usa `Rc<RefCell>` e non è `Send`: nell'app la valutazione girerà in un thread di
  lavoro dedicato che possiede la cache (non condividerla tra thread).
- `SourceCache` copre `include`/`use`; il file principale viene sempre ri-analizzato (nell'app
  arriva dal buffer dell'editor, non dal disco).
- `text()` è già meshato dal mesher tramite `scad-font` (vertici identici a OpenSCAD).
- `RenderOutput::triangle_sources` + `sources` danno la provenienza di ogni triangolo: base per il
  clic sul modello → riga di codice (non ancora collegato nell'app).
- Il mesher non è incrementale: ogni ricarica ricalcola tutto (40–80 ms sui modelli dell'utente).
  Per il Customizer (M4) valutare una cache per sotto-albero se servisse.
- Gli override `-D` sono la base per i valori del Customizer (M4): stessa semantica di
  OpenSCAD (assegnazione aggiunta in coda al file → sostituisce l'espressione nella posizione
  originale).

## Comandi

```powershell
cargo run -p app                                   # avvia l'app
cargo run -p app -- examples\supporto.stl          # apre un file
cargo run -p app -- --render-png in.stl out\x.png 1280x800   # snapshot senza finestra
cargo test --workspace
cargo build -p scadr; cargo run -p compat-runner                 # suite testuale vs OpenSCAD
cargo run -p compat-runner -- --geometry -v                      # suite geometrica vs OpenSCAD
.	argetdebugscadr.exe -o outx.stl in.scad                  # export STL (ASCII come OpenSCAD; binstl per il binario)
cargo clippy --workspace --all-targets
cargo fmt --all
& 'C:\Program Files\OpenSCAD (Nightly)\openscad.exe' --backend=manifold -o out.stl in.scad  # oracolo
```

La cartella `out/` è ignorata da git: usarla per output temporanei.

## Ambiente di sviluppo

- Windows 11 Pro, shell PowerShell 5.1.
- GPU: NVIDIA GeForce RTX 5080 + AMD Radeon integrata.
- Rust 1.99.1 stable, toolchain `x86_64-pc-windows-msvc` (clippy, rustfmt inclusi).
- VS Build Tools 2022 in `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools`.
- Git 2.56, CMake 4.4.4 (pacchetto winget, nel PATH utente).
- LLVM in `C:\Program Files\LLVM` (non nel PATH); `LIBCLANG_PATH` utente = `C:\Program Files\LLVM\bin` (per bindgen).
- OpenSCAD Nightly 2026.10.22 (oracolo di test): `C:\Program Files\OpenSCAD (Nightly)\openscad.exe` (non nel PATH).
  Esiste anche un vecchio OpenSCAD 2021.01 in `C:\Program Files\OpenSCAD` (preinstallato, non usarlo).
- Le shell già aperte prima dell'installazione non vedono il nuovo PATH: riaprirle o ricaricare
  `$env:Path` dal registro.
