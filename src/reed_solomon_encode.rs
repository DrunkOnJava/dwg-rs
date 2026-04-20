//! Reed-Solomon (255, 239) codeword encoder over GF(256) (L12-10, task #383).
//!
//! Given a 239-byte message, append 16 parity bytes per the ODA Open Design
//! Specification v5.4.1 §4.1 Reed-Solomon FEC convention. The resulting
//! 255-byte codeword is the inverse of what [`crate::reed_solomon::verify`]
//! consumes — the parity bytes are computed from the generator polynomial
//! `g(x) = Π (x − α^i)` for `i` in `1..=16`, where α is the canonical
//! generator 2 of GF(256) under the AES primitive polynomial `0x11D`.
//!
//! # Encoding algorithm
//!
//! 1. Multiply the message polynomial m(x) by x^16 (shift left 16 bytes).
//! 2. Divide by g(x) in GF(256); the remainder r(x) has degree < 16.
//! 3. The codeword is `m(x) · x^16 + r(x)` — verifiable via the decoder's
//!    zero-syndrome check because `codeword = g(x) · q(x)` by construction.
//!
//! # Tables
//!
//! The log / antilog tables for GF(256) are rebuilt locally instead of
//! imported from [`crate::reed_solomon`] — those helpers are
//! module-private. The tables are small (512 bytes total) and cheap to
//! recompute each call; a future optimization could make them `const`.
//!
//! # Invariant
//!
//! Every output of [`encode`] must pass [`crate::reed_solomon::verify`]
//! without correction. Tests below lock that property in for the empty
//! message, all-ones, random-ish patterns, and the boundary `0xFF` row.

use crate::error::{Error, Result};

/// Number of parity bytes per codeword — must match
/// [`crate::reed_solomon::PARITY_BYTES`].
pub const PARITY_BYTES: usize = 16;
/// Full codeword length — 255 bytes per GF(256) `(n=255)` convention.
pub const CODEWORD_BYTES: usize = 255;
/// Message length — codeword minus parity.
pub const MESSAGE_BYTES: usize = CODEWORD_BYTES - PARITY_BYTES;

/// Build the GF(256) log/antilog tables for the AES primitive polynomial
/// `0x11D` with generator α = 2. Matches the private tables in
/// [`crate::reed_solomon`]; the values are canonical for this field.
fn gf_tables() -> ([u8; 256], [u8; 256]) {
    let mut exp = [0u8; 256];
    let mut log = [0u8; 256];
    let mut x: u16 = 1;
    // Iterating by index is intentional — we write to two disjoint
    // arrays on each step. `enumerate()` over either can't express that.
    #[allow(clippy::needless_range_loop)]
    for i in 0..255 {
        exp[i] = x as u8;
        log[x as usize] = i as u8;
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= 0x11D;
        }
    }
    (exp, log)
}

fn gf_mul(a: u8, b: u8, exp: &[u8; 256], log: &[u8; 256]) -> u8 {
    if a == 0 || b == 0 {
        return 0;
    }
    let s = log[a as usize] as u16 + log[b as usize] as u16;
    exp[(s % 255) as usize]
}

/// Compute the Reed-Solomon generator polynomial `g(x) = Π (x − α^i)` for
/// `i` in `1..=2t` where `t = PARITY_BYTES / 2`. Degree is `2t` = 16 so
/// the returned slice has 17 coefficients, indexed from lowest to highest
/// degree (`g[0]` is the constant term, `g[16] = 1`).
fn generator_poly(exp: &[u8; 256], log: &[u8; 256]) -> [u8; PARITY_BYTES + 1] {
    // Start with g(x) = 1.
    let mut g = [0u8; PARITY_BYTES + 1];
    g[0] = 1;
    // Multiply by (x − α^i) = (x + α^i) in GF(256), for i = 1..=PARITY_BYTES.
    // `deg` is the current polynomial degree; after the loop it equals PARITY_BYTES.
    for (deg, i) in (1..=PARITY_BYTES).enumerate() {
        let alpha_i = exp[i % 255];
        // new_g = g(x) * (x + α^i)
        //       = g(x) * x + α^i * g(x)
        let mut new_g = [0u8; PARITY_BYTES + 1];
        // Shift up by 1 (multiply by x).
        for j in (1..=deg + 1).rev() {
            new_g[j] = g[j - 1];
        }
        // Add α^i · g(x) term-by-term.
        for j in 0..=deg {
            new_g[j] ^= gf_mul(g[j], alpha_i, exp, log);
        }
        g = new_g;
    }
    g
}

/// Encode a 239-byte `message` into a 255-byte Reed-Solomon codeword.
///
/// The codeword is `message || parity`, where `parity` is 16 bytes
/// chosen so the combined polynomial is divisible by the generator
/// `g(x) = Π (x − α^i)` for `i ∈ 1..=16`.
///
/// # Errors
///
/// Returns [`Error::SectionMap`] if `message.len() != MESSAGE_BYTES`
/// (239) — the (255, 239) shortened code requires a fixed message size.
///
/// # Invariant
///
/// `reed_solomon::verify(encode(m).unwrap())` is `Ok(())` for every
/// message `m` of length 239. Every codeword emitted by this function
/// has zero syndromes when evaluated by the decoder.
///
/// Spec reference: ODA Open Design Specification v5.4.1 §4.1 (R2004+
/// system-section Reed-Solomon FEC).
pub fn encode(message: &[u8]) -> Result<Vec<u8>> {
    if message.len() != MESSAGE_BYTES {
        return Err(Error::SectionMap(format!(
            "Reed-Solomon encode: message must be {} bytes, got {}",
            MESSAGE_BYTES,
            message.len()
        )));
    }
    let (exp, log) = gf_tables();
    let g = generator_poly(&exp, &log);

    // `working` stores the coefficients of m(x) · x^16 as a 255-byte
    // vector. Positions 0..MESSAGE_BYTES are the message bytes;
    // MESSAGE_BYTES..CODEWORD_BYTES are reserved for parity (zero-init).
    //
    // Note on convention: we treat index 0 as the HIGHEST-degree
    // coefficient (the leftmost byte of the codeword) to match the
    // decoder's `gf_poly_eval`, which computes Horner's rule starting
    // from `codeword[0]`. Under that convention, multiplying by x^16
    // corresponds to appending 16 zero parity bytes to the END — which
    // is exactly what the codeword layout encodes.
    let mut working = vec![0u8; CODEWORD_BYTES];
    working[..MESSAGE_BYTES].copy_from_slice(message);

    // Polynomial long division: reduce `working` modulo g(x).
    // For each message byte (highest-degree first), compute the
    // coefficient that kills the leading term, then subtract coef · g(x)
    // from the current position.
    for i in 0..MESSAGE_BYTES {
        let coef = working[i];
        if coef == 0 {
            continue;
        }
        // g has degree PARITY_BYTES; top coefficient is 1 so divison is
        // direct. For j in 0..=PARITY_BYTES: working[i+j] ^= coef · g[deg - j]
        // Because our `g` is indexed low-to-high, the coefficient for
        // term x^(PARITY_BYTES - j) is g[PARITY_BYTES - j]. Align to the
        // leading byte of the divisor.
        for j in 0..=PARITY_BYTES {
            let g_coef = g[PARITY_BYTES - j];
            working[i + j] ^= gf_mul(coef, g_coef, &exp, &log);
        }
    }
    // After division, working[0..MESSAGE_BYTES] is zeros; the original
    // message plus the parity tail (working[MESSAGE_BYTES..]) form the
    // codeword.
    let mut codeword = vec![0u8; CODEWORD_BYTES];
    codeword[..MESSAGE_BYTES].copy_from_slice(message);
    codeword[MESSAGE_BYTES..].copy_from_slice(&working[MESSAGE_BYTES..]);
    Ok(codeword)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reed_solomon;

    #[test]
    fn encode_rejects_wrong_message_length() {
        assert!(encode(&[0u8; 100]).is_err());
        assert!(encode(&[0u8; 240]).is_err());
        assert!(encode(&[]).is_err());
    }

    #[test]
    fn encoded_all_zero_message_has_zero_syndromes() {
        let msg = vec![0u8; MESSAGE_BYTES];
        let mut cw = encode(&msg).unwrap();
        assert_eq!(cw.len(), CODEWORD_BYTES);
        // Intact codeword — verify must succeed without correction.
        reed_solomon::verify(&mut cw).expect("encoded codeword must be intact");
    }

    #[test]
    fn encoded_all_ones_message_verifies() {
        let msg = vec![0xFFu8; MESSAGE_BYTES];
        let mut cw = encode(&msg).unwrap();
        reed_solomon::verify(&mut cw).expect("all-0xFF encodes cleanly");
    }

    #[test]
    fn encoded_counter_pattern_verifies() {
        let msg: Vec<u8> = (0..MESSAGE_BYTES).map(|i| (i & 0xFF) as u8).collect();
        let mut cw = encode(&msg).unwrap();
        // Sanity: message section of codeword equals the input.
        assert_eq!(&cw[..MESSAGE_BYTES], msg.as_slice());
        reed_solomon::verify(&mut cw).expect("counter pattern encodes cleanly");
    }

    #[test]
    fn encoded_random_ish_pattern_verifies() {
        // A slightly less regular byte stream than the counter; LCG-style.
        let mut msg = Vec::with_capacity(MESSAGE_BYTES);
        let mut state: u32 = 0xDEAD_BEEF;
        for _ in 0..MESSAGE_BYTES {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            msg.push(((state >> 16) & 0xFF) as u8);
        }
        let mut cw = encode(&msg).unwrap();
        reed_solomon::verify(&mut cw).expect("LCG pattern encodes cleanly");
    }

    #[test]
    fn encoded_codeword_is_exactly_255_bytes() {
        let msg = vec![0x42u8; MESSAGE_BYTES];
        let cw = encode(&msg).unwrap();
        assert_eq!(cw.len(), 255);
        // And the leading MESSAGE_BYTES of the codeword is the message
        // — systematic encoding preserves the input.
        assert_eq!(&cw[..MESSAGE_BYTES], msg.as_slice());
    }
}
