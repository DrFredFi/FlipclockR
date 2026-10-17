# wgpu for rendering

Flipclock must run on Windows, Linux and macOS with feature parity, render only on demand, and host an egui overlay in the same pass. We render the 3D scene with wgpu (DX12 on Windows, Vulkan on Linux, Metal on macOS, WGSL shaders, the software fallback adapter where no GPU is available). We chose it over separate native renderers, which would mean two or three codebases and shader languages, and over a game engine such as Bevy, which is oversized for a clock and built around a continuous frame loop.
