# Flipclock

A realistic 3D split-flap desktop clock for Windows, Linux and macOS, written in Rust.

> **Status: proof of concept.** See [`SPEC.md`](SPEC.md) for the full specification, [`GLOSSARY.md`](GLOSSARY.md) for terminology and [`docs/adr/`](docs/adr/) for design decisions.

- [Build on Windows](#build-on-windows)
- [Build on Linux](#build-on-linux)
- [Build on macOS](#build-on-macos)
- [Run the unit tests](#run-the-unit-tests)
- [Test checklist](#test-checklist)
- [Known gaps](#known-gaps)
- [Reporting a problem](#reporting-a-problem)

All commands below are run from the **root folder of the source** (the folder that contains this `README.md` and `Cargo.toml`). The first build downloads and compiles all dependencies and takes several minutes; later builds are much faster.

---

## Build on Windows

Reference platform: Windows 11 x64.

### 1. Install the prerequisites (once)

1. **Visual Studio C++ build tools.** Install either *Visual Studio Community* or *Build Tools for Visual Studio* from <https://visualstudio.microsoft.com/downloads/>. In the installer, select the workload **"Desktop development with C++"** and make sure these two components are ticked under *Installation details*:
   - **MSVC Build Tools for x64/x86 (latest)**
   - **Windows 11 SDK** (any recent version)
2. **Rust.** Download and run `rustup-init.exe` from <https://rustup.rs> and accept the defaults (toolchain `stable-x86_64-pc-windows-msvc`).
3. Open a **new** terminal (PowerShell or Command Prompt) and check:
   ```powershell
   rustc --version
   cargo --version
   ```
   Both must print a version number.

### 2. Build

```powershell
cargo build --release
```

### 3. Run

The result is a single file:

```
target\release\flipclock.exe
```

Double-click it, or run `.\target\release\flipclock.exe`. You can copy `flipclock.exe` to any **writable** folder; `config.toml` is created next to it.

---

## Build on Linux

Reference platform: Ubuntu 24.04 x86_64. The app runs on both X11 and Wayland sessions (under Wayland it runs through XWayland, by design — see ADR-0002).

### 1. Install the system packages (once)

Only the ALSA headers are needed to compile; the other packages are needed to run and are already present on most desktop installs.

**Ubuntu / Debian:**

```bash
sudo apt update
sudo apt install -y build-essential pkg-config curl libasound2-dev \
  libx11-6 libxcursor1 libxrandr2 libxi6 libxkbcommon-x11-0 \
  libvulkan1 mesa-vulkan-drivers
```

**Fedora:**

```bash
sudo dnf install -y gcc pkgconf-pkg-config curl alsa-lib-devel \
  libX11 libXcursor libXrandr libXi libxkbcommon-x11 \
  vulkan-loader mesa-vulkan-drivers
```

**Tray icon on GNOME:** the tray icon uses the StatusNotifierItem protocol, which GNOME does not show by default. Ubuntu ships the AppIndicator extension already enabled. KDE, Xfce and most other desktops support it out of the box. On Fedora and other GNOME distributions, install and enable it to get the tray icon (otherwise the app still works, but minimizing goes to the taskbar instead of the tray):

```bash
sudo dnf install -y gnome-shell-extension-appindicator   # Fedora
# then log out and back in, and enable "AppIndicator and KStatusNotifierItem Support" in the Extensions app
```

### 2. Install Rust (once)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# accept the defaults, then:
source "$HOME/.cargo/env"
rustc --version
cargo --version
```

### 3. Build

```bash
cargo build --release
```

### 4. Run

```bash
./target/release/flipclock
```

You can copy the `flipclock` binary to any **writable** folder; `config.toml` is created next to it.

### Running under WSL (Windows 11)

WSL2 with WSLg can build and display the Linux version. Install the Ubuntu packages above inside WSL and follow the same steps. Notes:
- Graphics may fall back to software rendering (llvmpipe), so it can be slower than on real Linux.
- Sound requires WSLg's PulseAudio bridge; if no audio device is found, the app runs silently.
- Treat WSL as a convenience for development, not as a reference Linux test.

---

## Build on macOS

> **Not yet verified by the author** (no Mac available). If you are testing on a Mac, please follow these steps exactly and report anything that doesn't match — see [Reporting a problem](#reporting-a-problem).

Requires macOS 12 or newer, on Apple Silicon (M1/M2/M3/…) or Intel.

### 1. Install the prerequisites (once)

1. **Xcode Command Line Tools.** Open *Terminal* and run:
   ```bash
   xcode-select --install
   ```
   Click *Install* in the dialog that appears. (If it says the tools are already installed, that's fine.)
2. **Rust:**
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   # accept the defaults, then:
   source "$HOME/.cargo/env"
   rustc --version
   cargo --version
   ```
3. **cargo-bundle** (creates the `.app`):
   ```bash
   cargo install cargo-bundle
   ```

### 2. Build the app

```bash
cd crates/flipclock-app
cargo bundle --release
cd ../..
```

The result is:

```
target/release/bundle/osx/Flipclock.app
```

### 3. Run

1. Copy `Flipclock.app` to a folder **you own and can write to**, for example a new folder `~/Flipclock/`:
   ```bash
   mkdir -p ~/Flipclock
   cp -R target/release/bundle/osx/Flipclock.app ~/Flipclock/
   open ~/Flipclock/Flipclock.app
   ```
2. `config.toml` is created **next to** `Flipclock.app` (in `~/Flipclock/`), not inside it.

The app is **not signed**. When you build it yourself on your Mac it opens normally. If instead you received a `Flipclock.app` someone else built (for example as a zip), macOS will block it; in that case either right-click the app → **Open** → **Open**, or run:

```bash
xattr -dr com.apple.quarantine ~/Flipclock/Flipclock.app
```

If the app shows a warning saying settings cannot be saved, the folder is not writable (or macOS is running the app from a temporary read-only location): move the app into a folder like `~/Flipclock/` as shown above.

macOS shortcuts: **⌘B** borderless, **⌃⌘F** or **F11** fullscreen, **⌘Q** quit. The clock icon lives in the **menu bar** (top right), not in the Dock, while the window is hidden.

---

## Run the unit tests

On any platform, from the source root:

```bash
cargo test --workspace
```

The tests cover the clock logic (digit sequences, rolls, cascade, ticks, date formatting, config, window placement). The renderer and UI are checked manually with the checklist below.

---

## Test checklist

Please go through this list on your platform and note anything that doesn't behave as described. Words in **bold** are defined in [`GLOSSARY.md`](GLOSSARY.md).

**Clock**
- [ ] Six **tiles** show the current time `HH:mm:ss`, with two lit dots as **separators**.
- [ ] Every second the rightmost tile **flips** (top **flap** rotates down, bounces, settles) and you hear a **tick**.
- [ ] At a minute change (`:59 → :00`) two tiles flip, right to left (**cascade**), with two ticks.
- [ ] Change the system time forward by a few hours: affected tiles **roll** quickly through the intermediate digits, with a tick per flap, then settle on the right time.
- [ ] The **date line** shows e.g. `Giovedì, 15 Ottobre 2026` and updates when the system date changes.

**Window**
- [ ] Resize the window: everything scales; it can't go below 480×270.
- [ ] **F11** (and double-click on the clock) toggles fullscreen. On macOS also ⌃⌘F.
- [ ] **Ctrl+B** (⌘B on macOS) toggles borderless. When borderless: drag anywhere to move, resize from the edges, double-click still toggles fullscreen.
- [ ] Always-on-top works (settings panel or tray menu).
- [ ] Close and reopen: position, size, borderless and always-on-top are restored; fullscreen is **not**.
- [ ] Launch the app a second time while it's running: the existing window comes to the front, no second clock appears.

**Tray / menu bar**
- [ ] The tray (menu bar on macOS) icon is present; its menu has Show/Hide, Settings, Always on top, Borderless, Mute tick, Exit.
- [ ] Double-clicking the tray icon (single click on Linux) shows/hides the window.
- [ ] Minimize hides the window to the tray, and no ticks are heard while hidden. Show it again: the time is correct immediately.
- [ ] Close (X / red button) exits the app.

**Settings panel**
- [ ] The cog in the top-right opens the panel; the cog fades after ~3 s without mouse movement.
- [ ] Changes preview live and are still there after restarting the app.
- [ ] "Test tick" plays a tick; "Test flip" flips all six tiles once.
- [ ] Fonts, colors, background image, sound file and date language all apply.
- [ ] "Reset to defaults" asks for confirmation, then resets.

**Config**
- [ ] `config.toml` appears next to the executable (next to `Flipclock.app` on macOS).

---

## Known gaps

Features not yet working on a given platform during the proof of concept. Updated as the implementation progresses.

| Platform | Gap |
|---|---|
| macOS | Never built or run by the author — everything is unverified. |
| Linux (GNOME without AppIndicator) | No tray icon (by design: minimize goes to the taskbar instead). |

---

## Reporting a problem

Please include:
1. Operating system and version (e.g. *macOS 15.1, Apple M2*; *Ubuntu 24.04, Wayland*; *Windows 11 24H2*).
2. Which step of this README failed, and the **full** error text from the terminal (copy it, don't paraphrase).
3. For runtime problems: which checklist item failed and what you saw instead. A screenshot or short screen recording helps.
4. To get more detail, run the app from a terminal with logging enabled and attach the output:
   - Windows (PowerShell): `$env:RUST_LOG="debug"; .\target\release\flipclock.exe`
   - Linux: `RUST_LOG=debug ./target/release/flipclock`
   - macOS: `RUST_LOG=debug ~/Flipclock/Flipclock.app/Contents/MacOS/flipclock`
