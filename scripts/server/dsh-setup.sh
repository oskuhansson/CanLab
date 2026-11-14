#!/usr/bin/env bash
# Put the DSH web UI on the tailnet, and keep it there.
#
# The server has an existing DSH install, launched as `ori dsh` (a wrapper around
# ~/.local/bin/dsh from @deepseek-ai/dsh), with one profile `web` whose bundles
# are already installed. This script does three things and is safe to run twice:
#
#   1. clones (or updates) the CanLab checkout in the server workspace;
#   2. writes a systemd *user* unit that serves the profile on the tailnet
#      address, with "--no-open" (there is no browser there) and a trusted host
#      so the /api browser-trust fence accepts the authority it is reached by;
#   3. enables lingering, so the service runs whether or not anyone is logged in.
#
# It prints the URL and the state of the unit. Nothing here grants access beyond
# the tailnet: the bind address is a concrete tailnet literal, which is why the
# user chose "tailnet-only, no token".
set -euo pipefail

WORKSPACE=/home/oskar/Documents/deepseek-harness/default-workspace
REPO=https://github.com/oskuhansson/CanLab.git
CHECKOUT="$WORKSPACE/cas"
PROFILE=web
UNIT=dsh-web.service

say() { printf '%s\n' "$*"; }

# --- 1. the checkout -------------------------------------------------------
mkdir -p "$WORKSPACE"
if [ -d "$CHECKOUT/.git" ]; then
  git -C "$CHECKOUT" fetch -q origin
  git -C "$CHECKOUT" checkout -q -f main
  git -C "$CHECKOUT" reset -q --hard origin/main
else
  git clone -q "$REPO" "$CHECKOUT"
fi
say "checkout: $(git -C "$CHECKOUT" log --oneline -1)"

# --- 2. the tailnet address and a free port --------------------------------
TAILNET=$(bash -lc 'tailscale ip -4' | grep -E '^100\.' | head -n1)
if [ -z "$TAILNET" ]; then
  say "no tailnet address found; is tailscale running?" >&2
  exit 1
fi

PORT=""
for candidate in $(seq 8600 8699); do
  if ! ss -ltn "sport = :$candidate" | grep -q LISTEN; then
    PORT="$candidate"
    break
  fi
done
if [ -z "$PORT" ]; then
  say "no free port in 8600-8699" >&2
  exit 1
fi
say "tailnet: $TAILNET:$PORT"

# --- 3. the unit ----------------------------------------------------------
UNIT_DIR="$HOME/.config/systemd/user"
mkdir -p "$UNIT_DIR"
cat >"$UNIT_DIR/$UNIT" <<UNITFILE
[Unit]
Description=DSH web UI for CanLab on the tailnet
After=network-online.target

[Service]
Type=simple
WorkingDirectory=$CHECKOUT
# The wrapper is a login-shell thing: ~/.local/bin is not on a non-login PATH.
ExecStart=/bin/bash -lc 'exec ori dsh --host $TAILNET --port $PORT --no-open --trusted-host $TAILNET:$PORT'
Restart=always
RestartSec=3
# The profile reads and writes under \$HOME/.dsh; a user unit already has that,
# but be explicit so a future hardening pass cannot silently break it.
Environment=HOME=$HOME

[Install]
WantedBy=default.target
UNITFILE

systemctl --user daemon-reload
systemctl --user enable "$UNIT" >/dev/null
systemctl --user restart "$UNIT"
loginctl enable-linger "$(id -un)" >/dev/null 2>&1 || true

# --- 4. report ------------------------------------------------------------
sleep 3
systemctl --user is-active "$UNIT" || true
TOKEN=$(journalctl --user -u "$UNIT" -n 60 --no-pager 2>/dev/null | grep -o "?token=[A-Za-z0-9_-]*" | tail -1 || true)
say "url: http://$TAILNET:$PORT/$TOKEN"
say "unit: $UNIT_DIR/$UNIT"
say "logs: journalctl --user -u $UNIT -n 40 --no-pager"
