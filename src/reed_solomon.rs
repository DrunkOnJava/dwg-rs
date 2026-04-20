//! Reed-Solomon(255,239) FEC decoder for R2004+ system sections (spec §4.1).
//!
//! DWG R2004+ interleaves ~239 bytes of payload with ~16 bytes of parity
//! per 255-byte chunk. When CRC-8 on a system page fails, these RS parity
//! bytes allow recovery of up to 8 corrupted bytes per chunk.
//!
//! # Finite field
//!
//! GF(256) over the AES primitive polynomial `0x11D = x^8 + x^4 + x^3 + x^2 + 1`.
//! Tables are computed at runtime from a canonical generator α = 2.
//!
//! # Decoder pipeline
//!
//! 1. **Syndrome calculation** — evaluate the received polynomial at α^1,
//!    α^2, … α^16. All zeros ⇒ no errors; `Ok(())` with no work.
//! 2. **Berlekamp-Massey** — solve for the error locator polynomial Λ(x).
//! 3. **Chien search** — find roots of Λ(x); each root points to a byte
//!    position in the codeword that's corrupted.
//! 4. **Forney's algorithm** — compute the error magnitudes.
//! 5. Apply corrections in place, re-verify syndromes are zero.
//!
//! Usage for DWG is defensive: we try this only when CRC-8 fails. Valid
//! files never touch the path. Included for repair-mode completeness.

use crate::error::{Error, Result};

/// Number of parity bytes per Reed-Solomon block. DWG uses (255, 239),
/// i.e. 239 message bytes + 16 parity bytes per 255-byte codeword.
pub const PARITY_BYTES: usize = 16;
pub const CODEWORD_BYTES: usize = 255;
pub const MESSAGE_BYTES: usize = CODEWORD_BYTES - PARITY_BYTES;

/// Generate the GF(256) log/antilog tables for the AES primitive
/// polynomial `0x11D` with generator α = 2.
fn gf_tables() -> ([u8; 256], [u8; 256]) {
    let mut exp = [0u8; 256];
    let mut log = [0u8; 256];
    let mut x: u16 = 1;
    // Iterating by index is intentional — we write to two disjoint
    // arrays on each step (exp[i] and log[x]), which enumerate() over
    // either array can't express.
    #[allow(clippy::needless_range_loop)]
    for i in 0..255 {
        exp[i] = x as u8;
        log[x as usize] = i as u8;
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= 0x11D;
        }
    }
    // exp[255] wraps to exp[0] = 1 by GF order; leave as zero for clarity.
    (exp, log)
}

fn gf_mul(a: u8, b: u8, exp: &[u8; 256], log: &[u8; 256]) -> u8 {
    if a == 0 || b == 0 {
        return 0;
    }
    let s = log[a as usize] as u16 + log[b as usize] as u16;
    exp[(s % 255) as usize]
}

fn gf_inv(a: u8, exp: &[u8; 256], log: &[u8; 256]) -> u8 {
    debug_assert!(a != 0);
    exp[(255 - log[a as usize] as u16) as usize % 255]
}

fn gf_poly_eval(poly: &[u8], x: u8, exp: &[u8; 256], log: &[u8; 256]) -> u8 {
    // Horner's rule — Σ poly[i] · x^i evaluated left-to-right.
    let mut y = poly[0];
    for &c in &poly[1..] {
        y = gf_mul(y, x, exp, log) ^ c;
    }
    y
}

/// Compute the 16 syndromes of a 255-byte codeword. All zeros ⇒ intact.
pub fn syndromes(codeword: &[u8]) -> [u8; PARITY_BYTES] {
    let (exp, log) = gf_tables();
    let mut s = [0u8; PARITY_BYTES];
    for i in 0..PARITY_BYTES {
        // α^(i+1)
        let alpha = exp[(i + 1) % 255];
        s[i] = gf_poly_eval(codeword, alpha, &exp, &log);
    }
    s
}

/// Berlekamp-Massey error locator polynomial.
fn berlekamp_massey(syn: &[u8; PARITY_BYTES], exp: &[u8; 256], log: &[u8; 256]) -> Vec<u8> {
    let mut lambda: Vec<u8> = vec![1];
    let mut corr: Vec<u8> = vec![1];
    let mut k: usize = 0;
    for n in 0..PARITY_BYTES {
        let mut delta: u8 = syn[n];
        for i in 1..lambda.len() {
            delta ^= gf_mul(lambda[i], syn[n - i], exp, log);
        }
        if delta == 0 {
            k += 1;
            // shift corr
            corr.insert(0, 0);
        } else if 2 * (lambda.len() - 1) <= n {
            let t = lambda.clone();
            // λ(x) = λ(x) + δ · x · corr(x)
            let shifted: Vec<u8> = std::iter::once(0u8).chain(corr.iter().copied()).collect();
            let mut new_lambda = lambda.clone();
            for (i, &c) in shifted.iter().enumerate() {
                if i < new_lambda.len() {
                    new_lambda[i] ^= gf_mul(delta, c, exp, log);
                } else {
                    new_lambda.push(gf_mul(delta, c, exp, log));
                }
            }
            lambda = new_lambda;
            // corr ← δ^-1 · t
            let d_inv = gf_inv(delta, exp, log);
            corr = t.iter().map(|&c| gf_mul(d_inv, c, exp, log)).collect();
            corr.insert(0, 0);
            k = 0;
        } else {
            let shifted: Vec<u8> = std::iter::once(0u8).chain(corr.iter().copied()).collect();
            for (i, &c) in shifted.iter().enumerate() {
                if i < lambda.len() {
                    lambda[i] ^= gf_mul(delta, c, exp, log);
                } else {
                    lambda.push(gf_mul(delta, c, exp, log));
                }
            }
            k += 1;
            corr.insert(0, 0);
        }
        let _ = k;
    }
    lambda
}

/// Chien search — find roots of the error locator polynomial in GF(256).
fn chien_search(lambda: &[u8], exp: &[u8; 256], log: &[u8; 256]) -> Vec<usize> {
    let mut roots = Vec::new();
    for i in 0..255usize {
        // evaluate λ(α^-i)
        let alpha_i = exp[(255 - i) % 255];
        if gf_poly_eval(lambda, alpha_i, exp, log) == 0 {
            roots.push(i);
        }
    }
    roots
}

/// Return `Ok(())` if the codeword has zero syndromes (intact) or can be
/// corrected, `Err` if decoding fails.
pub fn verify(codeword: &mut [u8]) -> Result<()> {
    if codeword.len() != CODEWORD_BYTES {
        return Err(Error::SectionMap(format!(
            "Reed-Solomon codeword must be {} bytes, got {}",
            CODEWORD_BYTES,
            codeword.len()
        )));
    }
    let syn = syndromes(codeword);
    if syn.iter().all(|&b| b == 0) {
        return Ok(());
    }
    let (exp, log) = gf_tables();
    let lambda = berlekamp_massey(&syn, &exp, &log);
    let roots = chien_search(&lambda, &exp, &log);
    if roots.is_empty() || roots.len() > PARITY_BYTES / 2 {
        return Err(Error::SectionMap(format!(
            "Reed-Solomon: {} error locations, max correctable is {}",
            roots.len(),
            PARITY_BYTES / 2
        )));
    }
    // Forney: for each root at position `i`, compute the magnitude.
    // Full Forney implementation requires Ω(x) = (S(x)·λ(x)) mod x^16
    // and λ'(x) (formal derivative). For brevity we recompute syndromes
    // after locating errors and solve via Gaussian elimination on the
    // resulting linear system.
    let n = roots.len();
    let mut mat = vec![vec![0u8; n + 1]; n];
    for (i, row) in mat.iter_mut().enumerate().take(n) {
        for (j, &pos) in roots.iter().enumerate() {
            let alpha_pos = exp[(pos) % 255];
            // α^((i+1)·pos) with i+1 from syndrome index, pos from root index.
            let mut power = 1u8;
            for _ in 0..(i + 1) {
                power = gf_mul(power, alpha_pos, &exp, &log);
            }
            row[j] = power;
        }
        row[n] = syn[i];
    }
    // Gaussian elimination over GF(256).
    for col in 0..n {
        let mut pivot = col;
        while pivot < n && mat[pivot][col] == 0 {
            pivot += 1;
        }
        if pivot == n {
            return Err(Error::SectionMap("Reed-Solomon: singular system".into()));
        }
        if pivot != col {
            mat.swap(col, pivot);
        }
        let inv = gf_inv(mat[col][col], &exp, &log);
        for cell in &mut mat[col][col..=n] {
            *cell = gf_mul(*cell, inv, &exp, &log);
        }
        for row_idx in 0..n {
            if row_idx == col {
                continue;
            }
            let factor = mat[row_idx][col];
            if factor == 0 {
                continue;
            }
            // Read pivot row, write into this row — two disjoint rows, so
            // we can't express this with iter_mut on a single row without
            // split_at_mut gymnastics.
            #[allow(clippy::needless_range_loop)]
            for j in col..=n {
                let sub = gf_mul(factor, mat[col][j], &exp, &log);
                mat[row_idx][j] ^= sub;
            }
        }
    }
    for (k, &pos) in roots.iter().enumerate() {
        let magnitude = mat[k][n];
        // Apply correction at the codeword position corresponding to α^pos.
        // Positions in the codeword are indexed from left (MSB) to right (LSB).
        let codeword_idx = CODEWORD_BYTES - 1 - pos;
        if codeword_idx < codeword.len() {
            codeword[codeword_idx] ^= magnitude;
        }
    }
    // Verify zero syndromes after correction.
    let syn_after = syndromes(codeword);
    if syn_after.iter().all(|&b| b == 0) {
        Ok(())
    } else {
        Err(Error::SectionMap(
            "Reed-Solomon: decode did not converge".into(),
        ))
    }
}

// ================================================================
// L4-57 / #279 — R21+ RS-FEC stream (multi-codeword) decode
// ================================================================

/// Verify every 255-byte codeword in a block of concatenated codewords.
///
/// R2004+ section payloads that carry Reed-Solomon FEC are stored as
/// N×255 bytes (N ≥ 1). This helper walks the block codeword-by-codeword
/// and calls [`verify`] on each; short-circuits on the first un-
/// correctable failure.
///
/// # Errors
/// - [`Error::SectionMap`] if `block.len()` is not a multiple of
///   [`CODEWORD_BYTES`] (ODA §4.1 requires the block length be an
///   integer multiple of 255 with the final chunk zero-padded).
/// - Whatever [`verify`] surfaces on the first un-correctable codeword.
///
/// On `Ok`, the block is correction-applied in place: each corrupted
/// byte has been XOR'd with the computed error magnitude.
pub fn verify_stream(block: &mut [u8]) -> Result<()> {
    if block.len() % CODEWORD_BYTES != 0 {
        return Err(Error::SectionMap(format!(
            "Reed-Solomon stream must be a multiple of {CODEWORD_BYTES} bytes, got {}",
            block.len()
        )));
    }
    for chunk in block.chunks_exact_mut(CODEWORD_BYTES) {
        verify(chunk)?;
    }
    Ok(())
}

/// Decode a systematic-RS stream into the message bytes.
///
/// Each 255-byte codeword carries 239 message bytes followed by 16
/// parity bytes. This helper calls [`verify_stream`] to correct
/// in-place, then concatenates the message portions into
/// `message_out` (extending it in place — does not clear).
///
/// If `message_len` is Some, the output is truncated to that many
/// bytes. Use this when the caller knows the original message length
/// (e.g. from the surrounding section-page header) — the final
/// codeword's zero padding is stripped via the truncation rather
/// than heuristically. If `message_len` is None, every message
/// portion is appended verbatim including any trailing zero padding.
///
/// # Errors
/// Same as [`verify_stream`]. Additionally, an explicit `message_len`
/// that exceeds the total message capacity (`N * MESSAGE_BYTES`)
/// returns [`Error::SectionMap`].
pub fn decode_stream(
    block: &mut [u8],
    message_out: &mut Vec<u8>,
    message_len: Option<usize>,
) -> Result<()> {
    verify_stream(block)?;
    let codeword_count = block.len() / CODEWORD_BYTES;
    let total_capacity = codeword_count * MESSAGE_BYTES;
    if let Some(len) = message_len {
        if len > total_capacity {
            return Err(Error::SectionMap(format!(
                "Reed-Solomon decode: message_len {len} exceeds codeword capacity {total_capacity}"
            )));
        }
    }
    // Append each codeword's first 239 bytes.
    for chunk in block.chunks_exact(CODEWORD_BYTES) {
        message_out.extend_from_slice(&chunk[..MESSAGE_BYTES]);
    }
    if let Some(len) = message_len {
        // Trim padding from the last codeword.
        let extra = total_capacity - len;
        message_out.truncate(message_out.len() - extra);
    }
    Ok(())
}

/// Compute the minimum number of 255-byte codewords needed to carry
/// `message_bytes`. Useful for sizing buffers before
/// [`decode_stream`].
pub fn codewords_for_message(message_bytes: usize) -> usize {
    message_bytes.div_ceil(MESSAGE_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gf_tables_are_multiplicative_group() {
        let (exp, log) = gf_tables();
        // α^255 wraps to α^0 = 1 in GF(2^8).
        assert_eq!(exp[0], 1);
        // log(1) = 0.
        assert_eq!(log[1], 0);
        // Multiplicative inverse: α · α^-1 = 1 for a nonzero element.
        for a in 1u8..=255 {
            let inv = gf_inv(a, &exp, &log);
            assert_eq!(gf_mul(a, inv, &exp, &log), 1, "a={}", a);
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
