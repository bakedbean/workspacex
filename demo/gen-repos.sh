#!/usr/bin/env bash
# Generate small synthetic repos with deliberately planted, reviewable bugs.
# Usage: gen-repos.sh <dest-dir>
set -euo pipefail
DEST="${1:?usage: gen-repos.sh <dest-dir>}"
mkdir -p "$DEST"

init_repo() { # <path>
  git -C "$1" init -q
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

echo "generated repos in $DEST"
