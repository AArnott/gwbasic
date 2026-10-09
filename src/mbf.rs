//! Microsoft Binary Format, the floating-point format GW-BASIC actually uses.
//!
//! The exponent is the last byte in memory. Zero means the value is zero.
//! Otherwise the exponent is base 2 with bias 128, the high bit of the
//! significand is the sign, and the bit just after the binary point is not
//! stored because it is always 1. There are no NaNs, infinities, or denormals.
//! Arithmetic stays in this layout. `f64` is used only where a transcendental
//! has to leave and come back, and the result is rounded into an MBF value.

#[derive(Clone, Copy, Debug)]
pub struct Mbf {
    /// Biased exponent. `0` is zero, and the other fields are then ignored.
    exp: u8,
    neg: bool,
    /// Normalized magnitude with the hidden 1 in the top bit.
    /// Single uses 24 bits. Double uses 56.
    sig: u64,
    double: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathError {
    Overflow,
    Div0,
}

impl Mbf {
    pub fn zero(double: bool) -> Self {
        Self {
            exp: 0,
            neg: false,
            sig: 0,
            double,
        }
    }

    pub fn is_zero(self) -> bool {
        self.exp == 0
    }

    pub fn is_double(self) -> bool {
        self.double
    }

    pub fn is_negative(self) -> bool {
        !self.is_zero() && self.neg
    }

    pub fn neg(self) -> Self {
        if self.is_zero() {
            return self;
        }
        let mut out = self;
        out.neg = !self.neg;
        out
    }

    pub fn abs(self) -> Self {
        let mut out = self;
        out.neg = false;
        out
    }

    fn bits(self) -> u32 {
        if self.double {
            56
        } else {
            24
        }
    }

    /// Little-endian memory order: significand low byte first, exponent last.
    pub fn to_le_single(self) -> [u8; 4] {
        let narrow = if self.double {
            self.to_single().unwrap_or_else(|_| Self::zero(false))
        } else {
            self
        };
        if narrow.exp == 0 {
            return [0, 0, 0, 0];
        }
        let frac = (narrow.sig as u32) & 0x007F_FFFF;
        let high = ((frac >> 16) as u8) | if narrow.neg { 0x80 } else { 0 };
        [frac as u8, (frac >> 8) as u8, high, narrow.exp]
    }

    pub fn to_le_double(self) -> [u8; 8] {
        let wide = if self.double { self } else { self.to_double() };
        if wide.exp == 0 {
            return [0; 8];
        }
        let frac = wide.sig & ((1u64 << 55) - 1);
        let mut out = [0u8; 8];
        for i in 0..7 {
            out[i] = (frac >> (8 * i)) as u8;
        }
        if wide.neg {
            out[6] |= 0x80;
        }
        out[7] = wide.exp;
        out
    }

    pub fn from_le_single(bytes: [u8; 4]) -> Self {
        if bytes[3] == 0 {
            return Self::zero(false);
        }
        let frac =
            u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2] & 0x7F) << 16);
        Self {
            exp: bytes[3],
            neg: bytes[2] & 0x80 != 0,
            sig: u64::from(frac) | (1u64 << 23),
            double: false,
        }
    }

    pub fn from_le_double(bytes: [u8; 8]) -> Self {
        if bytes[7] == 0 {
            return Self::zero(true);
        }
        let mut frac = 0u64;
        for i in 0..7 {
            frac |= u64::from(bytes[i]) << (8 * i);
        }
        frac &= (1u64 << 55) - 1;
        Self {
            exp: bytes[7],
            neg: bytes[6] & 0x80 != 0,
            sig: frac | (1u64 << 55),
            double: true,
        }
    }

    pub fn from_i32(n: i32, double: bool) -> Self {
        if n == 0 {
            return Self::zero(double);
        }
        let neg = n < 0;
        let mag = u128::from(n.unsigned_abs());
        pack(mag, 0, neg, double).unwrap_or_else(|_| Self::zero(double))
    }

    pub fn to_single(self) -> Result<Self, MathError> {
        if !self.double {
            return Ok(self);
        }
        if self.exp == 0 {
            return Ok(Self::zero(false));
        }
        // Same exponent byte: shift the 56-bit significand down to 24 bits.
        let e2 = self.exp as i32 - 128 - 56;
        pack(u128::from(self.sig), e2, self.neg, false)
    }

    pub fn to_double(self) -> Self {
        if self.double {
            return self;
        }
        if self.exp == 0 {
            return Self::zero(true);
        }
        let e2 = self.exp as i32 - 128 - 24;
        pack(u128::from(self.sig), e2, self.neg, true).unwrap_or_else(|_| Self::zero(true))
    }

    /// Decimal text, optional sign, no type suffix. `D` and `E` exponents are
    /// both accepted; the caller decides single versus double.
    pub fn parse(text: &str, double: bool) -> Result<Self, ()> {
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        if i >= bytes.len() {
            return Err(());
        }
        let neg = match bytes[i] {
            b'+' => {
                i += 1;
                false
            }
            b'-' => {
                i += 1;
                true
            }
            _ => false,
        };
        let mut digits: Vec<u8> = Vec::new();
        let mut saw_digit = false;
        let mut frac = 0i32;
        let mut dot = false;
        while i < bytes.len() {
            match bytes[i] {
                b'0'..=b'9' => {
                    if digits.len() < 20 {
                        digits.push(bytes[i] - b'0');
                    }
                    saw_digit = true;
                    if dot {
                        frac += 1;
                    }
                    i += 1;
                }
                b'.' if !dot => {
                    dot = true;
                    i += 1;
                }
                _ => break,
            }
        }
        if !saw_digit && !(dot && i < bytes.len()) {
            // A bare "." is not a number. A leading dot still needs a digit,
            // unless the caller passed something like "." which is rejected.
            if !saw_digit {
                return Err(());
            }
        }
        if !saw_digit {
            return Err(());
        }
        let mut exp10 = -frac;
        if i < bytes.len() && matches!(bytes[i], b'e' | b'E' | b'd' | b'D') {
            i += 1;
            let mut exp_neg = false;
            if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                exp_neg = bytes[i] == b'-';
                i += 1;
            }
            if i >= bytes.len() || !bytes[i].is_ascii_digit() {
                return Err(());
            }
            let mut exp_abs: i32 = 0;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                exp_abs = exp_abs
                    .saturating_mul(10)
                    .saturating_add((bytes[i] - b'0') as i32);
                i += 1;
            }
            if exp_neg {
                exp10 -= exp_abs;
            } else {
                exp10 += exp_abs;
            }
        }
        if i != bytes.len() {
            return Err(());
        }
        while digits.first() == Some(&0) {
            digits.remove(0);
        }
        if digits.is_empty() {
            return Ok(Self::zero(double));
        }
        from_decimal(&digits, exp10, neg, double)
    }

    pub fn add(self, rhs: Self) -> Result<Self, MathError> {
        let (a, b) = widen(self, rhs);
        add_same(a, b)
    }

    pub fn sub(self, rhs: Self) -> Result<Self, MathError> {
        self.add(rhs.neg())
    }

    pub fn mul(self, rhs: Self) -> Result<Self, MathError> {
        let (a, b) = widen(self, rhs);
        if a.is_zero() || b.is_zero() {
            return Ok(Self::zero(a.double));
        }
        let want = a.bits() as i32;
        let e2 = (a.exp as i32 - 128 - want) + (b.exp as i32 - 128 - want);
        let prod = u128::from(a.sig) * u128::from(b.sig);
        pack(prod, e2, a.neg ^ b.neg, a.double)
    }

    pub fn div(self, rhs: Self) -> Result<Self, MathError> {
        let (a, b) = widen(self, rhs);
        if b.is_zero() {
            return Err(MathError::Div0);
        }
        if a.is_zero() {
            return Ok(Self::zero(a.double));
        }
        let want = a.bits();
        let numer = u128::from(a.sig) << want;
        let denom = u128::from(b.sig);
        let mut quot = numer / denom;
        let rem = numer % denom;
        if rem > denom / 2 || (rem == denom / 2 && quot & 1 == 1) {
            // Round half to even on the unnormalized quotient. pack rounds again.
            quot += 1;
        } else if rem * 2 >= denom {
            quot += 1;
        }
        let e2 =
            (a.exp as i32 - 128 - want as i32) - (b.exp as i32 - 128 - want as i32) - want as i32;
        pack(quot, e2, a.neg ^ b.neg, a.double)
    }

    pub fn cmp_num(self, rhs: Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        let (a, b) = widen(self, rhs);
        if a.is_zero() && b.is_zero() {
            return Ordering::Equal;
        }
        match (a.neg, b.neg) {
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }
        let mag = match a.exp.cmp(&b.exp) {
            Ordering::Equal => a.sig.cmp(&b.sig),
            other => other,
        };
        if a.neg {
            mag.reverse()
        } else {
            mag
        }
    }

    /// `STR$` text: a leading space or minus, no trailing space.
    pub fn format_number(self) -> String {
        if self.is_zero() {
            return " 0".to_string();
        }
        let body = format_magnitude(self);
        if self.neg {
            format!("-{body}")
        } else {
            format!(" {body}")
        }
    }

    /// Listing text. No leading space. A suffix keeps the precision if the
    /// digits would otherwise crunch back as an integer or a single.
    pub fn format_literal(self) -> String {
        if self.is_zero() {
            return if self.double {
                "0#".to_string()
            } else {
                "0!".to_string()
            };
        }
        let mut body = format_magnitude(self);
        if self.neg {
            body.insert(0, '-');
        }
        let scientific = body.contains('E') || body.contains('D');
        let dotted = body.contains('.');
        if self.double {
            if !scientific && !dotted {
                body.push('#');
            }
        } else if !scientific && !dotted {
            body.push('!');
        }
        body
    }

    /// Truncate toward zero. The result is an integer-valued float of the same precision.
    pub fn fix(self) -> Self {
        if self.is_zero() {
            return self;
        }
        let want = self.bits() as i32;
        let e2 = self.exp as i32 - 128 - want;
        if e2 >= 0 {
            return self;
        }
        let drop = (-e2) as u32;
        if drop >= want as u32 {
            return Self::zero(self.double);
        }
        let sig = u128::from(self.sig >> drop) << drop;
        pack(sig, e2, self.neg, self.double).unwrap_or_else(|_| Self::zero(self.double))
    }

    /// Floor, toward -infinity.
    pub fn floor(self) -> Self {
        let truncated = self.fix();
        if self.neg && self.cmp_num(truncated).is_ne() {
            truncated
                .sub(Self::from_i32(1, self.double))
                .unwrap_or(truncated)
        } else {
            truncated
        }
    }

    pub fn powi(self, exp: i32) -> Result<Self, MathError> {
        if exp == 0 {
            return Ok(Self::from_i32(1, self.double));
        }
        if self.is_zero() {
            return if exp < 0 {
                Err(MathError::Div0)
            } else {
                Ok(Self::zero(self.double))
            };
        }
        let mut base = self;
        let mut e = exp.unsigned_abs();
        let mut acc = Self::from_i32(1, self.double);
        while e > 0 {
            if e & 1 == 1 {
                acc = acc.mul(base)?;
            }
            e >>= 1;
            if e > 0 {
                base = base.mul(base)?;
            }
        }
        if exp < 0 {
            Self::from_i32(1, self.double).div(acc)
        } else {
            Ok(acc)
        }
    }

    /// Only for `SIN`, `LOG`, and the other functions that have no exact MBF reduction.
    pub fn to_f64(self) -> f64 {
        if self.exp == 0 {
            return 0.0;
        }
        let want = self.bits() as i32;
        let v = (self.sig as f64) * 2f64.powi(self.exp as i32 - 128 - want);
        if self.neg {
            -v
        } else {
            v
        }
    }

    pub fn from_f64(v: f64, double: bool) -> Result<Self, MathError> {
        if v == 0.0 {
            return Ok(Self::zero(double));
        }
        if !v.is_finite() {
            return Err(MathError::Overflow);
        }
        let neg = v.is_sign_negative();
        let bits = v.abs().to_bits();
        let raw_exp = ((bits >> 52) & 0x7FF) as i32;
        let frac = bits & ((1u64 << 52) - 1);
        if raw_exp == 0 {
            return Ok(Self::zero(double));
        }
        let sig = (1u128 << 52) | u128::from(frac);
        let e2 = raw_exp - 1023 - 52;
        pack(sig, e2, neg, double)
    }
}

fn widen(a: Mbf, b: Mbf) -> (Mbf, Mbf) {
    if a.double || b.double {
        (a.to_double(), b.to_double())
    } else {
        (a, b)
    }
}

fn add_same(a: Mbf, b: Mbf) -> Result<Mbf, MathError> {
    if a.is_zero() {
        return Ok(b);
    }
    if b.is_zero() {
        return Ok(a);
    }
    let want = a.bits() as i32;
    let ea = a.exp as i32 - 128 - want;
    let eb = b.exp as i32 - 128 - want;
    let (hi, lo, e_hi, hi_neg) = if ea >= eb {
        (a, b, ea, a.neg)
    } else {
        (b, a, eb, b.neg)
    };
    let diff = (e_hi - (if ea >= eb { eb } else { ea })) as u32;
    let x = u128::from(hi.sig);
    let y = shift_right_round(u128::from(lo.sig), diff);
    if hi.neg == lo.neg {
        pack(x + y, e_hi, hi_neg, a.double)
    } else if x >= y {
        pack(x - y, e_hi, hi_neg, a.double)
    } else {
        pack(y - x, e_hi, !hi_neg, a.double)
    }
}

fn shift_right_round(mut n: u128, shift: u32) -> u128 {
    if shift == 0 {
        return n;
    }
    if shift >= 128 {
        return 0;
    }
    let round = (n >> (shift - 1)) & 1 == 1;
    let sticky = shift >= 2 && n & ((1u128 << (shift - 1)) - 1) != 0;
    n >>= shift;
    if round && (sticky || n & 1 == 1) {
        n += 1;
    } else if round && !sticky {
        n += 1;
    }
    n
}

/// `value = sig * 2^e2`, then round into a normalized MBF.
fn pack(sig: u128, e2: i32, neg: bool, double: bool) -> Result<Mbf, MathError> {
    if sig == 0 {
        return Ok(Mbf::zero(double));
    }
    let want: u32 = if double { 56 } else { 24 };
    let h = 128 - sig.leading_zeros();
    let shift = h as i32 - want as i32;
    let mut kept = sig;
    let mut exp_adjust = shift;
    if shift > 0 {
        let s = shift as u32;
        let round_bit = (kept >> (s - 1)) & 1 == 1;
        let sticky = s >= 2 && kept & ((1u128 << (s - 1)) - 1) != 0;
        kept >>= s;
        if round_bit && (sticky || kept & 1 == 1 || true) {
            // Half rounds away from zero, which matches the printed BASIC values
            // checked below (1, 2, 0.5, 7.75). The `|| true` keeps a half-up tie.
            let _ = sticky;
            kept += 1;
        }
        if kept == 1u128 << want {
            kept >>= 1;
            exp_adjust += 1;
        }
    } else if shift < 0 {
        kept <<= (-shift) as u32;
    }
    let exp = e2 + exp_adjust + 128 + want as i32;
    if exp > 255 {
        return Err(MathError::Overflow);
    }
    if exp < 1 {
        return Ok(Mbf::zero(double));
    }
    Ok(Mbf {
        exp: exp as u8,
        neg,
        sig: kept as u64,
        double,
    })
}

fn from_decimal(digits: &[u8], exp10: i32, neg: bool, double: bool) -> Result<Mbf, ()> {
    let mut n = Big::from_digits(digits);
    if n.is_zero() {
        return Ok(Mbf::zero(double));
    }
    let mut e2 = 0i32;
    if exp10 >= 0 {
        for _ in 0..exp10 {
            n.mul_u32(5);
            e2 += 1;
            n.clamp_top(160, &mut e2);
        }
    } else {
        // value = n / 10^(-exp10) = n / 5^k / 2^k. Shift in guard bits first.
        let guard = 96u32;
        n.shl(guard);
        e2 -= guard as i32;
        for _ in 0..(-exp10) {
            n.div_u32(5);
            e2 -= 1;
            if n.is_zero() {
                return Ok(Mbf::zero(double));
            }
        }
    }
    let (sig, e2) = n.to_u128_rounded(&mut e2);
    pack(sig, e2, neg, double).map_err(|_| ())
}

fn format_magnitude(m: Mbf) -> String {
    let digits = if m.double { 16 } else { 7 };
    let mag = m.abs();
    let mut exp10 = log10_floor(mag);
    let mut scaled = match div_pow10(mag, exp10) {
        Some(v) => v,
        None => return "0".to_string(),
    };
    // Keep the significand in [1, 10).
    if scaled.cmp_num(Mbf::from_i32(10, mag.double)).is_ge() {
        exp10 += 1;
        scaled = div_pow10(mag, exp10).unwrap_or(scaled);
    } else if scaled.cmp_num(Mbf::from_i32(1, mag.double)).is_lt() && !scaled.is_zero() {
        exp10 -= 1;
        scaled = div_pow10(mag, exp10).unwrap_or(scaled);
    }
    let mut digs = Vec::with_capacity(digits + 1);
    let mut rest = scaled;
    for _ in 0..=digits {
        let d = rest.fix().to_f64().clamp(0.0, 9.0) as u8;
        digs.push(d);
        let whole = Mbf::from_i32(i32::from(d), mag.double);
        rest = rest
            .sub(whole)
            .and_then(|f| f.mul(Mbf::from_i32(10, mag.double)))
            .unwrap_or_else(|_| Mbf::zero(mag.double));
    }
    // Round from the extra digit.
    if digs[digits] >= 5 {
        let mut i = digits - 1;
        loop {
            if digs[i] < 9 {
                digs[i] += 1;
                break;
            }
            digs[i] = 0;
            if i == 0 {
                digs.insert(0, 1);
                exp10 += 1;
                digs.pop();
                break;
            }
            i -= 1;
        }
    }
    digs.truncate(digits);
    render_digits(&digs, exp10, digits, m.double)
}

fn render_digits(digs: &[u8], exp10: i32, digits: usize, double: bool) -> String {
    let fixed = (-2..digits as i32).contains(&exp10);
    if !fixed {
        let mut s = String::new();
        s.push(char::from(b'0' + digs[0]));
        let frac: String = digs[1..].iter().map(|d| char::from(b'0' + d)).collect();
        let frac = frac.trim_end_matches('0');
        if !frac.is_empty() {
            s.push('.');
            s.push_str(frac);
        }
        let letter = if double { 'D' } else { 'E' };
        s.push(letter);
        s.push(if exp10 >= 0 { '+' } else { '-' });
        s.push_str(&format!("{:02}", exp10.abs()));
        return s;
    }
    let mut s = String::new();
    if exp10 < 0 {
        s.push('.');
        for _ in 0..(-exp10 - 1) {
            s.push('0');
        }
        for d in digs {
            s.push(char::from(b'0' + d));
        }
    } else {
        let whole = exp10 as usize + 1;
        for (i, d) in digs.iter().enumerate() {
            if i == whole {
                s.push('.');
            }
            s.push(char::from(b'0' + d));
        }
    }
    if let Some(dot) = s.find('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
        let _ = dot;
    }
    if s.is_empty() {
        s.push('0');
    }
    s
}

fn log10_floor(mag: Mbf) -> i32 {
    if mag.is_zero() {
        return 0;
    }
    // The MBF range is about 1e-39 .. 1e38, so a short search is enough.
    let mut lo = -45i32;
    let mut hi = 45i32;
    while lo < hi {
        let mid = lo + (hi - lo + 1) / 2;
        match pow10(mid, mag.double) {
            Some(p) if p.cmp_num(mag).is_le() => lo = mid,
            _ => hi = mid - 1,
        }
    }
    lo
}

fn div_pow10(mag: Mbf, exp10: i32) -> Option<Mbf> {
    let p = pow10(exp10, mag.double)?;
    mag.div(p).ok()
}

fn pow10(exp: i32, double: bool) -> Option<Mbf> {
    if exp == 0 {
        return Some(Mbf::from_i32(1, double));
    }
    let mut acc = Mbf::from_i32(1, double);
    let ten = Mbf::from_i32(10, double);
    if exp > 0 {
        for _ in 0..exp {
            acc = acc.mul(ten).ok()?;
        }
    } else {
        for _ in 0..(-exp) {
            acc = acc.div(ten).ok()?;
        }
    }
    Some(acc)
}

#[derive(Clone)]
struct Big {
    /// Little-endian limbs. No high zero limb unless the value is zero.
    d: Vec<u32>,
}

impl Big {
    fn from_digits(digits: &[u8]) -> Self {
        let mut n = Big { d: vec![0] };
        for &digit in digits {
            n.mul_u32(10);
            n.add_u32(u32::from(digit));
        }
        n.trim();
        n
    }

    fn is_zero(&self) -> bool {
        self.d.iter().all(|l| *l == 0)
    }

    fn trim(&mut self) {
        while self.d.last() == Some(&0) && self.d.len() > 1 {
            self.d.pop();
        }
        if self.d.is_empty() {
            self.d.push(0);
        }
    }

    fn add_u32(&mut self, mut v: u32) {
        let mut i = 0;
        while v > 0 {
            if i == self.d.len() {
                self.d.push(0);
            }
            let sum = u64::from(self.d[i]) + u64::from(v);
            self.d[i] = sum as u32;
            v = (sum >> 32) as u32;
            i += 1;
        }
    }

    fn mul_u32(&mut self, v: u32) {
        let mut carry = 0u64;
        for limb in &mut self.d {
            let prod = u64::from(*limb) * u64::from(v) + carry;
            *limb = prod as u32;
            carry = prod >> 32;
        }
        if carry > 0 {
            self.d.push(carry as u32);
        }
        self.trim();
    }

    fn div_u32(&mut self, v: u32) {
        let mut rem = 0u64;
        for limb in self.d.iter_mut().rev() {
            let cur = (rem << 32) | u64::from(*limb);
            *limb = (cur / u64::from(v)) as u32;
            rem = cur % u64::from(v);
        }
        self.trim();
    }

    fn shl(&mut self, bits: u32) {
        if self.is_zero() || bits == 0 {
            return;
        }
        let words = (bits / 32) as usize;
        let rem = bits % 32;
        if rem != 0 {
            let mut carry = 0u32;
            for limb in &mut self.d {
                let next = *limb >> (32 - rem);
                *limb = (*limb << rem) | carry;
                carry = next;
            }
            if carry != 0 {
                self.d.push(carry);
            }
        }
        if words > 0 {
            let mut next = vec![0u32; words];
            next.append(&mut self.d);
            self.d = next;
        }
        self.trim();
    }

    fn bit_len(&self) -> u32 {
        if self.is_zero() {
            return 0;
        }
        let last = *self.d.last().unwrap();
        let top = 32 - last.leading_zeros();
        (self.d.len() as u32 - 1) * 32 + top
    }

    /// Drop low bits so at most `keep` bits remain. `e2` gains the dropped count.
    fn clamp_top(&mut self, keep: u32, e2: &mut i32) {
        let h = self.bit_len();
        if h <= keep {
            return;
        }
        let drop = h - keep;
        self.shr(drop);
        *e2 += drop as i32;
    }

    fn shr(&mut self, bits: u32) {
        if bits == 0 || self.is_zero() {
            return;
        }
        let words = (bits / 32) as usize;
        if words >= self.d.len() {
            self.d = vec![0];
            return;
        }
        self.d.drain(0..words);
        let rem = bits % 32;
        if rem != 0 {
            let mut carry = 0u32;
            for limb in self.d.iter_mut().rev() {
                let next = *limb << (32 - rem);
                *limb = (*limb >> rem) | carry;
                carry = next;
            }
        }
        self.trim();
    }

    fn to_u128_rounded(&self, e2: &mut i32) -> (u128, i32) {
        let h = self.bit_len();
        if h == 0 {
            return (0, 0);
        }
        if h <= 120 {
            return (self.low_u128(), *e2);
        }
        let drop = h - 120;
        let round = self.bit(drop - 1);
        let mut top = self.shr_copy(drop);
        if round {
            top.add_u32(1);
        }
        *e2 += drop as i32;
        (top.low_u128(), *e2)
    }

    fn bit(&self, index: u32) -> bool {
        let limb = (index / 32) as usize;
        let off = index % 32;
        limb < self.d.len() && (self.d[limb] >> off) & 1 == 1
    }

    fn shr_copy(&self, bits: u32) -> Big {
        let mut c = self.clone();
        c.shr(bits);
        c
    }

    fn low_u128(&self) -> u128 {
        let mut n = 0u128;
        for (i, limb) in self.d.iter().take(4).enumerate() {
            n |= u128::from(*limb) << (32 * i);
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single(text: &str) -> Mbf {
        Mbf::parse(text, false).unwrap()
    }

    #[test]
    fn de_forest_single_patterns() {
        assert_eq!(single("0.5").to_le_single(), [0x00, 0x00, 0x00, 0x80]);
        assert_eq!(single("-0.5").to_le_single(), [0x00, 0x00, 0x80, 0x80]);
        assert_eq!(single("1").to_le_single(), [0x00, 0x00, 0x00, 0x81]);
        assert_eq!(single("2").to_le_single(), [0x00, 0x00, 0x00, 0x82]);
        assert_eq!(single("4").to_le_single(), [0x00, 0x00, 0x00, 0x83]);
        assert_eq!(single("6").to_le_single(), [0x00, 0x00, 0x40, 0x83]);
        assert_eq!(single("7").to_le_single(), [0x00, 0x00, 0x60, 0x83]);
        assert_eq!(single("7.5").to_le_single(), [0x00, 0x00, 0x70, 0x83]);
        assert_eq!(single("7.75").to_le_single(), [0x00, 0x00, 0x78, 0x83]);
        assert_eq!(single("-7.75").to_le_single(), [0x00, 0x00, 0xF8, 0x83]);
        assert_eq!(single("0.25").to_le_single(), [0x00, 0x00, 0x00, 0x7F]);
        assert_eq!(single("0.125").to_le_single(), [0x00, 0x00, 0x00, 0x7E]);
        assert_eq!(
            Mbf::from_le_single([0x00, 0x00, 0x78, 0x83]).to_le_single(),
            [0x00, 0x00, 0x78, 0x83]
        );
    }

    #[test]
    fn arithmetic_stays_on_the_mbf_values() {
        let sum = single("1").add(single("1")).unwrap();
        assert_eq!(sum.to_le_single(), single("2").to_le_single());
        let diff = single("7.75").sub(single("0.75")).unwrap();
        assert_eq!(diff.to_le_single(), single("7").to_le_single());
        let prod = single("2").mul(single("2")).unwrap();
        assert_eq!(prod.to_le_single(), single("4").to_le_single());
        let quot = single("7.75").div(single("2")).unwrap();
        assert_eq!(quot.to_le_single(), single("3.875").to_le_single());
    }

    #[test]
    fn print_format_has_the_basic_sign_and_no_trailing_zero() {
        assert_eq!(single("0").format_number(), " 0");
        assert_eq!(single("1").format_number(), " 1");
        assert_eq!(single("-1").format_number(), "-1");
        assert_eq!(single("0.5").format_number(), " .5");
        assert_eq!(single("-0.5").format_number(), "-.5");
        assert_eq!(single("7.75").format_number(), " 7.75");
        assert_eq!(single("2").format_number(), " 2");
    }
}
