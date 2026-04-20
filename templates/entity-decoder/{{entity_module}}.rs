//! {{entity_name | upper}} entity (§{{spec_section}}).
//!
//! TODO: document stream shape per spec.

use crate::bitcursor::BitCursor;
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct {{entity_name}} {
    // TODO: add fields per spec §{{spec_section}}.
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<{{entity_name}}> {
    let _ = c;
    Ok({{entity_name}} {})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_minimal() {
        let w = BitWriter::new();
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let _ = decode(&mut c).unwrap();
    }
}
