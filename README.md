# kSpaceMonger

A fast disk/folder space map for Linux, in the classic SpaceMonger style: readable colors, clear labels, and quick drill-down through directory hierarchies.

**Version:** 0.3.0

## What’s in 0.3

- **Spectrum** and **Pastel** colour schemes (alongside classic Rainbow)
- Zoom keeps the opened folder as the **outer frame** (contents inset inside)
- Folder selection highlights the **full rectangle**
- Click again to **toggle** selection off
- **Type-find:** with a folder selected, type a character to cycle visible children whose names start with that character (amber ring; 90 ms dwell; keystrokes during dwell dropped)
- Hover tips prefer **above** the cursor
- Colour palette editor: open `color-editor.html` in a browser, then copy Rust consts back into the app
- Linux packages: **AppImage** and **`.deb`** (menu icon + desktop entry)

## Install (Linux x86_64)

### Debian / Ubuntu / KDE Neon (`.deb`)

Download `kspacemonger_VERSION_amd64.deb` from the [GitHub Releases](https://github.com/IM2MikeJones/kSpaceMonger/releases) page, then:

```bash
sudo dpkg -i kspacemonger_VERSION_amd64.deb
```

Find **kSpaceMonger** in the application menu (Kickoff, GNOME Activities, etc.).

Remove with: `sudo apt remove kspacemonger`

### AppImage

Download `kSpaceMonger-VERSION-x86_64.AppImage`, then:

```bash
chmod +x kSpaceMonger-VERSION-x86_64.AppImage
./kSpaceMonger-VERSION-x86_64.AppImage [FOLDER]
```

Optional: use AppImageLauncher (or your DE’s “Integrate”) so it appears in the menu permanently.

### Build packages yourself

```bash
# needs: rustc/cargo, dpkg-deb, appimagetool (e.g. ~/.local/bin/appimagetool)
./packaging/make-release.sh
# → dist/kSpaceMonger-*-x86_64.AppImage
# → dist/kspacemonger_*_amd64.deb
```

## Build from source

```bash
cargo build --release
./target/release/kSpaceMonger [FOLDER]
```

Debug: `cargo run -- [FOLDER]`

## Credits

**kSpaceMonger** is a Linux rebuild inspired by the classic Windows **SpaceMonger** disk space map (the 1.x line, including the 1.3/1.4 freeware that this project studied).

| Role | Credit |
|------|--------|
| Original SpaceMonger 1.x | **Sean Werkema** — Copyright © 1998, 1999, and 2000 |
| Contact (historical) | sean@werkema.com · [werkema.com/software/spacemonger](http://www.werkema.com/software/spacemonger.html) |
| Preserved 1.4 source | [github.com/seanofw/spacemonger1](https://github.com/seanofw/spacemonger1) |
| Post-mortem / history | [The SpaceMonger 1.x Post-Mortem](https://www.werkema.com/programming/the-spacemonger-1-x-post-mortem/) |
| Treemap concept (prior art) | Dr. Ben Shneiderman (1990); Sean independently rediscovered the idea for SpaceMonger |
| Later commercial line | SpaceMonger 2.x (Sixty-Five / Sean Werkema); SpaceMonger 3.x (Edgerunner / Stardock partnership with Sean Werkema) — **not** the basis of this Linux rebuild |

kSpaceMonger is an independent reimplementation for Linux. It is not affiliated with Sean Werkema, Sixty-Five, Edgerunner, or Stardock.

## Notes

- Settings: `~/.config/kspacemonger/`
- Optional: `kSpaceMonger --screenshot out.png [FOLDER]`
- GitHub: https://github.com/IM2MikeJones/kSpaceMonger
