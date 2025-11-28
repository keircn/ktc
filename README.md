# ktc - Keiran's Tiling Compositor

A minimal Wayland tiling compositor written in Rust
        for the sake of learning how Wayland works.

## Building & Running

```bash
cargo build --release

# Run from a TTY (not from within another compositor)
# First, ensure you're in the video and input groups:
sudo usermod -aG video,input $USER
# Log out and back in, then from a TTY:
./target/release/ktc
```

Once running, use `Alt+T` to launch a terminal (requires `foot` terminal).

## Keybinds

- `Ctrl+Alt+Q` - Exit compositor
- `Alt+T` - Launch terminal (foot)
- `Alt+Tab` / `Alt+J` - Focus next window
- `Alt+K` - Focus previous window

## Configuration

Copy `example.config.toml` to `~/.config/ktc/config.toml` and customize.

## Features

- GPU-accelerated rendering via OpenGL ES 2.0 with EGL/GBM
- Vsync support using DRM page flipping for tear-free display
- DMA-BUF support for zero-copy buffer sharing with clients
- CPU fallback for systems without GPU support
- Tiling window management
- XDG shell support
- Configurable keybinds and appearance

## Dependencies

- `wayland-server` - Wayland protocol server implementation
- `wayland-protocols` - Standard Wayland protocols (xdg-shell, wlr-screencopy)
- `calloop` - Event loop for Wayland protocol dispatch
- `drm` / `gbm` - Direct Rendering Manager for display output
- `khronos-egl` / `glow` - OpenGL ES rendering
- `input` - libinput for keyboard/mouse handling
- `xkbcommon` - Keyboard layout handling
- `libc` / `nix` - Low-level system interfaces

## Current Limitations

- Single output only
