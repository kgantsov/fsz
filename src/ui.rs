use crate::tree::Tree;
use human_bytes::human_bytes;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use std::io;

/// Width, in cells, of the little size bar drawn in front of each entry.
const BAR_WIDTH: usize = 16;

/// Interactive navigator over an already-built [`Tree`]. Holds only a cursor
/// into the arena (`current` directory + the selected row); the tree itself is
/// immutable, so navigation is just swapping indices and re-sorting children.
pub struct App<'a> {
    tree: &'a Tree,
    /// Index of the directory currently being listed.
    current: usize,
    /// `current`'s children, largest first — cached so we sort once per move.
    children: Vec<usize>,
    state: ListState,
    /// When true, a "really quit?" popup is up and captures all input — so an
    /// accidental `q`/`Esc` can't drop you out of the app without a second,
    /// deliberate confirmation.
    confirm_quit: bool,
    quit: bool,
}

impl<'a> App<'a> {
    pub fn new(tree: &'a Tree) -> Self {
        let mut app = App {
            tree,
            current: tree.root_idx,
            children: Vec::new(),
            state: ListState::default(),
            confirm_quit: false,
            quit: false,
        };
        app.rebuild(None);
        app
    }

    /// Refresh the cached child list for `current` and place the cursor. When
    /// `select` names a child index, select that row (used when stepping *up*,
    /// so the folder you came from stays highlighted); otherwise select the
    /// first row.
    fn rebuild(&mut self, select: Option<usize>) {
        self.children = self.tree.children_by_size(self.current);
        let pos = select
            .and_then(|child| self.children.iter().position(|&c| c == child))
            .unwrap_or(0);
        self.state
            .select((!self.children.is_empty()).then_some(pos));
    }

    fn selected_child(&self) -> Option<usize> {
        self.state.selected().map(|row| self.children[row])
    }

    /// Descend into the highlighted row if it's a directory.
    fn enter(&mut self) {
        if let Some(child) = self.selected_child()
            && self.tree.nodes[child].is_dir()
        {
            self.current = child;
            self.rebuild(None);
        }
    }

    /// Step up to the parent directory, re-selecting the folder we left.
    fn go_up(&mut self) {
        if let Some(parent) = self.tree.nodes[self.current].parent {
            let came_from = self.current;
            self.current = parent;
            self.rebuild(Some(came_from));
        }
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.quit {
            terminal.draw(|frame| self.draw(frame))?;
            if let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                self.on_key(key.code);
            }
        }
        Ok(())
    }

    fn on_key(&mut self, code: KeyCode) {
        // While the quit popup is up it owns the keyboard: only an explicit
        // yes commits, and any other key backs out. That way the same `q`/`Esc`
        // that opened it can't also confirm it by accident.
        if self.confirm_quit {
            match code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => self.quit = true,
                _ => self.confirm_quit = false,
            }
            return;
        }

        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.confirm_quit = true,
            KeyCode::Down | KeyCode::Char('j') => self.state.select_next(),
            KeyCode::Up | KeyCode::Char('k') => self.state.select_previous(),
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => self.enter(),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => self.go_up(),
            KeyCode::Home => self.state.select_first(),
            KeyCode::End => self.state.select_last(),
            _ => {}
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame) {
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        // Header: the path being viewed and its aggregate size.
        let node = &self.tree.nodes[self.current];
        let title = Line::from(vec![
            Span::styled(
                self.tree
                    .path_of(self.current)
                    .to_string_lossy()
                    .into_owned(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                human_bytes(node.total_size as f64),
                Style::default().fg(Color::Cyan),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(title).block(Block::default().borders(Borders::ALL).title(" fsz ")),
            header,
        );

        // Body: one row per child, size-sorted, with a proportional bar.
        let max = self
            .children
            .first()
            .map(|&c| self.tree.nodes[c].total_size)
            .unwrap_or(0);
        let items: Vec<ListItem> = self
            .children
            .iter()
            .map(|&child| self.row(child, max))
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(list, body, &mut self.state);

        // Footer: key hints.
        let hint = if self.children.is_empty() {
            " (empty)   ↑/↓ move · → enter · ← back · q quit "
        } else {
            " ↑/↓ move · →/⏎ enter · ←/⌫ back · q quit "
        };
        frame.render_widget(
            Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
            footer,
        );

        // Quit confirmation, drawn last so it sits on top of everything.
        if self.confirm_quit {
            self.draw_quit_popup(frame);
        }
    }

    /// A small centered "really quit?" dialog over the current view.
    fn draw_quit_popup(&self, frame: &mut ratatui::Frame) {
        let area = centered_rect(frame.area(), 34, 5);
        // Clear wipes the cells behind the popup so the list doesn't show
        // through its interior.
        frame.render_widget(Clear, area);

        let body = Paragraph::new(vec![
            Line::from("Quit fsz?").centered(),
            Line::from(vec![
                Span::styled(
                    "Y",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("es    "),
                Span::styled(
                    "N",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw("o"),
            ])
            .centered(),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Confirm ")
                .border_style(Style::default().fg(Color::Yellow)),
        );
        frame.render_widget(body, area);
    }

    /// Render a single child as `[bar]  size  name`, dirs shown with a trailing
    /// slash and tinted so they read as navigable.
    fn row(&self, child: usize, max: u64) -> ListItem<'static> {
        let node = &self.tree.nodes[child];
        let frac = if max == 0 {
            0.0
        } else {
            node.total_size as f64 / max as f64
        };
        let filled = (frac * BAR_WIDTH as f64).round() as usize;
        let bar: String = "█".repeat(filled) + &"░".repeat(BAR_WIDTH - filled);

        let mut name = node.name.to_string_lossy().into_owned();
        let name_style = if node.is_dir() {
            name.push('/');
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        ListItem::new(Line::from(vec![
            Span::styled(bar, Style::default().fg(Color::Green)),
            Span::raw("  "),
            Span::styled(
                format!("{:>10}", human_bytes(node.total_size as f64)),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw("  "),
            Span::styled(name, name_style),
        ]))
    }
}

/// Carve a `width` × `height` rectangle centered inside `area`, for popups.
fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [cell] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(row);
    cell
}
