Pick a color theme with:

```
wsx config set theme dracula
wsx config set theme jellybeans
wsx config set theme nord
wsx config set theme default
```

Themes affect repo headers, the selected row, sub-line dimming, and the
error modal. The state indicators (status dots, activity labels, attention
marks) are not yet per-state coloured — that's a planned follow-up.

The `default` theme uses ANSI-named colors that adapt to your terminal's
palette. `dracula`, `jellybeans`, and `nord` are fixed RGB palettes.

Restart wsx after changing — themes are loaded once at startup.
