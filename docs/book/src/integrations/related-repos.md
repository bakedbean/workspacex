When you work across multiple repos that need to know about each other (a backend, a frontend, a marketing site), declare related repos per primary repo:

```bash
wsx repo set-related-repos backend frontend,marketing
```

When you spawn a workspace in `backend`, wsx invokes claude with `--add-dir` pointing at each related repo's source path. Claude can read, grep, and reference files in those directories freely.

To prevent claude from accidentally editing files in the source paths of related repos (which would land changes on whatever branch the source is on), wsx also appends a system-prompt instruction telling claude:

- Treat those directories as read-only.
- If changes are needed there, drive `wsx workspace create <other-repo> --name <slug>` from this session, `cd` into the new worktree path (`wsx workspace path <other-repo> <slug>`), and make the changes there. Each repo gets its own branch and PR; cross-link them and merge in dependency order.

This is a soft guard, not a tool-level lock — it relies on claude following the instruction. The same trust model as `custom_instructions`. Installing the bundled wsx skill (`wsx setup install-skill`, see [Agent skill](agent-skill.md)) reinforces this with the full CLI vocabulary and slug-naming rules.

Unknown names in the list (e.g. a repo you renamed or unregistered) are logged and skipped at spawn time; the spawn still proceeds with the recognized names.
