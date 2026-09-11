#!/usr/bin/env bash
# Generate small synthetic repos with deliberately planted, reviewable bugs.
# Usage: gen-repos.sh <dest-dir>
set -euo pipefail
DEST="${1:?usage: gen-repos.sh <dest-dir>}"
mkdir -p "$DEST"

init_repo() { # <path>
  # Default branch `main` so wsx's base-branch diff (which defaults to `main`)
  # resolves — this is what powers the RECENT FILES `+X −Y` line counts.
  git -C "$1" init -q -b main
  git -C "$1" config user.email demo@wsx.dev
  git -C "$1" config user.name "wsx demo"
}

# --- toy-api: a tiny Flask-style service with planted security bugs ---
API="$DEST/toy-api"; mkdir -p "$API/src"
cat > "$API/src/auth.py" <<'PY'
import sqlite3


def login(username, password):
    # BUG: SQL injection — username/password interpolated into the query.
    q = f"SELECT * FROM users WHERE name='{username}' AND pw='{password}'"
    return sqlite3.connect("app.db").execute(q).fetchone()


def is_admin(token):
    # BUG: auth bypass — any non-empty token is treated as admin.
    return bool(token)
PY
cat > "$API/src/app.py" <<'PY'
from src.auth import login, is_admin


def handle(req):
    user = login(req["user"], req["pw"])
    # BUG: unhandled None — login() returns None on bad creds, then .id crashes.
    return {"id": user.id, "admin": is_admin(req.get("token"))}
PY
cat > "$API/README.md" <<'MD'
# toy-api
A minimal example service used for wsx demo recordings.
MD
init_repo "$API"
git -C "$API" add -A
git -C "$API" commit -qm "feat: initial toy-api service"

# --- toy-cli: a small CLI with planted correctness/resource bugs ---
CLI="$DEST/toy-cli"; mkdir -p "$CLI/src"
cat > "$CLI/src/main.py" <<'PY'
import sys


def parse_args(argv):
    # BUG: off-by-one — skips the first real argument.
    return argv[2:]


def read_config(path):
    # BUG: file handle leaked — never closed.
    f = open(path)
    return f.read()


def main():
    args = parse_args(sys.argv)
    print(read_config(args[0]))


if __name__ == "__main__":
    main()
PY
cat > "$CLI/README.md" <<'MD'
# toy-cli
A minimal example CLI used for wsx demo recordings.
MD
init_repo "$CLI"
git -C "$CLI" add -A
git -C "$CLI" commit -qm "feat: initial toy-cli"

# --- toy-web: a tiny static front-end with planted UI/perf bugs ---
WEB="$DEST/toy-web"; mkdir -p "$WEB/src"
cat > "$WEB/src/app.js" <<'JS'
export function validateForm(form) {
  // BUG: empty email passes — only checks for "@", not for a non-empty local part.
  return form.email.includes("@");
}

export function renderList(items, el) {
  // BUG: O(n^2) DOM churn — re-renders the whole list on every item.
  for (const item of items) {
    el.innerHTML += `<li>${item}</li>`;
  }
}

export function applyTheme(theme) {
  // BUG: dark mode never persists — preference is not written to storage.
  document.body.dataset.theme = theme;
}
JS
cat > "$WEB/src/index.html" <<'HTML'
<!doctype html>
<title>toy-web</title>
<ul id="list"></ul>
<script type="module" src="./app.js"></script>
HTML
cat > "$WEB/README.md" <<'MD'
# toy-web
A minimal example front-end used for wsx demo recordings.
MD
init_repo "$WEB"
git -C "$WEB" add -A
git -C "$WEB" commit -qm "feat: initial toy-web"

echo "generated repos in $DEST"
