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

## Measured

| Run | Time |
|---|---|
| Cold Windows cross-build (Bevy + our nine crates) | **3m 23s** at `-j 8` |
| Incremental Windows cross-build (our crates only) | **5.7s** |
| Test suite on Linux (`canlab-solver` + `canlab-poly`) | 136 passed, matching local |

The incremental number is the point of the exercise: the edit → build → run loop
is five seconds of server time, where the same link locally costs ten minutes and
can run the machine out of memory. `~/canlab-target` is what makes that true, so
it is a cache to keep, not to clean.

## Honest status

Working end to end. Six bugs were found by running it rather than reasoning about
it, and each is recorded in `scripts/remote.ps1` where it bit:

1. `status` piped its own output into `Out-Null`, so it printed nothing.
2. `setup` sourced `~/.cargo/env` under `set -e`; cargo is on the server's PATH
   without that file, so the first line aborted the whole action.
3. cargo writes progress to **stderr**, which PowerShell turns into an error
   record, so a working build reported exit 1.
4. Appending `2>&1` to a remote command redirects only its **last** command of an
   `&&` chain; it must wrap the whole thing: `{ … ; } 2>&1`.
5. bsdtar on Windows stamps `SCHILY.fflags`, which GNU tar warns about — the
   archive is made with `git archive` now, from the commit, which also means a
   remote failure names a revisable revision.
6. An earlier extraction left the checkout root at mode 0555, so the next one
   failed with `Cannot open: Permission denied` on whichever path sorted first
   (`.cargo/config.toml`), which read like a corrupt archive for three rounds.
   The unpack now `chmod -R u+w`s and passes `--no-same-permissions`.

Still missing: a preflight that compares the workspace's `rust-version` with the
server's `rustc` (the first cross-build failed on 1.96.0 vs a required 1.97.1),
and printing a failed command's log before throwing.
## Getting a build to your screen

`scripts/canlab.ps1` is the short path from the server's artefacts to a running
app, so nobody has to remember a target path:

```powershell
.\scripts\canlab.ps1 status    # server build (commit, time, size) and your copy
.\scripts\canlab.ps1 get       # copy the newest build here, never start it
.\scripts\canlab.ps1 run       # fetch if stale, then start it  (the default)
.\scripts\canlab.ps1 build     # rebuild on the server from the pushed commit, then fetch
```

It searches the three places a build can be (`~/canlab-target`, the tarball
checkout's `target`, and the workspace checkout's `target`) and takes the newest,
so it does not depend on which route produced the executable. Alongside the
executable it writes `target/remote/canlab-gui.build.json` recording the commit,
the build time and where it came from, which is what makes "the app on screen is
commit X" answerable later.

`status` also says when the build is **behind** the checkout: a build older than
the commit it came from is worth rebuilding, and that is a sentence, not a hash to
interpret.

Options: `-Force` copies even when the local file looks current, `-Wait` keeps the
terminal attached to the app's output instead of detaching it.