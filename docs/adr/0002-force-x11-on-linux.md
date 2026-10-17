# Force X11 (XWayland) on Linux

On Linux, Flipclock always runs through X11, using XWayland under Wayland sessions, and never as a native Wayland client. Wayland does not let clients read or set their window position or stay always on top, and v1 requires feature parity on all platforms: restoring the saved position, always-on-top, and borderless drag-move and drag-resize. The accepted cost is possible blurriness at fractional scaling on Wayland, where the compositor upscales the XWayland surface. Do not "fix" this by enabling the Wayland backend without first dropping those features.
