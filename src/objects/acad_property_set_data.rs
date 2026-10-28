//! ACAD_PROPERTYSET_DATA — BIM-style tagged metadata attached to
//! entities by AutoCAD Architecture / MEP (spec §19.6.11).
//!
//! Property sets are the vertical applications' answer to IFC
//! psets: each entity can carry one or more named property-set
//! instances whose shape is defined by an
//! ACAD_PROPERTYSET_DEFINITION object elsewhere in the dictionary
//! tree. This decoder reads the *instance* — a list of (name,
//! typed value) tuples stamped with the definition's name.
//!
//! # Stream shape
//!
//! ```text
//! TV      propset_definition_name     -- reference to the definition's name
//! BL      num_properties              -- ≤ 1000
//! // For each property:
//! TV      property_name
//! BL      data_type                   -- 1=int, 2=real, 3=text, 4=enum, 5=date
//! // Typed payload, dispatched on data_type:
//! //   1 → BL (32-bit signed int)
//! //   2 → BD (IEEE 754 double)
//! //   3 → TV (variable-length string)
//! //   4 → BL (enum discriminator, 0-based ordinal into the definition)
//! //   5 → BD (Julian day, per AutoCAD's internal date representation)
//! ```
//!
//! # Why separate from XData
//!
//! XData is per-application, opaque, and defined only by the APPID
//! that registered it. Property sets are *schema'd* — the
//! definition object spells out field names and types, so consumers
//! can render a propset as a uniform table without knowing about
//! the specific vertical application.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::tables::read_tv;
use crate::version::Version;

/// Sanity cap on property count per set.
pub const MAX_PROPERTIES: usize = 1000;

/// Data-type tag for a single property value, per spec §19.6.11.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyDataType {
    Integer = 1,
    Real = 2,
    Text = 3,
    Enum = 4,
    Date = 5,
}

impl PropertyDataType {
    /// Maps the property data-type code (spec §19.6.11: 1 integer, 2 real, 3 text, 4 enum, 5 date); anything else is an error.
    pub fn from_raw(raw: u32) -> Result<Self> {
        match raw {
            1 => Ok(Self::Integer),
            2 => Ok(Self::Real),
            3 => Ok(Self::Text),
            4 => Ok(Self::Enum),
            5 => Ok(Self::Date),
            _ => Err(Error::SectionMap(format!(
                "ACAD_PROPERTYSET_DATA data_type {raw} outside spec §19.6.11 range 1..=5"
            ))),
        }
    }
}

/// A single property value within a property set.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    Integer(i32),
    Real(f64),
    Text(String),
    Enum(u32),
    /// AutoCAD Julian day (double-precision day number, fractional
    /// day = time-of-day). No conversion to chrono / OffsetDateTime
    /// here — callers decide how to interpret.
    Date(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropertySetProperty {
    pub name: String,
    pub value: PropertyValue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AcadPropertySetData {
    pub propset_definition_name: String,
    pub properties: Vec<PropertySetProperty>,
}

/// Decodes the `AcadPropertySetData` payload that follows the common object header.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<AcadPropertySetData> {
    let propset_definition_name = read_tv(c, version)?;
    let num_properties = c.read_bl()? as usize;
    if num_properties > MAX_PROPERTIES {
        return Err(Error::SectionMap(format!(
            "ACAD_PROPERTYSET_DATA claims {num_properties} properties (>{MAX_PROPERTIES} sanity cap)"
        )));
    }
    let mut properties = Vec::with_capacity(num_properties);
    for _ in 0..num_properties {
        let name = read_tv(c, version)?;
        let raw_type = c.read_bl()? as u32;
        let dtype = PropertyDataType::from_raw(raw_type)?;
        let value = match dtype {
            PropertyDataType::Integer => PropertyValue::Integer(c.read_bl()?),
            PropertyDataType::Real => PropertyValue::Real(c.read_bd()?),
            PropertyDataType::Text => PropertyValue::Text(read_tv(c, version)?),
            PropertyDataType::Enum => PropertyValue::Enum(c.read_bl()? as u32),
            PropertyDataType::Date => PropertyValue::Date(c.read_bd()?),
        };
        properties.push(PropertySetProperty { name, value });
    }
    Ok(AcadPropertySetData {
        propset_definition_name,
        properties,
    })
}
