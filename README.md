# kSpaceMonger

A fast disk/folder space map for Linux, in the classic SpaceMonger style: readable colors, clear labels, and quick drill-down through directory hierarchies.

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
