# Customizer Parameter Compatibility Corpus (`-o <name>.param`)

Test cases comparing Customizer parameter extraction, `.param` JSON export, range-adjustment
warnings, `-D` interaction, and `-p`/`-P` preset overrides produced by `scadr` against
**OpenSCAD Nightly 2026.10.22**. Each case is run with:

```
<engine> -o <name>.param [compat-args] <name>.scad
```

Both `expected/<name>.param` (compact single-line JSON with sorted keys and no trailing newline)
and `expected/<name>.log` (normalized stderr messages) are compared byte-for-byte. Observed
behavior is documented in [`docs/openscad-behavior.md`](../../../docs/openscad-behavior.md) §17.

Expected files are checked into the repository: for normal runs **OpenSCAD is not required**.

## Layout

```
tests/compat/params/
  cases/<name>.scad               one case per file
  cases/preset_fixture.json       preset fixture used by param_cli_preset.scad
  expected/<name>.param           exact .param JSON recorded from OpenSCAD
  expected/<name>.log             normalized stderr recorded from OpenSCAD
```

## Running

```powershell
cargo build -p scadr
cargo run -p compat-runner -- --params
cargo run -p compat-runner -- --params --filter porta -v
```

## Regenerating expected files

Never edit `expected/*` by hand; regenerate strictly from OpenSCAD:

```powershell
cargo run -p compat-runner -- --params --update-expected
```
