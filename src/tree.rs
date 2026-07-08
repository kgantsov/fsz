use ignore::{WalkBuilder, WalkState};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// Which notion of "size" the tree totals up. Both are captured per file
/// during the scan, so switching between them afterward is a cheap in-memory
/// recompute — no rescan (see [`Tree::toggle_mode`]).
#[derive(Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum SizeMode {
    /// Logical size (`st_size` / `len()`): the bytes the file *contains*.
    Apparent,
    /// On-disk footprint (`st_blocks` × 512): the blocks actually allocated,
    /// which for many small files rounds up well past the apparent size.
    #[default]
    Allocated,
}

impl SizeMode {
    /// Lower-case name shown in the header and used as the CLI value.
    pub fn label(self) -> &'static str {
        match self {
            SizeMode::Apparent => "apparent",
            SizeMode::Allocated => "allocated",
        }
    }

    /// The other mode — used to flip on the interactive toggle.
    fn toggled(self) -> SizeMode {
        match self {
            SizeMode::Apparent => SizeMode::Allocated,
            SizeMode::Allocated => SizeMode::Apparent,
        }
    }
}

/// A running snapshot of scan progress, handed to `Tree::build`'s callback.
/// `Copy` so it can be passed by value cheaply on every entry.
#[derive(Clone, Copy, Default)]
pub struct Progress {
    /// Entries seen so far (files *and* directories).
    pub entries: u64,
    /// Deduplicated bytes counted so far — tracks the eventual grand total.
    pub bytes: u64,
}

/// A single filesystem entry (file or directory) in the arena.
///
/// Nodes reference each other by their index into `Tree::nodes`, never by
/// pointer or path — that's what keeps navigation cheap and the store
/// contiguous. `name` is only the final path component; reconstruct a full
/// path by walking `parent` links when you actually need it.
pub struct Node {
    pub name: OsString,
    /// This entry's own apparent bytes (`st_size`), or 0 for a directory.
    pub own_apparent: u64,
    /// This entry's own allocated bytes (`st_blocks` × 512), or 0 for a
    /// directory. Both are stored so the mode can flip without a rescan.
    pub own_allocated: u64,
    /// The current mode's own size plus every descendant's, for the active
    /// [`SizeMode`]. Filled in post-order and recomputed on a mode change.
    pub total_size: u64,
    pub children: Vec<usize>,
    /// `None` only for the root. The TUI navigates "up" through it.
    pub parent: Option<usize>,
}

impl Node {
    /// A node is a directory if it has children. Empty directories look like
    /// files here, which is fine: there's nothing to descend into either way.
    pub fn is_dir(&self) -> bool {
        !self.children.is_empty()
    }

    /// This node's own size under `mode`.
    fn own_size(&self, mode: SizeMode) -> u64 {
        match mode {
            SizeMode::Apparent => self.own_apparent,
            SizeMode::Allocated => self.own_allocated,
        }
    }
}

/// Arena-backed tree: all nodes live in one `Vec`, addressed by `usize`.
pub struct Tree {
    pub nodes: Vec<Node>,
    /// Maps a path to its node index. Used only while building; the hot
    /// interactive path uses indices and never touches this.
    index: HashMap<PathBuf, usize>,
    root: PathBuf,
    pub root_idx: usize,
    /// Which size the totals currently reflect. Flip with [`Tree::toggle_mode`].
    mode: SizeMode,
}

impl Tree {
    /// Build the tree, invoking `on_progress` as entries stream in from the
    /// walk. `on_progress` receives a running [`Progress`] snapshot; it fires
    /// once per entry (potentially millions of times), so a caller that draws
    /// to the screen must throttle itself.
    pub fn build(root: &Path, mode: SizeMode, mut on_progress: impl FnMut(Progress)) -> Self {
        let mut tree = Tree {
            nodes: Vec::new(),
            index: HashMap::new(),
            root: root.to_path_buf(),
            root_idx: 0,
            mode,
        };

        // Ensure the root node exists even if the path is empty/unreadable.
        tree.root_idx = tree.intern(root);

        // Phase 1 — walk in parallel on a dedicated thread. This is the
        // I/O-bound half (directory reads and a `stat` per entry), so we let
        // `ignore`'s worker pool saturate the disk. Workers only ship raw
        // records over a channel; they touch no shared tree state, so there's
        // no lock on the hot path. Running the walk off the main thread lets
        // Phase 2 fold records *as they arrive*, which is what makes live
        // progress possible — otherwise the whole walk would finish before the
        // caller heard a thing.
        let (tx, rx) = mpsc::channel::<(PathBuf, Option<(u64, u64, u64, u64)>)>();
        let walk_root = root.to_path_buf();
        let walker = std::thread::spawn(move || {
            WalkBuilder::new(&walk_root)
                .standard_filters(false)
                .build_parallel()
                .run(|| {
                    let tx = tx.clone();
                    Box::new(move |result| {
                        if let Ok(entry) = result {
                            // For files, capture (dev, ino, apparent, allocated);
                            // dirs send None. Apparent is `len()` (`st_size`);
                            // allocated is the 512-byte blocks actually on disk
                            // (`st_blocks`). Both ride along so the mode can flip
                            // later without re-walking the filesystem.
                            let file = entry.metadata().ok().and_then(|m| {
                                m.is_file()
                                    .then(|| (m.dev(), m.ino(), m.len(), m.blocks() * 512))
                            });
                            let _ = tx.send((entry.path().to_path_buf(), file));
                        }
                        WalkState::Continue
                    })
                });
            // Every `tx` clone drops as the pool winds down; the last one drops
            // here when the closure returns, which is what ends the `rx` loop.
        });

        let mut seen: HashSet<(u64, u64)> = HashSet::new();
        let mut progress = Progress::default();
        for (path, file) in rx {
            let idx = tree.intern(&path);
            progress.entries += 1;
            if let Some((dev, ino, apparent, allocated)) = file
                && seen.insert((dev, ino))
            {
                tree.nodes[idx].own_apparent = apparent;
                tree.nodes[idx].own_allocated = allocated;
                progress.bytes += match mode {
                    SizeMode::Apparent => apparent,
                    SizeMode::Allocated => allocated,
                };
            }
            on_progress(progress);
        }
        // The walk is drained; joining surfaces any panic from a worker thread.
        let _ = walker.join();

        tree.compute_totals(tree.root_idx);
        tree
    }

    /// Return the index for `path`, creating the node (and any missing
    /// ancestors up to the root) on first sight. Order-independent, so it
    /// doesn't rely on the walker's traversal order.
    fn intern(&mut self, path: &Path) -> usize {
        if let Some(&idx) = self.index.get(path) {
            return idx;
        }

        // The root is the recursion's base case; every other node interns its
        // parent first so the child link can be attached.
        let parent = if path == self.root {
            None
        } else {
            match path.parent() {
                Some(p) if !p.as_os_str().is_empty() => Some(self.intern(p)),
                _ => None,
            }
        };

        let name = path
            .file_name()
            .map(OsString::from)
            .unwrap_or_else(|| path.as_os_str().to_os_string());

        let idx = self.nodes.len();
        self.nodes.push(Node {
            name,
            own_apparent: 0,
            own_allocated: 0,
            total_size: 0,
            children: Vec::new(),
            parent,
        });
        self.index.insert(path.to_path_buf(), idx);

        if let Some(p) = parent {
            self.nodes[p].children.push(idx);
        }
        idx
    }

    /// Post-order pass: a node's total is its own size plus the totals of all
    /// its children. Returns the subtotal so the parent can accumulate it.
    ///
    /// NOTE: recursive, so depth is bounded by filesystem depth. Fine for now;
    /// swap for an explicit stack if it ever overflows on pathological trees.
    fn compute_totals(&mut self, idx: usize) -> u64 {
        let mut total = self.nodes[idx].own_size(self.mode);
        let child_count = self.nodes[idx].children.len();
        for k in 0..child_count {
            let child = self.nodes[idx].children[k];
            total += self.compute_totals(child);
        }
        self.nodes[idx].total_size = total;
        total
    }

    /// The size mode the totals currently reflect.
    pub fn mode(&self) -> SizeMode {
        self.mode
    }

    /// Flip between apparent and allocated size and recompute every
    /// `total_size` from the already-captured per-node sizes. Purely
    /// in-memory — no filesystem access — so it's effectively instant.
    pub fn toggle_mode(&mut self) {
        self.mode = self.mode.toggled();
        self.compute_totals(self.root_idx);
    }

    /// A node's children, largest total first — the order the TUI lists them.
    pub fn children_by_size(&self, idx: usize) -> Vec<usize> {
        let mut children = self.nodes[idx].children.clone();
        children.sort_by(|&a, &b| self.nodes[b].total_size.cmp(&self.nodes[a].total_size));
        children
    }

    /// Remove `idx` from disk and detach it from the tree. Directories go via
    /// `remove_dir_all` (contents and all), everything else via `remove_file`;
    /// `symlink_metadata` keeps us from following a symlinked directory and
    /// deleting its target's contents. On success the node is unlinked from its
    /// parent and its `total_size` is subtracted from every ancestor, so the
    /// bars and header stay correct without a rescan. The node itself stays in
    /// the arena, just unreferenced — indices elsewhere remain valid.
    pub fn delete(&mut self, idx: usize) -> io::Result<()> {
        let path = self.path_of(idx);
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }

        let freed = self.nodes[idx].total_size;
        if let Some(parent) = self.nodes[idx].parent {
            self.nodes[parent].children.retain(|&c| c != idx);
            let mut cur = Some(parent);
            while let Some(i) = cur {
                self.nodes[i].total_size = self.nodes[i].total_size.saturating_sub(freed);
                cur = self.nodes[i].parent;
            }
        }
        Ok(())
    }

    /// Reconstruct a node's full path by walking `parent` links back to the
    /// root and rejoining the stored name components.
    pub fn path_of(&self, idx: usize) -> PathBuf {
        let mut parts: Vec<&OsString> = Vec::new();
        let mut cur = Some(idx);
        while let Some(i) = cur {
            parts.push(&self.nodes[i].name);
            cur = self.nodes[i].parent;
        }
        let mut path = PathBuf::new();
        for name in parts.into_iter().rev() {
            path.push(name);
        }
        path
    }
}
