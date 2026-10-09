//! Text-mode window. 80×25 is 640×400 and 40×25 is 320×400, drawn with an
//! integer scale and a black letterbox. Row 25 is the soft-key strip when
//! KEY is on. The cursor is an inverted cell, or the lower half of one in
//! insert mode; it is not stored in the cell buffer.

use crate::font;
use crate::input::{char_to_byte, ctrl_letter, Input};
use crate::options::parse_args;
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

const PALETTE: [u32; 16] = [
    0x000000, 0x0000AA, 0x00AA00, 0x00AAAA, 0xAA0000, 0xAA00AA, 0xAA5500, 0xAAAAAA, 0x555555,
    0x5555FF, 0x55FF55, 0x55FFFF, 0xFF5555, 0xFF55FF, 0xFFFF55, 0xFFFFFF,
];

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = parse_args(&args)
        .map_err(|message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message))?;
    let mut session = Session::new();
    session.boot(&options);

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
                    if self.session.take_beep() {
                        start_beep();
                    }
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
        if self.session.take_beep() {
            start_beep();
        }
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

fn paint(buffer: &mut [u32], win_w: usize, win_h: usize, session: &Session) {
    if win_w == 0 || win_h == 0 || buffer.len() < win_w.saturating_mul(win_h) {
        return;
    }
    buffer[..win_w * win_h].fill(0);
    let screen = session.screen();
    let cols = screen.width();
    let pix_w = cols * CELL_W;
    let pix_h = ROWS * CELL_H;
    let mut scale = (win_w / pix_w).min(win_h / pix_h);
    if scale == 0 {
        scale = 1;
    }
    let dest_w = pix_w * scale;
    let dest_h = pix_h * scale;
    let off_x = win_w.saturating_sub(dest_w) / 2;
    let off_y = win_h.saturating_sub(dest_h) / 2;
    let (cur_row, cur_col) = screen.cursor();
    let insert = screen.insert_mode();
    let strip = session.key_strip();

    for row in 0..ROWS {
        for col in 0..cols {
            let strip_row = row + 1 == ROWS && strip.is_some();
            let (ch, attr) = if strip_row {
                (strip.as_ref().unwrap()[col], KEY_ATTR)
            } else if row < ROWS {
                let cell = screen.cell(row, col);
                (cell.ch, cell.attr)
            } else {
                (b' ', 0x07)
            };
            let glyph = font::glyph(ch);
            let fg = PALETTE[(attr & 0x0F) as usize];
            let bg = PALETTE[((attr >> 4) & 0x0F) as usize];
            let cursor_here = !strip_row && row == cur_row && col == cur_col;
            for gy in 0..CELL_H {
                let bits = glyph[gy];
                let invert = cursor_here && (!insert || gy >= CELL_H / 2);
                let (fg, bg) = if invert { (bg, fg) } else { (fg, bg) };
                for gx in 0..CELL_W {
                    let on = font::pixel_on(bits, gx);
                    let color = if on { fg } else { bg };
                    let x0 = off_x + (col * CELL_W + gx) * scale;
                    let y0 = off_y + (row * CELL_H + gy) * scale;
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let x = x0 + dx;
                            let y = y0 + dy;
                            if x < win_w && y < win_h {
                                buffer[y * win_w + x] = color;
                            }
                        }
                    }
                }
            }
        }
    }
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

fn start_beep() {
    #[cfg(windows)]
    {
        std::thread::spawn(|| unsafe {
            // kernel32 Beep blocks, so it stays off the window thread.
            let _ = Beep(800, 200);
        });
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
