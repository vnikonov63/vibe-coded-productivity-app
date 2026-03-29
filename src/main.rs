use std::{
    io,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

fn parse_args() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "-f" {
            if let Some(path) = args.get(i + 1) {
                return PathBuf::from(path);
            } else {
                eprintln!("error: -f requires a file path");
                std::process::exit(1);
            }
        }
        i += 1;
    }
    // Default: visible file next to the binary, not a hidden dot-file
    PathBuf::from("timer/main_timer.json")
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    match (hours, minutes) {
        (0, 0) => format!("{} seconds", secs),
        (0, m) => format!("{} minutes, {} seconds", m, secs),
        (h, m) => format!("{} hours, {} minutes", h, m),
    }
}

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// A child timer — no further nesting.
#[derive(Serialize, Deserialize, Clone)]
struct SubTimer {
    name: String,
    total_seconds: u64,
    running_since: Option<u64>,
}

impl SubTimer {
    fn new(name: String) -> Self {
        Self { name, total_seconds: 0, running_since: None }
    }
    fn elapsed(&self) -> u64 {
        if let Some(since) = self.running_since {
            self.total_seconds + now_secs().saturating_sub(since)
        } else {
            self.total_seconds
        }
    }
    fn is_running(&self) -> bool { self.running_since.is_some() }
    fn toggle(&mut self) {
        if let Some(since) = self.running_since {
            self.total_seconds += now_secs().saturating_sub(since);
            self.running_since = None;
        } else {
            self.running_since = Some(now_secs());
        }
    }
}

/// A top-level timer that can optionally contain child timers.
/// Its displayed time = own elapsed + sum of all children's elapsed.
#[derive(Serialize, Deserialize, Clone)]
struct Timer {
    name: String,
    total_seconds: u64,
    running_since: Option<u64>,
    #[serde(default)]
    children: Vec<SubTimer>,
}

impl Timer {
    fn new(name: String) -> Self {
        Self { name, total_seconds: 0, running_since: None, children: vec![] }
    }
    fn own_elapsed(&self) -> u64 {
        if let Some(since) = self.running_since {
            self.total_seconds + now_secs().saturating_sub(since)
        } else {
            self.total_seconds
        }
    }
    /// Total = own time + all children's time.
    fn total_elapsed(&self) -> u64 {
        self.own_elapsed() + self.children.iter().map(|c| c.elapsed()).sum::<u64>()
    }
    fn any_running(&self) -> bool {
        self.running_since.is_some() || self.children.iter().any(|c| c.is_running())
    }
    fn toggle_own(&mut self) {
        if let Some(since) = self.running_since {
            self.total_seconds += now_secs().saturating_sub(since);
            self.running_since = None;
        } else {
            self.running_since = Some(now_secs());
        }
    }
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

#[derive(PartialEq)]
enum Mode {
    Normal,
    Insert,
    DeletePending(u8),
    ConfirmDelete,
}

/// Which level the user is currently viewing.
#[derive(Clone, Copy)]
enum View {
    Top,
    Children(usize), // index of the parent Timer
}

struct App {
    timers: Vec<Timer>,
    top_state: ListState,
    child_state: ListState,
    mode: Mode,
    view: View,
    input: String,
    state_path: PathBuf,
}

impl App {
    fn load(path: PathBuf) -> Self {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let timers: Vec<Timer> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default();

        let mut top_state = ListState::default();
        if !timers.is_empty() {
            top_state.select(Some(0));
        }

        Self {
            timers,
            top_state,
            child_state: ListState::default(),
            mode: Mode::Normal,
            view: View::Top,
            input: String::new(),
            state_path: path,
        }
    }

    fn save(&self) {
        if let Ok(data) = serde_json::to_string_pretty(&self.timers) {
            let _ = std::fs::write(&self.state_path, data);
        }
    }

    fn list_len(&self) -> usize {
        match self.view {
            View::Top => self.timers.len(),
            View::Children(pi) => self.timers.get(pi).map_or(0, |t| t.children.len()),
        }
    }

    fn next(&mut self) {
        let len = self.list_len();
        if len == 0 { return; }
        match self.view {
            View::Top => {
                let i = self.top_state.selected().map_or(0, |i| (i + 1) % len);
                self.top_state.select(Some(i));
            }
            View::Children(_) => {
                let i = self.child_state.selected().map_or(0, |i| (i + 1) % len);
                self.child_state.select(Some(i));
            }
        }
    }

    fn prev(&mut self) {
        let len = self.list_len();
        if len == 0 { return; }
        match self.view {
            View::Top => {
                let i = self.top_state.selected().map_or(0, |i| if i == 0 { len - 1 } else { i - 1 });
                self.top_state.select(Some(i));
            }
            View::Children(_) => {
                let i = self.child_state.selected().map_or(0, |i| if i == 0 { len - 1 } else { i - 1 });
                self.child_state.select(Some(i));
            }
        }
    }

    fn toggle_selected(&mut self) {
        match self.view {
            View::Top => {
                if let Some(i) = self.top_state.selected() {
                    if i < self.timers.len() {
                        self.timers[i].toggle_own();
                        self.save();
                    }
                }
            }
            View::Children(pi) => {
                if let Some(ci) = self.child_state.selected() {
                    if let Some(parent) = self.timers.get_mut(pi) {
                        if ci < parent.children.len() {
                            parent.children[ci].toggle();
                            self.save();
                        }
                    }
                }
            }
        }
    }

    fn add_item(&mut self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() { return; }
        match self.view {
            View::Top => {
                self.timers.push(Timer::new(name));
                self.top_state.select(Some(self.timers.len() - 1));
            }
            View::Children(pi) => {
                if let Some(parent) = self.timers.get_mut(pi) {
                    parent.children.push(SubTimer::new(name));
                    let len = parent.children.len();
                    self.child_state.select(Some(len - 1));
                }
            }
        }
        self.save();
    }

    fn delete_selected(&mut self) {
        match self.view {
            View::Top => {
                if let Some(i) = self.top_state.selected() {
                    if i < self.timers.len() {
                        self.timers.remove(i);
                        let sel = if self.timers.is_empty() { None } else { Some(i.min(self.timers.len() - 1)) };
                        self.top_state.select(sel);
                        self.save();
                    }
                }
            }
            View::Children(pi) => {
                if let Some(ci) = self.child_state.selected() {
                    if let Some(parent) = self.timers.get_mut(pi) {
                        if ci < parent.children.len() {
                            parent.children.remove(ci);
                            let sel = if parent.children.is_empty() { None } else { Some(ci.min(parent.children.len() - 1)) };
                            self.child_state.select(sel);
                            self.save();
                        }
                    }
                }
            }
        }
    }

    fn enter_children(&mut self) {
        if let View::Top = self.view {
            if let Some(i) = self.top_state.selected() {
                if i < self.timers.len() {
                    self.view = View::Children(i);
                    self.child_state = ListState::default();
                    if !self.timers[i].children.is_empty() {
                        self.child_state.select(Some(0));
                    }
                }
            }
        }
    }

    fn exit_children(&mut self) {
        self.view = View::Top;
        self.mode = Mode::Normal;
    }

    fn selected_name(&self) -> String {
        match self.view {
            View::Top => self.top_state.selected()
                .and_then(|i| self.timers.get(i))
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "this timer".into()),
            View::Children(pi) => self.child_state.selected()
                .and_then(|ci| self.timers.get(pi)?.children.get(ci))
                .map(|c| c.name.clone())
                .unwrap_or_else(|| "this timer".into()),
        }
    }

    fn parent_name(&self) -> &str {
        if let View::Children(pi) = self.view {
            self.timers.get(pi).map(|t| t.name.as_str()).unwrap_or("?")
        } else {
            ""
        }
    }
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn main() -> io::Result<()> {
    let path = parse_args();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::load(path);

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }

                match &app.mode {
                    Mode::Normal => match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Esc => {
                            if matches!(app.view, View::Children(_)) {
                                app.exit_children();
                            } else {
                                break;
                            }
                        }
                        KeyCode::Char('i') => { app.mode = Mode::Insert; app.input.clear(); }
                        KeyCode::Enter => app.toggle_selected(),
                        KeyCode::Char('l') | KeyCode::Right => app.enter_children(),
                        KeyCode::Char('h') | KeyCode::Left => {
                            if matches!(app.view, View::Children(_)) { app.exit_children(); }
                        }
                        KeyCode::Char('j') | KeyCode::Down => app.next(),
                        KeyCode::Char('k') | KeyCode::Up   => app.prev(),
                        KeyCode::Char('d') => {
                            if app.list_len() > 0 { app.mode = Mode::DeletePending(1); }
                        }
                        _ => {}
                    },

                    Mode::DeletePending(count) => {
                        let count = *count;
                        match key.code {
                            KeyCode::Char('d') => {
                                app.mode = if count >= 2 { Mode::ConfirmDelete } else { Mode::DeletePending(count + 1) };
                            }
                            _ => app.mode = Mode::Normal,
                        }
                    }

                    Mode::ConfirmDelete => match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            app.delete_selected();
                            app.mode = Mode::Normal;
                        }
                        _ => app.mode = Mode::Normal,
                    },

                    Mode::Insert => match key.code {
                        KeyCode::Esc => { app.mode = Mode::Normal; app.input.clear(); }
                        KeyCode::Enter => {
                            let name = app.input.clone();
                            app.add_item(name);
                            app.input.clear();
                            app.mode = Mode::Normal;
                        }
                        KeyCode::Backspace => { app.input.pop(); }
                        KeyCode::Char(c)   => app.input.push(c),
                        _ => {}
                    },
                }
            }
        }
    }

    app.save();
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------

fn ui(f: &mut ratatui::Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3), Constraint::Length(1)])
        .split(f.area());

    // ── List ────────────────────────────────────────────────────────────────
    match app.view {
        View::Top => {
            let items: Vec<ListItem> = app.timers.iter().map(|t| {
                let total = t.total_elapsed();
                let running = t.any_running();
                let indicator  = if running { "▶ " } else { "  " };
                let time_color = if running { Color::Green } else { Color::DarkGray };
                let arrow = if !t.children.is_empty() { Span::styled(" ›", Style::default().fg(Color::Blue)) }
                            else { Span::raw("") };
                ListItem::new(Line::from(vec![
                    Span::raw(indicator),
                    Span::styled(t.name.clone(), Style::default().add_modifier(Modifier::BOLD)),
                    arrow,
                    Span::raw("  —  "),
                    Span::styled(format_duration(total), Style::default().fg(time_color)),
                ]))
            }).collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(" Timers "))
                .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
                .highlight_symbol("► ");
            f.render_stateful_widget(list, chunks[0], &mut app.top_state);
        }

        View::Children(pi) => {
            let parent_name  = app.timers.get(pi).map(|t| t.name.clone()).unwrap_or_default();
            let parent_total = app.timers.get(pi).map(|t| t.total_elapsed()).unwrap_or(0);
            let title = format!(" {} ‹children›  total: {} ", parent_name, format_duration(parent_total));

            let items: Vec<ListItem> = app.timers.get(pi)
                .map(|parent| parent.children.iter().map(|c| {
                    let running    = c.is_running();
                    let indicator  = if running { "▶ " } else { "  " };
                    let time_color = if running { Color::Green } else { Color::DarkGray };
                    ListItem::new(Line::from(vec![
                        Span::raw(indicator),
                        Span::styled(c.name.clone(), Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw("  —  "),
                        Span::styled(format_duration(c.elapsed()), Style::default().fg(time_color)),
                    ]))
                }).collect::<Vec<_>>())
                .unwrap_or_default();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(title))
                .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
                .highlight_symbol("► ");
            f.render_stateful_widget(list, chunks[0], &mut app.child_state);
        }
    }

    // ── Middle box ──────────────────────────────────────────────────────────
    let is_children_view = matches!(app.view, View::Children(_));
    match &app.mode {
        Mode::Insert => {
            let title = if is_children_view {
                format!(" New sub-timer inside \"{}\" (Enter confirm, Esc cancel) ", app.parent_name())
            } else {
                " New timer (Enter confirm, Esc cancel) ".to_string()
            };
            let widget = Paragraph::new(app.input.as_str())
                .block(Block::default().borders(Borders::ALL).title(title))
                .style(Style::default().fg(Color::Yellow));
            f.render_widget(widget, chunks[1]);
            f.set_cursor_position((chunks[1].x + 1 + app.input.len() as u16, chunks[1].y + 1));
        }
        Mode::ConfirmDelete => {
            let name = app.selected_name();
            let msg = format!("Delete \"{}\" ?  Press y to confirm, any other key to cancel.", name);
            let widget = Paragraph::new(msg.as_str())
                .block(Block::default().borders(Borders::ALL).title(" Confirm Delete "))
                .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD));
            f.render_widget(widget, chunks[1]);
        }
        Mode::DeletePending(n) => {
            let n = *n;
            let msg = format!("\"{}\" — press d {} more time(s) to reach confirmation.", "d".repeat(n as usize), 3 - n);
            let widget = Paragraph::new(msg.as_str())
                .block(Block::default().borders(Borders::ALL).title(" Delete "))
                .style(Style::default().fg(Color::Yellow));
            f.render_widget(widget, chunks[1]);
        }
        _ => {
            let widget = Paragraph::new("")
                .block(Block::default().borders(Borders::ALL).title(if is_children_view { " New Sub-Timer " } else { " New Timer " }))
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(widget, chunks[1]);
        }
    }

    // ── Status bar ───────────────────────────────────────────────────────────
    let status = match (&app.mode, app.view) {
        (Mode::Normal, View::Top) =>
            " NORMAL  [i] new  [Enter] start/stop  [l/→] open children  [j/k ↑↓] navigate  [ddd] delete  [q] quit",
        (Mode::Normal, View::Children(_)) =>
            " CHILDREN  [i] new sub-timer  [Enter] start/stop  [h/←/Esc] back  [j/k ↑↓] navigate  [ddd] delete",
        (Mode::Insert, _) =>
            " INSERT  type a name  [Enter] confirm  [Esc] cancel",
        (Mode::DeletePending(_), _) =>
            " DELETE  keep pressing d (3× total)  any other key cancels",
        (Mode::ConfirmDelete, _) =>
            " CONFIRM DELETE  [y] delete  [any other key] cancel",
    };
    f.render_widget(
        Paragraph::new(status).style(Style::default().fg(Color::Cyan)),
        chunks[2],
    );
}
