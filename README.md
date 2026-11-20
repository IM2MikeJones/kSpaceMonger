# kSpaceMonger

A fast disk/folder space map for Linux, in the classic SpaceMonger style: readable colors, clear labels, and quick drill-down through directory hierarchies.

**Version:** 0.2.0-dev (unreleased — develop on branch `v0.2`)

0.2 UI: modern chrome with a normal **File / View / Item / Settings / Help** menu bar (Win95 toolbar removed). The classic treemap color schemes remain.

## Build

```bash
cargo build --release
./target/release/kSpaceMonger [FOLDER]
```

Debug build:

```bash
cargo run -- [FOLDER]
```

## Notes

- Settings live under `~/.config/kspacemonger/`
- Optional: `kSpaceMonger --screenshot out.png [FOLDER]` for a headless capture
