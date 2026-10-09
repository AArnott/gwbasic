//! A GW-BASIC 3.23 environment: the screen is the program line, F1–F10 are
//! the real soft keys, and numbered lines run when you type `RUN`.

mod basic;
mod crunch;
mod font;
mod input;
mod keys;
mod mbf;
mod options;
mod program;
mod screen;
mod session;
mod tokens;
mod video;

pub use input::{char_to_byte, ctrl_letter, Input};
pub use options::{parse_args, Options};
pub use session::Session;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    video::run()
}
