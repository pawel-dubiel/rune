use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::cursor::{MoveTo, Show};
use crossterm::event::{poll, read, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::queue;
use crossterm::style::{Color, Print, SetBackgroundColor, SetForegroundColor};
use crossterm::terminal::{self, Clear, ClearType};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::editor::Editor;

const STATUS_TIMEOUT_MS: u64 = 2000;

pub struct Ui {
    pub screen_cols: u16,
    pub screen_rows: u16, // excluding status row
    pub off_x: usize,
    pub off_y: usize,
    prev_lines: Vec<String>,
    prev_status: String,
    prev_message: String,
}

impl Ui {
    pub fn new() -> io::Result<Self> {
        let (cols, rows) = terminal::size()?;
        Ok(Self {
            screen_cols: cols,
            screen_rows: rows.saturating_sub(1),
            off_x: 0,
            off_y: 0,
            prev_lines: vec![String::new(); rows.saturating_sub(1) as usize],
            prev_status: String::new(),
            prev_message: String::new(),
        })
    }

    pub fn resize(&mut self, w: u16, h: u16) {
        self.screen_cols = w;
        self.screen_rows = h.saturating_sub(1);
        self.prev_lines = vec![String::new(); self.screen_rows as usize];
        self.prev_status.clear();
        self.prev_message.clear();
    }

    fn scroll(&mut self, ed: &Editor) {
        if ed.cy < self.off_y {
            self.off_y = ed.cy;
        }
        if ed.cy >= self.off_y + self.screen_rows as usize {
            self.off_y = ed.cy + 1 - self.screen_rows as usize;
        }
        if ed.cx < self.off_x {
            self.off_x = ed.cx;
        }
        if ed.cx >= self.off_x + self.screen_cols as usize {
            self.off_x = ed.cx + 1 - self.screen_cols as usize;
        }
    }

    fn draw_rows<W: Write>(&mut self, mut w: W, ed: &Editor) -> io::Result<()> {
        for row in 0..self.screen_rows as usize {
            let file_row = self.off_y + row;
            let mut out = String::new();
            if file_row >= ed.buf.rows.len() {
                out.push('~');
            } else {
                let line = &ed.buf.rows[file_row];
                let mut col = 0usize;
                let start_col = self.off_x as usize;
                let end_col = start_col + self.screen_cols as usize;
                for g in line.graphemes(true) {
                    let w = UnicodeWidthStr::width(g).max(1);
                    let next = col + w;
                    if next <= start_col {
                        col = next;
                        continue;
                    }
                    if col >= end_col {
                        break;
                    }
                    out.push_str(g);
                    col = next;
                    if col >= end_col {
                        break;
                    }
                }
            }
            if self.prev_lines[row] != out {
                queue!(
                    w,
                    MoveTo(0, row as u16),
                    Clear(ClearType::CurrentLine),
                    Print(&out)
                )?;
                self.prev_lines[row] = out;
            }
        }
        Ok(())
    }

    fn draw_status_bar<W: Write>(&mut self, mut w: W, ed: &Editor) -> io::Result<()> {
        let status_row = self.screen_rows;
        let fname = ed
            .filename
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("[No Name]");
        let dirty = if ed.dirty { " +" } else { "" };
        let mode = match ed.mode {
            crate::keymap::Mode::Normal => "NORMAL",
            crate::keymap::Mode::Insert => "INSERT",
        };
        let left = format!(
            " {}{} — {} lines [{}] ",
            fname,
            dirty,
            ed.buf.rows.len(),
            mode
        );
        let right = format!(" {}/{} ", ed.cy + 1, ed.buf.rows.len());
        let mut content = left;
        let total = self.screen_cols as usize;
        if content.len() > total {
            content.truncate(total);
        } else {
            let rlen = right.len();
            if total >= rlen + content.len() {
                content.push_str(&" ".repeat(total - rlen - content.len()));
                content.push_str(&right);
            } else {
                content.push_str(&" ".repeat(total - content.len()));
            }
        }
        if self.prev_status != content {
            queue!(
                w,
                MoveTo(0, status_row),
                Clear(ClearType::CurrentLine),
                SetForegroundColor(Color::Black),
                SetBackgroundColor(Color::White),
                Print(&content),
                SetForegroundColor(Color::Reset),
                SetBackgroundColor(Color::Reset)
            )?;
            self.prev_status = content;
        }
        Ok(())
    }

    fn draw_message_bar<W: Write>(&mut self, mut w: W, ed: &Editor) -> io::Result<()> {
        if ed.status.is_empty() {
            return Ok(());
        }
        if ed.status_time.elapsed() > Duration::from_millis(STATUS_TIMEOUT_MS) {
            return Ok(());
        }
        let mut msg = ed.status.clone();
        if msg.len() > self.screen_cols as usize {
            msg.truncate(self.screen_cols as usize);
        }
        if self.prev_message != msg {
            queue!(
                w,
                MoveTo(0, self.screen_rows),
                Clear(ClearType::CurrentLine),
                SetForegroundColor(Color::Yellow),
                Print(&msg),
                SetForegroundColor(Color::Reset)
            )?;
            self.prev_message = msg;
        }
        Ok(())
    }

    pub fn refresh<W: Write>(&mut self, mut w: W, ed: &mut Editor) -> io::Result<()> {
        ed.clamp_cursor();
        self.scroll(ed);
        self.draw_rows(&mut w, ed)?;
        self.draw_status_bar(&mut w, ed)?;
        self.draw_message_bar(&mut w, ed)?;
        let cur_y = (ed.cy - self.off_y) as u16;
        let cur_x = (ed.cx.saturating_sub(self.off_x)) as u16;
        queue!(w, MoveTo(cur_x, cur_y), Show)?;
        w.flush()?;
        Ok(())
    }

    pub fn clear_cache(&mut self) {
        self.prev_lines.fill(String::new());
        self.prev_status.clear();
        self.prev_message.clear();
    }

    pub fn time_until_status_expiry(&self, ed: &Editor) -> Option<Duration> {
        if ed.status.is_empty() {
            return None;
        }
        let timeout = Duration::from_millis(STATUS_TIMEOUT_MS);
        let now = Instant::now();
        let end = ed.status_time + timeout;
        if now >= end {
            Some(Duration::from_millis(0))
        } else {
            Some(end - now)
        }
    }

    pub fn prompt_filename<W: Write>(
        &mut self,
        ed: &mut Editor,
        mut w: W,
        prompt: &str,
    ) -> io::Result<Option<String>> {
        let mut input = String::new();
        loop {
            self.refresh(&mut w, ed)?;
            let mut shown = format!("{}{}", prompt, input);
            if shown.len() > self.screen_cols as usize {
                shown.truncate(self.screen_cols as usize);
            }
            queue!(
                w,
                MoveTo(0, self.screen_rows),
                Clear(ClearType::CurrentLine),
                Print(shown.clone())
            )?;
            w.flush()?;
            if poll(Duration::from_millis(250))? {
                match read()? {
                    Event::Key(KeyEvent {
                        code, modifiers, ..
                    }) => match (code, modifiers) {
                        (KeyCode::Esc, _) => return Ok(None),
                        (KeyCode::Enter, _) => {
                            if !input.is_empty() {
                                return Ok(Some(input));
                            }
                        }
                        (KeyCode::Backspace, _) | (KeyCode::Delete, _) => {
                            input.pop();
                        }
                        (KeyCode::Char(c), m) => {
                            if !m.contains(KeyModifiers::CONTROL) && !c.is_control() {
                                input.push(c);
                            }
                        }
                        _ => {}
                    },
                    Event::Resize(wid, hgt) => {
                        self.resize(wid, hgt);
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn prompt_command<W: Write>(
        &mut self,
        ed: &mut Editor,
        mut w: W,
    ) -> io::Result<Option<String>> {
        let mut input = String::new();
        loop {
            self.refresh(&mut w, ed)?;
            let mut display = format!(":{}", input);
            if display.len() > self.screen_cols as usize {
                display.truncate(self.screen_cols as usize);
            }
            queue!(
                w,
                MoveTo(0, self.screen_rows),
                Clear(ClearType::CurrentLine),
                Print(display)
            )?;
            w.flush()?;
            if poll(Duration::from_millis(250))? {
                match read()? {
                    Event::Key(KeyEvent {
                        code, modifiers, ..
                    }) => match (code, modifiers) {
                        (KeyCode::Esc, _) => return Ok(None),
                        (KeyCode::Enter, _) => return Ok(Some(input)),
                        (KeyCode::Backspace, _) | (KeyCode::Delete, _) => {
                            input.pop();
                        }
                        (KeyCode::Char(c), m) => {
                            if !m.contains(KeyModifiers::CONTROL) && !c.is_control() {
                                input.push(c);
                            }
                        }
                        _ => {}
                    },
                    Event::Resize(wid, hgt) => {
                        self.resize(wid, hgt);
                    }
                    _ => {}
                }
            }
        }
    }
}
