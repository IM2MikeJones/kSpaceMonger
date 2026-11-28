# kSpaceMonger

A fast disk/folder space map for Linux, in the classic SpaceMonger style: readable colors, clear labels, and quick drill-down through directory hierarchies.

**Version:** 1.0.0

## What you can scan

| Target | Support |
|--------|---------|
| **Local Linux** disks and folders | Yes — scanned in-process |
| **sshfs → remote Linux** (x86_64) | Yes — headless `ksm-agent` over SSH (no FUSE walk) |
| **sshfs → remote Windows** (x86_64) | Yes — headless `ksm-agent.exe` over SSH (no FUSE walk) |

Remote scans need **working SSH** (key/agent auth suitable for BatchMode) and a **writable** home or temp directory on the remote so the agent can install. Mount / host alias names are not used to detect the OS.

## What’s in 1.0.0

- Everything from the 0.3 line (Spectrum/Pastel, zoom frame, type-find, AppImage + `.deb`, …)
- Remote SSH agent for Linux and Windows remotes; status chips; failure dialog (no silent FUSE fallback)
- Desktop Categories/Keywords; `.deb` postinst/postrm refresh desktop DB and icon cache

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
# needs: rustc/cargo, cargo-xwin, dpkg-deb, appimagetool (e.g. ~/.local/bin/appimagetool)
# Windows agent target: rustup target add x86_64-pc-windows-msvc
./packaging/make-release.sh
# → dist/kSpaceMonger-*-x86_64.AppImage
# → dist/kspacemonger_*_amd64.deb
# (includes ksm-agent + ksm-agent.exe)
```

## Build from source

```bash
cargo build --release --bins
# optional Windows remote agent (for sshfs to Windows hosts):
cargo xwin build --release --bin ksm-agent --no-default-features --target x86_64-pc-windows-msvc
cp target/x86_64-pc-windows-msvc/release/ksm-agent.exe target/release/
./target/release/kSpaceMonger [FOLDER]
```

Debug: `cargo run -- [FOLDER]`

Optional SSH smoke (probe + deploy only):  
`cargo run --release --example remote_smoke -- <ssh-host>`

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
