use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event as CEvent, KeyCode, KeyEvent},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use rand::prelude::*;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::{
    cmp::max,
    io,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// Board dimensions (classic Tetris is 10x20)
const BOARD_WIDTH: usize = 10;
const BOARD_HEIGHT: usize = 20;

/// Represent each block cell as Option<BlockType>
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum BlockType {
    I,
    O,
    T,
    S,
    Z,
    J,
    L,
}

impl BlockType {
    fn all() -> &'static [BlockType] {
        &[
            BlockType::I,
            BlockType::O,
            BlockType::T,
            BlockType::S,
            BlockType::Z,
            BlockType::J,
            BlockType::L,
        ]
    }

    fn color(self) -> Color {
        match self {
            BlockType::I => Color::Cyan,
            BlockType::O => Color::Yellow,
            BlockType::T => Color::Magenta,
            BlockType::S => Color::Green,
            BlockType::Z => Color::Red,
            BlockType::J => Color::Blue,
            BlockType::L => Color::Rgb(255, 165, 0), // orange
        }
    }
}

/// A Tetromino has rotations represented as 4x4 bool grids (flattened).
#[derive(Clone)]
struct Tetromino {
    kind: BlockType,
    rotations: Vec<[u8; 16]>, // each rotation is 4x4 grid, row-major; 1 = block, 0 = empty
}

impl Tetromino {
    fn new(kind: BlockType) -> Self {
        let rotations = match kind {
            BlockType::I => vec![
                // ----  4x4
                // ....  rotated forms
                // ####
                // ....
                // ....
                // ....
                [0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0],
                [0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0],
            ],
            BlockType::O => vec![[0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]],
            BlockType::T => vec![
                [0, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                [0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0],
                [0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0],
                [0, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0],
            ],
            BlockType::S => vec![
                [0, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                [0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0],
            ],
            BlockType::Z => vec![
                [1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                [0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0],
            ],
            BlockType::J => vec![
                [1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                [0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0],
                [0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0],
                [0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0],
            ],
            BlockType::L => vec![
                [0, 0, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                [0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0],
                [0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0],
                [1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0],
            ],
        };

        Tetromino { kind, rotations }
    }
}

/// Active piece in play with position and rotation index
#[derive(Clone)]
struct ActivePiece {
    tetro: Tetromino,
    rotation: usize,
    x: i32, // position on board (x,y refer to top-left of 4x4)
    y: i32,
}

impl ActivePiece {
    fn new(kind: BlockType) -> Self {
        let tetro = Tetromino::new(kind);
        // spawn near top center
        ActivePiece {
            tetro,
            rotation: 0,
            x: (BOARD_WIDTH as i32 / 2) - 2,
            y: -1, // allow spawn partially above the visible board
        }
    }

    fn cells(&self) -> Vec<(i32, i32)> {
        let grid = &self.tetro.rotations[self.rotation % self.tetro.rotations.len()];
        let mut out = Vec::new();
        for by in 0..4 {
            for bx in 0..4 {
                if grid[(by * 4 + bx) as usize] != 0 {
                    out.push((self.x + bx as i32, self.y + by as i32));
                }
            }
        }
        out
    }

    fn rotate_cw(&mut self) {
        self.rotation = (self.rotation + 1) % self.tetro.rotations.len();
    }

    fn rotate_ccw(&mut self) {
        if self.rotation == 0 {
            self.rotation = self.tetro.rotations.len() - 1;
        } else {
            self.rotation -= 1;
        }
    }
}

/// Game state
struct Game {
    board: [[Option<BlockType>; BOARD_WIDTH]; BOARD_HEIGHT],
    rng: ThreadRng,
    current: ActivePiece,
    next: BlockType,
    score: usize,
    level: usize,
    lines_cleared: usize,
    start_time: Instant,
    paused: bool,
    game_over: bool,
    last_drop_instant: Instant,
    gravity_interval: Duration,
}

impl Game {
    fn new() -> Self {
        let mut rng = thread_rng();
        let next = *BlockType::all().choose(&mut rng).unwrap();
        let current_kind = *BlockType::all().choose(&mut rng).unwrap();
        let gravity_interval = Game::interval_for_level(1);
        Game {
            board: [[None; BOARD_WIDTH]; BOARD_HEIGHT],
            rng,
            current: ActivePiece::new(current_kind),
            next,
            score: 0,
            level: 1,
            lines_cleared: 0,
            start_time: Instant::now(),
            paused: false,
            game_over: false,
            last_drop_instant: Instant::now(),
            gravity_interval,
        }
    }

    fn interval_for_level(level: usize) -> Duration {
        // simple formula: base 700ms, reduce by level (cap at 50ms)
        let base_ms = 700i32;
        let ms = base_ms - ((level as i32 - 1) * 50);
        let ms = max(ms, 60);
        Duration::from_millis(ms as u64)
    }

    fn spawn_next(&mut self) {
        self.current = ActivePiece::new(self.next);
        self.next = *BlockType::all().choose(&mut self.rng).unwrap();
        // if spawn collides immediately -> game over
        if self.check_collision(&self.current, 0, 0) {
            self.game_over = true;
        }
    }

    fn check_collision(&self, piece: &ActivePiece, dx: i32, dy: i32) -> bool {
        for (x, y) in piece.cells() {
            let nx = x + dx;
            let ny = y + dy;
            if nx < 0 || nx >= BOARD_WIDTH as i32 {
                return true;
            }
            if ny >= BOARD_HEIGHT as i32 {
                return true;
            }
            if ny >= 0 {
                if let Some(_) = self.board[ny as usize][nx as usize] {
                    return true;
                }
            }
        }
        false
    }

    fn lock_piece(&mut self) {
        let kind = self.current.tetro.kind;
        for (x, y) in self.current.cells() {
            if y >= 0 && y < BOARD_HEIGHT as i32 && x >= 0 && x < BOARD_WIDTH as i32 {
                self.board[y as usize][x as usize] = Some(kind);
            }
        }
        self.clear_full_lines();
        self.spawn_next();
        self.last_drop_instant = Instant::now();
    }

    fn hard_drop(&mut self) {
        while !self.check_collision(&self.current, 0, 1) {
            self.current.y += 1;
        }
        self.lock_piece();
    }

    fn step(&mut self) {
        if self.paused || self.game_over {
            return;
        }
        if self.last_drop_instant.elapsed() >= self.gravity_interval {
            if !self.check_collision(&self.current, 0, 1) {
                self.current.y += 1;
            } else {
                // unlock to board
                self.lock_piece();
            }
            self.last_drop_instant = Instant::now();
        }
    }

    fn move_left(&mut self) {
        if !self.check_collision(&self.current, -1, 0) {
            self.current.x -= 1;
        }
    }

    fn move_right(&mut self) {
        if !self.check_collision(&self.current, 1, 0) {
            self.current.x += 1;
        }
    }

    fn move_down(&mut self) {
        if !self.check_collision(&self.current, 0, 1) {
            self.current.y += 1;
            // small score for soft drop
            self.score += 1;
        } else {
            // lock if can't move down
            self.lock_piece();
        }
    }

    fn rotate_cw(&mut self) {
        let mut test = self.current.clone();
        test.rotate_cw();
        // simple wall-kick: try no offset, left, right, up
        let kicks = [(0, 0), (-1, 0), (1, 0), (0, -1)];
        for (dx, dy) in &kicks {
            if !self.check_collision(&test, *dx, *dy) {
                self.current = test;
                self.current.x += dx;
                self.current.y += dy;
                break;
            }
        }
    }

    fn rotate_ccw(&mut self) {
        let mut test = self.current.clone();
        test.rotate_ccw();
        let kicks = [(0, 0), (-1, 0), (1, 0), (0, -1)];
        for (dx, dy) in &kicks {
            if !self.check_collision(&test, *dx, *dy) {
                self.current = test;
                self.current.x += dx;
                self.current.y += dy;
                break;
            }
        }
    }

    fn clear_full_lines(&mut self) {
        let mut new_board = [[None; BOARD_WIDTH]; BOARD_HEIGHT];
        let mut new_row = BOARD_HEIGHT as i32 - 1;
        let mut removed = 0usize;

        for y in (0..BOARD_HEIGHT).rev() {
            let mut full = true;
            for x in 0..BOARD_WIDTH {
                if self.board[y][x].is_none() {
                    full = false;
                    break;
                }
            }
            if !full {
                // copy this row to new_row
                for x in 0..BOARD_WIDTH {
                    new_board[new_row as usize][x] = self.board[y][x];
                }
                new_row -= 1;
            } else {
                removed += 1;
            }
        }

        if removed > 0 {
            // scoring: classic-ish: 1->100, 2->300, 3->500, 4->800 times level
            let points = match removed {
                1 => 100,
                2 => 300,
                3 => 500,
                _ => 800,
            } * self.level;
            self.score += points as usize;
            self.lines_cleared += removed;
            // level up every 10 lines
            let new_level = (self.lines_cleared / 10) + 1;
            if new_level != self.level {
                self.level = new_level;
                self.gravity_interval = Game::interval_for_level(self.level);
            }
            // replace board
            self.board = new_board;
        }
    }

    fn reset(&mut self) {
        *self = Game::new();
    }

    fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}

enum InternalEvent {
    Input(KeyEvent),
    Tick,
}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let minutes = secs / 60;
    let seconds = secs % 60;
    format!("{:02}:{:02}", minutes, seconds)
}

fn main() -> Result<(), io::Error> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Channel for input + ticks
    let (tx, rx) = mpsc::channel();
    // input thread
    let tx2 = tx.clone();
    thread::spawn(move || {
        loop {
            if event::poll(Duration::from_millis(50)).unwrap() {
                if let CEvent::Key(k) = event::read().unwrap() {
                    tx2.send(InternalEvent::Input(k)).unwrap();
                }
            }
            // small sleep to avoid busy loop
            thread::sleep(Duration::from_millis(10));
        }
    });

    // tick thread
    let tx3 = tx.clone();
    thread::spawn(move || {
        loop {
            tx3.send(InternalEvent::Tick).unwrap();
            thread::sleep(Duration::from_millis(20));
        }
    });

    // Create game
    let mut game = Game::new();

    // Game loop
    let mut last_frame = Instant::now();
    loop {
        // draw UI
        terminal.draw(|f| ui(f, &game)).unwrap();

        // handle events (non-blocking)
        let mut did_quit = false;
        // drain events available now
        while let Ok(ev) = rx.try_recv() {
            match ev {
                InternalEvent::Input(key) => {
                    match key.code {
                        KeyCode::Char('q') => {
                            did_quit = true;
                        }
                        KeyCode::Char('p') => {
                            game.paused = !game.paused;
                        }
                        KeyCode::Char('r') => {
                            if game.game_over {
                                game.reset();
                            } else {
                                // allow restart mid-game
                                game.reset();
                            }
                        }
                        KeyCode::Left => {
                            if !game.paused && !game.game_over {
                                game.move_left();
                            }
                        }
                        KeyCode::Right => {
                            if !game.paused && !game.game_over {
                                game.move_right();
                            }
                        }
                        KeyCode::Down => {
                            if !game.paused && !game.game_over {
                                game.move_down();
                                game.last_drop_instant = Instant::now(); // reset gravity timer after manual down
                            }
                        }
                        KeyCode::Up => {
                            if !game.paused && !game.game_over {
                                game.rotate_cw();
                            }
                        }
                        KeyCode::Char('z') => {
                            if !game.paused && !game.game_over {
                                game.rotate_ccw();
                            }
                        }
                        KeyCode::Char(' ') => {
                            if !game.paused && !game.game_over {
                                game.hard_drop();
                            }
                        }
                        _ => {}
                    }
                }
                InternalEvent::Tick => {
                    // update game step based on elapsed since last frame
                    game.step();
                }
            }
        }

        if did_quit {
            // cleanup and quit
            disable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture
            )?;
            terminal.show_cursor()?;
            break;
        }

        // small sleep to limit CPU (already mostly blocked by draw & events)
        let frame_time = Instant::now() - last_frame;
        if frame_time < Duration::from_millis(16) {
            thread::sleep(Duration::from_millis(16) - frame_time);
        }
        last_frame = Instant::now();
    }

    Ok(())
}

/// UI rendering function using ratatui widgets
fn ui<B: ratatui::backend::Backend>(f: &mut ratatui::Frame<B>, game: &Game) {
    let size = f.size();

    // Outer layout: main game area on left, sidebar on right
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)].as_ref())
        .split(size);

    // Left side: board with border
    // let board_area = centered_rect(60, 90, chunks[0]);
    let board_width_chars = (BOARD_WIDTH * 2) as u16;
    let board_height_chars = BOARD_HEIGHT as u16;
    let area = chunks[0];

    let offset_x = (area.width.saturating_sub(board_width_chars + 2)) / 2; // +2 for borders
    let offset_y = (area.height.saturating_sub(board_height_chars + 2)) / 2;

    let board_area = Rect {
        x: area.x + offset_x,
        y: area.y + offset_y,
        width: board_width_chars + 2,
        height: board_height_chars + 2,
    };

    let board_block = Block::default()
        .borders(Borders::ALL)
        .title(" Tetris ")
        .border_style(Style::default().fg(Color::White));
    f.render_widget(board_block, board_area);

    // compute inner area for drawing cells (1 char cell wide; we'll use two spaces "  " per cell)
    let inner = Rect {
        x: board_area.x + 1,
        y: board_area.y + 1,
        width: board_area.width.saturating_sub(2),
        height: board_area.height.saturating_sub(2),
    };

    // Build rows of text for board
    let mut rows: Vec<Line> = vec![];
    for y in 0..BOARD_HEIGHT {
        let mut spans: Vec<Span> = Vec::new();
        for x in 0..BOARD_WIDTH {
            let mut cell_color: Option<Color> = None;

            // check if current piece occupies this cell
            for (cx, cy) in game.current.cells() {
                if cx == x as i32 && cy == y as i32 {
                    cell_color = Some(game.current.tetro.kind.color());
                    break;
                }
            }
            // otherwise board content
            if cell_color.is_none() {
                if let Some(kind) = game.board[y][x] {
                    cell_color = Some(kind.color());
                }
            }

            if let Some(col) = cell_color {
                spans.push(Span::styled("██", Style::default().fg(col)));
            } else {
                spans.push(Span::styled("  ", Style::default().bg(Color::Black)));
            }
        }
        rows.push(Line::from(spans));
    }

    // render board text area
    let board_paragraph = Paragraph::new(rows)
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false })
        .block(Block::default());
    f.render_widget(board_paragraph, inner);

    // Right sidebar
    let side_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(7),
                Constraint::Length(5),
                Constraint::Length(5),
                Constraint::Min(3),
            ]
            .as_ref(),
        )
        .split(chunks[1]);

    // Next piece preview
    let next_block = Block::default().borders(Borders::ALL).title(" Next ");
    let mut next_rows: Vec<Line> = Vec::new();
    let next_tetro = Tetromino::new(game.next);
    let grid = &next_tetro.rotations[0];
    for by in 0..4 {
        let mut spans: Vec<Span> = Vec::new();
        for bx in 0..4 {
            if grid[(by * 4 + bx) as usize] != 0 {
                spans.push(Span::styled("  ", Style::default().bg(game.next.color())));
            } else {
                spans.push(Span::styled("  ", Style::default().bg(Color::Black)));
            }
        }
        next_rows.push(Line::from(spans));
    }
    let next_para = Paragraph::new(next_rows).block(next_block);
    f.render_widget(next_para, side_chunks[0]);

    // Score box
    let score_block = Block::default().borders(Borders::ALL).title(" Stats ");
    let score_text = vec![
        Line::from(vec![Span::raw(format!("Score: {}", game.score))]),
        Line::from(vec![Span::raw(format!("Level: {}", game.level))]),
        Line::from(vec![Span::raw(format!("Lines: {}", game.lines_cleared))]),
    ];
    let score_para = Paragraph::new(score_text).block(score_block);
    f.render_widget(score_para, side_chunks[1]);

    // Status / Controls
    let status_block = Block::default().borders(Borders::ALL).title(" Controls ");
    let status_text = vec![
        Line::from(vec![Span::raw("← → : Move     ↓ : Soft drop")]),
        Line::from(vec![Span::raw("↑ : Rotate CW  Z : Rotate CCW")]),
        Line::from(vec![Span::raw("Space : Hard drop")]),
        Line::from(vec![Span::raw("P : Pause   R : Restart   Q : Quit")]),
    ];
    let status_para = Paragraph::new(status_text).block(status_block);
    f.render_widget(status_para, side_chunks[2]);

    // Bottom area: runtime, level bar, pause/gameover message
    let bottom = Block::default().borders(Borders::ALL).title(" Status ");
    let mut bottom_text: Vec<Line> = vec![];
    let elapsed = format_duration(game.elapsed());
    bottom_text.push(Line::from(vec![Span::raw(format!("Time: {}", elapsed))]));
    bottom_text.push(Line::from(vec![Span::raw(format!(
        "Gravity: {:?}ms",
        game.gravity_interval.as_millis()
    ))]));
    if game.paused {
        bottom_text.push(Line::from(vec![Span::styled(
            " PAUSED ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));
    }
    if game.game_over {
        bottom_text.push(Line::from(vec![Span::styled(
            format!(" GAME OVER — Final score: {} ", game.score),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )]));
        bottom_text.push(Line::from(vec![Span::styled(
            " Press 'R' to restart or 'Q' to quit ",
            Style::default().fg(Color::White),
        )]));
    }

    let bottom_para = Paragraph::new(bottom_text).block(bottom);
    f.render_widget(bottom_para, side_chunks[3]);
}
