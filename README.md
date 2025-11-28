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
- Screen recording support (wlr-screencopy)
- Output management (wlr-output-management)

## Supported Protocols

| Protocol | Version | Status |
|----------|---------|--------|
| wl_compositor | 6 | Full |
| wl_subcompositor | 1 | Basic |
| wl_shm | 1 | Full |
| wl_seat | 7 | Keyboard + Pointer |
| wl_output | 4 | Full |
| wl_data_device_manager | 3 | Basic |
| xdg_wm_base | 5 | Full |
| xdg_output_manager | 3 | Full |
| xdg_decoration_manager | 1 | Full |
| zwlr_screencopy_manager | 3 | Full |
| zwlr_output_manager | 4 | Read-only |
| zwp_linux_dmabuf | 4 | Full |

## Roadmap

### Near-term
- [ ] Multi-output support (output hotplug, layout configuration)
- [ ] Output configuration apply (resolution/refresh changes via wlr-output-management)
- [ ] Pointer constraints protocol (for games/3D apps)
- [ ] Relative pointer protocol (for games/3D apps)
- [ ] Layer shell protocol (for panels, wallpapers, overlays)

### Medium-term
- [ ] Popup/menu positioning improvements
- [ ] Clipboard manager (wlr-data-control)
- [ ] Session lock protocol (screen locking)
- [ ] Drag and drop improvements
- [ ] Touch input support

### Long-term
- [ ] XWayland support
- [ ] Fractional scaling
- [ ] HDR/color management
- [ ] Virtual keyboard protocol
- [ ] Input method protocol

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
