//! The GW-BASIC window. Text mode is 80×25 (640×400) or 40×25 (320×400).
//! A graphics mode blits its pixel buffer, then draws the text cells on top.
//! Both use an integer scale and a black letterbox. Row 25 is the soft-key
//! strip when KEY is on. The cursor is an inverted cell, or the lower half
//! of one in insert mode; it is not stored in the cell buffer.

use crate::font;
use crate::input::{char_to_byte, ctrl_letter, Input};
use crate::options::parse_args;
use crate::screen::EGA16;
use crate::session::Session;
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{Window, WindowId};

const ROWS: usize = 25;
const CELL_W: usize = 8;
const CELL_H: usize = 16;
const KEY_ATTR: u8 = 0x70;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = parse_args(&args)
        .map_err(|message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message))?;
    let mut session = Session::new();
    session.boot(&options);
    if session.quit() {
        return Ok(());
    }

    let event_loop = EventLoop::new()?;
    let mut app = App {
        window: None,
        context: None,
        surface: None,
        session,
        mods: ModifiersState::empty(),
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct App {
    window: Option<Rc<Window>>,
    context: Option<softbuffer::Context<Rc<Window>>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    session: Session,
    mods: ModifiersState,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("GW-BASIC 3.23")
            .with_inner_size(LogicalSize::new(1280.0, 800.0));
        let window = match event_loop.create_window(attrs) {
            Ok(window) => Rc::new(window),
            Err(err) => {
                eprintln!("could not open the window: {err}");
                event_loop.exit();
                return;
            }
        };
        let context = match softbuffer::Context::new(window.clone()) {
            Ok(context) => context,
            Err(err) => {
                eprintln!("could not create a graphics context: {err}");
                event_loop.exit();
                return;
            }
        };
        let surface = match softbuffer::Surface::new(&context, window.clone()) {
            Ok(surface) => surface,
            Err(err) => {
                eprintln!("could not create a graphics surface: {err}");
                event_loop.exit();
                return;
            }
        };
        let size = window.inner_size();
        self.window = Some(window);
        self.context = Some(context);
        self.surface = Some(surface);
        self.resize(size.width, size.height);
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.resize(size.width, size.height);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.mods = modifiers.state();
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                if is_synthetic {
                    return;
                }
                if let Some(input) = map_key(&event, self.mods) {
                    self.session.handle(input);
                    play_pending(&mut self.session);
                    if self.session.quit() {
                        event_loop.exit();
                        return;
                    }
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.session.needs_poll() {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        event_loop.set_control_flow(ControlFlow::Poll);
        self.session.poll();
        play_pending(&mut self.session);
        if self.session.quit() {
            event_loop.exit();
            return;
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

impl App {
    fn resize(&mut self, width: u32, height: u32) {
        let (Some(width), Some(height)) = (NonZeroU32::new(width), NonZeroU32::new(height)) else {
            return;
        };
        if let Some(surface) = &mut self.surface {
            let _ = surface.resize(width, height);
        }
    }

    fn redraw(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let Some(surface) = &mut self.surface else {
            return;
        };
        let size = window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        if surface.resize(width, height).is_err() {
            return;
        }
        let mut buffer = match surface.buffer_mut() {
            Ok(buffer) => buffer,
            Err(_) => return,
        };
        paint(
            &mut buffer,
            size.width as usize,
            size.height as usize,
            &self.session,
        );
        let _ = buffer.present();
    }
}

struct Frame {
    scale: usize,
    off_x: usize,
    off_y: usize,
    pix_w: usize,
    pix_h: usize,
}

#[derive(Clone, Copy)]
struct CellRect {
    x0: usize,
    x1: usize,
    y0: usize,
    y1: usize,
}

fn frame_for(win_w: usize, win_h: usize, pix_w: usize, pix_h: usize) -> Frame {
    let mut scale = (win_w / pix_w).min(win_h / pix_h);
    if scale == 0 {
        scale = 1;
    }
    let dest_w = pix_w * scale;
    let dest_h = pix_h * scale;
    Frame {
        scale,
        off_x: win_w.saturating_sub(dest_w) / 2,
        off_y: win_h.saturating_sub(dest_h) / 2,
        pix_w,
        pix_h,
    }
}

fn paint(buffer: &mut [u32], win_w: usize, win_h: usize, session: &Session) {
    if win_w == 0 || win_h == 0 || buffer.len() < win_w.saturating_mul(win_h) {
        return;
    }
    buffer[..win_w * win_h].fill(0);
    let screen = session.screen();
    if screen.graphics_mode() == 0 {
        paint_text(buffer, win_w, win_h, session);
    } else {
        paint_graphics(buffer, win_w, win_h, session);
    }
}

fn paint_text(buffer: &mut [u32], win_w: usize, win_h: usize, session: &Session) {
    let screen = session.screen();
    let cols = screen.width();
    let frame = frame_for(win_w, win_h, cols * CELL_W, ROWS * CELL_H);
    let (cur_row, cur_col) = screen.cursor();
    let insert = screen.insert_mode();
    let strip = session.key_strip();

    for row in 0..ROWS {
        for col in 0..cols {
            let strip_row = row + 1 == ROWS && strip.is_some();
            let (ch, attr) = if strip_row {
                (strip.as_ref().unwrap()[col], KEY_ATTR)
            } else {
                let cell = screen.cell(row, col);
                (cell.ch, cell.attr)
            };
            let glyph = font::glyph(ch);
            let fg = EGA16[(attr & 0x0F) as usize];
            let bg = EGA16[((attr >> 4) & 0x0F) as usize];
            let cursor_here = !strip_row && row == cur_row && col == cur_col;
            for (gy, bits) in glyph.iter().enumerate() {
                let bits = *bits;
                let invert = cursor_here && (!insert || gy >= CELL_H / 2);
                let (fg, bg) = if invert { (bg, fg) } else { (fg, bg) };
                for gx in 0..CELL_W {
                    let on = font::pixel_on(bits, gx);
                    let color = if on { fg } else { bg };
                    stamp(
                        buffer,
                        win_w,
                        win_h,
                        &frame,
                        col * CELL_W + gx,
                        row * CELL_H + gy,
                        color,
                    );
                }
            }
        }
    }
}

fn paint_graphics(buffer: &mut [u32], win_w: usize, win_h: usize, session: &Session) {
    let screen = session.screen();
    let (pix_w, pix_h) = screen.pixel_size();
    if pix_w == 0 || pix_h == 0 {
        return;
    }
    let frame = frame_for(win_w, win_h, pix_w, pix_h);
    for y in 0..pix_h {
        for x in 0..pix_w {
            stamp(buffer, win_w, win_h, &frame, x, y, screen.pixel_rgb(x, y));
        }
    }
    overlay_text(buffer, win_w, win_h, &frame, session);
}

fn overlay_text(buffer: &mut [u32], win_w: usize, win_h: usize, frame: &Frame, session: &Session) {
    let screen = session.screen();
    let cols = screen.width();
    let (cur_row, cur_col) = screen.cursor();
    let insert = screen.insert_mode();
    let strip = session.key_strip();
    for row in 0..ROWS {
        let (y0, y1) = axis_span(row, ROWS, frame.pix_h);
        if y0 == y1 {
            continue;
        }
        let strip_row = row + 1 == ROWS && strip.is_some();
        for col in 0..cols {
            let (x0, x1) = axis_span(col, cols, frame.pix_w);
            if x0 == x1 {
                continue;
            }
            let rect = CellRect { x0, x1, y0, y1 };
            if strip_row {
                let ch = strip.as_ref().unwrap()[col];
                blit_cell(
                    buffer,
                    (win_w, win_h),
                    frame,
                    rect,
                    ch,
                    EGA16[(KEY_ATTR & 0x0F) as usize],
                    Some(EGA16[((KEY_ATTR >> 4) & 0x0F) as usize]),
                );
                continue;
            }
            let cell = screen.cell(row, col);
            if cell.ch != b' ' {
                blit_cell(
                    buffer,
                    (win_w, win_h),
                    frame,
                    rect,
                    cell.ch,
                    screen.attribute_rgb(cell.attr),
                    None,
                );
            }
            if row == cur_row && col == cur_col {
                invert_cell(buffer, (win_w, win_h), frame, rect, insert);
            }
        }
    }
}

fn blit_cell(
    buffer: &mut [u32],
    win: (usize, usize),
    frame: &Frame,
    rect: CellRect,
    ch: u8,
    fg: u32,
    bg: Option<u32>,
) {
    let (win_w, win_h) = win;
    let glyph = font::glyph(ch);
    let gw = rect.x1 - rect.x0;
    let gh = rect.y1 - rect.y0;
    for y in rect.y0..rect.y1 {
        let sy = (y - rect.y0) * 16 / gh;
        let bits = glyph[sy.min(15)];
        for x in rect.x0..rect.x1 {
            let sx = (x - rect.x0) * 8 / gw;
            let on = font::pixel_on(bits, sx.min(7));
            if on {
                stamp(buffer, win_w, win_h, frame, x, y, fg);
            } else if let Some(bg) = bg {
                stamp(buffer, win_w, win_h, frame, x, y, bg);
            }
        }
    }
}

fn invert_cell(
    buffer: &mut [u32],
    win: (usize, usize),
    frame: &Frame,
    rect: CellRect,
    insert: bool,
) {
    let (win_w, win_h) = win;
    let height = rect.y1 - rect.y0;
    for y in rect.y0..rect.y1 {
        if insert && (y - rect.y0) * 2 < height {
            continue;
        }
        for x in rect.x0..rect.x1 {
            let wx = frame.off_x + x * frame.scale;
            let wy = frame.off_y + y * frame.scale;
            if wx >= win_w || wy >= win_h {
                continue;
            }
            let color = buffer[wy * win_w + wx] ^ 0x00FF_FFFF;
            stamp(buffer, win_w, win_h, frame, x, y, color);
        }
    }
}

fn stamp(
    buffer: &mut [u32],
    win_w: usize,
    win_h: usize,
    frame: &Frame,
    x: usize,
    y: usize,
    color: u32,
) {
    let x0 = frame.off_x + x * frame.scale;
    let y0 = frame.off_y + y * frame.scale;
    for dy in 0..frame.scale {
        for dx in 0..frame.scale {
            let px = x0 + dx;
            let py = y0 + dy;
            if px < win_w && py < win_h {
                buffer[py * win_w + px] = color;
            }
        }
    }
}

fn axis_span(index: usize, count: usize, total: usize) -> (usize, usize) {
    (index * total / count, (index + 1) * total / count)
}

fn map_key(event: &KeyEvent, mods: ModifiersState) -> Option<Input> {
    if event.state != ElementState::Pressed {
        return None;
    }
    let ctrl = mods.control_key();
    let alt = mods.alt_key();
    // Alt is reserved for KEY traps. AltGr (Ctrl+Alt) still types its character.
    if alt && !ctrl {
        return None;
    }
    if ctrl && !alt {
        if let PhysicalKey::Code(code) = event.physical_key {
            if let Some(input) = ctrl_physical(code) {
                return Some(input);
            }
        }
    }
    match &event.logical_key {
        Key::Named(named) => map_named(*named, ctrl),
        Key::Character(text) if ctrl && !alt => text.chars().next().and_then(ctrl_letter),
        Key::Character(logical) if !ctrl && !alt => {
            let source = event.text.as_deref().unwrap_or(logical.as_str());
            one_char(source).and_then(char_to_byte).map(Input::Char)
        }
        _ => None,
    }
}

fn one_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        None
    } else {
        Some(ch)
    }
}

/// Letter chords come from the physical key so Ctrl+M stays Enter and
/// Ctrl+Enter stays the line-join key. Windows often reports both as Enter.
fn ctrl_physical(code: KeyCode) -> Option<Input> {
    match code {
        KeyCode::KeyB => Some(Input::WordLeft),
        KeyCode::KeyC => Some(Input::CtrlC),
        KeyCode::KeyE => Some(Input::CtrlEnd),
        KeyCode::KeyF => Some(Input::WordRight),
        KeyCode::KeyG => Some(Input::Bell),
        KeyCode::KeyH => Some(Input::Backspace),
        KeyCode::KeyJ => Some(Input::CtrlEnter),
        KeyCode::KeyK => Some(Input::Home),
        KeyCode::KeyL => Some(Input::CtrlHome),
        KeyCode::KeyM => Some(Input::Enter),
        KeyCode::KeyN => Some(Input::End),
        KeyCode::KeyR => Some(Input::Insert),
        KeyCode::KeyS => Some(Input::Pause),
        KeyCode::KeyT => Some(Input::CtrlT),
        KeyCode::Digit6 => Some(Input::Up),
        KeyCode::Minus => Some(Input::Down),
        KeyCode::BracketRight => Some(Input::Left),
        KeyCode::Backslash => Some(Input::Right),
        KeyCode::BracketLeft => Some(Input::Escape),
        _ => None,
    }
}

fn map_named(key: NamedKey, ctrl: bool) -> Option<Input> {
    match key {
        NamedKey::ArrowUp => Some(Input::Up),
        NamedKey::ArrowDown => Some(Input::Down),
        NamedKey::ArrowLeft => Some(if ctrl { Input::WordLeft } else { Input::Left }),
        NamedKey::ArrowRight => Some(if ctrl { Input::WordRight } else { Input::Right }),
        NamedKey::Backspace => Some(if ctrl {
            Input::Delete
        } else {
            Input::Backspace
        }),
        NamedKey::Delete => Some(Input::Delete),
        NamedKey::Insert => Some(Input::Insert),
        NamedKey::Tab => Some(Input::Tab),
        NamedKey::Escape => Some(Input::Escape),
        NamedKey::Enter => Some(if ctrl { Input::CtrlEnter } else { Input::Enter }),
        NamedKey::End => Some(if ctrl { Input::CtrlEnd } else { Input::End }),
        NamedKey::Home => Some(if ctrl { Input::CtrlHome } else { Input::Home }),
        NamedKey::Space => Some(Input::Char(b' ')),
        NamedKey::F1 => Some(Input::Function(1)),
        NamedKey::F2 => Some(Input::Function(2)),
        NamedKey::F3 => Some(Input::Function(3)),
        NamedKey::F4 => Some(Input::Function(4)),
        NamedKey::F5 => Some(Input::Function(5)),
        NamedKey::F6 => Some(Input::Function(6)),
        NamedKey::F7 => Some(Input::Function(7)),
        NamedKey::F8 => Some(Input::Function(8)),
        NamedKey::F9 => Some(Input::Function(9)),
        NamedKey::F10 => Some(Input::Function(10)),
        NamedKey::PrintScreen => {
            if ctrl {
                Some(Input::ToggleEcho)
            } else {
                Some(Input::PrintScreen)
            }
        }
        // Ctrl+Pause and Ctrl+ScrollLock stand in for Ctrl+Break.
        NamedKey::Pause => Some(if ctrl { Input::Break } else { Input::Pause }),
        NamedKey::ScrollLock if ctrl => Some(Input::Break),
        NamedKey::NumLock if ctrl => Some(Input::Pause),
        _ => None,
    }
}

fn play_pending(session: &mut Session) {
    if let Some((freq, ms)) = session.take_tone() {
        start_tone(freq, ms);
    } else if session.take_beep() {
        start_tone(800, 200);
    }
}

fn start_tone(freq: u32, ms: u32) {
    std::thread::spawn(move || play_tone(freq, ms));
}

fn play_tone(freq: u32, ms: u32) {
    #[cfg(windows)]
    unsafe {
        // kernel32 Beep blocks, so the caller runs it off the window thread.
        let _ = Beep(freq, ms);
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("osascript")
            .args(["-e", "beep"])
            .status();
        let _ = (freq, ms);
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("paplay")
            .arg("/usr/share/sounds/freedesktop/stereo/bell.oga")
            .status();
        let _ = (freq, ms);
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn Beep(freq: u32, dur: u32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_letter_keys_match_the_editor_chords() {
        assert_eq!(ctrl_physical(KeyCode::KeyM), Some(Input::Enter));
        assert_eq!(ctrl_physical(KeyCode::KeyJ), Some(Input::CtrlEnter));
        assert_eq!(ctrl_physical(KeyCode::KeyC), Some(Input::CtrlC));
        assert_eq!(ctrl_physical(KeyCode::Digit6), Some(Input::Up));
        assert_eq!(ctrl_physical(KeyCode::BracketLeft), Some(Input::Escape));
        assert_eq!(ctrl_physical(KeyCode::Enter), None);
    }
}
