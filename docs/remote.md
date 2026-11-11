# The build server

The machine this repository is developed on builds with `-j 1`. That is not a
preference: a clean Bevy build needs more memory than it has, and `target/` has
reached 42 GiB. The server on the tailnet exists to take the expensive half.

```powershell
.\scripts\remote.ps1 status
.\scripts\remote.ps1 test -Package canlab-solver, canlab-poly
.\scripts\remote.ps1 build
.\scripts\remote.ps1 snapshots
```

## The address

`oskar@100.88.227.20:2220` — the `ai` node's **tailnet** address. It is the one
that works: `ai.ragnarhansson.com:2220`, which is what `~/.ssh/config` says, does
not resolve to a reachable address from this network, and port 22 refuses
connections. If you want the friendly name back, point that hostname at the
tailnet address rather than the other way round.

The key is `~/.ssh/id_ed25519`. To authorise it, once, from this machine and with
the server's password:

```powershell
ssh-copy-id -p 2220 -i ~/.ssh/id_ed25519.pub oskar@100.88.227.20
```

`$env:CANLAB_REMOTE` overrides the address without editing the script.

## What the server is for

| Action | Why it belongs there |
|---|---|
| `test` | The pure-Rust crates (solver, poly, input, core, corpus, plot) compile with the cores the server has. Release by default: slower to compile once, much faster to run. |
| `build` | `cargo xwin build` produces the **Windows** executable on Linux, so the ten-minute Bevy link is not paid here. The exe comes back to `target/remote/canlab-gui.exe`. |
| `snapshots` | `egui_kittest` renders through `wgpu` with no window; on the server that is the 3080 rather than the integrated adapter. Frames land in `target/remote/snapshots/`. |
| `setup` | `rustup target add x86_64-pc-windows-msvc` and `cargo install cargo-xwin`, once. |

## How it gets the code there

A tarball, not a clone: the working tree may hold work that is not committed, and
a clone would need the server to authenticate to GitHub. `target/`, `.git/` and
`.obsidian/` are excluded. The checkout lives at `~/canlab-remote` and is
replaced on every sync.

The **build directory is shared** — `~/canlab-target`, via `CARGO_TARGET_DIR` — so
the second run reuses the first one's objects. That is the whole point: without a
warm cache the server is a slower copy of this machine.

There is no `--offline` on the server. Its registry cache is its own, and the
first build needs crates.io; the local `--offline` flag exists because this
machine's shell cannot complete a TLS handshake, which is not true there.

## Honest status

Written and parse-checked, and `remote.ps1 status` reaches the sshd and is
refused by key authentication, so the transport is right and the authorisation is
the one step left. Nothing has been built on the server yet.
