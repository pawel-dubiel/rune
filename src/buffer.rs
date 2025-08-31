use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
pub struct Buffer {
    pub rows: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_width_ascii_and_emoji() {
        let mut b = Buffer::default();
        b.rows = vec!["abc".to_string(), "😄x".to_string()];
        assert_eq!(b.line_width(0), 3);
        assert_eq!(b.line_width(1), 3);
    }

    #[test]
    fn insert_char_and_navigation_grapheme() {
        let mut b = Buffer::default();
        b.rows = vec!["a".to_string()];
        b.insert_char(1, 0, '😄');
        assert_eq!(b.rows[0], "a😄");
        assert_eq!(b.next_col(0, 0), 1);
        assert_eq!(b.next_col(1, 0), 3);
        assert_eq!(b.prev_col(3, 0), 1);
    }

    #[test]
    fn delete_prev_removes_entire_grapheme() {
        let mut b = Buffer::default();
        b.rows = vec!["a😄b".to_string()];
        let new_col = b.delete_prev(3, 0);
        assert_eq!(new_col, 1);
        assert_eq!(b.rows[0], "ab");
    }

    #[test]
    fn col_to_byte_with_multibyte_letters() {
        let mut b = Buffer::default();
        b.rows = vec!["żółw".to_string()];
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
        let mut b = Buffer::default();
        b.rows = vec!["foo😄bar".to_string()];
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
        let mut b = Buffer::default();
        b.rows = vec!["żo".to_string(), "łw".to_string()];
        let w = UnicodeWidthStr::width("żo");
        let got = b.merge_up(1);
        assert_eq!(got, w);
        assert_eq!(b.rows.len(), 1);
        assert_eq!(b.rows[0], "żołw");
    }
}

impl Buffer {
    pub fn from_string(s: String) -> Self {
        let mut rows: Vec<String> = s
            .replace('\r', "")
            .split('\n')
            .map(|l| l.to_string())
            .collect();
        if rows.is_empty() {
            rows.push(String::new());
        }
        Self { rows }
}

 

    pub fn to_string(&self) -> String {
        self.rows.join("\n")
    }

    pub fn line_width(&self, y: usize) -> usize {
        self.rows
            .get(y)
            .map(|s| UnicodeWidthStr::width(s.as_str()))
            .unwrap_or(0)
    }

    pub fn col_to_byte(&self, y: usize, col: usize) -> usize {
        let Some(row) = self.rows.get(y) else {
            return 0;
        };
        let mut acc = 0usize;
        let mut byte_idx = 0usize;
        for g in row.graphemes(true) {
            let w = UnicodeWidthStr::width(g);
            if acc >= col {
                break;
            }
            byte_idx += g.len();
            acc += w.max(1);
        }
        if acc > col {
            let mut acc2 = 0usize;
            let mut b2 = 0usize;
            for g in row.graphemes(true) {
                let w = UnicodeWidthStr::width(g).max(1);
                if acc2 + w > col {
                    break;
                }
                b2 += g.len();
                acc2 += w;
            }
            return b2;
        }
        byte_idx
    }

    pub fn insert_char(&mut self, col: usize, y: usize, ch: char) {
        let bi = {
            let len_row = self.rows.get(y).map(|r| r.len()).unwrap_or(0);
            let bi0 = self.col_to_byte(y, col);
            bi0.min(len_row)
        };
        if let Some(row) = self.rows.get_mut(y) {
            if bi <= row.len() {
                row.insert(bi, ch);
            }
        }
    }

    pub fn insert_newline(&mut self, col: usize, y: usize) {
        if y >= self.rows.len() {
            self.rows.push(String::new());
            return;
        }
        let split_at = self.col_to_byte(y, col);
        let rest = self.rows[y].split_off(split_at);
        self.rows.insert(y + 1, rest);
    }

    pub fn delete_prev(&mut self, col: usize, y: usize) -> usize {
        if let Some(row) = self.rows.get_mut(y) {
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
                row.replace_range(prev_b..cur_b, "");
                return prev_acc;
            }
        }
        col.saturating_sub(1)
    }

    pub fn delete_at(&mut self, x: usize, y: usize) {
        if let Some(row) = self.rows.get_mut(y) {
            if x < row.len() {
                row.remove(x);
            }
        }
    }

    pub fn merge_up(&mut self, y: usize) -> usize {
        if y > 0 && y < self.rows.len() {
            let cur = self.rows.remove(y);
            let new_x = UnicodeWidthStr::width(self.rows[y - 1].as_str());
            self.rows[y - 1].push_str(&cur);
            new_x
        } else {
            0
        }
    }

    pub fn prev_col(&self, col: usize, y: usize) -> usize {
        let Some(row) = self.rows.get(y) else {
            return 0;
        };
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
}
