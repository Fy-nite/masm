#![cfg(feature = "raylib_mni")]
use std::cell::RefCell;
use std::thread_local;

use crate::interpreter::{MniCtx, ModuleRegistry};

use raylib::prelude::*;

#[derive(Clone)]
enum DrawCommand {
    Texture(Vec<u8>, i32, i32),                    // pixels, width, height
    Text(String, i32, i32, i32, u8, u8, u8, u8),   // text, x, y, size, r, g, b, a
    Rectangle(i32, i32, i32, i32, u8, u8, u8, u8), // x, y, w, h, r, g, b, a
    RectangleLines(i32, i32, i32, i32, i32, u8, u8, u8, u8), // x, y, w, h, thickness, r, g, b, a
    Circle(i32, i32, i32, u8, u8, u8, u8),         // x, y, radius, r, g, b, a
    Line(i32, i32, i32, i32, u8, u8, u8, u8),      // x1, y1, x2, y2, r, g, b, a
}

struct RaylibGlobal {
    rl: RaylibHandle,
    th: RaylibThread,
    texture: Option<Texture2D>,
    tex_w: i32,
    tex_h: i32,
    commands: Vec<DrawCommand>,
}

impl RaylibGlobal {
    fn new(rl: RaylibHandle, th: RaylibThread) -> Self {
        Self {
            rl,
            th,
            texture: None,
            tex_w: 0,
            tex_h: 0,
            commands: Vec::new(),
        }
    }
}

thread_local! {
    static RL_STATE: RefCell<Option<RaylibGlobal>> = RefCell::new(None);
}

fn parse_u64(s: &str) -> Option<u64> {
    s.trim().parse::<u64>().ok()
}
fn parse_i32(s: &str) -> Option<i32> {
    s.trim().parse::<i32>().ok()
}

fn read_bytes_from_vm(ctx: &MniCtx, addr: usize, len: usize) -> Vec<u8> {
    let mem = ctx.state.memory.lock().unwrap();
    let end = addr.saturating_add(len).min(mem.len());
    if addr >= end {
        return Vec::new();
    }
    mem[addr..end].to_vec()
}

// Resolve register-name or numeric arguments
fn reg_value_by_name(ctx: &MniCtx, name: &str) -> Option<u64> {
    let nm = crate::register_map::RegisterMap::build_name_to_id();
    let key = name.trim().trim_start_matches('$').to_uppercase();
    nm.get(key.as_str())
        .and_then(|id| ctx.state.regs.get(id).copied())
}

fn parse_u64_arg(ctx: &MniCtx, s: &str) -> Option<u64> {
    let st = s.trim();
    if let Ok(v) = st.parse::<u64>() {
        return Some(v);
    }
    if let Some(hex) = st.strip_prefix("0x").or_else(|| st.strip_prefix("0X")) {
        if let Ok(v) = u64::from_str_radix(hex, 16) {
            return Some(v);
        }
    }
    reg_value_by_name(ctx, st)
}

fn parse_usize_arg(ctx: &MniCtx, s: &str) -> Option<usize> {
    parse_u64_arg(ctx, s).map(|v| v as usize)
}
fn parse_i32_arg(ctx: &MniCtx, s: &str) -> Option<i32> {
    if let Ok(v) = s.trim().parse::<i32>() {
        return Some(v);
    }
    parse_u64_arg(ctx, s).map(|v| v as i32)
}
fn parse_usize_lit(s: &str) -> Option<usize> {
    s.trim().parse::<usize>().ok()
}
fn parse_u8_lit(s: &str) -> Option<u8> {
    s.trim().parse::<u8>().ok()
}

fn key_from_code(code: i32) -> KeyboardKey {
    match code {
        256 => KeyboardKey::KEY_ESCAPE,
        257 => KeyboardKey::KEY_ENTER,
        258 => KeyboardKey::KEY_TAB,
        259 => KeyboardKey::KEY_BACKSPACE,
        260 => KeyboardKey::KEY_INSERT,
        261 => KeyboardKey::KEY_DELETE,
        262 => KeyboardKey::KEY_RIGHT,
        263 => KeyboardKey::KEY_LEFT,
        264 => KeyboardKey::KEY_DOWN,
        265 => KeyboardKey::KEY_UP,
        _ => KeyboardKey::KEY_NULL,
    }
}

pub fn register_raylib_mni(reg: &mut ModuleRegistry) {
    // Raylib.draw_text x y size r g b a text_addr text_len
    // Queues text drawing command for execution at present time
    reg.register("Raylib", "draw_text", |ctx: &mut MniCtx| {
        if ctx.args.len() < 9 {
            return;
        }
        let x = ctx
            .args
            .get(0)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let y = ctx
            .args
            .get(1)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let size = ctx
            .args
            .get(2)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(20);
        let r = ctx.args.get(3).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        let g = ctx.args.get(4).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        let b = ctx.args.get(5).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        let a = ctx.args.get(6).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        let addr = ctx
            .args
            .get(7)
            .and_then(|s| parse_usize_arg(ctx, s))
            .unwrap_or(0);
        let len = ctx
            .args
            .get(8)
            .and_then(|s| parse_usize_arg(ctx, s))
            .map(|v| v as usize)
            .unwrap_or(0);
        let mem = ctx.state.memory.lock().unwrap();
        let end = addr.saturating_add(len).min(mem.len());
        let text_bytes = &mem[addr..end];
        // Remove embedded nulls (C string safety)
        let sanitized: Vec<u8> = text_bytes.iter().cloned().filter(|&b| b != 0).collect();
        let text = String::from_utf8_lossy(&sanitized).to_string();
        drop(mem);

        RL_STATE.with(|cell| {
            if let Some(gl) = cell.borrow_mut().as_mut() {
                gl.commands
                    .push(DrawCommand::Text(text, x, y, size, r, g, b, a));
            }
        });
    });

    // Raylib.draw_rectangle x y w h r g b a
    // Queues rectangle drawing command for execution at present time
    reg.register("Raylib", "draw_rectangle", |ctx: &mut MniCtx| {
        if ctx.args.len() < 8 {
            return;
        }
        let x = ctx
            .args
            .get(0)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let y = ctx
            .args
            .get(1)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let w = ctx
            .args
            .get(2)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let h = ctx
            .args
            .get(3)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let r = ctx.args.get(4).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        let g = ctx.args.get(5).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        let b = ctx.args.get(6).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        let a = ctx.args.get(7).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        RL_STATE.with(|cell| {
            if let Some(gl) = cell.borrow_mut().as_mut() {
                gl.commands
                    .push(DrawCommand::Rectangle(x, y, w, h, r, g, b, a));
            }
        });
    });

    // Raylib.draw_rectangle_lines x y w h thickness r g b a
    // Queues rectangle outline drawing command
    reg.register("Raylib", "draw_rectangle_lines", |ctx: &mut MniCtx| {
        if ctx.args.len() < 9 {
            return;
        }
        let x = ctx
            .args
            .get(0)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let y = ctx
            .args
            .get(1)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let w = ctx
            .args
            .get(2)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let h = ctx
            .args
            .get(3)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let thickness = ctx
            .args
            .get(4)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(1);
        let r = ctx.args.get(5).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        let g = ctx.args.get(6).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        let b = ctx.args.get(7).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        let a = ctx.args.get(8).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        RL_STATE.with(|cell| {
            if let Some(gl) = cell.borrow_mut().as_mut() {
                gl.commands.push(DrawCommand::RectangleLines(
                    x, y, w, h, thickness, r, g, b, a,
                ));
            }
        });
    });

    // Raylib.draw_circle x y radius r g b a
    // Queues circle drawing command
    reg.register("Raylib", "draw_circle", |ctx: &mut MniCtx| {
        if ctx.args.len() < 7 {
            return;
        }
        let x = ctx
            .args
            .get(0)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let y = ctx
            .args
            .get(1)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let radius = ctx
            .args
            .get(2)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let r = ctx.args.get(3).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        let g = ctx.args.get(4).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        let b = ctx.args.get(5).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        let a = ctx.args.get(6).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        RL_STATE.with(|cell| {
            if let Some(gl) = cell.borrow_mut().as_mut() {
                gl.commands
                    .push(DrawCommand::Circle(x, y, radius, r, g, b, a));
            }
        });
    });

    // Raylib.draw_line x1 y1 x2 y2 r g b a
    // Queues line drawing command
    reg.register("Raylib", "draw_line", |ctx: &mut MniCtx| {
        if ctx.args.len() < 8 {
            return;
        }
        let x1 = ctx
            .args
            .get(0)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let y1 = ctx
            .args
            .get(1)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let x2 = ctx
            .args
            .get(2)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let y2 = ctx
            .args
            .get(3)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let r = ctx.args.get(4).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        let g = ctx.args.get(5).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        let b = ctx.args.get(6).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        let a = ctx.args.get(7).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        RL_STATE.with(|cell| {
            if let Some(gl) = cell.borrow_mut().as_mut() {
                gl.commands
                    .push(DrawCommand::Line(x1, y1, x2, y2, r, g, b, a));
            }
        });
    });

    // Raylib.init width height title
    reg.register("Raylib", "init", |ctx: &mut MniCtx| {
        let w = ctx
            .args
            .get(0)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(640);
        let h = ctx
            .args
            .get(1)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(480);
        let title = ctx.args.get(2).map(|s| s.as_str()).unwrap_or("masm");
        // Print all arguments for debugging
        println!(
            "[MNI] Raylib.init args: {:?} (w={}, h={}, title={})",
            ctx.args, w, h, title
        );
        RL_STATE.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_some() {
                *slot = None;
            }
            let mut init = raylib::init();
            let builder = init.size(w, h).title(title);
            let (rl, th) = builder.build();
            *slot = Some(RaylibGlobal::new(rl, th));
        });
    });

    // Raylib.set_target_fps fps
    reg.register("Raylib", "set_target_fps", |ctx: &mut MniCtx| {
        let fps = ctx
            .args
            .get(0)
            .and_then(|s| s.trim().parse::<u32>().ok())
            .or_else(|| {
                parse_u64_arg(ctx, ctx.args.get(0).map(String::as_str).unwrap_or(""))
                    .map(|v| v as u32)
            })
            .unwrap_or(60);
        RL_STATE.with(|cell| {
            if let Some(gl) = cell.borrow_mut().as_mut() {
                gl.rl.set_target_fps(fps);
            }
        });
    });

    // Raylib.should_close -> RAX=1/0
    reg.register("Raylib", "should_close", |ctx: &mut MniCtx| {
        let val = RL_STATE.with(|cell| match cell.borrow_mut().as_mut() {
            Some(gl) => gl.rl.window_should_close() as u64,
            None => 1u64,
        });
        if let Some(rax) = crate::register_map::RegisterMap::build_name_to_id()
            .get("RAX")
            .copied()
        {
            ctx.state.regs.insert(rax, val);
        }
    });

    // Raylib.is_key_down keycode -> RAX=1/0 (keycode matches raylib KeyboardKey as integer)
    reg.register("Raylib", "is_key_down", |ctx: &mut MniCtx| {
        let code = ctx
            .args
            .get(0)
            .and_then(|s| s.trim().parse::<i32>().ok())
            .or_else(|| {
                parse_u64_arg(ctx, ctx.args.get(0).map(String::as_str).unwrap_or(""))
                    .map(|v| v as i32)
            })
            .unwrap_or(0);
        let pressed = RL_STATE.with(|cell| match cell.borrow_mut().as_mut() {
            Some(gl) => {
                let key = key_from_code(code);
                gl.rl.is_key_down(key) as u64
            }
            None => 0u64,
        });
        if let Some(rax) = crate::register_map::RegisterMap::build_name_to_id()
            .get("RAX")
            .copied()
        {
            ctx.state.regs.insert(rax, pressed);
        }
    });

    // Raylib.is_key_pressed keycode -> RAX=1/0
    // Returns 1 if key was pressed during this frame (key went from up to down)
    reg.register("Raylib", "is_key_pressed", |ctx: &mut MniCtx| {
        let code = ctx
            .args
            .get(0)
            .and_then(|s| s.trim().parse::<i32>().ok())
            .or_else(|| {
                parse_u64_arg(ctx, ctx.args.get(0).map(String::as_str).unwrap_or(""))
                    .map(|v| v as i32)
            })
            .unwrap_or(0);
        let pressed = RL_STATE.with(|cell| match cell.borrow_mut().as_mut() {
            Some(gl) => {
                let key = key_from_code(code);
                gl.rl.is_key_pressed(key) as u64
            }
            None => 0u64,
        });
        if let Some(rax) = crate::register_map::RegisterMap::build_name_to_id()
            .get("RAX")
            .copied()
        {
            ctx.state.regs.insert(rax, pressed);
        }
    });

    // Raylib.is_key_released keycode -> RAX=1/0
    // Returns 1 if key was released during this frame (key went from down to up)
    reg.register("Raylib", "is_key_released", |ctx: &mut MniCtx| {
        let code = ctx
            .args
            .get(0)
            .and_then(|s| s.trim().parse::<i32>().ok())
            .or_else(|| {
                parse_u64_arg(ctx, ctx.args.get(0).map(String::as_str).unwrap_or(""))
                    .map(|v| v as i32)
            })
            .unwrap_or(0);
        let released = RL_STATE.with(|cell| match cell.borrow_mut().as_mut() {
            Some(gl) => {
                let key = key_from_code(code);
                gl.rl.is_key_released(key) as u64
            }
            None => 0u64,
        });
        if let Some(rax) = crate::register_map::RegisterMap::build_name_to_id()
            .get("RAX")
            .copied()
        {
            ctx.state.regs.insert(rax, released);
        }
    });

    // Raylib.get_key_pressed -> RAX
    // Returns the character code (ASCII/Unicode) of any key pressed this frame, or 0
    reg.register("Raylib", "get_key_pressed", |ctx: &mut MniCtx| {
        let key_code = RL_STATE.with(|cell| match cell.borrow_mut().as_mut() {
            Some(gl) => {
                match gl.rl.get_key_pressed() {
                    Some(key) => {
                        // Convert KeyboardKey enum to its numeric representation
                        key as i32 as u64
                    }
                    None => 0u64,
                }
            }
            None => 0u64,
        });
        if let Some(rax) = crate::register_map::RegisterMap::build_name_to_id()
            .get("RAX")
            .copied()
        {
            ctx.state.regs.insert(rax, key_code);
        }
    });

    // Raylib.present addr width height [pitch]
    // - Executes all queued drawing commands in a single frame
    // - Updates texture with pixel buffer and displays it
    reg.register("Raylib", "present", |ctx: &mut MniCtx| {
        let addr = ctx
            .args
            .get(0)
            .and_then(|s| parse_usize_arg(ctx, s))
            .unwrap_or(0);
        let w = ctx
            .args
            .get(1)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        let h = ctx
            .args
            .get(2)
            .and_then(|s| parse_i32_arg(ctx, s))
            .unwrap_or(0);
        if w <= 0 || h <= 0 {
            return;
        }

        let pitch = ctx
            .args
            .get(3)
            .and_then(|s| parse_usize_lit(s))
            .unwrap_or((w as usize) * 4);
        let needed = (h as usize).saturating_mul(pitch);
        let pixels = read_bytes_from_vm(ctx, addr, needed);

        RL_STATE.with(|cell| {
            if let Some(gl) = cell.borrow_mut().as_mut() {
                // Ensure texture exists and matches size
                let recreate = match &gl.texture {
                    Some(_) if gl.tex_w == w && gl.tex_h == h => false,
                    _ => true,
                };
                if recreate {
                    let img = Image::gen_image_color(w, h, Color::BLACK);
                    match gl.rl.load_texture_from_image(&gl.th, &img) {
                        Ok(tex) => {
                            gl.texture = Some(tex);
                            gl.tex_w = w;
                            gl.tex_h = h;
                        }
                        Err(e) => {
                            eprintln!("[MNI] Raylib.present: failed to create texture: {}", e);
                            return;
                        }
                    }
                }

                // Update texture with pixel data
                if let Some(tex) = gl.texture.as_mut() {
                    let _ = tex.update_texture(&pixels);
                }

                // Execute frame: all commands + texture draw
                let mut d = gl.rl.begin_drawing(&gl.th);
                d.clear_background(Color::BLACK);

                // Draw texture first
                if let Some(tex) = &gl.texture {
                    d.draw_texture(tex, 0, 0, Color::WHITE);
                }

                // Execute all queued drawing commands
                for cmd in gl.commands.drain(..) {
                    match cmd {
                        DrawCommand::Text(text, x, y, size, r, g, b, a) => {
                            d.draw_text(&text, x, y, size, Color::new(r, g, b, a));
                        }
                        DrawCommand::Rectangle(x, y, w, h, r, g, b, a) => {
                            d.draw_rectangle(x, y, w, h, Color::new(r, g, b, a));
                        }
                        DrawCommand::RectangleLines(x, y, w, h, thickness, r, g, b, a) => {
                            // Raylib's draw_rectangle_lines only takes (x, y, width, height, line_thick)
                            // It uses the current drawing color, so we need to draw it differently
                            // For now, draw a filled rectangle with a hollow center
                            d.draw_rectangle(x, y, w, thickness as i32, Color::new(r, g, b, a)); // top
                            d.draw_rectangle(
                                x,
                                y + h - thickness as i32,
                                w,
                                thickness as i32,
                                Color::new(r, g, b, a),
                            ); // bottom
                            d.draw_rectangle(x, y, thickness as i32, h, Color::new(r, g, b, a)); // left
                            d.draw_rectangle(
                                x + w - thickness as i32,
                                y,
                                thickness as i32,
                                h,
                                Color::new(r, g, b, a),
                            ); // right
                        }
                        DrawCommand::Circle(x, y, radius, r, g, b, a) => {
                            // Raylib's draw_circle expects center_x, center_y, radius (f32), color
                            d.draw_circle(x, y, radius as f32, Color::new(r, g, b, a));
                        }
                        DrawCommand::Line(x1, y1, x2, y2, r, g, b, a) => {
                            d.draw_line(x1, y1, x2, y2, Color::new(r, g, b, a));
                        }
                        DrawCommand::Texture(_, _, _) => {
                            // Texture is handled separately above
                        }
                    }
                }
                // End of drawing frame (d goes out of scope)
            }
        });
    });

    // Raylib.close
    reg.register("Raylib", "close", |_ctx: &mut MniCtx| {
        RL_STATE.with(|cell| {
            let mut slot = cell.borrow_mut();
            if let Some(gl) = slot.take() {
                drop(gl);
            }
        });
    });

    // Helpers that operate on VM memory pixel buffers --------------------

    // Raylib.set_pixel addr width x y r g b a
    // format: RGBA8
    reg.register("Raylib", "set_pixel", |ctx: &mut MniCtx| {
        if ctx.args.len() < 8 {
            return;
        }
        let base = ctx
            .args
            .get(0)
            .and_then(|s| parse_usize_arg(ctx, s))
            .unwrap_or(0);
        let w = ctx
            .args
            .get(1)
            .and_then(|s| parse_usize_arg(ctx, s))
            .unwrap_or(0);
        let x = ctx
            .args
            .get(2)
            .and_then(|s| parse_usize_arg(ctx, s))
            .unwrap_or(0);
        let y = ctx
            .args
            .get(3)
            .and_then(|s| parse_usize_arg(ctx, s))
            .unwrap_or(0);
        let r = ctx.args.get(4).and_then(|s| parse_u8_lit(s)).unwrap_or(0);
        let g = ctx.args.get(5).and_then(|s| parse_u8_lit(s)).unwrap_or(0);
        let b = ctx.args.get(6).and_then(|s| parse_u8_lit(s)).unwrap_or(0);
        let a = ctx.args.get(7).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        if w == 0 {
            return;
        }
        let idx = base.saturating_add((y.saturating_mul(w) + x).saturating_mul(4));
        let mut mem = ctx.state.memory.lock().unwrap();
        let need = idx.saturating_add(4);
        if mem.len() < need {
            mem.resize(need, 0);
        }
        if need <= mem.len() {
            mem[idx] = r;
            mem[idx + 1] = g;
            mem[idx + 2] = b;
            mem[idx + 3] = a;
        }
    });

    // Raylib.clear_buffer addr width height r g b a
    reg.register("Raylib", "clear_buffer", |ctx: &mut MniCtx| {
        if ctx.args.len() < 7 {
            return;
        }
        let base = ctx
            .args
            .get(0)
            .and_then(|s| parse_usize_arg(ctx, s))
            .unwrap_or(0);
        let w = ctx
            .args
            .get(1)
            .and_then(|s| parse_usize_arg(ctx, s))
            .unwrap_or(0);
        let h = ctx
            .args
            .get(2)
            .and_then(|s| parse_usize_arg(ctx, s))
            .unwrap_or(0);
        let r = ctx.args.get(3).and_then(|s| parse_u8_lit(s)).unwrap_or(0);
        let g = ctx.args.get(4).and_then(|s| parse_u8_lit(s)).unwrap_or(0);
        let b = ctx.args.get(5).and_then(|s| parse_u8_lit(s)).unwrap_or(0);
        let a = ctx.args.get(6).and_then(|s| parse_u8_lit(s)).unwrap_or(255);
        if w == 0 || h == 0 {
            return;
        }
        let mut mem = ctx.state.memory.lock().unwrap();
        let need = base.saturating_add(w.saturating_mul(h).saturating_mul(4));
        if mem.len() < need {
            mem.resize(need, 0);
        }
        for y in 0..h {
            for x in 0..w {
                let idx = base + (y * w + x) * 4;
                mem[idx] = r;
                mem[idx + 1] = g;
                mem[idx + 2] = b;
                mem[idx + 3] = a;
            }
        }
    });
}
