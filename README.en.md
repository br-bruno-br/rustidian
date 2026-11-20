<div align="center">

# Rustidian

<table>
  <tr>
    <td align="center"><img src="docs/brand/themes/signature.png" width="112" height="112" alt="Rustidian logo, Signature theme"><br><sub>Signature</sub></td>
    <td align="center"><img src="docs/brand/rustidian.png" width="112" height="112" alt="Rustidian logo, teal"><br><sub>Teal</sub></td>
    <td align="center"><img src="docs/brand/themes/love-space.png" width="112" height="112" alt="Rustidian logo, Love Space theme"><br><sub>Love Space</sub></td>
    <td align="center"><img src="docs/brand/themes/elpis-gold.png" width="112" height="112" alt="Rustidian logo, Elpis Gold theme"><br><sub>Elpis Gold</sub></td>
  </tr>
</table>

**A native Markdown notebook in Rust. Your files, your folders, no account.**

[Português](README.md) · English

</div>

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/images/rustidian-light.png">
  <img src="docs/images/rustidian.png" alt="Rustidian showing the bundled demo vault">
</picture>

## What it is

Rustidian keeps your notes in ordinary folders of Markdown files. No account, and it opens Obsidian vaults directly with no import. Everything is native Rust, from the UI to the editor.

This repository is a fork of [Rustidian by Masih Moafi](https://github.com/MasihMoafi/rustidian). The fork's goal is to use one editor on Windows, Android and Linux in place of Obsidian. Independent, not affiliated with Obsidian.

## What works today

**On the desktop (Windows and Linux)**
- A **Windows** build with native file dialogs and the system theme and fonts.
- **Brazilian Portuguese UI** and a **pt-BR spell checker** (the dictionary is downloaded from a button and checked by sha256).
- **Split view**: two panes, side by side or stacked, each with its own tabs. The same note in both panes is a single note.
- **Tables**: insert, paste from a spreadsheet, convert a selection, align.
- **Unlinked mentions**: the backlinks panel lists notes that mention the open note's title, and a **Link** button turns the mention into a `[[link]]`.
- **Large vaults and a reliable file watcher**: incremental index, tens of thousands of notes, and no note lost to sync or external editors.
- **Signed updates**: the app only installs what matches the embedded public key. The owner's public key is already in the app; the first signed release ships with the Windows installer.

**Heptabase-style board** (`.canvas` files, the same open format Obsidian uses)
- Each note becomes a **live card** that shows formatted text and edits the note in place.
- **Section as a card** (just part of a note) and **board inside a board** (a thumbnail of the smaller board).
- **Arrange**: align, distribute, grid, and **mind map** from a card.
- **Tags and filter** by color and by tag.

**Note history**
- Every note keeps its versions, stored as diffs (small on disk), with restore and a diff view.
- **Timeline** with a slider, **named versions** and **delete from history**.
- Retention is configurable in settings.

**Encrypted sync**
- The pieces exist and are tested: `nota-cripto` (end-to-end encryption), `nota-sync` and `nota-servidor` (the server only stores ciphertext). The desktop app has a settings screen, a status icon and a background cycle. When unsure, it keeps both versions.
- Not in daily use yet: the dedicated server and the backup are in the queue below.

**Android (early stage)**
- A Dioxus app with a note list, search, an editor with autosave, and a trash. It still shows raw text and does not sync yet.

## Your data is safe

Guarantees that exist in the code, with the number of `#[test]` functions in each area (counted with `grep`, not compiled; this is not coverage).

| Guarantee | How | Tests |
| --- | --- | --- |
| A note is never deleted for good | Whatever goes away moves to the vault's `.trash/` (a `rename`, never `rm`), and the app asks for confirmation before trashing a note (the prompt can be turned off) | `crates/nota-sync` 62; UI in `src/app/gui_tests` 270 |
| Conflicts lose nothing | If a note changed on both sides, one version becomes the main file and the other is saved as `name (conflict device date).md` | `crates/nota-sync` 62 |
| The server cannot read your notes | End-to-end encryption: the server stores only ciphertext. Every device keeps a full copy and works offline | `crates/nota-cripto` 19, `crates/nota-servidor` 20 |
| No half-written files | Atomic writes: a temp file in the same folder, `sync_all`, then `rename` over the target | `crates/nota-sync` 62 |
| Notes that could be damaged are not opened | A note that is too large or not UTF-8 is not opened, so the file is not corrupted. Limits: 8 MB for the desktop index (a bigger `.md` stays out of the index, with a warning) and 2 MB on Android | `src/app/gui_tests` 270; `android/` 51 |
| The file watcher does not drop notes | A note the scan did not find leaves the index only after it is checked twice | `src/app/gui_tests` 270 |
| Note history | Automatic diff-based versions, configurable retention (default: keep everything for 1 day), restore, named versions (never pruned), and "Delete from history" removes a snippet from every version, with no undo | `crates/nota-historico` 65 |
| Updates are the ones the owner signed | The app only installs what matches the embedded public key (ed25519 and SHA256SUMS). The owner's key is generated and the public half is in the repository | `src/atualizacao.rs` 13 |
| Undo that survives closing the app | **In progress** (history, phases H3/H4) | not yet |
| Encrypted sync on Android and the Google Drive backup | **In progress**, see the roadmap | not yet |

## Roadmap

The queue moves **one track at a time**: parallel Rust builds freeze the owner's 2-core PC, and each slice starts only after the previous one is verified and merged. No dates.

| Now | Next | Later |
| --- | --- | --- |
| **History:** an undo that survives closing the app, and a way to find what I deleted | **Formatted Markdown on Android** (read notes like on desktop, with tables and tasks) | **Windows release** with an installer (coming soon) |
| | **Encrypted sync on Android** (address, token and passphrase, status and conflicts) | **Portable Linux** package signed by the owner |
| | **Our own sync server** (a home server, reachable only over Tailscale) **and an encrypted Google Drive backup** | Finish replacing the Python scripts with Rust (`cargo xtask` and `egui_kittest` tests) |

Status of each step: [docs/bruno/FILA-ANDROID-SYNC.md](docs/bruno/FILA-ANDROID-SYNC.md). Full plan, with slices and what depends on the owner: [docs/bruno/PLANO.md](docs/bruno/PLANO.md). Both are written in Portuguese.

## Building

This fork has no release of its own yet. Windows release: coming soon. For now, build it.

**Windows** (Rust with the MSVC target):

```powershell
cargo build --release --bin rustidian
.\target\release\rustidian.exe
```

The executable ends up at `target\release\rustidian.exe`. Use **File → Open folder...** and pick a copy of your vault.

**Linux** (x86_64, glibc 2.39 or newer such as Ubuntu 24.04; X11 or Wayland):

```sh
cargo build --release --bin rustidian
./target/release/rustidian "/path/to/vault"
```

The Open dialogs use `zenity`; install it with your package manager. The `.deb`, AppImage and snap packages mentioned in the original's docs belong to the original project, not this fork.

Shortcuts: Ctrl+P command palette, Ctrl+O quick switcher, Ctrl+N new note, Ctrl+E toggle reading/editing, Ctrl+Shift+F vault search, Ctrl+S save. Autosave runs after 600 ms idle. Obsidian community plugins do not run.

## Measuring slowness

With `RUSTIDIAN_PERF=<file>`, the app appends one JSON line per action (`abrir_cofre`, `salvar`, `buscar`, `trocar_aba`, `quadro_lento` and more) with the time in ms and the vault size. Without the variable it opens no file and reads no clock. In PowerShell:

```powershell
$env:RUSTIDIAN_PERF = "$env:TEMP\rustidian-perf.jsonl"; .\target\release\rustidian.exe
```

About speed: the original author measured 0.61 s against Obsidian's 3.77 s to open a note, on Linux, with an older build and a single setup ([test](docs/performance/startup-2026-10-22/README.md)). It is not a measurement of this fork or of Windows.

## Credits and license

- Original: [Rustidian by Masih Moafi](https://github.com/MasihMoafi/rustidian).
- The ideas behind unlinked mentions, split view and boards come from [RustSidian](https://github.com/siddarth1872004/RustSidian) (MIT); the code is ours.
- Spell checker: [spellbook](https://github.com/helix-editor/spellbook) with the VERO dictionary (LibreOffice).
- Third-party libraries: [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt).
- License: [MIT](LICENSE).

## Contributing to the original

Fixes that help everyone go on `upstream/<topic>` branches, each with one clean English commit, to become a Pull Request on the original. Fork-only work (Windows, sync, Android, the owner's server) stays on `main`. See [docs/bruno/PLANO.md](docs/bruno/PLANO.md) and [CONTRIBUTING.md](CONTRIBUTING.md).
