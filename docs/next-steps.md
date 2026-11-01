# Prossimi passi — brief per chi continua il lavoro

Documento autosufficiente: chi lo legge non ha bisogno di altro contesto oltre al repository.
Aggiornato al 2026-11-01, dopo la milestone 3 (commit `8cde69a` su `main`).

---

## 0. Prima di iniziare

### Da leggere
1. `CLAUDE.md`: obiettivo del progetto, decisioni, stack, architettura, stato, comandi, ambiente.
2. `docs/openscad-behavior.md`: comportamento osservato di OpenSCAD (messaggi, numeri, CSG,
   export). È la "specifica" del progetto.
3. `tests/compat/README.md` e `tests/compat/geometry/README.md`: come funzionano i banchi di prova.
4. Il codice del crate su cui si lavora (ogni crate ha una descrizione in testa a `lib.rs`).

### Regole del progetto (non negoziabili)
- **Clean-room.** Mai leggere né copiare il sorgente C++ di OpenSCAD (GPL). Il comportamento si
  ricava solo eseguendo OpenSCAD Nightly (`C:\Program Files\OpenSCAD (Nightly)\openscad.exe`,
  con `--backend=manifold`) e dal manuale pubblico. Le regole ricavate vanno documentate nei
  commenti e, se generali, in `docs/openscad-behavior.md`.
- **Nessun `unsafe`** fuori da `crates/manifold` (il workspace ha `unsafe_code = "deny"`).
- **Mai modificare a mano** i file attesi in `tests/compat/expected` e `tests/compat/geometry/expected`.
  Si rigenerano solo da OpenSCAD con `--update-expected`, e solo per casi nuovi.
- **Il motore non dipende dalla UI**: `scad-*`, `geometry`, `mesher`, `mesh-io` non devono
  dipendere da `app`, `render` o `egui`.
- **Lingua**: codice, identificatori e commenti in inglese; documentazione e testi dell'interfaccia
  in italiano; i messaggi che imitano OpenSCAD restano nel suo inglese esatto.
- **Stile**: imitare il codice circostante (densità di commenti, nomi, idiomi). `cargo fmt --all`.

### Verifica obbligatoria prima di ogni commit
```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets          # deve dare 0 warning
cargo test --workspace
cargo build -p scadr
cargo run -p compat-runner                      # suite testuale: deve restare 217/217
cargo run -p compat-runner -- --geometry        # suite geometrica: oggi 39 exact, 60 equivalent, 1 failed
```
La suite geometrica non deve peggiorare: l'unico caso che oggi fallisce è `lex_scale_vector`
(vedi §3.4). Un caso EXACT che diventa EQUIVALENT è una regressione da spiegare.

### Note pratiche sull'ambiente (Windows 11, PowerShell 5.1)
- Le shell aperte prima dell'installazione degli strumenti non vedono il PATH: iniziare i comandi
  con `$env:Path = [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User');`
- **Python non è installato**: per modificare file usare gli strumenti di editing, non script Python.
- **Messaggi di commit su più righe**: PowerShell 5.1 rovina `-m` con virgolette; scrivere il
  messaggio in un file e usare `git commit -F file`. Terminare ogni messaggio con
  `Co-Authored-By: <modello> <indirizzo>` come nei commit esistenti.
- `Set-Content`/`Out-File` di PowerShell 5.1 aggiungono il BOM UTF-8: per file letti da altri
  programmi (RON, JSON) usare `[IO.File]::WriteAllText`.
- File temporanei in `out\` (ignorata da git).
- La GPU è una RTX 5080 (più una integrata AMD); l'app usa Vulkan sulla NVIDIA.

### Modo di consegna (ha funzionato bene finora)
- Lavorare su un **branch** dedicato (es. `m4-customizer`), commit piccoli e atomici con
  messaggi convenzionali (`feat(crate): …`, `fix(crate): …`).
- A fine lavoro scrivere `docs/<branch>-review.md` con: riepilogo, risultati delle verifiche,
  dettaglio commit per commit (cosa e perché), punti di attenzione per la revisione.
- Non fare push e non unire in `main`: la revisione e il merge li fa Claude.
- Aggiornare la sezione "Stato" di `CLAUDE.md` con una voce datata.

---

## 1. Stato attuale in breve

| Area | Stato |
|---|---|
| Linguaggio (parser + interprete) | Completo, 217/217 casi identici a OpenSCAD |
| Geometria (`crates/mesher`) | 99/100 casi equivalenti a OpenSCAD, 39 vertice per vertice |
| CLI `scadr` | `-o x.csg/.echo/.stl/.off`, `-D`, `-L`, `--enable`, `--export-format`; messaggi ed exit code di OpenSCAD |
| App | Apre `.scad`/`.stl`, calcolo in background, console, ricarica automatica, `#`/`%` trasparenti, export STL, impostazioni persistenti (frequenza fotogrammi costante per OLED/G-Sync) |
| Mancante | Customizer, editor integrato, clic modello→codice, import SVG/DXF/3MF/OBJ, export 3MF, compatibilità BOSL2 verificata, AI |

Da far confermare all'utente (non verificabile in automatico su questa macchina, i clic simulati
non arrivano alla finestra): interazione col mouse nel viewport, export dal menu, assenza di
flicker sul suo monitor OLED con la frequenza costante.

---

## 2. Milestone 4 — Customizer (priorità alta)

Obiettivo per l'utente: aprire un `.scad`, vedere un pannello con i parametri del progetto
(slider, caselle, menu a tendina), muoverli e vedere il modello aggiornarsi subito; salvare e
richiamare combinazioni di valori (preset) compatibili con OpenSCAD.

I file di prova reali sono `examples/porta_carte.scad` e `examples/separatore.scad`: usano tab
(`/* [Pezzo] */`, `/* [Hidden] */`), menu a tendina (`// [stampa, scatola, …]`), intervalli
(`// [30:1:270]`, `// [-0.25:0.05:0.6]`), booleani e stringhe, e commenti descrittivi sulla riga
precedente. Devono funzionare perfettamente.

### 2.1 Estrazione dei parametri (motore, nessuna UI)
- Nuovo modulo (in `scad-syntax` o nuovo crate `scad-customizer`) che, dal **CST lossless**,
  estrae i parametri del file principale: nome, valore di default, tipo, vincoli, descrizione,
  tab (gruppo), riga/span dell'assegnazione.
- Regole da ricavare **dall'oracolo**: OpenSCAD esporta i parametri con
  `openscad -o out.param file.scad` (formato `param`, JSON). Va usato per stabilire con precisione:
  - quali assegnazioni sono parametri (solo top-level, solo con valore letterale, fino a dove nel
    file, effetto di `/* [Hidden] */` e dei tab);
  - sintassi dei vincoli nel commento finale: `// [min:max]`, `// [min:step:max]`, `// [a, b, c]`,
    `// [10:Piccolo, 20:Grande]` (valore:etichetta), lunghezza massima delle stringhe, step soltanto;
  - descrizione dal commento della riga precedente;
  - tipi: numero, booleano, stringa, vettore di numeri (fino a 4 elementi).
- **Test di compatibilità**: aggiungere un corpus (es. `tests/compat/params/`) con casi `.scad`
  e il `.param` atteso generato da OpenSCAD, e una modalità `--params` in `tools/compat-runner`
  che confronta il JSON prodotto da noi (aggiungere a `scadr` l'export `-o x.param`).
  Includere i due file dell'utente.

### 2.2 Applicazione dei valori
- Usare `EvalOptions::defines` (`crates/scad-eval/src/eval.rs`): ha già la semantica di `-D` di
  OpenSCAD (sostituisce l'espressione nella posizione originale, senza warning di riassegnazione;
  i `-D` non validi danno un warning). Serve una funzione che formatti un valore del Customizer
  come letterale OpenSCAD (stringhe con escape, numeri con abbastanza cifre da non perdere
  precisione, vettori, booleani).

### 2.3 Preset compatibili con OpenSCAD
- Formato JSON di OpenSCAD (`<nome_file>.json` accanto al `.scad`):
  `{"parameterSets": {"nome": {"param": "valore"}}, "fileFormatVersion": "1"}` (verificare con
  l'oracolo: `openscad -p preset.json -P nome -o x.csg file.scad`).
- Leggere, scegliere, salvare, rinominare ed eliminare set, preservando i set e le chiavi esistenti.
- `scadr` deve supportare `-p file.json -P nome` come OpenSCAD, con test di confronto.

### 2.4 Interfaccia (crate `app`)
- Pannello "Parametri" (a destra o al posto del pannello Modello quando il file ha parametri):
  gruppi comprimibili per tab, un controllo per tipo:
  - numero con intervallo → slider + campo numerico (rispettando step e decimali);
  - numero senza intervallo → campo numerico con trascinamento;
  - lista di valori → menu a tendina (con etichette se presenti);
  - booleano → casella; stringa → campo di testo (con lunghezza massima); vettore → 2–4 campi.
- Descrizione come tooltip o testo sotto il controllo; pulsante per tornare al default (per
  parametro e globale); indicazione dei parametri modificati.
- Aggiornamento dal vivo: ricalcolo con *debounce* (~100–150 ms) mentre si muove lo slider; il
  worker (`crates/app/src/worker.rs`) annulla già il calcolo precedente. Mantenere ferma la
  telecamera (già così) e mostrare il tempo di calcolo.
- Preset: menu a tendina dei set + Salva / Salva come / Elimina.
- Quando il file su disco cambia (ricarica automatica), conservare i valori modificati se il
  parametro esiste ancora con lo stesso tipo.
- Persistenza: ricordare per file gli ultimi valori usati (nello storage di eframe, come le
  impostazioni in `crates/app/src/settings.rs`).

### 2.5 Scrittura dei valori nel file (facoltativo ma importante per il futuro)
- Comando "Applica al file": riscrive solo il letterale dell'assegnazione nel sorgente usando il
  CST rowan (commenti e formattazione intatti). È la stessa operazione che serviranno
  l'assistente AI e la modellazione diretta: farla come funzione riutilizzabile in `scad-syntax`
  (es. `replace_assignment_value(text, name, new_literal) -> Result<String, _>`) con test.

### 2.6 Prestazioni
- Valutazione ~1 ms e geometria 40–80 ms sui modelli dell'utente (build di debug): basta per lo
  slider dal vivo. Misurare anche in release. Se un modello reale supera ~200 ms, valutare una
  cache dei risultati per sotto-albero CSG (hash del nodo → mesh) nel mesher; documentare la
  scelta prima di implementarla.

### 2.7 Criteri di accettazione M4
- Corpus parametri: tutti i casi uguali a OpenSCAD; preset: `-p/-P` uguale a OpenSCAD.
- Nei file dell'utente compaiono tutti i parametri con i tab giusti; cambiando `PEZZO`,
  `N_FILE`, `LUNGHEZZA`, `SERRAGGIO`, `TESTO`, `FINESTRA` il modello si aggiorna e coincide con
  l'export di OpenSCAD con gli stessi `-D` (verificabile con `compat-runner --geometry` e la
  direttiva `// compat-args:`).
- Nessuna regressione nelle verifiche del §0.

---

## 3. Backlog dopo M4 (in ordine di valore per l'utente)

### 3.1 Clic sul modello → codice
- Il mesher fornisce già `RenderOutput::triangle_sources` + `sources` (provenienza di ogni
  triangolo, anche attraverso le booleane) e `Node::source`/`root_call`; l'interprete ha
  `Evaluator::resolve_source`.
- Serve il *picking*: un ID-buffer nel renderer (`crates/render/src/scene.rs`, seconda target
  di colore con l'indice del triangolo) oppure un raycast su CPU. Al clic mostrare file, riga e
  snippet; evidenziare in giallo tutti i triangoli dello stesso sorgente.

### 3.2 Editor integrato
- Pannello editor con evidenziazione della sintassi basata sul nostro lexer (`scad-syntax`),
  numeri di riga, errori di parsing sottolineati, Ctrl+S che salva e fa ricalcolare.
  Collegarlo al §3.1 (clic sul modello → cursore sulla riga). Il motore deve poter valutare il
  testo del buffer non salvato (oggi il worker legge dal disco).

### 3.3 Export 3MF (e OBJ)
- 3MF è il formato preferito dagli slicer moderni (unità in mm, più oggetti, colori). Scrittura
  in `crates/mesh-io`, menu nell'app, `-o x.3mf` in `scadr` con i messaggi di OpenSCAD.

### 3.4 Lacune note della geometria
- `linear_extrude` con scala non uniforme senza twist: OpenSCAD suddivide i lati con una regola
  non ancora trovata (caso `lex_scale_vector`, area +0,01%).
- Sfera `style="octa"` con `$fn > 12`: punti interni sbagliati di ~0,05.
- `linear_extrude(segments=…)` gestito solo in parte; `import(center/scale)` per i file 3D ignorati.
- Import SVG / DXF (crate `usvg`, `dxf`), 3MF / OBJ; `surface()` da PNG.
- Ogni correzione va accompagnata da casi in `tests/compat/geometry` (generati da OpenSCAD).

### 3.5 Milestone 6 — compatibilità con librerie reali
- Installare BOSL2 e MCAD in una cartella di librerie, scrivere un corpus di esempi che le
  usano, confrontare testo e geometria con OpenSCAD. Misurare le prestazioni dell'interprete
  su BOSL2 (molto più pesante dei file dell'utente); se serve, profilare prima di ottimizzare.

### 3.6 Distribuzione
- Build release con icona, `windows_subsystem` (già impostato in release), installer (es.
  `cargo-packager`), nome definitivo del prodotto (decisione dell'utente).

### 3.7 Milestone 7–8 — AI e modellazione diretta
- Rimandate. Le basi sono pronte: CST lossless per modifiche chirurgiche al codice, rendering
  headless in PNG (`render::render_image`) come "occhi" del modello, provenienza
  triangolo→sorgente. Richiedono decisioni di prodotto dell'utente prima di iniziare.
