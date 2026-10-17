# Flipclock — Specification

A realistic 3D split-flap desktop clock for **Windows, Linux and macOS**.

Status: **proof of concept**. Domain terms (tile, flap, flip, roll, cascade, tick, separator, date line) are defined in [`GLOSSARY.md`](GLOSSARY.md); architectural decisions are recorded in [`docs/adr/`](docs/adr/).

## Platforms

- Targets: Windows 10/11 x64, Linux x86_64 (X11 or Wayland session), macOS 12+ (Apple Silicon and Intel).
- **Goal for v1: full feature parity on all three platforms.** During the proof of concept a platform may have incomplete features, provided they are listed under "Known gaps" in `README.md`.
- On Linux the app always runs through **X11** (XWayland on Wayland sessions), never as a native Wayland client — required for window positioning and always-on-top (ADR-0002).
- The only unavoidable exception to parity is the tray on desktops without a tray host (see Tray icon).

## Stack

- **Rust** (stable toolchain, edition 2024). Cargo workspace, `Cargo.lock` committed, crate versions pinned to the latest compatible releases at implementation time.
- **winit** for windowing and input (borderless, fullscreen, always-on-top, drag-move, drag-resize, per-monitor DPI). On Linux, the event loop is built with the X11 backend forced.
- **wgpu** for 3D rendering: DX12 on Windows, Vulkan on Linux, Metal on macOS; shaders in **WGSL** (ADR-0001).
- **egui** (`egui` + `egui-winit` + `egui-wgpu`, with egui's bundled fonts) for the settings panel, cog button and warning banner, drawn on top of the resolved frame through a non-sRGB view. **The wgpu version must match the one required by `egui-wgpu`.**
- **fontdb** for enumerating installed system fonts; **swash** for rasterizing digit and date glyphs to textures (weights and variable fonts).
- **serde** + **toml** for config.
- **tray-icon** (+ its re-exported `muda` menu) for the tray / macOS menu bar icon, built with the `ksni` backend: on Linux the tray is a pure D-Bus StatusNotifierItem, with no GTK or libappindicator dependency.
- **rodio** (with `symphonia` mp3 support) for audio.
- **rfd** for native file dialogs.
- **image** for loading the background image.
- **chrono** for local time and date.
- **sys-locale** to detect the OS display language.
- **interprocess** (local sockets) for single-instance signalling.
- **log** + **env_logger** for diagnostics, controlled by `RUST_LOG` (off by default; README tells testers how to enable it).
- Thin platform gaps only, behind a small `platform` module with one implementation per OS:
  - Windows: **windows** crate (`GetDoubleClickTime`, `GetSystemMetrics(SM_CXDRAG)`).
  - macOS: **objc2** messages (`NSEvent.doubleClickInterval`, Dock visibility via `NSApplication.setActivationPolicy`).
  - Linux: fixed defaults (double-click 400 ms, drag threshold 4 px); tray host check via `dbus-send`.
- No asset files: the default tick is synthesized at startup and the app/tray icon is drawn procedurally (the same code generates the Windows `.ico` in `build.rs`).
- Windows: static CRT (`+crt-static`, set in `.cargo/config.toml` for the `x86_64-pc-windows-msvc` target), `#![windows_subsystem = "windows"]` in release, icon and manifest (per-monitor DPI v2) embedded via **embed-resource**.
- macOS: minimal `Flipclock.app` produced with **cargo-bundle** (metadata in the app crate's `Cargo.toml`).

## Workspace structure

| Crate | Type | Responsibility |
|---|---|---|
| `crates/flipclock-core` | lib | Digit sequences, flip/roll/cascade planning, tick scheduling, date formatting (localized tables), config load/validate/save, layout math, window placement rules, animation timing curves. No GPU/UI/OS dependencies. Covered by unit tests (`cargo test`). |
| `crates/flipclock-render` | lib | wgpu scene, tile geometry, PBR shaders, shadow maps, flip animation, glyph textures. |
| `crates/flipclock-app` | bin `flipclock` | winit event loop, egui settings panel, tray, window management, audio, single instance, platform module, UI string localization. |

Renderer and UI are verified manually (see the test checklist in `README.md`).

## Clock face

- 24h format `HH:mm:ss`, **six tiles** (one per digit), digits grouped in pairs.
- Separator rendered as **two lit 3D dots** in the digit color.
- Seconds tiles scale via slider **30–100%** of hour/minute tile size (default 100%), **bottom-aligned**. The second separator stays at hour/minute size.

## Rendering

- Straight-on camera, low FOV, mild perspective.
- Tiles: rounded, bevelled, visible thickness, visible horizontal split gap, subtle hinge pins.
- **PBR** shading (metallic/roughness); finish slider matte ↔ glossy (default satin plastic).
- One directional key light (sliders: **angle**, **intensity**) + soft ambient.
- **PCF soft shadow maps** — tiles cast shadows onto the background.
- 4× MSAA.
- **Render on demand** only (during flips, resize, settings changes, egui repaint requests) — no constant loop. Use `ControlFlow::Wait` / `WaitUntil` (next second boundary or next animation frame); never `Poll`.
- **No rendering while the window is hidden or minimized.**
- Adapter selection: request a hardware adapter; if none, retry with `force_fallback_adapter: true` (WARP on Windows, llvmpipe/lavapipe on Linux). If no adapter at all, show an error message box and exit.
- Digits are rasterized with swash into GPU textures mapped on tile faces, so any installed system font works and is lit by the scene. Glyph textures are regenerated only when font, weight, color or tile pixel size changes.

## Flip animation

- Classic split-flap: the top flap rotates down around the hinge, its back reveals the new bottom half, settles with a bounce.
- Flips start **on the second boundary**.
- **Digit sequences.** Each tile has its own cyclic sequence and only ever moves **forward**:
  - hours tens `0–2`; hours units `0–9`; minutes/seconds tens `0–5`; minutes/seconds units `0–9`.
  - Example: at midnight the hours-units tile goes 3 → 4 → … → 9 → 0 (7 flips). When DST moves the clock back one hour, it rolls 9 flips forward.
- **Roll.** When a tile's target is more than one step away (resume from sleep, DST, manual clock change), it rolls through every intermediate digit:
  - intermediate flips use a fixed short duration (~60–80 ms) **without bounce**;
  - only the final flip uses the configured flip duration and bounce.
- **Cascade.** When several tiles change in the same second, their flips/rolls start staggered right → left by the cascade delay.
- **Retargeting.** If a new second arrives while a tile is still rolling, its target is updated and the roll continues — it never restarts. Tiles that are idle flip normally.
- **Restore.** When the window becomes visible again after being hidden/minimized, the clock face snaps to the current time with no animation and no ticks.
- Sliders:
  - Flip duration: 150–1000 ms (default 450)
  - Cascade delay: 0–200 ms (default 60)
  - Bounce strength: 0 (none) → strong

## Sound

- **One tick every time a flap lands, on every tile, always** — normal flips and every flip of a roll. E.g. `09:59:59 → 10:00:00` produces 6 ticks staggered by the cascade; `:59 → :00` produces 2.
- Overlapping ticks are mixed, never dropped.
- **No ticks while the window is hidden or minimized.**
- Bundled default tick (embedded); user may select a custom `.mp3`.
- On/off toggle and volume slider.
- Audio output is opened once at startup; if no audio device is available, sound is silently disabled.

## Layout & sizing

- Clock fills the window minus a uniform margin.
- Margin slider **0–500** logical pixels (default 250).
- Margin auto-shrinks so the clock never drops below ~40% of the window.
- Everything scales with window resize and DPI (scale factor) changes.

## Date line

- Shows weekday and date, e.g. **`Giovedì, 15 Ottobre 2026`** (first letter of weekday and month capitalized).
- **Date language is configurable: Italian (`it-IT`, default), English (`en-GB`), Spanish (`es-ES`).** Same pattern `Weekday, D Month YYYY` in the selected language, e.g. `Thursday, 15 October 2026`, `Jueves, 15 Octubre 2026`. Names come from built-in tables in `flipclock-core` (no ICU).
- Rendered as a glyph texture (same swash rasterizer) on a screen-space quad drawn over the scene, pixel-aligned for crisp text.
- Position: **bottom-left / bottom-center / bottom-right**, inside the bottom margin.
- Size scales with window, plus a size multiplier; can be hidden.
- **Refreshes whenever the local date changes for any reason** (midnight, resume from sleep, manual clock change).

## Customization

- Colors: tile face, digits, date text, background fallback color.
- Fonts: separate pickers for **digits** and **date**, listing installed system font families (fontdb), with weight selection limited to the weights available for the chosen family. Missing font → fall back to a default system font.
- Background image: **Fill (uniform-to-fill, cropped)**; solid color when none; optional dim slider. Tiles cast shadows onto it.
- No theme presets in v1 — only current state is persisted.

## Window

- Normal resizable window. Minimum size 480×270 logical pixels.
- **Fullscreen** toggle: **F11** or double-click; on macOS also **⌃⌘F**. Windows/Linux use borderless fullscreen on the current monitor; macOS uses native fullscreen (own Space).
- Optional **always-on-top**.
- **Borderless** toggle anytime: panel switch or **Ctrl+B** (**⌘B** on macOS). When borderless:
  - drag anywhere to move — the drag starts only after the pointer moves past the system drag threshold with the button held (so double-click still works);
  - resize from invisible edges (~8 px hit zones, matching resize cursors).
- **Double-click** toggles fullscreen only on the clock area (not on the settings panel or the cog). Detected manually using the platform double-click interval.
- No window dragging while in fullscreen.
- **Persistence and startup placement:**
  - Persisted: position, size, maximized, borderless, always-on-top.
  - Never restored: fullscreen, hidden, minimized — the app always starts as a visible window.
  - The saved position is used if at least a 100×30 logical-pixel area of the title-bar region lies on an existing monitor.
  - Otherwise (first launch, monitor gone, off-screen): **1280×720** logical pixels, clamped to 80% of the work area, **centered on the primary monitor**.
- **Single instance:** a second launch signals the running instance (local socket) to show, restore and focus its window, then exits.

## Tray icon

- Always present where the platform has a tray host (Windows notification area, macOS menu bar, Linux StatusNotifier/AppIndicator).
- **Minimize → hides to tray. Close (X) → exits the application.**
- Menu: Show/Hide, Settings, Always on top, Borderless, Mute tick, Exit. Checkable items reflect current state.
- Double-click toggles show/hide (where the platform supports tray double-click).
- Tray and menu events are forwarded into the winit loop via an `EventLoopProxy` user event.
- **Linux:** the tray is a StatusNotifierItem over D-Bus (ksni backend). At startup, check for a tray host: the `org.kde.StatusNotifierWatcher` name must have an owner on the session bus. A single left click toggles show/hide, since StatusNotifier hosts do not report double-clicks.
- **No tray host** (e.g. GNOME without the AppIndicator extension): minimize behaves as a normal minimize to the taskbar/dock, and the tray menu actions (always on top, borderless, mute, exit) remain reachable from the settings panel.
- **macOS:** the menu bar icon replaces the tray; while the window is hidden the app has no Dock icon (accessory activation policy), and regains it when shown. ⌘Q exits.

## Settings panel

- Opened by a **cog button in the top-right corner**; cog auto-fades after ~3 s mouse idle.
- Slide-in overlay from the right, **live preview**, **debounced autosave** (~500 ms after the last change).
- "Reset to defaults" (with confirmation).
- "Test tick": plays one tick.
- "Test flip": all six tiles perform one flip (front and back show the current digit, so the displayed time stays correct), with cascade, bounce and ticks.
- Contains every option listed in this spec.

## Localization (app UI)

- Panel, tray menu and messages follow the OS display language (sys-locale); ships **Italian** and **English**; English is the fallback.
- UI strings live in a compile-time table in `flipclock-app`.
- Date language is a separate setting (see Date line).

## Configuration file

- `config.toml` **next to the executable** (ADR-0003). **No fallback location.**
  - Windows: next to `flipclock.exe`.
  - Linux: next to the binary; if run as an AppImage, next to the `.AppImage` file (`$APPIMAGE`).
  - macOS: in the folder that **contains** `Flipclock.app` — never inside the bundle.
- If the folder is not writable or saving fails (including macOS App Translocation): show a warning banner once per launch and continue with in-memory settings.
- Invalid keys/values revert to defaults individually (out-of-range numbers are clamped); a corrupt file is preserved as `config.toml.bak`.
- Background image and sound are stored as **original paths**; if missing, silently fall back to defaults.
- Saves are atomic: write to a temp file in the same folder, then rename.

## Error handling

- No panics on expected failures (config, audio, missing files, missing fonts).
- A panic hook shows a native message box with the error before exiting.

## Build & distribution

- **Proof of concept:** `cargo build --release` on each platform, plus `cargo bundle --release` for `Flipclock.app` on macOS. Step-by-step instructions per platform live in `README.md`.
- No CI during the proof of concept (ADR-0004).
- **Later (v1):** Windows `.zip` with `flipclock.exe` (x64, ARM64 optional); macOS universal `Flipclock.app` (arm64 + x86_64) in a `.zip`; Linux AppImage x86_64 + `.tar.gz`. GitHub Actions matrix. Code signing and notarization out of scope for v1.

## Out of scope for v1

12h mode, theme presets, start with the OS, multi-monitor spanning, native Wayland, code signing/notarization, anything not listed above.

## Reference

Layout inspiration: http://flipclock.online
