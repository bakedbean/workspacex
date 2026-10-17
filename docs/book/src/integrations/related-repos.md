# Related repos

Declare related repos when your work spans several repos that need to know about each other, such as a backend, a frontend and a marketing site. A workspace's agent can then read the related repos' source.

```bash
wsx repo set-related-repos backend frontend,marketing
wsx repo edit-related-repos backend      # edit the list in $EDITOR
```

The value is a comma-separated list of registered repo names. When a workspace in `backend` starts its agent, wsx:

- passes each related repo's source path with `--add-dir` (Claude Code and oh-my-pi);
- tells the agent to treat those paths as read-only;
- tells the agent that when a related repo needs changes, it should create a workspace there with `wsx workspace create`, hand the task to that workspace's agent with `wsx agent send`, and tell you which workspace owns it.

Each repo gets its own branch and pull request. Merge them in dependency order.

The read-only rule is an instruction, not a lock. Installing the wsx skill (see [Agent skills](../agents/skills.md)) reinforces it. Unknown repo names in the list are skipped.

See [`wsx repo set-related-repos`](../cli-reference/repo.md#wsx-repo-set-related-repos).
