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

Once running, use `Mod+Return` to launch a terminal (requires `foot` terminal). The default modifier key is `Alt`.

## Keybinds

Default keybinds (configurable via config file):

| Key | Action |
|-----|--------|
| `Ctrl+Alt+Q` | Exit compositor |
| `Mod+Return` | Launch terminal (foot) |
| `Mod+D` | Launch app launcher (fuzzel) |
| `Mod+J/K` | Focus next/previous window |
| `Mod+H/L` | Focus left/right window |
| `Mod+Shift+Q` | Close focused window |
| `Mod+Shift+J/K` | Swap with next/previous window |
| `Mod+Shift+H/L` | Swap with left/right window |
| `Mod+F` | Toggle fullscreen |
| `Mod+Shift+Space` | Toggle floating |
| `Mod+M` | Toggle maximize |
| `Mod+1-9` | Switch to workspace 1-9 |
| `Mod+Shift+1-9` | Move window to workspace 1-9 |
| `Mod+Ctrl+1-9` | Move window to workspace silently |
| `Mod+Ctrl+H/J/K/L` | Resize window |
| `Mod+/-/=` | Shrink/grow window |

## Configuration

Copy `example.config.toml` to `~/.config/ktc/config.toml` and customize.

Key configuration sections:

- `[display]` - DRM device, resolution, vsync, VRR
- `[appearance]` - Colors, title bar height, borders, gaps
- `[keyboard]` - XKB layout, model, options
- `[keybinds]` - Comprehensive keybinding system
- `[debug]` - Profiler overlay

## Components

### ktc

The main compositor binary.

### ktcbar

A status bar that uses the layer shell protocol. Displays:

- Workspace indicators
- Current time
- Focused window title

Run alongside the compositor:

```bash
./target/release/ktcbar
```

### ktc-common

Shared library containing common utilities:

- Color management
- Font rendering
- IPC protocol
- Logging system
- Path utilities

## Features

- **GPU-accelerated rendering** via OpenGL ES 2.0 with EGL/GBM
- **Vsync support** using DRM page flipping for tear-free display
- **Variable refresh rate (VRR)** support (FreeSync/G-Sync)
- **DMA-BUF support** for zero-copy buffer sharing with clients
- **CPU fallback** for systems without GPU support
- **Tiling window management** with 9 workspaces
- **Floating window support** with maximize/fullscreen states
- **Layer shell support** for panels, wallpapers, and overlays
- **IPC socket** for external tools (used by ktcbar)
- **XDG shell support** with proper popup positioning
- **Configurable keybinds** and appearance
- **Screen recording support** (wlr-screencopy)
- **Output management** (wlr-output-management, read-only)
- **Window decorations** with title bars and borders
- **Comprehensive window management** (focus, move, resize, swap)

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
| zwlr_layer_shell | 4 | Full |
| zwlr_screencopy_manager | 3 | Full |
| zwlr_output_manager | 4 | Read-only |
| zwp_linux_dmabuf | 4 | Full with feedback |

## Roadmap

### Near-term

- [ ] Multi-output support (output hotplug, layout configuration)
- [ ] Output configuration apply (resolution/refresh changes via wlr-output-management)
- [ ] Pointer constraints protocol (for games/3D apps)
- [ ] Relative pointer protocol (for games/3D apps)

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
- `wayland-protocols` / `wayland-protocols-wlr` - Standard and wlroots Wayland protocols
- `calloop` - Event loop for Wayland protocol dispatch
- `drm` / `gbm` - Direct Rendering Manager for display output
- `khronos-egl` / `glow` - OpenGL ES rendering
- `input` - libinput for keyboard/mouse handling
- `xkbcommon` - Keyboard layout handling
- `libc` / `nix` - Low-level system interfaces
- `drm-fourcc` - DRM format definitions

## Current Limitations

- Single output only
- No Vulkan support (EGL/OpenGL ES only)
- No touch input
- No XWayland support
