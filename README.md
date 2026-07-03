# memtty

A terminal emulator built with Rust and `wgpu`. 

`memtty` is a hardware-accelerated terminal emulator designed for high performance and modern rendering. It leverages the Rust ecosystem to provide a fast and robust terminal experience.

## Features

- **GPU Accelerated:** Uses `wgpu` for fast and efficient rendering across platforms.
- **Cross-Platform Windowing:** Powered by `winit` for seamless window management.
- **PTY Support:** Utilizes `portable-pty` for robust cross-platform pseudoterminal interactions.
- **VTE Parsing:** Uses `vte` for accurate parsing of virtual terminal emulator escape sequences.
- **Fast Text Rendering:** Employs `glyphon` for high-performance text rendering.

## Dependencies

- [wgpu](https://github.com/gfx-rs/wgpu) - Cross-platform, safe, pure-rust graphics API.
- [winit](https://github.com/rust-windowing/winit) - Window creation and management.
- [portable-pty](https://github.com/wez/wezterm/tree/main/pty) - Pseudoterminal interface.
- [vte](https://github.com/alacritty/vte) - Terminal escape sequence parser.
- [glyphon](https://github.com/grovesNL/glyphon) - Fast and simple 2D text rendering.

## Getting Started

### Prerequisites

You will need to have Rust and Cargo installed on your system. If you haven't installed them yet, you can do so by following the instructions at [rustup.rs](https://rustup.rs/).

### Building and Running

To build and run the project locally in release mode, use Cargo:

```bash
cargo run --release
```

To build without running:

```bash
cargo build --release
```

## Architecture

- `src/main.rs`: Application entry point.
- `src/terminal/`: Terminal state management and emulator logic.
- `src/pty.rs`: Integration with pseudoterminal interfaces.
- `src/ui/`: Window management and `wgpu` based rendering.
