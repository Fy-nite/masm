# Raylib MNI Module

Built-in MNI module that exposes a tiny subset of raylib for windowing and drawing from a VM pixel buffer.

Pixel format is RGBA8 (r,g,b,a per pixel), tightly packed unless otherwise noted.

Functions:

- Raylib.init width height title
  - Creates a window. If called again, recreates window.

- Raylib.set_target_fps fps

- Raylib.should_close -> RAX = 1 if window should close, 0 otherwise

- Raylib.is_key_down keycode -> RAX = 1/0
  - keycode is raylib KeyboardKey integer (e.g. 256 = KEY_ESCAPE)

- Raylib.present addr width height [pitch]
  - Uploads the pixel buffer at `addr` to a texture of size `width` x `height` and draws it to the window.
  - When `pitch` is omitted it is assumed to be width*4.

- Raylib.set_pixel addr width x y r g b a
  - Writes a single pixel into the VM buffer (RGBA8). Useful for simple drawing without a loop in host.

- Raylib.clear_buffer addr width height r g b a
  - Fills the VM buffer rectangle with the given color.

- Raylib.close
  - Destroys the window and releases resources.

See `masm/examples/raylib_demo.masm` for a minimal usage example.
