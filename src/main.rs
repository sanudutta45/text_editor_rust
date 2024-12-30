use core::str;
use crossterm::event::{poll, read, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, Clear, ClearType};
use crossterm::terminal::{enable_raw_mode, size};
use crossterm::{cursor, execute, queue};
use std::cmp::Ordering;
use std::io::{stdout, ErrorKind, Result, Write};
use std::path::Path;
use std::time::Duration;
use std::{cmp, env, fs};

struct Reader;

impl Reader {
    fn read_key(&self) -> Result<KeyEvent> {
        loop {
            if poll(Duration::from_millis(500))? {
                if let Event::Key(event) = read()? {
                    return Ok(event);
                }
            }
        }
    }
}

struct Editor {
    reader: Reader,
    output: Output,
}

impl Editor {
    fn new() -> Self {
        Self {
            reader: Reader,
            output: Output::new(),
        }
    }

    fn process_keypress(&mut self) -> Result<bool> {
        match self.reader.read_key()? {
            KeyEvent {
                code: KeyCode::Char('q'),
                modifiers: KeyModifiers::CONTROL,
                ..
            } => return Ok(false),
            KeyEvent {
                code:
                    direction @ (KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::Home
                    | KeyCode::End),
                modifiers: KeyModifiers::NONE,
                ..
            } => self.output.move_cursor(direction),
            KeyEvent {
                code: val @ (KeyCode::PageUp | KeyCode::PageDown),
                modifiers: KeyModifiers::NONE,
                ..
            } => {
                if matches!(val, KeyCode::PageUp) {
                    self.output.cursor_controller.cursor_y =
                        self.output.cursor_controller.row_offset
                } else {
                    self.output.cursor_controller.cursor_y = cmp::min(
                        self.output.win_size.1 + self.output.cursor_controller.row_offset - 1,
                        self.output.editor_rows.number_of_rows(),
                    )
                }
                (0..self.output.win_size.1).for_each(|_| {
                    self.output.move_cursor(if matches!(val, KeyCode::PageUp) {
                        KeyCode::Up
                    } else {
                        KeyCode::Down
                    })
                })
            }
            _ => {}
        }
        Ok(true)
    }

    fn run(&mut self) -> Result<bool> {
        self.output.refresh_screen()?;
        self.process_keypress()
    }
}

struct CleanUp;

impl Drop for CleanUp {
    fn drop(&mut self) {
        disable_raw_mode().expect("Unable to disable raw mode");
        Output::clear_screen().expect("Error");
    }
}

struct Row {
    row_content: Box<str>,
    render: String,
}

impl Row {
    fn new(row_content: Box<str>, render: String) -> Self {
        Self {
            row_content,
            render,
        }
    }
}

const TAB_STOP: usize = 8;
struct EditorRows {
    row_contents: Vec<Row>,
}

impl EditorRows {
    fn new() -> Self {
        let mut arg = env::args();

        match arg.nth(1) {
            None => Self {
                row_contents: Vec::new(),
            },
            Some(file) => Self::from_file(file.as_ref()),
        }
    }

    fn from_file(file: &Path) -> Self {
        let file_contents = fs::read_to_string(file).expect("Unable to read file");
        Self {
            row_contents: file_contents
                .lines()
                .map(|it| {
                    let mut row = Row::new(it.into(), String::new());
                    Self::render_row(&mut row);
                    row
                })
                .collect(),
        }
    }

    fn number_of_rows(&self) -> usize {
        self.row_contents.len()
    }

    fn get_render(&self, at: usize) -> &str {
        &self.row_contents[at].render
    }

    fn get_editor_row(&self, at: usize) -> &Row {
        &self.row_contents[at]
    }

    fn render_row(row: &mut Row) {
        let mut index = 0;
        let capacity = row
            .row_content
            .chars()
            .fold(0, |acc, next| acc + if next == '\t' { TAB_STOP } else { 1 });
        row.render = String::with_capacity(capacity);
        row.row_content.chars().for_each(|c| {
            index += 1;
            if c == '\t' {
                row.render.push(' ');
                while index % TAB_STOP != 0 {
                    row.render.push(' ');
                    index += 1
                }
            } else {
                row.render.push(c);
            }
        });
    }
}

struct EditorContents {
    content: String,
}

impl EditorContents {
    fn new() -> Self {
        Self {
            content: String::new(),
        }
    }

    fn push(&mut self, ch: char) {
        self.content.push(ch);
    }

    fn push_str(&mut self, string: &str) {
        self.content.push_str(string);
    }
}

impl Write for EditorContents {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        match str::from_utf8(buf) {
            Ok(s) => {
                self.content.push_str(s);
                Ok(s.len())
            }
            Err(_) => Err(ErrorKind::WriteZero.into()),
        }
    }

    fn flush(&mut self) -> Result<()> {
        let out = write!(stdout(), "{}", self.content);
        stdout().flush()?;
        self.content.clear();
        out
    }
}

struct Output {
    win_size: (usize, usize),
    editor_contents: EditorContents,
    cursor_controller: CursorController,
    editor_rows: EditorRows,
}

struct CursorController {
    cursor_x: usize,
    cursor_y: usize,
    screen_columns: usize,
    screen_rows: usize,
    row_offset: usize,
    column_offset: usize,
    render_x: usize,
}

impl CursorController {
    fn new(win_size: (usize, usize)) -> Self {
        Self {
            cursor_x: 0,
            cursor_y: 0,
            screen_columns: win_size.0,
            screen_rows: win_size.1,
            row_offset: 0,
            column_offset: 0,
            render_x: 0,
        }
    }

    fn move_cursor(&mut self, direction: KeyCode, editor_rows: &EditorRows) {
        let num_of_rows = editor_rows.number_of_rows();

        match direction {
            KeyCode::Up => {
                self.cursor_y = self.cursor_y.saturating_sub(1);
            }
            KeyCode::Left => {
                if self.cursor_x != 0 {
                    self.cursor_x -= 1;
                } else if self.cursor_y > 0 {
                    self.cursor_y -= 1;
                    self.cursor_x = editor_rows.get_render(self.cursor_y).len();
                }
            }
            KeyCode::Down => {
                if self.cursor_y < num_of_rows {
                    self.cursor_y += 1;
                }
            }
            KeyCode::Right => {
                if self.cursor_y < num_of_rows {
                    match self
                        .cursor_x
                        .cmp(&editor_rows.get_render(self.cursor_y).len())
                    {
                        Ordering::Less => self.cursor_x += 1,
                        Ordering::Equal => {
                            self.cursor_y += 1;
                            self.cursor_x = 0;
                        }
                        _ => {}
                    }
                }
            }
            KeyCode::End => {
                if self.cursor_y < num_of_rows {
                    self.cursor_x = editor_rows.get_render(self.cursor_y).len();
                }
            },
            KeyCode::Home => self.cursor_x = 0,
            _ => unimplemented!(),
        }
        let row_len = if self.cursor_y < num_of_rows {
            editor_rows.get_render(self.cursor_y).len()
        } else {
            0
        };
        self.cursor_x = cmp::min(self.cursor_x, row_len)
    }

    fn scroll(&mut self, editor_rows: &EditorRows) {
        self.render_x = 0;
        if self.cursor_y < editor_rows.number_of_rows() {
            self.render_x = self.get_render_x(editor_rows.get_editor_row(self.cursor_y))
        }

        self.row_offset = cmp::min(self.row_offset, self.cursor_y);
        if self.cursor_y >= self.row_offset + self.screen_rows {
            self.row_offset = self.cursor_y - self.screen_rows + 1;
        }

        self.column_offset = cmp::min(self.column_offset, self.cursor_x);
        if self.render_x >= self.column_offset + self.screen_columns {
            self.column_offset = self.render_x - self.screen_columns + 1;
        }
    }

    fn get_render_x(&self, row: &Row) -> usize {
        row.row_content[..self.cursor_x]
            .chars()
            .fold(0, |render_x, c| {
                if c == '\t' {
                    render_x + (TAB_STOP - 1) - (render_x % TAB_STOP) + 1
                } else {
                    render_x + 1
                }
            })
    }
}

impl Output {
    fn new() -> Self {
        let win_size = size().map(|(x, y)| (x as usize, y as usize)).unwrap();

        Self {
            win_size,
            editor_contents: EditorContents::new(),
            cursor_controller: CursorController::new(win_size),
            editor_rows: EditorRows::new(),
        }
    }

    fn clear_screen() -> Result<()> {
        execute!(stdout(), Clear(ClearType::All))?;
        execute!(stdout(), cursor::MoveTo(0, 0))
    }

    fn refresh_screen(&mut self) -> Result<()> {
        self.cursor_controller.scroll(&self.editor_rows);
        queue!(self.editor_contents, cursor::Hide, cursor::MoveTo(0, 0))?;
        self.draw_rows();
        let cursor_x = self.cursor_controller.render_x - self.cursor_controller.column_offset;
        let cursor_y = self.cursor_controller.cursor_y - self.cursor_controller.row_offset;
        queue!(
            self.editor_contents,
            cursor::MoveTo(cursor_x as u16, cursor_y as u16),
            cursor::Show
        )?;
        self.editor_contents.flush()
    }

    fn draw_rows(&mut self) {
        let screen_rows = self.win_size.1;
        let screen_columns = self.win_size.0;
        for i in 0..screen_rows {
            let file_row = i + self.cursor_controller.row_offset;
            if file_row >= self.editor_rows.number_of_rows() {
                if self.editor_rows.number_of_rows() == 0 && i == screen_rows / 3 {
                    let mut welcome = format!("Pound Editor --- Version {}", "3.0.0");
                    if welcome.len() > screen_columns {
                        welcome.truncate(screen_columns);
                    }
                    let mut padding = (screen_columns - welcome.len()) / 2;
                    if padding != 0 {
                        self.editor_contents.push('~');
                        padding -= 1;
                    }
                    (0..padding).for_each(|_| self.editor_contents.push(' '));
                    self.editor_contents.push_str(&welcome);
                } else {
                    self.editor_contents.push('~');
                }
            } else {
                let row = self.editor_rows.get_render(file_row);
                let column_offset = self.cursor_controller.column_offset;

                let len: usize = cmp::min(row.len().saturating_sub(column_offset), screen_columns);
                let start: usize = if len == 0 { 0 } else { column_offset };
                self.editor_contents
                    .push_str(&self.editor_rows.get_render(file_row)[start..start + len]);
            }

            queue!(self.editor_contents, Clear(ClearType::UntilNewLine)).unwrap();
            if i < screen_rows - 1 {
                self.editor_contents.push_str("\r\n");
            }
        }
    }

    fn move_cursor(&mut self, direction: KeyCode) {
        self.cursor_controller
            .move_cursor(direction, &self.editor_rows);
    }
}

fn main() -> Result<()> {
    let _clean_up = CleanUp;
    enable_raw_mode()?;

    let mut editor = Editor::new();

    while editor.run()? {}

    Ok(())
}
