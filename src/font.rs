//! 8×16 code-page glyphs. ASCII comes from a public-domain 8×8 VGA font,
//! doubled vertically. Byte 0x1B is the CP437 left arrow used on the soft-key line.

include!("font_basic.rs");

/// font8x8 stores the leftmost pixel in the least significant bit.
pub fn pixel_on(row: u8, x: usize) -> bool {
    x < 8 && row & (1 << x) != 0
}

pub fn glyph(ch: u8) -> [u8; 16] {
    let src = if ch < 128 {
        BASIC_8X8[ch as usize]
    } else {
        placeholder(ch)
    };
    let mut out = [0u8; 16];
    for (i, row) in src.iter().enumerate() {
        out[i * 2] = *row;
        out[i * 2 + 1] = *row;
    }
    out
}

fn placeholder(ch: u8) -> [u8; 8] {
    // A framed box so bytes above 127 are visible until a full CP437 face is embedded.
    let mut g = [0u8; 8];
    g[0] = 0x7E;
    g[1] = 0x42;
    g[2] = 0x42 | ((ch >> 4) & 0x0F);
    g[3] = 0x42 | (ch & 0x0F);
    g[4] = g[2];
    g[5] = g[3];
    g[6] = 0x42;
    g[7] = 0x7E;
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_are_not_mirrored() {
        // 'F' has a bar across the top and a stem on the left.
        let f = glyph(b'F');
        assert!(pixel_on(f[0], 0));
        assert!(!pixel_on(f[0], 7));
        assert!(pixel_on(f[2], 1));
        assert!(!pixel_on(f[2], 7));

        // The soft-key return mark points left: its widest row reaches column 0.
        let arrow = glyph(0x1B);
        assert!(pixel_on(arrow[6], 0));
        assert!(!pixel_on(arrow[6], 7));
        assert!(!pixel_on(arrow[2], 0));
    }
}
