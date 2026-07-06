mod tree;
mod ui;

use clap::Parser;
use human_bytes::human_bytes;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tree::Tree;
use ui::App;

/// Braille spinner frames, cycled while the scan runs.
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Don't repaint the progress line faster than this — the callback fires once
/// per entry, which is far more often than a human (or terminal) can use.
const REDRAW_EVERY: Duration = Duration::from_millis(80);

#[derive(Parser)]
struct Cli {
    /// The path to analyze. Defaults to the current directory.
    #[arg(default_value = ".")]
    path: PathBuf,
}

fn main() -> io::Result<()> {
    let args = Cli::parse();

    // Scanning blocks until the whole tree is built, so show a live spinner on
    // the normal screen *before* handing the terminal to ratatui. Throttled to
    // REDRAW_EVERY; `frame` advances the spinner only when we actually repaint.
    let mut last = Instant::now() - REDRAW_EVERY;
    let mut frame = 0usize;
    let tree = Tree::build(&args.path, |p| {
        if last.elapsed() < REDRAW_EVERY {
            return;
        }
        last = Instant::now();
        frame = (frame + 1) % SPINNER.len();
        eprint!(
            "\r\x1b[2K{} Scanning… {} entries, {}",
            SPINNER[frame],
            p.entries,
            human_bytes(p.bytes as f64),
        );
        let _ = io::stderr().flush();
    });
    // Wipe the progress line so nothing lingers behind the TUI (or after quit).
    eprint!("\r\x1b[2K");
    let _ = io::stderr().flush();

    let mut terminal = ratatui::init();
    let result = App::new(&tree).run(&mut terminal);
    ratatui::restore();
    result
}
