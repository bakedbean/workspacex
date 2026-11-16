//! The side panel's diff viewer: one changed file full screen, its diff on
//! the left and a minimap of the whole thing on the right.
//!
//! ```text
//! ╭ src/ui/dashboard/tall.rs  3/7  +412 −63 ──────────╮╭ minimap ────────╮
//! │  12  use crate::ui::text;                         ││⣿⣿⣿⡇  ⣤⣤⣤       │
//! │      -use crate::ui::old;                         ││⣿⣿⣿⣿⣿⣿⣷        │
//! │  13 +use crate::ui::new;                          ││⠿⠿⠿⠇           │
//! ╰───────────────────────────────────────────────────╯╰─────────────────╯
//!  esc close  ↑↓ scroll  [ ] file  m minimap
//! ```
//!
//! When HEAD still has the file, the diff pane shows all of it with the
//! branch's deletions and additions in place; otherwise it shows the diff's
//! hunks. The minimap draws every diff line as braille dots, four lines per
//! row, colors the changes and shades the window the diff pane shows.
//! Wheel over either pane scrolls; a click or drag on the minimap jumps.

use super::*;
use crate::data::store::WorkspaceId;
use crate::git::{ChangedFile, FileDiff};
use ratatui::widgets::BorderType;
use std::path::PathBuf;
use std::sync::Arc;
use unicode_width::UnicodeWidthChar;

/// What one diff line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Hunk,
    Add,
    Del,
    Ctx,
    /// A message in place of a diff ("binary file").
    Note,
}

/// One line of the diff pane. `text` keeps the diff's `+`/`-`/space prefix,
/// except on notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    /// The line's number in HEAD's copy of the file; `None` for deletions,
    /// hunk headers and notes.
    pub n: Option<usize>,
    pub kind: LineKind,
    pub text: String,
}

/// A loaded file's diff lines, with the figures every frame needs worked
/// out once.
#[derive(Debug)]
pub struct LoadedDiff {
    pub lines: Vec<DiffLine>,
    /// The widest line in columns, which sets the minimap's scale.
    widest: usize,
    /// The largest line number, which sets the gutter's width.
    max_n: usize,
}

impl LoadedDiff {
    pub fn new(lines: Vec<DiffLine>) -> Self {
        let widest = lines.iter().map(|l| text_width(&l.text)).max().unwrap_or(0);
        let max_n = lines.iter().filter_map(|l| l.n).max().unwrap_or(0);
        Self {
            lines,
            widest,
            max_n,
        }
    }
}

fn text_width(text: &str) -> usize {
    text.chars()
        .map(|c| if c == '\t' { 4 } else { c.width().unwrap_or(0) })
        .sum()
}

/// Where the file's diff is.
#[derive(Debug, Clone)]
pub enum DiffLoad {
    Loading,
    Ready(Arc<LoadedDiff>),
    Failed(String),
}

impl DiffLoad {
    pub fn ready(lines: Vec<DiffLine>) -> Self {
        Self::Ready(Arc::new(LoadedDiff::new(lines)))
    }
}

/// A fresh id for a load, so a result only lands on the request that asked
/// for it.
fn next_load_gen() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// The minimap's dots and colors for one load at one size; only the
/// visible-window shading changes from frame to frame.
#[derive(Debug, Clone)]
struct MinimapRaster {
    key: (u16, u16, u64),
    rows: Arc<Vec<(Color, String)>>,
}

/// The open viewer.
#[derive(Debug, Clone)]
pub struct FileDiffModal {
    pub workspace_id: WorkspaceId,
    pub worktree: PathBuf,
    /// The base the side panel listed the files against.
    pub base: String,
    /// The side panel's files when the viewer opened; `[` / `]` step
    /// through them.
    pub files: Arc<Vec<ChangedFile>>,
    pub index: usize,
    pub load: DiffLoad,
    /// The load in flight or shown; a result for any other is dropped.
    pub load_gen: u64,
    minimap_cache: Option<MinimapRaster>,
    /// The diff pane's first line, clamped on draw.
    pub scroll: usize,
    pub minimap: bool,
    /// Diff lines the last frame showed, which a page moves by.
    pub view_h: usize,
    /// A message for the hint row (a failed `o`), cleared by the next key.
    pub notice: Option<String>,
    /// The diff and minimap panes' insides as last drawn, for the mouse.
    pub diff_rect: Rect,
    pub minimap_rect: Rect,
}

impl FileDiffModal {
    pub fn new(
        workspace_id: WorkspaceId,
        worktree: PathBuf,
        base: String,
        files: Arc<Vec<ChangedFile>>,
        index: usize,
    ) -> Self {
        Self {
            workspace_id,
            worktree,
            base,
            index: index.min(files.len().saturating_sub(1)),
            files,
            load: DiffLoad::Loading,
            load_gen: next_load_gen(),
            minimap_cache: None,
            scroll: 0,
            minimap: true,
            view_h: 0,
            notice: None,
            diff_rect: Rect::default(),
            minimap_rect: Rect::default(),
        }
    }

    pub fn file(&self) -> Option<&ChangedFile> {
        self.files.get(self.index)
    }

    /// The line of HEAD's copy at the top of the diff pane (the first
    /// numbered line at or below it), where `o` opens the editor.
    pub fn top_line(&self) -> usize {
        let lines = self.lines();
        lines[self.scroll.min(lines.len())..]
            .iter()
            .find_map(|l| l.n)
            .unwrap_or(1)
    }

    pub fn lines(&self) -> &[DiffLine] {
        match &self.load {
            DiffLoad::Ready(l) => &l.lines,
            _ => &[],
        }
    }

    fn max_scroll(&self) -> usize {
        self.lines().len().saturating_sub(self.view_h.max(1))
    }

    pub fn scroll_by(&mut self, d: isize) {
        self.scroll = self.scroll.saturating_add_signed(d).min(self.max_scroll());
    }

    /// Move by a page, less two lines of overlap to keep the reader's place.
    pub fn page(&mut self, down: bool) {
        let n = self.view_h.saturating_sub(2).max(1) as isize;
        self.scroll_by(if down { n } else { -n });
    }

    pub fn to_top(&mut self) {
        self.scroll = 0;
    }

    pub fn to_bottom(&mut self) {
        self.scroll = self.max_scroll();
    }

    /// Show the file `d` steps along, clamped to the list. Returns whether
    /// the file changed, in which case its diff needs loading.
    pub fn step(&mut self, d: isize) -> bool {
        let last = self.files.len().saturating_sub(1);
        let next = self.index.saturating_add_signed(d).min(last);
        if next == self.index {
            return false;
        }
        self.index = next;
        self.load = DiffLoad::Loading;
        self.load_gen = next_load_gen();
        self.scroll = 0;
        true
    }

    /// Scroll so the diff line under minimap row `row` (screen coordinates)
    /// sits mid-pane. Rows outside the minimap are ignored.
    pub fn jump_to_minimap(&mut self, col: u16, row: u16) {
        let m = self.minimap_rect;
        if !self.minimap
            || m.width == 0
            || col < m.x
            || col >= m.right()
            || row < m.y
            || row >= m.bottom()
        {
            return;
        }
        let line = minimap_line_at(self.lines().len(), row - m.y, m.height);
        self.scroll = line.saturating_sub(self.view_h / 2).min(self.max_scroll());
    }
}

/// The diff pane's lines for a loaded file.
pub fn diff_lines(d: &FileDiff) -> Vec<DiffLine> {
    let note = |t: &str| DiffLine {
        n: None,
        kind: LineKind::Note,
        text: t.to_string(),
    };
    if d.binary {
        return vec![note("Binary file; no diff.")];
    }
    if d.diff.lines().all(|l| !l.starts_with("@@ ")) {
        return vec![note("No content change (a rename or mode change).")];
    }
    match &d.source {
        Some(src) => whole_file(&d.diff, src),
        None => numbered(&d.diff),
    }
}

/// `+start,count` of a hunk header's text after `@@ `; count defaults to 1
/// as in git.
fn hunk_new(rest: &str) -> (usize, usize) {
    let Some(t) = rest.split_whitespace().find_map(|t| t.strip_prefix('+')) else {
        return (0, 0);
    };
    let mut it = t.split(',');
    let start = it.next().and_then(|t| t.parse().ok()).unwrap_or(0);
    (start, it.next().and_then(|t| t.parse().ok()).unwrap_or(1))
}

/// Whether `l` is a line inside a hunk rather than the diff's file header.
fn in_hunk(l: &str, seen_hunk: bool) -> bool {
    seen_hunk && !l.starts_with("diff --git ")
}

/// The diff's hunks, context and added lines numbered by HEAD's copy.
fn numbered(diff: &str) -> Vec<DiffLine> {
    let mut out = vec![];
    let mut n = 0usize;
    let mut seen = false;
    for l in diff.lines() {
        if let Some(rest) = l.strip_prefix("@@ ") {
            seen = true;
            n = hunk_new(rest).0;
            out.push(DiffLine {
                n: None,
                kind: LineKind::Hunk,
                text: l.into(),
            });
        } else if !in_hunk(l, seen) || l.starts_with('\\') {
        } else if l.starts_with('-') {
            out.push(DiffLine {
                n: None,
                kind: LineKind::Del,
                text: l.into(),
            });
        } else {
            let kind = if l.starts_with('+') {
                LineKind::Add
            } else {
                LineKind::Ctx
            };
            out.push(DiffLine {
                n: Some(n),
                kind,
                text: l.into(),
            });
            n += 1;
        }
    }
    out
}

/// HEAD's whole file with the diff's deletions and additions in place; hunk
/// headers are dropped.
fn whole_file(diff: &str, src: &str) -> Vec<DiffLine> {
    let src: Vec<&str> = src.lines().collect();
    let mut out = vec![];
    let mut n = 1usize;
    let ctx_to = |out: &mut Vec<DiffLine>, n: &mut usize, end: usize| {
        while *n < end && *n <= src.len() {
            out.push(DiffLine {
                n: Some(*n),
                kind: LineKind::Ctx,
                text: format!(" {}", src[*n - 1]),
            });
            *n += 1;
        }
    };
    let mut seen = false;
    for l in diff.lines() {
        if let Some(rest) = l.strip_prefix("@@ ") {
            seen = true;
            // An empty `+start,0` side sits after line `start`.
            let first = match hunk_new(rest) {
                (start, 0) => start + 1,
                (start, _) => start,
            };
            ctx_to(&mut out, &mut n, first);
        } else if !in_hunk(l, seen) || l.starts_with('\\') {
        } else if l.starts_with('-') {
            out.push(DiffLine {
                n: None,
                kind: LineKind::Del,
                text: l.into(),
            });
        } else {
            let kind = if l.starts_with('+') {
                LineKind::Add
            } else {
                LineKind::Ctx
            };
            out.push(DiffLine {
                n: Some(n),
                kind,
                text: l.into(),
            });
            n += 1;
        }
    }
    ctx_to(&mut out, &mut n, usize::MAX);
    out
}

/// Columns the minimap takes, borders included.
const MINIMAP_WIDTH: u16 = 20;
/// Narrowest the diff pane gets before the minimap gives way.
const MIN_DIFF_WIDTH: u16 = 40;
/// Most source columns one braille dot stands for; longer lines are clipped.
const MAX_SCALE: usize = 4;
/// Braille dot bits by `[dot row][dot column]`, offsets from U+2800.
const DOTS: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

const HINTS: &[Hint] = &[
    ("esc", "close"),
    ("↑↓", "scroll"),
    ("J K", "page"),
    ("g G", "top/bottom"),
    ("[ ]", "file"),
    ("o", "open in editor"),
    ("m", "minimap"),
];

/// Draw the viewer over all of `area`, clamping its scroll and recording
/// its panes for the mouse.
pub fn render_file_diff(f: &mut Frame, area: Rect, m: &mut FileDiffModal, theme: &Theme) {
    f.render_widget(Clear, area);
    f.render_widget(
        Block::default().style(Style::default().bg(theme.bg_alt)),
        area,
    );
    if area.height < 4 || area.width < 20 {
        m.diff_rect = Rect::default();
        m.minimap_rect = Rect::default();
        return;
    }
    let body = Rect {
        height: area.height - 1,
        ..area
    };
    let hint_row = Rect {
        y: area.bottom() - 1,
        height: 1,
        x: area.x + 1,
        width: area.width - 1,
    };
    match &m.notice {
        Some(n) => f.render_widget(Span::styled(n.clone(), theme.err_style()), hint_row),
        None => f.render_widget(crate::ui::drawer::hint_line(HINTS, theme), hint_row),
    }

    let mm_w = if m.minimap && body.width >= MIN_DIFF_WIDTH + MINIMAP_WIDTH {
        MINIMAP_WIDTH
    } else {
        0
    };
    let diff_area = Rect {
        width: body.width - mm_w,
        ..body
    };
    let block = pane_block(diff_title(m, theme), theme);
    let inner = block.inner(diff_area);
    f.render_widget(block, diff_area);
    m.view_h = inner.height as usize;
    m.scroll = m.scroll.min(m.max_scroll());
    m.diff_rect = inner;
    draw_diff(f, inner, m, theme);

    m.minimap_rect = Rect::default();
    if mm_w > 0 {
        let mm_area = Rect {
            x: diff_area.right(),
            width: mm_w,
            ..body
        };
        let block = pane_block(Line::from(" minimap "), theme);
        let inner = block.inner(mm_area);
        f.render_widget(block, mm_area);
        m.minimap_rect = inner;
        draw_minimap(f, inner, m, theme);
    }
}

fn pane_block(title: Line<'static>, theme: &Theme) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.dim_style())
        .title(title)
}

/// ` path  3/7  +12 −4 `.
fn diff_title(m: &FileDiffModal, theme: &Theme) -> Line<'static> {
    let Some(file) = m.file() else {
        return Line::from(" no files ");
    };
    let mut spans = vec![
        Span::styled(format!(" {} ", file.display_path()), theme.header_style()),
        Span::styled(
            format!(" {}/{} ", m.index + 1, m.files.len()),
            theme.dim_style(),
        ),
    ];
    if let Some(s) = file.stats {
        spans.push(Span::styled(format!(" +{}", s.added), theme.ok_style()));
        spans.push(Span::styled(format!(" −{} ", s.removed), theme.err_style()));
    }
    Line::from(spans)
}

fn draw_diff(f: &mut Frame, inner: Rect, m: &FileDiffModal, theme: &Theme) {
    let lines = match &m.load {
        DiffLoad::Loading => {
            f.render_widget(
                Paragraph::new(Span::styled("loading…", theme.dim_style())),
                inner,
            );
            return;
        }
        DiffLoad::Failed(e) => {
            f.render_widget(
                Paragraph::new(Span::styled(e.clone(), theme.err_style()))
                    .wrap(ratatui::widgets::Wrap { trim: false }),
                inner,
            );
            return;
        }
        DiffLoad::Ready(l) => l,
    };
    let digits = lines.max_n.to_string().len().max(3);
    let lines = &lines.lines;
    let blank = " ".repeat(digits + 1);
    let rows: Vec<Line> = lines
        .iter()
        .skip(m.scroll)
        .take(inner.height as usize)
        .map(|l| {
            let fg = match l.kind {
                LineKind::Add => Style::default().fg(theme.ok),
                LineKind::Del => Style::default().fg(theme.err),
                LineKind::Hunk | LineKind::Note => theme.dim_style(),
                LineKind::Ctx => Style::default(),
            };
            let gutter = match (l.kind, l.n) {
                (LineKind::Note, _) => String::new(),
                (_, Some(n)) => format!("{n:>digits$} "),
                (_, None) => blank.clone(),
            };
            Line::from(vec![
                Span::styled(gutter, theme.dim_style()),
                Span::styled(l.text.replace('\t', "    "), fg),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(rows), inner);
}

/// Diff lines one dot row stands for when `total` lines share `rows` rows.
fn per_slot(total: usize, rows: u16) -> usize {
    total.div_ceil(4 * rows.max(1) as usize).max(1)
}

/// The diff line under minimap row `row` of a minimap `rows` tall.
pub fn minimap_line_at(total: usize, row: u16, rows: u16) -> usize {
    (4 * row as usize * per_slot(total, rows)).min(total.saturating_sub(1))
}

/// What a run of diff lines looks like: its strongest kind and which dot
/// columns hold text.
struct Slot {
    rank: u8,
    color: Color,
    dots: Vec<bool>,
}

fn slot(
    lines: &[DiffLine],
    range: std::ops::Range<usize>,
    scale: usize,
    cols: usize,
    theme: &Theme,
) -> Slot {
    let mut s = Slot {
        rank: 0,
        color: Color::Reset,
        dots: vec![false; cols],
    };
    let end = range.end.min(lines.len());
    for l in &lines[range.start.min(end)..end] {
        let (r, c) = match l.kind {
            LineKind::Add => (3, theme.ok),
            LineKind::Del => (2, theme.err),
            LineKind::Ctx | LineKind::Hunk | LineKind::Note => (1, theme.dim),
        };
        // Drop the diff's +/-/space prefix; notes have none.
        let text = if l.kind == LineKind::Note {
            l.text.as_str()
        } else {
            l.text.get(1..).unwrap_or("")
        };
        let mut col = 0;
        let mut any = false;
        for ch in text.chars() {
            let w = if ch == '\t' {
                4
            } else {
                ch.width().unwrap_or(0)
            };
            // Past the last dot column nothing more can show.
            if col / scale >= cols {
                break;
            }
            if !ch.is_whitespace() {
                for d in (col..col + w).map(|c| c / scale).take_while(|&d| d < cols) {
                    (s.dots[d], any) = (true, true);
                }
            }
            col += w;
        }
        if any && r > s.rank {
            (s.rank, s.color) = (r, c);
        }
    }
    s
}

/// The minimap's rows for `loaded` at `w`×`h`: each row's color and its
/// `w` braille cells.
fn raster(loaded: &LoadedDiff, w: usize, h: u16, theme: &Theme) -> Vec<(Color, String)> {
    let lines = &loaded.lines;
    let per = per_slot(lines.len(), h);
    let scale = loaded.widest.div_ceil(2 * w).clamp(1, MAX_SCALE);
    let mut rows = Vec::new();
    for row in 0..h as usize {
        let first = 4 * row * per;
        if first >= lines.len() {
            break;
        }
        let slots: Vec<Slot> = (0..4)
            .map(|d| first + d * per)
            .map(|s| slot(lines, s..s + per, scale, 2 * w, theme))
            .collect();
        let fg = slots
            .iter()
            .max_by_key(|s| s.rank)
            .map_or(Color::Reset, |s| s.color);
        let cells = (0..w)
            .map(|x| {
                let bits: u32 = (0..4)
                    .flat_map(|d| (0..2).map(move |c| (d, c)))
                    .filter(|&(d, c)| slots[d].dots[2 * x + c])
                    .map(|(d, c)| DOTS[d][c])
                    .sum();
                if bits == 0 {
                    ' '
                } else {
                    char::from_u32(0x2800 + bits).unwrap_or(' ')
                }
            })
            .collect();
        rows.push((fg, cells));
    }
    rows
}

fn draw_minimap(f: &mut Frame, inner: Rect, m: &mut FileDiffModal, theme: &Theme) {
    let DiffLoad::Ready(loaded) = &m.load else {
        return;
    };
    let (w, h) = (inner.width, inner.height);
    if w == 0 || h == 0 || loaded.lines.is_empty() {
        return;
    }
    let key = (w, h, m.load_gen);
    let rows = match &m.minimap_cache {
        Some(c) if c.key == key => Arc::clone(&c.rows),
        _ => {
            let rows = Arc::new(raster(loaded, w as usize, h, theme));
            m.minimap_cache = Some(MinimapRaster {
                key,
                rows: Arc::clone(&rows),
            });
            rows
        }
    };
    let per = per_slot(loaded.lines.len(), h);
    let (top, bottom) = (m.scroll, m.scroll + m.view_h.max(1));
    let buf = f.buffer_mut();
    for (row, (fg, cells)) in rows.iter().enumerate() {
        let first = 4 * row * per;
        let bg = if first < bottom && first + 4 * per > top {
            theme.selected_bg
        } else {
            theme.bg_alt
        };
        let style = Style::default().fg(*fg).bg(bg);
        for (x, sym) in cells.chars().enumerate() {
            buf[(inner.x + x as u16, inner.y + row as u16)]
                .set_char(sym)
                .set_style(style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::DiffStats;

    const DIFF: &str = concat!(
        "diff --git a/a.rs b/a.rs\n",
        "index 1..2 100644\n",
        "--- a/a.rs\n",
        "+++ b/a.rs\n",
        "@@ -1,4 +1,4 @@\n",
        " one\n",
        "-two\n",
        "+TWO\n",
        " three\n",
        " four\n",
    );

    fn modal(files: usize) -> FileDiffModal {
        let files = (0..files)
            .map(|i| ChangedFile {
                path: format!("src/f{i}.rs"),
                old_path: None,
                stats: Some(DiffStats {
                    added: 1,
                    removed: 1,
                }),
            })
            .collect();
        FileDiffModal::new(
            WorkspaceId(1),
            PathBuf::from("/tmp"),
            "main".into(),
            Arc::new(files),
            0,
        )
    }

    fn draw(m: &mut FileDiffModal, w: u16, h: u16) -> Vec<String> {
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        term.draw(|f| render_file_diff(f, f.area(), m, &Theme::wsx()))
            .unwrap();
        let buf = term.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn whole_file_puts_changes_in_place() {
        let lines = diff_lines(&FileDiff {
            diff: DIFF.into(),
            source: Some("one\nTWO\nthree\nfour\nfive\n".into()),
            binary: false,
        });
        let got: Vec<_> = lines
            .iter()
            .map(|l| (l.n, l.kind, l.text.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (Some(1), LineKind::Ctx, " one"),
                (None, LineKind::Del, "-two"),
                (Some(2), LineKind::Add, "+TWO"),
                (Some(3), LineKind::Ctx, " three"),
                (Some(4), LineKind::Ctx, " four"),
                (Some(5), LineKind::Ctx, " five"),
            ]
        );
    }

    #[test]
    fn without_head_copy_the_hunks_show() {
        let lines = diff_lines(&FileDiff {
            diff: DIFF.into(),
            source: None,
            binary: false,
        });
        assert_eq!(lines[0].kind, LineKind::Hunk);
        assert!(lines.iter().all(|l| !l.text.starts_with("+++")));
        assert_eq!(lines.len(), 6);
    }

    #[test]
    fn binary_and_empty_diffs_get_a_note() {
        let b = diff_lines(&FileDiff {
            binary: true,
            ..Default::default()
        });
        assert_eq!(b[0].kind, LineKind::Note);
        let e = diff_lines(&FileDiff::default());
        assert!(e[0].text.contains("No content change"));
    }

    #[test]
    fn renders_diff_title_minimap_and_hints() {
        let mut m = modal(3);
        m.load = DiffLoad::ready(diff_lines(&FileDiff {
            diff: DIFF.into(),
            source: Some("one\nTWO\nthree\nfour\n".into()),
            binary: false,
        }));
        let lines = draw(&mut m, 100, 12);
        let all = lines.join("\n");
        assert!(
            lines[0].contains("src/f0.rs") && lines[0].contains("1/3"),
            "{all}"
        );
        assert!(lines[0].contains("minimap"), "{all}");
        assert!(all.contains("  2 +TWO"), "{all}");
        assert!(all.contains("-two"), "{all}");
        assert!(
            lines[11].contains("esc") && lines[11].contains("close"),
            "{all}"
        );
        assert!(
            all.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)),
            "braille: {all}"
        );
        assert_eq!(m.view_h, 9);
        assert!(m.minimap_rect.width > 0);
    }

    #[test]
    fn narrow_screens_drop_the_minimap() {
        let mut m = modal(1);
        draw(&mut m, 50, 10);
        assert_eq!(m.minimap_rect, Rect::default());
        assert!(m.diff_rect.width > 0);
    }

    #[test]
    fn scroll_clamps_to_the_last_page() {
        let mut m = modal(1);
        let many = (1..=100)
            .map(|n| DiffLine {
                n: Some(n),
                kind: LineKind::Ctx,
                text: format!(" line {n}"),
            })
            .collect();
        m.load = DiffLoad::ready(many);
        draw(&mut m, 100, 22);
        m.scroll_by(1000);
        assert_eq!(m.scroll, 100 - m.view_h);
        m.page(false);
        assert_eq!(m.scroll, 100 - m.view_h - (m.view_h - 2));
        m.to_top();
        m.page(true);
        assert_eq!(m.scroll, m.view_h - 2);
    }

    #[test]
    fn minimap_click_centers_the_line() {
        let mut m = modal(1);
        let many = (1..=400)
            .map(|n| DiffLine {
                n: Some(n),
                kind: LineKind::Ctx,
                text: format!(" line {n}"),
            })
            .collect();
        m.load = DiffLoad::ready(many);
        draw(&mut m, 100, 22);
        let r = m.minimap_rect;
        m.jump_to_minimap(r.x, r.y + r.height / 2);
        let line = minimap_line_at(400, r.height / 2, r.height);
        assert_eq!(m.scroll, line - m.view_h / 2);
        // Off the minimap: nothing moves.
        let before = m.scroll;
        m.jump_to_minimap(0, 0);
        assert_eq!(m.scroll, before);
    }

    #[test]
    fn top_line_is_the_first_numbered_line_on_screen() {
        let mut m = modal(1);
        assert_eq!(m.top_line(), 1, "nothing loaded");
        m.load = DiffLoad::ready(diff_lines(&FileDiff {
            diff: DIFF.into(),
            source: Some("one\nTWO\nthree\nfour\n".into()),
            binary: false,
        }));
        m.scroll = 1; // the deleted `-two`, which has no number
        assert_eq!(m.top_line(), 2);
    }

    /// Each load is its own request: stepping away and back, or a new
    /// viewer on the same file, never reuses an id an older load holds.
    #[test]
    fn every_load_gets_a_fresh_id() {
        let mut m = modal(2);
        let first = m.load_gen;
        m.step(1);
        m.step(-1);
        assert_ne!(m.load_gen, first);
        assert_ne!(modal(2).load_gen, m.load_gen);
    }

    /// The minimap's dots are worked out once per load and size; scrolling
    /// only moves the shading.
    #[test]
    fn the_minimap_raster_is_cached() {
        let mut m = modal(1);
        let many = (1..=400)
            .map(|n| DiffLine {
                n: Some(n),
                kind: LineKind::Ctx,
                text: format!(" line {n}"),
            })
            .collect();
        m.load = DiffLoad::ready(many);
        draw(&mut m, 100, 22);
        let rows = Arc::clone(&m.minimap_cache.as_ref().unwrap().rows);
        m.scroll_by(50);
        draw(&mut m, 100, 22);
        assert!(Arc::ptr_eq(&rows, &m.minimap_cache.as_ref().unwrap().rows));
        draw(&mut m, 100, 30);
        assert!(!Arc::ptr_eq(&rows, &m.minimap_cache.as_ref().unwrap().rows));
    }

    #[test]
    fn step_clamps_and_resets() {
        let mut m = modal(2);
        m.scroll = 5;
        assert!(!m.step(-1));
        assert!(m.step(1));
        assert_eq!((m.index, m.scroll), (1, 0));
        assert!(matches!(m.load, DiffLoad::Loading));
        assert!(!m.step(1));
    }
}
