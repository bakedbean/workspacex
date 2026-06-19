```bash
cargo test -- --test-threads=1
```

The test suite substitutes `claude` with `cat` via `WSX_CLAUDE_BIN`, so it runs without Claude Code installed. `--test-threads=1` is required because several tests mutate `WSX_CLAUDE_BIN` and `HOME`.
