use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone)]
pub struct Buffer {
    rope: Rope,
}

impl Default for Buffer {
    fn default() -> Self {
        Self {
            rope: Rope::from_str(""),
        }
    }
}

impl Buffer {
    pub fn from_string(s: String) -> Self {
        Self {
            rope: Rope::from_str(&s.replace('\r', "")),
        }
    }

    #[allow(dead_code)]
    pub fn from_lines(lines: Vec<String>) -> Self {
        Self::from_string(lines.join("\n"))
    }

    #[allow(dead_code)]
    pub fn to_lines(&self) -> Vec<String> {
        self.to_string()
            .split('\n')
            .map(|s| s.to_string())
            .collect()
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn line_string(&self, y: usize) -> String {
        if y >= self.line_count() {
            return String::new();
        }
        let s = self.rope.line(y).to_string();
        // Be robust to whether rope includes trailing newline
        if let Some(stripped) = s.strip_suffix('\n') {
            stripped.to_string()
        } else {
            s
        }
    }

    pub fn line_width(&self, y: usize) -> usize {
        let s = self.line_string(y);
        UnicodeWidthStr::width(s.as_str())
    }

    fn line_start_char(&self, y: usize) -> usize {
        self.rope.line_to_char(y)
    }

    fn col_to_line_byte(&self, y: usize, col: usize) -> usize {
        let row = self.line_string(y);
        let mut acc = 0usize;
        let mut byte_idx = 0usize;
        for g in row.graphemes(true) {
            let w = UnicodeWidthStr::width(g).max(1);
            if acc + w > col {
                return byte_idx;
            }
            acc += w;
            byte_idx += g.len();
        }
        byte_idx.min(row.len())
    }

    #[allow(dead_code)]
    pub fn col_to_byte(&self, y: usize, col: usize) -> usize {
        self.col_to_line_byte(y, col)
    }

    fn col_to_char_index(&self, y: usize, col: usize) -> usize {
        let line = self.line_string(y);
        let byte = self.col_to_line_byte(y, col).min(line.len());
        let char_in_line = line[..byte].chars().count();
        self.line_start_char(y) + char_in_line
    }

    pub fn insert_char(&mut self, col: usize, y: usize, ch: char) {
        let idx = self.col_to_char_index(y, col);
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        self.rope.insert(idx, s);
    }

    pub fn insert_newline(&mut self, col: usize, y: usize) {
        let idx = self.col_to_char_index(y, col);
        self.rope.insert(idx, "\n");
    }

    pub fn delete_line(&mut self, y: usize) {
        if y >= self.line_count() {
            return;
        }
        let start = self.line_start_char(y);
        let end = if y + 1 < self.line_count() {
            self.line_start_char(y + 1)
        } else {
            self.rope.len_chars()
        };
        self.rope.remove(start..end);
    }

    pub fn delete_prev(&mut self, col: usize, y: usize) -> usize {
        let row = self.line_string(y);
        if row.is_empty() || col == 0 {
            return 0;
        }
        let mut acc = 0usize;
        let mut prev_acc = 0usize;
        let mut prev_b = 0usize;
        let mut cur_b = 0usize;
        for g in row.graphemes(true) {
            let w = UnicodeWidthStr::width(g).max(1);
            if acc >= col {
                break;
            }
            prev_acc = acc;
            prev_b = cur_b;
            cur_b += g.len();
            acc += w;
        }
        if prev_b < cur_b && cur_b <= row.len() {
            // Convert byte range within line to char indices in rope
            let start_chars = row[..prev_b].chars().count();
            let end_chars = row[..cur_b].chars().count();
            let start = self.line_start_char(y) + start_chars;
            let end = self.line_start_char(y) + end_chars;
            self.rope.remove(start..end);
            return prev_acc;
        }
        col.saturating_sub(1)
    }

    pub fn delete_at(&mut self, col: usize, y: usize) {
        let row = self.line_string(y);
        if row.is_empty() {
            return;
        }
        let mut acc = 0usize;
        let mut start_b = None::<usize>;
        let mut end_b = None::<usize>;
        let mut cur_b = 0usize;
        for g in row.graphemes(true) {
            let w = UnicodeWidthStr::width(g).max(1);
            let next = acc + w;
            if acc <= col && col < next {
                start_b = Some(cur_b);
                end_b = Some(cur_b + g.len());
                break;
            }
            acc = next;
            cur_b += g.len();
        }
        if let (Some(s), Some(e)) = (start_b, end_b) {
            let start_chars = row[..s].chars().count();
            let end_chars = row[..e].chars().count();
            let start = self.line_start_char(y) + start_chars;
            let end = self.line_start_char(y) + end_chars;
            self.rope.remove(start..end);
        }
    }

    pub fn merge_up(&mut self, y: usize) -> usize {
        if y == 0 || y >= self.line_count() {
            return 0;
        }
        let new_x = self.line_width(y - 1);
        let start = self.line_start_char(y);
        if start > 0 {
            // remove the newline just before this line
            self.rope.remove((start - 1)..start);
        }
        new_x
    }

    pub fn prev_col(&self, col: usize, y: usize) -> usize {
        let row = self.line_string(y);
        if col == 0 {
            return 0;
        }
        let mut acc = 0usize;
        let mut prev_acc = 0usize;
        for g in row.graphemes(true) {
            let w = UnicodeWidthStr::width(g).max(1);
            if acc >= col {
                break;
            }
            prev_acc = acc;
            acc += w;
        }
        prev_acc
    }

    pub fn next_col(&self, col: usize, y: usize) -> usize {
        let Some(row) = self.rows.get(y) else {
            return col;
        };
        let len = UnicodeWidthStr::width(row.as_str());
        if col >= len {
            return len;
        }
        let mut acc = 0usize;
        for g in row.graphemes(true) {
            let w = UnicodeWidthStr::width(g).max(1);
            if acc >= col {
                return (acc + w).min(len);
            }
            acc += w;
        }
        len
    }

    pub fn next_word_start(&self, col: usize, y: usize) -> usize {
        let Some(row) = self.rows.get(y) else {
            return col;
        };
        let bi = self.col_to_byte(y, col);
        let mut found = None::<usize>;
        let mut cur_end: Option<usize> = None;
        for (i, seg) in UnicodeSegmentation::split_word_bound_indices(row.as_str()) {
            let end = i + seg.len();
            let is_word = seg.chars().any(|c| c.is_alphanumeric() || c == '_');
            if cur_end.is_none() && i <= bi && bi < end {
                // We are in the current segment
                cur_end = Some(end);
                continue;
            }
            if i >= bi {
                if let Some(e) = cur_end {
                    if i >= e && is_word {
                        found = Some(i);
                        break;
                    }
                } else if is_word {
                    found = Some(i);
                    break;
                }
            }
        }
        let target_b = found.unwrap_or(row.len());
        // Recompute from start for simplicity
        let mut acc2 = 0usize;
        let mut bpos = 0usize;
        for g in row.graphemes(true) {
            if bpos >= target_b {
                break;
            }
            acc2 += UnicodeWidthStr::width(g).max(1);
            bpos += g.len();
        }
        acc2
    }

    pub fn prev_word_start(&self, col: usize, y: usize) -> usize {
        let Some(row) = self.rows.get(y) else {
            return col;
        };
        let bi = self.col_to_byte(y, col);
        let mut prev = None::<usize>;
        for (i, seg) in UnicodeSegmentation::split_word_bound_indices(row.as_str()) {
            if i >= bi {
                break;
            }
            if seg.chars().any(|c| c.is_alphanumeric() || c == '_') {
                prev = Some(i);
            }
        }
        let target_b = prev.unwrap_or(0);
        let mut acc2 = 0usize;
        let mut bpos = 0usize;
        for g in row.graphemes(true) {
            if bpos >= target_b {
                break;
            }
            acc2 += UnicodeWidthStr::width(g).max(1);
            bpos += g.len();
        }
        acc2
    }

    pub fn end_of_word(&self, col: usize, y: usize) -> usize {
        let Some(row) = self.rows.get(y) else {
            return col;
        };
        let bi = self.col_to_byte(y, col);
        let mut _cur_word_start = None::<usize>;
        let mut cur_word_end = None::<usize>;
        let mut after = None::<usize>;
        for (i, seg) in UnicodeSegmentation::split_word_bound_indices(row.as_str()) {
            let seg_is_word = seg.chars().any(|c| c.is_alphanumeric() || c == '_');
            let end = i + seg.len();
            if seg_is_word && i <= bi && bi < end {
                _cur_word_start = Some(i);
                cur_word_end = Some(end);
                break;
            }
            if i >= bi && seg_is_word {
                after = Some(end);
                break;
            }
        }
        let target_b = cur_word_end.or(after).unwrap_or(row.len());
        // Convert byte to col, but return the next column after end to be inclusive-friendly
        let mut acc2 = 0usize;
        let mut bpos = 0usize;
        for g in row.graphemes(true) {
            let next_b = bpos + g.len();
            let w = UnicodeWidthStr::width(g).max(1);
            if next_b > target_b {
                break;
            }
            acc2 += w;
            bpos = next_b;
        }
        acc2
    }
}

impl std::fmt::Display for Buffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.rows.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_width_ascii_and_emoji() {
        let b = Buffer {
            rows: vec!["abc".to_string(), "😄x".to_string()],
        };
        assert_eq!(b.line_width(0), 3);
        assert_eq!(b.line_width(1), 3);
    }

    #[test]
    fn insert_char_and_navigation_grapheme() {
        let mut b = Buffer {
            rows: vec!["a".to_string()],
        };
        b.insert_char(1, 0, '😄');
        assert_eq!(b.rows[0], "a😄");
        assert_eq!(b.next_col(0, 0), 1);
        assert_eq!(b.next_col(1, 0), 3);
        assert_eq!(b.prev_col(3, 0), 1);
    }

    #[test]
    fn delete_prev_removes_entire_grapheme() {
        let mut b = Buffer {
            rows: vec!["a😄b".to_string()],
        };
        let new_col = b.delete_prev(3, 0);
        assert_eq!(new_col, 1);
        assert_eq!(b.rows[0], "ab");
    }

    #[test]
    fn col_to_byte_with_multibyte_letters() {
        let b = Buffer {
            rows: vec!["żółw".to_string()],
        };
        let mut last = 0;
        for col in 0..=b.line_width(0) {
            let bi = b.col_to_byte(0, col);
            assert!(bi >= last);
            last = bi;
            assert!(bi <= b.rows[0].len());
        }
    }

    #[test]
    fn insert_newline_respects_grapheme_boundaries() {
        let mut b = Buffer {
            rows: vec!["foo😄bar".to_string()],
        };
        b.insert_newline(3, 0);
        assert_eq!(b.rows.len(), 2);
        assert_eq!(b.rows[0], "foo");
        assert_eq!(b.rows[1], "😄bar");

        b.insert_newline(1, 1);
        assert_eq!(b.rows.len(), 3);
        assert_eq!(b.rows[1], "");
        assert_eq!(b.rows[2], "😄bar");
    }

    #[test]
    fn merge_up_returns_display_width() {
        let mut b = Buffer {
            rows: vec!["żo".to_string(), "łw".to_string()],
        };
        let w = UnicodeWidthStr::width("żo");
        let got = b.merge_up(1);
        assert_eq!(got, w);
        assert_eq!(b.rows.len(), 1);
        assert_eq!(b.rows[0], "żołw");
    }

    #[test]
    fn delete_at_removes_grapheme_under_column() {
        let mut b = Buffer {
            rows: vec!["a😄b".to_string()],
        };
        // cursor at column 1 is on the emoji (width 2)
        b.delete_at(1, 0);
        assert_eq!(b.rows[0], "ab");
    }
}
