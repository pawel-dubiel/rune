use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::time::Instant;

use crate::buffer::Buffer;
use crate::keymap::{default_keymap, load_config, Action, Mode};

pub struct Editor {
    pub buf: Buffer,
    pub filename: Option<PathBuf>,
    pub dirty: bool,
    pub cx: usize,
    pub cy: usize,
    pub status: String,
    pub status_time: Instant,
    pub quit_times: u8,
    pub mode: Mode,
    pub keymap: HashMap<String, Action>,
    pub pending: Option<char>,
}

impl Editor {
    pub fn new() -> io::Result<Self> {
        let mut ed = Self {
            buf: Buffer::default(),
            filename: None,
            dirty: false,
            cx: 0,
            cy: 0,
            status: String::from(""),
            status_time: Instant::now(),
            quit_times: 1,
            mode: Mode::Normal,
            keymap: default_keymap(),
            pending: None,
        };
        let cfg = load_config(ed.keymap.clone());
        ed.keymap = cfg.keymap;
        if cfg.start_in_insert {
            ed.mode = Mode::Insert;
            ed.status = String::from("INSERT — Esc:normal  :w save  :q quit");
        } else {
            ed.status = String::from("NORMAL — i:insert  :w save  :q quit");
        }
        Ok(ed)
    }

    pub fn open(&mut self, path: PathBuf) -> io::Result<()> {
        let s = std::fs::read_to_string(&path)?;
        self.buf = Buffer::from_string(s);
        self.filename = Some(path);
        self.cx = 0;
        self.cy = 0;
        self.dirty = false;
        self.set_status("Opened file");
        Ok(())
    }

    pub fn save(&mut self) -> io::Result<()> {
        let Some(path) = self.filename.clone() else {
            self.set_status("No filename set");
            return Ok(());
        };
        std::fs::write(path, self.buf.to_string())?;
        self.dirty = false;
        self.set_status("Saved");
        Ok(())
    }

    pub fn set_status(&mut self, msg: &str) {
        self.status = msg.to_string();
        self.status_time = Instant::now();
    }

    pub fn clamp_cursor(&mut self) {
        let max_y = self.buf.rows.len().saturating_sub(1);
        self.cy = self.cy.min(max_y);
        let line_w = self.buf.line_width(self.cy);
        self.cx = self.cx.min(line_w);
    }

    pub fn insert_char(&mut self, ch: char) {
        use unicode_width::UnicodeWidthChar;
        self.buf.insert_char(self.cx, self.cy, ch);
        self.cx += UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        self.dirty = true;
    }

    pub fn insert_newline(&mut self) {
        let x = self.cx;
        self.buf.insert_newline(x, self.cy);
        self.cy += 1;
        self.cx = 0;
        self.dirty = true;
    }

    pub fn delete_char(&mut self) {
        if self.cx > 0 {
            self.cx = self.buf.delete_prev(self.cx, self.cy);
            self.dirty = true;
        } else if self.cy > 0 {
            let new_x = self.buf.merge_up(self.cy);
            self.cy -= 1;
            self.cx = new_x;
            self.dirty = true;
        }
    }

    pub fn apply_action(&mut self, act: Action) {
        use Action::*;
        match act {
            MoveLeft => {
                if self.cx > 0 {
                    self.cx = self.buf.prev_col(self.cx, self.cy);
                } else if self.cy > 0 {
                    self.cy -= 1;
                    self.cx = self.buf.line_width(self.cy);
                }
            }
            MoveRight => {
                let len = self.buf.line_width(self.cy);
                if self.cx < len {
                    self.cx = self.buf.next_col(self.cx, self.cy);
                } else if self.cy + 1 < self.buf.rows.len() {
                    self.cy += 1;
                    self.cx = 0;
                }
            }
            MoveUp => {
                if self.cy > 0 {
                    self.cy -= 1;
                }
            }
            MoveDown => {
                if self.cy + 1 < self.buf.rows.len() {
                    self.cy += 1;
                }
            }
            LineStart => {
                self.cx = 0;
            }
            LineEnd => {
                self.cx = self.buf.line_width(self.cy);
            }
            GotoTop => {
                self.cy = 0;
                self.cx = 0;
            }
            GotoBottom => {
                if !self.buf.rows.is_empty() {
                    self.cy = self.buf.rows.len() - 1;
                    self.cx = self.buf.line_width(self.cy);
                }
            }
            EnterInsert => {
                self.mode = Mode::Insert;
            }
            Append => {
                let len = self.buf.line_width(self.cy);
                if self.cx < len {
                    self.cx = self.buf.next_col(self.cx, self.cy);
                }
                self.mode = Mode::Insert;
            }
            OpenBelow => {
                let len = self.buf.line_width(self.cy);
                self.buf.insert_newline(len, self.cy);
                self.cy += 1;
                self.cx = 0;
                self.dirty = true;
                self.mode = Mode::Insert;
            }
            OpenAbove => {
                self.buf.insert_newline(0, self.cy);
                self.cx = 0;
                self.dirty = true;
                self.mode = Mode::Insert;
            }
            DeleteCharUnder => {
                self.buf.delete_at(self.cx, self.cy);
                self.dirty = true;
            }
            DeleteLine => {
                if !self.buf.rows.is_empty() {
                    self.buf.rows.remove(self.cy);
                    if self.buf.rows.is_empty() {
                        self.buf.rows.push(String::new());
                        self.cy = 0;
                        self.cx = 0;
                    } else if self.cy >= self.buf.rows.len() {
                        self.cy = self.buf.rows.len() - 1;
                        self.cx = 0;
                    } else {
                        self.cx = 0;
                    }
                    self.dirty = true;
                }
            }
            CommandPrompt => {}
        }
        self.clamp_cursor();
    }
}
