//! Tokenized statement runner. Values are integers or Microsoft Binary Format
//! singles and doubles. `f64` is used only inside the transcendental functions,
//! and the result is rounded back into an MBF value.

use crate::crunch::{self, Tok};
use crate::files::{self, Files, SaveKind, SeqMode};
use crate::mbf::{MathError, Mbf};
use crate::program::Program;
use crate::screen::Screen;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

pub const SYNTAX: &str = "Syntax error";
const MISSING: &str = "Missing operand";
const TYPE_MISMATCH: &str = "Type mismatch";
const OVERFLOW: &str = "Overflow";
const DIV0: &str = "Division by zero";
const ILLEGAL: &str = "Illegal function call";
const UNDEF_LINE: &str = "Undefined line number";
const SUBSCRIPT: &str = "Subscript out of range";
const OUT_OF_DATA: &str = "Out of data";
const RETURN_WITHOUT: &str = "RETURN without GOSUB";
const NEXT_WITHOUT: &str = "NEXT without FOR";
const WEND_WITHOUT: &str = "WEND without WHILE";
const WHILE_WITHOUT: &str = "WHILE without WEND";
pub(crate) const REDO: &str = "Redo from start";
const STRING_LONG: &str = "String too long";
const DUPLICATE: &str = "Duplicate Definition";
const OUT_OF_MEMORY: &str = "Out of memory";
const RESUME_WITHOUT: &str = "RESUME without error";
const UNPRINTABLE: &str = "Unprintable error";
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Ty {
    Int,
    Single,
    Double,
    Str,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Name {
    stem: String,
    ty: Ty,
}

#[derive(Clone, Debug)]
struct Ident {
    stem: String,
    suffix: Option<Ty>,
}

impl Ident {
    fn parse(text: &str) -> Self {
        let bytes = text.as_bytes();
        let suffix = match bytes.last().copied() {
            Some(b'%') => Some(Ty::Int),
            Some(b'!') => Some(Ty::Single),
            Some(b'#') => Some(Ty::Double),
            Some(b'$') => Some(Ty::Str),
            _ => None,
        };
        let stem = if suffix.is_some() {
            text[..text.len() - 1].to_string()
        } else {
            text.to_string()
        };
        Self { stem, suffix }
    }

    fn resolve(&self, defs: &[Ty; 26]) -> Name {
        let ty = self.suffix.unwrap_or_else(|| {
            let index = self.stem.as_bytes().first().copied().unwrap_or(b'A');
            defs[usize::from(index.saturating_sub(b'A')).min(25)]
        });
        Name {
            stem: self.stem.clone(),
            ty,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Number {
    Int(i16),
    Single(Mbf),
    Double(Mbf),
}

#[derive(Clone, Debug)]
enum Value {
    Int(i16),
    Single(Mbf),
    Double(Mbf),
    Str(String),
}

impl Value {
    fn zero(ty: Ty) -> Self {
        match ty {
            Ty::Int => Value::Int(0),
            Ty::Single => Value::Single(Mbf::zero(false)),
            Ty::Double => Value::Double(Mbf::zero(true)),
            Ty::Str => Value::Str(String::new()),
        }
    }

    fn ty(&self) -> Ty {
        match self {
            Value::Int(_) => Ty::Int,
            Value::Single(_) => Ty::Single,
            Value::Double(_) => Ty::Double,
            Value::Str(_) => Ty::Str,
        }
    }
}

struct RunErr {
    message: &'static str,
    code: i16,
}

fn fault(message: &'static str, code: i16) -> RunErr {
    RunErr { message, code }
}

#[derive(Clone, Debug)]
enum Expr {
    Int(i16),
    Single(Mbf),
    Double(Mbf),
    Str(String),
    Var(Ident),
    Index(Ident, Vec<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call(Func, Vec<Expr>),
    UserFn(Ident, Vec<Expr>),
    Naked(Naked),
}

#[derive(Clone, Copy, Debug)]
enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    IDiv,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    Xor,
    Eqv,
    Imp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Func {
    Left,
    Right,
    Mid,
    Chr,
    StrFn,
    StringFn,
    Space,
    Hex,
    Oct,
    Sgn,
    Int,
    Abs,
    Sqr,
    Rnd,
    Sin,
    Log,
    Exp,
    Cos,
    Tan,
    Atn,
    Fre,
    Len,
    Val,
    Asc,
    Instr,
    Cint,
    Csng,
    Cdbl,
    Fix,
    Pos,
    Peek,
    Eof,
    Lof,
    Loc,
    Mki,
    Mks,
    Mkd,
    Cvi,
    Cvs,
    Cvd,
    Environ,
    /// A tokenized function this milestone does not implement (`VARPTR`, `INP`, …).
    Unsupported,
}

#[derive(Clone, Copy, Debug)]
enum Naked {
    Inkey,
    Csrlin,
    Erl,
    Err,
    Date,
    Time,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sep {
    None,
    Semi,
    Comma,
}

enum PrintPart {
    Expr(Expr),
    Tab(Expr),
    Spc(Expr),
    Blank,
    Using(Expr, Vec<Expr>),
}

struct PrintItem {
    part: PrintPart,
    sep: Sep,
}

enum Target {
    Var(Ident),
    Index(Ident, Vec<Expr>),
}

#[derive(Clone, Debug)]
enum InSlot {
    Var(Name),
    Elem(Name, Vec<i16>),
}

struct Pending {
    slots: Vec<InSlot>,
    line_input: bool,
    prompt: String,
}

struct Array {
    dims: Vec<(i16, i16)>,
    data: Vec<Value>,
}

#[derive(Clone)]
struct UserFn {
    params: Vec<Ident>,
    body: Vec<u8>,
}

#[derive(Clone)]
struct ForFrame {
    var: Name,
    limit: Number,
    step: Number,
    body: Ip,
}

#[derive(Clone, Copy, Debug)]
struct Ip {
    line: Option<u16>,
    offset: usize,
}

struct Datum {
    line: u16,
    text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Run,
    Input,
}

enum Flow {
    Next,
    Jump(Ip),
    Ended,
    Stopped,
    Input,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Run,
    Check,
}

pub struct Vm {
    phase: Phase,
    scalars: HashMap<Name, Value>,
    arrays: HashMap<Name, Array>,
    defs: [Ty; 26],
    base: i16,
    base_locked: bool,
    fns: HashMap<Name, UserFn>,
    data: Vec<Datum>,
    data_at: usize,
    gosub: Vec<Ip>,
    fors: Vec<ForFrame>,
    whiles: Vec<Ip>,
    ip: Ip,
    direct: Vec<u8>,
    cont: Option<Ip>,
    announce: bool,
    pending: Option<Pending>,
    seed: u32,
    last_rnd: Mbf,
    err: i16,
    erl: i32,
    double_math: bool,
    reported_free: i32,
    date_override: Option<String>,
    time_override: Option<String>,
    lpt_col: usize,
    current_line: Option<u16>,
    /// One 64K data segment. `DEF SEG` is remembered for `BSAVE` and does not
    /// map a second segment or hardware.
    memory: Vec<u8>,
    segment: u16,
    on_error: Option<u16>,
    handling_error: bool,
    resume_at: Option<Ip>,
    /// Process environment captured at startup. `ENVIRON` edits this copy.
    /// `SHELL` passes it to the child. `RUN` and `CLEAR` do not reset it.
    environ: Vec<(String, String)>,
}

pub struct Host<'a> {
    pub program: &'a mut Program,
    pub screen: &'a mut Screen,
    pub beep: &'a mut bool,
    pub lpt: &'a mut String,
    pub echo: bool,
    pub inkey: &'a mut VecDeque<u8>,
    pub key_offset: &'a mut usize,
    pub trace: &'a mut bool,
    pub quit: &'a mut bool,
    pub files: &'a mut Files,
    pub tone: &'a mut Option<(u32, u32)>,
}

pub enum Status {
    Idle,
    Yield,
    Ended,
    Break {
        line: Option<u16>,
    },
    Error {
        message: &'static str,
        line: Option<u16>,
    },
    Input,
    Quit,
}

impl Vm {
    pub fn new() -> Self {
        Self {
            phase: Phase::Idle,
            scalars: HashMap::new(),
            arrays: HashMap::new(),
            defs: [Ty::Single; 26],
            base: 0,
            base_locked: false,
            fns: HashMap::new(),
            data: Vec::new(),
            data_at: 0,
            gosub: Vec::new(),
            fors: Vec::new(),
            whiles: Vec::new(),
            ip: Ip {
                line: None,
                offset: 0,
            },
            direct: Vec::new(),
            cont: None,
            announce: false,
            pending: None,
            seed: 0x123456,
            last_rnd: Mbf::zero(false),
            err: 0,
            erl: 0,
            double_math: false,
            reported_free: 60300,
            date_override: None,
            time_override: None,
            lpt_col: 0,
            current_line: None,
            memory: vec![0; 65536],
            segment: 0,
            on_error: None,
            handling_error: false,
            resume_at: None,
            environ: std::env::vars().collect(),
        }
    }

    pub fn set_double_math(&mut self, on: bool) {
        self.double_math = on;
    }

    pub fn set_free(&mut self, bytes: i32) {
        self.reported_free = bytes;
    }

    pub fn is_running(&self) -> bool {
        self.phase == Phase::Run
    }

    pub fn waiting_input(&self) -> bool {
        self.phase == Phase::Input
    }

    pub fn can_continue(&self) -> bool {
        self.cont.is_some() && self.phase == Phase::Idle
    }

    pub fn invalidate_cont(&mut self) {
        self.cont = None;
    }

    pub fn request_break(&mut self) {
        if self.phase == Phase::Run || self.phase == Phase::Input {
            self.phase = Phase::Idle;
            self.pending = None;
            // The caller prints Break. Continuation is the statement that was next.
            self.cont = Some(self.ip);
        }
    }

    /// A runtime error or a failed `INPUT` leaves nothing to `CONT`.
    pub fn abandon(&mut self) {
        self.phase = Phase::Idle;
        self.pending = None;
        self.cont = None;
    }

    pub fn break_line(&self) -> Option<u16> {
        self.current_line.or(self.ip.line)
    }

    pub fn reset_runtime(&mut self) {
        self.scalars.clear();
        self.arrays.clear();
        self.fns.clear();
        self.defs = [Ty::Single; 26];
        self.base = 0;
        self.base_locked = false;
        self.data_at = 0;
        self.gosub.clear();
        self.fors.clear();
        self.whiles.clear();
        self.cont = None;
        self.pending = None;
        self.err = 0;
        self.erl = 0;
        self.lpt_col = 0;
        self.on_error = None;
        self.handling_error = false;
        self.resume_at = None;
        // `memory` and `environ` stay. RUN must not wipe PEEK or ENVIRON.
    }

    fn clear_vars(&mut self) {
        self.scalars.clear();
        self.arrays.clear();
        self.fns.clear();
        self.gosub.clear();
        self.fors.clear();
        self.whiles.clear();
        self.base_locked = false;
        self.data_at = 0;
    }
}

struct Parser<'a> {
    toks: &'a [u8],
    i: usize,
    at: usize,
    cur: Tok,
}

impl<'a> Parser<'a> {
    fn new(toks: &'a [u8], offset: usize) -> Self {
        let mut parser = Self {
            toks,
            i: offset,
            at: offset,
            cur: Tok::End,
        };
        parser.bump();
        parser
    }

    fn bump(&mut self) {
        if matches!(self.cur, Tok::End) && self.i >= self.toks.len() {
            return;
        }
        self.at = self.i;
        self.cur = crunch::next_tok(self.toks, &mut self.i);
    }
}

/// Crunch and reject a line that is not a legal statement list.
pub fn store_source_line(
    program: &mut Program,
    number: u16,
    rest: &str,
) -> Result<(), crate::crunch::Fault> {
    if rest.trim().is_empty() {
        program.delete_line(number);
        return Ok(());
    }
    let tokens = prepare(rest)?;
    let listed = crunch::list_tokens(&tokens);
    program.insert(number, tokens, format!("{number} {listed}"));
    Ok(())
}

pub fn prepare(source: &str) -> Result<Vec<u8>, crate::crunch::Fault> {
    let tokens = crunch::crunch(source)?;
    if let Err(err) = check(&tokens) {
        return Err(crate::crunch::Fault {
            kind: crate::crunch::FaultKind::Syntax,
            column: err,
        });
    }
    Ok(tokens)
}

pub fn check(tokens: &[u8]) -> Result<(), usize> {
    let mut parser = Parser::new(tokens, 0);
    while !matches!(parser.cur, Tok::End) {
        if matches!(parser.cur, Tok::Colon) {
            parser.bump();
            continue;
        }
        if parser.cur.is("ELSE") {
            return Err(parser.at);
        }
        walk(&mut parser, Mode::Check, None).map_err(|_| parser.at)?;
    }
    Ok(())
}

pub fn begin_direct(vm: &mut Vm, tokens: Vec<u8>) {
    vm.direct = tokens;
    vm.ip = Ip {
        line: None,
        offset: 0,
    };
    vm.phase = Phase::Run;
    vm.announce = false;
    vm.current_line = None;
    vm.pending = None;
}

pub fn begin_program(vm: &mut Vm, program: &Program, start: Option<u16>) -> bool {
    vm.reset_runtime();
    vm.data = collect_data(program);
    if start.is_none() && program.is_empty() {
        vm.phase = Phase::Idle;
        return false;
    }
    let line = program.first_from(start.unwrap_or(0));
    let Some(line) = line else {
        vm.phase = Phase::Idle;
        return false;
    };
    vm.ip = Ip {
        line: Some(line),
        offset: 0,
    };
    vm.phase = Phase::Run;
    vm.announce = true;
    vm.current_line = Some(line);
    true
}

pub fn resume(vm: &mut Vm) -> bool {
    let Some(ip) = vm.cont.take() else {
        return false;
    };
    vm.ip = ip;
    vm.phase = Phase::Run;
    vm.announce = true;
    vm.current_line = ip.line;
    true
}

pub fn burst(vm: &mut Vm, host: &mut Host<'_>, mut budget: u32) -> Status {
    loop {
        if vm.phase == Phase::Input {
            return Status::Input;
        }
        if vm.phase != Phase::Run {
            return Status::Idle;
        }
        if budget == 0 {
            return Status::Yield;
        }
        budget -= 1;
        let line_no = vm.ip.line;
        vm.current_line = line_no;
        let tokens = match line_no {
            Some(n) => match host.program.code(n) {
                Some(code) => code.to_vec(),
                None => {
                    vm.phase = Phase::Idle;
                    vm.cont = None;
                    return Status::Ended;
                }
            },
            None => vm.direct.clone(),
        };
        if vm.ip.offset >= tokens.len() || tokens.get(vm.ip.offset) == Some(&0) {
            if !advance_line(vm, host.program) {
                vm.phase = Phase::Idle;
                vm.cont = None;
                return Status::Ended;
            }
            continue;
        }
        if vm.announce {
            if *host.trace {
                if let Some(n) = line_no {
                    emit(host, &format!("[{n}]"));
                }
            }
            vm.announce = false;
        }
        let mut parser = Parser::new(&tokens, vm.ip.offset);
        if matches!(parser.cur, Tok::End) {
            if !advance_line(vm, host.program) {
                vm.phase = Phase::Idle;
                vm.cont = None;
                return Status::Ended;
            }
            continue;
        }
        if matches!(parser.cur, Tok::Colon) {
            parser.bump();
            vm.ip.offset = parser.at;
            continue;
        }
        if parser.cur.is("ELSE") {
            parser.bump();
            skip_until_else(&mut parser, false);
            vm.ip.offset = parser.at;
            if matches!(parser.cur, Tok::End) {
                vm.ip.offset = parser.i;
            }
            continue;
        }
        let flow = match walk(&mut parser, Mode::Run, Some((vm, host))) {
            Ok(flow) => flow,
            Err(err) => {
                let line = vm.current_line;
                vm.err = err.code;
                vm.erl = i32::from(line.unwrap_or(0));
                if let Some(handler) = vm.on_error.filter(|n| *n != 0) {
                    if !vm.handling_error {
                        vm.handling_error = true;
                        vm.resume_at = Some(vm.ip);
                        vm.ip = Ip {
                            line: Some(handler),
                            offset: 0,
                        };
                        vm.announce = true;
                        vm.current_line = Some(handler);
                        continue;
                    }
                }
                vm.phase = Phase::Idle;
                vm.cont = None;
                vm.pending = None;
                vm.handling_error = false;
                return Status::Error {
                    message: err.message,
                    line,
                };
            }
        };
        match flow {
            Flow::Next => {
                vm.ip.offset = if matches!(parser.cur, Tok::End) {
                    parser.i
                } else {
                    parser.at
                };
            }
            Flow::Jump(ip) => {
                vm.ip = ip;
                vm.announce = true;
                vm.current_line = ip.line;
            }
            Flow::Ended => {
                vm.phase = Phase::Idle;
                vm.cont = None;
                return Status::Ended;
            }
            Flow::Stopped => {
                vm.phase = Phase::Idle;
                vm.cont = Some(if matches!(parser.cur, Tok::End) {
                    Ip {
                        line: line_no,
                        offset: parser.i,
                    }
                } else {
                    Ip {
                        line: line_no,
                        offset: parser.at,
                    }
                });
                return Status::Break { line: line_no };
            }
            Flow::Input => return Status::Input,
            Flow::Quit => {
                vm.phase = Phase::Idle;
                vm.cont = None;
                *host.quit = true;
                return Status::Quit;
            }
        }
    }
}

fn advance_line(vm: &mut Vm, program: &Program) -> bool {
    let Some(line) = vm.ip.line else {
        return false;
    };
    let Some(next) = program.next_after(line) else {
        return false;
    };
    vm.ip = Ip {
        line: Some(next),
        offset: 0,
    };
    vm.announce = true;
    vm.current_line = Some(next);
    true
}

pub fn input_prompt(vm: &Vm) -> &str {
    vm.pending.as_ref().map(|p| p.prompt.as_str()).unwrap_or("")
}

pub fn finish_input(vm: &mut Vm, text: &str) -> Result<(), &'static str> {
    let pending = vm.pending.take().ok_or(REDO)?;
    if pending.line_input {
        let Some(InSlot::Var(name)) = pending.slots.first() else {
            return Err(REDO);
        };
        if text.len() > 255 {
            vm.pending = Some(pending);
            return Err(STRING_LONG);
        }
        vm.scalars
            .insert(name.clone(), Value::Str(text.to_string()));
        vm.phase = Phase::Run;
        return Ok(());
    }
    let fields = split_fields(text);
    if fields.len() != pending.slots.len() {
        vm.pending = Some(pending);
        return Err(REDO);
    }
    let mut assigned = Vec::new();
    for (slot, field) in pending.slots.iter().zip(fields.iter()) {
        let value = match slot {
            InSlot::Var(name) | InSlot::Elem(name, _) => coerce_field(field, name.ty)?,
        };
        assigned.push(value);
    }
    for (slot, value) in pending.slots.iter().zip(assigned) {
        match slot {
            InSlot::Var(name) => {
                vm.scalars.insert(name.clone(), value);
            }
            InSlot::Elem(name, idx) => {
                let Some(array) = vm.arrays.get_mut(name) else {
                    vm.pending = Some(pending);
                    return Err(SUBSCRIPT);
                };
                let off = index_of(&array.dims, idx).map_err(|_| SUBSCRIPT)?;
                array.data[off] = value;
            }
        }
    }
    vm.phase = Phase::Run;
    Ok(())
}

fn coerce_field(field: &str, ty: Ty) -> Result<Value, &'static str> {
    let field = field.trim();
    if ty == Ty::Str {
        let text = if field.len() >= 2 && field.starts_with('"') && field.ends_with('"') {
            field[1..field.len() - 1].to_string()
        } else {
            field.to_string()
        };
        if text.len() > 255 {
            return Err(STRING_LONG);
        }
        return Ok(Value::Str(text));
    }
    if field.is_empty() {
        return Ok(Value::zero(ty));
    }
    let (neg, rest) = if let Some(rest) = field.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = field.strip_prefix('+') {
        (false, rest)
    } else {
        (false, field)
    };
    let double = rest.bytes().any(|c| c == b'D' || c == b'd' || c == b'#') || ty == Ty::Double;
    let mut number = Mbf::parse(rest.trim_end_matches(['!', '#']), double).map_err(|_| REDO)?;
    if neg {
        number = number.neg();
    }
    match ty {
        Ty::Int => Value::Int(cint_mbf(number).map_err(|_| REDO)?).pipe(Ok),
        Ty::Single => Ok(Value::Single(number.to_single().map_err(|_| REDO)?)),
        Ty::Double => Ok(Value::Double(number.to_double())),
        Ty::Str => unreachable!(),
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

fn split_fields(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for &b in bytes {
        if b == b'"' {
            quoted = !quoted;
            cur.push('"');
        } else if b == b',' && !quoted {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(char::from(b));
        }
    }
    out.push(cur);
    out
}

fn walk(
    parser: &mut Parser<'_>,
    mode: Mode,
    mut ctx: Option<(&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    if matches!(parser.cur, Tok::Rem(_) | Tok::Tick(_) | Tok::Data(_)) {
        parser.bump();
        return Ok(Flow::Next);
    }
    if parser.cur.is("LET") {
        parser.bump();
    }
    if parser.cur.is("MID$") {
        return assign_mid(parser, mode, ctx.as_mut());
    }
    if matches!(parser.cur, Tok::Ident(_)) {
        let ident = take_ident(parser)?;
        if matches!(parser.cur, Tok::Punct(b'(')) {
            let indexes = arg_list(parser)?;
            expect_word(parser, "=")?;
            let expr = parse_expr(parser)?;
            if mode == Mode::Run {
                let (vm, host) = ctx.as_mut().ok_or_else(|| fault(SYNTAX, 2))?;
                let name = ident.resolve(&vm.defs);
                let idx = eval_indexes(vm, host, &indexes)?;
                let value = eval_expr(vm, host, &expr)?;
                store_elem(vm, name, &idx, value)?;
            }
            return Ok(Flow::Next);
        }
        expect_word(parser, "=")?;
        let expr = parse_expr(parser)?;
        if mode == Mode::Run {
            let (vm, host) = ctx.as_mut().ok_or_else(|| fault(SYNTAX, 2))?;
            let name = ident.resolve(&vm.defs);
            let value = eval_expr(vm, host, &expr)?;
            store_var(vm, name, value)?;
        }
        return Ok(Flow::Next);
    }
    let word = parser.cur.word().unwrap_or("").to_string();
    match word.as_str() {
        "PRINT" | "LPRINT" | "?" => {
            let lpt = word == "LPRINT";
            parser.bump();
            let channel = if !lpt && matches!(parser.cur, Tok::Punct(b'#')) {
                parser.bump();
                let n = parse_expr(parser)?;
                expect_punct(parser, b',')?;
                Some(n)
            } else {
                None
            };
            let items = parse_print_items(parser)?;
            if mode == Mode::Run {
                let (vm, host) = ctx.as_mut().ok_or_else(|| fault(SYNTAX, 2))?;
                if let Some(n) = channel {
                    let num = cint_value(&eval_expr(vm, host, &n)?)?;
                    let text = format_print(vm, host, &items)?;
                    host.files.write_text(num, &text).map_err(file_fault)?;
                } else {
                    do_print(vm, host, lpt, &items)?;
                }
            }
            Ok(Flow::Next)
        }
        "GOTO" => {
            parser.bump();
            let dest = parse_line_ref(parser)?;
            if mode == Mode::Check {
                return Ok(Flow::Next);
            }
            let (vm, host) = ctx.as_mut().ok_or_else(|| fault(SYNTAX, 2))?;
            Ok(Flow::Jump(resolve_line(vm, host.program, dest)?))
        }
        "GOSUB" => {
            parser.bump();
            let dest = parse_line_ref(parser)?;
            let ret = parser.at;
            if mode == Mode::Check {
                return Ok(Flow::Next);
            }
            let (vm, host) = ctx.as_mut().ok_or_else(|| fault(SYNTAX, 2))?;
            let target = resolve_line(vm, host.program, dest)?;
            vm.gosub.push(Ip {
                line: vm.current_line,
                offset: ret,
            });
            Ok(Flow::Jump(target))
        }
        "RETURN" => {
            parser.bump();
            if mode == Mode::Check {
                return Ok(Flow::Next);
            }
            let (vm, _) = ctx.as_mut().ok_or_else(|| fault(SYNTAX, 2))?;
            let ip = vm.gosub.pop().ok_or_else(|| fault(RETURN_WITHOUT, 3))?;
            Ok(Flow::Jump(ip))
        }
        "IF" => exec_if(parser, mode, ctx.as_mut()),
        "FOR" => exec_for(parser, mode, ctx.as_mut()),
        "NEXT" => exec_next(parser, mode, ctx.as_mut()),
        "WHILE" => exec_while(parser, mode, ctx.as_mut()),
        "WEND" => {
            parser.bump();
            if mode == Mode::Check {
                return Ok(Flow::Next);
            }
            let (vm, _) = ctx.as_mut().ok_or_else(|| fault(SYNTAX, 2))?;
            let ip = vm.whiles.pop().ok_or_else(|| fault(WEND_WITHOUT, 30))?;
            Ok(Flow::Jump(ip))
        }
        "ON" => exec_on(parser, mode, ctx.as_mut()),
        "END" => {
            parser.bump();
            if mode == Mode::Run {
                Ok(Flow::Ended)
            } else {
                Ok(Flow::Next)
            }
        }
        "STOP" => {
            parser.bump();
            if mode == Mode::Run {
                Ok(Flow::Stopped)
            } else {
                Ok(Flow::Next)
            }
        }
        "REM" => {
            parser.bump();
            Ok(Flow::Next)
        }
        "CLS" => {
            parser.bump();
            if mode == Mode::Run {
                let (_, host) = ctx.as_mut().ok_or_else(|| fault(SYNTAX, 2))?;
                host.screen.clear();
            }
            Ok(Flow::Next)
        }
        "BEEP" => {
            parser.bump();
            if mode == Mode::Run {
                let (_, host) = ctx.as_mut().ok_or_else(|| fault(SYNTAX, 2))?;
                *host.beep = true;
            }
            Ok(Flow::Next)
        }
        "WIDTH" => exec_width(parser, mode, ctx.as_mut()),
        "LOCATE" => exec_locate(parser, mode, ctx.as_mut()),
        "COLOR" => exec_color(parser, mode, ctx.as_mut()),
        "DIM" => exec_dim(parser, mode, ctx.as_mut()),
        "ERASE" => exec_erase(parser, mode, ctx.as_mut()),
        "DEFINT" => exec_def(parser, Ty::Int, mode, ctx.as_mut()),
        "DEFSNG" => exec_def(parser, Ty::Single, mode, ctx.as_mut()),
        "DEFDBL" => exec_def(parser, Ty::Double, mode, ctx.as_mut()),
        "DEFSTR" => exec_def(parser, Ty::Str, mode, ctx.as_mut()),
        "DEF" => {
            parser.bump();
            if name_is(&parser.cur, "SEG") {
                exec_def_seg(parser, mode, ctx.as_mut())
            } else {
                exec_def_fn(parser, mode, ctx.as_mut())
            }
        }
        "SWAP" => exec_swap(parser, mode, ctx.as_mut()),
        "INPUT" => exec_input(parser, mode, ctx.as_mut(), false),
        "WRITE" => exec_write(parser, mode, ctx.as_mut()),
        "OPEN" => exec_open(parser, mode, ctx.as_mut()),
        "CLOSE" => exec_close(parser, mode, ctx.as_mut()),
        "LOAD" => exec_load(parser, mode, ctx.as_mut(), false),
        "MERGE" => exec_merge(parser, mode, ctx.as_mut()),
        "SAVE" => exec_save(parser, mode, ctx.as_mut()),
        "CHAIN" => exec_load(parser, mode, ctx.as_mut(), true),
        "KILL" => exec_kill(parser, mode, ctx.as_mut()),
        "NAME" => exec_name(parser, mode, ctx.as_mut()),
        "FILES" => exec_files(parser, mode, ctx.as_mut()),
        "CHDIR" => exec_dir(parser, mode, ctx.as_mut(), DirOp::Chdir),
        "MKDIR" => exec_dir(parser, mode, ctx.as_mut(), DirOp::Mkdir),
        "RMDIR" => exec_dir(parser, mode, ctx.as_mut(), DirOp::Rmdir),
        "SHELL" => exec_shell(parser, mode, ctx.as_mut()),
        "RENUM" => exec_renum(parser, mode, ctx.as_mut()),
        "LSET" => exec_justify(parser, mode, ctx.as_mut(), true),
        "RSET" => exec_justify(parser, mode, ctx.as_mut(), false),
        "ENVIRON" => exec_environ(parser, mode, ctx.as_mut()),
        "RESET" => exec_reset(parser, mode, ctx.as_mut()),
        "POKE" => exec_poke(parser, mode, ctx.as_mut()),
        "BSAVE" => exec_bsave(parser, mode, ctx.as_mut()),
        "BLOAD" => exec_bload(parser, mode, ctx.as_mut()),
        "SOUND" => exec_sound(parser, mode, ctx.as_mut()),
        "ERROR" => exec_error(parser, mode, ctx.as_mut()),
        "RESUME" => exec_resume(parser, mode, ctx.as_mut()),
        "LINE" => {
            parser.bump();
            if !parser.cur.is("INPUT") {
                return Err(fault(SYNTAX, 2));
            }
            exec_input(parser, mode, ctx.as_mut(), true)
        }
        "READ" => exec_read(parser, mode, ctx.as_mut()),
        "RESTORE" => exec_restore(parser, mode, ctx.as_mut()),
        "DATA" => {
            parser.bump();
            Ok(Flow::Next)
        }
        "TRON" => {
            parser.bump();
            if mode == Mode::Run {
                let (_, host) = ctx.as_mut().ok_or_else(|| fault(SYNTAX, 2))?;
                *host.trace = true;
            }
            Ok(Flow::Next)
        }
        "TROFF" => {
            parser.bump();
            if mode == Mode::Run {
                let (_, host) = ctx.as_mut().ok_or_else(|| fault(SYNTAX, 2))?;
                *host.trace = false;
            }
            Ok(Flow::Next)
        }
        "CLEAR" => exec_clear(parser, mode, ctx.as_mut()),
        "OPTION" => exec_option(parser, mode, ctx.as_mut()),
        "RANDOMIZE" => exec_randomize(parser, mode, ctx.as_mut()),
        "RUN" => exec_run(parser, mode, ctx.as_mut()),
        "CONT" => {
            parser.bump();
            if mode == Mode::Check {
                return Ok(Flow::Next);
            }
            Err(fault("Can't continue", 17))
        }
        "NEW" => {
            parser.bump();
            if mode == Mode::Run {
                let (vm, host) = ctx.as_mut().ok_or_else(|| fault(SYNTAX, 2))?;
                host.program.clear();
                vm.reset_runtime();
                Ok(Flow::Ended)
            } else {
                Ok(Flow::Next)
            }
        }
        "LIST" | "LLIST" => {
            let to_lpt = word == "LLIST";
            parser.bump();
            let (start, end) = parse_list_range(parser)?;
            if mode == Mode::Run {
                let (_, host) = ctx.as_mut().ok_or_else(|| fault(SYNTAX, 2))?;
                if host.program.is_protected() {
                    return Err(fault(ILLEGAL, 5));
                }
                let start = concrete_line(host.program, start)?;
                let end = concrete_line(host.program, end)?;
                let lines: Vec<String> = host
                    .program
                    .in_range(start, end)
                    .into_iter()
                    .map(|(_, text)| text.to_string())
                    .collect();
                for line in lines {
                    if to_lpt {
                        host.lpt.push_str(&line);
                        host.lpt.push('\n');
                    } else {
                        emit(host, &line);
                        emit(host, "\n");
                    }
                }
            }
            Ok(Flow::Next)
        }
        "SYSTEM" => {
            parser.bump();
            if mode == Mode::Run {
                Ok(Flow::Quit)
            } else {
                Ok(Flow::Next)
            }
        }
        "CALL" | "OUT" | "WAIT" | "PLAY" | "DRAW" | "PSET" | "PRESET" | "CIRCLE" | "PAINT"
        | "SCREEN" | "KEY" | "EDIT" | "DELETE" | "AUTO" | "FIELD" | "GET" | "PUT" => {
            // Recognized, not part of this milestone. Skip the rest of the statement.
            parser.bump();
            skip_statement(parser);
            if mode == Mode::Run {
                Err(fault(ILLEGAL, 5))
            } else {
                Ok(Flow::Next)
            }
        }
        "DATE$" | "TIME$" => assign_clock(parser, mode, ctx.as_mut(), word == "DATE$"),
        "" => Err(fault(SYNTAX, 2)),
        _ => Err(fault(SYNTAX, 2)),
    }
}

fn emit(host: &mut Host<'_>, text: &str) {
    if text.bytes().any(|b| b == 0x07) {
        *host.beep = true;
    }
    if host.echo {
        host.lpt.push_str(text);
    }
    host.screen.print_str(text);
}

fn math(result: Result<Mbf, MathError>) -> Result<Mbf, RunErr> {
    match result {
        Ok(value) => Ok(value),
        Err(MathError::Overflow) => Err(fault(OVERFLOW, 6)),
        Err(MathError::Div0) => Err(fault(DIV0, 11)),
    }
}

fn bytes_string(bytes: &[u8]) -> String {
    bytes.iter().copied().map(char::from).collect()
}

fn collect_data(program: &Program) -> Vec<Datum> {
    let mut out = Vec::new();
    for (line, code) in program.iter_code() {
        let mut i = 0;
        loop {
            match crunch::next_tok(code, &mut i) {
                Tok::End => break,
                Tok::Data(bytes) => {
                    for item in split_fields(&bytes_string(&bytes)) {
                        out.push(Datum {
                            line,
                            text: item.trim().to_string(),
                        });
                    }
                }
                Tok::Rem(_) | Tok::Tick(_) => break,
                _ => {}
            }
        }
    }
    out
}

fn is_line_ref(tok: &Tok) -> bool {
    matches!(tok, Tok::Line(_) | Tok::Punct(b'.'))
}

#[derive(Clone, Copy)]
enum LineRef {
    Num(u16),
    Dot,
}

fn parse_line_ref(parser: &mut Parser<'_>) -> Result<LineRef, RunErr> {
    match parser.cur {
        Tok::Line(n) => {
            parser.bump();
            Ok(LineRef::Num(n))
        }
        Tok::Punct(b'.') => {
            parser.bump();
            Ok(LineRef::Dot)
        }
        _ => Err(fault(SYNTAX, 2)),
    }
}

fn resolve_line(_vm: &Vm, program: &Program, dest: LineRef) -> Result<Ip, RunErr> {
    let number = match dest {
        LineRef::Num(n) => n,
        LineRef::Dot => program.last.ok_or_else(|| fault(UNDEF_LINE, 8))?,
    };
    if program.contains(number) {
        Ok(Ip {
            line: Some(number),
            offset: 0,
        })
    } else {
        Err(fault(UNDEF_LINE, 8))
    }
}

fn skip_statement(parser: &mut Parser<'_>) {
    loop {
        match parser.cur {
            Tok::End | Tok::Colon => return,
            Tok::Rem(_) | Tok::Tick(_) => {
                parser.bump();
                return;
            }
            _ => parser.bump(),
        }
    }
}

fn skip_until_else(parser: &mut Parser<'_>, consume: bool) {
    let mut depth = 0i32;
    loop {
        if matches!(parser.cur, Tok::End) {
            return;
        }
        if matches!(parser.cur, Tok::Rem(_) | Tok::Tick(_)) {
            parser.bump();
            return;
        }
        if parser.cur.is("IF") {
            depth += 1;
            parser.bump();
            continue;
        }
        if parser.cur.is("ELSE") {
            if depth == 0 {
                if consume {
                    parser.bump();
                }
                return;
            }
            depth -= 1;
            parser.bump();
            continue;
        }
        parser.bump();
    }
}

fn terminated(tok: &Tok) -> bool {
    matches!(tok, Tok::End | Tok::Colon) || tok.is("ELSE")
}

fn expect_word(parser: &mut Parser<'_>, name: &str) -> Result<(), RunErr> {
    if parser.cur.is(name) {
        parser.bump();
        Ok(())
    } else {
        Err(fault(SYNTAX, 2))
    }
}

fn expect_punct(parser: &mut Parser<'_>, ch: u8) -> Result<(), RunErr> {
    if matches!(parser.cur, Tok::Punct(c) if c == ch) {
        parser.bump();
        Ok(())
    } else {
        Err(fault(SYNTAX, 2))
    }
}

fn take_ident(parser: &mut Parser<'_>) -> Result<Ident, RunErr> {
    let name = match &parser.cur {
        Tok::Ident(name) => name.clone(),
        _ => return Err(fault(SYNTAX, 2)),
    };
    parser.bump();
    let mut ident = Ident::parse(&name);
    // `A#` is stored as the name bytes plus a `#` punctuation byte, same as
    // the file mark in `AS #1`. A name absorbs that byte; `AS` does not,
    // because this function is only called when a variable is expected.
    if ident.suffix.is_none() && matches!(parser.cur, Tok::Punct(b'#')) {
        ident.suffix = Some(Ty::Double);
        parser.bump();
    }
    Ok(ident)
}

fn arg_list(parser: &mut Parser<'_>) -> Result<Vec<Expr>, RunErr> {
    expect_punct(parser, b'(')?;
    let mut args = Vec::new();
    if matches!(parser.cur, Tok::Punct(b')')) {
        parser.bump();
        return Ok(args);
    }
    loop {
        args.push(parse_expr(parser)?);
        if matches!(parser.cur, Tok::Punct(b',')) {
            parser.bump();
            continue;
        }
        expect_punct(parser, b')')?;
        break;
    }
    Ok(args)
}

fn parse_expr(parser: &mut Parser<'_>) -> Result<Expr, RunErr> {
    parse_imp(parser)
}

fn parse_binary(
    parser: &mut Parser<'_>,
    mut child: impl FnMut(&mut Parser<'_>) -> Result<Expr, RunErr>,
    mut op: impl FnMut(&Tok) -> Option<BinOp>,
) -> Result<Expr, RunErr> {
    let mut left = child(parser)?;
    while let Some(kind) = op(&parser.cur) {
        parser.bump();
        let right = child(parser)?;
        left = Expr::Binary(kind, Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_imp(parser: &mut Parser<'_>) -> Result<Expr, RunErr> {
    parse_binary(parser, parse_eqv, |t| t.is("IMP").then_some(BinOp::Imp))
}
fn parse_eqv(parser: &mut Parser<'_>) -> Result<Expr, RunErr> {
    parse_binary(parser, parse_xor, |t| t.is("EQV").then_some(BinOp::Eqv))
}
fn parse_xor(parser: &mut Parser<'_>) -> Result<Expr, RunErr> {
    parse_binary(parser, parse_or, |t| t.is("XOR").then_some(BinOp::Xor))
}
fn parse_or(parser: &mut Parser<'_>) -> Result<Expr, RunErr> {
    parse_binary(parser, parse_and, |t| t.is("OR").then_some(BinOp::Or))
}
fn parse_and(parser: &mut Parser<'_>) -> Result<Expr, RunErr> {
    parse_binary(parser, parse_not, |t| t.is("AND").then_some(BinOp::And))
}

fn parse_not(parser: &mut Parser<'_>) -> Result<Expr, RunErr> {
    if parser.cur.is("NOT") {
        parser.bump();
        Ok(Expr::Unary(UnOp::Not, Box::new(parse_not(parser)?)))
    } else {
        parse_rel(parser)
    }
}

fn rel_op(parser: &mut Parser<'_>) -> Option<BinOp> {
    if parser.cur.is("=") {
        parser.bump();
        return Some(BinOp::Eq);
    }
    if parser.cur.is(">") {
        parser.bump();
        if parser.cur.is("=") {
            parser.bump();
            Some(BinOp::Ge)
        } else {
            Some(BinOp::Gt)
        }
    } else if parser.cur.is("<") {
        parser.bump();
        if parser.cur.is("=") {
            parser.bump();
            Some(BinOp::Le)
        } else if parser.cur.is(">") {
            parser.bump();
            Some(BinOp::Ne)
        } else {
            Some(BinOp::Lt)
        }
    } else {
        None
    }
}

fn parse_rel(parser: &mut Parser<'_>) -> Result<Expr, RunErr> {
    let mut left = parse_add(parser)?;
    while let Some(op) = rel_op(parser) {
        let right = parse_add(parser)?;
        left = Expr::Binary(op, Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_add(parser: &mut Parser<'_>) -> Result<Expr, RunErr> {
    parse_binary(parser, parse_mod, |t| {
        if t.is("+") {
            Some(BinOp::Add)
        } else if t.is("-") {
            Some(BinOp::Sub)
        } else {
            None
        }
    })
}

fn parse_mod(parser: &mut Parser<'_>) -> Result<Expr, RunErr> {
    parse_binary(parser, parse_idiv, |t| t.is("MOD").then_some(BinOp::Mod))
}

fn parse_idiv(parser: &mut Parser<'_>) -> Result<Expr, RunErr> {
    parse_binary(parser, parse_mul, |t| t.is("\\").then_some(BinOp::IDiv))
}

fn parse_mul(parser: &mut Parser<'_>) -> Result<Expr, RunErr> {
    parse_binary(parser, parse_unary, |t| {
        if t.is("*") {
            Some(BinOp::Mul)
        } else if t.is("/") {
            Some(BinOp::Div)
        } else {
            None
        }
    })
}

fn parse_unary(parser: &mut Parser<'_>) -> Result<Expr, RunErr> {
    if parser.cur.is("-") {
        parser.bump();
        Ok(Expr::Unary(UnOp::Neg, Box::new(parse_unary(parser)?)))
    } else if parser.cur.is("+") {
        parser.bump();
        parse_unary(parser)
    } else {
        parse_pow(parser)
    }
}

fn parse_pow(parser: &mut Parser<'_>) -> Result<Expr, RunErr> {
    let mut left = parse_primary(parser)?;
    if parser.cur.is("^") {
        parser.bump();
        let right = parse_unary(parser)?;
        left = Expr::Binary(BinOp::Pow, Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn func_of(name: &str) -> Option<Func> {
    Some(match name {
        "LEFT$" => Func::Left,
        "RIGHT$" => Func::Right,
        "MID$" => Func::Mid,
        "CHR$" => Func::Chr,
        "STR$" => Func::StrFn,
        "STRING$" => Func::StringFn,
        "SPACE$" => Func::Space,
        "HEX$" => Func::Hex,
        "OCT$" => Func::Oct,
        "SGN" => Func::Sgn,
        "INT" => Func::Int,
        "ABS" => Func::Abs,
        "SQR" => Func::Sqr,
        "RND" => Func::Rnd,
        "SIN" => Func::Sin,
        "LOG" => Func::Log,
        "EXP" => Func::Exp,
        "COS" => Func::Cos,
        "TAN" => Func::Tan,
        "ATN" => Func::Atn,
        "FRE" => Func::Fre,
        "LEN" => Func::Len,
        "VAL" => Func::Val,
        "ASC" => Func::Asc,
        "INSTR" => Func::Instr,
        "CINT" => Func::Cint,
        "CSNG" => Func::Csng,
        "CDBL" => Func::Cdbl,
        "FIX" => Func::Fix,
        "POS" => Func::Pos,
        "PEEK" => Func::Peek,
        "EOF" => Func::Eof,
        "LOF" => Func::Lof,
        "LOC" => Func::Loc,
        "MKI$" => Func::Mki,
        "MKS$" => Func::Mks,
        "MKD$" => Func::Mkd,
        "CVI" => Func::Cvi,
        "CVS" => Func::Cvs,
        "CVD" => Func::Cvd,
        _ => return None,
    })
}

fn naked_of(name: &str) -> Option<Naked> {
    Some(match name {
        "INKEY$" => Naked::Inkey,
        "CSRLIN" => Naked::Csrlin,
        "ERL" => Naked::Erl,
        "ERR" => Naked::Err,
        "DATE$" => Naked::Date,
        "TIME$" => Naked::Time,
        _ => return None,
    })
}

fn arity(func: Func) -> (usize, usize) {
    match func {
        Func::Left | Func::Right | Func::StringFn => (2, 2),
        Func::Mid | Func::Instr => (2, 3),
        Func::Rnd => (0, 1),
        Func::Unsupported => (0, 16),
        _ => (1, 1),
    }
}

fn parse_primary(parser: &mut Parser<'_>) -> Result<Expr, RunErr> {
    if matches!(parser.cur, Tok::Ident(_)) {
        let ident = take_ident(parser)?;
        return if matches!(parser.cur, Tok::Punct(b'(')) {
            Ok(Expr::Index(ident, arg_list(parser)?))
        } else {
            Ok(Expr::Var(ident))
        };
    }
    match &parser.cur {
        Tok::Int(n) => {
            let n = *n;
            parser.bump();
            Ok(Expr::Int(n))
        }
        Tok::Hex(n) | Tok::Oct(n) => {
            let n = *n as i16;
            parser.bump();
            Ok(Expr::Int(n))
        }
        Tok::Single(m) => {
            let m = *m;
            parser.bump();
            Ok(Expr::Single(m))
        }
        Tok::Double(m) => {
            let m = *m;
            parser.bump();
            Ok(Expr::Double(m))
        }
        Tok::Str(bytes) => {
            let text = bytes_string(bytes);
            parser.bump();
            Ok(Expr::Str(text))
        }
        Tok::Punct(b'(') => {
            parser.bump();
            let expr = parse_expr(parser)?;
            expect_punct(parser, b')')?;
            Ok(expr)
        }
        Tok::Word(name) => {
            let name = *name;
            parser.bump();
            if let Some(naked) = naked_of(name) {
                return Ok(Expr::Naked(naked));
            }
            if name == "FN" {
                let ident = take_ident(parser)?;
                let args = if matches!(parser.cur, Tok::Punct(b'(')) {
                    arg_list(parser)?
                } else {
                    Vec::new()
                };
                return Ok(Expr::UserFn(ident, args));
            }
            if name == "RND" && !matches!(parser.cur, Tok::Punct(b'(')) {
                return Ok(Expr::Call(Func::Rnd, Vec::new()));
            }
            if name == "ENVIRON" && matches!(parser.cur, Tok::Punct(b'$')) {
                parser.bump();
                let args = arg_list(parser)?;
                if args.len() != 1 {
                    return Err(fault(SYNTAX, 2));
                }
                return Ok(Expr::Call(Func::Environ, args));
            }
            if let Some(func) = func_of(name) {
                let args = arg_list(parser)?;
                let (min, max) = arity(func);
                if args.len() < min || args.len() > max {
                    return Err(fault(SYNTAX, 2));
                }
                return Ok(Expr::Call(func, args));
            }
            // PEEK, VARPTR, and the other later functions still crunch.
            if matches!(parser.cur, Tok::Punct(b'(')) {
                let _ = arg_list(parser)?;
                return Ok(Expr::Call(Func::Unsupported, Vec::new()));
            }
            Err(fault(SYNTAX, 2))
        }
        _ => Err(fault(MISSING, 22)),
    }
}

fn is_dash(tok: &Tok) -> bool {
    tok.is("-") || matches!(tok, Tok::Punct(b'-'))
}

fn parse_list_range(parser: &mut Parser<'_>) -> Result<(Option<LineRef>, Option<LineRef>), RunErr> {
    if terminated(&parser.cur) {
        return Ok((None, None));
    }
    if is_dash(&parser.cur) {
        parser.bump();
        return Ok((None, Some(parse_line_ref(parser)?)));
    }
    if !is_line_ref(&parser.cur) {
        return Err(fault(SYNTAX, 2));
    }
    let start = parse_line_ref(parser)?;
    if is_dash(&parser.cur) {
        parser.bump();
        let end = if is_line_ref(&parser.cur) {
            Some(parse_line_ref(parser)?)
        } else {
            None
        };
        return Ok((Some(start), end));
    }
    Ok((Some(start), Some(start)))
}

fn concrete_line(program: &Program, line: Option<LineRef>) -> Result<Option<u16>, RunErr> {
    match line {
        None => Ok(None),
        Some(LineRef::Num(n)) => Ok(Some(n)),
        Some(LineRef::Dot) => program.last.map(Some).ok_or_else(|| fault(UNDEF_LINE, 8)),
    }
}

fn num_is_zero(n: Number) -> bool {
    match n {
        Number::Int(i) => i == 0,
        Number::Single(m) | Number::Double(m) => m.is_zero(),
    }
}

fn step_negative(n: Number) -> bool {
    match n {
        Number::Int(i) => i < 0,
        Number::Single(m) | Number::Double(m) => m.is_negative(),
    }
}

fn to_mbf(n: Number, double: bool) -> Result<Mbf, RunErr> {
    let m = match n {
        Number::Int(i) => Mbf::from_i32(i32::from(i), double),
        Number::Single(m) | Number::Double(m) => m,
    };
    if double {
        Ok(m.to_double())
    } else {
        math(m.to_single())
    }
}

fn pack_mbf(m: Mbf, double: bool) -> Number {
    if double {
        Number::Double(m)
    } else {
        Number::Single(m)
    }
}

fn widen(a: Number, b: Number) -> Result<(Mbf, Mbf, bool), RunErr> {
    let double = matches!(a, Number::Double(_)) || matches!(b, Number::Double(_));
    Ok((to_mbf(a, double)?, to_mbf(b, double)?, double))
}

fn value_num(v: &Value) -> Result<Number, RunErr> {
    match v {
        Value::Int(i) => Ok(Number::Int(*i)),
        Value::Single(m) => Ok(Number::Single(*m)),
        Value::Double(m) => Ok(Number::Double(*m)),
        Value::Str(_) => Err(fault(TYPE_MISMATCH, 13)),
    }
}

fn num_value(n: Number) -> Value {
    match n {
        Number::Int(i) => Value::Int(i),
        Number::Single(m) => Value::Single(m),
        Number::Double(m) => Value::Double(m),
    }
}

fn cint_mbf(m: Mbf) -> Result<i16, RunErr> {
    let neg = m.is_negative();
    let half = Mbf::parse("0.5", m.is_double()).map_err(|_| fault(OVERFLOW, 6))?;
    let rounded = math(m.abs().add(half))?.fix();
    let signed = if neg { rounded.neg() } else { rounded };
    let f = signed.to_f64();
    if !(-32768.0..=32767.0).contains(&f) {
        return Err(fault(OVERFLOW, 6));
    }
    Ok(f as i16)
}

fn cint_value(v: &Value) -> Result<i16, RunErr> {
    match v {
        Value::Int(i) => Ok(*i),
        Value::Single(m) | Value::Double(m) => cint_mbf(*m),
        Value::Str(_) => Err(fault(TYPE_MISMATCH, 13)),
    }
}

fn bits_of(n: Number) -> Result<i16, RunErr> {
    match n {
        Number::Int(i) => Ok(i),
        Number::Single(m) | Number::Double(m) => cint_mbf(m),
    }
}

fn coerce_value(v: Value, ty: Ty) -> Result<Value, RunErr> {
    match ty {
        Ty::Str => match v {
            Value::Str(s) if s.chars().count() > 255 => Err(fault(STRING_LONG, 15)),
            Value::Str(s) => Ok(Value::Str(s)),
            _ => Err(fault(TYPE_MISMATCH, 13)),
        },
        Ty::Int => Ok(Value::Int(cint_value(&v)?)),
        Ty::Single => Ok(Value::Single(to_mbf(value_num(&v)?, false)?)),
        Ty::Double => Ok(Value::Double(to_mbf(value_num(&v)?, true)?)),
    }
}

fn is_true(v: &Value) -> Result<bool, RunErr> {
    Ok(!num_is_zero(value_num(v)?))
}

fn cmp_numbers(a: Number, b: Number) -> Result<std::cmp::Ordering, RunErr> {
    if let (Number::Int(a), Number::Int(b)) = (a, b) {
        return Ok(a.cmp(&b));
    }
    let (a, b, _) = widen(a, b)?;
    Ok(a.cmp_num(b))
}

fn neg_num(n: Number) -> Result<Number, RunErr> {
    match n {
        Number::Int(i) => i
            .checked_neg()
            .map(Number::Int)
            .ok_or_else(|| fault(OVERFLOW, 6)),
        Number::Single(m) => Ok(Number::Single(m.neg())),
        Number::Double(m) => Ok(Number::Double(m.neg())),
    }
}

fn add_num(a: Number, b: Number) -> Result<Number, RunErr> {
    if let (Number::Int(a), Number::Int(b)) = (a, b) {
        return a
            .checked_add(b)
            .map(Number::Int)
            .ok_or_else(|| fault(OVERFLOW, 6));
    }
    let (a, b, double) = widen(a, b)?;
    Ok(pack_mbf(math(a.add(b))?, double))
}

fn sub_num(a: Number, b: Number) -> Result<Number, RunErr> {
    if let (Number::Int(a), Number::Int(b)) = (a, b) {
        return a
            .checked_sub(b)
            .map(Number::Int)
            .ok_or_else(|| fault(OVERFLOW, 6));
    }
    let (a, b, double) = widen(a, b)?;
    Ok(pack_mbf(math(a.sub(b))?, double))
}

fn mul_num(a: Number, b: Number) -> Result<Number, RunErr> {
    if let (Number::Int(a), Number::Int(b)) = (a, b) {
        return a
            .checked_mul(b)
            .map(Number::Int)
            .ok_or_else(|| fault(OVERFLOW, 6));
    }
    let (a, b, double) = widen(a, b)?;
    Ok(pack_mbf(math(a.mul(b))?, double))
}

fn div_num(a: Number, b: Number) -> Result<Number, RunErr> {
    if num_is_zero(b) {
        return Err(fault(DIV0, 11));
    }
    let double = matches!(a, Number::Double(_)) || matches!(b, Number::Double(_));
    let (a, b, _) = widen(a, b)?;
    Ok(pack_mbf(math(a.div(b))?, double))
}

fn idiv_num(a: Number, b: Number) -> Result<Number, RunErr> {
    let a = bits_of(a)?;
    let b = bits_of(b)?;
    if b == 0 {
        return Err(fault(DIV0, 11));
    }
    let q = i32::from(a) / i32::from(b);
    i16::try_from(q)
        .map(Number::Int)
        .map_err(|_| fault(OVERFLOW, 6))
}

fn mod_num(a: Number, b: Number) -> Result<Number, RunErr> {
    let a = bits_of(a)?;
    let b = bits_of(b)?;
    if b == 0 {
        return Err(fault(DIV0, 11));
    }
    Ok(Number::Int(a % b))
}

fn pow_num(a: Number, b: Number) -> Result<Number, RunErr> {
    let double = matches!(a, Number::Double(_)) || matches!(b, Number::Double(_));
    if let Number::Int(exp) = b {
        let base = to_mbf(a, double)?;
        return Ok(pack_mbf(math(base.powi(i32::from(exp)))?, double));
    }
    let base = to_mbf(a, double)?;
    let exp = to_mbf(b, double)?;
    if base.is_negative() {
        let exp_i = exp.fix();
        if exp.cmp_num(exp_i).is_ne() {
            return Err(fault(ILLEGAL, 5));
        }
    }
    let y = base.to_f64().powf(exp.to_f64());
    if !y.is_finite() {
        return Err(fault(OVERFLOW, 6));
    }
    Ok(pack_mbf(math(Mbf::from_f64(y, double))?, double))
}

fn bit_op(a: Number, b: Number, op: BinOp) -> Result<Number, RunErr> {
    let a = bits_of(a)?;
    let b = bits_of(b)?;
    let r = match op {
        BinOp::And => a & b,
        BinOp::Or => a | b,
        BinOp::Xor => a ^ b,
        BinOp::Eqv => !(a ^ b),
        BinOp::Imp => !a | b,
        _ => return Err(fault(SYNTAX, 2)),
    };
    Ok(Number::Int(r))
}

fn concat(a: &str, b: &str) -> Result<String, RunErr> {
    if a.chars().count() + b.chars().count() > 255 {
        return Err(fault(STRING_LONG, 15));
    }
    Ok(format!("{a}{b}"))
}

fn make_array(dims: Vec<(i16, i16)>, ty: Ty) -> Result<Array, RunErr> {
    let mut count = 1usize;
    for &(lo, hi) in &dims {
        if hi < lo {
            return Err(fault(ILLEGAL, 5));
        }
        let span = (i32::from(hi) - i32::from(lo) + 1) as usize;
        count = count.saturating_mul(span);
        if count > 65535 {
            return Err(fault(OUT_OF_MEMORY, 7));
        }
    }
    Ok(Array {
        dims,
        data: vec![Value::zero(ty); count],
    })
}

fn index_of(dims: &[(i16, i16)], idx: &[i16]) -> Result<usize, ()> {
    if dims.len() != idx.len() {
        return Err(());
    }
    let mut stride = 1usize;
    let mut offset = 0usize;
    for i in (0..dims.len()).rev() {
        let (lo, hi) = dims[i];
        let ix = idx[i];
        if ix < lo || ix > hi {
            return Err(());
        }
        offset += (i32::from(ix) - i32::from(lo)) as usize * stride;
        let span = (i32::from(hi) - i32::from(lo) + 1) as usize;
        stride = stride.saturating_mul(span);
    }
    Ok(offset)
}

fn ensure_array<'a>(vm: &'a mut Vm, name: &Name) -> Result<&'a mut Array, RunErr> {
    if !vm.arrays.contains_key(name) {
        let arr = make_array(vec![(vm.base, 10)], name.ty)?;
        vm.base_locked = true;
        vm.arrays.insert(name.clone(), arr);
    }
    vm.arrays.get_mut(name).ok_or_else(|| fault(SUBSCRIPT, 9))
}

fn load_var(vm: &Vm, name: &Name) -> Value {
    vm.scalars
        .get(name)
        .cloned()
        .unwrap_or_else(|| Value::zero(name.ty))
}

fn store_var(vm: &mut Vm, name: Name, value: Value) -> Result<(), RunErr> {
    let value = coerce_value(value, name.ty)?;
    vm.scalars.insert(name, value);
    Ok(())
}

fn load_elem(vm: &mut Vm, name: &Name, idx: &[i16]) -> Result<Value, RunErr> {
    let arr = ensure_array(vm, name)?;
    let off = index_of(&arr.dims, idx).map_err(|_| fault(SUBSCRIPT, 9))?;
    Ok(arr.data[off].clone())
}

fn store_elem(vm: &mut Vm, name: Name, idx: &[i16], value: Value) -> Result<(), RunErr> {
    let value = coerce_value(value, name.ty)?;
    let arr = ensure_array(vm, &name)?;
    let off = index_of(&arr.dims, idx).map_err(|_| fault(SUBSCRIPT, 9))?;
    arr.data[off] = value;
    Ok(())
}

fn eval_indexes(vm: &mut Vm, host: &mut Host<'_>, exprs: &[Expr]) -> Result<Vec<i16>, RunErr> {
    let mut out = Vec::with_capacity(exprs.len());
    for expr in exprs {
        out.push(cint_value(&eval_expr(vm, host, expr)?)?);
    }
    Ok(out)
}

fn eval_expr(vm: &mut Vm, host: &mut Host<'_>, expr: &Expr) -> Result<Value, RunErr> {
    match expr {
        Expr::Int(n) => Ok(Value::Int(*n)),
        Expr::Single(m) => Ok(Value::Single(*m)),
        Expr::Double(m) => Ok(Value::Double(*m)),
        Expr::Str(s) => Ok(Value::Str(s.clone())),
        Expr::Var(ident) => Ok(load_var(vm, &ident.resolve(&vm.defs))),
        Expr::Index(ident, exprs) => {
            let name = ident.resolve(&vm.defs);
            let idx = eval_indexes(vm, host, exprs)?;
            load_elem(vm, &name, &idx)
        }
        Expr::Unary(UnOp::Neg, inner) => Ok(num_value(neg_num(value_num(&eval_expr(
            vm, host, inner,
        )?)?)?)),
        Expr::Unary(UnOp::Not, inner) => {
            let n = bits_of(value_num(&eval_expr(vm, host, inner)?)?)?;
            Ok(Value::Int(!n))
        }
        Expr::Binary(op, left, right) => eval_binary(vm, host, *op, left, right),
        Expr::Call(func, args) => eval_call(vm, host, *func, args),
        Expr::UserFn(ident, args) => eval_user(vm, host, ident, args),
        Expr::Naked(naked) => eval_naked(vm, host, *naked),
    }
}

fn eval_binary(
    vm: &mut Vm,
    host: &mut Host<'_>,
    op: BinOp,
    left: &Expr,
    right: &Expr,
) -> Result<Value, RunErr> {
    let a = eval_expr(vm, host, left)?;
    let b = eval_expr(vm, host, right)?;
    if matches!(op, BinOp::Add) {
        if let (Value::Str(a), Value::Str(b)) = (&a, &b) {
            return Ok(Value::Str(concat(a, b)?));
        }
    }
    let rel = matches!(
        op,
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge
    );
    if rel {
        if let (Value::Str(a), Value::Str(b)) = (&a, &b) {
            let ord = a.chars().cmp(b.chars());
            let yes = match op {
                BinOp::Eq => ord.is_eq(),
                BinOp::Ne => ord.is_ne(),
                BinOp::Lt => ord.is_lt(),
                BinOp::Gt => ord.is_gt(),
                BinOp::Le => ord.is_le(),
                BinOp::Ge => ord.is_ge(),
                _ => false,
            };
            return Ok(Value::Int(if yes { -1 } else { 0 }));
        }
    }
    let a = value_num(&a)?;
    let b = value_num(&b)?;
    let n = match op {
        BinOp::Add => add_num(a, b)?,
        BinOp::Sub => sub_num(a, b)?,
        BinOp::Mul => mul_num(a, b)?,
        BinOp::Div => div_num(a, b)?,
        BinOp::IDiv => idiv_num(a, b)?,
        BinOp::Mod => mod_num(a, b)?,
        BinOp::Pow => pow_num(a, b)?,
        BinOp::And | BinOp::Or | BinOp::Xor | BinOp::Eqv | BinOp::Imp => bit_op(a, b, op)?,
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
            let ord = cmp_numbers(a, b)?;
            let yes = match op {
                BinOp::Eq => ord.is_eq(),
                BinOp::Ne => ord.is_ne(),
                BinOp::Lt => ord.is_lt(),
                BinOp::Gt => ord.is_gt(),
                BinOp::Le => ord.is_le(),
                BinOp::Ge => ord.is_ge(),
                _ => false,
            };
            Number::Int(if yes { -1 } else { 0 })
        }
    };
    Ok(num_value(n))
}

fn eval_call(vm: &mut Vm, host: &mut Host<'_>, func: Func, args: &[Expr]) -> Result<Value, RunErr> {
    if func == Func::Unsupported {
        return Err(fault(ILLEGAL, 5));
    }
    let mut values = Vec::with_capacity(args.len());
    for arg in args {
        values.push(eval_expr(vm, host, arg)?);
    }
    match func {
        Func::Left => Ok(Value::Str(take_chars(
            &need_str(&values[0])?,
            0,
            need_len(&values[1])?,
        ))),
        Func::Right => {
            let s = need_str(&values[0])?;
            let n = need_len(&values[1])?;
            let skip = s.chars().count().saturating_sub(n);
            Ok(Value::Str(s.chars().skip(skip).collect()))
        }
        Func::Mid => {
            let s = need_str(&values[0])?;
            let start = cint_value(&values[1])?;
            if start < 1 {
                return Err(fault(ILLEGAL, 5));
            }
            let from = (start as usize) - 1;
            let n = if values.len() == 3 {
                need_len(&values[2])?
            } else {
                255
            };
            Ok(Value::Str(take_chars(&s, from, n)))
        }
        Func::Chr => {
            let n = cint_value(&values[0])?;
            if !(0..=255).contains(&n) {
                return Err(fault(ILLEGAL, 5));
            }
            Ok(Value::Str(char::from(n as u8).to_string()))
        }
        Func::StrFn => {
            let m = to_mbf(
                value_num(&values[0])?,
                matches!(values[0], Value::Double(_)),
            )?;
            Ok(Value::Str(m.format_number()))
        }
        Func::StringFn => {
            let n = need_len(&values[0])?;
            let ch = match &values[1] {
                Value::Str(s) => s.chars().next().ok_or_else(|| fault(ILLEGAL, 5))?,
                other => {
                    let code = cint_value(other)?;
                    if !(0..=255).contains(&code) {
                        return Err(fault(ILLEGAL, 5));
                    }
                    char::from(code as u8)
                }
            };
            Ok(Value::Str(std::iter::repeat_n(ch, n).collect()))
        }
        Func::Space => Ok(Value::Str(" ".repeat(need_len(&values[0])?))),
        Func::Hex => Ok(Value::Str(format!(
            "{:X}",
            bits_of(value_num(&values[0])?)? as u16
        ))),
        Func::Oct => Ok(Value::Str(format!(
            "{:o}",
            bits_of(value_num(&values[0])?)? as u16
        ))),
        Func::Sgn => {
            let n = value_num(&values[0])?;
            let s = if num_is_zero(n) {
                0
            } else if step_negative(n) {
                -1
            } else {
                1
            };
            Ok(Value::Int(s))
        }
        Func::Int => {
            let n = value_num(&values[0])?;
            let double = matches!(n, Number::Double(_));
            Ok(num_value(pack_mbf(to_mbf(n, double)?.floor(), double)))
        }
        Func::Abs => Ok(num_value(abs_num(value_num(&values[0])?)?)),
        Func::Fix => {
            let n = value_num(&values[0])?;
            let double = matches!(n, Number::Double(_));
            Ok(num_value(pack_mbf(to_mbf(n, double)?.fix(), double)))
        }
        Func::Sqr | Func::Sin | Func::Log | Func::Exp | Func::Cos | Func::Tan | Func::Atn => {
            let n = value_num(&values[0])?;
            let double = matches!(n, Number::Double(_));
            Ok(num_value(transcend(vm, func, to_mbf(n, double)?)?))
        }
        Func::Rnd => {
            let arg = if values.is_empty() {
                None
            } else {
                Some(to_mbf(value_num(&values[0])?, false)?.to_f64())
            };
            Ok(Value::Single(next_rnd(vm, arg)?))
        }
        Func::Fre => {
            let _ = &values;
            Ok(Value::Single(Mbf::from_i32(vm.reported_free, false)))
        }
        Func::Len => Ok(Value::Int(need_str(&values[0])?.chars().count() as i16)),
        Func::Val => Ok(num_value(val_of(&need_str(&values[0])?))),
        Func::Asc => {
            let s = need_str(&values[0])?;
            let ch = s.chars().next().ok_or_else(|| fault(ILLEGAL, 5))?;
            Ok(Value::Int(ch as i16))
        }
        Func::Instr => {
            let (start, hay, needle) = if values.len() == 3 {
                (
                    cint_value(&values[0])?,
                    need_str(&values[1])?,
                    need_str(&values[2])?,
                )
            } else if matches!(values[0], Value::Str(_)) {
                (1, need_str(&values[0])?, need_str(&values[1])?)
            } else {
                return Err(fault(TYPE_MISMATCH, 13));
            };
            if start < 1 {
                return Err(fault(ILLEGAL, 5));
            }
            Ok(Value::Int(instr(start as usize, &hay, &needle)))
        }
        Func::Cint => Ok(Value::Int(cint_value(&values[0])?)),
        Func::Csng => Ok(Value::Single(to_mbf(value_num(&values[0])?, false)?)),
        Func::Cdbl => Ok(Value::Double(to_mbf(value_num(&values[0])?, true)?)),
        Func::Pos => {
            let _ = value_num(&values[0])?;
            Ok(Value::Int(host.screen.cursor().1 as i16 + 1))
        }
        Func::Peek => {
            if host.program.is_protected() {
                return Err(fault(ILLEGAL, 5));
            }
            let addr = u16_addr(&values[0])?;
            Ok(Value::Int(i16::from(vm.memory[addr as usize])))
        }
        Func::Eof => {
            let n = cint_value(&values[0])?;
            let end = host.files.eof(n).map_err(file_fault)?;
            Ok(Value::Int(if end { -1 } else { 0 }))
        }
        Func::Lof => {
            let n = cint_value(&values[0])?;
            Ok(value_from_i32(host.files.lof(n).map_err(file_fault)?))
        }
        Func::Loc => {
            let n = cint_value(&values[0])?;
            Ok(value_from_i32(host.files.loc(n).map_err(file_fault)?))
        }
        Func::Mki => {
            let n = cint_value(&values[0])?;
            Ok(Value::Str(bytes_string(&n.to_le_bytes())))
        }
        Func::Mks => {
            let m = to_mbf(value_num(&values[0])?, false)?;
            Ok(Value::Str(bytes_string(&m.to_le_single())))
        }
        Func::Mkd => {
            let m = to_mbf(value_num(&values[0])?, true)?;
            Ok(Value::Str(bytes_string(&m.to_le_double())))
        }
        Func::Cvi => {
            let bytes = string_bytes(&values[0])?;
            if bytes.len() < 2 {
                return Err(fault(ILLEGAL, 5));
            }
            Ok(Value::Int(i16::from_le_bytes([bytes[0], bytes[1]])))
        }
        Func::Cvs => {
            let bytes = string_bytes(&values[0])?;
            if bytes.len() < 4 {
                return Err(fault(ILLEGAL, 5));
            }
            let mut raw = [0u8; 4];
            raw.copy_from_slice(&bytes[..4]);
            Ok(Value::Single(Mbf::from_le_single(raw)))
        }
        Func::Cvd => {
            let bytes = string_bytes(&values[0])?;
            if bytes.len() < 8 {
                return Err(fault(ILLEGAL, 5));
            }
            let mut raw = [0u8; 8];
            raw.copy_from_slice(&bytes[..8]);
            Ok(Value::Double(Mbf::from_le_double(raw)))
        }
        Func::Environ => {
            let name = need_str(&values[0])?;
            let value = environ_get(vm, &name);
            if value.chars().count() > 255 {
                return Err(fault(STRING_LONG, 15));
            }
            Ok(Value::Str(value))
        }
        Func::Unsupported => Err(fault(ILLEGAL, 5)),
    }
}

fn need_str(v: &Value) -> Result<String, RunErr> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        _ => Err(fault(TYPE_MISMATCH, 13)),
    }
}

fn need_len(v: &Value) -> Result<usize, RunErr> {
    let n = cint_value(v)?;
    if n < 0 {
        Err(fault(ILLEGAL, 5))
    } else {
        Ok(n as usize)
    }
}

fn take_chars(s: &str, from: usize, n: usize) -> String {
    s.chars().skip(from).take(n).collect()
}

fn instr(start: usize, hay: &str, needle: &str) -> i16 {
    if needle.is_empty() {
        return start as i16;
    }
    let hay: Vec<char> = hay.chars().collect();
    let needle: Vec<char> = needle.chars().collect();
    if start == 0 || start > hay.len() || needle.len() > hay.len() {
        return 0;
    }
    for i in (start - 1)..=hay.len() - needle.len() {
        if hay[i..i + needle.len()] == needle[..] {
            return (i + 1) as i16;
        }
    }
    0
}

fn abs_num(n: Number) -> Result<Number, RunErr> {
    if step_negative(n) {
        neg_num(n)
    } else {
        Ok(n)
    }
}

fn transcend(vm: &Vm, func: Func, m: Mbf) -> Result<Number, RunErr> {
    let x = m.to_f64();
    let y = match func {
        Func::Sin => x.sin(),
        Func::Cos => x.cos(),
        Func::Tan => x.tan(),
        Func::Atn => x.atan(),
        Func::Log => {
            if x <= 0.0 {
                return Err(fault(ILLEGAL, 5));
            }
            x.ln()
        }
        Func::Exp => x.exp(),
        Func::Sqr => {
            if x < 0.0 {
                return Err(fault(ILLEGAL, 5));
            }
            x.sqrt()
        }
        _ => return Err(fault(ILLEGAL, 5)),
    };
    if !y.is_finite() {
        return Err(fault(OVERFLOW, 6));
    }
    let double = vm.double_math || m.is_double();
    Ok(pack_mbf(math(Mbf::from_f64(y, double))?, double))
}

fn next_rnd(vm: &mut Vm, arg: Option<f64>) -> Result<Mbf, RunErr> {
    let n = arg.unwrap_or(1.0);
    if n < 0.0 {
        let bits = (n.to_bits() as u32) & 0x00FF_FFFF;
        vm.seed = if bits == 0 { 1 } else { bits };
    }
    if n != 0.0 {
        vm.seed = vm.seed.wrapping_mul(214013).wrapping_add(2531011) & 0x00FF_FFFF;
        let unit = f64::from(vm.seed) / 16_777_216.0;
        vm.last_rnd = math(Mbf::from_f64(unit, false))?;
    }
    Ok(vm.last_rnd)
}

fn val_of(text: &str) -> Number {
    let bytes = text.trim_start().as_bytes();
    if bytes.is_empty() {
        return Number::Int(0);
    }
    let mut i = 0;
    if matches!(bytes[0], b'+' | b'-') {
        i = 1;
    }
    let mut saw = false;
    let mut dot = false;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || (bytes[i] == b'.' && !dot)) {
        if bytes[i] == b'.' {
            dot = true;
        } else {
            saw = true;
        }
        i += 1;
    }
    let mut exp_d = false;
    if i < bytes.len() && matches!(bytes[i], b'E' | b'e' | b'D' | b'd') {
        exp_d = matches!(bytes[i], b'D' | b'd');
        let mut j = i + 1;
        if j < bytes.len() && matches!(bytes[j], b'+' | b'-') {
            j += 1;
        }
        let digits = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > digits {
            i = j;
        }
    }
    if !saw {
        return Number::Int(0);
    }
    let literal = std::str::from_utf8(&bytes[..i]).unwrap_or("");
    let double = exp_d || literal.bytes().any(|c| c == b'#');
    match Mbf::parse(literal.trim_end_matches(['!', '#']), double) {
        Ok(m) if double => Number::Double(m),
        Ok(m)
            if dot
                || literal
                    .bytes()
                    .any(|c| matches!(c, b'E' | b'e' | b'D' | b'd')) =>
        {
            Number::Single(m)
        }
        Ok(m) => match cint_mbf(m) {
            Ok(n) if !dot => Number::Int(n),
            _ => Number::Single(m),
        },
        Err(_) => Number::Int(0),
    }
}

fn eval_naked(vm: &mut Vm, host: &mut Host<'_>, naked: Naked) -> Result<Value, RunErr> {
    match naked {
        Naked::Inkey => {
            let ch = host.inkey.pop_front();
            Ok(Value::Str(
                ch.map(|b| char::from(b).to_string()).unwrap_or_default(),
            ))
        }
        Naked::Csrlin => Ok(Value::Int(host.screen.cursor().0 as i16 + 1)),
        Naked::Erl => Ok(Value::Single(Mbf::from_i32(vm.erl, false))),
        Naked::Err => Ok(Value::Int(vm.err)),
        Naked::Date => Ok(Value::Str(
            vm.date_override
                .clone()
                .unwrap_or_else(|| "01-01-1980".to_string()),
        )),
        Naked::Time => Ok(Value::Str(
            vm.time_override
                .clone()
                .unwrap_or_else(|| "00:00:00".to_string()),
        )),
    }
}

fn eval_user(
    vm: &mut Vm,
    host: &mut Host<'_>,
    ident: &Ident,
    args: &[Expr],
) -> Result<Value, RunErr> {
    let name = ident.resolve(&vm.defs);
    let func = vm
        .fns
        .get(&name)
        .cloned()
        .ok_or_else(|| fault("Undefined user function", 18))?;
    if func.params.len() != args.len() {
        return Err(fault(ILLEGAL, 5));
    }
    let mut actuals = Vec::with_capacity(args.len());
    for arg in args {
        actuals.push(eval_expr(vm, host, arg)?);
    }
    let mut saved = Vec::new();
    for (param, value) in func.params.iter().zip(actuals) {
        let pname = param.resolve(&vm.defs);
        let old = vm.scalars.remove(&pname);
        let value = coerce_value(value, pname.ty)?;
        vm.scalars.insert(pname.clone(), value);
        saved.push((pname, old));
    }
    let mut parser = Parser::new(&func.body, 0);
    let body = parse_expr(&mut parser)?;
    let result = eval_expr(vm, host, &body);
    for (pname, old) in saved {
        match old {
            Some(v) => {
                vm.scalars.insert(pname, v);
            }
            None => {
                vm.scalars.remove(&pname);
            }
        }
    }
    coerce_value(result?, name.ty)
}

fn parse_print_items(parser: &mut Parser<'_>) -> Result<Vec<PrintItem>, RunErr> {
    let mut items = Vec::new();
    if terminated(&parser.cur) {
        return Ok(items);
    }
    if parser.cur.is("USING") {
        parser.bump();
        let fmt = parse_expr(parser)?;
        if !matches!(parser.cur, Tok::Punct(b';' | b',')) {
            return Err(fault(SYNTAX, 2));
        }
        parser.bump();
        let mut vals = Vec::new();
        let mut sep = Sep::None;
        if !terminated(&parser.cur) {
            loop {
                vals.push(parse_expr(parser)?);
                if matches!(parser.cur, Tok::Punct(b',')) {
                    parser.bump();
                    sep = Sep::Comma;
                    if terminated(&parser.cur) {
                        break;
                    }
                    continue;
                }
                if matches!(parser.cur, Tok::Punct(b';')) {
                    parser.bump();
                    sep = Sep::Semi;
                    if terminated(&parser.cur) {
                        break;
                    }
                    continue;
                }
                sep = Sep::None;
                break;
            }
        }
        items.push(PrintItem {
            part: PrintPart::Using(fmt, vals),
            sep,
        });
        return Ok(items);
    }
    loop {
        if terminated(&parser.cur) {
            break;
        }
        if matches!(parser.cur, Tok::Punct(b',')) {
            parser.bump();
            items.push(PrintItem {
                part: PrintPart::Blank,
                sep: Sep::Comma,
            });
            continue;
        }
        if matches!(parser.cur, Tok::Punct(b';')) {
            parser.bump();
            items.push(PrintItem {
                part: PrintPart::Blank,
                sep: Sep::Semi,
            });
            continue;
        }
        let part = if parser.cur.is("TAB(") {
            parser.bump();
            let expr = parse_expr(parser)?;
            expect_punct(parser, b')')?;
            PrintPart::Tab(expr)
        } else if parser.cur.is("SPC(") {
            parser.bump();
            let expr = parse_expr(parser)?;
            expect_punct(parser, b')')?;
            PrintPart::Spc(expr)
        } else {
            PrintPart::Expr(parse_expr(parser)?)
        };
        let sep = if matches!(parser.cur, Tok::Punct(b',')) {
            parser.bump();
            Sep::Comma
        } else if matches!(parser.cur, Tok::Punct(b';')) {
            parser.bump();
            Sep::Semi
        } else if terminated(&parser.cur) {
            Sep::None
        } else {
            Sep::Semi
        };
        let stop = sep == Sep::None;
        items.push(PrintItem { part, sep });
        if stop {
            break;
        }
    }
    Ok(items)
}

fn do_print(
    vm: &mut Vm,
    host: &mut Host<'_>,
    lpt: bool,
    items: &[PrintItem],
) -> Result<(), RunErr> {
    if items.is_empty() {
        print_text(vm, host, lpt, "\n");
        return Ok(());
    }
    let mut suppress = false;
    for item in items {
        match &item.part {
            PrintPart::Expr(expr) => {
                let value = eval_expr(vm, host, expr)?;
                print_value(vm, host, lpt, &value)?;
            }
            PrintPart::Tab(expr) => {
                let n = i32::from(cint_value(&eval_expr(vm, host, expr)?)?);
                do_tab(vm, host, lpt, n)?;
            }
            PrintPart::Spc(expr) => {
                let n = cint_value(&eval_expr(vm, host, expr)?)?;
                if n < 0 {
                    return Err(fault(ILLEGAL, 5));
                }
                print_text(vm, host, lpt, &" ".repeat(n as usize));
            }
            PrintPart::Blank => {}
            PrintPart::Using(fmt, vals) => {
                let fmt = need_str(&eval_expr(vm, host, fmt)?)?;
                let mut values = Vec::new();
                for v in vals {
                    values.push(eval_expr(vm, host, v)?);
                }
                let text = format_using(&fmt, &values)?;
                print_text(vm, host, lpt, &text);
            }
        }
        match item.sep {
            Sep::Semi => suppress = true,
            Sep::Comma => {
                advance_zone(vm, host, lpt);
                suppress = true;
            }
            Sep::None => suppress = false,
        }
    }
    if !suppress {
        print_text(vm, host, lpt, "\n");
    }
    Ok(())
}

fn print_value(vm: &mut Vm, host: &mut Host<'_>, lpt: bool, value: &Value) -> Result<(), RunErr> {
    match value {
        Value::Str(s) => print_text(vm, host, lpt, s),
        other => {
            let n = value_num(other)?;
            let double = matches!(n, Number::Double(_));
            let mut text = to_mbf(n, double)?.format_number();
            text.push(' ');
            print_text(vm, host, lpt, &text);
        }
    }
    Ok(())
}

fn print_text(vm: &mut Vm, host: &mut Host<'_>, lpt: bool, text: &str) {
    if text.bytes().any(|b| b == 0x07) {
        *host.beep = true;
    }
    if lpt {
        for ch in text.chars() {
            if ch == '\n' {
                host.lpt.push('\n');
                vm.lpt_col = 0;
            } else if ch != '\u{0007}' {
                host.lpt.push(ch);
                vm.lpt_col += 1;
            }
        }
    } else {
        emit(host, text);
    }
}

fn cursor_col(vm: &Vm, host: &Host<'_>, lpt: bool) -> usize {
    if lpt {
        vm.lpt_col
    } else {
        host.screen.cursor().1
    }
}

fn advance_zone(vm: &mut Vm, host: &mut Host<'_>, lpt: bool) {
    let width = host.screen.width();
    let col = cursor_col(vm, host, lpt);
    let next = (col / 14 + 1) * 14;
    if next >= width {
        print_text(vm, host, lpt, "\n");
    } else {
        print_text(vm, host, lpt, &" ".repeat(next - col));
    }
}

fn do_tab(vm: &mut Vm, host: &mut Host<'_>, lpt: bool, n: i32) -> Result<(), RunErr> {
    if n < 1 {
        return Err(fault(ILLEGAL, 5));
    }
    let width = host.screen.width() as i32;
    let target = (n - 1) % width;
    let col = cursor_col(vm, host, lpt) as i32;
    if target < col {
        print_text(vm, host, lpt, "\n");
    }
    let col = cursor_col(vm, host, lpt) as i32;
    if target > col {
        print_text(vm, host, lpt, &" ".repeat((target - col) as usize));
    }
    Ok(())
}

fn format_using(fmt: &str, values: &[Value]) -> Result<String, RunErr> {
    let mut out = String::new();
    let mut rest = values;
    loop {
        let (piece, used) = format_using_once(fmt, rest)?;
        out.push_str(&piece);
        if used == 0 || used >= rest.len() {
            break;
        }
        rest = &rest[used..];
    }
    Ok(out)
}

fn format_using_once(fmt: &str, values: &[Value]) -> Result<(String, usize), RunErr> {
    let bytes = fmt.as_bytes();
    let mut i = 0;
    let mut vi = 0;
    let mut out = String::new();
    while i < bytes.len() {
        if bytes[i] == b'_' && i + 1 < bytes.len() {
            out.push(char::from(bytes[i + 1]));
            i += 2;
            continue;
        }
        if bytes[i] == b'!' {
            out.push(take_str(values, &mut vi)?.chars().next().unwrap_or(' '));
            i += 1;
            continue;
        }
        if bytes[i] == b'&' {
            out.push_str(&take_str(values, &mut vi)?);
            i += 1;
            continue;
        }
        if bytes[i] == b'\\' {
            if let Some(rel) = bytes[i + 1..].iter().position(|c| *c == b'\\') {
                let end = i + 1 + rel;
                let width = end - i + 1;
                out.push_str(&fit_string(&take_str(values, &mut vi)?, width));
                i = end + 1;
                continue;
            }
        }
        if is_num_field(bytes[i]) {
            let start = i;
            while i < bytes.len() && is_num_field(bytes[i]) {
                i += 1;
            }
            let spec = &fmt[start..i];
            if spec.contains('#') || spec.contains("^^^^") {
                out.push_str(&format_field(spec, take_num(values, &mut vi)?)?);
                continue;
            }
            out.push_str(spec);
            continue;
        }
        out.push(char::from(bytes[i]));
        i += 1;
    }
    Ok((out, vi))
}

fn take_str(values: &[Value], vi: &mut usize) -> Result<String, RunErr> {
    let v = values.get(*vi).ok_or_else(|| fault(ILLEGAL, 5))?;
    *vi += 1;
    need_str(v)
}

fn take_num(values: &[Value], vi: &mut usize) -> Result<Number, RunErr> {
    let v = values.get(*vi).ok_or_else(|| fault(ILLEGAL, 5))?;
    *vi += 1;
    value_num(v)
}

fn is_num_field(b: u8) -> bool {
    matches!(b, b'#' | b'.' | b',' | b'+' | b'-' | b'$' | b'*' | b'^')
}

fn fit_string(s: &str, width: usize) -> String {
    let chars: String = s.chars().take(width).collect();
    format!("{chars:<width$}")
}

fn format_field(spec: &str, n: Number) -> Result<String, RunErr> {
    let double = matches!(n, Number::Double(_));
    let value = to_mbf(n, double)?.to_f64();
    if spec.contains("^^^^") {
        let digits = spec.chars().filter(|c| *c == '#').count().max(1);
        let text = format!("{value:.prec$E}", prec = digits.saturating_sub(1));
        let body = if text.len() > spec.len() {
            format!("%{text}")
        } else {
            format!("{text:>width$}", width = spec.len())
        };
        return Ok(body);
    }
    let dot = spec.find('.');
    let after = dot
        .map(|p| spec[p + 1..].chars().filter(|c| *c == '#').count())
        .unwrap_or(0);
    let before = spec[..dot.unwrap_or(spec.len())]
        .chars()
        .filter(|c| *c == '#')
        .count();
    let commas = spec.contains(',');
    let dollar = spec.contains('$');
    let stars = spec.contains('*');
    let plus = spec.starts_with('+');
    let trail = spec.ends_with('-') && !plus;
    let neg = value < 0.0;
    let mag = value.abs();
    let scale = 10f64.powi(after as i32);
    let rounded = (mag * scale).round() / scale;
    let mut digits = format!("{rounded:.after$}");
    if commas {
        if let Some(dot_at) = digits.find('.') {
            let int = &digits[..dot_at];
            let frac = &digits[dot_at..];
            let mut grouped = String::new();
            for (i, ch) in int.chars().rev().enumerate() {
                if i > 0 && i % 3 == 0 {
                    grouped.push(',');
                }
                grouped.push(ch);
            }
            digits = grouped.chars().rev().collect::<String>() + frac;
        }
    }
    let int_digits = digits.find('.').unwrap_or(digits.len());
    let int_part = digits[..int_digits].trim_start_matches('0');
    let int_len = if int_part.is_empty() {
        1
    } else {
        int_part.len()
    };
    if before > 0 && int_len > before {
        return Ok(format!("%{digits}"));
    }
    let mut body = digits;
    if dollar {
        body.insert(0, '$');
    }
    if plus {
        body.insert(0, if neg { '-' } else { '+' });
    } else if !trail && neg {
        body.insert(0, '-');
    }
    if trail {
        body.push(if neg { '-' } else { ' ' });
    }
    let width = spec.chars().count();
    if body.chars().count() > width {
        return Ok(format!("%{body}"));
    }
    if stars {
        Ok(format!("{body:*>width$}"))
    } else {
        Ok(format!("{body:>width$}"))
    }
}

fn here(parser: &Parser<'_>) -> usize {
    if matches!(parser.cur, Tok::End) {
        parser.i
    } else {
        parser.at
    }
}

fn field_fault(msg: &'static str) -> RunErr {
    if msg == REDO {
        fault(SYNTAX, 2)
    } else if msg == STRING_LONG {
        fault(STRING_LONG, 15)
    } else if msg == OVERFLOW {
        fault(OVERFLOW, 6)
    } else if msg == SUBSCRIPT {
        fault(SUBSCRIPT, 9)
    } else {
        fault(SYNTAX, 2)
    }
}

fn ensure_data(vm: &mut Vm, program: &Program) {
    if vm.ip.line.is_none() && vm.data.is_empty() {
        vm.data = collect_data(program);
    }
}

fn parse_target(parser: &mut Parser<'_>) -> Result<Target, RunErr> {
    let ident = take_ident(parser)?;
    if matches!(parser.cur, Tok::Punct(b'(')) {
        Ok(Target::Index(ident, arg_list(parser)?))
    } else {
        Ok(Target::Var(ident))
    }
}

fn target_ty(vm: &Vm, target: &Target) -> Result<Ty, RunErr> {
    match target {
        Target::Var(ident) | Target::Index(ident, _) => Ok(ident.resolve(&vm.defs).ty),
    }
}

fn load_target(vm: &mut Vm, host: &mut Host<'_>, target: &Target) -> Result<Value, RunErr> {
    match target {
        Target::Var(ident) => Ok(load_var(vm, &ident.resolve(&vm.defs))),
        Target::Index(ident, exprs) => {
            let name = ident.resolve(&vm.defs);
            let idx = eval_indexes(vm, host, exprs)?;
            load_elem(vm, &name, &idx)
        }
    }
}

fn store_target(
    vm: &mut Vm,
    host: &mut Host<'_>,
    target: &Target,
    value: Value,
) -> Result<(), RunErr> {
    match target {
        Target::Var(ident) => store_var(vm, ident.resolve(&vm.defs), value),
        Target::Index(ident, exprs) => {
            let name = ident.resolve(&vm.defs);
            let idx = eval_indexes(vm, host, exprs)?;
            store_elem(vm, name, &idx, value)
        }
    }
}

fn walk_checked_until_else(parser: &mut Parser<'_>) -> Result<(), RunErr> {
    loop {
        if matches!(parser.cur, Tok::End) || parser.cur.is("ELSE") {
            return Ok(());
        }
        if matches!(parser.cur, Tok::Colon) {
            parser.bump();
            continue;
        }
        let _ = walk(parser, Mode::Check, None)?;
    }
}

fn exec_sequence_until_else(
    parser: &mut Parser<'_>,
    ctx: &mut (&mut Vm, &mut Host<'_>),
) -> Result<Flow, RunErr> {
    loop {
        if matches!(parser.cur, Tok::End) || parser.cur.is("ELSE") {
            return Ok(Flow::Next);
        }
        if matches!(parser.cur, Tok::Colon) {
            parser.bump();
            continue;
        }
        match walk(parser, Mode::Run, Some((&mut *ctx.0, &mut *ctx.1)))? {
            Flow::Next => {}
            other => return Ok(other),
        }
    }
}

fn exec_if(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let cond = parse_expr(parser)?;
    if parser.cur.is("THEN") || parser.cur.is("GOTO") {
        parser.bump();
    } else {
        return Err(fault(SYNTAX, 2));
    }
    if mode == Mode::Check {
        if is_line_ref(&parser.cur) {
            let _ = parse_line_ref(parser)?;
            // `ELSE` is stored as colon + ELSE. That colon is not a statement break.
            if matches!(parser.cur, Tok::Colon) {
                let look = Parser::new(parser.toks, parser.i);
                if look.cur.is("ELSE") {
                    parser.bump();
                }
            }
        } else {
            walk_checked_until_else(parser)?;
        }
        if parser.cur.is("ELSE") {
            parser.bump();
            if is_line_ref(&parser.cur) {
                let _ = parse_line_ref(parser)?;
            } else {
                walk_checked_until_else(parser)?;
            }
        }
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let truth = is_true(&eval_expr(ctx.0, ctx.1, &cond)?)?;
    if is_line_ref(&parser.cur) {
        let dest = parse_line_ref(parser)?;
        if truth {
            return Ok(Flow::Jump(resolve_line(ctx.0, ctx.1.program, dest)?));
        }
        skip_until_else(parser, true);
    } else if truth {
        return Ok(Flow::Next);
    } else {
        skip_until_else(parser, true);
    }
    if is_line_ref(&parser.cur) {
        let dest = parse_line_ref(parser)?;
        return Ok(Flow::Jump(resolve_line(ctx.0, ctx.1.program, dest)?));
    }
    exec_sequence_until_else(parser, ctx)
}

fn exec_for(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let ident = take_ident(parser)?;
    expect_word(parser, "=")?;
    let start = parse_expr(parser)?;
    expect_word(parser, "TO")?;
    let limit = parse_expr(parser)?;
    let step = if parser.cur.is("STEP") {
        parser.bump();
        parse_expr(parser)?
    } else {
        Expr::Int(1)
    };
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let name = ident.resolve(&ctx.0.defs);
    let start = eval_expr(ctx.0, ctx.1, &start)?;
    store_var(ctx.0, name.clone(), start)?;
    let limit = value_num(&eval_expr(ctx.0, ctx.1, &limit)?)?;
    let step = value_num(&eval_expr(ctx.0, ctx.1, &step)?)?;
    if let Some(pos) = ctx.0.fors.iter().position(|f| f.var == name) {
        ctx.0.fors.truncate(pos);
    }
    ctx.0.fors.push(ForFrame {
        var: name,
        limit,
        step,
        body: Ip {
            line: ctx.0.current_line,
            offset: here(parser),
        },
    });
    Ok(Flow::Next)
}

fn exec_next(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let mut idents = Vec::new();
    if matches!(parser.cur, Tok::Ident(_)) {
        loop {
            idents.push(take_ident(parser)?);
            if matches!(parser.cur, Tok::Punct(b',')) {
                parser.bump();
                continue;
            }
            break;
        }
    }
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    if idents.is_empty() {
        return do_next(ctx.0, None);
    }
    let mut flow = Flow::Next;
    for ident in idents {
        let name = ident.resolve(&ctx.0.defs);
        flow = do_next(ctx.0, Some(name))?;
        if matches!(flow, Flow::Jump(_)) {
            return Ok(flow);
        }
    }
    Ok(flow)
}

fn do_next(vm: &mut Vm, wanted: Option<Name>) -> Result<Flow, RunErr> {
    let pos = if let Some(name) = &wanted {
        vm.fors
            .iter()
            .rposition(|frame| &frame.var == name)
            .ok_or_else(|| fault(NEXT_WITHOUT, 1))?
    } else if vm.fors.is_empty() {
        return Err(fault(NEXT_WITHOUT, 1));
    } else {
        vm.fors.len() - 1
    };
    vm.fors.truncate(pos + 1);
    let frame = vm
        .fors
        .last()
        .cloned()
        .ok_or_else(|| fault(NEXT_WITHOUT, 1))?;
    let current = value_num(&load_var(vm, &frame.var))?;
    let sum = add_num(current, frame.step)?;
    let stored = coerce_value(num_value(sum), frame.var.ty)?;
    let compare = value_num(&stored)?;
    store_var(vm, frame.var.clone(), stored)?;
    let ord = cmp_numbers(compare, frame.limit)?;
    let keep = if step_negative(frame.step) {
        ord.is_ge()
    } else {
        ord.is_le()
    };
    if keep {
        Ok(Flow::Jump(frame.body))
    } else {
        vm.fors.pop();
        Ok(Flow::Next)
    }
}

fn exec_while(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let cond = parse_expr(parser)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let restart = ctx.0.ip;
    if is_true(&eval_expr(ctx.0, ctx.1, &cond)?)? {
        ctx.0.whiles.push(restart);
        return Ok(Flow::Next);
    }
    let offset = here(parser);
    if let Some(line) = ctx.0.current_line {
        Ok(Flow::Jump(scan_wend(ctx.1.program, line, offset)?))
    } else {
        let at = scan_direct_wend(&ctx.0.direct, offset)?;
        Ok(Flow::Jump(Ip {
            line: None,
            offset: at,
        }))
    }
}

fn scan_wend(program: &Program, mut line: u16, mut offset: usize) -> Result<Ip, RunErr> {
    let mut depth = 0i32;
    loop {
        let code = program
            .code(line)
            .ok_or_else(|| fault(WHILE_WITHOUT, 29))?
            .to_vec();
        let mut i = offset;
        loop {
            match crunch::next_tok(&code, &mut i) {
                Tok::End => break,
                Tok::Word("WHILE") => depth += 1,
                Tok::Word("WEND") => {
                    if depth == 0 {
                        return Ok(Ip {
                            line: Some(line),
                            offset: i,
                        });
                    }
                    depth -= 1;
                }
                Tok::Rem(_) | Tok::Tick(_) => break,
                _ => {}
            }
        }
        match program.next_after(line) {
            Some(next) => {
                line = next;
                offset = 0;
            }
            None => return Err(fault(WHILE_WITHOUT, 29)),
        }
    }
}

fn scan_direct_wend(code: &[u8], offset: usize) -> Result<usize, RunErr> {
    let mut depth = 0i32;
    let mut i = offset;
    loop {
        match crunch::next_tok(code, &mut i) {
            Tok::End | Tok::Rem(_) | Tok::Tick(_) => return Err(fault(WHILE_WITHOUT, 29)),
            Tok::Word("WHILE") => depth += 1,
            Tok::Word("WEND") => {
                if depth == 0 {
                    return Ok(i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
}

fn exec_on(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    if parser.cur.is("ERROR") {
        parser.bump();
        expect_word(parser, "GOTO")?;
        let dest = parse_line_ref(parser)?;
        if mode == Mode::Check {
            return Ok(Flow::Next);
        }
        let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
        ctx.0.on_error = match dest {
            LineRef::Num(0) => None,
            LineRef::Num(n) => Some(n),
            LineRef::Dot => Some(ctx.1.program.last.ok_or_else(|| fault(UNDEF_LINE, 8))?),
        };
        return Ok(Flow::Next);
    }
    let expr = parse_expr(parser)?;
    let gosub = if parser.cur.is("GOSUB") {
        true
    } else if parser.cur.is("GOTO") {
        false
    } else {
        return Err(fault(SYNTAX, 2));
    };
    parser.bump();
    let mut lines = Vec::new();
    loop {
        lines.push(parse_line_ref(parser)?);
        if matches!(parser.cur, Tok::Punct(b',')) {
            parser.bump();
            continue;
        }
        break;
    }
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let n = cint_value(&eval_expr(ctx.0, ctx.1, &expr)?)?;
    if n < 0 {
        return Err(fault(ILLEGAL, 5));
    }
    if n == 0 || n as usize > lines.len() {
        return Ok(Flow::Next);
    }
    let dest = resolve_line(ctx.0, ctx.1.program, lines[(n as usize) - 1])?;
    if gosub {
        ctx.0.gosub.push(Ip {
            line: ctx.0.current_line,
            offset: here(parser),
        });
    }
    Ok(Flow::Jump(dest))
}

fn exec_width(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let expr = parse_expr(parser)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let n = cint_value(&eval_expr(ctx.0, ctx.1, &expr)?)?;
    if n != 40 && n != 80 {
        return Err(fault(ILLEGAL, 5));
    }
    ctx.1.screen.set_width(n as usize);
    *ctx.1.key_offset = 0;
    Ok(Flow::Next)
}

fn parse_slots(parser: &mut Parser<'_>, max: usize) -> Result<Vec<Option<Expr>>, RunErr> {
    let mut slots = Vec::new();
    loop {
        if slots.len() == max {
            return Err(fault(SYNTAX, 2));
        }
        if terminated(&parser.cur) {
            break;
        }
        if matches!(parser.cur, Tok::Punct(b',')) {
            slots.push(None);
            parser.bump();
            continue;
        }
        slots.push(Some(parse_expr(parser)?));
        if matches!(parser.cur, Tok::Punct(b',')) {
            parser.bump();
            continue;
        }
        break;
    }
    if slots.is_empty() {
        return Err(fault(SYNTAX, 2));
    }
    Ok(slots)
}

fn slot_int(
    vm: &mut Vm,
    host: &mut Host<'_>,
    slot: &Option<Expr>,
    default: i16,
) -> Result<i16, RunErr> {
    match slot {
        Some(expr) => cint_value(&eval_expr(vm, host, expr)?),
        None => Ok(default),
    }
}

fn exec_locate(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let slots = parse_slots(parser, 5)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let (row0, col0) = ctx.1.screen.cursor();
    let row = slot_int(
        ctx.0,
        ctx.1,
        slots.first().unwrap_or(&None),
        row0 as i16 + 1,
    )?;
    let col = slot_int(ctx.0, ctx.1, slots.get(1).unwrap_or(&None), col0 as i16 + 1)?;
    let rows = ctx.1.screen.editable_rows() as i16;
    let cols = ctx.1.screen.width() as i16;
    if row < 1 || row > rows || col < 1 || col > cols {
        return Err(fault(ILLEGAL, 5));
    }
    ctx.1
        .screen
        .set_cursor((row - 1) as usize, (col - 1) as usize);
    Ok(Flow::Next)
}

fn exec_color(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let slots = parse_slots(parser, 3)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let attr = ctx.1.screen.attr();
    let fg = slot_int(
        ctx.0,
        ctx.1,
        slots.first().unwrap_or(&None),
        i16::from(attr & 0x0F),
    )?;
    let bg = slot_int(
        ctx.0,
        ctx.1,
        slots.get(1).unwrap_or(&None),
        i16::from(attr >> 4),
    )?;
    if !(0..=15).contains(&fg) || !(0..=15).contains(&bg) {
        return Err(fault(ILLEGAL, 5));
    }
    ctx.1.screen.set_attr(((bg as u8) << 4) | (fg as u8));
    Ok(Flow::Next)
}

fn exec_dim(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let mut specs = Vec::new();
    loop {
        let ident = take_ident(parser)?;
        expect_punct(parser, b'(')?;
        let mut dims = Vec::new();
        loop {
            let first = parse_expr(parser)?;
            if parser.cur.is("TO") {
                parser.bump();
                dims.push((Some(first), parse_expr(parser)?));
            } else {
                dims.push((None, first));
            }
            if matches!(parser.cur, Tok::Punct(b',')) {
                parser.bump();
                continue;
            }
            break;
        }
        expect_punct(parser, b')')?;
        specs.push((ident, dims));
        if matches!(parser.cur, Tok::Punct(b',')) {
            parser.bump();
            continue;
        }
        break;
    }
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    for (ident, dims) in specs {
        let name = ident.resolve(&ctx.0.defs);
        if ctx.0.arrays.contains_key(&name) {
            return Err(fault(DUPLICATE, 10));
        }
        let mut bounds = Vec::new();
        for (lo, hi) in dims {
            let hi = cint_value(&eval_expr(ctx.0, ctx.1, &hi)?)?;
            let lo = if let Some(lo) = lo {
                cint_value(&eval_expr(ctx.0, ctx.1, &lo)?)?
            } else {
                ctx.0.base
            };
            if hi < lo {
                return Err(fault(ILLEGAL, 5));
            }
            bounds.push((lo, hi));
        }
        let arr = make_array(bounds, name.ty)?;
        ctx.0.base_locked = true;
        ctx.0.arrays.insert(name, arr);
    }
    Ok(Flow::Next)
}

fn exec_erase(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let mut idents = Vec::new();
    loop {
        idents.push(take_ident(parser)?);
        if matches!(parser.cur, Tok::Punct(b',')) {
            parser.bump();
            continue;
        }
        break;
    }
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    for ident in idents {
        let name = ident.resolve(&ctx.0.defs);
        if ctx.0.arrays.remove(&name).is_none() {
            return Err(fault(ILLEGAL, 5));
        }
    }
    Ok(Flow::Next)
}

fn letter_of(ident: &Ident) -> Result<u8, RunErr> {
    let b = ident.stem.as_bytes().first().copied().unwrap_or(0);
    if b.is_ascii_alphabetic() {
        Ok(b.to_ascii_uppercase())
    } else {
        Err(fault(SYNTAX, 2))
    }
}

fn exec_def(
    parser: &mut Parser<'_>,
    ty: Ty,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let mut ranges = Vec::new();
    loop {
        let start = letter_of(&take_ident(parser)?)?;
        let end = if parser.cur.is("-") {
            parser.bump();
            letter_of(&take_ident(parser)?)?
        } else {
            start
        };
        if end < start {
            return Err(fault(SYNTAX, 2));
        }
        ranges.push((start, end));
        if matches!(parser.cur, Tok::Punct(b',')) {
            parser.bump();
            continue;
        }
        break;
    }
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    for (start, end) in ranges {
        for ch in start..=end {
            ctx.0.defs[usize::from(ch - b'A')] = ty;
        }
    }
    Ok(Flow::Next)
}

fn exec_def_fn(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    expect_word(parser, "FN")?;
    let ident = take_ident(parser)?;
    let mut params = Vec::new();
    if matches!(parser.cur, Tok::Punct(b'(')) {
        parser.bump();
        if !matches!(parser.cur, Tok::Punct(b')')) {
            loop {
                params.push(take_ident(parser)?);
                if matches!(parser.cur, Tok::Punct(b',')) {
                    parser.bump();
                    continue;
                }
                break;
            }
        }
        expect_punct(parser, b')')?;
    }
    expect_word(parser, "=")?;
    let start = parser.at;
    let _ = parse_expr(parser)?;
    let end = parser.at;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let mut body = parser.toks[start..end].to_vec();
    body.push(0);
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let name = ident.resolve(&ctx.0.defs);
    ctx.0.fns.insert(name, UserFn { params, body });
    Ok(Flow::Next)
}

fn exec_swap(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let left = parse_target(parser)?;
    expect_punct(parser, b',')?;
    let right = parse_target(parser)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let a = load_target(ctx.0, ctx.1, &left)?;
    let b = load_target(ctx.0, ctx.1, &right)?;
    if a.ty() != b.ty() {
        return Err(fault(TYPE_MISMATCH, 13));
    }
    store_target(ctx.0, ctx.1, &left, b)?;
    store_target(ctx.0, ctx.1, &right, a)?;
    Ok(Flow::Next)
}

fn exec_input(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
    line_input: bool,
) -> Result<Flow, RunErr> {
    parser.bump();
    if matches!(parser.cur, Tok::Punct(b'#')) {
        return exec_file_input(parser, mode, ctx, line_input);
    }
    if !line_input && matches!(parser.cur, Tok::Punct(b';')) {
        parser.bump();
    }
    let mut prompt = String::new();
    let mut question = !line_input;
    if let Tok::Str(bytes) = &parser.cur {
        prompt = bytes_string(bytes);
        parser.bump();
        if matches!(parser.cur, Tok::Punct(b';')) {
            parser.bump();
        } else if matches!(parser.cur, Tok::Punct(b',')) {
            parser.bump();
            question = false;
        } else {
            return Err(fault(SYNTAX, 2));
        }
    }
    if question {
        prompt.push_str("? ");
    }
    let mut targets = Vec::new();
    loop {
        targets.push(parse_target(parser)?);
        if matches!(parser.cur, Tok::Punct(b',')) {
            parser.bump();
            continue;
        }
        break;
    }
    if line_input && targets.len() != 1 {
        return Err(fault(SYNTAX, 2));
    }
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let mut slots = Vec::new();
    for target in targets {
        match target {
            Target::Var(ident) => {
                let name = ident.resolve(&ctx.0.defs);
                if line_input && name.ty != Ty::Str {
                    return Err(fault(TYPE_MISMATCH, 13));
                }
                slots.push(InSlot::Var(name));
            }
            Target::Index(ident, exprs) => {
                if line_input {
                    return Err(fault(TYPE_MISMATCH, 13));
                }
                let name = ident.resolve(&ctx.0.defs);
                let idx = eval_indexes(ctx.0, ctx.1, &exprs)?;
                let _ = ensure_array(ctx.0, &name)?;
                slots.push(InSlot::Elem(name, idx));
            }
        }
    }
    ctx.0.pending = Some(Pending {
        slots,
        line_input,
        prompt,
    });
    ctx.0.phase = Phase::Input;
    Ok(Flow::Input)
}

fn exec_read(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let mut targets = Vec::new();
    loop {
        targets.push(parse_target(parser)?);
        if matches!(parser.cur, Tok::Punct(b',')) {
            parser.bump();
            continue;
        }
        break;
    }
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    ensure_data(ctx.0, ctx.1.program);
    for target in &targets {
        if ctx.0.data_at >= ctx.0.data.len() {
            return Err(fault(OUT_OF_DATA, 4));
        }
        let text = ctx.0.data[ctx.0.data_at].text.clone();
        ctx.0.data_at += 1;
        let ty = target_ty(ctx.0, target)?;
        let value = coerce_field(&text, ty).map_err(field_fault)?;
        store_target(ctx.0, ctx.1, target, value)?;
    }
    Ok(Flow::Next)
}

fn exec_restore(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let dest = if is_line_ref(&parser.cur) {
        Some(parse_line_ref(parser)?)
    } else if terminated(&parser.cur) {
        None
    } else {
        return Err(fault(SYNTAX, 2));
    };
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    ensure_data(ctx.0, ctx.1.program);
    ctx.0.data_at = if let Some(dest) = dest {
        let ip = resolve_line(ctx.0, ctx.1.program, dest)?;
        let line = ip.line.unwrap_or(0);
        ctx.0
            .data
            .iter()
            .position(|item| item.line >= line)
            .unwrap_or(ctx.0.data.len())
    } else {
        0
    };
    Ok(Flow::Next)
}

fn exec_clear(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    if !terminated(&parser.cur) {
        let _ = parse_expr(parser)?;
        if matches!(parser.cur, Tok::Punct(b',')) {
            parser.bump();
            if !terminated(&parser.cur) {
                let _ = parse_expr(parser)?;
            }
        }
    }
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    ctx.0.clear_vars();
    Ok(Flow::Next)
}

fn exec_option(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let name = if let Tok::Ident(name) = &parser.cur {
        name.clone()
    } else {
        return Err(fault(SYNTAX, 2));
    };
    parser.bump();
    if name != "BASE" {
        return Err(fault(SYNTAX, 2));
    }
    let base = match parser.cur {
        Tok::Int(0) => 0,
        Tok::Int(1) => 1,
        _ => return Err(fault(SYNTAX, 2)),
    };
    parser.bump();
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    if ctx.0.base_locked {
        return Err(fault(DUPLICATE, 10));
    }
    ctx.0.base = base;
    ctx.0.base_locked = true;
    Ok(Flow::Next)
}

fn exec_randomize(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let expr = if terminated(&parser.cur) {
        None
    } else {
        Some(parse_expr(parser)?)
    };
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let seed = if let Some(expr) = expr {
        bits_of(value_num(&eval_expr(ctx.0, ctx.1, &expr)?)?)? as u32
    } else {
        1
    };
    ctx.0.seed = if seed == 0 { 1 } else { seed & 0x00FF_FFFF };
    Ok(Flow::Next)
}

fn exec_run(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    if !is_line_ref(&parser.cur) && !terminated(&parser.cur) {
        skip_statement(parser);
        return if mode == Mode::Run {
            Err(fault(ILLEGAL, 5))
        } else {
            Ok(Flow::Next)
        };
    }
    let dest = if is_line_ref(&parser.cur) {
        Some(parse_line_ref(parser)?)
    } else {
        None
    };
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let start = match dest {
        None => None,
        Some(LineRef::Num(n)) => Some(n),
        Some(LineRef::Dot) => Some(ctx.1.program.last.ok_or_else(|| fault(UNDEF_LINE, 8))?),
    };
    if !begin_program(ctx.0, ctx.1.program, start) {
        return Ok(Flow::Ended);
    }
    Ok(Flow::Jump(ctx.0.ip))
}

fn assign_mid(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    expect_punct(parser, b'(')?;
    let dest = parse_target(parser)?;
    expect_punct(parser, b',')?;
    let start = parse_expr(parser)?;
    let len = if matches!(parser.cur, Tok::Punct(b',')) {
        parser.bump();
        Some(parse_expr(parser)?)
    } else {
        None
    };
    expect_punct(parser, b')')?;
    expect_word(parser, "=")?;
    let value = parse_expr(parser)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let start = cint_value(&eval_expr(ctx.0, ctx.1, &start)?)?;
    if start < 1 {
        return Err(fault(ILLEGAL, 5));
    }
    let repl = need_str(&eval_expr(ctx.0, ctx.1, &value)?)?;
    let take = if let Some(len) = len {
        need_len(&eval_expr(ctx.0, ctx.1, &len)?)?
    } else {
        repl.chars().count()
    };
    let current = need_str(&load_target(ctx.0, ctx.1, &dest)?)?;
    let updated = replace_mid(&current, (start as usize) - 1, take, &repl);
    store_target(ctx.0, ctx.1, &dest, Value::Str(updated))?;
    Ok(Flow::Next)
}

fn replace_mid(text: &str, start: usize, take: usize, repl: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    if start >= chars.len() || take == 0 {
        return text.to_string();
    }
    let rep: Vec<char> = repl.chars().collect();
    let n = take.min(rep.len()).min(chars.len() - start);
    for (slot, ch) in chars.iter_mut().skip(start).take(n).zip(rep) {
        *slot = ch;
    }
    chars.into_iter().collect()
}

fn assign_clock(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
    date: bool,
) -> Result<Flow, RunErr> {
    parser.bump();
    expect_word(parser, "=")?;
    let expr = parse_expr(parser)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let Value::Str(text) = eval_expr(ctx.0, ctx.1, &expr)? else {
        return Err(fault(TYPE_MISMATCH, 13));
    };
    if text.chars().count() > 255 {
        return Err(fault(STRING_LONG, 15));
    }
    if date {
        ctx.0.date_override = Some(text);
    } else {
        ctx.0.time_override = Some(text);
    }
    Ok(Flow::Next)
}

fn name_is(tok: &Tok, name: &str) -> bool {
    match tok {
        Tok::Word(word) => word.eq_ignore_ascii_case(name),
        Tok::Ident(ident) => ident.eq_ignore_ascii_case(name),
        _ => false,
    }
}

/// `FOR OUTPUT AS` crunches `OUTPUT` and `AS` as plain letters. A file that
/// dropped the separating space stores them as one name, `OUTPUTAS`.
fn take_open_mode(parser: &mut Parser<'_>) -> Result<(String, bool), RunErr> {
    let upper = take_name(parser)?.to_ascii_uppercase();
    for mode in ["OUTPUT", "APPEND", "RANDOM", "INPUT"] {
        let Some(rest) = upper.strip_prefix(mode) else {
            continue;
        };
        if rest.is_empty() {
            return Ok((mode.to_string(), false));
        }
        if rest == "AS" {
            return Ok((mode.to_string(), true));
        }
    }
    Err(fault(SYNTAX, 2))
}

fn take_name(parser: &mut Parser<'_>) -> Result<String, RunErr> {
    if !matches!(parser.cur, Tok::Word(_) | Tok::Ident(_)) {
        return Err(fault(SYNTAX, 2));
    }
    let name = match &parser.cur {
        Tok::Word(word) => (*word).to_string(),
        Tok::Ident(ident) => ident.clone(),
        _ => return Err(fault(SYNTAX, 2)),
    };
    parser.bump();
    Ok(name)
}

fn expect_name(parser: &mut Parser<'_>, name: &str) -> Result<(), RunErr> {
    if name_is(&parser.cur, name) {
        parser.bump();
        Ok(())
    } else {
        Err(fault(SYNTAX, 2))
    }
}

fn file_fault(message: &'static str) -> RunErr {
    let code = match message {
        files::BAD_FILE_NUMBER => 52,
        files::FILE_NOT_FOUND => 53,
        files::BAD_FILE_MODE => 54,
        files::FILE_ALREADY_OPEN => 55,
        files::FILE_ALREADY_EXISTS => 58,
        files::INPUT_PAST_END => 62,
        files::BAD_FILE_NAME => 64,
        files::PERMISSION_DENIED => 70,
        files::PATH_NOT_FOUND => 76,
        files::OUT_OF_MEMORY => 7,
        _ => 75,
    };
    fault(message, code)
}

fn value_from_i32(n: i32) -> Value {
    if (-32768..=32767).contains(&n) {
        Value::Int(n as i16)
    } else {
        Value::Single(Mbf::from_i32(n, false))
    }
}

fn u16_addr(value: &Value) -> Result<u16, RunErr> {
    let number = to_mbf(value_num(value)?, true)?;
    let rounded = number.to_f64().round();
    if !(0.0..=65535.0).contains(&rounded) {
        return Err(fault(ILLEGAL, 5));
    }
    Ok(rounded as u16)
}

fn byte_of(value: &Value) -> Result<u8, RunErr> {
    let n = u16_addr(value)?;
    if n > 255 {
        Err(fault(ILLEGAL, 5))
    } else {
        Ok(n as u8)
    }
}

fn message_for(code: i16) -> &'static str {
    match code {
        1 => NEXT_WITHOUT,
        2 => SYNTAX,
        3 => RETURN_WITHOUT,
        4 => OUT_OF_DATA,
        5 => ILLEGAL,
        6 => OVERFLOW,
        7 => OUT_OF_MEMORY,
        8 => UNDEF_LINE,
        9 => SUBSCRIPT,
        11 => DIV0,
        13 => TYPE_MISMATCH,
        15 => STRING_LONG,
        17 => "Can't continue",
        18 => "Undefined user function",
        20 => RESUME_WITHOUT,
        22 => MISSING,
        29 => WHILE_WITHOUT,
        30 => WEND_WITHOUT,
        52 => files::BAD_FILE_NUMBER,
        53 => files::FILE_NOT_FOUND,
        54 => files::BAD_FILE_MODE,
        55 => files::FILE_ALREADY_OPEN,
        58 => files::FILE_ALREADY_EXISTS,
        62 => files::INPUT_PAST_END,
        64 => files::BAD_FILE_NAME,
        70 => files::PERMISSION_DENIED,
        75 => files::PATH_FILE_ACCESS,
        76 => files::PATH_NOT_FOUND,
        _ => UNPRINTABLE,
    }
}

fn value_text(value: &Value) -> Result<String, RunErr> {
    match value {
        Value::Str(text) => Ok(text.clone()),
        other => {
            let number = value_num(other)?;
            let double = matches!(number, Number::Double(_));
            let mut text = to_mbf(number, double)?.format_number();
            text.push(' ');
            Ok(text)
        }
    }
}

fn push_text(out: &mut String, col: &mut usize, text: &str) {
    for ch in text.chars() {
        if ch == '\n' {
            out.push('\n');
            *col = 0;
        } else {
            out.push(ch);
            *col += 1;
        }
    }
}

fn format_print(vm: &mut Vm, host: &mut Host<'_>, items: &[PrintItem]) -> Result<String, RunErr> {
    if items.is_empty() {
        return Ok("\n".to_string());
    }
    let width = host.screen.width();
    let mut out = String::new();
    let mut col = 0usize;
    let mut suppress = false;
    for item in items {
        match &item.part {
            PrintPart::Expr(expr) => {
                let text = value_text(&eval_expr(vm, host, expr)?)?;
                push_text(&mut out, &mut col, &text);
            }
            PrintPart::Tab(expr) => {
                let n = i32::from(cint_value(&eval_expr(vm, host, expr)?)?);
                if n < 1 {
                    return Err(fault(ILLEGAL, 5));
                }
                let target = (n as usize).saturating_sub(1);
                if col > target {
                    push_text(&mut out, &mut col, "\n");
                }
                let pad = target.saturating_sub(col);
                push_text(&mut out, &mut col, &" ".repeat(pad));
            }
            PrintPart::Spc(expr) => {
                let n = cint_value(&eval_expr(vm, host, expr)?)?;
                if n < 0 {
                    return Err(fault(ILLEGAL, 5));
                }
                push_text(&mut out, &mut col, &" ".repeat(n as usize));
            }
            PrintPart::Blank => {}
            PrintPart::Using(fmt, vals) => {
                let fmt = need_str(&eval_expr(vm, host, fmt)?)?;
                let mut values = Vec::new();
                for value in vals {
                    values.push(eval_expr(vm, host, value)?);
                }
                let text = format_using(&fmt, &values)?;
                push_text(&mut out, &mut col, &text);
            }
        }
        match item.sep {
            Sep::Semi => suppress = true,
            Sep::Comma => {
                let next = (col / 14 + 1) * 14;
                if next >= width {
                    push_text(&mut out, &mut col, "\n");
                } else {
                    let spaces = " ".repeat(next - col);
                    push_text(&mut out, &mut col, &spaces);
                }
                suppress = true;
            }
            Sep::None => suppress = false,
        }
    }
    if !suppress {
        push_text(&mut out, &mut col, "\n");
    }
    Ok(out)
}

fn format_write(values: &[Value]) -> Result<String, RunErr> {
    let mut parts = Vec::new();
    for value in values {
        match value {
            Value::Str(text) => parts.push(format!("\"{text}\"")),
            other => {
                let number = value_num(other)?;
                let double = matches!(number, Number::Double(_));
                let mut text = to_mbf(number, double)?.format_number();
                if text.starts_with(' ') {
                    text.remove(0);
                }
                parts.push(text);
            }
        }
    }
    let mut line = parts.join(",");
    line.push('\n');
    Ok(line)
}

pub fn load_bytes(program: &mut Program, bytes: &[u8]) -> Result<(), &'static str> {
    if matches!(bytes.first(), Some(0xFF | 0xFE)) {
        let decoded = files::decode_tokenized(bytes)?;
        program.clear();
        for (number, tokens) in decoded.lines {
            let listed = crunch::list_tokens(&tokens);
            program.insert(number, tokens, format!("{number} {listed}"));
        }
        program.set_protected(decoded.protected);
        Ok(())
    } else {
        load_ascii(program, &files::decode_ascii(bytes), true)
    }
}

fn load_ascii(program: &mut Program, text: &str, replace: bool) -> Result<(), &'static str> {
    if replace {
        program.clear();
    }
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }
        let (number, rest) = numbered_source(line).map_err(|err| err.message)?;
        store_source_line(program, number, rest).map_err(|fault| match fault.kind {
            crate::crunch::FaultKind::Overflow => OVERFLOW,
            crate::crunch::FaultKind::Syntax => SYNTAX,
        })?;
    }
    Ok(())
}

fn numbered_source(text: &str) -> Result<(u16, &str), RunErr> {
    let bytes = text.as_bytes();
    if !bytes.first().is_some_and(|b| b.is_ascii_digit()) {
        return Err(fault(SYNTAX, 2));
    }
    let mut index = 0;
    let mut number: u32 = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        number = number
            .saturating_mul(10)
            .saturating_add(u32::from(bytes[index] - b'0'));
        if number > 65529 {
            return Err(fault(SYNTAX, 2));
        }
        index += 1;
    }
    Ok((number as u16, &text[index..]))
}

fn eval_string(vm: &mut Vm, host: &mut Host<'_>, expr: &Expr) -> Result<String, RunErr> {
    need_str(&eval_expr(vm, host, expr)?)
}

fn file_number(parser: &mut Parser<'_>) -> Result<Expr, RunErr> {
    if matches!(parser.cur, Tok::Punct(b'#')) {
        parser.bump();
    }
    parse_expr(parser)
}

fn exec_file_input(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
    line_input: bool,
) -> Result<Flow, RunErr> {
    parser.bump();
    let number = file_number(parser)?;
    expect_punct(parser, b',')?;
    let mut targets = Vec::new();
    loop {
        targets.push(parse_target(parser)?);
        if matches!(parser.cur, Tok::Punct(b',')) {
            if line_input {
                return Err(fault(SYNTAX, 2));
            }
            parser.bump();
            continue;
        }
        break;
    }
    if targets.is_empty() {
        return Err(fault(SYNTAX, 2));
    }
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let number = cint_value(&eval_expr(ctx.0, ctx.1, &number)?)?;
    for target in targets {
        let text = if line_input {
            ctx.1.files.read_line(number)
        } else {
            ctx.1.files.read_field(number)
        }
        .map_err(file_fault)?;
        let ty = target_ty(ctx.0, &target)?;
        let value = if line_input {
            if ty != Ty::Str {
                return Err(fault(TYPE_MISMATCH, 13));
            }
            if text.len() > 255 {
                return Err(fault(STRING_LONG, 15));
            }
            Value::Str(text)
        } else {
            coerce_field(&text, ty).map_err(field_fault)?
        };
        store_target(ctx.0, ctx.1, &target, value)?;
    }
    Ok(Flow::Next)
}

fn exec_write(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let channel = if matches!(parser.cur, Tok::Punct(b'#')) {
        parser.bump();
        let number = parse_expr(parser)?;
        expect_punct(parser, b',')?;
        Some(number)
    } else {
        None
    };
    let mut exprs = Vec::new();
    if !terminated(&parser.cur) {
        loop {
            exprs.push(parse_expr(parser)?);
            if matches!(parser.cur, Tok::Punct(b',')) {
                parser.bump();
                if terminated(&parser.cur) {
                    break;
                }
                continue;
            }
            break;
        }
    }
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let mut values = Vec::new();
    for expr in &exprs {
        values.push(eval_expr(ctx.0, ctx.1, expr)?);
    }
    let text = format_write(&values)?;
    if let Some(number) = channel {
        let number = cint_value(&eval_expr(ctx.0, ctx.1, &number)?)?;
        ctx.1.files.write_text(number, &text).map_err(file_fault)?;
    } else {
        emit(ctx.1, &text);
    }
    Ok(Flow::Next)
}

fn exec_open(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let first = parse_expr(parser)?;
    let (filename, mode_expr, mode_name, number, words) = if parser.cur.is("FOR") {
        parser.bump();
        let (mode_name, as_taken) = take_open_mode(parser)?;
        if !as_taken {
            expect_name(parser, "AS")?;
        }
        let number = file_number(parser)?;
        if name_is(&parser.cur, "LEN") {
            parser.bump();
            expect_word(parser, "=")?;
            let _ = parse_expr(parser)?;
        }
        (first, None, mode_name, number, true)
    } else {
        expect_punct(parser, b',')?;
        let number = file_number(parser)?;
        expect_punct(parser, b',')?;
        let filename = parse_expr(parser)?;
        if matches!(parser.cur, Tok::Punct(b',')) {
            parser.bump();
            let _ = parse_expr(parser)?;
        }
        (filename, Some(first), String::new(), number, false)
    };
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let filename = eval_string(ctx.0, ctx.1, &filename)?;
    let number = cint_value(&eval_expr(ctx.0, ctx.1, &number)?)?;
    let seq = if words {
        match mode_name.to_ascii_uppercase().as_str() {
            "INPUT" => SeqMode::Input,
            "OUTPUT" => SeqMode::Output,
            "APPEND" => SeqMode::Append,
            "RANDOM" => return Err(fault(files::BAD_FILE_MODE, 54)),
            _ => return Err(fault(SYNTAX, 2)),
        }
    } else {
        let mode_text = eval_string(ctx.0, ctx.1, mode_expr.as_ref().unwrap())?;
        match mode_text.chars().next().map(|ch| ch.to_ascii_uppercase()) {
            Some('I') => SeqMode::Input,
            Some('O') => SeqMode::Output,
            Some('A') => SeqMode::Append,
            Some('R') => return Err(fault(files::BAD_FILE_MODE, 54)),
            _ => return Err(fault(files::BAD_FILE_MODE, 54)),
        }
    };
    ctx.1
        .files
        .open(number, &filename, seq)
        .map_err(file_fault)?;
    Ok(Flow::Next)
}

fn exec_close(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let mut numbers = Vec::new();
    if !terminated(&parser.cur) {
        loop {
            numbers.push(file_number(parser)?);
            if matches!(parser.cur, Tok::Punct(b',')) {
                parser.bump();
                continue;
            }
            break;
        }
    }
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    if numbers.is_empty() {
        ctx.1.files.close_all();
        return Ok(Flow::Next);
    }
    for number in numbers {
        let number = cint_value(&eval_expr(ctx.0, ctx.1, &number)?)?;
        ctx.1.files.close(number).map_err(file_fault)?;
    }
    Ok(Flow::Next)
}

fn exec_save(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let name = parse_expr(parser)?;
    let kind = if matches!(parser.cur, Tok::Punct(b',')) {
        parser.bump();
        match take_name(parser)?.to_ascii_uppercase().as_str() {
            "A" => SaveKind::Ascii,
            "P" => SaveKind::Protected,
            _ => return Err(fault(SYNTAX, 2)),
        }
    } else {
        SaveKind::Tokenized
    };
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    if ctx.1.program.is_protected() && kind != SaveKind::Protected {
        return Err(fault(ILLEGAL, 5));
    }
    let name = eval_string(ctx.0, ctx.1, &name)?;
    let path = files::bas_path(&name).map_err(file_fault)?;
    let bytes = match kind {
        SaveKind::Ascii => files::encode_ascii(&ctx.1.program.listing()),
        SaveKind::Tokenized | SaveKind::Protected => {
            let owned = ctx.1.program.line_pairs();
            let view: Vec<(u16, &[u8])> =
                owned.iter().map(|(n, tok)| (*n, tok.as_slice())).collect();
            files::encode_tokenized(&view, kind == SaveKind::Protected).map_err(file_fault)?
        }
    };
    files::write_bytes(&path, &bytes).map_err(file_fault)?;
    Ok(Flow::Next)
}

fn exec_load(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
    chain: bool,
) -> Result<Flow, RunErr> {
    parser.bump();
    let name = parse_expr(parser)?;
    let mut run = chain;
    let mut start = None;
    if matches!(parser.cur, Tok::Punct(b',')) {
        parser.bump();
        if chain && is_line_ref(&parser.cur) {
            start = Some(parse_line_ref(parser)?);
        } else if !chain && name_is(&parser.cur, "R") {
            parser.bump();
            run = true;
        } else {
            return Err(fault(SYNTAX, 2));
        }
    }
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let name = eval_string(ctx.0, ctx.1, &name)?;
    let path = files::bas_path(&name).map_err(file_fault)?;
    let bytes = files::read_bytes(&path).map_err(file_fault)?;
    load_bytes(ctx.1.program, &bytes).map_err(file_fault)?;
    let start = match start {
        Some(dest) => match dest {
            LineRef::Num(n) => Some(n),
            LineRef::Dot => ctx.1.program.last,
        },
        None => None,
    };
    if run {
        if begin_program(ctx.0, ctx.1.program, start) {
            return Ok(Flow::Jump(ctx.0.ip));
        }
    } else {
        ctx.0.reset_runtime();
    }
    Ok(Flow::Ended)
}

fn exec_merge(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let name = parse_expr(parser)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    if ctx.1.program.is_protected() {
        return Err(fault(ILLEGAL, 5));
    }
    let name = eval_string(ctx.0, ctx.1, &name)?;
    let path = files::bas_path(&name).map_err(file_fault)?;
    let bytes = files::read_bytes(&path).map_err(file_fault)?;
    if matches!(bytes.first(), Some(0xFF | 0xFE)) {
        return Err(fault(files::BAD_FILE_MODE, 54));
    }
    load_ascii(ctx.1.program, &files::decode_ascii(&bytes), false).map_err(file_fault)?;
    Ok(Flow::Next)
}

fn exec_kill(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let name = parse_expr(parser)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let name = eval_string(ctx.0, ctx.1, &name)?;
    files::kill(&name).map_err(file_fault)?;
    Ok(Flow::Next)
}

fn exec_name(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let from = parse_expr(parser)?;
    expect_name(parser, "AS")?;
    let to = parse_expr(parser)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let from = eval_string(ctx.0, ctx.1, &from)?;
    let to = eval_string(ctx.0, ctx.1, &to)?;
    files::rename_file(&from, &to).map_err(file_fault)?;
    Ok(Flow::Next)
}

fn exec_files(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let spec = if terminated(&parser.cur) {
        None
    } else {
        Some(parse_expr(parser)?)
    };
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let spec = if let Some(spec) = spec {
        eval_string(ctx.0, ctx.1, &spec)?
    } else {
        String::new()
    };
    let names = files::list_dir(&spec).map_err(file_fault)?;
    if names.is_empty() {
        emit(ctx.1, "File not found\n");
    } else {
        for name in names {
            emit(ctx.1, &name);
            emit(ctx.1, "\n");
        }
    }
    Ok(Flow::Next)
}

#[derive(Clone, Copy)]
enum DirOp {
    Chdir,
    Mkdir,
    Rmdir,
}

fn exec_dir(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
    op: DirOp,
) -> Result<Flow, RunErr> {
    parser.bump();
    let path = parse_expr(parser)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let path = eval_string(ctx.0, ctx.1, &path)?;
    let result = match op {
        DirOp::Chdir => files::chdir(&path),
        DirOp::Mkdir => files::mkdir(&path),
        DirOp::Rmdir => files::rmdir(&path),
    };
    result.map_err(file_fault)?;
    Ok(Flow::Next)
}

fn exec_shell(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let command = if terminated(&parser.cur) {
        None
    } else {
        Some(parse_expr(parser)?)
    };
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let command = if let Some(command) = command {
        eval_string(ctx.0, ctx.1, &command)?
    } else {
        String::new()
    };
    let text = files::run_shell(&command, &ctx.0.environ).map_err(file_fault)?;
    if !text.is_empty() {
        emit(ctx.1, &text);
        if !text.ends_with('\n') {
            emit(ctx.1, "\n");
        }
    }
    Ok(Flow::Next)
}

fn guard_memory(program: &Program) -> Result<(), RunErr> {
    if program.is_protected() {
        Err(fault(ILLEGAL, 5))
    } else {
        Ok(())
    }
}

fn exec_poke(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let addr = parse_expr(parser)?;
    expect_punct(parser, b',')?;
    let value = parse_expr(parser)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    guard_memory(ctx.1.program)?;
    let addr = u16_addr(&eval_expr(ctx.0, ctx.1, &addr)?)?;
    let value = byte_of(&eval_expr(ctx.0, ctx.1, &value)?)?;
    ctx.0.memory[addr as usize] = value;
    Ok(Flow::Next)
}

fn exec_def_seg(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let segment = if parser.cur.is("=") {
        parser.bump();
        Some(parse_expr(parser)?)
    } else {
        None
    };
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    ctx.0.segment = if let Some(segment) = segment {
        u16_addr(&eval_expr(ctx.0, ctx.1, &segment)?)?
    } else {
        0
    };
    Ok(Flow::Next)
}

fn exec_bsave(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let name = parse_expr(parser)?;
    expect_punct(parser, b',')?;
    let offset = parse_expr(parser)?;
    expect_punct(parser, b',')?;
    let length = parse_expr(parser)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    guard_memory(ctx.1.program)?;
    let name = eval_string(ctx.0, ctx.1, &name)?;
    let offset = u16_addr(&eval_expr(ctx.0, ctx.1, &offset)?)?;
    let length = u16_addr(&eval_expr(ctx.0, ctx.1, &length)?)?;
    let path = files::plain_path(&name).map_err(file_fault)?;
    let start = offset as usize;
    let end = start
        .saturating_add(length as usize)
        .min(ctx.0.memory.len());
    let mut bytes = vec![0xFD];
    bytes.extend_from_slice(&ctx.0.segment.to_le_bytes());
    bytes.extend_from_slice(&offset.to_le_bytes());
    bytes.extend_from_slice(&(length).to_le_bytes());
    bytes.extend_from_slice(&ctx.0.memory[start..end]);
    files::write_bytes(&path, &bytes).map_err(file_fault)?;
    Ok(Flow::Next)
}

fn exec_bload(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let name = parse_expr(parser)?;
    let offset = if matches!(parser.cur, Tok::Punct(b',')) {
        parser.bump();
        Some(parse_expr(parser)?)
    } else {
        None
    };
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    guard_memory(ctx.1.program)?;
    let name = eval_string(ctx.0, ctx.1, &name)?;
    let path = files::plain_path(&name).map_err(file_fault)?;
    let bytes = files::read_bytes(&path).map_err(file_fault)?;
    if bytes.first() != Some(&0xFD) || bytes.len() < 7 {
        return Err(fault(files::BAD_FILE_MODE, 54));
    }
    let stored = u16::from_le_bytes([bytes[3], bytes[4]]);
    let offset = if let Some(offset) = offset {
        u16_addr(&eval_expr(ctx.0, ctx.1, &offset)?)?
    } else {
        stored
    };
    let data = &bytes[7..];
    let start = offset as usize;
    let end = start.saturating_add(data.len()).min(ctx.0.memory.len());
    let count = end - start;
    ctx.0.memory[start..end].copy_from_slice(&data[..count]);
    Ok(Flow::Next)
}

fn exec_sound(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let freq = parse_expr(parser)?;
    expect_punct(parser, b',')?;
    let duration = parse_expr(parser)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let freq = u16_addr(&eval_expr(ctx.0, ctx.1, &freq)?)?;
    let duration = cint_value(&eval_expr(ctx.0, ctx.1, &duration)?)?;
    if !(37..=32767).contains(&freq) || duration < 0 {
        return Err(fault(ILLEGAL, 5));
    }
    let ms = ((f64::from(duration) * 1000.0) / 18.2).round() as u32;
    *ctx.1.tone = Some((u32::from(freq), ms.max(1)));
    Ok(Flow::Next)
}

fn exec_error(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let code = parse_expr(parser)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let code = cint_value(&eval_expr(ctx.0, ctx.1, &code)?)?;
    if !(0..=255).contains(&code) {
        return Err(fault(ILLEGAL, 5));
    }
    Err(fault(message_for(code), code))
}

fn exec_resume(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let how = if parser.cur.is("NEXT") {
        parser.bump();
        ResumeHow::Next
    } else if is_line_ref(&parser.cur) {
        ResumeHow::Line(parse_line_ref(parser)?)
    } else {
        ResumeHow::Same
    };
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    if !ctx.0.handling_error {
        return Err(fault(RESUME_WITHOUT, 20));
    }
    ctx.0.handling_error = false;
    let ip = match how {
        ResumeHow::Same => ctx.0.resume_at.ok_or_else(|| fault(RESUME_WITHOUT, 20))?,
        ResumeHow::Next => {
            let at = ctx.0.resume_at.ok_or_else(|| fault(RESUME_WITHOUT, 20))?;
            skip_resumed(ctx.1.program, &ctx.0.direct, at)?
        }
        ResumeHow::Line(dest) => resolve_line(ctx.0, ctx.1.program, dest)?,
    };
    Ok(Flow::Jump(ip))
}

enum ResumeHow {
    Same,
    Next,
    Line(LineRef),
}

fn skip_resumed(program: &Program, direct: &[u8], ip: Ip) -> Result<Ip, RunErr> {
    let code = if let Some(number) = ip.line {
        program
            .code(number)
            .ok_or_else(|| fault(UNDEF_LINE, 8))?
            .to_vec()
    } else {
        direct.to_vec()
    };
    let mut parser = Parser::new(&code, ip.offset);
    skip_statement(&mut parser);
    if matches!(parser.cur, Tok::End) {
        if let Some(number) = ip.line {
            if let Some(next) = program.next_after(number) {
                return Ok(Ip {
                    line: Some(next),
                    offset: 0,
                });
            }
        }
        return Ok(Ip {
            line: ip.line,
            offset: parser.i,
        });
    }
    Ok(Ip {
        line: ip.line,
        offset: here(&parser),
    })
}

fn string_bytes(v: &Value) -> Result<Vec<u8>, RunErr> {
    let text = need_str(v)?;
    let mut bytes = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let code = u32::from(ch);
        if code > 255 {
            return Err(fault(ILLEGAL, 5));
        }
        bytes.push(code as u8);
    }
    Ok(bytes)
}

fn environ_get(vm: &Vm, name: &str) -> String {
    vm.environ
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
        .unwrap_or_default()
}

fn environ_assign(vm: &mut Vm, text: &str) -> Result<(), RunErr> {
    let Some(eq) = text.find('=') else {
        return Err(fault(ILLEGAL, 5));
    };
    let name = &text[..eq];
    let value = &text[eq + 1..];
    if name.is_empty() {
        return Err(fault(ILLEGAL, 5));
    }
    if value.is_empty() {
        vm.environ
            .retain(|(key, _)| !key.eq_ignore_ascii_case(name));
        return Ok(());
    }
    if let Some((_, slot)) = vm
        .environ
        .iter_mut()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
    {
        *slot = value.to_string();
    } else {
        vm.environ.push((name.to_string(), value.to_string()));
    }
    Ok(())
}

fn exec_environ(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let expr = parse_expr(parser)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let text = eval_string(ctx.0, ctx.1, &expr)?;
    environ_assign(ctx.0, &text)?;
    Ok(Flow::Next)
}

fn exec_reset(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    ctx.1.files.close_all();
    Ok(Flow::Next)
}

fn exec_justify(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
    left: bool,
) -> Result<Flow, RunErr> {
    parser.bump();
    let dest = parse_target(parser)?;
    expect_word(parser, "=")?;
    let expr = parse_expr(parser)?;
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    let value = eval_string(ctx.0, ctx.1, &expr)?;
    let current = need_str(&load_target(ctx.0, ctx.1, &dest)?)?;
    store_target(
        ctx.0,
        ctx.1,
        &dest,
        Value::Str(justify(&current, &value, left)),
    )?;
    Ok(Flow::Next)
}

fn justify(current: &str, value: &str, left: bool) -> String {
    let width = current.chars().count();
    let chars: Vec<char> = value.chars().take(width).collect();
    if chars.len() >= width {
        return chars.into_iter().collect();
    }
    let pad = width - chars.len();
    let mut out = String::new();
    if left {
        out.extend(chars);
        out.extend(std::iter::repeat_n(' ', pad));
    } else {
        out.extend(std::iter::repeat_n(' ', pad));
        out.extend(chars);
    }
    out
}

fn optional_renum_number(parser: &mut Parser<'_>) -> Result<Option<u32>, RunErr> {
    if terminated(&parser.cur) || matches!(parser.cur, Tok::Punct(b',')) {
        return Ok(None);
    }
    let n = match &parser.cur {
        Tok::Int(n) => {
            if *n < 0 {
                return Err(fault(ILLEGAL, 5));
            }
            u32::from(*n as u16)
        }
        Tok::Hex(n) | Tok::Oct(n) | Tok::Line(n) => u32::from(*n),
        Tok::Single(m) | Tok::Double(m) => whole_line_number(*m)?,
        _ => return Err(fault(SYNTAX, 2)),
    };
    parser.bump();
    if n > 65529 {
        return Err(fault(ILLEGAL, 5));
    }
    Ok(Some(n))
}

fn whole_line_number(m: Mbf) -> Result<u32, RunErr> {
    let f = m.to_f64();
    if !(0.0..=65529.0).contains(&f) || f.fract() != 0.0 {
        return Err(fault(ILLEGAL, 5));
    }
    Ok(f as u32)
}

fn exec_renum(
    parser: &mut Parser<'_>,
    mode: Mode,
    ctx: Option<&mut (&mut Vm, &mut Host<'_>)>,
) -> Result<Flow, RunErr> {
    parser.bump();
    let new_start = optional_renum_number(parser)?.unwrap_or(10);
    let mut old = None;
    let mut increment = 10u32;
    if matches!(parser.cur, Tok::Punct(b',')) {
        parser.bump();
        old = optional_renum_number(parser)?;
        if matches!(parser.cur, Tok::Punct(b',')) {
            parser.bump();
            if let Some(n) = optional_renum_number(parser)? {
                increment = n;
            }
        }
    }
    if !terminated(&parser.cur) {
        return Err(fault(SYNTAX, 2));
    }
    if mode == Mode::Check {
        return Ok(Flow::Next);
    }
    let ctx = ctx.ok_or_else(|| fault(SYNTAX, 2))?;
    if ctx.0.current_line.is_some() || ctx.1.program.is_protected() {
        return Err(fault(ILLEGAL, 5));
    }
    if increment == 0 {
        return Err(fault(ILLEGAL, 5));
    }
    if ctx.1.program.is_empty() {
        return Ok(Flow::Next);
    }
    let numbers: Vec<u16> = ctx.1.program.iter_code().map(|(n, _)| n).collect();
    let start_at = if let Some(old) = old {
        let old = u16::try_from(old).map_err(|_| fault(ILLEGAL, 5))?;
        numbers
            .iter()
            .position(|&n| n == old)
            .ok_or_else(|| fault(UNDEF_LINE, 8))?
    } else {
        0
    };
    if start_at > 0 && new_start <= u32::from(numbers[start_at - 1]) {
        return Err(fault(ILLEGAL, 5));
    }
    let moving = &numbers[start_at..];
    let mut next = new_start;
    let mut map = BTreeMap::new();
    for (index, &number) in moving.iter().enumerate() {
        if next > 65529 {
            return Err(fault(ILLEGAL, 5));
        }
        let new_number = u16::try_from(next).map_err(|_| fault(ILLEGAL, 5))?;
        map.insert(number, new_number);
        if index + 1 == moving.len() {
            break;
        }
        next = next
            .checked_add(increment)
            .ok_or_else(|| fault(ILLEGAL, 5))?;
    }
    let present: BTreeSet<u16> = numbers.iter().copied().collect();
    let mut rebuilt = Vec::new();
    let mut notes = Vec::new();
    for (number, code) in ctx.1.program.iter_code() {
        let (tokens, missing) = crunch::retarget_lines(code, &map, &present);
        let new_number = map.get(&number).copied().unwrap_or(number);
        for miss in missing {
            notes.push(format!("Undefined line {miss} in {new_number}"));
        }
        let listed = crunch::list_tokens(&tokens);
        let text = if listed.is_empty() {
            new_number.to_string()
        } else {
            format!("{new_number} {listed}")
        };
        rebuilt.push((new_number, tokens, text));
    }
    ctx.1.program.rewrite(rebuilt, &map);
    ctx.0.invalidate_cont();
    for note in notes {
        emit(ctx.1, &note);
        emit(ctx.1, "\n");
    }
    Ok(Flow::Next)
}
