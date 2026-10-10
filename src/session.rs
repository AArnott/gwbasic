//! Direct mode and the running program share one screen. Enter stores a
//! numbered line or runs a statement. `RUN` executes the program. Ctrl+Break
//! stops it; Ctrl+C does that only at the editor and at `INPUT`.

use crate::basic;
use crate::crunch::FaultKind;
use crate::files;
use crate::input::Input;
use crate::keys::{FunctionKeys, MACRO_LEN};
use crate::options::Options;
use crate::program::Program;
use crate::screen::Screen;
use std::collections::VecDeque;
use std::path::Path;

const SYNTAX: &str = "Syntax error";
const UNDEFINED: &str = "Undefined line number";
const ILLEGAL: &str = "Illegal function call";
const CANT_CONTINUE: &str = "Can't continue";
const FILE_NOT_FOUND: &str = "File not found";

/// Bytes GW-BASIC keeps for its own tables inside the 64K workspace.
/// 65536 − 5236 = 60300, the figure a stock interpreter prints at startup.
const WORKSPACE_OVERHEAD: u32 = 5236;
const DEFAULT_WORKSPACE: u32 = 65536;

pub struct Session {
    screen: Screen,
    program: Program,
    keys: FunctionKeys,
    vm: basic::Vm,
    /// Characters typed while a program is running. `INKEY$` reads them.
    inkey: VecDeque<u8>,
    workspace: u32,
    queue: VecDeque<u8>,
    /// Next AUTO line and the step. `next` may pass 65529, which ends AUTO.
    auto: Option<(u32, u32)>,
    auto_prompt: Option<String>,
    auto_just_started: bool,
    trace: bool,
    quit: bool,
    paused: bool,
    beep_pending: bool,
    tone: Option<(u32, u32)>,
    echo_printer: bool,
    lpt: String,
    files: files::Files,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    /// A clear 80-column screen with the soft keys on. No banner yet.
    pub fn new() -> Self {
        Self {
            screen: Screen::new(80),
            program: Program::default(),
            keys: FunctionKeys::standard(),
            vm: basic::Vm::new(),
            inkey: VecDeque::new(),
            workspace: DEFAULT_WORKSPACE,
            queue: VecDeque::new(),
            auto: None,
            auto_prompt: None,
            auto_just_started: false,
            trace: false,
            quit: false,
            paused: false,
            beep_pending: false,
            tone: None,
            echo_printer: false,
            lpt: String::new(),
            files: files::Files::new(3, 128),
        }
    }

    pub fn boot(&mut self, options: &Options) {
        self.vm.set_double_math(options.double_math);
        self.files = files::Files::new(options.files, options.record_size);
        self.workspace = options
            .max_memory
            .unwrap_or(DEFAULT_WORKSPACE)
            .min(DEFAULT_WORKSPACE);
        let load_error = options
            .program
            .as_ref()
            .and_then(|path| self.load_program_file(path).err());
        self.banner();
        if let Some(err) = load_error {
            self.error(&err);
        }
    }

    pub fn banner(&mut self) {
        let free = self.bytes_free();
        self.emit_str(&format!(
            "GW-BASIC 3.23\n\
             (C) Copyright Microsoft 1983,1984,1985,1986,1987,1988\n\
             {free} Bytes free\n\
             Ok\n"
        ));
    }

    pub fn bytes_free(&self) -> u32 {
        self.workspace
            .saturating_sub(WORKSPACE_OVERHEAD)
            .saturating_sub(self.program.stored_bytes() as u32)
    }

    pub fn handle(&mut self, input: Input) {
        if self.quit {
            return;
        }
        // Pause holds until the next key, and that key is discarded.
        if self.paused {
            self.paused = false;
            return;
        }
        if self.vm.waiting_keys() {
            self.handle_key_wait(input);
            return;
        }
        if self.vm.is_running() {
            self.handle_running(input);
            return;
        }
        match input {
            Input::Char(ch) => self.type_user_char(ch),
            Input::Up => self.move_arrow(|s| s.move_up()),
            Input::Down => self.move_arrow(|s| s.move_down()),
            Input::Left => self.move_arrow(|s| s.move_left()),
            Input::Right => self.move_arrow(|s| s.move_right()),
            Input::WordLeft => self.move_arrow(|s| s.word_left()),
            Input::WordRight => self.move_arrow(|s| s.word_right()),
            Input::Backspace => self.screen.backspace(),
            Input::Delete => self.screen.delete_char(),
            Input::Insert => self.screen.set_insert(!self.screen.insert_mode()),
            Input::Tab => self.screen.tab(),
            Input::Escape => self.screen.escape_line(),
            Input::CtrlEnd => self.screen.erase_to_end(),
            Input::End => self.screen.end_of_line(),
            Input::Home => self.screen.home(),
            Input::CtrlHome => self.screen.clear(),
            Input::Enter => {
                if self.vm.waiting_input() {
                    self.submit_input();
                } else {
                    self.on_enter();
                }
            }
            Input::CtrlEnter => self.screen.ctrl_enter(),
            Input::Break => self.on_break(),
            Input::CtrlC => self.on_break(),
            Input::Bell => self.beep_pending = true,
            Input::Pause => self.paused = true,
            Input::CtrlT => self.scroll_soft_keys(),
            Input::Function(n) => self.push_macro(n),
            Input::PrintScreen => self.copy_screen_to_printer(),
            Input::ToggleEcho => self.echo_printer = !self.echo_printer,
        }
        self.drain_queue();
    }

    /// `\n` is Enter. `\r` is ignored so `\r\n` is a single Enter.
    pub fn type_ascii(&mut self, text: &str) {
        for ch in text.chars() {
            if ch == '\n' {
                self.handle(Input::Enter);
            } else if ch != '\r' {
                if let Some(byte) = crate::input::char_to_byte(ch) {
                    self.handle(Input::Char(byte));
                }
            }
        }
    }

    pub fn snapshot(&self) -> String {
        self.screen.snapshot()
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    pub fn cursor(&self) -> (usize, usize) {
        self.screen.cursor()
    }

    pub fn insert_mode(&self) -> bool {
        self.screen.insert_mode()
    }

    pub fn program_line(&self, number: u16) -> Option<&str> {
        self.program.get(number)
    }

    pub fn key_strip(&self) -> Option<Vec<u8>> {
        if self.keys.on {
            Some(self.keys.strip(self.screen.width()))
        } else {
            None
        }
    }

    pub fn trace(&self) -> bool {
        self.trace
    }

    pub fn quit(&self) -> bool {
        self.quit
    }

    pub fn lpt(&self) -> &str {
        &self.lpt
    }

    pub fn take_beep(&mut self) -> bool {
        let pending = self.beep_pending;
        self.beep_pending = false;
        pending
    }

    pub fn take_tone(&mut self) -> Option<(u32, u32)> {
        self.tone.take()
    }

    /// A program is between statements and the window should keep polling so
    /// Ctrl+Break can land.
    pub fn needs_poll(&self) -> bool {
        self.vm.is_running()
    }

    /// Run a short slice of a program that did not finish inside `Enter`.
    pub fn poll(&mut self) {
        if self.vm.waiting_keys() {
            if self.inkey.len() < self.vm.keys_needed() {
                return;
            }
            self.vm.resume_keys();
        }
        if self.vm.is_running() {
            let status = self.burst(4_000);
            let _ = self.apply_status(status);
        }
    }

    fn handle_key_wait(&mut self, input: Input) {
        match input {
            Input::Break | Input::CtrlC => self.on_break(),
            Input::Char(ch) => self.inkey.push_back(ch),
            Input::Enter => self.inkey.push_back(b'\r'),
            Input::Function(n) if (1..=10).contains(&n) => {
                self.inkey.extend(self.keys.get(n).iter().copied());
            }
            _ => return,
        }
        self.poll();
    }

    fn move_arrow(&mut self, step: impl FnOnce(&mut Screen)) {
        // Arrow keys and word-arrows leave insert mode.
        self.screen.set_insert(false);
        step(&mut self.screen);
    }

    fn type_user_char(&mut self, ch: u8) {
        if self.echo_printer {
            self.lpt.push(char::from(ch));
        }
        self.screen.type_char(ch);
    }

    fn emit_str(&mut self, text: &str) {
        if self.echo_printer {
            self.lpt.push_str(text);
        }
        self.screen.print_str(text);
    }

    fn emit_bytes(&mut self, bytes: &[u8]) {
        if self.echo_printer {
            for &byte in bytes {
                self.lpt.push(char::from(byte));
            }
        }
        self.screen.print_bytes(bytes);
    }

    fn print_ok(&mut self) {
        self.emit_str("Ok\n");
    }

    fn error(&mut self, message: &str) {
        self.emit_str(message);
        self.emit_str("\n");
        self.print_ok();
    }

    fn handle_running(&mut self, input: Input) {
        match input {
            Input::Break => self.on_break(),
            Input::CtrlC => {}
            Input::Char(ch) => self.inkey.push_back(ch),
            Input::Enter => self.inkey.push_back(b'\r'),
            Input::Function(n) if (1..=10).contains(&n) => {
                self.inkey.extend(self.keys.get(n).iter().copied());
            }
            Input::Function(_) => {}
            _ => {}
        }
    }

    fn on_break(&mut self) {
        if self.vm.is_running() || self.vm.waiting_input() || self.vm.waiting_keys() {
            let line = self.vm.break_line();
            self.vm.request_break();
            self.auto = None;
            self.auto_prompt = None;
            self.auto_just_started = false;
            self.print_break(line);
            return;
        }
        self.auto = None;
        self.auto_prompt = None;
        self.auto_just_started = false;
        self.screen.set_insert(false);
        self.screen.break_line();
        self.print_ok();
    }

    fn on_enter(&mut self) {
        let was_auto = self.auto.is_some();
        let text = self.screen.take_logical_line();
        if was_auto && self.unchanged_auto_line(&text) {
            self.bump_auto();
            return;
        }
        self.auto_just_started = false;
        self.dispatch(&text);
        if self.quit {
            return;
        }
        if was_auto && !self.auto_just_started && self.auto.is_some() {
            self.bump_auto();
        }
        self.auto_just_started = false;
    }

    fn unchanged_auto_line(&self, text: &str) -> bool {
        // Enter on the generated number, on the "number*", or on a blank
        // prompt skips that line and does not delete it.
        if text.is_empty() {
            return true;
        }
        if self.auto_prompt.as_deref() == Some(text) {
            return true;
        }
        let Some((next, _)) = self.auto else {
            return false;
        };
        if next > 65529 {
            return false;
        }
        let number = (next as u16).to_string();
        text == number || text == format!("{number}*")
    }

    fn bump_auto(&mut self) {
        let Some((next, step)) = self.auto else {
            return;
        };
        self.auto = Some((next.saturating_add(step), step));
        self.paint_auto_prompt();
    }

    fn paint_auto_prompt(&mut self) {
        let Some((next, _)) = self.auto else {
            return;
        };
        if next > 65529 {
            self.auto = None;
            self.auto_prompt = None;
            self.print_ok();
            return;
        }
        let number = (next as u16).to_string();
        self.emit_str(&number);
        let mut shown = number;
        if self.program.contains(next as u16) {
            // The asterisk sits under the cursor so the next typed character
            // replaces it instead of becoming part of the line.
            self.type_user_char(b'*');
            self.screen.move_left();
            shown.push('*');
        }
        self.auto_prompt = Some(shown);
    }

    fn push_macro(&mut self, n: u8) {
        if (1..=10).contains(&n) {
            self.queue.extend(self.keys.get(n).iter().copied());
        }
    }

    fn drain_queue(&mut self) {
        let mut steps = 0;
        while let Some(byte) = self.queue.pop_front() {
            steps += 1;
            if steps > 64 {
                break;
            }
            match byte {
                b'\r' | b'\n' => {
                    if self.vm.waiting_input() {
                        self.submit_input();
                    } else {
                        self.on_enter();
                    }
                }
                0x07 => self.beep_pending = true,
                ch => self.type_user_char(ch),
            }
            if self.quit {
                break;
            }
        }
    }

    fn scroll_soft_keys(&mut self) {
        // Width 80 already shows every key, so Ctrl+T does nothing there.
        if self.keys.on && self.screen.width() < 80 {
            self.keys.offset = (self.keys.offset + 1) % 10;
        }
    }

    fn copy_screen_to_printer(&mut self) {
        let width = self.screen.width();
        let rows = if self.keys.on {
            self.screen.editable_rows()
        } else {
            crate::screen::SCREEN_ROWS
        };
        for row in 0..rows {
            let mut line = String::with_capacity(width);
            for col in 0..width {
                line.push(char::from(self.screen.cell(row, col).ch));
            }
            while line.ends_with(' ') {
                line.pop();
            }
            self.lpt.push_str(&line);
            self.lpt.push('\n');
        }
        if let Some(strip) = self.key_strip() {
            let text: String = strip.iter().copied().map(char::from).collect();
            self.lpt.push_str(text.trim_end());
            self.lpt.push('\n');
        }
    }

    fn dispatch(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            self.print_ok();
            return;
        }
        if text.starts_with('\'') {
            self.print_ok();
            return;
        }
        match split_program_line(text) {
            Some(Ok((number, rest))) => {
                self.store_program_line(number, rest);
                return;
            }
            Some(Err(())) => {
                self.error(SYNTAX);
                return;
            }
            None => {}
        }
        let (command, rest) = split_command(text);
        if command.is_empty() {
            self.exec_direct(text);
            return;
        }
        match command.to_ascii_uppercase().as_str() {
            "NEW" => self.cmd_new(rest),
            "LIST" => self.cmd_list(rest),
            "EDIT" => self.cmd_edit(rest),
            "DELETE" => self.cmd_delete(rest),
            "AUTO" => self.cmd_auto(rest),
            "SYSTEM" => self.cmd_system(rest),
            "KEY" => self.cmd_key(rest),
            "RUN" => self.cmd_run(rest),
            "CONT" => self.cmd_bare(rest, |session| {
                if session.vm.can_continue() && basic::resume(&mut session.vm) {
                    session.pump_until_settled();
                } else {
                    session.error(CANT_CONTINUE);
                }
            }),
            "TRON" => self.cmd_bare(rest, |session| {
                session.trace = true;
                session.print_ok();
            }),
            "TROFF" => self.cmd_bare(rest, |session| {
                session.trace = false;
                session.print_ok();
            }),
            "WIDTH" => self.cmd_width(rest),
            "CLS" => self.cmd_bare(rest, |session| {
                session.screen.clear();
                session.print_ok();
            }),
            "REM" => self.print_ok(),
            _ => self.exec_direct(text),
        }
    }

    fn refuse_protected(&mut self) -> bool {
        if self.program.is_protected() {
            self.error(ILLEGAL);
            true
        } else {
            false
        }
    }

    fn store_program_line(&mut self, number: u16, rest: &str) {
        if self.refuse_protected() {
            return;
        }
        match put_line(&mut self.program, number, rest) {
            Ok(()) => self.vm.invalidate_cont(),
            Err(fault) => {
                let column = number.to_string().len() + fault.column;
                self.emit_str(&format!("{}^\n", " ".repeat(column)));
                let message = match fault.kind {
                    FaultKind::Overflow => "Overflow",
                    FaultKind::Missing => "Missing operand",
                    FaultKind::Syntax => SYNTAX,
                };
                self.error(message);
            }
        }
    }

    fn exec_direct(&mut self, text: &str) {
        match basic::prepare(text) {
            Ok(tokens) => {
                basic::begin_direct(&mut self.vm, tokens);
                self.pump_until_settled();
            }
            Err(fault) => {
                self.emit_str(&format!("{}^\n", " ".repeat(fault.column)));
                let message = match fault.kind {
                    FaultKind::Overflow => "Overflow",
                    FaultKind::Missing => "Missing operand",
                    FaultKind::Syntax => SYNTAX,
                };
                self.error(message);
            }
        }
    }

    fn burst(&mut self, budget: u32) -> basic::Status {
        self.vm.set_free(self.bytes_free() as i32);
        let mut host = basic::Host {
            program: &mut self.program,
            screen: &mut self.screen,
            beep: &mut self.beep_pending,
            lpt: &mut self.lpt,
            echo: self.echo_printer,
            inkey: &mut self.inkey,
            key_offset: &mut self.keys.offset,
            trace: &mut self.trace,
            quit: &mut self.quit,
            files: &mut self.files,
            tone: &mut self.tone,
        };
        basic::burst(&mut self.vm, &mut host, budget)
    }

    fn pump_until_settled(&mut self) {
        for _ in 0..80 {
            if !self.vm.is_running() {
                return;
            }
            let status = self.burst(8_000);
            if self.apply_status(status) {
                return;
            }
        }
    }

    /// `true` when the program stopped and the caller should not pump again.
    fn apply_status(&mut self, status: basic::Status) -> bool {
        match status {
            basic::Status::Idle | basic::Status::Yield => false,
            basic::Status::Ended => {
                self.print_ok();
                true
            }
            basic::Status::Break { line } => {
                self.print_break(line);
                true
            }
            basic::Status::Error { message, line } => {
                self.runtime_error(message, line);
                true
            }
            basic::Status::Input => {
                let prompt = basic::input_prompt(&self.vm).to_string();
                self.emit_str(&prompt);
                true
            }
            basic::Status::Quit => true,
        }
    }

    fn print_break(&mut self, line: Option<u16>) {
        match line {
            Some(n) => self.emit_str(&format!("Break in {n}\n")),
            None => self.emit_str("Break\n"),
        }
        self.print_ok();
    }

    fn runtime_error(&mut self, message: &str, line: Option<u16>) {
        match line {
            Some(n) => self.error(&format!("{message} in {n}")),
            None => self.error(message),
        }
    }

    fn submit_input(&mut self) {
        let text = self.screen.take_logical_line();
        match basic::finish_input(&mut self.vm, &text) {
            Ok(()) => self.pump_until_settled(),
            Err(msg) if msg == basic::REDO => {
                let prompt = basic::input_prompt(&self.vm).to_string();
                self.emit_str("Redo from start\n");
                self.emit_str(&prompt);
            }
            Err(msg) => {
                let line = self.vm.break_line();
                self.vm.abandon();
                self.runtime_error(msg, line);
            }
        }
    }

    fn cmd_bare(&mut self, rest: &str, then: impl FnOnce(&mut Self)) {
        if rest.trim().is_empty() {
            then(self);
        } else {
            self.error(SYNTAX);
        }
    }

    fn cmd_new(&mut self, rest: &str) {
        self.cmd_bare(rest, |session| {
            session.program.clear();
            session.vm.reset_runtime();
            session.print_ok();
        });
    }

    fn cmd_system(&mut self, rest: &str) {
        if rest.trim().is_empty() {
            self.quit = true;
        } else {
            self.error(SYNTAX);
        }
    }

    fn cmd_run(&mut self, rest: &str) {
        let rest = rest.trim();
        let start = if rest.is_empty() {
            None
        } else {
            match parse_line_ref(rest, self.program.last) {
                Ok(number) => Some(number),
                Err(err) => {
                    self.error(err);
                    return;
                }
            }
        };
        if basic::begin_program(&mut self.vm, &self.program, start) {
            self.pump_until_settled();
        } else {
            self.print_ok();
        }
    }

    fn cmd_list(&mut self, rest: &str) {
        if self.refuse_protected() {
            return;
        }
        let (start, end) = match parse_range(rest, self.program.last) {
            Ok(range) => range,
            Err(err) => {
                self.error(err);
                return;
            }
        };
        let lines: Vec<String> = self
            .program
            .in_range(start, end)
            .into_iter()
            .map(|(_, text)| text.to_string())
            .collect();
        for line in lines {
            self.emit_str(&line);
            self.emit_str("\n");
        }
        self.print_ok();
    }

    fn cmd_delete(&mut self, rest: &str) {
        if self.refuse_protected() {
            return;
        }
        if rest.trim().is_empty() {
            self.error(SYNTAX);
            return;
        }
        let (start, end) = match parse_range(rest, self.program.last) {
            Ok(range) => range,
            Err(err) => {
                self.error(err);
                return;
            }
        };
        self.program.delete_range(start, end);
        self.vm.invalidate_cont();
        self.print_ok();
    }

    fn cmd_edit(&mut self, rest: &str) {
        if self.refuse_protected() {
            return;
        }
        let rest = rest.trim();
        let number = if rest.is_empty() {
            match self.program.last {
                Some(number) => number,
                None => {
                    self.error(UNDEFINED);
                    return;
                }
            }
        } else {
            let mut parts = rest.split_whitespace();
            let Some(token) = parts.next() else {
                self.error(SYNTAX);
                return;
            };
            if parts.next().is_some() {
                self.error(ILLEGAL);
                return;
            }
            match parse_line_ref(token, self.program.last) {
                Ok(number) => number,
                Err(err) => {
                    self.error(err);
                    return;
                }
            }
        };
        self.show_for_edit(number);
    }

    fn show_for_edit(&mut self, number: u16) {
        let Some(text) = self.program.get(number).map(str::to_string) else {
            self.error(UNDEFINED);
            return;
        };
        self.program.last = Some(number);
        let column = statement_column(&text);
        let length = text.len();
        self.emit_str(&text);
        for _ in column..length {
            self.screen.move_left();
        }
    }

    fn cmd_auto(&mut self, rest: &str) {
        if self.refuse_protected() {
            return;
        }
        let (start, step) = match parse_auto(rest, self.program.last) {
            Ok(pair) => pair,
            Err(err) => {
                self.error(err);
                return;
            }
        };
        self.auto = Some((u32::from(start), step));
        self.auto_just_started = true;
        self.paint_auto_prompt();
    }

    fn cmd_key(&mut self, rest: &str) {
        let rest = rest.trim();
        if rest.is_empty() {
            self.error(SYNTAX);
            return;
        }
        let upper = rest.to_ascii_uppercase();
        if upper == "ON" {
            self.keys.on = true;
            self.screen.set_keys_on(true);
            self.print_ok();
            return;
        }
        if upper == "OFF" {
            self.keys.on = false;
            self.screen.set_keys_on(false);
            self.print_ok();
            return;
        }
        if upper == "LIST" {
            let lines = self.keys.list_lines();
            for line in lines {
                self.emit_bytes(&line);
                self.emit_str("\n");
            }
            self.print_ok();
            return;
        }
        let (number, after) = match take_u32(rest) {
            Some((number, after)) => (number, after),
            None => {
                self.error(SYNTAX);
                return;
            }
        };
        if !(1..=10).contains(&number) {
            self.error(ILLEGAL);
            return;
        }
        let after = after.trim_start();
        if !after.starts_with(',') {
            self.error(SYNTAX);
            return;
        }
        match parse_key_string(after[1..].trim_start()) {
            Ok(bytes) => {
                self.keys.set(number as u8, &bytes);
                self.print_ok();
            }
            Err(err) => self.error(err),
        }
    }

    fn cmd_width(&mut self, rest: &str) {
        let rest = rest.trim();
        let Some((width, after)) = take_u32(rest) else {
            self.error(SYNTAX);
            return;
        };
        if !after.trim().is_empty() {
            self.error(SYNTAX);
            return;
        }
        if width != 40 && width != 80 {
            self.error(ILLEGAL);
            return;
        }
        if !self.screen.set_width(width as usize) {
            self.error(ILLEGAL);
            return;
        }
        self.keys.offset = 0;
        self.print_ok();
    }

    fn load_program_file(&mut self, path: &Path) -> Result<(), String> {
        let path = resolve_bas(path);
        let bytes = std::fs::read(&path).map_err(|_| FILE_NOT_FOUND.to_string())?;
        let mut program = Program::default();
        basic::load_bytes(&mut program, &bytes).map_err(|err| err.to_string())?;
        self.program = program;
        Ok(())
    }
}

fn put_line(program: &mut Program, number: u16, rest: &str) -> Result<(), crate::crunch::Fault> {
    basic::store_source_line(program, number, rest)
}

fn resolve_bas(path: &Path) -> std::path::PathBuf {
    if path.extension().is_some() {
        path.to_path_buf()
    } else {
        path.with_extension("BAS")
    }
}

/// A leading line number in 0..=65529, plus the untouched tail (which keeps
/// the single space after the digits). `None` means this is not a program line.
fn split_program_line(text: &str) -> Option<Result<(u16, &str), ()>> {
    let bytes = text.as_bytes();
    match bytes.first() {
        Some(b) if b.is_ascii_digit() => {}
        _ => return None,
    }
    let mut index = 0;
    let mut number: u32 = 0;
    let mut overflow = false;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        number = number
            .saturating_mul(10)
            .saturating_add(u32::from(bytes[index] - b'0'));
        if number > 65529 {
            overflow = true;
        }
        index += 1;
    }
    if overflow {
        Some(Err(()))
    } else {
        Some(Ok((number as u16, &text[index..])))
    }
}

fn split_command(text: &str) -> (&str, &str) {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
        index += 1;
    }
    (&text[..index], &text[index..])
}

fn statement_column(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index < bytes.len() && bytes[index] == b' ' {
        index += 1;
    }
    index
}

fn parse_range(text: &str, last: Option<u16>) -> Result<(Option<u16>, Option<u16>), &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return Ok((None, None));
    }
    if let Some((left, right)) = text.split_once('-') {
        let start = if left.trim().is_empty() {
            None
        } else {
            Some(parse_line_ref(left.trim(), last)?)
        };
        let end = if right.trim().is_empty() {
            None
        } else {
            Some(parse_line_ref(right.trim(), last)?)
        };
        if start.is_none() && end.is_none() {
            return Err(SYNTAX);
        }
        Ok((start, end))
    } else {
        let number = parse_line_ref(text, last)?;
        Ok((Some(number), Some(number)))
    }
}

fn parse_line_ref(text: &str, last: Option<u16>) -> Result<u16, &'static str> {
    if text == "." {
        last.ok_or(UNDEFINED)
    } else {
        parse_u16_line(text)
    }
}

fn parse_u16_line(text: &str) -> Result<u16, &'static str> {
    let number = parse_u32_token(text)?;
    if number > 65529 {
        Err(SYNTAX)
    } else {
        Ok(number as u16)
    }
}

fn parse_u32_token(text: &str) -> Result<u32, &'static str> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(SYNTAX);
    }
    text.parse().map_err(|_| SYNTAX)
}

fn take_u32(text: &str) -> Option<(u32, &str)> {
    let bytes = text.as_bytes();
    let mut index = 0;
    if index >= bytes.len() || !bytes[index].is_ascii_digit() {
        return None;
    }
    let mut number: u32 = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        number = number
            .saturating_mul(10)
            .saturating_add(u32::from(bytes[index] - b'0'));
        index += 1;
    }
    Some((number, &text[index..]))
}

fn parse_auto(text: &str, last: Option<u16>) -> Result<(u16, u32), &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return Ok((10, 10));
    }
    let (start_text, step_text) = match text.split_once(',') {
        Some((start, step)) => (start.trim(), Some(step.trim())),
        None => (text, None),
    };
    let start = if start_text.is_empty() {
        10
    } else {
        parse_line_ref(start_text, last)?
    };
    let step = match step_text {
        None => 10,
        Some("") => 10,
        Some(".") => u32::from(last.ok_or(UNDEFINED)?),
        Some(step) => {
            let step = parse_u32_token(step)?;
            if step == 0 {
                return Err(ILLEGAL);
            }
            step
        }
    };
    Ok((start, step))
}

fn parse_key_string(text: &str) -> Result<Vec<u8>, &'static str> {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut out = Vec::new();
    let mut saw_piece = false;
    loop {
        index = skip_spaces(bytes, index);
        if index >= bytes.len() {
            break;
        }
        if saw_piece {
            if bytes[index] != b'+' {
                return Err(SYNTAX);
            }
            index += 1;
            index = skip_spaces(bytes, index);
            if index >= bytes.len() {
                return Err(SYNTAX);
            }
        }
        if bytes[index] == b'"' {
            index += 1;
            while index < bytes.len() && bytes[index] != b'"' {
                if out.len() < MACRO_LEN {
                    out.push(bytes[index]);
                }
                index += 1;
            }
            if index >= bytes.len() {
                return Err(SYNTAX);
            }
            index += 1;
        } else if starts_with_ignore_ascii(&bytes[index..], b"CHR$") {
            index += 4;
            let (value, next) = parse_chr_arg(bytes, index)?;
            if out.len() < MACRO_LEN {
                out.push(value);
            }
            index = next;
        } else {
            return Err(SYNTAX);
        }
        saw_piece = true;
    }
    if !saw_piece {
        return Err(SYNTAX);
    }
    Ok(out)
}

fn parse_chr_arg(bytes: &[u8], mut index: usize) -> Result<(u8, usize), &'static str> {
    index = skip_spaces(bytes, index);
    if index >= bytes.len() || bytes[index] != b'(' {
        return Err(SYNTAX);
    }
    index += 1;
    index = skip_spaces(bytes, index);
    let start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == start {
        return Err(SYNTAX);
    }
    let text = std::str::from_utf8(&bytes[start..index]).map_err(|_| SYNTAX)?;
    let number: u32 = text.parse().unwrap_or(u32::MAX);
    if number > 255 {
        return Err(ILLEGAL);
    }
    index = skip_spaces(bytes, index);
    if index >= bytes.len() || bytes[index] != b')' {
        return Err(SYNTAX);
    }
    Ok((number as u8, index + 1))
}

fn skip_spaces(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index] == b' ' {
        index += 1;
    }
    index
}

fn starts_with_ignore_ascii(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len() && haystack[..needle.len()].eq_ignore_ascii_case(needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Input;

    fn fresh() -> Session {
        Session::new()
    }

    #[test]
    fn blank_enter_prints_ok() {
        let mut session = fresh();
        session.type_ascii("\n");
        // The blank row that was submitted stays, and Ok is printed under it.
        assert_eq!(session.snapshot().trim(), "Ok");
    }

    #[test]
    fn numbered_line_is_stored_without_ok() {
        let mut session = fresh();
        session.type_ascii("10 REM A\n");
        assert_eq!(session.program_line(10), Some("10 REM A"));
        assert!(!session.snapshot().contains("Ok"));
    }

    #[test]
    fn leading_zeros_are_canonical_and_a_bare_number_deletes() {
        let mut session = fresh();
        session.type_ascii("   00010 PRINT\n");
        assert_eq!(session.program_line(10), Some("10 PRINT"));
        session.type_ascii("10\n");
        assert!(session.program_line(10).is_none());
    }

    #[test]
    fn enter_in_the_middle_submits_the_whole_line() {
        let mut session = fresh();
        session.type_ascii("10 REM SIDE");
        for _ in 0..4 {
            session.handle(Input::Left);
        }
        session.handle(Input::Enter);
        assert_eq!(session.program_line(10), Some("10 REM SIDE"));
    }

    #[test]
    fn line_above_65529_is_a_syntax_error_and_65529_stores() {
        let mut session = fresh();
        session.type_ascii("65529 REM\n");
        assert_eq!(session.program_line(65529), Some("65529 REM"));
        session.type_ascii("65530 REM\n");
        assert!(session.snapshot().contains(SYNTAX));
    }

    #[test]
    fn escape_then_enter_does_not_store() {
        let mut session = fresh();
        session.type_ascii("10 REM Z");
        session.handle(Input::Escape);
        session.handle(Input::Enter);
        assert!(session.program_line(10).is_none());
        assert!(session.snapshot().contains("Ok"));
    }

    #[test]
    fn f1_types_list_and_f2_runs_and_f3_stays_typed() {
        let mut listed = fresh();
        listed.handle(Input::Function(1));
        assert!(listed.snapshot().starts_with("LIST"));
        assert!(!listed.snapshot().contains("Ok"));

        let mut ran = fresh();
        ran.handle(Input::Function(2));
        assert!(ran.snapshot().contains("RUN"));
        assert!(ran.snapshot().contains("Ok"));

        let mut load = fresh();
        load.handle(Input::Function(3));
        assert!(load.snapshot().starts_with("LOAD\""));
        assert!(!load.snapshot().contains("Ok"));
    }

    #[test]
    fn f10_clears_the_screen_and_prints_ok() {
        let mut session = fresh();
        session.handle(Input::Function(10));
        assert_eq!(session.snapshot(), "Ok");
    }

    #[test]
    fn list_then_edit_the_listed_line() {
        let mut session = fresh();
        session.type_ascii("10 REM A\nLIST\n");
        session.handle(Input::Up);
        session.handle(Input::Up);
        session.handle(Input::End);
        session.handle(Input::Backspace);
        session.handle(Input::Char(b'B'));
        session.handle(Input::Enter);
        assert_eq!(session.program_line(10), Some("10 REM B"));
    }

    #[test]
    fn edit_places_the_cursor_on_the_statement_and_a_missing_line_errors() {
        let mut session = fresh();
        session.type_ascii("10 REM A\nEDIT 10\n");
        assert_eq!(session.cursor().1, 3);
        assert!(!session.snapshot().contains("Ok"));
        assert_eq!(session.screen().cell(session.cursor().0, 3).ch, b'R');

        session.type_ascii("\nEDIT 99\n");
        assert!(session.snapshot().contains(UNDEFINED));
    }

    #[test]
    fn edit_with_trailing_junk_is_illegal() {
        let mut session = fresh();
        session.type_ascii("10 REM A\nEDIT 10 FOO\n");
        assert!(session.snapshot().contains(ILLEGAL));
    }

    #[test]
    fn auto_star_skips_without_deleting_and_steps() {
        let mut session = fresh();
        session.type_ascii("10 REM A\nAUTO\n");
        assert_eq!(session.snapshot().lines().last(), Some("10*"));
        let (row, col) = session.cursor();
        assert_eq!(session.screen().cell(row, col).ch, b'*');
        session.handle(Input::Enter);
        assert_eq!(session.program_line(10), Some("10 REM A"));
        assert_eq!(session.snapshot().lines().last(), Some("20"));

        session.type_ascii(" REM B\n");
        assert_eq!(session.program_line(20), Some("20 REM B"));
    }

    #[test]
    fn auto_step_and_a_zero_step_is_illegal() {
        let mut session = fresh();
        session.type_ascii("AUTO 100,50\n");
        assert_eq!(session.snapshot().lines().last(), Some("100"));
        session.handle(Input::Enter);
        assert_eq!(session.snapshot().lines().last(), Some("150"));

        let mut bad = fresh();
        bad.type_ascii("AUTO 10,0\n");
        assert!(bad.snapshot().contains(ILLEGAL));
    }

    #[test]
    fn delete_range_and_delete_without_arguments() {
        let mut session = fresh();
        session.type_ascii("10 REM A\n20 REM B\n30 REM C\nDELETE 10-20\n");
        assert!(session.program_line(10).is_none());
        assert!(session.program_line(20).is_none());
        assert_eq!(session.program_line(30), Some("30 REM C"));

        let mut bare = fresh();
        bare.type_ascii("DELETE\n");
        assert!(bare.snapshot().contains(SYNTAX));
    }

    #[test]
    fn new_clears_the_program_and_cls_clears_only_the_screen() {
        let mut session = fresh();
        session.type_ascii("10 REM A\nNEW\n");
        assert!(session.program_line(10).is_none());
        assert!(session.snapshot().contains("Ok"));

        let mut cleared = fresh();
        cleared.type_ascii("10 REM A\nCLS\n");
        assert_eq!(cleared.program_line(10), Some("10 REM A"));
        assert_eq!(cleared.snapshot(), "Ok");
    }

    #[test]
    fn width_40_ctrl_t_scrolls_the_strip() {
        let mut session = fresh();
        session.type_ascii("WIDTH 40\n");
        assert_eq!(session.key_strip().unwrap()[0], b'1');
        session.handle(Input::CtrlT);
        assert_eq!(session.key_strip().unwrap()[0], b'2');
        session.handle(Input::CtrlT);
        session.handle(Input::CtrlT);
        session.handle(Input::CtrlT);
        session.handle(Input::CtrlT);
        session.handle(Input::CtrlT);
        session.handle(Input::CtrlT);
        session.handle(Input::CtrlT);
        session.handle(Input::CtrlT);
        session.handle(Input::CtrlT);
        assert_eq!(session.key_strip().unwrap()[0], b'1');
    }

    #[test]
    fn pause_consumes_the_next_key_and_arrows_leave_insert_mode() {
        let mut session = fresh();
        session.handle(Input::Char(b'A'));
        session.handle(Input::Pause);
        session.handle(Input::Char(b'B'));
        session.handle(Input::Char(b'C'));
        assert_eq!(session.snapshot(), "AC");

        session.handle(Input::Insert);
        assert!(session.insert_mode());
        session.handle(Input::Left);
        assert!(!session.insert_mode());
    }

    #[test]
    fn tron_troff_cont_and_an_unknown_command() {
        let mut session = fresh();
        session.type_ascii("TRON\n");
        assert!(session.trace());
        session.type_ascii("TROFF\n");
        assert!(!session.trace());
        session.type_ascii("CONT\n");
        assert!(session.snapshot().contains(CANT_CONTINUE));
        session.type_ascii("FOO\n");
        assert!(session.snapshot().contains(SYNTAX));
    }

    #[test]
    fn banner_reports_gw_basic_323_and_60300_bytes() {
        let mut session = fresh();
        session.banner();
        let snapshot = session.snapshot();
        assert!(snapshot.contains("GW-BASIC 3.23"));
        assert!(snapshot.contains("60300 Bytes free"));
        assert!(snapshot.contains("Ok"));
    }

    #[test]
    fn memory_switch_changes_bytes_free() {
        let options = Options {
            max_memory: Some(32768),
            ..Options::default()
        };
        let mut session = fresh();
        session.boot(&options);
        assert!(session.snapshot().contains("27532 Bytes free"));
    }

    #[test]
    fn system_quits_without_ok() {
        let mut session = fresh();
        session.type_ascii("SYSTEM\n");
        assert!(session.quit());
        assert!(!session.snapshot().contains("Ok"));
    }

    #[test]
    fn key_assignment_executes_a_trailing_return_and_key_off_hides_the_strip() {
        let mut session = fresh();
        session.type_ascii("KEY 1,\"AB\"+CHR$(13)\n");
        session.handle(Input::Function(1));
        assert!(session.snapshot().contains("AB"));
        assert!(session.snapshot().contains(SYNTAX));

        session.type_ascii("KEY OFF\n");
        assert!(session.key_strip().is_none());
        session.type_ascii("KEY 11,\"Z\"\n");
        assert!(session.snapshot().contains(ILLEGAL));
    }

    #[test]
    fn break_leaves_auto_without_storing() {
        let mut session = fresh();
        session.type_ascii("AUTO 30,10\n");
        session.type_ascii("REM NO");
        session.handle(Input::Break);
        assert!(session.program_line(30).is_none());
        assert!(session.snapshot().contains("Ok"));
        assert_eq!(session.snapshot().lines().last(), Some("Ok"));
    }

    #[test]
    fn loads_ascii_before_the_banner_and_stays_up_when_load_fails() {
        let hello = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/hello.bas");
        let options = Options {
            program: Some(hello),
            ..Options::default()
        };
        let mut session = fresh();
        session.boot(&options);
        assert!(session.program_line(10).unwrap().contains("Hello"));
        assert!(session.snapshot().contains("GW-BASIC 3.23"));
        assert!(!session.snapshot().contains("60300 Bytes free"));
        assert!(!session.quit());

        let missing = Options {
            program: Some(std::path::PathBuf::from("no-such-gwbasic-program")),
            ..Options::default()
        };
        let mut session = fresh();
        session.boot(&missing);
        assert!(session.snapshot().contains(FILE_NOT_FOUND));
        assert!(session.snapshot().contains("60300 Bytes free"));
        assert!(!session.quit());

        let path = std::env::temp_dir().join("gwbasic-milestone1-token.bas");
        std::fs::write(&path, [0xFF, 0x01, 0x00]).unwrap();
        let tokenized = Options {
            program: Some(path),
            ..Options::default()
        };
        let mut session = fresh();
        session.boot(&tokenized);
        assert!(session.snapshot().contains(ILLEGAL));
        assert!(session.program_line(10).is_none());
        assert!(!session.quit());
    }

    #[test]
    fn printer_echo_and_print_screen_append_to_the_lpt_buffer() {
        let mut session = fresh();
        session.handle(Input::ToggleEcho);
        session.handle(Input::Char(b'A'));
        session.handle(Input::PrintScreen);
        assert!(session.lpt().contains('A'));
    }

    #[test]
    fn print_hello_and_the_question_mark_alias() {
        let mut session = fresh();
        session.type_ascii("PRINT \"Hello\"\n");
        let snap = session.snapshot();
        let lines: Vec<&str> = snap.lines().collect();
        assert!(lines.contains(&"Hello"));
        assert!(lines.contains(&"Ok"));

        let mut alias = fresh();
        alias.type_ascii("? \"Hi\"\n");
        assert!(alias.snapshot().lines().any(|line| line == "Hi"));
        assert!(alias.snapshot().lines().any(|line| line == "Ok"));
    }

    #[test]
    fn print_numbers_use_mbf_format() {
        let mut session = fresh();
        session.type_ascii("PRINT 1\n");
        assert!(session.snapshot().lines().any(|line| line == " 1"));

        let mut half = fresh();
        half.type_ascii("PRINT 1/2\n");
        assert!(half.snapshot().lines().any(|line| line == " .5"));

        let mut glued = fresh();
        glued.type_ascii("PRINT \"Hello\";\n");
        assert!(glued.snapshot().lines().any(|line| line == "HelloOk"));
    }

    #[test]
    fn run_executes_a_stored_print_and_a_bad_line_is_not_stored() {
        let mut session = fresh();
        session.type_ascii("10 PRINT \"Hello\"\n20 END\nRUN\n");
        assert_eq!(session.program_line(10), Some("10 PRINT \"Hello\""));
        assert!(session.snapshot().lines().any(|line| line == "Hello"));
        assert!(session.snapshot().lines().any(|line| line == "Ok"));

        let mut bad = fresh();
        bad.type_ascii("10 PRIN X\n");
        assert!(bad.program_line(10).is_none());
        assert!(bad.snapshot().contains('^'));
        assert!(bad.snapshot().contains(SYNTAX));
    }

    #[test]
    fn for_next_and_if_run() {
        let mut session = fresh();
        session.type_ascii(
            "10 A=1\n20 FOR I=1 TO 3\n30 A=A+I\n40 NEXT I\n50 IF A=7 THEN PRINT \"YES\" ELSE PRINT \"NO\"\n60 END\nRUN\n",
        );
        assert!(session.snapshot().lines().any(|line| line == "YES"));
        assert!(!session.snapshot().lines().any(|line| line == "NO"));
    }

    #[test]
    fn save_load_files_peek_and_on_error() {
        let dir = std::env::temp_dir().join("gwbasic-milestone3");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("HELLO.BAS");
        let data = dir.join("note.txt");
        let program_lit = program.display().to_string();
        let data_lit = data.display().to_string();

        let mut session = fresh();
        session.type_ascii("10 PRINT \"Hello\"\n");
        session.type_ascii(&format!("SAVE \"{program_lit}\"\n"));
        session.type_ascii("NEW\n");
        assert!(session.program_line(10).is_none());
        session.type_ascii(&format!("LOAD \"{program_lit}\"\n"));
        assert_eq!(session.program_line(10), Some("10 PRINT \"Hello\""));

        session.type_ascii(&format!("SAVE \"{program_lit}\",P\n"));
        session.type_ascii("NEW\n");
        session.type_ascii(&format!("LOAD \"{program_lit}\"\n"));
        assert!(session.program_line(10).unwrap().contains("Hello"));
        session.type_ascii("LIST\n");
        assert!(session.snapshot().contains(ILLEGAL));
        session.type_ascii("RUN\n");
        assert!(session.snapshot().lines().any(|line| line == "Hello"));

        let mut files = fresh();
        files.type_ascii(&format!("OPEN \"{data_lit}\" FOR OUTPUT AS #1\n"));
        files.type_ascii("PRINT #1, \"Hello\"\n");
        files.type_ascii("CLOSE #1\n");
        files.type_ascii(&format!("OPEN \"{data_lit}\" FOR INPUT AS #1\n"));
        files.type_ascii("LINE INPUT #1, A$\n");
        files.type_ascii("PRINT A$\n");
        let snap = files.snapshot();
        assert!(
            snap.lines().any(|line| line == "Hello"),
            "file snapshot:\n{snap}"
        );

        let mut memory = fresh();
        memory.type_ascii("POKE 100,65\nPRINT PEEK(100)\n");
        assert!(memory.snapshot().lines().any(|line| line == " 65"));

        let mut trapped = fresh();
        trapped.type_ascii(
            "10 ON ERROR GOTO 50\n20 ERROR 5\n30 PRINT \"NO\"\n40 END\n50 PRINT \"TRAP\"\n60 RESUME 40\nRUN\n",
        );
        assert!(trapped.snapshot().lines().any(|line| line == "TRAP"));
        assert!(!trapped.snapshot().lines().any(|line| line == "NO"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrapped_open_keeps_the_space_before_as() {
        let path = std::env::temp_dir()
            .join("gwbasic-milestone3")
            .join("note.txt");
        let line = format!("OPEN \"{}\" FOR OUTPUT AS #1", path.display());
        let mut session = fresh();
        for ch in line.chars() {
            if let Some(byte) = crate::input::char_to_byte(ch) {
                session.handle(Input::Char(byte));
            }
        }
        assert_eq!(session.screen().logical_text(), line);
        assert!(crate::basic::prepare(&line).is_ok());
    }

    #[test]
    fn a_double_suffix_is_not_swallowed_by_the_file_mark() {
        let mut session = fresh();
        session.type_ascii("DEFINT A\nA#=1/2\nPRINT A#\n");
        assert!(session.snapshot().lines().any(|line| line == " .5"));
    }

    #[test]
    fn goto_gosub_while_and_read_run() {
        let mut jumped = fresh();
        jumped.type_ascii("10 GOTO 30\n20 PRINT \"NO\"\n30 PRINT \"YES\"\n40 END\nRUN\n");
        assert!(jumped.snapshot().lines().any(|line| line == "YES"));
        assert!(!jumped.snapshot().lines().any(|line| line == "NO"));

        let mut sub = fresh();
        sub.type_ascii("10 GOSUB 40\n20 PRINT \"BACK\"\n30 END\n40 PRINT \"IN\"\n50 RETURN\nRUN\n");
        let snap = sub.snapshot();
        let inn = snap.lines().position(|line| line == "IN");
        let back = snap.lines().position(|line| line == "BACK");
        assert!(inn.is_some() && back.is_some() && inn < back);

        let mut looping = fresh();
        looping.type_ascii("10 N=0\n20 WHILE N<3\n30 N=N+1\n40 WEND\n50 PRINT N\n60 END\nRUN\n");
        assert!(looping.snapshot().lines().any(|line| line == " 3"));

        let mut data = fresh();
        data.type_ascii("10 READ A$\n20 PRINT A$\n30 DATA Hello\n40 END\nRUN\n");
        assert!(data.snapshot().lines().any(|line| line == "Hello"));
    }

    #[test]
    fn renum_rewrites_refs_and_refuses_to_reorder() {
        let mut session = fresh();
        session.type_ascii(
            "10 GOTO 40\n20 ON X GOTO 40,50\n30 IF A=0 THEN 40 ELSE 50\n40 RESUME 50\n50 END\n",
        );
        session.type_ascii("RENUM 100,,10\n");
        assert_eq!(session.program_line(100), Some("100 GOTO 130"));
        assert_eq!(session.program_line(110), Some("110 ON X GOTO 130,140"));
        assert_eq!(
            session.program_line(120),
            Some("120 IF A=0 THEN 130 ELSE 140")
        );
        assert_eq!(session.program_line(130), Some("130 RESUME 140"));
        assert_eq!(session.program_line(140), Some("140 END"));
        assert!(session.program_line(10).is_none());

        let mut undef = fresh();
        undef.type_ascii("10 GOTO 99\n20 END\nRENUM 100\n");
        let snap = undef.snapshot();
        assert!(snap.contains("Undefined line 99 in 100"), "{snap}");
        assert_eq!(undef.program_line(100), Some("100 GOTO 99"));
        assert_eq!(undef.program_line(110), Some("110 END"));

        let mut bad = fresh();
        bad.type_ascii("10 REM\n20 REM\n30 REM\nRENUM 15,30\n");
        assert!(bad.snapshot().contains(ILLEGAL));
        assert_eq!(bad.program_line(10), Some("10 REM"));
        assert_eq!(bad.program_line(20), Some("20 REM"));
        assert_eq!(bad.program_line(30), Some("30 REM"));

        let mut missing_old = fresh();
        missing_old.type_ascii("10 REM\nRENUM ,50\n");
        assert!(missing_old.snapshot().contains("Undefined line number"));
        assert_eq!(missing_old.program_line(10), Some("10 REM"));

        let mut inside = fresh();
        inside.type_ascii("10 RENUM\n20 END\nRUN\n");
        assert!(inside.snapshot().contains(ILLEGAL));
        assert_eq!(inside.program_line(10), Some("10 RENUM"));
        assert_eq!(inside.program_line(20), Some("20 END"));

        let mut moved = fresh();
        moved.type_ascii("0 REM\n10 GOTO 0\n20 ON ERROR GOTO 0\nRENUM 100\n");
        let snap = moved.snapshot();
        assert!(!snap.contains("Undefined line 0"), "{snap}");
        assert_eq!(moved.program_line(100), Some("100 REM"));
        assert_eq!(moved.program_line(110), Some("110 GOTO 100"));
        assert_eq!(moved.program_line(120), Some("120 ON ERROR GOTO 0"));

        let dir = std::env::temp_dir().join("gwbasic-milestone4-renum");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("P.BAS");
        let mut protected = fresh();
        protected.type_ascii("20 REM\n");
        protected.type_ascii(&format!("SAVE \"{}\",P\n", path.display()));
        protected.type_ascii("NEW\n");
        protected.type_ascii(&format!("LOAD \"{}\"\n", path.display()));
        protected.type_ascii("RENUM 100\n");
        assert!(protected.snapshot().contains(ILLEGAL));
        assert_eq!(protected.program_line(20), Some("20 REM"));
        assert!(protected.program_line(100).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resume_line_continues_at_that_line() {
        let mut session = fresh();
        session.type_ascii(
            "10 ON ERROR GOTO 50\n20 ERROR 5\n30 PRINT \"AFTER\"\n40 END\n50 PRINT \"TRAP\"\n60 RESUME 30\nRUN\n",
        );
        let snap = session.snapshot();
        assert!(snap.lines().any(|line| line == "TRAP"), "{snap}");
        assert!(snap.lines().any(|line| line == "AFTER"), "{snap}");
    }

    #[test]
    fn lset_and_rset_keep_the_current_length() {
        let mut session = fresh();
        session.type_ascii(
            "A$=\"ABCD\"\nLSET A$=\"XA\"\nPRINT LEN(A$);ASC(A$);ASC(MID$(A$,2));ASC(MID$(A$,4))\n",
        );
        assert!(session
            .snapshot()
            .lines()
            .any(|line| line == " 4  88  65  32"));

        let mut right = fresh();
        right.type_ascii("A$=\"ABCD\"\nRSET A$=\"XA\"\nPRINT ASC(A$);ASC(RIGHT$(A$,1))\n");
        assert!(right.snapshot().lines().any(|line| line == " 32  65"));

        let mut empty = fresh();
        empty.type_ascii("LSET B$=\"HI\"\nPRINT LEN(B$)\n");
        assert!(empty.snapshot().lines().any(|line| line == " 0"));

        let mut bad = fresh();
        bad.type_ascii("A%=1\nLSET A%=\"HI\"\n");
        assert!(bad.snapshot().contains("Type mismatch"));
    }

    #[test]
    fn conversions_environ_and_reset() {
        let mut session = fresh();
        session.type_ascii("A$=MKI$(65)\nPRINT LEN(A$);CVI(A$)\n");
        assert!(session.snapshot().lines().any(|line| line == " 2  65"));
        session.type_ascii("PRINT CVI(MKI$(-1))\n");
        assert!(session.snapshot().lines().any(|line| line == "-1"));
        session.type_ascii("PRINT CVI(MKI$(65)+\"Z\")\n");
        assert!(session.snapshot().lines().any(|line| line == " 65"));

        let mut wide = fresh();
        wide.type_ascii("A$=MKS$(1)\nB$=MKD$(1)\nPRINT LEN(A$);CVS(A$)\nPRINT LEN(B$);CVD(B$)\n");
        let snap = wide.snapshot();
        assert!(snap.lines().any(|line| line == " 4  1"), "{snap}");
        assert!(snap.lines().any(|line| line == " 8  1"), "{snap}");

        let mut short = fresh();
        short.type_ascii("PRINT CVI(\"A\")\n");
        assert!(short.snapshot().contains(ILLEGAL));

        let mut env = fresh();
        env.type_ascii(
            "ENVIRON \"GWTEST4=hi\"\nPRINT ENVIRON$(\"gwtest4\")\nENVIRON \"GWTEST4=\"\nPRINT LEN(ENVIRON$(\"GWTEST4\"))\n",
        );
        let snap = env.snapshot();
        assert!(snap.lines().any(|line| line == "hi"), "{snap}");
        assert!(snap.lines().any(|line| line == " 0"), "{snap}");

        let mut bad = fresh();
        bad.type_ascii("ENVIRON \"NOPE\"\n");
        assert!(bad.snapshot().contains(ILLEGAL));

        let shell = if cfg!(windows) {
            "SHELL \"echo %GWTEST4%\"\n"
        } else {
            "SHELL \"echo $GWTEST4\"\n"
        };
        let mut child = fresh();
        child.type_ascii("ENVIRON \"GWTEST4=hi\"\n");
        child.type_ascii(shell);
        assert!(
            child.snapshot().lines().any(|line| line == "hi"),
            "{}",
            child.snapshot()
        );

        let dir = std::env::temp_dir().join("gwbasic-milestone4");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        let mut reset = fresh();
        reset.type_ascii(&format!("OPEN \"{}\" FOR OUTPUT AS #1\n", path.display()));
        reset.type_ascii("RESET\nPRINT EOF(1)\n");
        assert!(reset.snapshot().contains("Bad file mode"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn common_survives_chain_and_input_dollar_reads() {
        let dir = std::env::temp_dir().join("gwbasic-milestone5");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let chained = dir.join("NEXT.BAS");
        std::fs::write(&chained, "10 PRINT A;B\n20 PRINT A(1)\n").unwrap();
        let chained_lit = chained.display().to_string();

        let mut session = fresh();
        session.type_ascii(&format!(
            "10 COMMON A,A()\n20 A=4\n30 B=9\n40 DIM A(2)\n50 A(1)=7\n60 CHAIN \"{chained_lit}\"\nRUN\n"
        ));
        let snap = session.snapshot();
        assert!(snap.lines().any(|line| line == " 4  0"), "{snap}");
        assert!(snap.lines().any(|line| line == " 7"), "{snap}");

        let cleared = dir.join("ZERO.BAS");
        std::fs::write(&cleared, "10 PRINT A\n").unwrap();
        let mut lost = fresh();
        lost.type_ascii(&format!(
            "10 A=4\n20 CHAIN \"{}\"\nRUN\n",
            cleared.display()
        ));
        assert!(
            lost.snapshot().lines().any(|line| line == " 0"),
            "{}",
            lost.snapshot()
        );

        let note = dir.join("note.txt");
        let mut typed = fresh();
        typed.type_ascii(&format!(
            "OPEN \"{}\" FOR OUTPUT AS #1\nPRINT #1, \"Ab\";\nCLOSE #1\n",
            note.display()
        ));
        typed.type_ascii(&format!(
            "OPEN \"{}\" FOR INPUT AS #1\nPRINT INPUT$(2,#1)\n",
            note.display()
        ));
        assert!(typed.snapshot().lines().any(|line| line == "Ab"));
        typed.type_ascii("PRINT INPUT$(1,#1)\n");
        assert!(typed.snapshot().contains("Input past end"));

        let mut keys = fresh();
        keys.type_ascii("10 PRINT INPUT$(1);INPUT$(1)\nRUN\nAB");
        let snap = keys.snapshot();
        assert_eq!(
            snap.lines().filter(|line| *line == "AB").count(),
            1,
            "{snap}"
        );

        let mut bad = fresh();
        bad.type_ascii("PRINT INPUT$(0)\n");
        assert!(bad.snapshot().contains(ILLEGAL));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn chain_merge_starts_at_a_line_and_can_keep_everything() {
        let dir = std::env::temp_dir().join("gwbasic-milestone6");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let later = dir.join("LATER.BAS");
        std::fs::write(&later, "10 PRINT \"NO\"\n100 PRINT \"YES\"\n").unwrap();
        let mut start = fresh();
        start.type_ascii(&format!("10 CHAIN \"{}\", 100\nRUN\n", later.display()));
        let snap = start.snapshot();
        assert!(snap.lines().any(|line| line == "YES"), "{snap}");
        assert!(!snap.lines().any(|line| line == "NO"), "{snap}");

        let only = dir.join("ONLY.BAS");
        std::fs::write(&only, "10 PRINT \"LOADED\"\n").unwrap();
        let mut missing = fresh();
        missing.type_ascii(&format!("10 CHAIN \"{}\", 50\nRUN\nLIST\n", only.display()));
        let snap = missing.snapshot();
        assert!(snap.contains(ILLEGAL), "{snap}");
        assert!(
            snap.lines().any(|line| line == "10 PRINT \"LOADED\""),
            "{snap}"
        );

        let overlay = dir.join("ADD.BAS");
        std::fs::write(&overlay, "15 PRINT \"ADD\"\n").unwrap();
        let mut merged = fresh();
        merged.type_ascii(&format!(
            "10 PRINT \"KEEP\"\n20 CHAIN MERGE \"{}\", 15, DELETE 20-40\n40 PRINT \"DROP\"\nRUN\nLIST\n",
            overlay.display()
        ));
        let snap = merged.snapshot();
        assert!(snap.lines().any(|line| line == "KEEP"), "{snap}");
        assert!(snap.lines().any(|line| line == "ADD"), "{snap}");
        assert!(!snap.lines().any(|line| line == "DROP"), "{snap}");
        assert_eq!(
            snap.lines()
                .filter(|line| *line == "15 PRINT \"ADD\"")
                .count(),
            1,
            "{snap}"
        );
        assert_eq!(
            snap.lines()
                .filter(|line| *line == "40 PRINT \"DROP\"")
                .count(),
            1,
            "{snap}"
        );
        assert!(snap.contains("Ok"), "{snap}");

        let replaced = dir.join("NEW.BAS");
        std::fs::write(&replaced, "10 PRINT \"NEW\"\n").unwrap();
        let mut untouched = fresh();
        untouched.type_ascii(&format!(
            "10 PRINT \"OLD\"\n20 CHAIN MERGE \"{}\",,DELETE 50-90\nRUN\nLIST\n",
            replaced.display()
        ));
        let snap = untouched.snapshot();
        assert!(snap.contains(ILLEGAL), "{snap}");
        assert!(snap.lines().any(|line| line == "OLD"), "{snap}");
        assert!(!snap.lines().any(|line| line == "NEW"), "{snap}");
        assert!(
            snap.lines().any(|line| line.contains("CHAIN MERGE")),
            "{snap}"
        );

        let tokenized = dir.join("TOK.BAS");
        std::fs::write(&tokenized, [0xFF]).unwrap();
        let mut binary = fresh();
        binary.type_ascii(&format!(
            "10 PRINT \"SAFE\"\n20 CHAIN MERGE \"{}\"\nRUN\nLIST\n",
            tokenized.display()
        ));
        let snap = binary.snapshot();
        assert!(snap.contains("Bad file mode"), "{snap}");
        assert!(snap.lines().any(|line| line == "SAFE"), "{snap}");
        assert!(
            snap.lines().any(|line| line.contains("CHAIN MERGE")),
            "{snap}"
        );

        let passed = dir.join("ALL.BAS");
        std::fs::write(&passed, "10 PRINT B\n").unwrap();
        let mut keep = fresh();
        keep.type_ascii(&format!(
            "10 B=9\n20 CHAIN \"{}\",,ALL\nRUN\n",
            passed.display()
        ));
        assert!(
            keep.snapshot().lines().any(|line| line == " 9"),
            "{}",
            keep.snapshot()
        );

        let based = dir.join("BASE.BAS");
        std::fs::write(&based, "10 DIM A(1)\n20 PRINT A(0)\n").unwrap();
        let mut base = fresh();
        base.type_ascii(&format!(
            "10 OPTION BASE 1\n20 CHAIN \"{}\"\nRUN\n",
            based.display()
        ));
        assert!(
            base.snapshot().contains("Subscript out of range"),
            "{}",
            base.snapshot()
        );

        let typed = dir.join("INT.BAS");
        std::fs::write(&typed, "10 A=3/2\n20 PRINT A\n").unwrap();
        let mut ints = fresh();
        ints.type_ascii(&format!(
            "10 DEFINT A\n20 CHAIN MERGE \"{}\",,ALL\nRUN\n",
            typed.display()
        ));
        assert!(
            ints.snapshot().lines().any(|line| line == " 2"),
            "{}",
            ints.snapshot()
        );
        let mut singles = fresh();
        singles.type_ascii(&format!(
            "10 DEFINT A\n20 CHAIN \"{}\",,ALL\nRUN\n",
            typed.display()
        ));
        assert!(
            singles.snapshot().lines().any(|line| line == " 1.5"),
            "{}",
            singles.snapshot()
        );

        let func = dir.join("FN.BAS");
        std::fs::write(&func, "10 PRINT FNA(2)\n").unwrap();
        let mut kept_fn = fresh();
        kept_fn.type_ascii(&format!(
            "10 DEF FNA(X)=X+1\n20 CHAIN \"{}\",,ALL\nRUN\n",
            func.display()
        ));
        assert!(
            kept_fn.snapshot().lines().any(|line| line == " 3"),
            "{}",
            kept_fn.snapshot()
        );
        let mut lost_fn = fresh();
        lost_fn.type_ascii(&format!(
            "10 DEF FNA(X)=X+1\n20 CHAIN \"{}\"\nRUN\n",
            func.display()
        ));
        assert!(
            lost_fn.snapshot().contains("Undefined user function"),
            "{}",
            lost_fn.snapshot()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn has_line(session: &Session, line: &str) {
        assert!(
            session.snapshot().lines().any(|row| row == line),
            "missing {line:?} in:\n{}",
            session.snapshot()
        );
    }

    #[test]
    fn screen_1_plots_and_maps_points() {
        let mut center = fresh();
        center.type_ascii("SCREEN 1\nPRINT POINT(0);POINT(1);POINT(2);POINT(3)\n");
        has_line(&center, " 160  100  160  100");

        let mut plotted = fresh();
        plotted.type_ascii(
            "SCREEN 1\n\
             PSET(0,0)\n\
             PRINT POINT(0,0)\n\
             PRESET(0,0)\n\
             PRINT POINT(0,0)\n\
             PSET(0,0),1\n\
             PRINT POINT(0,0);POINT(-1,0);POINT(0,200)\n",
        );
        has_line(&plotted, " 3");
        has_line(&plotted, " 0");
        has_line(&plotted, " 1 -1 -1");

        let mut stepped = fresh();
        stepped.type_ascii(
            "SCREEN 1\n\
             PSET(4,5)\n\
             PSET STEP(1,2)\n\
             PRINT POINT(0);POINT(1)\n\
             PRINT PMAP(3,0);PMAP(4,1)\n",
        );
        has_line(&stepped, " 5  7");
        has_line(&stepped, " 3  4");

        let mut up = fresh();
        up.type_ascii("SCREEN 1\nWINDOW (0,0)-(319,199)\nPRINT PMAP(0,1);PMAP(199,1)\n");
        has_line(&up, " 199  0");

        let mut down = fresh();
        down.type_ascii("SCREEN 1\nWINDOW SCREEN (0,0)-(319,199)\nPRINT PMAP(0,1);PMAP(199,1)\n");
        has_line(&down, " 0  199");

        let mut view = fresh();
        view.type_ascii(
            "SCREEN 1\n\
             VIEW (10,10)-(100,100)\n\
             PSET(0,0),1\n\
             PRINT POINT(0,0);POINT(90,0);POINT(0,-1)\n",
        );
        has_line(&view, " 1  0 -1");

        let mut reset = fresh();
        reset.type_ascii("SCREEN 1\nPSET(1,1)\nVIEW\nPRINT POINT(0);POINT(1)\n");
        has_line(&reset, " 160  100");

        let mut wide = fresh();
        wide.type_ascii(
            "SCREEN 2\n\
             PSET(400,10)\n\
             PRINT POINT(400,10);POINT(639,0);POINT(640,0)\n\
             PRINT POINT(0)\n",
        );
        has_line(&wide, " 1  0 -1");
        has_line(&wide, " 400");
    }

    #[test]
    fn screen_0_keeps_the_pen_and_text_reads_cells() {
        let mut session = fresh();
        session.type_ascii("PRINT \"HI\"\nSCREEN 0,0,0\n");
        assert_eq!(session.snapshot(), "Ok");

        let mut kept = fresh();
        kept.type_ascii(
            "SCREEN 2\n\
             PSET(400,10)\n\
             SCREEN 0\n\
             PRINT POINT(0);PMAP(9,0)\n\
             PRINT POINT(1,1)\n",
        );
        has_line(&kept, " 400  0");
        assert!(kept.snapshot().contains(ILLEGAL), "{}", kept.snapshot());

        let mut narrow = fresh();
        narrow.type_ascii("SCREEN 1\nSCREEN 0\nPRINT SCREEN(1,41)\n");
        assert!(narrow.snapshot().contains(ILLEGAL), "{}", narrow.snapshot());
        assert!(
            !narrow.snapshot().lines().any(|line| line == " 32"),
            "{}",
            narrow.snapshot()
        );

        let mut cells = fresh();
        cells.type_ascii(
            "10 CLS\n20 PRINT \"AB\"\n30 PRINT SCREEN(1,1);SCREEN(1,2);SCREEN(1,1,1)\nRUN\n",
        );
        has_line(&cells, " 65  66  7");

        let mut key_row = fresh();
        key_row.type_ascii("PRINT SCREEN(25,1)\n");
        assert!(
            key_row.snapshot().contains(ILLEGAL),
            "{}",
            key_row.snapshot()
        );
        key_row.type_ascii("KEY OFF\nPRINT SCREEN(25,1)\n");
        has_line(&key_row, " 32");

        let mut bad_cell = fresh();
        bad_cell.type_ascii("PRINT SCREEN(0,1)\n");
        assert!(
            bad_cell.snapshot().contains(ILLEGAL),
            "{}",
            bad_cell.snapshot()
        );
    }

    #[test]
    fn bad_graphics_modes_do_not_clear_and_syntax_is_not_stored() {
        let mut session = fresh();
        session.type_ascii("PRINT \"HI\"\nSCREEN 3\n");
        assert!(
            session.snapshot().lines().any(|line| line == "HI"),
            "{}",
            session.snapshot()
        );
        assert!(
            session.snapshot().contains(ILLEGAL),
            "{}",
            session.snapshot()
        );

        let mut huge = fresh();
        huge.type_ascii("PRINT \"HI\"\nSCREEN 99\n");
        assert!(huge.snapshot().lines().any(|line| line == "HI"));
        assert!(huge.snapshot().contains(ILLEGAL));

        let mut program = fresh();
        program.type_ascii("10 PRINT \"HI\"\n20 SCREEN 3\nRUN\n");
        assert!(program.snapshot().lines().any(|line| line == "HI"));
        assert!(program.snapshot().contains("Illegal function call in 20"));

        let mut page = fresh();
        page.type_ascii("SCREEN 1\nPSET(0,0),1\nSCREEN 1,0,1\nPRINT POINT(0,0)\n");
        has_line(&page, " 1");
        assert!(page.snapshot().contains(ILLEGAL), "{}", page.snapshot());

        let mut drawn = fresh();
        drawn.type_ascii("10 SCREEN 1\n20 PSET (0,0),2\n30 PRINT POINT(0,0)\nRUN\n");
        assert_eq!(drawn.snapshot(), " 2\nOk");

        let mut palette = fresh();
        palette.type_ascii("10 PALETTE USING A\n");
        assert!(palette.program_line(10).is_none());
        assert!(palette.snapshot().contains(SYNTAX));

        let mut view = fresh();
        view.type_ascii("10 VIEW PRINT 1 TO 24\n");
        assert!(view.program_line(10).is_none());
        assert!(view.snapshot().contains(SYNTAX));

        let mut stored = fresh();
        stored.type_ascii("10 PSET(0,0)\nRUN\n");
        assert!(stored.program_line(10).is_some());
        assert!(stored.snapshot().contains("Illegal function call in 10"));

        let mut overflow = fresh();
        overflow.type_ascii("PRINT POINT(32768,0)\nPRINT PMAP(1,4)\nPRINT PMAP(1,\"A\")\n");
        assert!(
            overflow.snapshot().contains("Overflow"),
            "{}",
            overflow.snapshot()
        );
        assert!(
            overflow.snapshot().contains(ILLEGAL),
            "{}",
            overflow.snapshot()
        );
        assert!(
            overflow.snapshot().contains("Type mismatch"),
            "{}",
            overflow.snapshot()
        );
    }

    #[test]
    fn width_switches_paired_graphics_modes() {
        let mut session = fresh();
        session.type_ascii("SCREEN 1\nWIDTH 80\nPRINT POINT(400,0)\n");
        has_line(&session, " 0");
        assert!(!session.snapshot().lines().any(|line| line == " -1"));

        let mut program = fresh();
        program.type_ascii("10 SCREEN 1\n20 WIDTH 80\n30 PRINT POINT(400,0)\nRUN\n");
        assert_eq!(program.snapshot(), " 0\nOk");

        let mut stuck = fresh();
        stuck.type_ascii("SCREEN 11\nWIDTH 40\nPRINT POINT(400,0)\n");
        assert!(stuck.snapshot().contains(ILLEGAL), "{}", stuck.snapshot());
        has_line(&stuck, " 0");
        assert!(!stuck.snapshot().lines().any(|line| line == " -1"));

        let mut down = fresh();
        down.type_ascii("SCREEN 9\nWIDTH 40\nPRINT POINT(0)\nPSET(0,0)\nPRINT POINT(0,0)\n");
        has_line(&down, " 160");
        has_line(&down, " 15");
    }

    #[test]
    fn color_and_palette_check_their_ranges() {
        let mut cga = fresh();
        cga.type_ascii("SCREEN 1\nCOLOR 256\n");
        assert!(cga.snapshot().contains(ILLEGAL), "{}", cga.snapshot());

        let mut mono = fresh();
        mono.type_ascii("SCREEN 2\nCOLOR 1\n");
        assert!(mono.snapshot().contains(ILLEGAL), "{}", mono.snapshot());

        let mut text = fresh();
        text.type_ascii("PALETTE 1,1\n");
        assert!(text.snapshot().contains(ILLEGAL), "{}", text.snapshot());

        let mut range = fresh();
        range.type_ascii("SCREEN 1\nPALETTE 4,1\nPALETTE 1,-2\n");
        assert!(range.snapshot().contains(ILLEGAL), "{}", range.snapshot());

        let mut kept = fresh();
        kept.type_ascii(
            "SCREEN 1\n\
             PALETTE 1,-1\n\
             PALETTE\n\
             COLOR ,0\n\
             PSET(0,0)\n\
             PRINT POINT(0,0)\n\
             PSET(0,0),9\n\
             PRINT POINT(0,0)\n\
             PRESET(0,0),2\n\
             PRINT POINT(0,0)\n",
        );
        assert!(!kept.snapshot().contains(ILLEGAL), "{}", kept.snapshot());
        has_line(&kept, " 3");
        has_line(&kept, " 2");

        let mut clamped = fresh();
        clamped.type_ascii("SCREEN 1\nPSET(0,0),256\n");
        assert!(
            clamped.snapshot().contains(ILLEGAL),
            "{}",
            clamped.snapshot()
        );
    }

    #[test]
    fn line_draws_segments_boxes_and_styles() {
        let mut segment = fresh();
        segment.type_ascii(
            "10 SCREEN 1\n\
             20 LINE (0,0)-(10,0),1\n\
             30 PRINT POINT(0,0);POINT(10,0);POINT(5,0);POINT(0,1)\n\
             40 PRINT POINT(0)\n\
             RUN\n",
        );
        assert_eq!(segment.snapshot(), " 1  1  1  0\n 10\nOk");

        let mut continued = fresh();
        continued.type_ascii(
            "10 SCREEN 1\n\
             20 PSET(0,0),1\n\
             30 LINE -(5,0)\n\
             40 PRINT POINT(5,0);POINT(0,0);POINT(0)\n\
             RUN\n",
        );
        assert_eq!(continued.snapshot(), " 3  3  5\nOk");

        let mut stepped = fresh();
        stepped.type_ascii(
            "10 SCREEN 1\n\
             20 PSET(0,0),1\n\
             30 LINE -STEP(5,0)\n\
             40 PRINT POINT(0);POINT(1)\n\
             RUN\n",
        );
        assert_eq!(stepped.snapshot(), " 5  0\nOk");

        let mut outline = fresh();
        outline.type_ascii(
            "10 SCREEN 1\n\
             20 LINE (0,0)-(4,2),1,B\n\
             30 PRINT POINT(2,1);POINT(0,0)\n\
             RUN\n",
        );
        assert_eq!(outline.snapshot(), " 0  1\nOk");

        let mut filled = fresh();
        filled.type_ascii(
            "10 SCREEN 1\n\
             20 LINE (0,0)-(4,2),1,BF\n\
             30 PRINT POINT(2,1);POINT(5,1)\n\
             RUN\n",
        );
        assert_eq!(filled.snapshot(), " 1  0\nOk");

        let mut high = fresh();
        high.type_ascii(
            "10 SCREEN 1\n\
             20 LINE (0,0)-(3,0),1,,&H8000\n\
             30 PRINT POINT(0,0);POINT(1,0)\n\
             RUN\n",
        );
        assert_eq!(high.snapshot(), " 1  0\nOk");

        let mut mask = fresh();
        mask.type_ascii(
            "10 SCREEN 1\n\
             20 LINE (0,0)-(3,0),1,,&HA000\n\
             30 PRINT POINT(0,0);POINT(1,0);POINT(2,0);POINT(3,0)\n\
             RUN\n",
        );
        assert_eq!(mask.snapshot(), " 1  0  1  0\nOk");

        let mut styled = fresh();
        styled.type_ascii("10 LINE (0,0)-(2,2),,BF,&HFFFF\n");
        assert!(styled.program_line(10).is_none());
        assert!(styled.snapshot().contains(SYNTAX), "{}", styled.snapshot());

        let mut text = fresh();
        text.type_ascii("10 LINE (0,0)-(1,1)\nRUN\n");
        assert!(text.program_line(10).is_some());
        assert!(text.snapshot().contains("Illegal function call in 10"));

        let mut trailing = fresh();
        trailing.type_ascii("10 LINE (0,0)-(1,1),\n");
        assert!(trailing.program_line(10).is_none());
        assert!(
            trailing.snapshot().contains("Missing operand"),
            "{}",
            trailing.snapshot()
        );
    }

    #[test]
    fn circle_draws_arcs_and_spokes() {
        let mut full = fresh();
        full.type_ascii(
            "10 SCREEN 1\n\
             20 CIRCLE (160,100),10,1,,,1\n\
             30 PRINT POINT(170,100);POINT(150,100);POINT(160,90);POINT(160,110);POINT(160,100)\n\
             40 PRINT POINT(0);POINT(1)\n\
             RUN\n",
        );
        assert_eq!(full.snapshot(), " 1  1  1  1  0\n 160  100\nOk");

        let mut arc = fresh();
        arc.type_ascii(
            "10 SCREEN 1\n\
             20 CIRCLE (160,100),10,1,0,1.5708,1\n\
             30 PRINT POINT(170,100);POINT(160,90);POINT(150,100);POINT(160,110)\n\
             RUN\n",
        );
        assert_eq!(arc.snapshot(), " 1  1  0  0\nOk");

        let mut spoke = fresh();
        spoke.type_ascii(
            "10 SCREEN 1\n\
             20 CIRCLE (160,100),10,1,0,-1.5708,1\n\
             30 PRINT POINT(160,100);POINT(170,100);POINT(160,90);POINT(150,100)\n\
             RUN\n",
        );
        assert_eq!(spoke.snapshot(), " 1  1  1  0\nOk");

        let mut negative = fresh();
        negative.type_ascii("10 SCREEN 1\n20 CIRCLE (10,10),-1\nRUN\n");
        assert!(negative.snapshot().contains("Illegal function call in 20"));

        let mut text = fresh();
        text.type_ascii("10 CIRCLE (0,0),10\nRUN\n");
        assert!(text.program_line(10).is_some());
        assert!(text.snapshot().contains("Illegal function call in 10"));

        let mut trailing = fresh();
        trailing.type_ascii("10 CIRCLE (0,0),10,\n");
        assert!(trailing.program_line(10).is_none());
        assert!(
            trailing.snapshot().contains("Missing operand"),
            "{}",
            trailing.snapshot()
        );
    }

    #[test]
    fn paint_fills_regions_and_tiles() {
        let mut solid = fresh();
        solid.type_ascii(
            "10 SCREEN 1\n\
             20 LINE (10,10)-(20,20),1,B\n\
             30 PAINT (15,15),2,1\n\
             40 PRINT POINT(15,15);POINT(10,10);POINT(9,15)\n\
             RUN\n",
        );
        assert_eq!(solid.snapshot(), " 2  1  0\nOk");

        let mut border = fresh();
        border.type_ascii(
            "10 SCREEN 1\n\
             20 LINE (10,10)-(20,20),1,B\n\
             30 PAINT (10,10),2,1\n\
             40 PRINT POINT(15,15);POINT(10,10)\n\
             RUN\n",
        );
        assert_eq!(border.snapshot(), " 0  1\nOk");

        let mut outside = fresh();
        outside.type_ascii(
            "10 SCREEN 1\n\
             20 LINE (10,10)-(20,20),1,B\n\
             30 PAINT (-1,-1),2,1\n\
             40 PRINT POINT(15,15)\n\
             RUN\n",
        );
        assert_eq!(outside.snapshot(), " 0\nOk");

        let mut tile = fresh();
        tile.type_ascii(
            "10 SCREEN 2\n\
             20 LINE (0,0)-(8,8),1,B\n\
             30 PAINT (4,4),CHR$(255),1\n\
             40 PRINT POINT(4,4);POINT(0,0);POINT(9,4)\n\
             RUN\n",
        );
        assert_eq!(tile.snapshot(), " 1  1  0\nOk");

        let mut empty = fresh();
        empty.type_ascii("10 SCREEN 2\n20 PAINT (1,1),\"\"\nRUN\n");
        assert!(empty.snapshot().contains("Illegal function call in 20"));

        let mut long = fresh();
        long.type_ascii("10 SCREEN 2\n20 PAINT (1,1),STRING$(65,\"A\")\nRUN\n");
        assert!(long.snapshot().contains("Illegal function call in 20"));
    }

    #[test]
    fn draw_runs_the_macro_language() {
        let mut right = fresh();
        right.type_ascii(
            "10 SCREEN 12\n\
             20 DRAW \"BM0,0C1R10\"\n\
             30 PRINT POINT(10,0);POINT(0,0);POINT(0)\n\
             RUN\n",
        );
        assert_eq!(right.snapshot(), " 1  1  10\nOk");

        let mut blank = fresh();
        blank.type_ascii(
            "10 SCREEN 12\n\
             20 DRAW \"BM100,100BU10\"\n\
             30 PRINT POINT(100,90);POINT(0);POINT(1)\n\
             RUN\n",
        );
        assert_eq!(blank.snapshot(), " 0  100  90\nOk");

        let mut back = fresh();
        back.type_ascii(
            "10 SCREEN 12\n\
             20 DRAW \"BM100,100C1NU10\"\n\
             30 PRINT POINT(100,90);POINT(0);POINT(1)\n\
             RUN\n",
        );
        assert_eq!(back.snapshot(), " 1  100  100\nOk");

        let mut scaled = fresh();
        scaled.type_ascii(
            "10 SCREEN 12\n\
             20 DRAW \"BM0,0C1S8R4\"\n\
             30 PRINT POINT(8,0);POINT(0)\n\
             RUN\n",
        );
        assert_eq!(scaled.snapshot(), " 1  8\nOk");

        let mut turned = fresh();
        turned.type_ascii(
            "10 SCREEN 12\n\
             20 DRAW \"BM100,100C1A1R10\"\n\
             30 PRINT POINT(100,90);POINT(110,100);POINT(0);POINT(1)\n\
             RUN\n",
        );
        assert_eq!(turned.snapshot(), " 1  0  100  90\nOk");

        let mut angled = fresh();
        angled.type_ascii(
            "10 SCREEN 12\n\
             20 DRAW \"BM100,100C1TA90R10\"\n\
             30 PRINT POINT(100,90);POINT(0);POINT(1)\n\
             RUN\n",
        );
        assert_eq!(angled.snapshot(), " 1  100  90\nOk");

        let mut filled = fresh();
        filled.type_ascii(
            "10 SCREEN 12\n\
             20 LINE (10,10)-(20,20),1,B\n\
             30 DRAW \"BM15,15P2,1\"\n\
             40 PRINT POINT(15,15);POINT(10,10);POINT(9,15)\n\
             RUN\n",
        );
        assert_eq!(filled.snapshot(), " 2  1  0\nOk");

        let mut nested = fresh();
        nested.type_ascii(
            "10 SCREEN 12\n\
             20 A$=\"BM10,10C1R4\"\n\
             30 DRAW \"XA$;\"\n\
             40 PRINT POINT(14,10);POINT(10,10);POINT(0)\n\
             RUN\n",
        );
        assert_eq!(nested.snapshot(), " 1  1  14\nOk");

        let mut variable = fresh();
        variable.type_ascii(
            "10 SCREEN 12\n\
             20 A%=3\n\
             30 DRAW \"BM0,0C1R=A%;\"\n\
             40 PRINT POINT(3,0);POINT(0)\n\
             RUN\n",
        );
        assert_eq!(variable.snapshot(), " 1  3\nOk");

        let mut relative = fresh();
        relative.type_ascii(
            "10 SCREEN 12\n\
             20 DRAW \"BM0,0C1M+5,0\"\n\
             30 PRINT POINT(5,0);POINT(0)\n\
             RUN\n",
        );
        assert_eq!(relative.snapshot(), " 1  5\nOk");

        let mut bad = fresh();
        bad.type_ascii("10 SCREEN 12\n20 DRAW \"Z\"\nRUN\n");
        assert!(bad.snapshot().contains("Illegal function call in 20"));

        let mut partial = fresh();
        partial.type_ascii("SCREEN 12\nDRAW \"BM0,0C1R4Z\"\nPRINT POINT(4,0)\n");
        assert!(
            partial.snapshot().contains(ILLEGAL),
            "{}",
            partial.snapshot()
        );
        has_line(&partial, " 1");

        let mut text = fresh();
        text.type_ascii("10 DRAW \"R1\"\nRUN\n");
        assert!(text.program_line(10).is_some());
        assert!(text.snapshot().contains("Illegal function call in 10"));

        let mut missing = fresh();
        missing.type_ascii("10 DRAW\n");
        assert!(missing.program_line(10).is_none());
        assert!(
            missing.snapshot().contains("Missing operand"),
            "{}",
            missing.snapshot()
        );

        let mut deep = fresh();
        deep.type_ascii(
            "10 SCREEN 12\n\
             20 A$=\"XB$;\"\n\
             30 B$=\"XA$;\"\n\
             40 DRAW \"XA$;\"\n\
             RUN\n",
        );
        assert!(
            deep.snapshot().contains("Illegal function call in 40"),
            "{}",
            deep.snapshot()
        );
    }
}
