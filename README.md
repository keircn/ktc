# KTC - Minimal Wayland Compositor

A minimal, from-scratch Wayland compositor written in Rust. Built for learning and experimentation.

## Features

### Protocol Support
- Core protocols: `wl_compositor`, `wl_surface`, `wl_region`, `wl_shm`, `wl_buffer`
- Shell: `xdg_wm_base`, `xdg_surface`, `xdg_toplevel`, `xdg_popup`, `xdg_positioner`
- Input: `wl_seat`, `wl_pointer`, `wl_keyboard` (events advertised, forwarding TODO)
- Output: `wl_output` (1920x1080@60Hz advertised)
- Data: `wl_data_device_manager` (clipboard/DnD)

### Rendering
- Shared memory (SHM) buffer support with mmap
- Software rendering via `softbuffer`
- Frame callbacks for smooth client updates
- Buffer release events
- XDG surface configure events for window management
- Nested window mode (runs within existing compositor)

### In Progress
- Input event forwarding (keyboard/mouse)
- Multi-surface compositing
- Window management (focus, stacking, positioning)
- Damage tracking optimization

## Building & Running

```bash
cargo run

# In another terminal, test with clients:
WAYLAND_DISPLAY=wayland-1 gnome-control-center
WAYLAND_DISPLAY=wayland-1 gnome-calculator
```

The compositor creates a window showing connected clients. Successfully renders GTK applications like gnome-control-center.

## Dependencies

- `wayland-server` - Wayland protocol server implementation
- `wayland-protocols` - Standard Wayland protocols
- `calloop` - Event loop for Wayland protocol dispatch
- `winit` - Cross-platform window creation
- `softbuffer` - Software rendering to window
- `libc` - For mmap-ing shared memory buffers

## Current Limitations

- Only renders first surface (no multi-window compositing yet)
- No input forwarding (can't interact with apps)
- No window positioning/decoration
- Software rendering only (no GPU acceleration)
- Nested mode only (no DRM/KMS backend)
- SHM buffers only (no DMA-BUF/Vulkan support)

