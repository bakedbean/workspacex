//! Bottom-pinned detail bar shown when a workspace is selected on the
//! dashboard. Renders header strip, a 1–4 container body (each
//! container holding one or more modules from `crate::ui::detail_modules`),
//! and an inline reply input.
//!
//! See `docs/superpowers/specs/2026-05-25-detail-bar-modules-design.md`.

use crate::activity::events::WorkspaceEvents;
use crate::activity::proc::ProcInfo;
use crate::config::detail_bar_config::DetailBarConfig;
use crate::data::store::{Repo, Workspace, WorkspaceRecap};
use crate::git::DiffStats;
use crate::git::forge::{BranchLifecycle, ReviewDecision};
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;

/// What `app.rs::draw` assembles for the detail bar. Borrowed for the
/// duration of a single draw call.
#[derive(Debug)]
pub struct DetailInputs<'a> {
    pub repo: &'a Repo,
    pub workspace: &'a Workspace,
    pub events: Option<&'a WorkspaceEvents>,
    /// The workspace's latest `wsx recap set` digest, when it has one.
    /// SESSION SUMMARY leads with it in place of the first user prompt.
    pub recap: Option<&'a WorkspaceRecap>,
    pub procs: &'a [ProcInfo],
    pub diff: Option<DiffStats>,
    /// Per-file diff stats keyed by path relative to the worktree
    /// root. Used to annotate RECENT FILES entries with `+X −Y`.
    pub diff_per_file: Option<&'a std::collections::HashMap<String, DiffStats>>,
    pub lifecycle: Option<BranchLifecycle>,
    pub pr_title: Option<&'a str>,
    pub pr_number: Option<u32>,
    /// The PR's review verdict, drawn as a trailing mark on the header chip.
    /// `None` when the repo has no approval gate or the verdict is unknown.
    pub review: Option<ReviewDecision>,
    /// Unresolved review-thread count, drawn as digits after the mark.
    pub unresolved: Option<u32>,
    pub status: Status,
    pub ago_secs: Option<u64>,
    pub reply_draft: &'a str,
    pub reply_focused: bool,
    /// True once the workspace's JSONL has been scanned at least once
    /// (`workspace_events_scanned` on `App`). When false, SESSION
    /// SUMMARY and RECENT CHAT show `loading…` placeholders instead
    /// of derived content.
    pub events_scanned: bool,
    pub config: &'a DetailBarConfig,
    pub registry: &'a crate::ui::detail_modules::Registry,
    /// Pinned commands resolved for the selected workspace's repo. The chip
    /// row is rendered when these exist or when `[dashboard_detail]` places
    /// a module (see `BarSpecs::places_module`).
    pub pinned: &'a [crate::commands::pinned::PinnedCommand],
    /// The resolved bar theme, so the chip row draws through
    /// `crate::ui::bar::dashboard_detail` instead of a bespoke painter.
    pub bar_specs: &'a crate::config::theme_file::BarSpecs,
    pub fleet: &'a crate::ui::bar::segment::SegmentMap,
    /// Per-slot scroll offsets. Borrowed mutably so the container can
    /// clamp them to the current content height during render.
    pub scroll_offsets: &'a mut [u16; 4],
}

#[derive(Debug, Default)]
pub struct DetailDrawOutput {
    /// `(pinned-command index, rect)` per pinned chip — see
    /// `crate::ui::attached::PanesDrawOutput::chip_rects` for why the index
    /// travels with the rect.
    pub chip_rects: Vec<(usize, ratatui::layout::Rect)>,
    pub container_rects: [Option<ratatui::layout::Rect>; 4],
    pub pr_link_rect: Option<ratatui::layout::Rect>,
}

/// Render the detail bar into `area`. No-op when `area.height` is below
/// the config's `minimum_height()` — which is `CHROME_ROWS` (4) when no
/// container has any modules, or `min_rows` otherwise (caller is expected
/// to fall back to a condensed banner — see `app.rs::draw`).
pub fn render(
    f: &mut Frame,
    area: Rect,
    inputs: &mut DetailInputs<'_>,
    theme: &Theme,
) -> DetailDrawOutput {
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::widgets::Paragraph;

    // The chip row holds pinned commands and whatever `[module.*]` the
    // theme places in `[dashboard_detail]`. Either earns it a row — unless
    // the reply row places `$pins` itself, which folds the chips into it.
    let specs = inputs.bar_specs;
    let pins_in_reply = specs.places(&specs.dashboard_detail_reply, "pins");
    let chip_present = (!inputs.pinned.is_empty() && !pins_in_reply)
        || specs.places_module(&specs.dashboard_detail);
    let has_body = inputs.config.has_body();
    // The body region holds the top horizontal rule, container content,
    // and bottom horizontal rule as a single 3+ row strip — so that
    // vertical separators between containers run uninterrupted across
    // all three rows. When `!has_body` it collapses to just the two
    // rule rows (no content between).
    let body_region_rows: u16 = if has_body { 3 } else { 2 };
    let min_rows: u16 = 1 // header
        + body_region_rows
        + if chip_present { 1 } else { 0 } // chip slot
        + 1; // reply
    let needed = inputs.config.minimum_height().max(min_rows);
    if area.height == 0 || area.height < needed {
        return DetailDrawOutput::default();
    }

    let body_region_constraint = if has_body {
        Constraint::Min(body_region_rows)
    } else {
        Constraint::Length(body_region_rows)
    };
    let constraints: Vec<Constraint> = if chip_present {
        vec![
            Constraint::Length(1), // header
            body_region_constraint,
            Constraint::Length(1), // chips
            Constraint::Length(1), // reply
        ]
    } else {
        vec![
            Constraint::Length(1), // header
            body_region_constraint,
            Constraint::Length(1), // reply
        ]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let header_area = chunks[0];
    let body_region = chunks[1];
    let (chip_area, reply_area) = if chip_present {
        (Some(chunks[2]), chunks[3])
    } else {
        (None, chunks[2])
    };

    // The header row is a themed bar (`[dashboard_detail_header]`); its
    // `$pr` chip carries `Hit::Pr`, which becomes the PR link rect.
    let pr = inputs
        .pr_number
        .zip(inputs.lifecycle)
        .map(|(number, lifecycle)| crate::ui::attached::ChipPr {
            lifecycle,
            number,
            review: inputs.review,
            unresolved: inputs.unresolved,
        });
    let header = crate::ui::bar::dashboard_detail_header(
        inputs.bar_specs,
        theme,
        &crate::ui::bar::DetailHeaderInputs {
            agent: inputs.workspace.agent,
            name: &inputs.workspace.name,
            branch: &inputs.workspace.branch,
            pr,
            diff: inputs.diff,
            procs: inputs.procs.len() as u32,
            status: inputs.status,
            ago_secs: inputs.ago_secs,
            fleet: inputs.fleet,
        },
        header_area.width,
    );
    f.render_widget(Paragraph::new(header.line), header_area);
    let pr_link_rect = crate::ui::bar::render::hit_rects(header_area, &header.hits)
        .into_iter()
        .find_map(|(rect, hit)| (hit == crate::ui::bar::segment::Hit::Pr).then_some(rect));

    let container_rects = render_body_region(f, body_region, inputs, theme);

    // The detail bar's PR chip, diff count, and procs count live in the header
    // strip and row (above/elsewhere), so the chip row here carries pinned
    // commands only — no right-justified agent pills, procs, diff, or PR chip.
    // Themed via `[dashboard_detail]`, the fourth bar the engine draws.
    let mut chip_rects: Vec<(usize, Rect)> = if let Some(area) = chip_area {
        let rendered = crate::ui::bar::dashboard_detail(
            inputs.bar_specs,
            theme,
            inputs.pinned,
            inputs.fleet,
            area.width,
        );
        f.render_widget(Paragraph::new(rendered.line), area);
        crate::ui::bar::render::hit_rects(area, &rendered.hits)
            .into_iter()
            .filter_map(|(rect, hit)| match hit {
                crate::ui::bar::segment::Hit::PinnedChip(i) => Some((i, rect)),
                _ => None,
            })
            .collect()
    } else {
        Vec::new()
    };

    // The reply row is a themed bar too (`[dashboard_detail_reply]`), drawn
    // like a shell prompt with the draft between its two sides.
    let reply = crate::ui::bar::dashboard_detail_reply(
        inputs.bar_specs,
        theme,
        &crate::ui::bar::DetailReplyInputs {
            agent: inputs.workspace.agent,
            name: &inputs.workspace.name,
            branch: &inputs.workspace.branch,
            draft: inputs.reply_draft,
            focused: inputs.reply_focused,
            pinned: inputs.pinned,
            fleet: inputs.fleet,
        },
        reply_area.width,
    );
    f.render_widget(Paragraph::new(reply.line), reply_area);
    chip_rects.extend(
        crate::ui::bar::render::hit_rects(reply_area, &reply.hits)
            .into_iter()
            .filter_map(|(rect, hit)| match hit {
                crate::ui::bar::segment::Hit::PinnedChip(i) => Some((i, rect)),
                _ => None,
            }),
    );
    if inputs.reply_focused {
        f.set_cursor_position((reply_area.x + reply.cursor_x, reply_area.y));
    }

    DetailDrawOutput {
        chip_rects,
        container_rects,
        pr_link_rect,
    }
}

fn render_body_region(
    f: &mut Frame,
    area: Rect,
    inputs: &mut DetailInputs<'_>,
    theme: &Theme,
) -> [Option<Rect>; 4] {
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::widgets::Paragraph;

    let mut rects: [Option<Rect>; 4] = [None; 4];
    if area.height < 2 || area.width == 0 {
        return rects;
    }
    let cfg = inputs.config;

    // Always draw full-width top and bottom horizontal rules. When
    // multiple containers are present, the vertical separators drawn
    // below overwrite the rule's `─` cells with `┬` / `┴` junctions
    // so both lines stay visually continuous through the intersection.
    let rule_style = theme.dim_style();
    let rule_line = Line::from(Span::styled("─".repeat(area.width as usize), rule_style));
    let top_rule = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    let bottom_rule = Rect {
        x: area.x,
        y: area.y + area.height - 1,
        width: area.width,
        height: 1,
    };
    f.render_widget(Paragraph::new(rule_line.clone()), top_rule);
    f.render_widget(Paragraph::new(rule_line), bottom_rule);

    if !cfg.has_body() {
        return rects;
    }

    // Narrow-terminal collapse: < 80 cols → first non-empty container only.
    let containers: Vec<&Vec<String>> = if area.width < 80 {
        cfg.containers
            .iter()
            .find(|c| !c.is_empty())
            .into_iter()
            .collect()
    } else {
        cfg.containers.iter().collect()
    };

    let n = containers.len();
    if n == 0 {
        return rects;
    }

    // Horizontal split: N columns share the remaining width equally
    // (Fill(1)), with a single 1-cell separator chunk between each
    // pair. No additional gap cells — module renderers already pad
    // their content internally.
    let mut h_constraints: Vec<Constraint> = Vec::with_capacity(2 * n - 1);
    for i in 0..n {
        if i > 0 {
            h_constraints.push(Constraint::Length(1));
        }
        h_constraints.push(Constraint::Fill(1));
    }
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(h_constraints)
        .split(area);

    let ctx = crate::ui::detail_modules::DetailContext {
        repo: inputs.repo,
        workspace: inputs.workspace,
        events: inputs.events,
        recap: inputs.recap,
        procs: inputs.procs,
        diff: inputs.diff,
        diff_per_file: inputs.diff_per_file,
        lifecycle: inputs.lifecycle,
        pr_title: inputs.pr_title,
        pr_number: inputs.pr_number,
        status: inputs.status,
        ago_secs: inputs.ago_secs,
        events_scanned: inputs.events_scanned,
        theme,
    };
    let registry = inputs.registry;
    let scroll_offsets: &mut [u16; 4] = inputs.scroll_offsets;

    // Container content sits between the two rule rows. Column i sits
    // at chunk i*2 (pattern: col, sep, col, sep, col, …).
    for (i, ids) in containers.iter().enumerate() {
        let col = h_chunks[i * 2];
        let content = Rect {
            x: col.x,
            y: col.y + 1,
            width: col.width,
            height: col.height.saturating_sub(2),
        };
        let slot_idx = if area.width < 80 {
            // Narrow-collapse path: only the first non-empty container renders.
            // Its slot index in the App-state array is its original position
            // in cfg.containers — not its position in the filtered view.
            cfg.containers
                .iter()
                .position(|c| !c.is_empty())
                .unwrap_or(0)
        } else {
            // Normal path: container order is preserved, slot = loop index.
            i
        };
        if slot_idx < 4 {
            render_container(
                f,
                content,
                ids,
                &ctx,
                registry,
                theme,
                &mut scroll_offsets[slot_idx],
            );
            rects[slot_idx] = Some(content);
        }
    }

    // Vertical separators run the FULL body-region height. At the top
    // and bottom rule rows we draw `┬` / `┴` instead of `│` so the
    // horizontal rule maintains visual continuity through the
    // intersection (its arms tie into the junction's horizontal arms).
    for i in 1..n {
        let sep_area = h_chunks[i * 2 - 1];
        let last = sep_area.height.saturating_sub(1);
        let sep_lines: Vec<Line<'static>> = (0..sep_area.height)
            .map(|row| {
                let glyph = if row == 0 {
                    "┬"
                } else if row == last {
                    "┴"
                } else {
                    "│"
                };
                Line::from(Span::styled(glyph.to_string(), rule_style))
            })
            .collect();
        f.render_widget(Paragraph::new(sep_lines), sep_area);
    }

    rects
}

fn render_container(
    f: &mut Frame,
    area: Rect,
    module_ids: &[String],
    ctx: &crate::ui::detail_modules::DetailContext<'_>,
    reg: &crate::ui::detail_modules::Registry,
    theme: &Theme,
    offset: &mut u16,
) {
    use ratatui::widgets::Paragraph;

    if module_ids.is_empty() || area.height == 0 || area.width == 0 {
        return;
    }

    // Scrollbars are intentionally not drawn: content scrolls via keyboard/wheel
    // and the full container width is used for content. Overflow is silent.
    let content_width = area.width;
    let content_area = area;

    let label_style = Style::default().fg(theme.path).add_modifier(Modifier::BOLD);

    // Build virtual line list: title row + body lines + 1-row gap between
    // modules. Last module has no trailing gap.
    let mut virtual_lines: Vec<Line<'static>> = Vec::new();
    let last_idx = module_ids.len().saturating_sub(1);
    for (i, id) in module_ids.iter().enumerate() {
        match reg.get(id) {
            Some(m) => {
                virtual_lines.push(Line::from(Span::styled(m.title(), label_style)));
                virtual_lines.extend(m.lines(ctx, content_width));
            }
            None => {
                tracing::warn!(id = %id, "detail_bar: unknown module id in container");
                virtual_lines.push(Line::from(Span::styled(
                    format!("[unknown: {id}]"),
                    theme.dim_style(),
                )));
            }
        }
        if i != last_idx {
            virtual_lines.push(Line::from(""));
        }
    }

    let content_height: u16 = virtual_lines.len().min(u16::MAX as usize) as u16;
    let max_offset = content_height.saturating_sub(area.height);
    if *offset > max_offset {
        *offset = max_offset;
    }

    let start = *offset as usize;
    let end = (start + area.height as usize).min(virtual_lines.len());
    let visible: Vec<Line<'static>> = virtual_lines[start..end].to_vec();
    f.render_widget(Paragraph::new(visible), content_area);
}

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// Render the PROCESSES module body. Returns one row per process
/// (capped at 5, with a "+N more" suffix when over the cap), or a
/// single "—" placeholder when empty. The host (`render_container`)
/// draws the title row separately.
pub(crate) fn build_processes(
    procs: &[ProcInfo],
    theme: &Theme,
    column_width: usize,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    if procs.is_empty() {
        out.push(Line::from(Span::styled("—".to_string(), theme.dim_style())));
    } else {
        let visible = procs.iter().take(5);
        for p in visible {
            let cmd = truncate_to_chars(&p.command, column_width.saturating_sub(4));
            out.push(Line::from(vec![
                Span::styled("● ".to_string(), theme.status_style(Status::Thinking)),
                Span::styled(cmd, theme.dim_style()),
            ]));
        }
        if procs.len() > 5 {
            out.push(Line::from(Span::styled(
                format!("+{} more", procs.len() - 5),
                theme.dim_style(),
            )));
        }
    }
    out
}

/// Render the RECENT FILES module body. Returns one row per file
/// (capped at 5), each annotated with per-file diff stats when
/// available, or a single "—" placeholder when empty. The host
/// (`render_container`) draws the title row separately.
pub(crate) fn build_recent_files(
    events: Option<&WorkspaceEvents>,
    diff_per_file: Option<&std::collections::HashMap<String, DiffStats>>,
    worktree_path: &std::path::Path,
    theme: &Theme,
    column_width: usize,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    // The list is the union of (a) files the tailed agent edited this session and
    // (b) files that changed vs the base branch (the same git diff that powers the
    // row's +N −M). Session edits come first, in recency order, so live edits show
    // immediately; committed/delegated changes still appear when the tailed session
    // made no edits of its own — e.g. one agent delegates the fix to another, or the
    // session reset after a commit. Session paths are reduced to worktree-relative
    // form to match `diff_per_file`'s keys (a file edited outside the worktree keeps
    // its absolute path and simply won't find a diff entry), so the two sources
    // dedupe against each other and annotate against `diff_per_file`.
    let mut files: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(e) = events {
        for f in &e.recent_edited_files {
            let rel = display_relative_path(f, worktree_path);
            if seen.insert(rel.clone()) {
                files.push(rel);
            }
        }
    }
    if let Some(map) = diff_per_file {
        let mut git_only: Vec<&String> = map.keys().filter(|k| !seen.contains(*k)).collect();
        git_only.sort(); // deterministic order for files with no session signal
        for k in git_only {
            seen.insert(k.clone());
            files.push(k.clone());
        }
    }
    if files.is_empty() {
        out.push(Line::from(Span::styled("—".to_string(), theme.dim_style())));
    } else {
        for f in files.iter().take(5) {
            // `f` is already worktree-relative: both the display path and the key.
            let diff = diff_per_file.and_then(|m| m.get(f)).copied();
            // The right-aligned `+added −removed` cluster, when present.
            let counts = match diff {
                Some(d) if d.added > 0 || d.removed > 0 => {
                    Some((format!("+{}", d.added), format!("−{}", d.removed)))
                }
                _ => None,
            };
            // Visible width of that cluster ("+A" + space + "−R").
            let cluster_width = counts
                .as_ref()
                .map(|(added, removed)| added.chars().count() + 1 + removed.chars().count())
                .unwrap_or(0);
            // Reserve the cluster plus a minimum 2-space gutter so the
            // path can never butt up against the counts. With no counts
            // the path takes the full width — no gutter to reserve.
            let path_width = if counts.is_some() {
                column_width.saturating_sub(cluster_width + 2)
            } else {
                column_width
            };
            let truncated = truncate_to_chars_left(f, path_width);
            let mut spans: Vec<Span<'static>> =
                vec![Span::styled(truncated.clone(), theme.dim_style())];
            if let Some((added, removed)) = counts {
                // Pad so the cluster's last glyph lands exactly at
                // `column_width`, keeping counts flush-right across rows.
                let pad = column_width.saturating_sub(cluster_width + truncated.chars().count());
                spans.push(Span::raw(" ".repeat(pad)));
                spans.push(Span::styled(added, theme.ok_style()));
                spans.push(Span::raw(" ".to_string()));
                spans.push(Span::styled(removed, theme.err_style()));
            }
            out.push(Line::from(spans));
        }
    }
    out
}

/// Render a recent-edited file path relative to the worktree root.
/// Returns the absolute path unchanged when it doesn't sit inside the
/// worktree (rare — only happens if claude wrote a path outside its
/// own cwd).
fn display_relative_path(file: &str, worktree_path: &std::path::Path) -> String {
    std::path::Path::new(file)
        .strip_prefix(worktree_path)
        .ok()
        .and_then(|p| p.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| file.to_string())
}

fn truncate_to_chars(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn truncate_to_chars_left(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else {
        let skip = count.saturating_sub(max.saturating_sub(1));
        let tail: String = s.chars().skip(skip).collect();
        format!("…{tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::forge::ReviewDecision;
    use crate::pty::session::AgentKind;
    use crate::ui::attached::ChipPr;
    use crate::ui::dashboard::status::Status;
    use crate::ui::theme::Theme;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn line_to_string(line: &ratatui::text::Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn make_registry() -> crate::ui::detail_modules::Registry {
        let mut reg = crate::ui::detail_modules::Registry::new();
        crate::ui::detail_modules::register_builtins(&mut reg);
        reg
    }

    fn bar_specs() -> crate::config::theme_file::BarSpecs {
        crate::config::theme_file::bundled_default(&Theme::wsx())
    }

    fn render_to_text(inputs: &mut DetailInputs<'_>, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let theme = Theme::wsx();
                render(f, Rect::new(0, 0, w, h), inputs, &theme);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut s = String::new();
        for y in 0..h {
            for x in 0..w {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    fn seed_workspace() -> (
        crate::data::store::Store,
        crate::data::store::Repo,
        crate::data::store::Workspace,
    ) {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "ws",
                branch: "repo/ws",
                worktree_path: std::path::Path::new("/tmp/r/ws"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == repo_id)
            .unwrap();
        let ws = store
            .workspaces(repo_id)
            .unwrap()
            .into_iter()
            .find(|w| w.id == id)
            .unwrap();
        (store, repo, ws)
    }

    #[test]
    fn render_into_zero_area_is_a_noop() {
        // Sanity: rendering into a zero-height area must not panic.
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let (_store, repo, ws) = seed_workspace();
        let reg = make_registry();
        let result = terminal.draw(|f| {
            let theme = Theme::wsx();
            let cfg = DetailBarConfig::default();
            let mut offsets = [0u16; 4];
            let specs = bar_specs();
            let mut inputs = DetailInputs {
                repo: &repo,
                workspace: &ws,
                events: None,
                recap: None,
                procs: &[],
                diff: None,
                diff_per_file: None,
                lifecycle: None,
                pr_title: None,
                pr_number: None,
                review: None,
                unresolved: None,
                status: Status::Idle,
                ago_secs: None,
                reply_draft: "",
                reply_focused: false,
                events_scanned: false,
                config: &cfg,
                registry: &reg,
                pinned: &[],
                bar_specs: &specs,
                fleet: crate::ui::bar::fleet::empty(),
                scroll_offsets: &mut offsets,
            };
            render(f, Rect::new(0, 0, 80, 0), &mut inputs, &theme);
        });
        assert!(result.is_ok());
    }

    /// The header row's inputs for a Claude workspace `ws` on branch `br`
    /// with nothing else going on; tests override what they exercise.
    fn header_inputs() -> crate::ui::bar::DetailHeaderInputs<'static> {
        crate::ui::bar::DetailHeaderInputs {
            agent: AgentKind::Claude,
            name: "ws",
            branch: "br",
            pr: None,
            diff: None,
            procs: 0,
            status: Status::Idle,
            ago_secs: None,
            fleet: crate::ui::bar::fleet::empty(),
        }
    }

    fn chip_pr(lifecycle: BranchLifecycle, review: Option<ReviewDecision>) -> Option<ChipPr> {
        Some(ChipPr {
            lifecycle,
            number: 152,
            review,
            unresolved: None,
        })
    }

    /// The header row as drawn by `specs`, with its text and the PR chip's
    /// hit span, if any.
    fn header_with(
        specs: &crate::config::theme_file::BarSpecs,
        inputs: &crate::ui::bar::DetailHeaderInputs<'_>,
    ) -> (
        crate::ui::bar::render::Rendered,
        String,
        Option<crate::ui::bar::segment::HitSpan>,
    ) {
        let rendered = crate::ui::bar::dashboard_detail_header(specs, &Theme::wsx(), inputs, 120);
        let text = line_to_string(&rendered.line);
        let pr = rendered
            .hits
            .iter()
            .find(|h| h.hit == crate::ui::bar::segment::Hit::Pr)
            .cloned();
        (rendered, text, pr)
    }

    fn header(
        inputs: &crate::ui::bar::DetailHeaderInputs<'_>,
    ) -> (String, Option<crate::ui::bar::segment::HitSpan>) {
        let (_, text, pr) = header_with(&bar_specs(), inputs);
        (text, pr)
    }

    /// The cell column `needle` starts at in `text`.
    fn col_of(text: &str, needle: &str) -> u16 {
        let byte = text
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} in {text:?}"));
        unicode_width::UnicodeWidthStr::width(&text[..byte]) as u16
    }

    #[test]
    fn header_strip_contains_all_chips_in_order() {
        let (text, _) = header(&crate::ui::bar::DetailHeaderInputs {
            name: "repo-overview",
            branch: "bakedbean/repo-overview",
            pr: chip_pr(BranchLifecycle::PrOpen, None),
            diff: Some(DiffStats {
                added: 12,
                removed: 3,
            }),
            procs: 2,
            status: Status::Question,
            ago_secs: Some(29),
            ..header_inputs()
        });
        let order = [
            "▎ repo-overview",
            "⎇ bakedbean/repo-overview",
            "#152 open",
            "+12 −3",
            "● 2p",
            "? question",
            "· 29s",
        ];
        let cols: Vec<u16> = order.iter().map(|n| col_of(&text, n)).collect();
        assert!(cols.is_sorted(), "{order:?} out of order in {text:?}");
    }

    #[test]
    fn header_strip_omits_what_the_workspace_lacks() {
        let (text, pr) = header(&header_inputs());
        assert_eq!(text.trim_end(), "▎ ws  ⎇ br  · idle", "{text:?}");
        assert!(pr.is_none(), "no chip without a PR");
    }

    #[test]
    fn header_strip_omits_chip_for_no_pr_even_with_number() {
        // `NoPr` is what the store emits for a branch with no PR; its
        // lifecycle_chip glyph is empty, so no chip is drawn even if a
        // stale number is somehow present.
        let (text, pr) = header(&crate::ui::bar::DetailHeaderInputs {
            pr: chip_pr(BranchLifecycle::NoPr, None),
            ..header_inputs()
        });
        assert!(!text.contains("#152"), "no chip number for NoPr: {text:?}");
        assert!(pr.is_none(), "no chip rect for NoPr: {text:?}");
    }

    /// The lead cell is `$agent_bar`: the bundled `▎` in the agent's
    /// colour, or the kind's `[agent_bar.symbols]` icon when the theme
    /// draws one — which shifts the PR chip's hit by its extra cell.
    #[test]
    fn header_strip_leads_with_the_agent_bar() {
        let inputs = crate::ui::bar::DetailHeaderInputs {
            pr: chip_pr(BranchLifecycle::PrOpen, None),
            ..header_inputs()
        };
        let theme = Theme::wsx();
        let (plain, text, plain_pr) = header_with(&bar_specs(), &inputs);
        assert!(text.starts_with("▎ ws"), "{text:?}");
        assert_eq!(
            plain.line.spans[0].style.fg,
            theme.agent_style(AgentKind::Claude).fg
        );

        let iconed = crate::config::theme_file::resolve(
            crate::config::theme_file::ThemeFile::parse("[agent_bar.symbols]\nclaude = \"🤖\"\n")
                .unwrap(),
            &theme,
        )
        .unwrap();
        let (line, text, pr) = header_with(&iconed, &inputs);
        assert!(text.starts_with("🤖 ws"), "{text:?}");
        assert_eq!(
            line.line.spans[0].style.fg,
            theme.agent_style(AgentKind::Claude).fg
        );
        // The icon is two cells wide against the bar's one.
        assert_eq!(pr.unwrap().start_col, plain_pr.unwrap().start_col + 1);
    }

    #[test]
    fn header_strip_marks_an_approved_pr_and_covers_it_with_the_chip_rect() {
        let (text, pr) = header(&crate::ui::bar::DetailHeaderInputs {
            pr: chip_pr(BranchLifecycle::PrOpen, Some(ReviewDecision::Approved)),
            ..header_inputs()
        });
        // The mark is part of the chip, so the click rect must include it —
        // otherwise clicking the tick lands on the next cell.
        let pr = pr.expect("chip rect should be present");
        assert_eq!(pr.start_col, col_of(&text, "⏺ #152 open ✓"));
        assert_eq!(
            pr.width as usize,
            unicode_width::UnicodeWidthStr::width("⏺ #152 open ✓")
        );
    }

    #[test]
    fn header_strip_leaves_a_merged_pr_unmarked() {
        let (text, _) = header(&crate::ui::bar::DetailHeaderInputs {
            pr: chip_pr(BranchLifecycle::PrMerged, Some(ReviewDecision::Approved)),
            ..header_inputs()
        });
        assert!(text.contains("⏺ #152 merged"), "chip present: {text:?}");
        assert!(!text.contains('✓'), "merged PRs carry no mark: {text:?}");
    }

    /// A theme's `[dashboard_detail_header]` restyles the row like any other
    /// bar: here a background block behind the name, the status flush right.
    #[test]
    fn header_strip_follows_the_theme() {
        let specs = crate::config::theme_file::resolve(
            crate::config::theme_file::ThemeFile::parse(
                "[dashboard_detail_header]\n\
                 format = \"[ $workspace ](bg:#303030)( $pr)\"\n\
                 right_format = \"$status\"\n",
            )
            .unwrap(),
            &Theme::wsx(),
        )
        .unwrap();
        let (rendered, text, pr) = header_with(
            &specs,
            &crate::ui::bar::DetailHeaderInputs {
                pr: chip_pr(BranchLifecycle::PrOpen, None),
                ..header_inputs()
            },
        );
        assert!(text.starts_with(" ws  ⏺ #152 open"), "{text:?}");
        assert!(text.ends_with("· idle"), "status flush right: {text:?}");
        assert!(!text.contains('⎇'), "branch not placed: {text:?}");
        let name = rendered
            .line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "ws")
            .expect("name span");
        assert_eq!(
            name.style.bg,
            Some(ratatui::style::Color::Rgb(0x30, 0x30, 0x30))
        );
        assert_eq!(pr.unwrap().start_col, col_of(&text, "⏺ #152"));
    }

    fn reply_row(draft: &str, focused: bool, width: u16) -> crate::ui::bar::bars::ReplyRendered {
        let theme = Theme::wsx();
        crate::ui::bar::dashboard_detail_reply(
            &bar_specs(),
            &theme,
            &crate::ui::bar::DetailReplyInputs {
                agent: AgentKind::Claude,
                name: "foo",
                branch: "wsx/foo",
                draft,
                focused,
                pinned: &[],
                fleet: crate::ui::bar::fleet::empty(),
            },
            width,
        )
    }

    #[test]
    fn reply_input_row_is_a_prompt_then_the_draft() {
        let text = line_to_string(&reply_row("hello agent", false, 80).line);
        assert!(
            text.starts_with("❯ hello agent"),
            "prompt, then draft: {text:?}"
        );
        assert!(
            !text.contains("Reply to agent"),
            "no placeholder over a draft: {text:?}"
        );
    }

    #[test]
    fn reply_input_row_shows_placeholder_when_empty() {
        let text = line_to_string(&reply_row("", false, 80).line);
        assert!(text.starts_with("❯ Reply to agent"), "ghost text: {text:?}");
    }

    #[test]
    fn reply_input_row_prompt_is_live_only_when_focused() {
        let theme = Theme::wsx();
        let focused = reply_row("", true, 80).line;
        let idle = reply_row("", false, 80).line;
        assert_eq!(
            focused.spans[0].style.fg,
            theme.agent_style(AgentKind::Claude).fg,
            "focused prompt wears the agent colour"
        );
        assert_eq!(
            idle.spans[0].style.fg,
            theme.dim_style().fg,
            "idle prompt is dim"
        );
    }

    #[test]
    fn reply_input_row_shows_send_hint_when_focused() {
        let text = line_to_string(&reply_row("", true, 80).line);
        assert!(
            text.contains("send"),
            "send hint present when focused: {text:?}"
        );
        assert!(
            text.contains("cancel"),
            "cancel hint present when focused: {text:?}"
        );
        assert_eq!(text.chars().count(), 80, "row fills the width: {text:?}");
    }

    #[test]
    fn reply_input_row_hides_hints_when_unfocused() {
        let text = line_to_string(&reply_row("", false, 80).line);
        assert!(
            !text.contains("send"),
            "send hint absent when unfocused: {text:?}"
        );
        assert!(
            !text.contains("cancel"),
            "cancel hint absent when unfocused: {text:?}"
        );
    }

    #[test]
    fn reply_input_row_scrolls_long_drafts_to_end() {
        // A long draft must show its END (where the cursor lives), not
        // its beginning — otherwise the user can't see what they're typing.
        let draft = format!("{} END", "a".repeat(60));
        let rendered = reply_row(&draft, true, 60);
        let text = line_to_string(&rendered.line);
        assert!(text.contains("END"), "tail of draft visible: {text:?}");
        assert_eq!(text.chars().count(), 60, "row fits the width: {text:?}");
        let end = text.find("END").map(|b| text[..b].chars().count()).unwrap();
        assert_eq!(
            rendered.cursor_x as usize,
            end + 3,
            "cursor just past the draft"
        );
    }

    #[test]
    fn reply_input_row_cursor_sits_after_prompt_and_draft() {
        assert_eq!(
            reply_row("", true, 80).cursor_x,
            2,
            "on the placeholder's first cell"
        );
        assert_eq!(reply_row("hi", true, 80).cursor_x, 4);
    }

    #[test]
    fn reply_input_row_drops_the_hint_when_too_narrow() {
        let text = line_to_string(&reply_row("typing", true, 20).line);
        assert!(!text.contains("send"), "hint yields to the draft: {text:?}");
        assert!(text.starts_with("❯ typing"), "{text:?}");
    }

    #[test]
    fn full_render_paints_header_body_and_reply_row() {
        let (_store, repo, ws) = seed_workspace();
        let evt = crate::activity::events::WorkspaceEvents {
            first_user_text: Some("give me a tour".into()),
            tool_use_counts: crate::activity::events::ToolUseCounts {
                read: 14,
                bash: 2,
                ..Default::default()
            },
            last_assistant_text: Some("Reading the repo now.".into()),
            ..Default::default()
        };
        let cfg = DetailBarConfig::default();
        let reg = make_registry();
        let mut offsets = [0u16; 4];
        let specs = bar_specs();
        let mut inputs = DetailInputs {
            repo: &repo,
            workspace: &ws,
            events: Some(&evt),
            recap: None,
            procs: &[],
            diff: Some(DiffStats {
                added: 12,
                removed: 3,
            }),
            diff_per_file: None,
            lifecycle: Some(BranchLifecycle::PrOpen),
            pr_title: None,
            pr_number: None,
            review: None,
            unresolved: None,
            status: Status::Question,
            ago_secs: Some(29),
            reply_draft: "",
            reply_focused: false,
            events_scanned: true,
            config: &cfg,
            registry: &reg,
            pinned: &[],
            bar_specs: &specs,
            fleet: crate::ui::bar::fleet::empty(),
            scroll_offsets: &mut offsets,
        };
        let text = render_to_text(&mut inputs, 120, 10);
        assert!(
            text.contains("repo-overview") || text.contains("ws"),
            "header name: {text:?}"
        );
        assert!(text.contains("SESSION SUMMARY"), "summary label: {text:?}");
        assert!(text.contains("RECENT CHAT"), "chat label: {text:?}");
        assert!(text.contains("PROCESSES"), "procs label: {text:?}");
        assert!(text.contains("Reply to agent"), "reply chip: {text:?}");
    }

    #[test]
    fn chrome_only_mode_renders_header_and_reply_no_body_labels() {
        let (_store, repo, ws) = seed_workspace();
        let evt = crate::activity::events::WorkspaceEvents {
            first_user_text: Some("hi".into()),
            last_assistant_text: Some("ack".into()),
            ..Default::default()
        };
        // All containers empty — bar should collapse to 4
        // rows (header + 2 rules + reply input). Use all-empty inner
        // lists (sanitize resets an empty outer vec to defaults, but
        // empty inner lists are preserved as-is).
        let cfg = DetailBarConfig {
            containers: vec![vec![], vec![], vec![]],
            ..DetailBarConfig::default()
        };
        let reg = make_registry();
        let mut offsets = [0u16; 4];
        let specs = bar_specs();
        let mut inputs = DetailInputs {
            repo: &repo,
            workspace: &ws,
            events: Some(&evt),
            recap: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            review: None,
            unresolved: None,
            status: Status::Idle,
            ago_secs: None,
            reply_draft: "",
            reply_focused: false,
            events_scanned: true,
            config: &cfg,
            registry: &reg,
            pinned: &[],
            bar_specs: &specs,
            fleet: crate::ui::bar::fleet::empty(),
            scroll_offsets: &mut offsets,
        };
        // Width 100, height exactly CHROME_ROWS (4).
        let text = render_to_text(&mut inputs, 100, DetailBarConfig::CHROME_ROWS);
        assert!(text.contains("Reply to agent"), "reply chip: {text:?}");
        assert!(
            !text.contains("SESSION SUMMARY"),
            "no summary label: {text:?}"
        );
        assert!(!text.contains("RECENT CHAT"), "no chat label: {text:?}");
        assert!(!text.contains("PROCESSES"), "no procs label: {text:?}");
        assert!(
            !text.contains("give me a tour"),
            "no initial-prompt body: {text:?}"
        );
    }

    #[test]
    fn narrow_terminal_drops_chat_and_procs_columns() {
        let (_store, repo, ws) = seed_workspace();
        let evt = crate::activity::events::WorkspaceEvents {
            first_user_text: Some("hi".into()),
            last_assistant_text: Some("ack".into()),
            ..Default::default()
        };
        let cfg = DetailBarConfig::default();
        let reg = make_registry();
        let mut offsets = [0u16; 4];
        let specs = bar_specs();
        let mut inputs = DetailInputs {
            repo: &repo,
            workspace: &ws,
            events: Some(&evt),
            recap: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            review: None,
            unresolved: None,
            status: Status::Idle,
            ago_secs: None,
            reply_draft: "",
            reply_focused: false,
            events_scanned: true,
            config: &cfg,
            registry: &reg,
            pinned: &[],
            bar_specs: &specs,
            fleet: crate::ui::bar::fleet::empty(),
            scroll_offsets: &mut offsets,
        };
        let text = render_to_text(&mut inputs, 70, 10);
        assert!(text.contains("SESSION SUMMARY"), "summary kept: {text:?}");
        assert!(
            !text.contains("RECENT CHAT"),
            "chat dropped on narrow: {text:?}"
        );
        assert!(
            !text.contains("PROCESSES"),
            "procs dropped on narrow: {text:?}"
        );
    }

    #[test]
    fn renders_three_columns_with_default_config() {
        let cfg = DetailBarConfig::default();
        assert!(cfg.has_body());
        // Default containers: session_summary, recent_chat, processes+recent_files
        assert_eq!(cfg.containers.len(), 3);
        assert!(cfg.containers[0].contains(&"session_summary".to_string()));
        assert!(cfg.containers[1].contains(&"recent_chat".to_string()));
        assert!(cfg.containers[2].contains(&"processes".to_string()));
    }

    #[test]
    fn render_with_unknown_module_id_shows_placeholder() {
        let (_store, repo, ws) = seed_workspace();
        let cfg = DetailBarConfig {
            containers: vec![vec!["seshun_summary".into()]],
            ..Default::default()
        };
        let reg = make_registry();
        let mut offsets = [0u16; 4];
        let specs = bar_specs();
        let mut inputs = DetailInputs {
            repo: &repo,
            workspace: &ws,
            events: None,
            recap: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            review: None,
            unresolved: None,
            status: Status::Idle,
            ago_secs: None,
            reply_draft: "",
            reply_focused: false,
            events_scanned: true,
            config: &cfg,
            registry: &reg,
            pinned: &[],
            bar_specs: &specs,
            fleet: crate::ui::bar::fleet::empty(),
            scroll_offsets: &mut offsets,
        };
        let text = render_to_text(&mut inputs, 120, 10);
        assert!(
            text.contains("[unknown: seshun_summary]"),
            "expected unknown placeholder in: {text:?}",
        );
    }

    #[test]
    fn render_one_container_fills_full_width() {
        let (_store, repo, ws) = seed_workspace();
        let evt = crate::activity::events::WorkspaceEvents {
            last_assistant_text: Some("hello".into()),
            ..Default::default()
        };
        let cfg = DetailBarConfig {
            containers: vec![vec!["recent_chat".into()]],
            ..Default::default()
        };
        let reg = make_registry();
        let mut offsets = [0u16; 4];
        let specs = bar_specs();
        let mut inputs = DetailInputs {
            repo: &repo,
            workspace: &ws,
            events: Some(&evt),
            recap: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            review: None,
            unresolved: None,
            status: Status::Idle,
            ago_secs: None,
            reply_draft: "",
            reply_focused: false,
            events_scanned: true,
            config: &cfg,
            registry: &reg,
            pinned: &[],
            bar_specs: &specs,
            fleet: crate::ui::bar::fleet::empty(),
            scroll_offsets: &mut offsets,
        };
        let text = render_to_text(&mut inputs, 120, 10);
        assert!(text.contains("RECENT CHAT"), "chat title: {text:?}");
        // Other module titles must NOT appear when only recent_chat is configured.
        assert!(
            !text.contains("SESSION SUMMARY"),
            "summary leaked: {text:?}"
        );
        assert!(!text.contains("PROCESSES"), "procs leaked: {text:?}");
        assert!(!text.contains("RECENT FILES"), "files leaked: {text:?}");
    }

    #[test]
    fn render_with_pinned_includes_chip_row_above_reply() {
        let (_store, repo, ws) = seed_workspace();
        let cfg = DetailBarConfig::default();
        let reg = make_registry();
        let pinned = vec![
            crate::commands::pinned::PinnedCommand {
                label: "PR".into(),
                command: "/pull-request".into(),
                submit: true,
            },
            crate::commands::pinned::PinnedCommand {
                label: "FB".into(),
                command: "/feedback".into(),
                submit: true,
            },
        ];
        let mut offsets = [0u16; 4];
        let specs = bar_specs();
        let mut inputs = DetailInputs {
            repo: &repo,
            workspace: &ws,
            events: None,
            recap: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            review: None,
            unresolved: None,
            status: Status::Idle,
            ago_secs: None,
            reply_draft: "",
            reply_focused: false,
            events_scanned: true,
            config: &cfg,
            registry: &reg,
            pinned: &pinned,
            bar_specs: &specs,
            fleet: crate::ui::bar::fleet::empty(),
            scroll_offsets: &mut offsets,
        };
        let text = render_to_text(&mut inputs, 120, 12);
        // Chip labels must appear, and "Reply to agent" must still appear
        // (we only inserted a row, didn't remove the reply row).
        assert!(text.contains("PR"), "chip label PR present: {text:?}");
        assert!(text.contains("FB"), "chip label FB present: {text:?}");
        assert!(
            text.contains("Reply to agent"),
            "reply chip still present: {text:?}"
        );

        // Chip row must sit ABOVE the reply row.
        let pr_line = text
            .lines()
            .position(|l| l.contains(" PR "))
            .expect("PR line");
        let reply_line = text
            .lines()
            .position(|l| l.contains("Reply to agent"))
            .expect("reply line");
        assert!(
            pr_line < reply_line,
            "chip row above reply: pr={pr_line} reply={reply_line}"
        );
    }

    #[test]
    fn render_without_pinned_omits_chip_row() {
        let (_store, repo, ws) = seed_workspace();
        let cfg = DetailBarConfig::default();
        let reg = make_registry();
        let mut offsets = [0u16; 4];
        let specs = bar_specs();
        let mut inputs = DetailInputs {
            repo: &repo,
            workspace: &ws,
            events: None,
            recap: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            review: None,
            unresolved: None,
            status: Status::Idle,
            ago_secs: None,
            reply_draft: "",
            reply_focused: false,
            events_scanned: true,
            config: &cfg,
            registry: &reg,
            pinned: &[],
            bar_specs: &specs,
            fleet: crate::ui::bar::fleet::empty(),
            scroll_offsets: &mut offsets,
        };
        // Capture render's returned rects via a closure-bound outer mut
        // (Terminal::draw can't propagate values out of its closure).
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 12)).unwrap();
        let mut returned: Vec<(usize, ratatui::layout::Rect)> = Vec::new();
        terminal
            .draw(|f| {
                let theme = Theme::wsx();
                let out = render(f, Rect::new(0, 0, 120, 12), &mut inputs, &theme);
                returned = out.chip_rects;
            })
            .unwrap();
        assert!(returned.is_empty(), "no chip rects when pinned empty");
    }

    /// A `[dashboard_detail]` that places only a module — no `$pins`, no
    /// pinned commands — must still get its row. The row is allocated on
    /// what the theme *places*, not on what the fleet currently counts, so
    /// the layout does not jump as counts come and go.
    #[test]
    fn render_allocates_chip_row_for_a_module_without_pinned() {
        let (_store, repo, ws) = seed_workspace();
        let cfg = DetailBarConfig::default();
        let reg = make_registry();
        let mut offsets = [0u16; 4];
        let specs = crate::config::theme_file::resolve(
            crate::config::theme_file::ThemeFile::parse(
                "[module.pipe]\nformat = \"$workspaces workspaces\"\n\n[dashboard_detail]\nformat = \"$pipe\"\n",
            )
            .unwrap(),
            &Theme::wsx(),
        )
        .unwrap();
        let mut inputs = DetailInputs {
            repo: &repo,
            workspace: &ws,
            events: None,
            recap: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            review: None,
            unresolved: None,
            status: Status::Idle,
            ago_secs: None,
            reply_draft: "",
            reply_focused: false,
            events_scanned: true,
            config: &cfg,
            registry: &reg,
            pinned: &[],
            bar_specs: &specs,
            fleet: crate::ui::bar::fleet::empty(),
            scroll_offsets: &mut offsets,
        };
        let text = render_to_text(&mut inputs, 120, 12);
        let module_line = text
            .lines()
            .position(|l| l.contains("0 workspaces"))
            .unwrap_or_else(|| panic!("module rendered in the detail bar: {text:?}"));
        let reply_line = text
            .lines()
            .position(|l| l.contains("Reply to agent"))
            .expect("reply line");
        assert!(
            module_line < reply_line,
            "module row above reply: module={module_line} reply={reply_line}"
        );
    }

    /// A disabled module is not content: placing only it must not cost a row.
    #[test]
    fn render_omits_chip_row_for_a_disabled_module() {
        let (_store, repo, ws) = seed_workspace();
        let cfg = DetailBarConfig::default();
        let reg = make_registry();
        let mut offsets = [0u16; 4];
        let specs = crate::config::theme_file::resolve(
            crate::config::theme_file::ThemeFile::parse(
                "[module.pipe]\nformat = \"$workspaces workspaces\"\ndisabled = true\n\n[dashboard_detail]\nformat = \"$pipe\"\n",
            )
            .unwrap(),
            &Theme::wsx(),
        )
        .unwrap();
        let mut inputs = DetailInputs {
            repo: &repo,
            workspace: &ws,
            events: None,
            recap: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            review: None,
            unresolved: None,
            status: Status::Idle,
            ago_secs: None,
            reply_draft: "",
            reply_focused: false,
            events_scanned: true,
            config: &cfg,
            registry: &reg,
            pinned: &[],
            bar_specs: &specs,
            fleet: crate::ui::bar::fleet::empty(),
            scroll_offsets: &mut offsets,
        };
        let with_disabled = render_to_text(&mut inputs, 120, 12);
        let stock = bar_specs();
        inputs.bar_specs = &stock;
        let mut offsets2 = [0u16; 4];
        inputs.scroll_offsets = &mut offsets2;
        let without = render_to_text(&mut inputs, 120, 12);
        assert_eq!(
            with_disabled, without,
            "a disabled module must not add a row"
        );
    }

    /// A theme that places `$pins` in the reply row folds the chip row into
    /// it: no separate row, the chips lead the prompt, and their click
    /// rects land on the reply row.
    #[test]
    fn pins_in_the_reply_row_fold_the_chip_row_into_it() {
        let (_store, repo, ws) = seed_workspace();
        let cfg = DetailBarConfig::default();
        let reg = make_registry();
        let pinned = vec![crate::commands::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
            submit: true,
        }];
        let mut offsets = [0u16; 4];
        let specs = crate::config::theme_file::resolve(
            crate::config::theme_file::ThemeFile::parse(
                "[dashboard_detail_reply]\nformat = \"($pins  )$prompt \"\n",
            )
            .unwrap(),
            &Theme::wsx(),
        )
        .unwrap();
        let mut inputs = DetailInputs {
            repo: &repo,
            workspace: &ws,
            events: None,
            recap: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            review: None,
            unresolved: None,
            status: Status::Idle,
            ago_secs: None,
            reply_draft: "",
            reply_focused: false,
            events_scanned: true,
            config: &cfg,
            registry: &reg,
            pinned: &pinned,
            bar_specs: &specs,
            fleet: crate::ui::bar::fleet::empty(),
            scroll_offsets: &mut offsets,
        };
        let (w, h) = (100u16, 12u16);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        let mut rects: Vec<(usize, ratatui::layout::Rect)> = Vec::new();
        terminal
            .draw(|f| {
                rects = render(f, Rect::new(0, 0, w, h), &mut inputs, &Theme::wsx()).chip_rects;
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let row = |y: u16| -> String { (0..w).map(|x| buf[(x, y)].symbol()).collect() };
        let last = row(h - 1);
        assert!(
            last.starts_with(" 1  PR  ❯ Reply to agent"),
            "chips lead the prompt: {last:?}"
        );
        assert!(
            !row(h - 2).contains(" PR "),
            "no separate chip row: {:?}",
            row(h - 2)
        );
        assert_eq!(rects.len(), 1, "one chip rect");
        assert_eq!(rects[0].0, 0);
        assert_eq!(
            rects[0].1.y,
            h - 1,
            "the chip is clickable on the reply row"
        );
    }

    #[test]
    fn render_returns_empty_rects_when_area_too_short_for_chip_row() {
        // Regression guard for the latent cliff flagged in PR #104 review:
        // with chips configured but a chrome-only DetailBarConfig
        // (`minimum_height()` returns CHROME_ROWS == 4), if the available
        // area is 4 rows the layout doesn't fit chrome (5 rows including
        // chip slot). The early-return must bail so we don't return
        // invisible-but-clickable chip rects from a 0-height chunk.
        let (_store, repo, ws) = seed_workspace();
        let cfg = DetailBarConfig {
            containers: vec![vec![], vec![], vec![]], // all empty → no body
            ..DetailBarConfig::default()
        };
        let reg = make_registry();
        let pinned = vec![crate::commands::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pr".into(),
            submit: true,
        }];
        let mut offsets = [0u16; 4];
        let specs = bar_specs();
        let mut inputs = DetailInputs {
            repo: &repo,
            workspace: &ws,
            events: None,
            recap: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            review: None,
            unresolved: None,
            status: Status::Idle,
            ago_secs: None,
            reply_draft: "",
            reply_focused: false,
            events_scanned: true,
            config: &cfg,
            registry: &reg,
            pinned: &pinned,
            bar_specs: &specs,
            fleet: crate::ui::bar::fleet::empty(),
            scroll_offsets: &mut offsets,
        };
        // Area height exactly CHROME_ROWS (4). With chips present we need 5.
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 4)).unwrap();
        let mut returned: Vec<(usize, ratatui::layout::Rect)> = Vec::new();
        terminal
            .draw(|f| {
                let theme = Theme::wsx();
                let out = render(f, Rect::new(0, 0, 80, 4), &mut inputs, &theme);
                returned = out.chip_rects;
            })
            .unwrap();
        assert!(
            returned.is_empty(),
            "no chip rects when area is too short to fit chip row + chrome"
        );
    }

    #[test]
    fn build_processes_empty_emits_dash() {
        // Builders return body lines only; the dispatcher draws titles
        // (see render_container). Empty case = single "—" placeholder.
        let theme = Theme::default();
        let lines = build_processes(&[], &theme, 40);
        assert_eq!(lines.len(), 1);
        let placeholder = line_to_string(&lines[0]);
        assert_eq!(placeholder, "—");
    }

    #[test]
    fn build_recent_files_empty_emits_dash() {
        let theme = Theme::default();
        let path = std::path::PathBuf::from("/wt");
        let lines = build_recent_files(None, None, &path, &theme, 40);
        assert_eq!(lines.len(), 1);
        let placeholder = line_to_string(&lines[0]);
        assert_eq!(placeholder, "—");
    }

    #[test]
    fn build_recent_files_right_justifies_diff_counts() {
        // Counts must sit flush against the right edge regardless of how
        // long the path is or how many digits the counts have, so each
        // rendered row fills exactly `column_width` and ends with its
        // own `+added −removed` cluster.
        let theme = Theme::default();
        let worktree = std::path::PathBuf::from("/wt");
        let mut evt = crate::activity::events::WorkspaceEvents::default();
        evt.recent_edited_files
            .push_back("/wt/short.rs".to_string());
        evt.recent_edited_files
            .push_back("/wt/a/longer/nested/path/name.rs".to_string());
        let mut diff = std::collections::HashMap::new();
        diff.insert(
            "short.rs".to_string(),
            DiffStats {
                added: 5,
                removed: 3,
            },
        );
        diff.insert(
            "a/longer/nested/path/name.rs".to_string(),
            DiffStats {
                added: 120,
                removed: 40,
            },
        );

        let column_width = 40;
        let lines = build_recent_files(Some(&evt), Some(&diff), &worktree, &theme, column_width);
        assert_eq!(lines.len(), 2);
        for line in &lines {
            let s = line_to_string(line);
            assert_eq!(
                s.chars().count(),
                column_width,
                "row not right-justified to column_width: {s:?}"
            );
        }
        assert!(line_to_string(&lines[0]).ends_with("+5 −3"));
        assert!(line_to_string(&lines[1]).ends_with("+120 −40"));
    }

    #[test]
    fn build_recent_files_no_diff_uses_full_path_width() {
        // With no per-file diff there is no count cluster, so no gutter
        // should be reserved — a path longer than column_width gets
        // truncated to the full width, not width − 2.
        let theme = Theme::default();
        let worktree = std::path::PathBuf::from("/wt");
        let mut evt = crate::activity::events::WorkspaceEvents::default();
        evt.recent_edited_files
            .push_back("/wt/a/deeply/nested/directory/structure/file.rs".to_string());

        let column_width = 20;
        // No diff map at all → counts == None for every row.
        let lines = build_recent_files(Some(&evt), None, &worktree, &theme, column_width);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            line_to_string(&lines[0]).chars().count(),
            column_width,
            "path should fill the full column when no counts follow it"
        );
    }

    #[test]
    fn build_recent_files_lists_committed_files_with_no_session_edits() {
        // Regression: RECENT FILES must reflect the worktree's changes vs base
        // (the same git diff that powers the row's +N −M) even when the tailed
        // agent's session recorded no edits — e.g. one agent delegated the fix to
        // another (the editor's log isn't this session's), or the session reset
        // after a commit. Previously this collapsed to "—" while the row still
        // showed a count.
        let theme = Theme::default();
        let worktree = std::path::PathBuf::from("/wt");
        let mut diff = std::collections::HashMap::new();
        diff.insert(
            "src/auth.py".to_string(),
            DiffStats {
                added: 2,
                removed: 3,
            },
        );
        // No session events at all (tailed agent only read / ran bash).
        let lines = build_recent_files(None, Some(&diff), &worktree, &theme, 40);
        assert_eq!(
            lines.len(),
            1,
            "expected the committed file, got: {:?}",
            lines.iter().map(line_to_string).collect::<Vec<_>>()
        );
        let s = line_to_string(&lines[0]);
        assert!(s.contains("src/auth.py"), "row should list the file: {s:?}");
        assert!(s.ends_with("+2 −3"), "row should carry git counts: {s:?}");
    }

    #[test]
    fn build_recent_files_unions_session_then_git_only_files() {
        // The list is the union of session-edited files (first, in recency order)
        // and the remaining git-changed files (after, deterministic order).
        let theme = Theme::default();
        let worktree = std::path::PathBuf::from("/wt");
        let mut evt = crate::activity::events::WorkspaceEvents::default();
        evt.recent_edited_files.push_back("/wt/b.rs".to_string());
        let mut diff = std::collections::HashMap::new();
        diff.insert(
            "a.rs".to_string(),
            DiffStats {
                added: 1,
                removed: 0,
            },
        );
        diff.insert(
            "b.rs".to_string(),
            DiffStats {
                added: 2,
                removed: 2,
            },
        );
        let lines = build_recent_files(Some(&evt), Some(&diff), &worktree, &theme, 40);
        assert_eq!(lines.len(), 2);
        // session-edited b.rs first, then git-only a.rs.
        assert!(line_to_string(&lines[0]).contains("b.rs"));
        assert!(line_to_string(&lines[1]).contains("a.rs"));
    }

    #[test]
    fn body_renders_vertical_separator_between_containers() {
        // Three columns → two vertical rules running floor-to-ceiling
        // of the body region, including the top and bottom horizontal
        // rule rows. Every row between the header and reply must show
        // exactly two `│` glyphs.
        let (_store, repo, ws) = seed_workspace();
        let evt = crate::activity::events::WorkspaceEvents {
            first_user_text: Some("hi".into()),
            last_assistant_text: Some("ack".into()),
            ..Default::default()
        };
        let cfg = DetailBarConfig::default();
        let reg = make_registry();
        let mut offsets = [0u16; 4];
        let specs = bar_specs();
        let mut inputs = DetailInputs {
            repo: &repo,
            workspace: &ws,
            events: Some(&evt),
            recap: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            review: None,
            unresolved: None,
            status: Status::Idle,
            ago_secs: None,
            reply_draft: "",
            reply_focused: false,
            events_scanned: true,
            config: &cfg,
            registry: &reg,
            pinned: &[],
            bar_specs: &specs,
            fleet: crate::ui::bar::fleet::empty(),
            scroll_offsets: &mut offsets,
        };
        let text = render_to_text(&mut inputs, 120, 10);
        let lines: Vec<&str> = text.lines().collect();
        let reply_idx = lines
            .iter()
            .position(|l| l.contains("Reply to agent"))
            .expect("reply row");
        // Body region: rows 1..reply_idx (after the header, before the
        // reply). Each row carries exactly 2 separator glyphs (3
        // containers → 2 separators), but the glyph differs by row:
        //   - first row (top rule):    `┬`
        //   - middle rows (content):   `│`
        //   - last row (bottom rule):  `┴`
        let body_rows: Vec<&str> = lines[1..reply_idx].to_vec();
        assert!(body_rows.len() >= 3, "body region needs >= 3 rows");
        let last = body_rows.len() - 1;
        for (i, row) in body_rows.iter().enumerate() {
            let total = row
                .chars()
                .filter(|c| matches!(*c, '│' | '┬' | '┴'))
                .count();
            assert_eq!(total, 2, "expected 2 separator glyphs in row {i}: {row:?}");
            let expected = if i == 0 {
                '┬'
            } else if i == last {
                '┴'
            } else {
                '│'
            };
            let kind_count = row.chars().filter(|c| *c == expected).count();
            assert_eq!(kind_count, 2, "expected 2 `{expected}` in row {i}: {row:?}",);
        }
        // The top and bottom rule rows must still show plenty of `─`
        // (otherwise the horizontal rule wouldn't actually be drawn).
        let top_dashes = body_rows[0].chars().filter(|c| *c == '─').count();
        assert!(
            top_dashes >= 10,
            "top rule needs `─` glyphs: {:?}",
            body_rows[0]
        );
        let bot_dashes = body_rows[last].chars().filter(|c| *c == '─').count();
        assert!(
            bot_dashes >= 10,
            "bottom rule needs `─` glyphs: {:?}",
            body_rows[last]
        );
    }

    #[test]
    fn render_container_short_content_no_scrollbar() {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let reg = make_registry();
        let ids = vec!["processes".to_string()];
        let mut offset: u16 = 0;
        let (_store, repo, workspace) = seed_workspace();
        let theme = Theme::default();
        let ctx = crate::ui::detail_modules::DetailContext {
            repo: &repo,
            workspace: &workspace,
            events: None,
            recap: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            status: crate::ui::dashboard::status::Status::Idle,
            ago_secs: None,
            events_scanned: true,
            theme: &theme,
        };
        terminal
            .draw(|f| {
                let area = Rect {
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 10,
                };
                render_container(f, area, &ids, &ctx, &reg, &theme, &mut offset);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        // No column is reserved for a scrollbar, so no scrollbar track/thumb
        // glyph is drawn in the rightmost column (x=39). Content is free to
        // render there, so we assert the absence of scrollbar glyphs rather
        // than blankness.
        for y in 0..10 {
            let sym = buf[(39, y)].symbol();
            assert!(
                sym != "│" && sym != "█",
                "unexpected scrollbar glyph {sym:?} in rightmost column at row {y}"
            );
        }
    }

    #[test]
    fn render_container_tall_content_draws_no_scrollbar() {
        let backend = TestBackend::new(40, 4); // very short — forces overflow
        let mut terminal = Terminal::new(backend).unwrap();
        let reg = make_registry();
        // Stack two modules so the virtual line list exceeds 4 rows
        // (2 titles + at least 1 body line each + 1 gap = >= 5).
        let ids = vec!["processes".to_string(), "session_summary".to_string()];
        let mut offset: u16 = 0;
        let (_store, repo, workspace) = seed_workspace();
        let theme = Theme::default();
        let ctx = crate::ui::detail_modules::DetailContext {
            repo: &repo,
            workspace: &workspace,
            events: None,
            recap: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            status: crate::ui::dashboard::status::Status::Idle,
            ago_secs: None,
            events_scanned: true,
            theme: &theme,
        };
        terminal
            .draw(|f| {
                let area = Rect {
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 4,
                };
                render_container(f, area, &ids, &ctx, &reg, &theme, &mut offset);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        // Even though content overflows, no scrollbar is drawn. The bar would
        // have occupied the rightmost column (x=39), so we constrain the check
        // there rather than scanning the whole buffer — `│` legitimately
        // appears elsewhere in the UI.
        for y in 0..4 {
            let sym = buf[(39, y)].symbol();
            assert!(
                sym != "│" && sym != "█",
                "unexpected scrollbar glyph {sym:?} in rightmost column at row {y}"
            );
        }
    }

    #[test]
    fn render_container_clamps_offset_when_content_shrinks() {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let reg = make_registry();
        let ids = vec!["processes".to_string()];
        let mut offset: u16 = 50; // wildly past end
        let (_store, repo, workspace) = seed_workspace();
        let theme = Theme::default();
        let ctx = crate::ui::detail_modules::DetailContext {
            repo: &repo,
            workspace: &workspace,
            events: None,
            recap: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            status: crate::ui::dashboard::status::Status::Idle,
            ago_secs: None,
            events_scanned: true,
            theme: &theme,
        };
        terminal
            .draw(|f| {
                let area = Rect {
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 10,
                };
                render_container(f, area, &ids, &ctx, &reg, &theme, &mut offset);
            })
            .unwrap();
        // Title + 1 dash line = 2 rows total. Area is 10 rows. max_offset = 0.
        assert_eq!(offset, 0);
    }
}
