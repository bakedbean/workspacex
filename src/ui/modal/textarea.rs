//! A minimal multi-line text box for modals: char-indexed lines, a cursor,
//! and a soft-wrapping renderer that keeps the cursor in view. No vim keys,
//! no selection — the prompt-tag body box is its only user.

use crate::ui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextArea {
    lines: Vec<String>,
    row: usize,
    /// Cursor column in CHARS (not bytes), so multi-byte input behaves.
    col: usize,
    /// The column ↑/↓ aim for; set by a vertical move, cleared by any other
    /// edit so a horizontal move re-anchors it.
    goal_col: Option<usize>,
}

impl Default for TextArea {
    fn default() -> Self {
        Self::new()
    }
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// Byte offset of char index `col` in `s` (or `s.len()` past the end).
fn byte_at(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map(|(i, _)| i).unwrap_or(s.len())
}

impl TextArea {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            row: 0,
            col: 0,
            goal_col: None,
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn is_blank(&self) -> bool {
        self.lines.iter().all(|l| l.trim().is_empty())
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    fn line(&self) -> &String {
        &self.lines[self.row]
    }

    pub fn insert_char(&mut self, c: char) {
        if c == '\n' {
            self.newline();
            return;
        }
        let at = byte_at(self.line(), self.col);
        self.lines[self.row].insert(at, c);
        self.col += 1;
        self.goal_col = None;
    }

    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            self.insert_char(c);
        }
    }

    pub fn newline(&mut self) {
        let at = byte_at(self.line(), self.col);
        let rest = self.lines[self.row].split_off(at);
        self.lines.insert(self.row + 1, rest);
        self.row += 1;
        self.col = 0;
        self.goal_col = None;
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            let at = byte_at(self.line(), self.col - 1);
            self.lines[self.row].remove(at);
            self.col -= 1;
        } else if self.row > 0 {
            let tail = self.lines.remove(self.row);
            self.row -= 1;
            self.col = char_len(self.line());
            self.lines[self.row].push_str(&tail);
        }
        self.goal_col = None;
    }

    pub fn delete(&mut self) {
        if self.col < char_len(self.line()) {
            let at = byte_at(self.line(), self.col);
            self.lines[self.row].remove(at);
        } else if self.row + 1 < self.lines.len() {
            let tail = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&tail);
        }
        self.goal_col = None;
    }

    pub fn move_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = char_len(self.line());
        }
        self.goal_col = None;
    }

    pub fn move_right(&mut self) {
        if self.col < char_len(self.line()) {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
        self.goal_col = None;
    }

    fn move_vertical(&mut self, target_row: usize) {
        let goal = self.goal_col.unwrap_or(self.col);
        self.row = target_row;
        self.col = goal.min(char_len(self.line()));
        self.goal_col = Some(goal);
    }

    pub fn move_up(&mut self) {
        if self.row > 0 {
            self.move_vertical(self.row - 1);
        }
    }

    pub fn move_down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.move_vertical(self.row + 1);
        }
    }

    pub fn home(&mut self) {
        self.col = 0;
        self.goal_col = None;
    }

    pub fn end(&mut self) {
        self.col = char_len(self.line());
        self.goal_col = None;
    }

    /// Soft-wrap every line at `width` chars. Returns the visual rows and
    /// the cursor's `(visual row, visual col)`. A cursor exactly at a wrap
    /// boundary sits at the start of the next visual row, so typing at the
    /// end of a full row continues on the row below.
    ///
    /// A tab is one char for editing (see `insert_char`/`byte_at`/etc, which
    /// stay char-indexed on the raw text) but is expanded here to 4 display
    /// columns, since a raw tab char renders as a single (usually blank)
    /// terminal cell. `self.col` (a raw char index) is mapped to its
    /// expanded display column before chunking into rows.
    fn wrap_rows(&self, width: usize) -> (Vec<String>, (usize, usize)) {
        const TAB_COLS: usize = 4;
        let width = width.max(1);
        let mut rows = Vec::new();
        let mut cursor = (0, 0);
        for (i, line) in self.lines.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            let mut disp: Vec<char> = Vec::with_capacity(chars.len());
            let mut cursor_disp_col = None;
            for (ci, &c) in chars.iter().enumerate() {
                if i == self.row && ci == self.col {
                    cursor_disp_col = Some(disp.len());
                }
                if c == '\t' {
                    disp.extend(std::iter::repeat_n(' ', TAB_COLS));
                } else {
                    disp.push(c);
                }
            }
            if i == self.row && self.col == chars.len() {
                cursor_disp_col = Some(disp.len());
            }
            let first_row = rows.len();
            if disp.is_empty() {
                rows.push(String::new());
            } else {
                for chunk in disp.chunks(width) {
                    rows.push(chunk.iter().collect());
                }
                // A line that fills its last row exactly gets an empty row
                // after it so the cursor at `col == len` has somewhere to
                // sit. Added for every such line, not only the cursor's, so
                // the layout depends on the text alone and never shifts as
                // the cursor moves.
                if disp.len() % width == 0 {
                    rows.push(String::new());
                }
            }
            if i == self.row {
                let dcol = cursor_disp_col.unwrap_or(0);
                cursor = (first_row + dcol / width, dcol % width);
            }
        }
        (rows, cursor)
    }

    /// Draw the box into `area` and place the terminal cursor. Scrolls so
    /// the cursor row is always visible, keeping it on the last row once
    /// the text is taller than the area.
    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let (rows, (crow, ccol)) = self.wrap_rows(area.width as usize);
        let height = area.height as usize;
        let top = crow.saturating_sub(height - 1);
        let visible: Vec<Line> = rows
            .iter()
            .skip(top)
            .take(height)
            .map(|r| Line::from(Span::raw(r.clone())))
            .collect();
        f.render_widget(
            Paragraph::new(visible).style(Style::default().fg(theme.path)),
            area,
        );
        f.set_cursor_position((area.x + ccol as u16, area.y + (crow - top) as u16));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(text: &str) -> TextArea {
        let mut t = TextArea::new();
        t.insert_str(text);
        t
    }

    #[test]
    fn starts_empty_and_blank() {
        let t = TextArea::new();
        assert_eq!(t.text(), "");
        assert!(t.is_blank());
        assert_eq!(t.cursor(), (0, 0));
    }

    #[test]
    fn insert_str_splits_on_newlines_and_leaves_the_cursor_at_the_end() {
        let t = with("ab\ncd");
        assert_eq!(t.text(), "ab\ncd");
        assert_eq!(t.cursor(), (1, 2));
        assert!(!t.is_blank());
        assert!(with("  \n\t").is_blank());
    }

    #[test]
    fn newline_splits_the_current_line_at_the_cursor() {
        let mut t = with("abcd");
        t.move_left();
        t.move_left();
        t.newline();
        assert_eq!(t.text(), "ab\ncd");
        assert_eq!(t.cursor(), (1, 0));
    }

    #[test]
    fn backspace_joins_lines_at_a_line_start_and_is_a_noop_at_the_origin() {
        let mut t = with("ab\ncd");
        t.home();
        t.backspace();
        assert_eq!(t.text(), "abcd");
        assert_eq!(t.cursor(), (0, 2));
        let mut t = with("x");
        t.home();
        t.backspace();
        assert_eq!(t.text(), "x");
    }

    #[test]
    fn delete_removes_under_the_cursor_and_joins_at_a_line_end() {
        let mut t = with("ab\ncd");
        t.move_up();
        t.end();
        t.delete();
        assert_eq!(t.text(), "abcd");
        t.home();
        t.delete();
        assert_eq!(t.text(), "bcd");
    }

    #[test]
    fn up_and_down_remember_the_goal_column_across_short_lines() {
        let mut t = with("abcdef\nx\nabcdef");
        // cursor at (2, 6)
        t.move_up();
        assert_eq!(t.cursor(), (1, 1));
        t.move_up();
        assert_eq!(t.cursor(), (0, 6), "goal column survives the short line");
        t.move_down();
        t.move_down();
        assert_eq!(t.cursor(), (2, 6));
        t.move_up();
        t.move_left();
        t.move_down();
        assert_eq!(
            t.cursor(),
            (2, 0),
            "a horizontal move resets the goal to the current column"
        );
    }

    #[test]
    fn horizontal_moves_wrap_across_line_boundaries() {
        let mut t = with("ab\ncd");
        t.home();
        t.move_left();
        assert_eq!(t.cursor(), (0, 2));
        t.move_right();
        assert_eq!(t.cursor(), (1, 0));
    }

    #[test]
    fn multibyte_chars_count_as_one_column() {
        let mut t = with("héllo");
        assert_eq!(t.cursor(), (0, 5));
        t.move_left();
        t.move_left();
        t.move_left();
        t.move_left();
        t.delete();
        assert_eq!(t.text(), "hllo");
    }

    #[test]
    fn wrap_rows_soft_wrap_at_width_and_locate_the_cursor() {
        let t = with("abcdefgh\nij");
        // width 3: "abc" "def" "gh" | "ij"
        let (rows, (crow, ccol)) = t.wrap_rows(3);
        assert_eq!(rows, vec!["abc", "def", "gh", "ij"]);
        assert_eq!((crow, ccol), (3, 2));
        let mut t = with("abcdef");
        t.home();
        t.move_right();
        t.move_right();
        t.move_right();
        // col 3 on a width-3 line is the START of the second visual row
        assert_eq!(t.wrap_rows(3).1, (1, 0));
    }

    #[test]
    fn a_tab_expands_to_four_display_columns() {
        let mut t = with("a\tb");
        assert_eq!(t.wrap_rows(10).0, vec!["a    b"]);
        assert_eq!(t.wrap_rows(10).1, (0, 6));
        t.move_left();
        t.move_left();
        assert_eq!(
            t.wrap_rows(10).1,
            (0, 1),
            "raw col 1 (just before the tab) maps to expanded col 1"
        );
    }

    #[test]
    fn a_line_that_exactly_fills_its_rows_keeps_a_trailing_empty_row_wherever_the_cursor_is() {
        // Without this, the empty row would appear only while the cursor sat
        // on it, shifting every line below as the cursor moved.
        let mut t = with("abc\nxyz");
        assert_eq!(t.wrap_rows(3).0, vec!["abc", "", "xyz", ""]);
        t.move_up();
        assert_eq!(t.cursor(), (0, 3));
        assert_eq!(
            t.wrap_rows(3),
            (
                vec![
                    "abc".to_string(),
                    String::new(),
                    "xyz".to_string(),
                    String::new()
                ],
                (1, 0)
            )
        );
        assert_eq!(with("ab\nxyz").wrap_rows(3).0, vec!["ab", "xyz", ""]);
    }
}
