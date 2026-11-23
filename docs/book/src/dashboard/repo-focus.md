# Repo focus view

The focus view shows one repo on the dashboard and gives each of its workspaces several lines instead of one, laid out to be read at a glance. Workspaces are grouped into sections by pull request state, each row is a strip of fixed columns that line up down the view, and a [side panel](#side-panel) on the right shows the selected workspace's next step, goal, first prompt, commits or changed files in full. Every other workspace has a faint background tint so each reads as one block.

```text
◂ all repos / workspacex  ~/src/workspacex  ──  No PR 1 › Draft 1 › Open 1 › Done 1    ? 1 question   ⠋ 1 thinking   · 2 idle    4 ws

▾ No PR · 1                                                                                  │  Prompt  Goal  Next  Commits 4  Files 7  Chat
──────────────────────────────────────────────────────────────────────────────────────────── │ bakedbean/repo-focus-view vs main
▎▎ ⠋ bakedbean/try-new-parser     no PR          ▸ working  ● 2p  ↑1 vs main  not pushed  2m │ ─────────────────────────────────────
 ▎   ▎ claude  opus 4.8 61k/200k  #spike                                                     │ write the docs, then open the PR
                                                                                             │
▾ Draft · 1 · ? 1 asking                                                                     │ state
──────────────────────────────────────────────────────────────────────────────────────────── │ implemented sections and the strip;
▎▎ ? bakedbean/detail-bar-scroll  ⏷ #459 draft   └ asking         ↑2 vs main  ██▎ +61 −8 14m │ tests are green
 ▎   ▎ claude                                                                                │
                                                                                             │ Recap set 5h ago; the workspace has
▾ Open · 1                                                                                   │ kept working since.
──────────────────────────────────────────────────────────────────────────────────────────── │
▍▍ · bakedbean/repo-focus-view    ⏺ #461 open ✓  ▸ idle           ↑4 vs main ███ +184 −22 3h │
 ▍   part    3 · Repo focus view  #455 ⏺  #458 ⏺                                             │
 ▍   ▎ claude  opus 4.8 172k/200k  #ui                                                       │
                                                                                             │
▾ Done · 1                                                                                   │
──────────────────────────────────────────────────────────────────────────────────────────── │
▎▎ ✓ bakedbean/fix-flaky-poller   ⏺ #455 merged  ▸ done           ↑0 vs main             48h │
 ▎   ▎ claude                                                                                │
```


The header line sums the repo up by PR stage (`No PR 1 › Draft 1 › Open 1 › Done 1`, each stage in its section's color and dim when empty) before the status counts. On a narrow terminal the stages drop first, then the path. Click `◂ all repos` to leave the view, or a status in the tally to filter by it. With a [bar theme](../configuration/themes.md#the-focus-header), the `[dashboard_focus]` bar lays this line out, for example as powerline blocks.

## Sections

A header line (`▾ Open · 2`, with the workspace count, in the section's color) underlined by a full-width rule starts each section, as on the main dashboard's repo headers. After the count it tallies the section's workspaces that need a look, each in its own color: those asking a question (`? 1 asking`), those stalled (`! 1 stalled`), and open PRs that conflict with their base (`⏺ 1 conflict`). Nothing is added when there are none.

| Section | Holds |
| --- | --- |
| **No PR** | Workspaces with no PR, or whose PR state isn't known yet |
| **Draft** | Draft PRs |
| **Open** | Open PRs, including those with merge conflicts |
| **Done** | Merged and closed PRs |

Sections with no workspaces are left out. Within a section the dashboard's sort applies, but the focus view doesn't pin workspaces that just started asking or stalled to the top, as the list does; the sections already order the view by PR state. Hidden groups (see [Filters and hiding](#filters-and-hiding)) come after all the sections, as they are on the list.

Sections fold like the list's repo headers. `j` and `k` stop on each header; `h` on it folds the section and `l` or `enter` unfolds it. A folded header shows `▸` and keeps its count and tallies, and `j` / `k` step past its hidden rows. `h` on a workspace folds the section it's in and moves the selection to the header. `z z` toggles the selected section, and `z M` / `z R` fold and unfold every section of the repo. Clicking a header toggles it too. Folds are kept per repo until you quit wsx; turning the sections off with `G` sets them aside (and `z M` / `z R` do nothing) until you turn them back on.

Press `G` to turn the sections off and see the workspaces in the list's own order instead, with no headers and with workspaces that just started asking or stalled pinned to the top again. `G` again brings the sections back. The choice is saved (the `dashboard_focus_group_by_pr` [setting](../configuration/settings.md#dashboard)). Without sections, `{` goes to the first workspace and `}` to the hidden groups.

## The row

Each row has three zones.

**The strip** is line 1: columns that are as wide as the widest value in the view, so each field lines up down the screen. Left to right:

- the agent strip and the status glyph (a spinner while the agent works),
- the **branch**, at its full length: the column is as wide as the longest branch name, so names never wrap (only a pane too narrow to fit one beside the PR chip wraps it),
- the **PR chip**, as on the dashboard list's PR column: number, state, the review mark with its unresolved-thread count, and the comment count. A workspace without a PR shows a dim `no PR`,
- the **status word**, as on the dashboard row, with the running-process count and any badge,
- commits ahead of the repo's base branch (`↑4 vs main`, from the repo's **Base branch** setting; `↑4 base` when it isn't set),
- where the branch stands against its remote: `⇡2 unpushed` for commits not pushed yet and `⇣3 behind` for commits on the remote the workspace doesn't have, or a dim `not pushed` for a branch with commits but no upstream and no PR. Nothing when it is in sync,
- uncommitted changes (`3 mod 1 untr`; nothing when the tree is clean),
- the diff, led by a bar sized against the largest diff in the view, green for the share of lines added and red for those removed. The scale is logarithmic, so a small change still shows next to a large one,
- how long ago the workspace was active, toned by hour range so quiet workspaces fade: bright under 1h, plain under 6h, dim under 24h, and faintest from 24h on.

**The part line** sits under the branch for a workspace in a [series](../cli-reference/workspace.md#wsx-series-show): its part number and the series title (part 1's slug until it has one), then the earlier parts' PRs, oldest first, each with its PR chip's glyph and color, or the slug of a part with no PR. When a later part exists, the line ends with `→ part N`. It always fits on one line: the oldest PRs fold into `+k` first, then the title is cut.

What each workspace is for and doing is in the [side panel](#side-panel), for the selected one. With the panel closed, or on a terminal too narrow for it (under 100 columns), each row shows one line of the panel tab's text under its branch instead: `prompt` (cut to two lines), `goal` or `next` (up to three), with `next` standing in for the Commits, Files and Chat tabs. Without a recap, `next` gives way to a `last` line, the agent's last message.

**The footer** is a dim line of the workspace's agents, each named with its icon from the theme's [`[agent_bar.symbols]`](../configuration/themes.md) table (or the `▎` bar when the theme has none), then the agent's model and how full its context window is (`opus 4.8 45k/200k`, the fill in the warn color from 85%; just the token count when the window size isn't known), as the attached view's top bar shows them. That appears once the agent's session log reports token usage. Then come `◇ shared` for a [shared](../integrations/shared-workspaces.md) workspace (red `shared, no session` when its tmux session is gone) and `layout` when it has a saved multi-pane layout, followed by its tags. It is always shown.

When the strip doesn't fit on one line, it breaks up. Line 1 keeps the glyph, branch, PR chip and age; the status, commits ahead, remote state, uncommitted changes and diff move under the branch, usually onto one line, more when the terminal is narrow. Every row breaks at the same points, so the columns still line up. A field wider than the whole line, such as a very long base branch name, is cut short with `…`.

In the focus view the [detail bar](detail-bar.md) shrinks to its reply row, so the workspaces get nearly the full height. Press `Tab` to type a message to the selected workspace's agent, as you would on the list.

## Side panel

The [side panel](side-panel.md) is open when you enter the focus view, on the selected workspace, and follows the selection. It starts on **Next**, so you can step through workspaces with `j` / `k` and read what each will do next; the rows stay one strip and a footer each.

| Tab | Shows |
| --- | --- |
| **Prompt** | The first prompt the workspace's agent was given, in full, once the agent's session log has been read. |
| **Goal** | The recap goal, in full. |
| **Next** | The recap's next step, then its state. When the workspace kept working for at least half an hour after the recap was last set, a note says how old the recap is, since it may no longer say what the workspace is doing. Without a recap, the agent's last message, unless that is just the initial prompt. |
| **Commits** | The branch's commits against the repo's base branch. |
| **Files** | The files the branch changed against its base. |
| **Chat** | The agent's latest message, rendered as markdown: the same text as the detail bar's RECENT CHAT, shown whether or not the workspace has a recap. |

Press `1` to `6` for a tab, `<` / `>` for the previous or next one, or click it. The focus view keeps its own tab, apart from the list's panel, and saves it (the `dashboard_focus_tab` [setting](../configuration/settings.md#dashboard)). Nothing is cut short: a long prompt or recap scrolls. Press `→` to give the panel the keyboard, then `j` / `k` (or `PgDn` / `PgUp`, `g` / `G`) scroll it; `←` or `esc` gives the keys back. The mouse wheel over the panel scrolls it too.

`c` closes and reopens it, as on the list; `esc` closes it before it leaves the focus view. Closed, it stays closed as `[` / `]` move between repos, and opens again the next time you enter the focus view. The focus view's panel opens and closes apart from the list's. It hides on terminals under 100 columns wide.

## Opening and closing it

- Press `f` on a repo header, or on any workspace to focus that workspace's repo. Click a repo header's name to do the same.
- Press `f` again, press `esc`, or click `◂ all repos` to go back to every repo. While a `/` filter or status filter is set, `esc` clears that first.
- `[` and `]` move to the previous and next repo in the dashboard's repo order.

## Moving around

The focus view moves like the list: `j` / `k` (or the arrows) select the next or previous workspace, and `enter`, `i` or `l` attaches. `}` jumps to the next section's header and `{` to the previous one's. Pressed in the middle of a section, `{` goes to the current section's header first. At the first and last section they do nothing. Hidden groups count as a section of their own. A jump, or `j` / `k` onto a section's first workspace, scrolls its header into view. Clicking anywhere on a workspace attaches to it, and clicking its PR chip opens the PR. `n` creates a workspace in the focused repo.

The view keeps the selected workspace on screen. To read a workspace taller than the screen, page with `PgDn` / `PgUp` or scroll with the mouse wheel; the view stays put until you select a different workspace. The other workspace keys (`d`, `e`, `#`, `?`, and so on) act on the selected workspace as usual.

## Filters and hiding

The `/` filter and the [status filter](status-indicators.md#filtering-by-status) narrow the view the same way they narrow the list. Workspaces a [hide rule](tags-and-hiding.md#hiding-workspaces) hides collapse into a summary row per rule after the other workspaces (`▸ #parked  ·  2 hidden`), as they do under the repo's header on the list, rather than being sorted into the sections. `enter` or `l` on the row, or a click, shows its workspaces under it; `h` on the row or on one of them hides them again. A group left open in one view is open in the other. A `/` filter shows hidden workspaces among the others, as it does on the list.

The focus view lasts until you leave it or quit wsx.
