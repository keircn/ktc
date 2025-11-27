# ktc - Keiran's Tiling Compositor

A crappy Wayland compositor written in Rust for the sake of learning how wayland works.

## Building & Running

```bash
cargo run start --nested

# In another terminal, test with clients (change wayland-1 to whatever is printed by ktc)
WAYLAND_DISPLAY=wayland-1 gnome-control-center
WAYLAND_DISPLAY=wayland-1 foot
```

The compositor creates a window showing connected clients. Successfully renders most native wayland applications with some success

## Dependencies

- `wayland-server` - Wayland protocol server implementation
- `wayland-protocols` - Standard Wayland protocols
- `calloop` - Event loop for Wayland protocol dispatch
- `winit` - Cross-platform window creation
- `softbuffer` - Software rendering to window
- `libc` - For mmap-ing shared memory buffers

## Current Limitations

- No window positioning/decoration
- Software rendering only (no GPU acceleration)
- SHM buffers only (no DMA-BUF/Vulkan support)

