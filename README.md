# ktc - Keiran's Tiling Compositor

A minimal Wayland tiling compositor written in Rust for the sake of learning how Wayland works.

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

## Dependencies

- `wayland-server` - Wayland protocol server implementation
- `wayland-protocols` - Standard Wayland protocols (xdg-shell, wlr-screencopy)
- `calloop` - Event loop for Wayland protocol dispatch
- `drm` / `gbm` - Direct Rendering Manager for display output
- `input` - libinput for keyboard/mouse handling
- `xkbcommon` - Keyboard layout handling
- `libc` / `nix` - Low-level system interfaces

## Current Limitations

- Software rendering only (no GPU acceleration)
- SHM buffers only (no DMA-BUF support)
- Single output only
