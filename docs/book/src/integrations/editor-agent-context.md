# Editor-hosted agents

An AI agent inside your editor (magenta.nvim, Cursor, a VS Code extension) doesn't know what wsx knows about the workspace. `wsx context` writes that knowledge to one markdown file your editor can hand to its agent, and the agent reports back to the workspace's primary agent with `wsx agent send`.

## The contract

1. **Write the digest.** Run `wsx context write` from the worktree. It prints the file's path and exits 0. Outside a wsx worktree it exits non-zero, so an integration can be installed globally and stay inert elsewhere.

   ```bash
   wsx context show     # print the digest
   wsx context write    # write it to a file and print the path
   ```

2. **Refresh it.** Your editor decides when to rerun `wsx context write`. Good triggers are editor start, window focus, and opening the agent's chat panel. Guard against overlapping runs on rapid focus events.
3. **Hand it to the agent.** Add the path through the editor's context mechanism. If the agent snapshots files when they're added, re-add the file after each refresh.
4. **Report back.** The digest tells the agent to report with `wsx agent send <primary> "<summary>"`, run from the worktree. The message arrives at the primary agent without a sender label.

The file lives at `$XDG_STATE_HOME/wsx/context/<repo>/<workspace>.md` and is readable only by you. It can quote the primary agent's last message, so don't commit or share it. The path follows the workspace name, so it changes after a rename.

See [`wsx context`](../cli-reference/status-recap-context.md#wsx-context-show) and [`wsx agent send`](../cli-reference/agent.md#wsx-agent-send).

## What the digest contains

- Repo, workspace, branch, base ref and worktree path.
- Attached agents, with the primary marked.
- The last status the primary agent pushed.
- The recap: goal, state and next step.
- Up to 20 commits ahead of the base, and a count of uncommitted changes.
- The primary agent's last message, up to 2,000 characters.
- **External instructions** for the editor-hosted agent: check `git status` and `git diff` before editing, keep edits small, leave branches, status and recap to the primary agent, and report back with `wsx agent send`.

Sections with no data are left out. wsx-spawned agents are told that an editor-hosted agent may share their worktree and that its messages arrive unlabeled.

## neovim with magenta.nvim

magenta.nvim re-reads its context files before each request, so a digest that wsx keeps fresh stays live. Add this to your neovim config:

```lua
local worktrees = vim.fn.expand("~/.local/state/wsx/worktrees/")
local added_path, in_flight = nil, false

local function magenta_visible()
  for _, win in ipairs(vim.api.nvim_list_wins()) do
    local name = vim.api.nvim_buf_get_name(vim.api.nvim_win_get_buf(win))
    if name:find("Magenta Input", 1, true) then return true end
  end
  return false
end

local function wsx_context()
  if in_flight or not vim.startswith(vim.fn.getcwd(), worktrees) then return end
  in_flight = true
  vim.system({ "wsx", "context", "write" }, { text = true }, function(out)
    in_flight = false
    if out.code ~= 0 then return end
    local path = vim.trim(out.stdout)
    if path == "" or path == added_path then return end
    vim.schedule(function()
      if magenta_visible() then
        vim.cmd("Magenta context-files " .. vim.fn.fnameescape(path))
        added_path = path
      end
    end)
  end)
end

vim.api.nvim_create_autocmd({ "VimEnter", "FocusGained" }, { callback = wsx_context })
vim.api.nvim_create_autocmd("BufWinEnter", {
  callback = function(ev)
    if vim.api.nvim_buf_get_name(ev.buf):find("Magenta Input", 1, true) then
      added_path = nil
      wsx_context()
    end
  end,
})
vim.api.nvim_create_user_command("WsxContext", function()
  added_path = nil
  wsx_context()
end, {})
```

- The digest refreshes on start and on focus, and attaches when you open the magenta sidebar or start a new thread.
- The path is added only while the sidebar is open, because `:Magenta context-files` opens it otherwise.
- `:WsxContext` forces a refresh and re-add.
- If you set `$XDG_STATE_HOME`, change `worktrees` to match.

## Other editors

Any agent that can read a file can use the digest. Run `wsx context write` on the editor's start and focus events, ignore a non-zero exit, and pass the printed path to the agent: through a context-file API, or a rules file that tells the agent to read that path. The External instructions block handles the rest.
