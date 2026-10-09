//! GW-BASIC keyword tokens. One-byte codes are 0x81–0xF4. Two-byte codes
//! follow an 0xFD, 0xFE, or 0xFF prefix. The names and codes are the 3.23 set.

#[derive(Clone, Copy)]
pub struct Keyword {
    pub name: &'static str,
    pub prefix: u8,
    pub code: u8,
}

pub const T_OCT: u8 = 0x0B;
pub const T_HEX: u8 = 0x0C;
pub const T_LINE: u8 = 0x0E;
pub const T_BYTE: u8 = 0x0F;
pub const T_INT0: u8 = 0x11;
pub const T_I16: u8 = 0x1C;
pub const T_SNG: u8 = 0x1D;
pub const T_DBL: u8 = 0x1F;
pub const T_QUOTE: u8 = 0x22;
pub const T_COLON: u8 = 0x3A;
pub const T_REM: u8 = 0x8F;
pub const T_DATA: u8 = 0x84;
pub const T_ELSE: u8 = 0xA1;
pub const T_WHILE: u8 = 0xB1;
pub const T_TICK: u8 = 0xD9;
pub const T_PLUS: u8 = 0xE9;

const KEYWORDS: &[Keyword] = &[
    Keyword {
        name: "END",
        prefix: 0,
        code: 0x81,
    },
    Keyword {
        name: "FOR",
        prefix: 0,
        code: 0x82,
    },
    Keyword {
        name: "NEXT",
        prefix: 0,
        code: 0x83,
    },
    Keyword {
        name: "DATA",
        prefix: 0,
        code: 0x84,
    },
    Keyword {
        name: "INPUT",
        prefix: 0,
        code: 0x85,
    },
    Keyword {
        name: "DIM",
        prefix: 0,
        code: 0x86,
    },
    Keyword {
        name: "READ",
        prefix: 0,
        code: 0x87,
    },
    Keyword {
        name: "LET",
        prefix: 0,
        code: 0x88,
    },
    Keyword {
        name: "GOTO",
        prefix: 0,
        code: 0x89,
    },
    Keyword {
        name: "RUN",
        prefix: 0,
        code: 0x8A,
    },
    Keyword {
        name: "IF",
        prefix: 0,
        code: 0x8B,
    },
    Keyword {
        name: "RESTORE",
        prefix: 0,
        code: 0x8C,
    },
    Keyword {
        name: "GOSUB",
        prefix: 0,
        code: 0x8D,
    },
    Keyword {
        name: "RETURN",
        prefix: 0,
        code: 0x8E,
    },
    Keyword {
        name: "REM",
        prefix: 0,
        code: 0x8F,
    },
    Keyword {
        name: "STOP",
        prefix: 0,
        code: 0x90,
    },
    Keyword {
        name: "PRINT",
        prefix: 0,
        code: 0x91,
    },
    Keyword {
        name: "CLEAR",
        prefix: 0,
        code: 0x92,
    },
    Keyword {
        name: "LIST",
        prefix: 0,
        code: 0x93,
    },
    Keyword {
        name: "NEW",
        prefix: 0,
        code: 0x94,
    },
    Keyword {
        name: "ON",
        prefix: 0,
        code: 0x95,
    },
    Keyword {
        name: "WAIT",
        prefix: 0,
        code: 0x96,
    },
    Keyword {
        name: "DEF",
        prefix: 0,
        code: 0x97,
    },
    Keyword {
        name: "POKE",
        prefix: 0,
        code: 0x98,
    },
    Keyword {
        name: "CONT",
        prefix: 0,
        code: 0x99,
    },
    Keyword {
        name: "OUT",
        prefix: 0,
        code: 0x9C,
    },
    Keyword {
        name: "LPRINT",
        prefix: 0,
        code: 0x9D,
    },
    Keyword {
        name: "LLIST",
        prefix: 0,
        code: 0x9E,
    },
    Keyword {
        name: "WIDTH",
        prefix: 0,
        code: 0xA0,
    },
    Keyword {
        name: "ELSE",
        prefix: 0,
        code: 0xA1,
    },
    Keyword {
        name: "TRON",
        prefix: 0,
        code: 0xA2,
    },
    Keyword {
        name: "TROFF",
        prefix: 0,
        code: 0xA3,
    },
    Keyword {
        name: "SWAP",
        prefix: 0,
        code: 0xA4,
    },
    Keyword {
        name: "ERASE",
        prefix: 0,
        code: 0xA5,
    },
    Keyword {
        name: "EDIT",
        prefix: 0,
        code: 0xA6,
    },
    Keyword {
        name: "ERROR",
        prefix: 0,
        code: 0xA7,
    },
    Keyword {
        name: "RESUME",
        prefix: 0,
        code: 0xA8,
    },
    Keyword {
        name: "DELETE",
        prefix: 0,
        code: 0xA9,
    },
    Keyword {
        name: "AUTO",
        prefix: 0,
        code: 0xAA,
    },
    Keyword {
        name: "RENUM",
        prefix: 0,
        code: 0xAB,
    },
    Keyword {
        name: "DEFSTR",
        prefix: 0,
        code: 0xAC,
    },
    Keyword {
        name: "DEFINT",
        prefix: 0,
        code: 0xAD,
    },
    Keyword {
        name: "DEFSNG",
        prefix: 0,
        code: 0xAE,
    },
    Keyword {
        name: "DEFDBL",
        prefix: 0,
        code: 0xAF,
    },
    Keyword {
        name: "LINE",
        prefix: 0,
        code: 0xB0,
    },
    Keyword {
        name: "WHILE",
        prefix: 0,
        code: 0xB1,
    },
    Keyword {
        name: "WEND",
        prefix: 0,
        code: 0xB2,
    },
    Keyword {
        name: "CALL",
        prefix: 0,
        code: 0xB3,
    },
    Keyword {
        name: "WRITE",
        prefix: 0,
        code: 0xB7,
    },
    Keyword {
        name: "OPTION",
        prefix: 0,
        code: 0xB8,
    },
    Keyword {
        name: "RANDOMIZE",
        prefix: 0,
        code: 0xB9,
    },
    Keyword {
        name: "OPEN",
        prefix: 0,
        code: 0xBA,
    },
    Keyword {
        name: "CLOSE",
        prefix: 0,
        code: 0xBB,
    },
    Keyword {
        name: "LOAD",
        prefix: 0,
        code: 0xBC,
    },
    Keyword {
        name: "MERGE",
        prefix: 0,
        code: 0xBD,
    },
    Keyword {
        name: "SAVE",
        prefix: 0,
        code: 0xBE,
    },
    Keyword {
        name: "COLOR",
        prefix: 0,
        code: 0xBF,
    },
    Keyword {
        name: "CLS",
        prefix: 0,
        code: 0xC0,
    },
    Keyword {
        name: "MOTOR",
        prefix: 0,
        code: 0xC1,
    },
    Keyword {
        name: "BSAVE",
        prefix: 0,
        code: 0xC2,
    },
    Keyword {
        name: "BLOAD",
        prefix: 0,
        code: 0xC3,
    },
    Keyword {
        name: "SOUND",
        prefix: 0,
        code: 0xC4,
    },
    Keyword {
        name: "BEEP",
        prefix: 0,
        code: 0xC5,
    },
    Keyword {
        name: "PSET",
        prefix: 0,
        code: 0xC6,
    },
    Keyword {
        name: "PRESET",
        prefix: 0,
        code: 0xC7,
    },
    Keyword {
        name: "SCREEN",
        prefix: 0,
        code: 0xC8,
    },
    Keyword {
        name: "KEY",
        prefix: 0,
        code: 0xC9,
    },
    Keyword {
        name: "LOCATE",
        prefix: 0,
        code: 0xCA,
    },
    Keyword {
        name: "TO",
        prefix: 0,
        code: 0xCC,
    },
    Keyword {
        name: "THEN",
        prefix: 0,
        code: 0xCD,
    },
    Keyword {
        name: "TAB(",
        prefix: 0,
        code: 0xCE,
    },
    Keyword {
        name: "STEP",
        prefix: 0,
        code: 0xCF,
    },
    Keyword {
        name: "USR",
        prefix: 0,
        code: 0xD0,
    },
    Keyword {
        name: "FN",
        prefix: 0,
        code: 0xD1,
    },
    Keyword {
        name: "SPC(",
        prefix: 0,
        code: 0xD2,
    },
    Keyword {
        name: "NOT",
        prefix: 0,
        code: 0xD3,
    },
    Keyword {
        name: "ERL",
        prefix: 0,
        code: 0xD4,
    },
    Keyword {
        name: "ERR",
        prefix: 0,
        code: 0xD5,
    },
    Keyword {
        name: "STRING$",
        prefix: 0,
        code: 0xD6,
    },
    Keyword {
        name: "USING",
        prefix: 0,
        code: 0xD7,
    },
    Keyword {
        name: "INSTR",
        prefix: 0,
        code: 0xD8,
    },
    Keyword {
        name: "'",
        prefix: 0,
        code: 0xD9,
    },
    Keyword {
        name: "VARPTR",
        prefix: 0,
        code: 0xDA,
    },
    Keyword {
        name: "CSRLIN",
        prefix: 0,
        code: 0xDB,
    },
    Keyword {
        name: "POINT",
        prefix: 0,
        code: 0xDC,
    },
    Keyword {
        name: "OFF",
        prefix: 0,
        code: 0xDD,
    },
    Keyword {
        name: "INKEY$",
        prefix: 0,
        code: 0xDE,
    },
    Keyword {
        name: ">",
        prefix: 0,
        code: 0xE6,
    },
    Keyword {
        name: "=",
        prefix: 0,
        code: 0xE7,
    },
    Keyword {
        name: "<",
        prefix: 0,
        code: 0xE8,
    },
    Keyword {
        name: "+",
        prefix: 0,
        code: 0xE9,
    },
    Keyword {
        name: "-",
        prefix: 0,
        code: 0xEA,
    },
    Keyword {
        name: "*",
        prefix: 0,
        code: 0xEB,
    },
    Keyword {
        name: "/",
        prefix: 0,
        code: 0xEC,
    },
    Keyword {
        name: "^",
        prefix: 0,
        code: 0xED,
    },
    Keyword {
        name: "AND",
        prefix: 0,
        code: 0xEE,
    },
    Keyword {
        name: "OR",
        prefix: 0,
        code: 0xEF,
    },
    Keyword {
        name: "XOR",
        prefix: 0,
        code: 0xF0,
    },
    Keyword {
        name: "EQV",
        prefix: 0,
        code: 0xF1,
    },
    Keyword {
        name: "IMP",
        prefix: 0,
        code: 0xF2,
    },
    Keyword {
        name: "MOD",
        prefix: 0,
        code: 0xF3,
    },
    Keyword {
        name: "\\",
        prefix: 0,
        code: 0xF4,
    },
    Keyword {
        name: "CVI",
        prefix: 0xFD,
        code: 0x81,
    },
    Keyword {
        name: "CVS",
        prefix: 0xFD,
        code: 0x82,
    },
    Keyword {
        name: "CVD",
        prefix: 0xFD,
        code: 0x83,
    },
    Keyword {
        name: "MKI$",
        prefix: 0xFD,
        code: 0x84,
    },
    Keyword {
        name: "MKS$",
        prefix: 0xFD,
        code: 0x85,
    },
    Keyword {
        name: "MKD$",
        prefix: 0xFD,
        code: 0x86,
    },
    Keyword {
        name: "EXTERR",
        prefix: 0xFD,
        code: 0x8B,
    },
    Keyword {
        name: "FILES",
        prefix: 0xFE,
        code: 0x81,
    },
    Keyword {
        name: "FIELD",
        prefix: 0xFE,
        code: 0x82,
    },
    Keyword {
        name: "SYSTEM",
        prefix: 0xFE,
        code: 0x83,
    },
    Keyword {
        name: "NAME",
        prefix: 0xFE,
        code: 0x84,
    },
    Keyword {
        name: "LSET",
        prefix: 0xFE,
        code: 0x85,
    },
    Keyword {
        name: "RSET",
        prefix: 0xFE,
        code: 0x86,
    },
    Keyword {
        name: "KILL",
        prefix: 0xFE,
        code: 0x87,
    },
    Keyword {
        name: "PUT",
        prefix: 0xFE,
        code: 0x88,
    },
    Keyword {
        name: "GET",
        prefix: 0xFE,
        code: 0x89,
    },
    Keyword {
        name: "RESET",
        prefix: 0xFE,
        code: 0x8A,
    },
    Keyword {
        name: "COMMON",
        prefix: 0xFE,
        code: 0x8B,
    },
    Keyword {
        name: "CHAIN",
        prefix: 0xFE,
        code: 0x8C,
    },
    Keyword {
        name: "DATE$",
        prefix: 0xFE,
        code: 0x8D,
    },
    Keyword {
        name: "TIME$",
        prefix: 0xFE,
        code: 0x8E,
    },
    Keyword {
        name: "PAINT",
        prefix: 0xFE,
        code: 0x8F,
    },
    Keyword {
        name: "COM",
        prefix: 0xFE,
        code: 0x90,
    },
    Keyword {
        name: "CIRCLE",
        prefix: 0xFE,
        code: 0x91,
    },
    Keyword {
        name: "DRAW",
        prefix: 0xFE,
        code: 0x92,
    },
    Keyword {
        name: "PLAY",
        prefix: 0xFE,
        code: 0x93,
    },
    Keyword {
        name: "TIMER",
        prefix: 0xFE,
        code: 0x94,
    },
    Keyword {
        name: "ERDEV",
        prefix: 0xFE,
        code: 0x95,
    },
    Keyword {
        name: "IOCTL",
        prefix: 0xFE,
        code: 0x96,
    },
    Keyword {
        name: "CHDIR",
        prefix: 0xFE,
        code: 0x97,
    },
    Keyword {
        name: "MKDIR",
        prefix: 0xFE,
        code: 0x98,
    },
    Keyword {
        name: "RMDIR",
        prefix: 0xFE,
        code: 0x99,
    },
    Keyword {
        name: "SHELL",
        prefix: 0xFE,
        code: 0x9A,
    },
    Keyword {
        name: "ENVIRON",
        prefix: 0xFE,
        code: 0x9B,
    },
    Keyword {
        name: "VIEW",
        prefix: 0xFE,
        code: 0x9C,
    },
    Keyword {
        name: "WINDOW",
        prefix: 0xFE,
        code: 0x9D,
    },
    Keyword {
        name: "PMAP",
        prefix: 0xFE,
        code: 0x9E,
    },
    Keyword {
        name: "PALETTE",
        prefix: 0xFE,
        code: 0x9F,
    },
    Keyword {
        name: "LCOPY",
        prefix: 0xFE,
        code: 0xA0,
    },
    Keyword {
        name: "CALLS",
        prefix: 0xFE,
        code: 0xA1,
    },
    Keyword {
        name: "NOISE",
        prefix: 0xFE,
        code: 0xA4,
    },
    Keyword {
        name: "PCOPY",
        prefix: 0xFE,
        code: 0xA5,
    },
    Keyword {
        name: "TERM",
        prefix: 0xFE,
        code: 0xA6,
    },
    Keyword {
        name: "LOCK",
        prefix: 0xFE,
        code: 0xA7,
    },
    Keyword {
        name: "UNLOCK",
        prefix: 0xFE,
        code: 0xA8,
    },
    Keyword {
        name: "LEFT$",
        prefix: 0xFF,
        code: 0x81,
    },
    Keyword {
        name: "RIGHT$",
        prefix: 0xFF,
        code: 0x82,
    },
    Keyword {
        name: "MID$",
        prefix: 0xFF,
        code: 0x83,
    },
    Keyword {
        name: "SGN",
        prefix: 0xFF,
        code: 0x84,
    },
    Keyword {
        name: "INT",
        prefix: 0xFF,
        code: 0x85,
    },
    Keyword {
        name: "ABS",
        prefix: 0xFF,
        code: 0x86,
    },
    Keyword {
        name: "SQR",
        prefix: 0xFF,
        code: 0x87,
    },
    Keyword {
        name: "RND",
        prefix: 0xFF,
        code: 0x88,
    },
    Keyword {
        name: "SIN",
        prefix: 0xFF,
        code: 0x89,
    },
    Keyword {
        name: "LOG",
        prefix: 0xFF,
        code: 0x8A,
    },
    Keyword {
        name: "EXP",
        prefix: 0xFF,
        code: 0x8B,
    },
    Keyword {
        name: "COS",
        prefix: 0xFF,
        code: 0x8C,
    },
    Keyword {
        name: "TAN",
        prefix: 0xFF,
        code: 0x8D,
    },
    Keyword {
        name: "ATN",
        prefix: 0xFF,
        code: 0x8E,
    },
    Keyword {
        name: "FRE",
        prefix: 0xFF,
        code: 0x8F,
    },
    Keyword {
        name: "INP",
        prefix: 0xFF,
        code: 0x90,
    },
    Keyword {
        name: "POS",
        prefix: 0xFF,
        code: 0x91,
    },
    Keyword {
        name: "LEN",
        prefix: 0xFF,
        code: 0x92,
    },
    Keyword {
        name: "STR$",
        prefix: 0xFF,
        code: 0x93,
    },
    Keyword {
        name: "VAL",
        prefix: 0xFF,
        code: 0x94,
    },
    Keyword {
        name: "ASC",
        prefix: 0xFF,
        code: 0x95,
    },
    Keyword {
        name: "CHR$",
        prefix: 0xFF,
        code: 0x96,
    },
    Keyword {
        name: "PEEK",
        prefix: 0xFF,
        code: 0x97,
    },
    Keyword {
        name: "SPACE$",
        prefix: 0xFF,
        code: 0x98,
    },
    Keyword {
        name: "OCT$",
        prefix: 0xFF,
        code: 0x99,
    },
    Keyword {
        name: "HEX$",
        prefix: 0xFF,
        code: 0x9A,
    },
    Keyword {
        name: "LPOS",
        prefix: 0xFF,
        code: 0x9B,
    },
    Keyword {
        name: "CINT",
        prefix: 0xFF,
        code: 0x9C,
    },
    Keyword {
        name: "CSNG",
        prefix: 0xFF,
        code: 0x9D,
    },
    Keyword {
        name: "CDBL",
        prefix: 0xFF,
        code: 0x9E,
    },
    Keyword {
        name: "FIX",
        prefix: 0xFF,
        code: 0x9F,
    },
    Keyword {
        name: "PEN",
        prefix: 0xFF,
        code: 0xA0,
    },
    Keyword {
        name: "STICK",
        prefix: 0xFF,
        code: 0xA1,
    },
    Keyword {
        name: "STRIG",
        prefix: 0xFF,
        code: 0xA2,
    },
    Keyword {
        name: "EOF",
        prefix: 0xFF,
        code: 0xA3,
    },
    Keyword {
        name: "LOC",
        prefix: 0xFF,
        code: 0xA4,
    },
    Keyword {
        name: "LOF",
        prefix: 0xFF,
        code: 0xA5,
    },
];

pub fn find_keyword(src: &[u8], index: usize) -> Option<&'static Keyword> {
    let mut best: Option<&Keyword> = None;
    for kw in KEYWORDS {
        let name = kw.name.as_bytes();
        if src.len().saturating_sub(index) < name.len() {
            continue;
        }
        if !src[index..index + name.len()].eq_ignore_ascii_case(name) {
            continue;
        }
        if !boundary(src, index + name.len(), kw.name) {
            continue;
        }
        let longer = best.is_none_or(|have| kw.name.len() > have.name.len());
        if longer {
            best = Some(kw);
        }
    }
    best
}

pub fn keyword_name(prefix: u8, code: u8) -> Option<&'static str> {
    KEYWORDS
        .iter()
        .find(|kw| kw.prefix == prefix && kw.code == code)
        .map(|kw| kw.name)
}

fn boundary(src: &[u8], next: usize, name: &str) -> bool {
    let bytes = name.as_bytes();
    let word = bytes.first().is_some_and(|c| c.is_ascii_alphabetic());
    if !word {
        return true;
    }
    let glued = bytes.last().is_some_and(|c| *c == b'$' || *c == b'(');
    if glued {
        return true;
    }
    next >= src.len() || !src[next].is_ascii_alphanumeric()
}

pub fn emit_keyword(out: &mut Vec<u8>, kw: &Keyword) {
    if kw.prefix != 0 {
        out.push(kw.prefix);
    }
    out.push(kw.code);
}

pub fn is_word(name: &str) -> bool {
    name.bytes().next().is_some_and(|c| c.is_ascii_alphabetic())
}
