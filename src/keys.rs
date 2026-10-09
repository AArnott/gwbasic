//! Soft-key macros. Each of F1–F10 holds at most 15 bytes. A trailing CR
//! (shown as a left arrow, CP437 0x1B) means the editor submits the line.

pub const CR_GLYPH: u8 = 0x1B;
pub const MACRO_LEN: usize = 15;

#[derive(Clone, Debug)]
pub struct FunctionKeys {
    macros: [Vec<u8>; 10],
    pub on: bool,
    /// Leftmost soft key when the width cannot show all ten (width 40).
    pub offset: usize,
}

impl FunctionKeys {
    pub fn standard() -> Self {
        let mut keys = Self {
            macros: Default::default(),
            on: true,
            offset: 0,
        };
        keys.macros[0] = b"LIST".to_vec();
        keys.macros[1] = with_cr("RUN");
        keys.macros[2] = b"LOAD\"".to_vec();
        keys.macros[3] = b"SAVE\"".to_vec();
        keys.macros[4] = with_cr("CONT");
        keys.macros[5] = with_cr(",\"LPT1:\"");
        keys.macros[6] = with_cr("TRON");
        keys.macros[7] = with_cr("TROFF");
        keys.macros[8] = b"KEY".to_vec();
        keys.macros[9] = with_cr("SCREEN 0,0,0");
        keys
    }

    pub fn get(&self, n: u8) -> &[u8] {
        &self.macros[index(n)]
    }

    pub fn set(&mut self, n: u8, bytes: &[u8]) {
        let mut v = bytes.to_vec();
        v.truncate(MACRO_LEN);
        self.macros[index(n)] = v;
    }

    /// Digit + first six macro characters, 8 columns per key.
    /// Width 80 shows all ten. Width 40 shows five, starting at `offset`.
    pub fn strip(&self, width: usize) -> Vec<u8> {
        let slots = width / 8;
        let mut out = vec![b' '; width];
        for i in 0..slots {
            let key = (self.offset + i) % 10;
            let pos = i * 8;
            out[pos] = if key == 9 { b'0' } else { b'1' + key as u8 };
            for (j, &b) in self.display(key).iter().take(6).enumerate() {
                out[pos + 1 + j] = b;
            }
        }
        out
    }

    pub fn list_lines(&self) -> Vec<Vec<u8>> {
        (0..10)
            .map(|i| {
                let mut line = format!("F{} ", i + 1).into_bytes();
                line.extend(self.display(i));
                line
            })
            .collect()
    }

    fn display(&self, index: usize) -> Vec<u8> {
        self.macros[index]
            .iter()
            .copied()
            .map(display_byte)
            .collect()
    }
}

impl Default for FunctionKeys {
    fn default() -> Self {
        Self::standard()
    }
}

fn index(n: u8) -> usize {
    (n.clamp(1, 10) - 1) as usize
}

fn with_cr(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(b'\r');
    v
}

fn display_byte(b: u8) -> u8 {
    if b == b'\r' || b == b'\n' {
        CR_GLYPH
    } else if b < 0x20 || b == 0x7F {
        b'.'
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_manual() {
        let keys = FunctionKeys::standard();
        assert_eq!(keys.get(1), b"LIST");
        assert_eq!(keys.get(2), b"RUN\r");
        assert_eq!(keys.get(3), b"LOAD\"");
        assert_eq!(keys.get(10), b"SCREEN 0,0,0\r");
        let strip = keys.strip(80);
        assert_eq!(&strip[0..5], b"1LIST");
        assert_eq!(strip[8], b'2');
        assert_eq!(&strip[9..12], b"RUN");
        assert_eq!(strip[12], CR_GLYPH);
        assert_eq!(strip[72], b'0');
        assert_eq!(&strip[73..79], b"SCREEN");
    }

    #[test]
    fn width_40_scrolls_five_keys() {
        let mut keys = FunctionKeys::standard();
        assert_eq!(keys.strip(40)[0], b'1');
        keys.offset = 1;
        assert_eq!(keys.strip(40)[0], b'2');
        assert_eq!(keys.strip(40).len(), 40);
    }

    #[test]
    fn set_truncates_to_15() {
        let mut keys = FunctionKeys::standard();
        keys.set(1, b"0123456789ABCDEF");
        assert_eq!(keys.get(1).len(), 15);
    }
}
