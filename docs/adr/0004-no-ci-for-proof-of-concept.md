# No CI while the project is a proof of concept

Flipclock is currently a proof of concept, so builds and tests run manually on each platform, following the instructions in `README.md`, and there is no CI. When the project moves past the proof of concept, the plan is a GitHub Actions matrix (Windows, Linux, macOS) that runs `cargo test`, builds release binaries and produces the platform packages on every push. CI is also the only practical way to build macOS without a local Mac.
