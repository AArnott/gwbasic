//! Program files and sequential data files.
//!
//! Tokenized programs start with `FF`. Protected programs start with `FE` and
//! the rest of the file is the Stephen Kitt cipher (counters begin at `0x0D`
//! and `0x0B`). ASCII programs are the listing text. `LOAD` and `SAVE` add
//! `.BAS` when the name has no extension. `OPEN` uses the name as given.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const FILE_NOT_FOUND: &str = "File not found";
pub const BAD_FILE_NUMBER: &str = "Bad file number";
pub const BAD_FILE_MODE: &str = "Bad file mode";
pub const FILE_ALREADY_OPEN: &str = "File already open";
pub const INPUT_PAST_END: &str = "Input past end";
pub const BAD_FILE_NAME: &str = "Bad file name";
pub const FILE_ALREADY_EXISTS: &str = "File already exists";
pub const PATH_NOT_FOUND: &str = "Path not found";
pub const PATH_FILE_ACCESS: &str = "Path/File access error";
pub const PERMISSION_DENIED: &str = "Permission Denied";
pub const OUT_OF_MEMORY: &str = "Out of memory";
pub const ILLEGAL: &str = "Illegal function call";

const KEY1: [u8; 13] = [
    0x9A, 0xF7, 0x19, 0x83, 0x24, 0x63, 0x43, 0x83, 0x75, 0xCD, 0x8D, 0x84, 0xA9,
];
const KEY2: [u8; 11] = [
    0x7C, 0x88, 0x59, 0x74, 0xE0, 0x97, 0x26, 0x77, 0xC4, 0x1D, 0x1E,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeqMode {
    Input,
    Output,
    Append,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveKind {
    Tokenized,
    Ascii,
    Protected,
}

struct Channel {
    mode: SeqMode,
    data: Vec<u8>,
    pos: usize,
    path: PathBuf,
    file: Option<File>,
}

pub struct Files {
    max: usize,
    channels: Vec<Option<Channel>>,
}

impl Files {
    pub fn new(max_files: u32, _record_size: u32) -> Self {
        let max = (max_files as usize).min(15);
        Self {
            max,
            channels: (0..=max).map(|_| None).collect(),
        }
    }

    pub fn close_all(&mut self) {
        for slot in &mut self.channels {
            *slot = None;
        }
    }

    pub fn open(&mut self, number: i16, name: &str, mode: SeqMode) -> Result<(), &'static str> {
        let number = self.slot(number)?;
        if self.channels[number].is_some() {
            return Err(FILE_ALREADY_OPEN);
        }
        let path = plain_path(name)?;
        if self.channels.iter().flatten().any(|ch| ch.path == path) {
            return Err(FILE_ALREADY_OPEN);
        }
        let channel = match mode {
            SeqMode::Input => {
                let data = std::fs::read(&path).map_err(io_err)?;
                Channel {
                    mode,
                    pos: 0,
                    data,
                    path,
                    file: None,
                }
            }
            SeqMode::Output => {
                let file = File::create(&path).map_err(io_err)?;
                Channel {
                    mode,
                    data: Vec::new(),
                    pos: 0,
                    path,
                    file: Some(file),
                }
            }
            SeqMode::Append => {
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .read(true)
                    .open(&path)
                    .map_err(io_err)?;
                let mut data = Vec::new();
                file.read_to_end(&mut data).map_err(io_err)?;
                file.seek(SeekFrom::End(0)).map_err(io_err)?;
                let pos = data.len();
                Channel {
                    mode,
                    data,
                    pos,
                    path,
                    file: Some(file),
                }
            }
        };
        self.channels[number] = Some(channel);
        Ok(())
    }

    pub fn close(&mut self, number: i16) -> Result<(), &'static str> {
        let number = self.slot(number)?;
        self.channels[number] = None;
        Ok(())
    }

    pub fn write_text(&mut self, number: i16, text: &str) -> Result<(), &'static str> {
        let number = self.slot(number)?;
        let channel = self.channels[number].as_mut().ok_or(BAD_FILE_MODE)?;
        if channel.mode == SeqMode::Input {
            return Err(BAD_FILE_MODE);
        }
        let bytes = dos_newlines(text);
        let file = channel.file.as_mut().ok_or(BAD_FILE_MODE)?;
        file.write_all(&bytes).map_err(io_err)?;
        file.flush().map_err(io_err)?;
        channel.data.extend_from_slice(&bytes);
        channel.pos = channel.data.len();
        Ok(())
    }

    pub fn read_field(&mut self, number: i16) -> Result<String, &'static str> {
        let number = self.slot(number)?;
        let channel = self.channels[number].as_mut().ok_or(BAD_FILE_MODE)?;
        if channel.mode != SeqMode::Input {
            return Err(BAD_FILE_MODE);
        }
        if channel.pos >= channel.data.len() {
            return Err(INPUT_PAST_END);
        }
        let data = &channel.data;
        let mut i = channel.pos;
        let mut out = Vec::new();
        if data[i] == b'"' {
            i += 1;
            while i < data.len() && data[i] != b'"' {
                out.push(data[i]);
                i += 1;
            }
            if i < data.len() && data[i] == b'"' {
                i += 1;
            }
        } else {
            while i < data.len() && data[i] != b',' && data[i] != b'\r' && data[i] != b'\n' {
                out.push(data[i]);
                i += 1;
            }
        }
        if i < data.len() && data[i] == b',' {
            i += 1;
        } else {
            if i < data.len() && data[i] == b'\r' {
                i += 1;
            }
            if i < data.len() && data[i] == b'\n' {
                i += 1;
            }
        }
        channel.pos = i;
        Ok(out.into_iter().map(char::from).collect())
    }

    pub fn read_line(&mut self, number: i16) -> Result<String, &'static str> {
        let number = self.slot(number)?;
        let channel = self.channels[number].as_mut().ok_or(BAD_FILE_MODE)?;
        if channel.mode != SeqMode::Input {
            return Err(BAD_FILE_MODE);
        }
        if channel.pos >= channel.data.len() {
            return Err(INPUT_PAST_END);
        }
        let data = &channel.data;
        let mut i = channel.pos;
        let mut out = Vec::new();
        while i < data.len() && data[i] != b'\r' && data[i] != b'\n' {
            out.push(data[i]);
            i += 1;
        }
        if i < data.len() && data[i] == b'\r' {
            i += 1;
        }
        if i < data.len() && data[i] == b'\n' {
            i += 1;
        }
        channel.pos = i;
        Ok(out.into_iter().map(char::from).collect())
    }

    pub fn eof(&self, number: i16) -> Result<bool, &'static str> {
        let channel = self.get(number)?;
        if channel.mode != SeqMode::Input {
            return Err(BAD_FILE_MODE);
        }
        Ok(channel.pos >= channel.data.len())
    }

    pub fn lof(&self, number: i16) -> Result<i32, &'static str> {
        let channel = self.get(number)?;
        Ok(channel.data.len() as i32)
    }

    pub fn loc(&self, number: i16) -> Result<i32, &'static str> {
        let channel = self.get(number)?;
        Ok((channel.pos / 128) as i32)
    }

    fn slot(&self, number: i16) -> Result<usize, &'static str> {
        if number < 1 || number as usize > self.max {
            Err(BAD_FILE_NUMBER)
        } else {
            Ok(number as usize)
        }
    }

    fn get(&self, number: i16) -> Result<&Channel, &'static str> {
        let number = self.slot(number)?;
        self.channels[number].as_ref().ok_or(BAD_FILE_MODE)
    }
}

pub fn bas_path(name: &str) -> Result<PathBuf, &'static str> {
    let path = plain_path(name)?;
    if path.extension().is_some() {
        Ok(path)
    } else {
        Ok(path.with_extension("BAS"))
    }
}

pub fn plain_path(name: &str) -> Result<PathBuf, &'static str> {
    let name = name.trim();
    if name.is_empty()
        || name
            .bytes()
            .any(|b| matches!(b, b'*' | b'?' | b'"' | b'<' | b'>' | b'|') || b < 0x20)
    {
        return Err(BAD_FILE_NAME);
    }
    Ok(PathBuf::from(name))
}

pub fn encode_ascii(lines: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in lines {
        out.extend(line.bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.push(0x1A);
    out
}

pub fn decode_ascii(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let bytes = bytes.strip_suffix(&[0x1A]).unwrap_or(bytes);
    bytes.iter().copied().map(char::from).collect()
}

pub fn encode_tokenized(lines: &[(u16, &[u8])], protected: bool) -> Result<Vec<u8>, &'static str> {
    let mut body = Vec::new();
    let mut pos = 1usize;
    for (number, tokens) in lines {
        let mut tokens = tokens.to_vec();
        if tokens.last() != Some(&0) {
            tokens.push(0);
        }
        let next = pos + 4 + tokens.len();
        if next > 65535 {
            return Err(OUT_OF_MEMORY);
        }
        body.extend_from_slice(&(next as u16).to_le_bytes());
        body.extend_from_slice(&number.to_le_bytes());
        body.extend_from_slice(&tokens);
        pos = next;
    }
    body.extend_from_slice(&0u16.to_le_bytes());
    body.push(0x1A);
    if protected {
        encrypt(&mut body);
    }
    let mut out = Vec::with_capacity(body.len() + 1);
    out.push(if protected { 0xFE } else { 0xFF });
    out.extend(body);
    Ok(out)
}

pub struct TokenizedProgram {
    pub lines: Vec<(u16, Vec<u8>)>,
    pub protected: bool,
}

pub fn decode_tokenized(bytes: &[u8]) -> Result<TokenizedProgram, &'static str> {
    let protected = match bytes.first().copied() {
        Some(0xFF) => false,
        Some(0xFE) => true,
        _ => return Err(ILLEGAL),
    };
    let mut file = bytes.to_vec();
    if protected {
        decrypt(&mut file[1..]);
    }
    // Next-line pointers are offsets from the start of the file. Byte 0 is the magic.
    let mut i = 1usize;
    let mut lines = Vec::new();
    loop {
        if i + 1 >= file.len() {
            return Err(ILLEGAL);
        }
        let next = u16::from_le_bytes([file[i], file[i + 1]]) as usize;
        if next == 0 {
            break;
        }
        if next <= i + 4 || next > file.len() || i + 4 > file.len() {
            return Err(ILLEGAL);
        }
        let number = u16::from_le_bytes([file[i + 2], file[i + 3]]);
        if number > 65529 {
            return Err(ILLEGAL);
        }
        let tokens = file[i + 4..next].to_vec();
        if tokens.last() != Some(&0) {
            return Err(ILLEGAL);
        }
        lines.push((number, tokens));
        i = next;
    }
    Ok(TokenizedProgram { lines, protected })
}

pub fn read_bytes(path: &Path) -> Result<Vec<u8>, &'static str> {
    std::fs::read(path).map_err(io_err)
}

pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), &'static str> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            return Err(PATH_NOT_FOUND);
        }
    }
    std::fs::write(path, bytes).map_err(io_err)
}

pub fn kill(name: &str) -> Result<(), &'static str> {
    let path = plain_path(name)?;
    std::fs::remove_file(path).map_err(io_err)
}

pub fn rename_file(from: &str, to: &str) -> Result<(), &'static str> {
    let from = plain_path(from)?;
    let to = plain_path(to)?;
    if to.exists() {
        return Err(FILE_ALREADY_EXISTS);
    }
    std::fs::rename(from, to).map_err(io_err)
}

pub fn list_dir(spec: &str) -> Result<Vec<String>, &'static str> {
    let spec = if spec.trim().is_empty() {
        "*.*".to_string()
    } else {
        spec.trim().to_string()
    };
    let (dir, pat) = match spec.rsplit_once(['\\', '/']) {
        Some((dir, pat)) => (PathBuf::from(dir), pat.to_string()),
        None => (PathBuf::from("."), spec),
    };
    let entries = std::fs::read_dir(&dir).map_err(io_err)?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io_err)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if dos_match(&pat, &name) {
            names.push(name.to_ascii_uppercase());
        }
    }
    names.sort();
    Ok(names)
}

pub fn chdir(path: &str) -> Result<(), &'static str> {
    std::env::set_current_dir(plain_path(path)?).map_err(io_err)
}

pub fn mkdir(path: &str) -> Result<(), &'static str> {
    std::fs::create_dir(plain_path(path)?).map_err(io_err)
}

pub fn rmdir(path: &str) -> Result<(), &'static str> {
    std::fs::remove_dir(plain_path(path)?).map_err(io_err)
}

pub fn run_shell(command: &str, env: &[(String, String)]) -> Result<String, &'static str> {
    if command.trim().is_empty() {
        return Ok(String::new());
    }
    let (program, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    let output = std::process::Command::new(program)
        .arg(flag)
        .arg(command)
        .env_clear()
        .envs(
            env.iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        )
        .output()
        .map_err(|_| PATH_FILE_ACCESS)?;
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    if !output.status.success() {
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    Ok(text.replace("\r\n", "\n"))
}

fn io_err(err: std::io::Error) -> &'static str {
    match err.kind() {
        std::io::ErrorKind::NotFound => FILE_NOT_FOUND,
        std::io::ErrorKind::AlreadyExists => FILE_ALREADY_EXISTS,
        std::io::ErrorKind::PermissionDenied => PERMISSION_DENIED,
        _ => PATH_FILE_ACCESS,
    }
}

fn dos_newlines(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\r' => {
                out.push(b'\r');
                if chars.get(i + 1) == Some(&'\n') {
                    i += 1;
                }
                out.push(b'\n');
            }
            '\n' => {
                out.push(b'\r');
                out.push(b'\n');
            }
            ch if (ch as u32) <= 255 => out.push(ch as u8),
            _ => {}
        }
        i += 1;
    }
    out
}

fn dos_match(pat: &str, name: &str) -> bool {
    let pat = pat.as_bytes().to_ascii_uppercase();
    let name = name.as_bytes().to_ascii_uppercase();
    if pat == b"*.*" || pat == b"*" || pat.is_empty() {
        return true;
    }
    fn rec(pat: &[u8], name: &[u8]) -> bool {
        match (pat.first(), name.first()) {
            (None, None) => true,
            (Some(b'*'), _) => rec(&pat[1..], name) || (!name.is_empty() && rec(pat, &name[1..])),
            (Some(b'?'), Some(_)) => rec(&pat[1..], &name[1..]),
            (Some(a), Some(b)) if a == b => rec(&pat[1..], &name[1..]),
            _ => false,
        }
    }
    rec(&pat, &name)
}

fn decrypt(body: &mut [u8]) {
    let mut ch = 0x0Du8;
    let mut cl = 0x0Bu8;
    for byte in body.iter_mut() {
        let mixed = byte.wrapping_sub(cl) ^ KEY1[(ch - 1) as usize] ^ KEY2[(cl - 1) as usize];
        *byte = mixed.wrapping_add(ch);
        cl -= 1;
        if cl == 0 {
            cl = 0x0B;
        }
        ch -= 1;
        if ch == 0 {
            ch = 0x0D;
        }
    }
}

fn encrypt(body: &mut [u8]) {
    let mut ch = 0x0Du8;
    let mut cl = 0x0Bu8;
    for byte in body.iter_mut() {
        let mixed = byte.wrapping_sub(ch) ^ KEY1[(ch - 1) as usize] ^ KEY2[(cl - 1) as usize];
        *byte = mixed.wrapping_add(cl);
        cl -= 1;
        if cl == 0 {
            cl = 0x0B;
        }
        ch -= 1;
        if ch == 0 {
            ch = 0x0D;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protection_round_trips_a_tokenized_line() {
        let lines = [(10u16, [0x91u8, 0x00].as_slice())];
        let plain = encode_tokenized(&lines, false).unwrap();
        assert_eq!(plain[0], 0xFF);
        assert_eq!(*plain.last().unwrap(), 0x1A);
        let back = decode_tokenized(&plain).unwrap();
        assert!(!back.protected);
        assert_eq!(back.lines, vec![(10, vec![0x91, 0x00])]);

        let hidden = encode_tokenized(&lines, true).unwrap();
        assert_eq!(hidden[0], 0xFE);
        assert_ne!(hidden, plain);
        let back = decode_tokenized(&hidden).unwrap();
        assert!(back.protected);
        assert_eq!(back.lines[0].1, vec![0x91, 0x00]);
    }

    #[test]
    fn a_truncated_tokenized_file_is_rejected() {
        assert!(decode_tokenized(&[0xFF, 0x01, 0x00]).is_err());
    }
}
