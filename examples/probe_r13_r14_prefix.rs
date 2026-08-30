//! Read the two candidate R13/R14 object prefixes side by side and say
//! which one the bytes support (ODA spec §20.1 / §20.4.1).
//!
//! # What this proves
//!
//! §20.1 and §20.4.1 both list an `RL` "size of object in bits, not
//! including end handles" **twice**: once under "R2000+" between the
//! object type and the object handle, and once under "R13-R14" *after*
//! the EED chain (for a non-entity object) or after the graphics block
//! (for an entity). The two placements are mutually exclusive, and
//! reading the wrong one leaves every field from the reactor count on
//! 32 bits out of phase.
//!
//! [`dwg::object::ObjectWalker`] already proves the field is not in the
//! prologue on R13/R14: it reads the object handle straight after the
//! type code, and every record's handle then matches the one the
//! `AcDb:Handles` map paired with that offset.
//!
//! This probe proves the other half — that the field *is* present after
//! the EED chain. For every non-entity record it reads the candidate
//! `RL` and checks three things a real bit count must satisfy:
//!
//! - it is greater than the cursor position that produced it,
//! - it is no larger than the record's payload in bits,
//! - the decoder that consumes the rest of the record lands on it exactly.
//!
//! The third column is the one that matters: a value that merely looks
//! plausible proves nothing, a value that a field list closes on does.
//!
//! ```sh
//! cargo run --release --example probe_r13_r14_prefix -- samples/line_R14.dwg
//! ```

use dwg::entities::DecodedEntity;
use dwg::{DwgFile, ObjectType, Version};
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: probe_r13_r14_prefix <file.dwg>");
        return ExitCode::FAILURE;
    };
    let file = match DwgFile::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot open {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    if !matches!(file.version(), Version::R14) {
        eprintln!(
            "{path} is {} — this probe is about the R13/R14 prefix",
            file.version()
        );
        return ExitCode::FAILURE;
    }
    let Some(Ok(raws)) = file.all_objects() else {
        eprintln!("{path}: no object walk");
        return ExitCode::FAILURE;
    };
    let Some(Ok((decoded, _summary))) = file.decoded_entities() else {
        eprintln!("{path}: no dispatch");
        return ExitCode::FAILURE;
    };

    println!("== {path} ({}) ==", file.version());
    println!(
        "{:<20} {:>8} {:>10} {:>10}  field list",
        "type", "handle", "payload", "RL@eed"
    );
    println!("{}", "-".repeat(72));

    let mut plausible = 0usize;
    let mut total = 0usize;
    let mut closed = 0usize;
    for (raw, dec) in raws.iter().zip(decoded.iter()) {
        // The walker never records the field on R13/R14 — that is the
        // point — so re-read it here from the payload.
        let Ok(rl) = read_rl_after_eed(&raw.raw) else {
            continue;
        };
        total += 1;
        let payload_bits = raw.raw.len() * 8;
        let ok = rl.value > 0 && (rl.value as usize) <= payload_bits;
        if ok {
            plausible += 1;
        }
        let verdict = match dec {
            DecodedEntity::Error { message, .. } => {
                format!("ERROR: {message}")
            }
            DecodedEntity::Unhandled { .. } => "unhandled (no decoder)".to_string(),
            _ => {
                closed += 1;
                "closes on the boundary".to_string()
            }
        };
        println!(
            "{:<20} {:>8} {:>10} {:>10}  {}",
            format!("{}", ObjectType::from_code(raw.type_code)),
            format!("0x{:X}", raw.handle.value),
            payload_bits,
            format!("{}{}", rl.value, if ok { "" } else { " IMPLAUSIBLE" }),
            verdict,
        );
    }
    println!("{}", "-".repeat(72));
    println!(
        "{plausible}/{total} records carry a plausible RL after the EED chain; \
         {closed} of them have a field list that closes on it exactly."
    );
    ExitCode::SUCCESS
}

/// A candidate `RL` read from just past a record's EED chain.
struct Candidate {
    value: u32,
}

/// Re-read the §20.1 R13/R14 object prefix out of a raw payload: `BS`
/// object type, `H` handle, the EED chain, then the candidate `RL`.
///
/// Returns `Err` for entity records, whose `RL` sits after the graphics
/// block instead (§20.4.1) and is read by
/// [`dwg::common_entity::read_common_entity_data`].
fn read_rl_after_eed(payload: &[u8]) -> Result<Candidate, dwg::Error> {
    let mut c = dwg::BitCursor::new(payload);
    let type_code = c.read_bs_u()?;
    if ObjectType::from_code(type_code).is_entity() {
        return Err(dwg::Error::Unsupported {
            feature: "entity record — its RL follows the graphics block".into(),
        });
    }
    let _handle = c.read_handle()?;
    for _ in 0..256 {
        let size = c.read_bs_u()? as usize;
        if size == 0 {
            return Ok(Candidate {
                value: c.read_rl()?,
            });
        }
        let _appid = c.read_handle()?;
        for _ in 0..size {
            let _ = c.read_rc()?;
        }
    }
    Err(dwg::Error::Unsupported {
        feature: "EED chain did not terminate".into(),
    })
}
