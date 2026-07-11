# fsz

`fsz` (**F**ile/Folder **S**ize) is a fast, interactive terminal disk usage analyzer
written in Rust. Point it at a directory and it scans the tree in parallel, then drops
you into a keyboard-driven TUI where you can drill down into whatever is eating your
disk — biggest folders first, every step of the way.


![Screenshot-6](/.github/screenshots/Screenshot-6.png)

![Screenshot-7](/.github/screenshots/Screenshot-7.png)

## Features

- **Parallel scan.** Traversal runs on `ignore`'s worker pool on a dedicated thread,
  saturating your disk while a live spinner reports entries and bytes seen so far.
- **On-disk sizes, not apparent sizes.** Each file is measured by its *allocated*
  size (`st_blocks × 512`), so the totals reflect what the filesystem actually spends —
  sparse files and block rounding included.
- **Hardlink-aware.** Files sharing an inode are counted once, so totals don't
  double-count hardlinked content.
- **Interactive navigation.** Built on [`ratatui`](https://ratatui.rs). Every directory
  lists its children largest-first, each with a proportional size bar. Descend, step
  back up, and the folder you came from stays highlighted.
- **Delete from the TUI.** Hit `Ctrl` + `D` to remove the selected file or folder
  (recursively) right where you spotted it, behind a confirmation prompt — the size
  totals update instantly, no rescan needed.
- **Single static binary.** No runtime, no interpreter — just `cargo build --release`.

## Installation

### Install script (macOS & Linux)

The quickest way to get `fsz` is the install script, which detects your OS
(macOS or Linux) and architecture (Apple Silicon / ARM64 or Intel / x86_64),
downloads the latest prebuilt binary, and installs it to `/usr/local/bin`:

```bash
curl -fsSL https://raw.githubusercontent.com/kgantsov/fsz/main/install.sh | bash
```

Or download and run it manually if you'd rather inspect it first:

```bash
curl -fsSLO https://raw.githubusercontent.com/kgantsov/fsz/main/install.sh
bash install.sh
```

The script may prompt for `sudo` to write to `/usr/local/bin`. Once it finishes,
run `fsz --help` to verify.

### Build from source

Requires a Rust toolchain (edition 2024). Works on any supported Unix-like system.

```bash
git clone <repo-url> fsz
cd fsz
cargo build --release
# binary lands at ./target/release/fsz
```

> **Platform note:** `fsz` currently uses Unix inode/block metadata (`st_dev`,
> `st_ino`, `st_blocks`) and targets Unix-like systems (macOS, Linux). Windows is not
> supported yet.

## Usage

```bash
fsz [PATH]
```

`PATH` defaults to the current directory. `fsz` scans the whole tree first (with a
progress spinner), then opens the interactive view.

```bash
fsz              # analyze the current directory
fsz ~/Downloads  # analyze a specific directory
cargo run -- .   # run from source
```

Unlike tools that honor `.gitignore`, `fsz` deliberately walks **everything** —
ignored and hidden files included — so the totals reflect real disk usage.

### Keys

| Key                     | Action                                  |
| ----------------------- | --------------------------------------- |
| `↑` / `k`               | Move selection up                       |
| `↓` / `j`               | Move selection down                     |
| `→` / `l` / `Enter`     | Enter the selected directory            |
| `←` / `h` / `Backspace` | Go back to the parent directory         |
| `Home` / `End`          | Jump to the first / last entry          |
| `Ctrl` + `D`            | Delete the selected entry (asks first)  |
| `q` / `Esc`             | Quit (asks for confirmation)            |

> **Warning:** `Ctrl` + `D` deletes permanently — files and folders are removed
> from disk (not moved to Trash), and directories are deleted recursively. There is
> a confirmation prompt, but no undo.

## Development

```bash
cargo run -- .           # run against a directory
cargo build --release    # optimized build
cargo clippy             # lint
cargo fmt                # format
cargo test               # tests (none yet)
```
