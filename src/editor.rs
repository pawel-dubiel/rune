use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::time::Instant;

use crate::buffer::Buffer;
use crate::keymap::{default_keymap, load_config, Action, Mode};

#[derive(Clone)]
struct EditorSnapshot {
    text: String,
    cx: usize,
    cy: usize,
    #[allow(dead_code)]
    mode: Mode,
}

impl EditorSnapshot {
    fn from_editor(ed: &Editor) -> Self {
        Self {
            text: ed.buf.to_string(),
            cx: ed.cx,
            cy: ed.cy,
            mode: ed.mode,
        }
    }
}

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
    pub pending: String,
    pub pending_started: Option<Instant>,
    pub op_pending: Option<(Action, usize)>,
    pub clipboard: String,
    undo_stack: Vec<EditorSnapshot>,
    redo_stack: Vec<EditorSnapshot>,
    undo_group_active: bool,
    count_group_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalInputResult {
    None,
    CommandPrompt,
}

impl Editor {
    fn on_edit_start(&mut self) {
        // Group insert-mode edits into a single undo step until leaving Insert
        if self.count_group_active {
        } else if matches!(self.mode, Mode::Insert) {
            if !self.undo_group_active {
                let snap = EditorSnapshot::from_editor(self);
                self.undo_stack.push(snap);
                self.redo_stack.clear();
                self.undo_group_active = true;
            }
        } else {
            let snap = EditorSnapshot::from_editor(self);
            self.undo_stack.push(snap);
            self.redo_stack.clear();
            self.undo_group_active = false;
        }
    }

    pub fn end_undo_group(&mut self) {
        self.undo_group_active = false;
    }

    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo_stack.pop() {
            let cur_mode = self.mode;
            let cur = EditorSnapshot::from_editor(self);
            self.redo_stack.push(cur);
            self.buf = Buffer::from_string(prev.text);
            self.cx = prev.cx;
            self.cy = prev.cy;
            // Do not change current mode on undo (match Vim: stay in Normal)
            self.mode = cur_mode;
            self.clamp_cursor();
            self.undo_group_active = false;
            return true;
        }
        false
    }

    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo_stack.pop() {
            let cur_mode = self.mode;
            let cur = EditorSnapshot::from_editor(self);
            self.undo_stack.push(cur);
            self.buf = Buffer::from_string(next.text);
            self.cx = next.cx;
            self.cy = next.cy;
            // Do not change current mode on redo
            self.mode = cur_mode;
            self.clamp_cursor();
            self.undo_group_active = false;
            return true;
        }
        false
    }
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
            pending: String::new(),
            pending_started: None,
            op_pending: None,
            clipboard: String::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            undo_group_active: false,
            count_group_active: false,
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
        let max_y = self.buf.line_count().saturating_sub(1);
        self.cy = self.cy.min(max_y);
        let line_w = self.buf.line_width(self.cy);
        self.cx = self.cx.min(line_w);
    }

    pub fn insert_char(&mut self, ch: char) {
        use unicode_width::UnicodeWidthChar;
        self.on_edit_start();
        self.buf.insert_char(self.cx, self.cy, ch);
        self.cx += UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        self.dirty = true;
    }

    pub fn insert_newline(&mut self) {
        self.on_edit_start();
        let x = self.cx;
        self.buf.insert_newline(x, self.cy);
        self.cy += 1;
        self.cx = 0;
        self.dirty = true;
    }

    pub fn delete_char(&mut self) {
        self.on_edit_start();
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
                } else if self.cy + 1 < self.buf.line_count() {
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
                if self.cy + 1 < self.buf.line_count() {
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
                // Default gg behavior: go to first line
                self.cy = 0;
                self.cx = 0;
            }
            GotoBottom => {
                // Default G behavior: go to last line
                if self.buf.line_count() > 0 {
                    self.cy = self.buf.line_count() - 1;
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
                self.on_edit_start();
                let len = self.buf.line_width(self.cy);
                self.buf.insert_newline(len, self.cy);
                self.cy += 1;
                self.cx = 0;
                self.dirty = true;
                self.mode = Mode::Insert;
            }
            OpenAbove => {
                self.on_edit_start();
                self.buf.insert_newline(0, self.cy);
                self.cx = 0;
                self.dirty = true;
                self.mode = Mode::Insert;
            }
            DeleteCharUnder => {
                self.on_edit_start();
                self.buf.delete_at(self.cx, self.cy);
                self.dirty = true;
            }
            DeleteLine => {
                self.on_edit_start();
                if self.buf.line_count() > 0 {
                    self.buf.delete_line(self.cy);
                    if self.cy >= self.buf.line_count() {
                        self.cy = self.buf.line_count().saturating_sub(1);
                    }
                    self.cx = 0;
                    self.dirty = true;
                }
            }
            CommandPrompt => {}
            OperatorDelete => {
                // Operator pending until a motion or target is supplied
                self.op_pending = Some((Action::OperatorDelete, 1));
            }
            OperatorChange => {
                self.op_pending = Some((Action::OperatorChange, 1));
            }
            OperatorYank => {
                self.op_pending = Some((Action::OperatorYank, 1));
            }
            Undo => {
                let _ = self.undo();
            }
            Redo => {
                let _ = self.redo();
            }
            MoveWordForward | MoveWordBackward | MoveEndWord => {
                self.apply_motion(act, 1, None);
            }
        }
        self.clamp_cursor();
    }

    fn apply_action_count(&mut self, act: Action, count: usize) {
        let n = count.max(1);
        // Only group counts for editing actions; movement-only counts should not create undo steps
        if n > 1 && !self.count_group_active && Self::is_editing_action(act) {
            let snap = EditorSnapshot::from_editor(self);
            self.undo_stack.push(snap);
            self.redo_stack.clear();
            self.count_group_active = true;
        }
        for _ in 0..n {
            self.apply_action(act);
        }
        self.count_group_active = false;
    }

    fn is_editing_action(act: Action) -> bool {
        matches!(
            act,
            Action::DeleteLine | Action::DeleteCharUnder | Action::OpenAbove | Action::OpenBelow
        )
    }

    fn parse_count_prefix(seq: &str) -> (Option<usize>, usize) {
        // Vim-style: counts start with [1-9], not 0. A leading '0' with no prior digits is a command (line start), not a count.
        let mut idx = 0usize;
        let mut first = true;
        for (i, ch) in seq.char_indices() {
            if first {
                if ('1'..='9').contains(&ch) {
                    idx = i + ch.len_utf8();
                    first = false;
                    continue;
                } else {
                    return (None, 0);
                }
            } else if ch.is_ascii_digit() {
                idx = i + ch.len_utf8();
            } else {
                break;
            }
        }
        if idx == 0 {
            (None, 0)
        } else {
            let count = seq[..idx].parse::<usize>().ok();
            (count, idx)
        }
    }

    pub fn process_normal_char(&mut self, c: char) -> NormalInputResult {
        // Append char and resolve pending with count support
        self.pending.push(c);
        loop {
            // Extract count prefix if present
            let (count, rest_idx) = Self::parse_count_prefix(&self.pending);
            let rest = &self.pending[rest_idx..];
            if count.is_some() && rest.is_empty() {
                // Have a count but no command yet; wait for more input
                if self.pending_started.is_none() {
                    self.pending_started = Some(Instant::now());
                }
                return NormalInputResult::None;
            }
            // Exact match on remainder?
            if !rest.is_empty() {
                if let Some(&act) = self.keymap.get(rest) {
                    if matches!(act, Action::CommandPrompt) {
                        self.pending.clear();
                        self.pending_started = None;
                        return NormalInputResult::CommandPrompt;
                    } else {
                        // Special Vim semantics for counts on gg and G
                        if let Some(n) = count {
                            if rest == "gg" {
                                self.goto_line(n);
                                self.pending.clear();
                                self.pending_started = None;
                                return NormalInputResult::None;
                            }
                            if rest == "G" {
                                self.goto_line(n);
                                self.pending.clear();
                                self.pending_started = None;
                                return NormalInputResult::None;
                            }
                        }
                        // Operator/motion handling
                        if let Some((op_kind, n0)) = self.op_pending.take() {
                            let effective = count.unwrap_or(n0);
                            match (op_kind, act) {
                                (Action::OperatorDelete, Action::OperatorDelete)
                                | (Action::OperatorDelete, Action::DeleteLine) => {
                                    self.apply_action_count(Action::DeleteLine, effective);
                                }
                                (Action::OperatorChange, Action::OperatorChange) => {
                                    // Change whole line(s): clear content but keep line
                                    for _ in 0..effective.max(1) {
                                        self.buf.clear_line(self.cy);
                                        self.cx = 0;
                                        self.dirty = true;
                                        if self.cy + 1 < self.buf.line_count() {
                                            self.cy += 1;
                                        }
                                    }
                                    self.mode = Mode::Insert;
                                    self.cy = self.cy.saturating_sub(effective.saturating_sub(1));
                                }
                                (Action::OperatorYank, Action::OperatorYank) => {
                                    // Yank whole line(s)
                                    let end = (self.cy + effective).min(self.buf.line_count());
                                    let mut parts = Vec::new();
                                    for y in self.cy..end {
                                        parts.push(self.buf.line_string(y));
                                    }
                                    self.clipboard = parts.join("\n");
                                }
                                (opk, Action::MoveWordForward)
                                | (opk, Action::MoveWordBackward)
                                | (opk, Action::MoveEndWord)
                                | (opk, Action::LineStart)
                                | (opk, Action::LineEnd) => {
                                    self.apply_motion(act, effective, Some((opk, effective)));
                                }
                                (_, other) => {
                                    // Fallback, apply action normally and drop operator
                                    self.apply_action_count(other, effective);
                                }
                            }
                        } else if matches!(
                            act,
                            Action::OperatorDelete | Action::OperatorChange | Action::OperatorYank
                        ) {
                            // operator becomes pending
                            let opk = act;
                            self.op_pending = Some((opk, count.unwrap_or(1)));
                            self.pending.clear();
                            self.pending_started = Some(Instant::now());
                            return NormalInputResult::None;
                        } else {
                            self.apply_action_count(act, count.unwrap_or(1));
                        }
                        self.pending.clear();
                        self.pending_started = None;
                        return NormalInputResult::None;
                    }
                }
            }
            // Any prefix match on remainder?
            let has_prefix = self
                .keymap
                .keys()
                .any(|k| !rest.is_empty() && k.starts_with(rest));
            if has_prefix {
                // Wait for more input
                if self.pending_started.is_none() {
                    self.pending_started = Some(Instant::now());
                }
                return NormalInputResult::None;
            }
            // No prefix: try longest valid prefix from the start (taking into account counts)
            let mut consumed = 0usize;
            let mut run: Option<(Action, usize)> = None;
            // Try all splits of pending into [count][key]
            for i in (1..=self.pending.len()).rev() {
                let candidate = &self.pending[..i];
                let (cnt, key_idx) = Self::parse_count_prefix(candidate);
                let key = &candidate[key_idx..];
                if !key.is_empty() {
                    if let Some(&act) = self.keymap.get(key) {
                        run = Some((act, cnt.unwrap_or(1)));
                        consumed = i;
                        break;
                    }
                }
            }
            if let Some((act, n)) = run {
                if matches!(act, Action::CommandPrompt) {
                    self.pending.clear();
                    self.pending_started = None;
                    return NormalInputResult::CommandPrompt;
                } else {
                    // Handle counts on gg/G in greedy resolution too
                    let candidate = &self.pending[..consumed];
                    let (cnt, idx2) = Self::parse_count_prefix(candidate);
                    let key2 = &candidate[idx2..];
                    if let Some(nn) = cnt {
                        if key2 == "gg" || key2 == "G" {
                            self.goto_line(nn);
                        } else if let Some((Action::OperatorDelete, n0)) = self.op_pending.take() {
                            let effective = nn.max(n0);
                            match act {
                                Action::OperatorDelete | Action::DeleteLine => {
                                    self.apply_action_count(Action::DeleteLine, effective);
                                }
                                Action::MoveWordForward
                                | Action::MoveWordBackward
                                | Action::MoveEndWord
                                | Action::LineStart
                                | Action::LineEnd => {
                                    self.apply_motion(
                                        act,
                                        effective,
                                        Some((Action::OperatorDelete, effective)),
                                    );
                                }
                                _ => {
                                    self.apply_action_count(act, effective);
                                }
                            }
                        } else if matches!(act, Action::OperatorDelete) {
                            self.op_pending = Some((Action::OperatorDelete, nn));
                        } else {
                            self.op_pending = None;
                            self.apply_action_count(act, n);
                        }
                    } else if let Some((op_kind, n0)) = self.op_pending.take() {
                        let effective = n.max(n0);
                        match act {
                            Action::OperatorDelete | Action::DeleteLine => {
                                self.apply_action_count(Action::DeleteLine, effective);
                            }
                            Action::MoveWordForward
                            | Action::MoveWordBackward
                            | Action::MoveEndWord
                            | Action::LineStart
                            | Action::LineEnd => {
                                self.apply_motion(act, effective, Some((op_kind, effective)));
                            }
                            _ => {
                                self.apply_action_count(act, effective);
                            }
                        }
                    } else {
                        self.op_pending = None;
                        self.apply_action_count(act, n);
                    }
                    self.pending = self.pending[consumed..].to_string();
                    // Loop to handle possibly more commands buffered
                    if self.pending.is_empty() {
                        self.pending_started = None;
                        return NormalInputResult::None;
                    }
                    continue;
                }
            }
            // Drop the first char and try again; if empty, give up
            if !self.pending.is_empty() {
                self.pending.remove(0);
            }
            if self.pending.is_empty() {
                self.pending_started = None;
                return NormalInputResult::None;
            }
        }
    }

    fn goto_line(&mut self, n1: usize) {
        if self.buf.line_count() == 0 {
            self.cy = 0;
            self.cx = 0;
            return;
        }
        let max = self.buf.line_count();
        let target = n1.min(max).saturating_sub(1);
        self.cy = target;
        self.cx = 0;
        self.clamp_cursor();
    }

    fn apply_motion(&mut self, act: Action, count: usize, op: Option<(Action, usize)>) {
        let n = count.max(1);
        let mut grouped_here = false;
        if n > 1 {
            if let Some((op_kind, _)) = op {
                // Group counted operator+motion as single undo unit
                if matches!(op_kind, Action::OperatorDelete | Action::OperatorChange)
                    && !self.count_group_active
                {
                    let snap = EditorSnapshot::from_editor(self);
                    self.undo_stack.push(snap);
                    self.redo_stack.clear();
                    self.count_group_active = true;
                    grouped_here = true;
                }
            }
        }
        match act {
            Action::MoveWordForward => {
                let mut y = self.cy;
                let mut target_c = self.cx;
                for step in 0..n {
                    let cur_next = self.buf.next_word_start(target_c, y);
                    let next_line_word = if cur_next == target_c {
                        // No progress on this line: choose next line start or next word depending on remaining count and operator
                        if y + 1 >= self.buf.line_count() {
                            None
                        } else if let Some((opk, _)) = op {
                            let rem = n - step;
                            if matches!(opk, Action::OperatorDelete | Action::OperatorChange) {
                                if rem <= 1 {
                                    // dw at EOL: delete just the newline (preserve indentation)
                                    Some((y + 1, 0))
                                } else {
                                    // more to go: move to next word start on next line
                                    self.find_next_word_start_from(y + 1)
                                }
                            } else {
                                self.find_next_word_start_from(y + 1)
                            }
                        } else {
                            self.find_next_word_start_from(y + 1)
                        }
                    } else {
                        Some((y, cur_next))
                    };
                    if let Some((ny, nx)) = next_line_word {
                        y = ny;
                        target_c = nx;
                    } else {
                        break;
                    }
                }
                self.apply_range_or_move((y, target_c), false, op);
            }
            Action::MoveWordBackward => {
                let mut y = self.cy;
                let mut target_c = self.cx;
                for _ in 0..n {
                    let cur_prev = self.buf.prev_word_start(target_c, y);
                    let prev_line_word = if cur_prev == target_c {
                        self.find_prev_word_start_from(y.saturating_sub(1))
                    } else {
                        Some((y, cur_prev))
                    };
                    if let Some((ny, nx)) = prev_line_word {
                        y = ny;
                        target_c = nx;
                    } else {
                        break;
                    }
                }
                self.apply_range_or_move((y, target_c), false, op);
            }
            Action::MoveEndWord => {
                let mut y = self.cy;
                let mut target_c = self.cx;
                for _ in 0..n {
                    let cur_end = self.buf.end_of_word(target_c, y);
                    // If no progress, go to end of first word in next lines
                    let next_line_end = if cur_end == target_c {
                        self.find_next_word_end_from(y + 1)
                    } else {
                        Some((y, cur_end))
                    };
                    if let Some((ny, nx)) = next_line_end {
                        y = ny;
                        target_c = nx;
                    } else {
                        break;
                    }
                }
                self.apply_range_or_move((y, target_c), true, op);
            }
            Action::LineStart => {
                self.apply_range_or_move((self.cy, 0), false, op);
            }
            Action::LineEnd => {
                let end = self.buf.line_width(self.cy);
                self.apply_range_or_move((self.cy, end), true, op);
            }
            _ => {
                // Fallback
                if op.is_none() {
                    self.apply_action_count(act, n);
                }
            }
        }
        if grouped_here {
            self.count_group_active = false;
        }
    }

    fn apply_range_or_move(
        &mut self,
        target: (usize, usize),
        inclusive: bool,
        op: Option<(Action, usize)>,
    ) {
        if let Some((op_kind, _op_count)) = op.or_else(|| self.op_pending.take()) {
            // When operator pending, the motion count applies to the motion if present.
            if matches!(op_kind, Action::OperatorDelete | Action::OperatorChange) {
                self.on_edit_start();
            }
            let target_col = target.1;
            let target_line = target.0;
            let last_col = self.cx;
            let last_line = self.cy;
            let (sy, sx, ey, ex) = if (target_line > last_line)
                || (target_line == last_line && target_col >= last_col)
            {
                (last_line, last_col, target_line, target_col)
            } else {
                (target_line, target_col, last_line, last_col)
            };
            if (ey, ex) > (sy, sx) {
                match op_kind {
                    Action::OperatorDelete | Action::OperatorChange => {
                        self.delete_range((sy, sx), (ey, ex), inclusive);
                        self.cx = sx;
                        self.cy = sy;
                        self.dirty = true;
                        if matches!(op_kind, Action::OperatorChange) {
                            self.mode = Mode::Insert;
                        }
                    }
                    Action::OperatorYank => {
                        self.clipboard = self.extract_range((sy, sx), (ey, ex), inclusive);
                    }
                    _ => {}
                }
            }
        } else if op.is_none() {
            self.cx = target.1;
            self.cy = target.0;
        }
    }

    fn extract_range(&self, start: (usize, usize), end: (usize, usize), inclusive: bool) -> String {
        let (sy, sx) = start;
        let (ey, ex0) = end;
        let start_char = self.buf.char_index_at_col(sy, sx);
        let end_col = if inclusive {
            self.buf.next_col(ex0, ey)
        } else {
            ex0
        };
        let end_char = self.buf.char_index_at_col(ey, end_col);
        self.buf.string_from_char_range(start_char, end_char)
    }

    fn delete_range(&mut self, start: (usize, usize), end: (usize, usize), inclusive: bool) {
        let (sy, sx) = start;
        let (ey, ex0) = end;
        let start_char = self.buf.char_index_at_col(sy, sx);
        let end_col = if inclusive {
            self.buf.next_col(ex0, ey)
        } else {
            ex0
        };
        let end_char = self.buf.char_index_at_col(ey, end_col);
        self.buf.remove_char_range(start_char, end_char);
    }

    #[cfg(test)]
    fn undo_stack_len(&self) -> usize {
        self.undo_stack.len()
    }

    fn find_next_word_start_from(&self, mut y: usize) -> Option<(usize, usize)> {
        use unicode_segmentation::UnicodeSegmentation;
        while y < self.buf.line_count() {
            let row = self.buf.line_string(y);
            for (i, seg) in UnicodeSegmentation::split_word_bound_indices(row.as_str()) {
                if seg.chars().any(|c| c.is_alphanumeric() || c == '_') {
                    // convert i (byte) to col
                    let mut acc = 0usize;
                    let mut bpos = 0usize;
                    for g in row.graphemes(true) {
                        if bpos >= i {
                            break;
                        }
                        acc += unicode_width::UnicodeWidthStr::width(g).max(1);
                        bpos += g.len();
                    }
                    return Some((y, acc));
                }
            }
            y += 1;
        }
        None
    }

    fn find_prev_word_start_from(&self, y: usize) -> Option<(usize, usize)> {
        use unicode_segmentation::UnicodeSegmentation;
        let mut yy = y;
        loop {
            if yy < self.buf.line_count() {
                let row = self.buf.line_string(yy);
                let mut last: Option<usize> = None;
                for (i, seg) in UnicodeSegmentation::split_word_bound_indices(row.as_str()) {
                    if seg.chars().any(|c| c.is_alphanumeric() || c == '_') {
                        last = Some(i);
                    }
                }
                if let Some(i) = last {
                    // byte to col
                    let mut acc = 0usize;
                    let mut bpos = 0usize;
                    for g in row.graphemes(true) {
                        if bpos >= i {
                            break;
                        }
                        acc += unicode_width::UnicodeWidthStr::width(g).max(1);
                        bpos += g.len();
                    }
                    return Some((yy, acc));
                }
            }
            if yy == 0 {
                break;
            }
            yy -= 1;
        }
        None
    }

    fn find_next_word_end_from(&self, mut y: usize) -> Option<(usize, usize)> {
        use unicode_segmentation::UnicodeSegmentation;
        while y < self.buf.line_count() {
            let row = self.buf.line_string(y);
            for (i, seg) in UnicodeSegmentation::split_word_bound_indices(row.as_str()) {
                if seg.chars().any(|c| c.is_alphanumeric() || c == '_') {
                    let end_b = i + seg.len();
                    // byte to col including end
                    let mut acc = 0usize;
                    let mut bpos = 0usize;
                    for g in row.graphemes(true) {
                        let next_b = bpos + g.len();
                        let w = unicode_width::UnicodeWidthStr::width(g).max(1);
                        if next_b > end_b {
                            break;
                        }
                        acc += w;
                        bpos = next_b;
                    }
                    return Some((y, acc));
                }
            }
            y += 1;
        }
        None
    }

    pub fn process_pending_timeout(&mut self) -> NormalInputResult {
        if self.pending.is_empty() {
            return NormalInputResult::None;
        }
        // If only a count is pending, clear it
        let (cnt, idx) = Self::parse_count_prefix(&self.pending);
        if cnt.is_some() && idx == self.pending.len() {
            self.pending.clear();
            self.pending_started = None;
            return NormalInputResult::None;
        }
        // Try greedy longest prefix split into [count][key]
        let mut consumed = 0usize;
        let mut run: Option<(Action, usize)> = None;
        for i in (1..=self.pending.len()).rev() {
            let candidate = &self.pending[..i];
            let (cnt2, key_idx) = Self::parse_count_prefix(candidate);
            let key = &candidate[key_idx..];
            if !key.is_empty() {
                if let Some(&act) = self.keymap.get(key) {
                    run = Some((act, cnt2.unwrap_or(1)));
                    consumed = i;
                    break;
                }
            }
        }
        if let Some((act, n)) = run {
            if matches!(act, Action::CommandPrompt) {
                self.pending.clear();
                self.pending_started = None;
                return NormalInputResult::CommandPrompt;
            } else {
                if matches!(act, Action::OperatorDelete) {
                    self.op_pending = Some((Action::OperatorDelete, n));
                } else {
                    self.apply_action_count(act, n);
                    self.op_pending = None;
                }
                self.pending = self.pending[consumed..].to_string();
                if self.pending.is_empty() {
                    self.pending_started = None;
                    return NormalInputResult::None;
                }
                // If more remains but no more input, keep it but reset timer
                self.pending_started = Some(Instant::now());
                return NormalInputResult::None;
            }
        }
        // Nothing matched; clear pending
        self.pending.clear();
        self.pending_started = None;
        NormalInputResult::None
    }

    pub fn time_until_pending_timeout(&self, timeout_ms: u64) -> Option<std::time::Duration> {
        let start = self.pending_started?;
        let timeout = std::time::Duration::from_millis(timeout_ms);
        let now = Instant::now();
        let end = start + timeout;
        if now >= end {
            Some(std::time::Duration::from_millis(0))
        } else {
            Some(end - now)
        }
    }

    pub fn execute_ex_command(&mut self, cmd: &str) -> bool {
        let s = cmd.trim();
        if s == "$" {
            if self.buf.line_count() > 0 {
                self.cy = self.buf.line_count() - 1;
                self.cx = 0;
            }
            return true;
        }
        if (s.starts_with('+') || s.starts_with('-')) && s.len() > 1 {
            let sign = if s.starts_with('+') { 1isize } else { -1isize };
            if let Ok(n) = s[1..].parse::<isize>() {
                let base = self.cy as isize;
                let target = (base + sign * n)
                    .clamp(0, (self.buf.line_count().saturating_sub(1)) as isize)
                    as usize;
                self.cy = target;
                self.cx = 0;
                self.clamp_cursor();
                return true;
            }
        }
        if s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty() {
            if let Ok(mut n) = s.parse::<usize>() {
                if n == 0 {
                    n = 1;
                }
                if self.buf.line_count() > 0 {
                    let max = self.buf.line_count();
                    let target = n.min(max).saturating_sub(1);
                    self.cy = target;
                    self.cx = 0;
                    self.clamp_cursor();
                }
            }
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_mode_3dd_deletes_three_lines() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Normal;
        ed.buf.rows = vec![
            "l1".to_string(),
            "l2".to_string(),
            "l3".to_string(),
            "l4".to_string(),
            "l5".to_string(),
        ];
        ed.cy = 1; // start at second line
        for ch in "3dd".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.buf.rows, vec!["l1".to_string(), "l5".to_string()]);
        assert_eq!(ed.cy, 1); // cursor on what became former l5
    }

    #[test]
    fn normal_mode_10j_moves_down_multiple_lines() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Normal;
        ed.buf.rows = (0..15).map(|i| format!("{}", i)).collect();
        ed.cy = 0;
        for ch in "10j".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.cy, 10);
        for ch in "5k".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.cy, 5);
    }

    #[test]
    fn normal_mode_count_gg_and_g_go_to_line() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Normal;
        ed.buf.rows = (1..=20).map(|i| format!("{}", i)).collect();
        ed.cy = 10;
        for ch in "5gg".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.cy, 4); // line 5 (0-based)
        for ch in "10G".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.cy, 9); // line 10
        for ch in "G".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.cy, 19); // bottom
        for ch in "99gg".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.cy, 19); // clamped bottom
    }

    #[test]
    fn ex_command_number_moves_to_line() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Normal;
        ed.buf.rows = (1..=15).map(|i| format!("{}", i)).collect();
        ed.cy = 5;
        // Simulate entering ':' then numeric command
        let res = ed.process_normal_char(':');
        assert_eq!(res, NormalInputResult::CommandPrompt);
        assert!(ed.execute_ex_command("10"));
        assert_eq!(ed.cy, 9);
        assert!(ed.execute_ex_command("999"));
        assert_eq!(ed.cy, 14);
        assert!(ed.execute_ex_command("0")); // clamps to 1
        assert_eq!(ed.cy, 0);
    }

    #[test]
    fn pending_timeout_clears_incomplete_sequence() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Normal;
        ed.buf.rows = (0..=5).map(|i| format!("{}", i)).collect();
        let _ = ed.process_normal_char('g'); // start a prefix
                                             // Simulate timeout without sleeping
        let _ = ed.process_pending_timeout();
        assert_eq!(ed.pending, "");
        // Count-only pending also clears on timeout
        let _ = ed.process_normal_char('3');
        let _ = ed.process_pending_timeout();
        assert_eq!(ed.pending, "");
    }

    #[test]
    fn operator_dw_and_d_dollar() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Normal;
        ed.buf.rows = vec!["hello world".into(), "second".into()];
        ed.cy = 0;
        ed.cx = 0;
        // dw from start should remove "hello "
        for ch in "dw".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.buf.rows[0], "world");
        // reset
        ed.buf.rows[0] = "hello world".into();
        ed.cx = 6; // before 'w'
        for ch in "d$".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.buf.rows[0], "hello ");
        // counts: 3w moves three words (here only two words so clamp)
        ed.buf.rows = vec!["one two three four".into()];
        ed.cy = 0;
        ed.cx = 0;
        for ch in "3w".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert!(ed.cx > 0);
        // 2dw deletes two words from current position
        let start_text = ed.buf.rows[0].clone();
        for ch in "2dw".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert!(ed.buf.rows[0].len() < start_text.len());
    }

    #[test]
    fn undo_redo_insert_and_delete() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Insert;
        ed.buf.rows = vec![String::new()];
        ed.insert_char('a');
        ed.insert_char('b');
        ed.insert_char('c');
        assert_eq!(ed.buf.rows[0], "abc");
        // Undo should revert the entire insert group while in insert mode
        assert!(ed.undo());
        assert_eq!(ed.buf.rows[0], "");
        assert!(ed.redo());
        assert_eq!(ed.buf.rows[0], "abc");

        ed.mode = Mode::Normal;
        // dd with undo/redo
        for ch in "dd".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.buf.rows[0], "");
        assert!(ed.undo());
        assert_eq!(ed.buf.rows.len(), 1);
        // buffer restored before dd
        assert_eq!(ed.buf.rows[0], "abc");
        assert!(ed.redo());
        assert_eq!(ed.buf.rows[0], "");
    }

    #[test]
    fn undo_redo_for_3dd() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Normal;
        ed.buf.rows = vec![
            "l1".into(),
            "l2".into(),
            "l3".into(),
            "l4".into(),
            "l5".into(),
        ];
        ed.cy = 1;
        for ch in "3dd".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.buf.rows, vec![String::from("l1"), String::from("l5")]);
        // Single undo restores all 3 lines; redo removes them again
        assert!(ed.undo());
        assert_eq!(
            ed.buf.rows,
            vec![
                String::from("l1"),
                String::from("l2"),
                String::from("l3"),
                String::from("l4"),
                String::from("l5"),
            ]
        );
        // Redo should reapply the 3dd change
        // Redo should reapply the 3dd change
        // Note: redo behavior is covered in other tests
        assert!(ed.redo());
        assert_eq!(ed.buf.rows, vec![String::from("l1"), String::from("l5")]);
    }

    #[test]
    fn undo_redo_dw_and_dollar() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Normal;
        ed.buf.rows = vec!["hello world".into()];
        ed.cy = 0;
        ed.cx = 0;
        for ch in "dw".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.buf.rows[0], "world");
        assert!(ed.undo());
        assert_eq!(ed.buf.rows[0], "hello world");
        assert!(ed.redo());
        assert_eq!(ed.buf.rows[0], "world");

        ed.buf.rows = vec!["hello world".into()];
        ed.cx = 6;
        for ch in "d$".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.buf.rows[0], "hello ");
        assert!(ed.undo());
        assert_eq!(ed.buf.rows[0], "hello world");
        assert!(ed.redo());
        assert_eq!(ed.buf.rows[0], "hello ");
    }

    #[test]
    fn counted_motion_does_not_create_undo_step() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Normal;
        ed.buf.rows = (0..30).map(|i| format!("line{}", i)).collect();
        let before = ed.undo_stack_len();
        for ch in "10j".chars() {
            let _ = ed.process_normal_char(ch);
        }
        // Pure motion should not add to undo stack
        assert_eq!(ed.undo_stack_len(), before);
        // Undo should be a no-op
        assert!(!ed.undo());
    }

    #[test]
    fn undo_redo_cw() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Normal;
        ed.buf.rows = vec!["hello world".into()];
        ed.cy = 0;
        ed.cx = 0;
        for ch in "cw".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.buf.rows[0], "world");
        // Insert-mode undo grouping: leaving insert not required here; cw is one change
        assert!(ed.undo());
        assert_eq!(ed.buf.rows[0], "hello world");
        assert!(ed.redo());
        assert_eq!(ed.buf.rows[0], "world");
    }

    #[test]
    fn insert_undo_break_ctrl_g_u_behaviour() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Insert;
        ed.buf.rows = vec![String::new()];
        ed.insert_char('a');
        ed.insert_char('b');
        ed.insert_char('c');
        // simulate Ctrl-g u: end current undo group but stay in insert mode
        ed.end_undo_group();
        ed.insert_char('d');
        ed.insert_char('e');
        ed.insert_char('f');
        assert_eq!(ed.buf.rows[0], "abcdef");
        // Undo only removes 'def'
        assert!(ed.undo());
        assert_eq!(ed.buf.rows[0], "abc");
        // Undo again removes 'abc'
        assert!(ed.undo());
        assert_eq!(ed.buf.rows[0], "");
        // Redo restores 'abc', then 'def'
        assert!(ed.redo());
        assert_eq!(ed.buf.rows[0], "abc");
        assert!(ed.redo());
        assert_eq!(ed.buf.rows[0], "abcdef");
    }

    #[test]
    fn dw_at_eol_joins_next_line() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Normal;
        ed.buf.rows = vec!["foo".into(), "  bar baz".into()];
        ed.cy = 0;
        ed.cx = ed.buf.line_width(0); // end of first line
        for ch in "dw".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.buf.rows, vec![String::from("foo  bar baz")]);
        assert_eq!(ed.cy, 0);
    }

    #[test]
    fn cw_changes_word_and_enters_insert() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Normal;
        ed.buf.rows = vec!["hello world".into()];
        ed.cy = 0;
        ed.cx = 0;
        for ch in "cw".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.buf.rows[0], "world");
        assert!(matches!(ed.mode, Mode::Insert));
    }

    #[test]
    fn y_dollar_yanks_to_clipboard() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Normal;
        ed.buf.rows = vec!["hello world".into()];
        ed.cy = 0;
        ed.cx = 6; // before 'w'
        for ch in "y$".chars() {
            let _ = ed.process_normal_char(ch);
        }
        assert_eq!(ed.clipboard, "world");
        // content remains unchanged
        assert_eq!(ed.buf.rows[0], "hello world");
    }

    #[test]
    fn operator_3dw_across_lines() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Normal;
        ed.buf.rows = vec!["one two".into(), "three four".into()];
        ed.cy = 0;
        ed.cx = 0;
        for ch in "3dw".chars() {
            let _ = ed.process_normal_char(ch);
        }
        // Current semantics: deletes first two words across EOL, leaving the next line intact
        assert_eq!(ed.buf.rows, vec![String::from("three four")]);
        // Single undo should restore original
        assert!(ed.undo());
        assert_eq!(
            ed.buf.rows,
            vec![String::from("one two"), String::from("three four")]
        );
    }

    #[test]
    fn operator_2cw_across_lines_enters_insert() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Normal;
        ed.buf.rows = vec!["one two".into(), "three four".into()];
        ed.cy = 0;
        ed.cx = 0;
        for ch in "2cw".chars() {
            let _ = ed.process_normal_char(ch);
        }
        // After changing two words across lines, leaves "three four" and enters Insert
        // Current semantics: changes up to start of next line; leaves an empty first line
        assert_eq!(
            ed.buf.rows,
            vec![String::from(""), String::from("three four")]
        );
        assert!(matches!(ed.mode, super::Mode::Insert));
        // Undo restores original in single step
        assert!(ed.undo());
        assert_eq!(
            ed.buf.rows,
            vec![String::from("one two"), String::from("three four")]
        );
        // Redo re-applies change and returns to Insert
        assert!(ed.redo());
        assert_eq!(
            ed.buf.rows,
            vec![String::from(""), String::from("three four")]
        );
        assert!(matches!(ed.mode, super::Mode::Insert));
    }

    #[test]
    fn ex_commands_dollar_plus_minus() {
        let mut ed = Editor::new().unwrap();
        ed.mode = Mode::Normal;
        ed.buf.rows = (1..=10).map(|i| format!("{}", i)).collect();
        ed.cy = 5;
        assert!(ed.execute_ex_command("$"));
        assert_eq!(ed.cy, 9);
        assert!(ed.execute_ex_command("+2"));
        assert_eq!(ed.cy, 9); // clamp beyond bottom
        assert!(ed.execute_ex_command("-3"));
        assert_eq!(ed.cy, 6);
        assert!(ed.execute_ex_command("-100"));
        assert_eq!(ed.cy, 0);
    }
}
