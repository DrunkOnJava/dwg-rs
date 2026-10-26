//! Reed-Solomon encoder for the two codes of R2007 pages — ODA spec §5.13.
//!
//! [`encode_with`] appends a code's parity to a message so that
//! [`crate::reed_solomon::Code::verify`] accepts the codeword unchanged;
//! [`encode`] does it for the system-page code. The layout is the one
//! AutoCAD writes, measured in `crate::reed_solomon`: the message is the
//! codeword's first *k* bytes, the parity follows, byte *i* is the
//! coefficient of x^i, and the generator's roots are α^1 … α^(n-k).
//! `examples/probe_r2007_rs.rs` re-encodes every codeword of the corpus's
//! R2007 files and gets AutoCAD's parity bytes back.
//!
//! Read the other way round — byte 0 as the highest power of *y* — the
//! codeword is an ordinary systematic codeword whose generator has the
//! roots α^-1 … α^-(n-k), so the parity is the remainder of the message,
//! shifted by n-k, divided by that generator.

use crate::error::{Error, Result};
use crate::reed_solomon::{Code, Field};

pub use crate::reed_solomon::{CODEWORD_BYTES, MESSAGE_BYTES, PARITY_BYTES};

/// The generator Π (y + α^-j), j = 1 … n-k, highest degree first.
fn generator(code: Code, f: &Field) -> Vec<u8> {
    let mut g = vec![1u8];
    for j in 1..=code.parity_bytes() {
        let root = f.alpha_inverse(j);
        let mut next = g.clone();
        next.push(0);
        for (i, &c) in g.iter().enumerate() {
            next[i + 1] ^= f.mul(c, root);
        }
        g = next;
    }
    g
}

/// Encode a `k`-byte message as a 255-byte codeword of `code` (§5.13):
/// the message, then its `n - k` parity bytes.
pub fn encode_with(code: Code, message: &[u8]) -> Result<Vec<u8>> {
    let k = code.message_bytes();
    if message.len() != k {
        return Err(Error::SectionMap(format!(
            "Reed-Solomon encode: message must be {k} bytes, got {}",
            message.len()
        )));
    }
    let f = Field::new(code.primitive_polynomial());
    let g = generator(code, &f);
    let mut work = vec![0u8; CODEWORD_BYTES];
    work[..k].copy_from_slice(message);
    for i in 0..k {
        let coef = work[i];
        if coef == 0 {
            continue;
        }
        for (j, &gj) in g.iter().enumerate().skip(1) {
            work[i + j] ^= f.mul(coef, gj);
        }
    }
    work[..k].copy_from_slice(message);
    Ok(work)
}

/// Encode a 239-byte message as a system-page codeword ([`Code::SYSTEM_PAGE`]).
pub fn encode(message: &[u8]) -> Result<Vec<u8>> {
    encode_with(Code::SYSTEM_PAGE, message)
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
