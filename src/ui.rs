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
    fn truncate_to_width(s: &str, max_w: usize) -> String {
        if max_w == 0 {
            return String::new();
        }
        let mut out = String::new();
        let mut acc = 0usize;
        for g in s.graphemes(true) {
            let w = UnicodeWidthStr::width(g).max(1);
            if acc + w > max_w {
                break;
            }
            out.push_str(g);
            acc += w;
        }
        out
    }
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
        // Determine selection bounds if in Visual mode
        enum Sel {
            None,
            Char {
                sy: usize,
                sx: usize,
                ey: usize,
                ex: usize,
            },
            Line {
                sy: usize,
                ey: usize,
            },
            Block {
                sy: usize,
                ey: usize,
                left: usize,
                right: usize,
            },
        }
        let selection = match ed.mode {
            crate::keymap::Mode::Visual => {
                if let Some((ax, ay)) = ed.visual_anchor {
                    let (cx, cy) = (ed.cx, ed.cy);
                    let (sy, sx, ey, ex) = if (cy > ay) || (cy == ay && cx >= ax) {
                        (ay, ax, cy, cx)
                    } else {
                        (cy, cx, ay, ax)
                    };
                    Sel::Char { sy, sx, ey, ex }
                } else {
                    Sel::None
                }
            }
            crate::keymap::Mode::VisualLine => {
                if let Some((_, ay)) = ed.visual_anchor {
                    let cy = ed.cy;
                    let (sy, ey) = if cy >= ay { (ay, cy) } else { (cy, ay) };
                    Sel::Line { sy, ey }
                } else {
                    Sel::None
                }
            }
            crate::keymap::Mode::VisualBlock => {
                if let Some((ax, ay)) = ed.visual_anchor {
                    let (cx, cy) = (ed.cx, ed.cy);
                    let (sy, ey) = if cy >= ay { (ay, cy) } else { (cy, ay) };
                    let (left, right) = if cx >= ax { (ax, cx) } else { (cx, ax) };
                    Sel::Block {
                        sy,
                        ey,
                        left,
                        right,
                    }
                } else {
                    Sel::None
                }
            }
            _ => Sel::None,
        };

        for row in 0..self.screen_rows as usize {
            let file_row = self.off_y + row;
            // When highlighting selection, skip string-diff cache to ensure correct rendering
            let use_cache = matches!(selection, Sel::None);
            if file_row >= ed.buf.line_count() {
                // Tilde rows
                if !use_cache || self.prev_lines[row] != "~" {
                    queue!(
                        w,
                        MoveTo(0, row as u16),
                        Clear(ClearType::CurrentLine),
                        Print("~")
                    )?;
                    self.prev_lines[row] = "~".to_string();
                }
                continue;
            }
            let line = ed.buf.line_string(file_row);
            let mut col = 0usize;
            let start_col = self.off_x;
            let end_col = start_col + self.screen_cols as usize;
            if matches!(selection, Sel::None) {
                // Fast path: no selection; build string and cache
                let mut out = String::new();
                for g in line.graphemes(true) {
                    let gw = UnicodeWidthStr::width(g).max(1);
                    let next = col + gw;
                    if col < start_col {
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
                if self.prev_lines[row] != out {
                    queue!(
                        w,
                        MoveTo(0, row as u16),
                        Clear(ClearType::CurrentLine),
                        Print(&out)
                    )?;
                    self.prev_lines[row] = out;
                }
            } else {
                // Visual mode: render with background on selection; no caching
                queue!(w, MoveTo(0, row as u16), Clear(ClearType::CurrentLine))?;
                let (sel_start, sel_end) = match selection {
                    Sel::Char { sy, sx, ey, ex } => {
                        if file_row < sy || file_row > ey {
                            (usize::MAX, usize::MAX)
                        } else if sy == ey {
                            (sx, ex)
                        } else if file_row == sy {
                            (sx, ed.buf.line_width(file_row))
                        } else if file_row == ey {
                            (0, ex)
                        } else {
                            (0, ed.buf.line_width(file_row))
                        }
                    }
                    Sel::Line { sy, ey } => {
                        if file_row < sy || file_row > ey {
                            (usize::MAX, usize::MAX)
                        } else {
                            (0, ed.buf.line_width(file_row))
                        }
                    }
                    Sel::Block {
                        sy,
                        ey,
                        left,
                        right,
                    } => {
                        if file_row < sy || file_row > ey {
                            (usize::MAX, usize::MAX)
                        } else {
                            (left, right)
                        }
                    }
                    Sel::None => (usize::MAX, usize::MAX),
                };
                for g in line.graphemes(true) {
                    let gw = UnicodeWidthStr::width(g).max(1);
                    let next = col + gw;
                    if next <= start_col {
                        col = next;
                        continue;
                    }
                    if col >= end_col {
                        break;
                    }
                    let overlapped = sel_start != usize::MAX && (col < sel_end && next > sel_start);
                    if overlapped {
                        queue!(
                            w,
                            SetBackgroundColor(Color::DarkGrey),
                            SetForegroundColor(Color::White)
                        )?;
                    }
                    // Grapheme never needs clipping; viewport is grapheme-aligned via start_col check
                    queue!(w, Print(g))?;
                    if overlapped {
                        queue!(
                            w,
                            SetForegroundColor(Color::Reset),
                            SetBackgroundColor(Color::Reset)
                        )?;
                    }
                    col = next;
                    if col >= end_col {
                        break;
                    }
                }
                // No change to prev_lines in visual mode (we always redraw)
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
            crate::keymap::Mode::Visual => "VISUAL",
            crate::keymap::Mode::VisualLine => "VISUAL-LINE",
            crate::keymap::Mode::VisualBlock => "VISUAL-BLOCK",
        };
        let left_full = format!(
            " {}{} — {} lines [{}] ",
            fname,
            dirty,
            ed.buf.line_count(),
            mode
        );
        let right_full = format!(" {}/{} ", ed.cy + 1, ed.buf.line_count());
        let total = self.screen_cols as usize;
        // Compute widths
        let right_w = UnicodeWidthStr::width(right_full.as_str());
        // Space available for left side including padding
        let left = total.saturating_sub(right_w);
        let left_str = if left > 0 {
            // keep one space padding if possible
            let left_target = left;
            Self::truncate_to_width(&left_full, left_target)
        } else {
            String::new()
        };
        // If left side took less space than allocated, pad with spaces
        let left_w = UnicodeWidthStr::width(left_str.as_str());
        let pad = left.saturating_sub(left_w);
        let mut content = String::new();
        content.push_str(&left_str);
        if pad > 0 {
            content.push_str(&" ".repeat(pad));
        }
        // Append right side if it fits
        if right_w <= total.saturating_sub(left_w + pad) {
            content.push_str(&right_full);
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
        let msg = Self::truncate_to_width(&ed.status, self.screen_cols as usize);
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
            let shown =
                Self::truncate_to_width(&format!("{}{}", prompt, input), self.screen_cols as usize);
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
            let display =
                Self::truncate_to_width(&format!(":{}", input), self.screen_cols as usize);
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
