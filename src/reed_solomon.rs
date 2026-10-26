//! The two Reed-Solomon codes of R2007 (`AC1021`) pages — ODA spec §5.13.
//!
//! R2007 protects its pages with Reed-Solomon codes over GF(2^8), 255-byte
//! codewords, in two configurations (§5.13):
//!
//! | Pages | (n, k) | Primitive polynomial coefficients | Field polynomial |
//! |---|---|---|---|
//! | system pages: file header, page map, section map | (255, 239) | `1, 0, 0, 1, 0, 1, 1, 0` | `0x169` |
//! | data pages | (255, 251) | `1, 0, 1, 1, 1, 0, 0, 0` | `0x11D` |
//!
//! The coefficients are listed from x^0 up, the convention of the Rockliff
//! implementation §5.13 cites, with x^8 implied. Three things §5.13 leaves
//! out are measured on every page of the corpus's three R2007 files (see
//! `examples/probe_r2007_rs.rs`): the generator's roots are α^1 … α^(n-k),
//! byte *i* of a stored codeword is the coefficient of x^i, and the message
//! is the codeword's first *k* bytes with the parity after it. Under those
//! readings all 22 system-page codewords of each file and all 62 or 63
//! data-page codewords verify, AutoCAD's parity bytes re-encode exactly,
//! and under any other polynomial, root offset or byte order none does.
//!
//! Interleaving (§5.13.2) puts byte *i* of codeword *j* of an *N*-codeword
//! block at position `N·i + j`; [`Code::verify_interleaved`] works on that
//! layout. The reader takes the message bytes without checking the parity
//! ([`crate::r2007::deinterleave`]); this module is the check and the
//! repair path, correcting up to `(n - k) / 2` bytes per codeword.
//!
//! # Decoder
//!
//! 1. Syndromes S_j = c(α^j), j = 1 … n-k. All zero: the codeword is intact.
//! 2. Berlekamp-Massey finds the error locator Λ(x).
//! 3. Chien search: Λ(α^-i) = 0 marks byte *i* as wrong.
//! 4. The magnitudes solve S_j = Σ Y_l · X_l^j by Gaussian elimination.
//! 5. The corrected codeword must have zero syndromes, or it is refused.

use crate::error::{Error, Result};

/// Bytes in a codeword of either code.
pub const CODEWORD_BYTES: usize = 255;
/// Parity bytes of the system-page code, [`Code::SYSTEM_PAGE`].
pub const PARITY_BYTES: usize = 16;
/// Message bytes of the system-page code, [`Code::SYSTEM_PAGE`].
pub const MESSAGE_BYTES: usize = CODEWORD_BYTES - PARITY_BYTES;

/// One of §5.13's two Reed-Solomon codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Code {
    parity: usize,
    primitive: u16,
}

impl Code {
    /// §5.13 system pages: (255, 239) over `0x169` (the file header, the
    /// page map and the section map).
    pub const SYSTEM_PAGE: Code = Code {
        parity: 16,
        primitive: 0x169,
    };

    /// §5.13 data pages: (255, 251) over `0x11D` (pages of sections whose
    /// encoding is 4).
    pub const DATA_PAGE: Code = Code {
        parity: 4,
        primitive: 0x11D,
    };

    /// Parity bytes per codeword, `n - k` (§5.13).
    pub const fn parity_bytes(self) -> usize {
        self.parity
    }

    /// Message bytes per codeword, `k` (§5.13).
    pub const fn message_bytes(self) -> usize {
        CODEWORD_BYTES - self.parity
    }

    /// The field polynomial with its x^8 term, e.g. `0x169` (§5.13).
    pub const fn primitive_polynomial(self) -> u16 {
        self.primitive
    }

    /// Codewords needed to carry `message_bytes` bytes (§5.13).
    pub const fn codewords_for(self, message_bytes: usize) -> usize {
        message_bytes.div_ceil(self.message_bytes())
    }

    /// The `n - k` syndromes of a 255-byte codeword (§5.13); all zero when
    /// it is intact.
    pub fn syndromes(self, codeword: &[u8]) -> Result<Vec<u8>> {
        check_len(codeword)?;
        let f = Field::new(self.primitive);
        Ok((1..=self.parity)
            .map(|j| f.eval_low_first(codeword, f.alpha(j)))
            .collect())
    }

    /// Check a 255-byte codeword and correct it in place (§5.13).
    ///
    /// Returns the number of bytes corrected, 0 for an intact codeword.
    /// More than `(n - k) / 2` errors, or a correction that does not leave
    /// zero syndromes, is an [`Error::SectionMap`] and leaves the codeword
    /// as it was.
    pub fn verify(self, codeword: &mut [u8]) -> Result<usize> {
        let syn = self.syndromes(codeword)?;
        if syn.iter().all(|&s| s == 0) {
            return Ok(0);
        }
        let f = Field::new(self.primitive);
        let lambda = berlekamp_massey(&syn, &f);
        let degree = lambda.len() - 1;
        let positions: Vec<usize> = (0..CODEWORD_BYTES)
            .filter(|&i| f.eval_low_first(&lambda, f.alpha_inverse(i)) == 0)
            .collect();
        if degree == 0 || degree > self.parity / 2 || positions.len() != degree {
            return Err(Error::SectionMap(format!(
                "Reed-Solomon: uncorrectable codeword ({} error locations for a locator of degree {degree}, at most {} correctable)",
                positions.len(),
                self.parity / 2
            )));
        }
        let magnitudes = solve_magnitudes(&syn, &positions, &f)?;
        let mut fixed = codeword.to_vec();
        for (&at, &y) in positions.iter().zip(&magnitudes) {
            fixed[at] ^= y;
        }
        if self.syndromes(&fixed)?.iter().any(|&s| s != 0) {
            return Err(Error::SectionMap(
                "Reed-Solomon: correction did not converge".into(),
            ));
        }
        codeword.copy_from_slice(&fixed);
        Ok(positions.len())
    }

    /// Check and correct, in place, the `blocks` codewords of a §5.13.2
    /// interleaved block (byte *i* of codeword *j* at `blocks·i + j`).
    ///
    /// Returns the bytes corrected in all. `page` must hold at least
    /// `blocks · 255` bytes; bytes after that are ignored.
    pub fn verify_interleaved(self, page: &mut [u8], blocks: usize) -> Result<usize> {
        blocks
            .checked_mul(CODEWORD_BYTES)
            .filter(|&s| s <= page.len())
            .ok_or_else(|| {
                Error::SectionMap(format!(
                    "Reed-Solomon: {blocks} interleaved codewords need {} bytes, page has {}",
                    blocks.saturating_mul(CODEWORD_BYTES),
                    page.len()
                ))
            })?;
        let mut corrected = 0;
        let mut codeword = [0u8; CODEWORD_BYTES];
        for j in 0..blocks {
            for (i, b) in codeword.iter_mut().enumerate() {
                *b = page[blocks * i + j];
            }
            let n = self.verify(&mut codeword)?;
            if n > 0 {
                for (i, &b) in codeword.iter().enumerate() {
                    page[blocks * i + j] = b;
                }
                corrected += n;
            }
        }
        Ok(corrected)
    }
}

/// The 16 syndromes of a system-page codeword ([`Code::SYSTEM_PAGE`]); a
/// codeword of the wrong length yields all `0xFF`.
pub fn syndromes(codeword: &[u8]) -> [u8; PARITY_BYTES] {
    let mut out = [0xFFu8; PARITY_BYTES];
    if let Ok(syn) = Code::SYSTEM_PAGE.syndromes(codeword) {
        out.copy_from_slice(&syn);
    }
    out
}

/// Check and correct a system-page codeword in place ([`Code::SYSTEM_PAGE`]).
pub fn verify(codeword: &mut [u8]) -> Result<()> {
    Code::SYSTEM_PAGE.verify(codeword).map(|_| ())
}

/// System-page codewords needed for `message_bytes` bytes ([`Code::SYSTEM_PAGE`]).
pub fn codewords_for_message(message_bytes: usize) -> usize {
    Code::SYSTEM_PAGE.codewords_for(message_bytes)
}

fn check_len(codeword: &[u8]) -> Result<()> {
    if codeword.len() == CODEWORD_BYTES {
        Ok(())
    } else {
        Err(Error::SectionMap(format!(
            "Reed-Solomon codeword must be {CODEWORD_BYTES} bytes, got {}",
            codeword.len()
        )))
    }
}

/// GF(2^8) under one primitive polynomial, α = 2.
pub(crate) struct Field {
    exp: [u8; 512],
    log: [u8; 256],
}

impl Field {
    pub(crate) fn new(primitive: u16) -> Self {
        let mut exp = [0u8; 512];
        let mut log = [0u8; 256];
        let mut x: u16 = 1;
        for (i, e) in exp.iter_mut().enumerate().take(255) {
            *e = x as u8;
            log[usize::from(x)] = i as u8;
            x <<= 1;
            if x & 0x100 != 0 {
                x ^= primitive;
            }
        }
        for i in 255..512 {
            exp[i] = exp[i - 255];
        }
        Field { exp, log }
    }

    pub(crate) fn mul(&self, a: u8, b: u8) -> u8 {
        if a == 0 || b == 0 {
            0
        } else {
            self.exp[usize::from(self.log[usize::from(a)]) + usize::from(self.log[usize::from(b)])]
        }
    }

    pub(crate) fn inv(&self, a: u8) -> u8 {
        debug_assert!(a != 0);
        self.exp[255 - usize::from(self.log[usize::from(a)])]
    }

    /// α^e.
    pub(crate) fn alpha(&self, e: usize) -> u8 {
        self.exp[e % 255]
    }

    /// α^-e.
    pub(crate) fn alpha_inverse(&self, e: usize) -> u8 {
        self.exp[(255 - e % 255) % 255]
    }

    /// Σ poly[i] · x^i.
    pub(crate) fn eval_low_first(&self, poly: &[u8], x: u8) -> u8 {
        poly.iter().rev().fold(0, |y, &c| self.mul(y, x) ^ c)
    }
}

/// Berlekamp-Massey: the error locator Λ(x), lowest degree first, from the
/// syndromes S_1 … S_(n-k).
fn berlekamp_massey(syn: &[u8], f: &Field) -> Vec<u8> {
    let mut c = vec![1u8];
    let mut b = vec![1u8];
    let mut l = 0usize;
    let mut m = 1usize;
    let mut last = 1u8;
    for n in 0..syn.len() {
        let mut d = syn[n];
        for i in 1..=l.min(c.len() - 1) {
            d ^= f.mul(c[i], syn[n - i]);
        }
        if d == 0 {
            m += 1;
            continue;
        }
        let scale = f.mul(d, f.inv(last));
        let previous = c.clone();
        if c.len() < b.len() + m {
            c.resize(b.len() + m, 0);
        }
        for (i, &bi) in b.iter().enumerate() {
            c[i + m] ^= f.mul(scale, bi);
        }
        if 2 * l <= n {
            l = n + 1 - l;
            b = previous;
            last = d;
            m = 1;
        } else {
            m += 1;
        }
    }
    c.truncate(l + 1);
    c
}

/// Error magnitudes Y_l from S_j = Σ Y_l · X_l^j (j = 1 … ν), X_l = α^position.
fn solve_magnitudes(syn: &[u8], positions: &[usize], f: &Field) -> Result<Vec<u8>> {
    let n = positions.len();
    let mut mat: Vec<Vec<u8>> = (0..n)
        .map(|row| {
            let mut r: Vec<u8> = positions
                .iter()
                .map(|&p| f.alpha(p * (row + 1) % 255))
                .collect();
            r.push(syn[row]);
            r
        })
        .collect();
    for col in 0..n {
        let pivot = (col..n)
            .find(|&r| mat[r][col] != 0)
            .ok_or_else(|| Error::SectionMap("Reed-Solomon: singular system".into()))?;
        mat.swap(col, pivot);
        let inv = f.inv(mat[col][col]);
        for cell in &mut mat[col][col..=n] {
            *cell = f.mul(*cell, inv);
        }
        let pivot_row = mat[col].clone();
        for (r, row) in mat.iter_mut().enumerate() {
            if r == col || row[col] == 0 {
                continue;
            }
            let factor = row[col];
            for (cell, &p) in row[col..=n].iter_mut().zip(&pivot_row[col..=n]) {
                *cell ^= f.mul(factor, p);
            }
        }
    }
    Ok(mat.iter().map(|row| row[n]).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gf_tables_are_multiplicative_group() {
        let f = Field::new(Code::SYSTEM_PAGE.primitive_polynomial());
        // α^255 wraps to α^0 = 1 in GF(2^8).
        assert_eq!(f.alpha(0), 1);
        // log(1) = 0.
        assert_eq!(f.log[1], 0);
        // Multiplicative inverse: α · α^-1 = 1 for a nonzero element.
        for a in 1u8..=255 {
            let inv = f.inv(a);
            assert_eq!(f.mul(a, inv), 1, "a={}", a);
        }
    }

    #[test]
    fn all_zero_codeword_has_zero_syndromes() {
        let cw = [0u8; CODEWORD_BYTES];
        let syn = syndromes(&cw);
        assert!(syn.iter().all(|&b| b == 0));
    }

    #[test]
    fn verify_accepts_intact_codeword() {
        let mut cw = vec![0u8; CODEWORD_BYTES];
        assert!(verify(&mut cw).is_ok());
    }

    #[test]
    fn verify_rejects_wrong_length() {
        let mut cw = vec![0u8; 100];
        assert!(verify(&mut cw).is_err());
    }
}
