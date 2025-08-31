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
    pub pending: String,
    pub pending_started: Option<Instant>,
    pub op_pending: Option<(Action, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalInputResult {
    None,
    CommandPrompt,
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
            pending: String::new(),
            pending_started: None,
            op_pending: None,
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
                // Default gg behavior: go to first line
                self.cy = 0;
                self.cx = 0;
            }
            GotoBottom => {
                // Default G behavior: go to last line
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
            OperatorDelete => {
                // Operator pending until a motion or target is supplied
                self.op_pending = Some((Action::OperatorDelete, 1));
            }
            MoveWordForward | MoveWordBackward | MoveEndWord => {
                self.apply_motion(act, 1, None);
            }
        }
        self.clamp_cursor();
    }

    fn apply_action_count(&mut self, act: Action, count: usize) {
        let n = count.max(1);
        for _ in 0..n {
            self.apply_action(act);
        }
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
                        if let Some((Action::OperatorDelete, n0)) = self.op_pending.take() {
                            let effective = count.unwrap_or(n0);
                            match act {
                                Action::OperatorDelete | Action::DeleteLine => {
                                    self.apply_action_count(Action::DeleteLine, effective);
                                }
                                Action::MoveWordForward
                                | Action::MoveWordBackward
                                | Action::MoveEndWord
                                | Action::LineStart
                                | Action::LineEnd => {
                                    self.apply_motion(act, effective, Some(effective));
                                }
                                _ => {
                                    // Fallback, apply action normally and drop operator
                                    self.apply_action_count(act, effective);
                                }
                            }
                        } else if matches!(act, Action::OperatorDelete) {
                            // 'd' alone becomes operator-pending
                            self.op_pending = Some((Action::OperatorDelete, count.unwrap_or(1)));
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
                                    self.apply_motion(act, effective, Some(effective));
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
                    } else if let Some((Action::OperatorDelete, n0)) = self.op_pending.take() {
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
                                self.apply_motion(act, effective, Some(effective));
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
        if self.buf.rows.is_empty() {
            self.cy = 0;
            self.cx = 0;
            return;
        }
        let max = self.buf.rows.len();
        let target = n1.min(max).saturating_sub(1);
        self.cy = target;
        self.cx = 0;
        self.clamp_cursor();
    }

    fn apply_motion(&mut self, act: Action, count: usize, op_count: Option<usize>) {
        let n = count.max(1);
        match act {
            Action::MoveWordForward => {
                let mut target_c = self.cx;
                for _ in 0..n {
                    let next = self.buf.next_word_start(target_c, self.cy);
                    if next == target_c {
                        break;
                    }
                    target_c = next;
                }
                self.apply_range_or_move(target_c, false, op_count);
            }
            Action::MoveWordBackward => {
                let mut target_c = self.cx;
                for _ in 0..n {
                    let prev = self.buf.prev_word_start(target_c, self.cy);
                    if prev == target_c { break; }
                    target_c = prev;
                }
                self.apply_range_or_move(target_c, false, op_count);
            }
            Action::MoveEndWord => {
                let mut target_c = self.cx;
                for _ in 0..n {
                    let end = self.buf.end_of_word(target_c, self.cy);
                    if end == target_c { break; }
                    target_c = end;
                }
                self.apply_range_or_move(target_c, true, op_count);
            }
            Action::LineStart => {
                self.apply_range_or_move(0, false, op_count);
            }
            Action::LineEnd => {
                let end = self.buf.line_width(self.cy);
                self.apply_range_or_move(end, true, op_count);
            }
            _ => {
                // Fallback
                if op_count.is_none() {
                    self.apply_action_count(act, n);
                }
            }
        }
    }

    fn apply_range_or_move(&mut self, target_col: usize, inclusive: bool, op_count: Option<usize>) {
        if let Some(op_count) = op_count.or_else(|| self.op_pending.take().map(|(_, n)| n)) {
            // When operator pending, the motion count applies to the motion if present.
            let mut target = target_col;
            let mut last = self.cx;
            // Apply operator count by repeating deletion over repeated motions
            for _ in 0..op_count {
                let (start, end) = if target >= last {
                    (last, target)
                } else {
                    (target, last)
                };
                if end > start {
                    let start_b = self.buf.col_to_byte(self.cy, start);
                    let end_b = if inclusive {
                        // include the grapheme at target
                        let after = self.buf.next_col(target, self.cy);
                        self.buf.col_to_byte(self.cy, after)
                    } else {
                        self.buf.col_to_byte(self.cy, end)
                    };
                    if let Some(row) = self.buf.rows.get_mut(self.cy) {
                        if start_b <= row.len() && end_b <= row.len() && start_b < end_b {
                            row.replace_range(start_b..end_b, "");
                            self.cx = start;
                            self.dirty = true;
                        }
                    }
                }
                last = self.cx;
                target = target_col; // fixed target for repeated operator; simple approach
            }
        } else if op_count.is_none() {
            self.cx = target_col;
        }
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
            if !self.buf.rows.is_empty() {
                self.cy = self.buf.rows.len() - 1;
                self.cx = 0;
            }
            return true;
        }
        if (s.starts_with('+') || s.starts_with('-')) && s.len() > 1 {
            let sign = if s.starts_with('+') { 1isize } else { -1isize };
            if let Ok(n) = s[1..].parse::<isize>() {
                let base = self.cy as isize;
                let target = (base + sign * n).clamp(0, (self.buf.rows.len().saturating_sub(1)) as isize) as usize;
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
                if !self.buf.rows.is_empty() {
                    let max = self.buf.rows.len();
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
        for ch in "dw".chars() { let _ = ed.process_normal_char(ch); }
        assert_eq!(ed.buf.rows[0], "world");
        // reset
        ed.buf.rows[0] = "hello world".into(); ed.cx = 6; // before 'w'
        for ch in "d$".chars() { let _ = ed.process_normal_char(ch); }
        assert_eq!(ed.buf.rows[0], "hello ");
        // counts: 3w moves three words (here only two words so clamp)
        ed.buf.rows = vec!["one two three four".into()]; ed.cy = 0; ed.cx = 0;
        for ch in "3w".chars() { let _ = ed.process_normal_char(ch); }
        assert!(ed.cx > 0);
        // 2dw deletes two words from current position
        let start_text = ed.buf.rows[0].clone();
        for ch in "2dw".chars() { let _ = ed.process_normal_char(ch); }
        assert!(ed.buf.rows[0].len() < start_text.len());
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
