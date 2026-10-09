//! Tokenize a source line the way GW-BASIC does on Enter, and list it back.
//!
//! Spaces outside strings, `REM`, and `DATA` are not stored. `'` is stored as
//! the `:REM'` sequence. `WHILE` is followed by a hidden `+`. `ELSE` is stored
//! as colon + `ELSE`, and listings hide that colon. Keywords come back in
//! uppercase. A line number after `GOTO`, `GOSUB`, `THEN`, `ELSE`, `RESTORE`,
//! or `RESUME` is the `0x0E` operand, not an integer constant.

use crate::mbf::Mbf;
use crate::tokens::{
    self, emit_keyword, find_keyword, keyword_name, T_BYTE, T_COLON, T_DATA, T_DBL, T_ELSE, T_HEX,
    T_I16, T_INT0, T_LINE, T_OCT, T_PLUS, T_QUOTE, T_REM, T_SNG, T_TICK, T_WHILE,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FaultKind {
    Syntax,
    Overflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub kind: FaultKind,
    pub column: usize,
}

impl Fault {
    fn syntax(column: usize) -> Self {
        Self {
            kind: FaultKind::Syntax,
            column,
        }
    }

    fn overflow(column: usize) -> Self {
        Self {
            kind: FaultKind::Overflow,
            column,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Tok {
    End,
    Rem(Vec<u8>),
    Data(Vec<u8>),
    Tick(Vec<u8>),
    Word(&'static str),
    Int(i16),
    Hex(u16),
    Oct(u16),
    Line(u16),
    Single(Mbf),
    Double(Mbf),
    Str(Vec<u8>),
    Ident(String),
    Colon,
    Punct(u8),
}

impl Tok {
    pub fn word(&self) -> Option<&str> {
        match self {
            Tok::Word(name) => Some(name),
            _ => None,
        }
    }

    pub fn is(&self, name: &str) -> bool {
        self.word() == Some(name)
    }
}

pub fn crunch(source: &str) -> Result<Vec<u8>, Fault> {
    let src = source.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    let mut line_mode = false;
    while i < src.len() {
        if src[i] == b' ' {
            while src.get(i) == Some(&b' ') {
                i += 1;
            }
            // Spaces inside a name are discarded (`MY VAR` is `MYVAR`).
            // A space before AS / OUTPUT / APPEND / RANDOM is kept, because
            // those words are not keywords and would otherwise merge (`OUTPUTAS`).
            if keep_name_separator(&out, &src[i..]) {
                out.push(b' ');
            }
            continue;
        }
        if line_mode {
            if let Some(done) = take_line_number(src, &mut i, &mut out)? {
                line_mode = !done;
                continue;
            }
            line_mode = false;
        }
        if src[i] == b'"' {
            out.push(T_QUOTE);
            i += 1;
            let start = i;
            while i < src.len() && src[i] != b'"' {
                i += 1;
            }
            if i >= src.len() {
                return Err(Fault::syntax(start.saturating_sub(1)));
            }
            out.extend_from_slice(&src[start..i]);
            out.push(T_QUOTE);
            i += 1;
            continue;
        }
        if src[i] == b'&' || src[i].is_ascii_digit() || (src[i] == b'.' && peek_digit(src, i + 1)) {
            take_number(src, &mut i, &mut out, false)?;
            continue;
        }
        if src[i] == b'.' {
            out.push(b'.');
            i += 1;
            continue;
        }
        if src[i] == b'?' {
            out.push(0x91);
            i += 1;
            line_mode = false;
            continue;
        }
        if let Some(kw) = find_keyword(src, i) {
            i += kw.name.len();
            match kw.name {
                "'" => {
                    out.push(T_COLON);
                    out.push(T_REM);
                    out.push(T_TICK);
                    out.extend_from_slice(&src[i..]);
                    break;
                }
                "REM" => {
                    out.push(T_REM);
                    out.extend_from_slice(&src[i..]);
                    break;
                }
                "DATA" => {
                    out.push(T_DATA);
                    let start = i;
                    let mut quoted = false;
                    while i < src.len() {
                        if src[i] == b'"' {
                            quoted = !quoted;
                        } else if src[i] == b':' && !quoted {
                            break;
                        }
                        i += 1;
                    }
                    out.extend_from_slice(&src[start..i]);
                    continue;
                }
                "ELSE" => {
                    out.push(T_COLON);
                    out.push(T_ELSE);
                    line_mode = true;
                    continue;
                }
                "WHILE" => {
                    out.push(T_WHILE);
                    out.push(T_PLUS);
                    continue;
                }
                "GOTO" | "GOSUB" | "THEN" | "RESTORE" | "RESUME" => {
                    emit_keyword(&mut out, kw);
                    line_mode = true;
                    continue;
                }
                _ => {
                    emit_keyword(&mut out, kw);
                    continue;
                }
            }
        }
        if src[i].is_ascii_alphabetic() {
            take_ident(src, &mut i, &mut out);
            continue;
        }
        match src[i] {
            b'(' | b')' | b',' | b';' | b'#' | b'$' => {
                out.push(src[i]);
                i += 1;
            }
            _ => return Err(Fault::syntax(i)),
        }
    }
    out.push(0);
    Ok(out)
}

/// `true` when line-number mode should end (no following comma).
fn take_line_number(src: &[u8], i: &mut usize, out: &mut Vec<u8>) -> Result<Option<bool>, Fault> {
    let at = *i;
    if src.get(*i) == Some(&b'.') && !peek_digit(src, *i + 1) {
        out.push(b'.');
        *i += 1;
        return Ok(Some(!comma_continues(src, i, out)));
    }
    if !src.get(*i).is_some_and(|c| c.is_ascii_digit()) {
        return Ok(None);
    }
    let mut n: u32 = 0;
    while src.get(*i).is_some_and(|c| c.is_ascii_digit()) {
        n = n
            .saturating_mul(10)
            .saturating_add(u32::from(src[*i] - b'0'));
        *i += 1;
    }
    if src
        .get(*i)
        .is_some_and(|c| matches!(c, b'.' | b'!' | b'#' | b'E' | b'e' | b'D' | b'd' | b'%'))
    {
        *i = at;
        return Ok(None);
    }
    if n > 65529 {
        return Err(Fault::syntax(at));
    }
    out.push(T_LINE);
    out.extend_from_slice(&(n as u16).to_le_bytes());
    Ok(Some(!comma_continues(src, i, out)))
}

fn comma_continues(src: &[u8], i: &mut usize, out: &mut Vec<u8>) -> bool {
    let mut j = *i;
    while src.get(j) == Some(&b' ') {
        j += 1;
    }
    if src.get(j) == Some(&b',') {
        out.push(b',');
        *i = j + 1;
        true
    } else {
        false
    }
}

fn peek_digit(src: &[u8], i: usize) -> bool {
    src.get(i).is_some_and(|c| c.is_ascii_digit())
}

fn take_number(src: &[u8], i: &mut usize, out: &mut Vec<u8>, _line: bool) -> Result<(), Fault> {
    let at = *i;
    if src[*i] == b'&' {
        *i += 1;
        let hex = matches!(src.get(*i), Some(b'H' | b'h'));
        let oct = !hex;
        if matches!(src.get(*i), Some(b'H' | b'h' | b'O' | b'o')) {
            *i += 1;
        }
        let start_digits = *i;
        let mut value: u32 = 0;
        if hex {
            while src.get(*i).is_some_and(|c| c.is_ascii_hexdigit()) {
                let d = match src[*i] {
                    b'0'..=b'9' => src[*i] - b'0',
                    b'a'..=b'f' => src[*i] - b'a' + 10,
                    _ => src[*i] - b'A' + 10,
                };
                value = value.saturating_mul(16).saturating_add(u32::from(d));
                *i += 1;
            }
        } else {
            while src.get(*i).is_some_and(|c| matches!(c, b'0'..=b'7')) {
                value = value
                    .saturating_mul(8)
                    .saturating_add(u32::from(src[*i] - b'0'));
                *i += 1;
            }
        }
        if *i == start_digits || value > 0xFFFF {
            return Err(Fault::syntax(at));
        }
        let bits = value as u16;
        out.push(if hex { T_HEX } else { T_OCT });
        out.extend_from_slice(&(bits as i16).to_le_bytes());
        let _ = oct;
        return Ok(());
    }

    let start = *i;
    let mut saw_digit = false;
    let mut dot = false;
    let mut sig = 0usize;
    while src
        .get(*i)
        .is_some_and(|c| c.is_ascii_digit() || (*c == b'.' && !dot))
    {
        if src[*i] == b'.' {
            dot = true;
        } else {
            saw_digit = true;
            if src[*i] != b'0' || sig > 0 {
                sig += 1;
            }
        }
        *i += 1;
    }
    if !saw_digit {
        return Err(Fault::syntax(at));
    }
    let mut exp_letter = None;
    if matches!(src.get(*i), Some(b'E' | b'e' | b'D' | b'd')) {
        exp_letter = Some(src[*i]);
        *i += 1;
        if matches!(src.get(*i), Some(b'+' | b'-')) {
            *i += 1;
        }
        let exp_digits = *i;
        while src.get(*i).is_some_and(|c| c.is_ascii_digit()) {
            *i += 1;
        }
        if *i == exp_digits {
            return Err(Fault::syntax(at));
        }
    }
    let mut suffix = None;
    if matches!(src.get(*i), Some(b'%' | b'!' | b'#')) {
        suffix = Some(src[*i]);
        *i += 1;
    }
    let literal = std::str::from_utf8(&src[start..(*i - usize::from(suffix.is_some()))])
        .map_err(|_| Fault::syntax(at))?;
    if !dot && exp_letter.is_none() && (suffix == Some(b'%') || suffix.is_none()) {
        match literal.parse::<i32>() {
            Ok(n) if (-32768..=32767).contains(&n) => {
                emit_int(out, n as i16);
                return Ok(());
            }
            _ if suffix == Some(b'%') => return Err(Fault::overflow(at)),
            _ => {}
        }
    }
    let double = suffix == Some(b'#')
        || matches!(exp_letter, Some(b'D' | b'd'))
        || (suffix.is_none() && sig > 7 && exp_letter.is_none());
    // An `E` exponent forces single even when there are many digits.
    let double = if matches!(exp_letter, Some(b'E' | b'e')) && suffix != Some(b'#') {
        false
    } else {
        double
    };
    if suffix == Some(b'!') {
        let value = Mbf::parse(literal, false).map_err(|_| Fault::overflow(at))?;
        out.push(T_SNG);
        out.extend_from_slice(&value.to_le_single());
        return Ok(());
    }
    let value = Mbf::parse(literal, double).map_err(|_| Fault::overflow(at))?;
    if double {
        out.push(T_DBL);
        out.extend_from_slice(&value.to_le_double());
    } else {
        out.push(T_SNG);
        out.extend_from_slice(&value.to_le_single());
    }
    Ok(())
}

fn emit_int(out: &mut Vec<u8>, n: i16) {
    if (0..=10).contains(&n) {
        out.push(T_INT0 + n as u8);
    } else if (11..=255).contains(&n) {
        out.push(T_BYTE);
        out.push(n as u8);
    } else {
        out.push(T_I16);
        out.extend_from_slice(&n.to_le_bytes());
    }
}

fn emit_renumbered_const(out: &mut Vec<u8>, n: u16) {
    if let Ok(narrow) = i16::try_from(n) {
        emit_int(out, narrow);
    } else {
        let value = Mbf::from_i32(i32::from(n), false);
        out.push(T_SNG);
        out.extend_from_slice(&value.to_le_single());
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ErlScan {
    No,
    Name,
    Rel,
}

/// Rewrite line-number operands after `RENUM`.
///
/// A `T_LINE` that names a renumbered line is replaced. One that names no line
/// in `present` is left as written and returned. `ON ERROR GOTO 0` disables
/// trapping, so that zero is not a reference to line 0. An integer compared
/// with `ERL` is updated when it names a renumbered line, and is not reported
/// when it does not. `REM`, remarks, `DATA`, and quoted strings are copied
/// through.
pub fn retarget_lines(
    tokens: &[u8],
    map: &BTreeMap<u16, u16>,
    present: &BTreeSet<u16>,
) -> (Vec<u8>, Vec<u16>) {
    let mut out = Vec::with_capacity(tokens.len());
    let mut missing = Vec::new();
    let mut i = 0;
    let mut erl = ErlScan::No;
    let mut recent = ["", "", ""];
    while i < tokens.len() {
        let b = tokens[i];
        if b == 0 {
            out.push(0);
            break;
        }
        if b == b' ' {
            out.push(b);
            i += 1;
            continue;
        }
        if erl != ErlScan::No && is_relation(b) {
            out.push(b);
            i += 1;
            erl = ErlScan::Rel;
            if let Some(name) = keyword_name(0, b) {
                push_recent(&mut recent, name);
            }
            continue;
        }
        if erl == ErlScan::Rel {
            if let Some((value, len)) = int_at(tokens, i) {
                if (0..=32767).contains(&value) {
                    let old = u16::try_from(value).unwrap_or(0);
                    if let Some(&new_n) = map.get(&old) {
                        emit_renumbered_const(&mut out, new_n);
                        i += len;
                        erl = ErlScan::No;
                        continue;
                    }
                }
                if !copy_width(&mut out, tokens, &mut i, len) {
                    break;
                }
                erl = ErlScan::No;
                continue;
            }
        }
        erl = ErlScan::No;
        if b == T_COLON && tokens.get(i + 1) == Some(&T_REM) && tokens.get(i + 2) == Some(&T_TICK) {
            if !copy_width(&mut out, tokens, &mut i, 3) {
                break;
            }
            copy_until_zero(&mut out, tokens, &mut i);
            continue;
        }
        if b == T_REM {
            out.push(b);
            i += 1;
            copy_until_zero(&mut out, tokens, &mut i);
            continue;
        }
        if b == T_DATA {
            out.push(b);
            i += 1;
            copy_data(&mut out, tokens, &mut i);
            continue;
        }
        if b == T_QUOTE {
            out.push(b);
            i += 1;
            while i < tokens.len() && tokens[i] != T_QUOTE && tokens[i] != 0 {
                out.push(tokens[i]);
                i += 1;
            }
            if tokens.get(i) == Some(&T_QUOTE) {
                out.push(T_QUOTE);
                i += 1;
            }
            continue;
        }
        if b == T_LINE {
            if i + 3 > tokens.len() {
                out.extend_from_slice(&tokens[i..]);
                break;
            }
            let n = u16::from_le_bytes([tokens[i + 1], tokens[i + 2]]);
            i += 3;
            let disable = n == 0 && recent == ["ON", "ERROR", "GOTO"];
            let rewritten = if disable {
                n
            } else {
                map.get(&n).copied().unwrap_or(n)
            };
            if !disable && !map.contains_key(&n) && !present.contains(&n) {
                missing.push(n);
            }
            out.push(T_LINE);
            out.extend_from_slice(&rewritten.to_le_bytes());
            continue;
        }
        if matches!(b, 0xFD..=0xFF) {
            let prefix = b;
            let code = tokens.get(i + 1).copied().unwrap_or(0);
            if !copy_width(&mut out, tokens, &mut i, 2) {
                break;
            }
            if let Some(name) = keyword_name(prefix, code) {
                push_recent(&mut recent, name);
            }
            continue;
        }
        if (0x80..=0xFC).contains(&b) {
            out.push(b);
            i += 1;
            if let Some(name) = keyword_name(0, b) {
                push_recent(&mut recent, name);
                if name == "ERL" {
                    erl = ErlScan::Name;
                }
            }
            continue;
        }
        if let Some((_, len)) = int_at(tokens, i) {
            if !copy_width(&mut out, tokens, &mut i, len) {
                break;
            }
            continue;
        }
        let width = match b {
            T_OCT | T_HEX => 3,
            T_SNG => 5,
            T_DBL => 9,
            _ => 1,
        };
        if width > 1 {
            if !copy_width(&mut out, tokens, &mut i, width) {
                break;
            }
            continue;
        }
        if b.is_ascii_alphabetic() {
            let start = i;
            i += 1;
            while tokens.get(i).is_some_and(|c| c.is_ascii_alphanumeric()) {
                i += 1;
            }
            if tokens
                .get(i)
                .is_some_and(|c| matches!(c, b'%' | b'!' | b'$'))
            {
                i += 1;
            }
            out.extend_from_slice(&tokens[start..i]);
            continue;
        }
        out.push(b);
        i += 1;
    }
    (out, missing)
}

fn push_recent(recent: &mut [&'static str; 3], name: &'static str) {
    recent[0] = recent[1];
    recent[1] = recent[2];
    recent[2] = name;
}

fn is_relation(b: u8) -> bool {
    matches!(keyword_name(0, b), Some("<" | "=" | ">"))
}

fn int_at(tokens: &[u8], i: usize) -> Option<(i16, usize)> {
    let b = *tokens.get(i)?;
    if (T_INT0..=T_INT0 + 10).contains(&b) {
        return Some((i16::from(b - T_INT0), 1));
    }
    if b == T_BYTE {
        return Some((i16::from(*tokens.get(i + 1)?), 2));
    }
    if b == T_I16 {
        let lo = *tokens.get(i + 1)?;
        let hi = *tokens.get(i + 2)?;
        return Some((i16::from_le_bytes([lo, hi]), 3));
    }
    None
}

fn copy_width(out: &mut Vec<u8>, tokens: &[u8], i: &mut usize, n: usize) -> bool {
    if *i + n > tokens.len() {
        out.extend_from_slice(&tokens[*i..]);
        *i = tokens.len();
        false
    } else {
        out.extend_from_slice(&tokens[*i..*i + n]);
        *i += n;
        true
    }
}

fn copy_until_zero(out: &mut Vec<u8>, tokens: &[u8], i: &mut usize) {
    while *i < tokens.len() && tokens[*i] != 0 {
        out.push(tokens[*i]);
        *i += 1;
    }
}

fn copy_data(out: &mut Vec<u8>, tokens: &[u8], i: &mut usize) {
    let mut quoted = false;
    while *i < tokens.len() && tokens[*i] != 0 {
        let b = tokens[*i];
        if b == T_QUOTE {
            quoted = !quoted;
        } else if b == T_COLON && !quoted {
            break;
        }
        out.push(b);
        *i += 1;
    }
}

fn take_ident(src: &[u8], i: &mut usize, out: &mut Vec<u8>) {
    let mut kept = 0;
    while src.get(*i).is_some_and(|c| c.is_ascii_alphanumeric()) {
        if kept < 40 {
            out.push(src[*i].to_ascii_uppercase());
            kept += 1;
        }
        *i += 1;
    }
    if matches!(src.get(*i), Some(b'%' | b'!' | b'#' | b'$')) {
        out.push(src[*i]);
        *i += 1;
    }
}

pub fn next_tok(toks: &[u8], i: &mut usize) -> Tok {
    while toks.get(*i) == Some(&b' ') {
        *i += 1;
    }
    if *i >= toks.len() || toks[*i] == 0 {
        return Tok::End;
    }
    let b = toks[*i];
    if b == T_COLON && toks.get(*i + 1) == Some(&T_REM) && toks.get(*i + 2) == Some(&T_TICK) {
        *i += 3;
        return Tok::Tick(take_rest(toks, i));
    }
    if b == T_COLON {
        *i += 1;
        return Tok::Colon;
    }
    if b == T_REM {
        *i += 1;
        return Tok::Rem(take_rest(toks, i));
    }
    if b == T_DATA {
        *i += 1;
        return Tok::Data(take_data_payload(toks, i));
    }
    if b == T_WHILE {
        *i += 1;
        if toks.get(*i) == Some(&T_PLUS) {
            *i += 1;
        }
        return Tok::Word("WHILE");
    }
    match b {
        0x11..=0x1B => {
            *i += 1;
            Tok::Int(i16::from(b - 0x11))
        }
        T_BYTE => {
            let n = toks.get(*i + 1).copied().unwrap_or(0);
            *i += 2;
            Tok::Int(i16::from(n))
        }
        T_I16 => {
            let n = read_i16(toks, *i + 1);
            *i += 3;
            Tok::Int(n)
        }
        T_OCT => {
            let n = read_i16(toks, *i + 1) as u16;
            *i += 3;
            Tok::Oct(n)
        }
        T_HEX => {
            let n = read_i16(toks, *i + 1) as u16;
            *i += 3;
            Tok::Hex(n)
        }
        T_LINE => {
            let n = read_i16(toks, *i + 1) as u16;
            *i += 3;
            Tok::Line(n)
        }
        T_SNG => {
            let mut bytes = [0u8; 4];
            for (k, slot) in bytes.iter_mut().enumerate() {
                *slot = toks.get(*i + 1 + k).copied().unwrap_or(0);
            }
            *i += 5;
            Tok::Single(Mbf::from_le_single(bytes))
        }
        T_DBL => {
            let mut bytes = [0u8; 8];
            for (k, slot) in bytes.iter_mut().enumerate() {
                *slot = toks.get(*i + 1 + k).copied().unwrap_or(0);
            }
            *i += 9;
            Tok::Double(Mbf::from_le_double(bytes))
        }
        0xFD..=0xFF => {
            let code = toks.get(*i + 1).copied().unwrap_or(0);
            *i += 2;
            Tok::Word(keyword_name(b, code).unwrap_or("?"))
        }
        0x80..=0xFC => {
            *i += 1;
            Tok::Word(keyword_name(0, b).unwrap_or("?"))
        }
        T_QUOTE => {
            *i += 1;
            let start = *i;
            while *i < toks.len() && toks[*i] != T_QUOTE && toks[*i] != 0 {
                *i += 1;
            }
            let text = toks[start..*i].to_vec();
            if *i < toks.len() && toks[*i] == T_QUOTE {
                *i += 1;
            }
            Tok::Str(text)
        }
        c if c.is_ascii_alphabetic() => {
            let start = *i;
            while *i < toks.len() && toks[*i].is_ascii_alphanumeric() {
                *i += 1;
            }
            // `%` `!` `$` are only type suffixes. `#` is also the file-number
            // mark in `AS #1`, so it stays punctuation and variable parsing
            // absorbs it when a name is expected.
            if *i < toks.len() && matches!(toks[*i], b'%' | b'!' | b'$') {
                *i += 1;
            }
            let name = toks[start..*i].iter().map(|b| char::from(*b)).collect();
            Tok::Ident(name)
        }
        c => {
            *i += 1;
            Tok::Punct(c)
        }
    }
}

fn read_i16(toks: &[u8], at: usize) -> i16 {
    let lo = toks.get(at).copied().unwrap_or(0);
    let hi = toks.get(at + 1).copied().unwrap_or(0);
    i16::from_le_bytes([lo, hi])
}

fn take_rest(toks: &[u8], i: &mut usize) -> Vec<u8> {
    let start = *i;
    while *i < toks.len() && toks[*i] != 0 {
        *i += 1;
    }
    toks[start..*i].to_vec()
}

fn take_data_payload(toks: &[u8], i: &mut usize) -> Vec<u8> {
    let start = *i;
    let mut quoted = false;
    while *i < toks.len() && toks[*i] != 0 {
        if toks[*i] == T_QUOTE {
            quoted = !quoted;
        } else if toks[*i] == T_COLON && !quoted {
            break;
        }
        *i += 1;
    }
    toks[start..*i].to_vec()
}

pub fn list_tokens(tokens: &[u8]) -> String {
    let mut i = 0;
    let mut toks = Vec::new();
    loop {
        let tok = next_tok(tokens, &mut i);
        let stop = matches!(tok, Tok::End | Tok::Rem(_) | Tok::Tick(_));
        toks.push(tok);
        if stop {
            break;
        }
    }
    render(&toks)
}

fn render(toks: &[Tok]) -> String {
    let mut out = String::new();
    let mut index = 0;
    while index < toks.len() {
        if matches!(toks[index], Tok::End) {
            break;
        }
        if matches!(toks[index], Tok::Colon) && toks.get(index + 1).is_some_and(|t| t.is("ELSE")) {
            index += 1;
            continue;
        }
        let piece = render_one(&toks[index]);
        if space_before(&out, &toks[index]) || space_before_file_mark(&out, &toks[index..]) {
            out.push(' ');
        }
        out.push_str(&piece);
        if space_after(&toks[index], toks.get(index + 1)) {
            out.push(' ');
        }
        index += 1;
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

fn render_one(tok: &Tok) -> String {
    match tok {
        Tok::End => String::new(),
        Tok::Rem(text) => format!("REM{}", latin1(text)),
        Tok::Data(text) => format!("DATA{}", latin1(text)),
        Tok::Tick(text) => format!("'{}", latin1(text)),
        Tok::Word(name) => (*name).to_string(),
        Tok::Int(n) => n.to_string(),
        Tok::Hex(n) => format!("&H{n:X}"),
        Tok::Oct(n) => format!("&O{n:o}"),
        Tok::Line(n) => n.to_string(),
        Tok::Single(n) => n.format_literal(),
        Tok::Double(n) => n.format_literal(),
        Tok::Str(text) => format!("\"{}\"", latin1(text)),
        Tok::Ident(name) => name.clone(),
        Tok::Colon => ":".to_string(),
        Tok::Punct(c) => char::from(*c).to_string(),
    }
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().copied().map(char::from).collect()
}

fn space_before_file_mark(out: &str, toks: &[Tok]) -> bool {
    if out.is_empty() || out.ends_with(' ') || !ends_with_name(out) {
        return false;
    }
    let Some(Tok::Punct(b'#')) = toks.first() else {
        return false;
    };
    // `A# = 1` keeps the suffix glued. `AS #1` puts a space before the mark
    // when a value follows it.
    matches!(
        toks.get(1),
        Some(
            Tok::Int(_)
                | Tok::Hex(_)
                | Tok::Oct(_)
                | Tok::Single(_)
                | Tok::Double(_)
                | Tok::Ident(_)
                | Tok::Line(_)
                | Tok::Punct(b'(')
        )
    )
}

fn ends_with_name(out: &str) -> bool {
    out.ends_with(|c: char| c.is_ascii_alphanumeric() || matches!(c, '$' | '%' | '!' | '#'))
}

/// `true` when `next` is a non-keyword word that must not stick to the name
/// just written. `INPUT` is a real keyword, so it does not need this.
fn keep_name_separator(out: &[u8], next: &[u8]) -> bool {
    let Some(prev) = out.last().copied() else {
        return false;
    };
    if !prev.is_ascii_alphanumeric() {
        return false;
    }
    for word in ["OUTPUT", "APPEND", "RANDOM", "AS"] {
        if starts_with_word(next, word) {
            return true;
        }
    }
    false
}

fn starts_with_word(src: &[u8], word: &str) -> bool {
    let name = word.as_bytes();
    if src.len() < name.len() || !src[..name.len()].eq_ignore_ascii_case(name) {
        return false;
    }
    !src.get(name.len())
        .is_some_and(|c| c.is_ascii_alphanumeric())
}

fn space_before(out: &str, tok: &Tok) -> bool {
    if out.is_empty() || out.ends_with(' ') {
        return false;
    }
    match tok {
        Tok::Word(name) if tokens::is_word(name) && !matches!(*name, "REM" | "DATA") => true,
        Tok::Ident(_) => out.ends_with(':') || ends_with_name(out),
        Tok::Int(_)
        | Tok::Hex(_)
        | Tok::Oct(_)
        | Tok::Line(_)
        | Tok::Single(_)
        | Tok::Double(_)
        | Tok::Str(_) => out.ends_with(':'),
        _ => false,
    }
}

fn space_after(tok: &Tok, next: Option<&Tok>) -> bool {
    let Some(next) = next else {
        return false;
    };
    if matches!(next, Tok::End) {
        return false;
    }
    match tok {
        Tok::Colon => true,
        Tok::Word(name) if tokens::is_word(name) => {
            if matches!(*name, "REM" | "DATA") || name.ends_with('$') || name.ends_with('(') {
                return false;
            }
            !matches!(
                next,
                Tok::Colon
                    | Tok::Punct(b',')
                    | Tok::Punct(b';')
                    | Tok::Punct(b')')
                    | Tok::Punct(b'$')
                    | Tok::End
            )
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(source: &str) -> String {
        list_tokens(&crunch(source).unwrap())
    }

    #[test]
    fn lists_the_editor_lines_the_tests_store() {
        assert_eq!(round_trip(" REM A"), "REM A");
        assert_eq!(round_trip(" PRINT"), "PRINT");
        assert_eq!(round_trip(" REM SIDE"), "REM SIDE");
        assert_eq!(round_trip(" REM"), "REM");
        assert_eq!(
            round_trip(" PRINT \"Hello from GW-BASIC\""),
            "PRINT \"Hello from GW-BASIC\""
        );
        assert_eq!(round_trip(" END"), "END");
    }

    #[test]
    fn output_and_as_stay_separate_words() {
        assert_eq!(
            round_trip(r#"OPEN "F" FOR OUTPUT AS #1"#),
            r#"OPEN "F" FOR OUTPUT AS #1"#
        );
        assert_eq!(round_trip("MY VAR=1"), "MYVAR=1");
    }

    #[test]
    fn question_mark_is_print_and_while_hides_its_plus() {
        assert_eq!(round_trip("? \"Hi\""), "PRINT \"Hi\"");
        assert_eq!(round_trip(" WHILE A=0"), "WHILE A=0");
        assert_eq!(round_trip(" ' hi"), "' hi");
        assert_eq!(round_trip(" FOR I=1 TO 10"), "FOR I=1 TO 10");
        assert_eq!(round_trip(" A=1"), "A=1");
    }

    #[test]
    fn resume_and_environ_dollar_round_trip_and_renumber_patches_refs() {
        use std::collections::{BTreeMap, BTreeSet};

        let tokens = crunch("RESUME 40").unwrap();
        assert!(tokens
            .windows(3)
            .any(|w| { w[0] == crate::tokens::T_LINE && u16::from_le_bytes([w[1], w[2]]) == 40 }));
        assert_eq!(round_trip("ENVIRON$(\"A\")"), "ENVIRON$(\"A\")");

        let mut map = BTreeMap::new();
        map.insert(0, 100);
        let present = BTreeSet::from([0]);
        let (out, missing) = retarget_lines(&crunch("ON ERROR GOTO 0").unwrap(), &map, &present);
        assert!(missing.is_empty());
        assert_eq!(list_tokens(&out), "ON ERROR GOTO 0");
        let (out, missing) = retarget_lines(&crunch("GOTO 0").unwrap(), &map, &present);
        assert!(missing.is_empty());
        assert_eq!(list_tokens(&out), "GOTO 100");

        let mut map = BTreeMap::new();
        map.insert(30, 110);
        let present = BTreeSet::from([30]);
        let (out, missing) = retarget_lines(&crunch("GOTO 99").unwrap(), &map, &present);
        assert_eq!(missing, vec![99]);
        assert_eq!(list_tokens(&out), "GOTO 99");
        let (out, missing) = retarget_lines(&crunch("IF ERL=30 THEN END").unwrap(), &map, &present);
        assert!(missing.is_empty());
        assert_eq!(list_tokens(&out), "IF ERL =110 THEN END");
        let (out, missing) = retarget_lines(&crunch("DATA 30").unwrap(), &map, &present);
        assert!(missing.is_empty());
        assert_eq!(list_tokens(&out), "DATA 30");
        let (out, _) = retarget_lines(&crunch("REM GOTO 30").unwrap(), &map, &present);
        assert_eq!(list_tokens(&out), "REM GOTO 30");
    }
}
