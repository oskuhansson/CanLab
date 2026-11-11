# Building on the server

The desktop this project is developed on has 6 cores, 7.6 GiB of RAM and builds
with `-j 1` because anything more exhausts commit charge. A machine with an RTX
3080, SSH and 16 GiB can do four things this one cannot do at a sane speed:

| Task | Here | There |
|---|---|---|
| Cold build of the Bevy tree | ~26 min, `-j 1` | minutes, `-j 8` |
| `cargo test --workspace` | one link at a time | parallel links |
| Headless frame tests | works (wgpu, offscreen) | works if a Vulkan adapter is visible; **the PNGs will not match Windows pixel for pixel**, so the committed baselines stay here |
| The light-transport work (W7) | an Intel UHD iGPU | the 3080, which is the whole point |

What it cannot do is show the window to you. So the division is: **the server
builds, tests, benchmarks and eventually runs the GPU kernels; this machine runs
the window you look at.**

## One-time setup on the server

```bash
# The same toolchain the workspace needs (edition 2024, so 1.85+; 1.99 here).
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
. "$HOME/.cargo/env"

# Build prerequisites for wgpu/bevy on a headless Linux box, Debian/Ubuntu names:
sudo apt install -y build-essential pkg-config libx11-dev libxkbcommon-dev \
  libwayland-dev libudev-dev libasound2-dev libvulkan1 vulkan-tools vulkan-validationlayers

# A checker: "llvmpipe" or "lavapipe" means no hardware Vulkan, which is fine for
# the library tests and enough for the frame tests at a few seconds per image.
vulkaninfo --summary | head -20
```

## Running it

`scripts/remote.ps1` sends the working tree — committed *and* uncommitted, which
matters while a change is in flight — and runs the tests there. It uses only what
Windows 11 ships: `tar.exe`, `scp.exe`, `ssh.exe`.

```powershell
.\scripts\remote.ps1 -HostName canlab.example -User oskuh -RemotePath /home/oskuh/canlab
.\scripts\remote.ps1 -HostName ... -Filter "-p canlab-core -p canlab-solver"
.\scripts\remote.ps1 -HostName ... -Bench          # also writes docs/bench.md there
```

The first run installs nothing; it assumes `cargo` is on the server's `PATH`.

## Why a tarball and not `rsync` or a clone

- **Not a clone**: the tree is usually dirty, and a clone would test the last
  commit instead of the working copy.
- **Not `rsync`**: Windows does not ship it, and asking someone to install it to
  save a few seconds of transfer is a bad trade for a 1.2 MB tarball without
  `target/`.

## What stays local, on purpose

- Opening the window (it has to be in front of you).
- The committed snapshot baselines: rasterisation differs between drivers, so a
  Linux image would fail a Windows comparison. If the server becomes the main
  test box, add a second baseline directory for it rather than loosening the
  tolerance.
- Anything that needs `%APPDATA%` or the user's session file.
