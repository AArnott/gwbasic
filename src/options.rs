//! `gwbasic [filename] [/F:n] [/I] [/S:n] [/C:n] [/M:[n][,n]] [/D]`
//!
//! Switches follow chapter 2 of the User's Guide. Numbers may be decimal,
//! `&H` hex, or `&O` octal. A filename means "load this program".

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Options {
    pub program: Option<PathBuf>,
    pub files: u32,
    pub static_files: bool,
    pub record_size: u32,
    pub com_buffer: Option<u32>,
    pub max_memory: Option<u32>,
    pub block_size: Option<u32>,
    pub double_math: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            program: None,
            files: 3,
            static_files: false,
            record_size: 128,
            com_buffer: None,
            max_memory: None,
            block_size: None,
            double_math: false,
        }
    }
}

pub fn parse_args(args: &[String]) -> Result<Options, String> {
    if args
        .iter()
        .any(|a| a == "/?" || a == "-h" || a.eq_ignore_ascii_case("--help"))
    {
        return Err(usage().to_string());
    }
    let mut opts = Options::default();
    for arg in args {
        if let Some(rest) = strip_switch(arg) {
            apply_switch(&mut opts, rest)?;
        } else if opts.program.is_some() {
            return Err(format!("unexpected argument '{arg}'"));
        } else {
            opts.program = Some(PathBuf::from(arg));
        }
    }
    Ok(opts)
}

pub fn usage() -> &'static str {
    "\
gwbasic [filename] [/F:n] [/I] [/S:n] [/C:n] [/M:n[,block]] [/D]

  filename   program to load (.BAS assumed if omitted). ASCII, tokenized, or protected
  /F:n       maximum open files (default 3, capped at 15)
  /I         accepted; the file table is sized by /F either way
  /S:n       random-file record length (default 128; random files are not implemented)
  /C:n       communications receive buffer (accepted, serial I/O is not wired up)
  /M:n[,m]   workspace size in bytes, capped at 65536, and reserved block size
  /D         SIN, COS, TAN, ATN, LOG, EXP, and SQR return doubles"
}

fn strip_switch(arg: &str) -> Option<&str> {
    arg.strip_prefix('/')
        .or_else(|| arg.strip_prefix('-'))
        .filter(|s| !s.is_empty())
}

fn apply_switch(opts: &mut Options, body: &str) -> Result<(), String> {
    let (name, value) = match body.split_once(':') {
        Some((n, v)) => (n, Some(v)),
        None => (body, None),
    };
    match name.to_ascii_uppercase().as_str() {
        "F" => opts.files = required_num(value, "F")?,
        "I" => {
            reject_value(value, "I")?;
            opts.static_files = true;
        }
        "S" => opts.record_size = required_num(value, "S")?,
        "C" => opts.com_buffer = Some(required_num(value, "C")?),
        "D" => {
            reject_value(value, "D")?;
            opts.double_math = true;
        }
        "M" => {
            let value = value.ok_or_else(|| "/M needs a value".to_string())?;
            let (first, second) = match value.split_once(',') {
                Some((a, b)) => (a, Some(b)),
                None => (value, None),
            };
            if !first.is_empty() {
                opts.max_memory =
                    Some(parse_num(first).ok_or_else(|| format!("bad /M value '{first}'"))?);
            }
            if let Some(second) = second.filter(|s| !s.is_empty()) {
                opts.block_size =
                    Some(parse_num(second).ok_or_else(|| format!("bad /M block '{second}'"))?);
            }
        }
        other => return Err(format!("unknown switch '/{other}'")),
    }
    Ok(())
}

fn required_num(value: Option<&str>, name: &str) -> Result<u32, String> {
    let value = value.ok_or_else(|| format!("/{name} needs a value"))?;
    parse_num(value).ok_or_else(|| format!("bad /{name} value '{value}'"))
}

fn reject_value(value: Option<&str>, name: &str) -> Result<(), String> {
    if value.is_some() {
        Err(format!("/{name} does not take a value"))
    } else {
        Ok(())
    }
}

fn parse_num(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("&H").or_else(|| s.strip_prefix("&h")) {
        u32::from_str_radix(rest, 16).ok()
    } else if let Some(rest) = s.strip_prefix("&O").or_else(|| s.strip_prefix("&o")) {
        u32::from_str_radix(rest, 8).ok()
    } else {
        s.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn parses_filename_and_switches() {
        let opts = parse_args(&args(&[
            "PAYROLL",
            "/F:6",
            "/S:&H200",
            "/D",
            "/M:32768,4096",
        ]))
        .unwrap();
        assert_eq!(opts.program.unwrap().to_string_lossy(), "PAYROLL");
        assert_eq!(opts.files, 6);
        assert_eq!(opts.record_size, 0x200);
        assert!(opts.double_math);
        assert_eq!(opts.max_memory, Some(32768));
        assert_eq!(opts.block_size, Some(4096));
    }

    #[test]
    fn octal_and_static_files() {
        let opts = parse_args(&args(&["/I", "/C:&O10"])).unwrap();
        assert!(opts.static_files);
        assert_eq!(opts.com_buffer, Some(8));
        assert!(opts.program.is_none());
    }

    #[test]
    fn rejects_unknown_switch() {
        assert!(parse_args(&args(&["/Z"])).is_err());
    }
}
