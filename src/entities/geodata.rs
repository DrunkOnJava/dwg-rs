//! GEODATA object (§20.4.78, `AcDbGeoData`).
//!
//! GEODATA attaches a geographic coordinate reference system to the
//! model-space block: a design point in the drawing, the reference
//! point it maps to in that system, units and scale, the north and up
//! directions, the coordinate system definition itself, and a mesh of
//! source and destination points. It is a non-entity object with a
//! class number from `AcDb:Classes`, so the dispatcher routes it by its
//! DXF class name.
//!
//! # Field list (§20.4.78)
//!
//! After the common object data (§20.1):
//!
//! ```text
//! BL   object_version                -- 1 AutoCAD 2009, 2 AutoCAD 2010, 3 AutoCAD 2013
//! H    host block                    -- soft pointer, in the handle stream
//! BS   design coordinate type        -- 0 unknown, 1 local grid, 2 projected, 3 geographic
//! (object_version >= 2)
//!   3BD  design point
//!   3BD  reference point
//!   BD   horizontal unit scale
//!   BL   horizontal units
//!   BD   vertical unit scale
//!   BL   vertical units
//!   3BD  up direction
//!   2RD  north direction             -- the spec prints 3RD; see below
//!   BL   scale estimation method
//!   BD   user scale factor
//!   B    sea level correction
//!   BD   sea level elevation
//!   BD   coordinate projection radius
//!   TV   coordinate system definition
//!   TV   GeoRSS tag
//! (object_version 1)
//!   3BD  reference point
//!   BL   horizontal units
//!   3BD  design point
//!   3BD  obsolete (0, 0, 0)
//!   3BD  up direction
//!   BD   north direction angle (radians, clockwise from (0, 1))
//!   3BD  obsolete (1, 1, 1)
//!   TV   coordinate system definition (WKT)
//!   TV   GeoRSS tag
//!   BD   horizontal unit scale
//!   TV   obsolete datum name
//!   TV   obsolete WKT
//! TV   observation from tag
//! TV   observation to tag
//! TV   observation coverage tag
//! BL   mesh point count, then per point: 2RD source, 2RD destination
//! BL   mesh face count, then per face: BL, BL, BL
//! (R2007 and earlier files) the civil block:
//!   B    has civil data
//!   B    (false)
//!   RD   reference point Y, RD reference point X,
//!   RD   reference point Y, RD reference point X
//!   BL   0, BL 0
//!   2RD  (0, 0), 2RD (0, 0)
//!   B    (false)
//!   BD   north direction angle (degrees)
//!   BD   north direction angle (radians)
//!   BL   scale estimation method
//!   BD   user scale factor
//!   B    sea level correction
//!   BD   sea level elevation
//!   BD   coordinate projection radius
//! ```
//!
//! # Measured: the north direction is a `2RD`
//!
//! The corpus's one GEODATA record, handle `0x241` of ACadSharp's
//! `geoloc.dwg` (R2018, object version 3), closes on its string stream
//! with the north direction read as `2RD`. Read as the spec's `3RD`, the
//! third value is a denormal and the record overruns. DXF writes the same
//! direction as two groups, `12` and `22`. Every other value equals
//! AutoCAD's DXF export of the drawing: the design point (`10`), the
//! reference point (`11`), both unit scales and units (`40`, `91`, `41`,
//! `92`), the up direction (`210`), the north direction (`12`), the
//! scale estimation (`95`, `141`), the sea-level fields (`294`, `142`),
//! the projection radius (`143`), the coordinate system definition
//! (`303`/`301`), the GeoRSS tag (`302`), the three observation tags
//! (`305`-`307`), all 12 mesh points (`13`/`23`, `14`/`24`) and the face
//! count (`96`). `examples/probe_geodata.rs` makes that comparison.
//!
//! The object version 1 layout and the civil block of R2007 and earlier
//! files are as §20.4.78 prints them. The corpus has neither, so both
//! are unmeasured; the data-stream boundary check rejects a record they
//! do not fit.

use crate::bitcursor::BitCursor;
use crate::entities::{Point2D, Point3D, Vec3D, read_bd3, read_rd2};
use crate::error::{Error, Result};
use crate::objects::modern;
use crate::string_stream::StringReader;
use crate::version::Version;

/// Mesh points and faces each cost at least this many bits, so a count
/// larger than the remaining bits divided by it cannot be honest.
const MIN_MESH_POINT_BITS: usize = 256;
const MIN_MESH_FACE_BITS: usize = 6;

/// A decoded GEODATA record.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GeoData {
    /// 1 = AutoCAD 2009, 2 = AutoCAD 2010, 3 = AutoCAD 2013 (same layout as 2).
    pub object_version: u32,
    /// 0 unknown, 1 local grid, 2 projected grid, 3 geographic.
    pub design_coordinate_type: i16,
    pub design_point: Point3D,
    pub reference_point: Point3D,
    pub horizontal_unit_scale: f64,
    /// The units enumeration of §20.4.78 (1 inches, 2 feet, ... 6 meters, ...).
    pub horizontal_units: u32,
    pub vertical_unit_scale: f64,
    pub vertical_units: u32,
    pub up_direction: Vec3D,
    /// Object version 2 and later; zero on version 1.
    pub north_direction: Point2D,
    /// Object version 1 (and the civil block): radians, clockwise from (0, 1).
    pub north_direction_angle: f64,
    /// 1 none, 2 user specified, 3 grid scale at reference point, 4 prismoidal.
    pub scale_estimation_method: u32,
    pub user_scale_factor: f64,
    pub sea_level_correction: bool,
    pub sea_level_elevation: f64,
    pub coordinate_projection_radius: f64,
    pub coordinate_system_definition: String,
    pub geo_rss_tag: String,
    pub observation_from_tag: String,
    pub observation_to_tag: String,
    pub observation_coverage_tag: String,
    /// `(source, destination)` pairs.
    pub mesh_points: Vec<(Point2D, Point2D)>,
    pub mesh_faces: Vec<[u32; 3]>,
    /// R2007 and earlier files: whether the civil block says it holds data.
    pub has_civil_data: Option<bool>,
}

/// Decodes the GEODATA payload that follows the common object data.
///
/// `strings` is the R2007+ string stream; `None` reads `TV`s inline.
fn read_body(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<GeoData> {
    let mut g = GeoData {
        object_version: c.read_bl_u()?,
        design_coordinate_type: c.read_bs()?,
        ..GeoData::default()
    };
    if g.object_version >= 2 {
        g.design_point = read_bd3(c)?;
        g.reference_point = read_bd3(c)?;
        g.horizontal_unit_scale = c.read_bd()?;
        g.horizontal_units = c.read_bl_u()?;
        g.vertical_unit_scale = c.read_bd()?;
        g.vertical_units = c.read_bl_u()?;
        g.up_direction = read_bd3(c)?;
        g.north_direction = read_rd2(c)?;
        g.scale_estimation_method = c.read_bl_u()?;
        g.user_scale_factor = c.read_bd()?;
        g.sea_level_correction = c.read_b()?;
        g.sea_level_elevation = c.read_bd()?;
        g.coordinate_projection_radius = c.read_bd()?;
        g.coordinate_system_definition = modern::read_tv(c, strings, version)?;
        g.geo_rss_tag = modern::read_tv(c, strings, version)?;
    } else {
        g.reference_point = read_bd3(c)?;
        g.horizontal_units = c.read_bl_u()?;
        g.design_point = read_bd3(c)?;
        let _obsolete_zero = read_bd3(c)?;
        g.up_direction = read_bd3(c)?;
        g.north_direction_angle = c.read_bd()?;
        let _obsolete_one = read_bd3(c)?;
        g.coordinate_system_definition = modern::read_tv(c, strings, version)?;
        g.geo_rss_tag = modern::read_tv(c, strings, version)?;
        g.horizontal_unit_scale = c.read_bd()?;
        let _obsolete_datum_name = modern::read_tv(c, strings, version)?;
        let _obsolete_wkt = modern::read_tv(c, strings, version)?;
    }
    g.observation_from_tag = modern::read_tv(c, strings, version)?;
    g.observation_to_tag = modern::read_tv(c, strings, version)?;
    g.observation_coverage_tag = modern::read_tv(c, strings, version)?;

    let num_points = c.read_bl_u()? as usize;
    check_count(num_points, MIN_MESH_POINT_BITS, c, "GEODATA mesh points")?;
    g.mesh_points.reserve(num_points);
    for _ in 0..num_points {
        g.mesh_points.push((read_rd2(c)?, read_rd2(c)?));
    }
    let num_faces = c.read_bl_u()? as usize;
    check_count(num_faces, MIN_MESH_FACE_BITS, c, "GEODATA mesh faces")?;
    g.mesh_faces.reserve(num_faces);
    for _ in 0..num_faces {
        g.mesh_faces
            .push([c.read_bl_u()?, c.read_bl_u()?, c.read_bl_u()?]);
    }

    if !version.is_r2010_plus() {
        g.has_civil_data = Some(c.read_b()?);
        let _false = c.read_b()?;
        for _ in 0..4 {
            let _reference_coordinate = c.read_rd()?;
        }
        let _zero_1 = c.read_bl()?;
        let _zero_2 = c.read_bl()?;
        let _origin_1 = read_rd2(c)?;
        let _origin_2 = read_rd2(c)?;
        let _false_2 = c.read_b()?;
        let _north_degrees = c.read_bd()?;
        g.north_direction_angle = c.read_bd()?;
        g.scale_estimation_method = c.read_bl_u()?;
        g.user_scale_factor = c.read_bd()?;
        g.sea_level_correction = c.read_b()?;
        g.sea_level_elevation = c.read_bd()?;
        g.coordinate_projection_radius = c.read_bd()?;
    }
    Ok(g)
}

fn check_count(n: usize, min_bits: usize, c: &BitCursor<'_>, what: &str) -> Result<()> {
    if n > c.remaining_bits() / min_bits {
        return Err(Error::SectionMap(format!(
            "{what}: count {n} cannot fit in the {} bits left",
            c.remaining_bits()
        )));
    }
    Ok(())
}

/// Decodes the GEODATA payload that follows the common object data,
/// reading every `TV` inline.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<GeoData> {
    read_body(c, &mut None, version)
}

/// Decode a GEODATA straight from its raw object payload, taking the
/// `TV`s from the R2007+ string stream when the file has one and
/// checking the data fields end exactly on the data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<GeoData> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = read_body(&mut split.data, &mut split.strings, version)?;
    split.finish("GEODATA")?;
    Ok(out)
}
