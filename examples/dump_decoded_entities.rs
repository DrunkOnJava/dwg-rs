//! Dump the decoded field values of every entity in a DWG file.
//!
//! This is the "spot-check correctness" tool. The coverage report tells
//! you which objects *successfully decoded*; this tool tells you what
//! the decoded values *are*. A LINE reported as decoded is only
//! actually decoded correctly if its endpoints are finite, plausible,
//! and match what AutoCAD shows when it opens the same file.
//!
//! Output is human-readable and intended for eyeballing against a
//! reference tool (AutoCAD, BricsCAD, LibreCAD, any DWG viewer). The
//! companion test [`tests/r2013_entity_values.rs`] pins a
//! machine-checkable subset of these invariants (finite coords,
//! non-negative radii, 2D flag consistency).
//!
//! ```bash
//! cargo run --release --example dump_decoded_entities -- path/to/file.dwg
//! cargo run --release --example dump_decoded_entities -- ../../samples/line_2013.dwg
//! ```
//!
//! Exit codes:
//! - `0` — file opened and at least one entity was decoded
//! - `1` — file open / decode infrastructure failed
//! - `2` — no entities decoded (format-level issue, not decoder)

use dwg::DwgFile;
use dwg::entities::DecodedEntity;
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: dump_decoded_entities <file.dwg>");
        return ExitCode::FAILURE;
    };

    let file = match DwgFile::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("open failed ({path}): {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("=== {path} ===");
    println!("version: {}", file.version());

    match file.decoded_entities() {
        Some(Ok((entities, summary))) => {
            println!(
                "decoded: {}, unhandled: {}, errored: {}, ratio: {:.1}%",
                summary.decoded,
                summary.unhandled,
                summary.errored,
                summary.decoded_ratio() * 100.0
            );
            println!();

            if entities.is_empty() {
                eprintln!(
                    "note: zero entities decoded — {} supports handle walking but \
                     entity-decoded count is zero",
                    file.version()
                );
                return ExitCode::from(2);
            }

            let mut typed = 0usize;
            for (i, e) in entities.iter().enumerate() {
                print_entity(i, e);
                if !matches!(
                    e,
                    DecodedEntity::Unhandled { .. } | DecodedEntity::Error { .. }
                ) {
                    typed += 1;
                }
            }

            println!();
            println!("---");
            println!("typed variants: {typed}");
            if typed == 0 {
                eprintln!("no typed entity variants — nothing to validate");
                return ExitCode::from(2);
            }
            ExitCode::SUCCESS
        }
        Some(Err(e)) => {
            eprintln!("decoded_entities returned error: {e}");
            ExitCode::FAILURE
        }
        None => {
            eprintln!(
                "format {} does not support handle-driven entity iteration yet \
                 (R14/R2000/R2007 need the object-stream layout work)",
                file.version()
            );
            ExitCode::FAILURE
        }
    }
}

fn print_entity(i: usize, e: &DecodedEntity) {
    match e {
        DecodedEntity::Line(l) => {
            println!(
                "[{i}] LINE  start=({:.6}, {:.6}, {:.6}) end=({:.6}, {:.6}, {:.6}) \
                 thickness={:.6} is_2d={}",
                l.start.x, l.start.y, l.start.z, l.end.x, l.end.y, l.end.z, l.thickness, l.is_2d
            );
            println!(
                "       extrusion=({:.6}, {:.6}, {:.6})",
                l.extrusion.x, l.extrusion.y, l.extrusion.z
            );
        }
        DecodedEntity::Circle(c) => {
            println!(
                "[{i}] CIRCLE  center=({:.6}, {:.6}, {:.6}) radius={:.6} thickness={:.6}",
                c.center.x, c.center.y, c.center.z, c.radius, c.thickness
            );
            println!(
                "        extrusion=({:.6}, {:.6}, {:.6})",
                c.extrusion.x, c.extrusion.y, c.extrusion.z
            );
        }
        DecodedEntity::Arc(a) => {
            println!(
                "[{i}] ARC  center=({:.6}, {:.6}, {:.6}) radius={:.6} \
                 start_angle={:.6}rad end_angle={:.6}rad thickness={:.6}",
                a.center.x,
                a.center.y,
                a.center.z,
                a.radius,
                a.start_angle,
                a.end_angle,
                a.thickness
            );
            println!(
                "       extrusion=({:.6}, {:.6}, {:.6})",
                a.extrusion.x, a.extrusion.y, a.extrusion.z
            );
        }
        DecodedEntity::Point(p) => {
            println!(
                "[{i}] POINT  position=({:.6}, {:.6}, {:.6}) thickness={:.6} x_axis_angle={:.6}rad",
                p.position.x, p.position.y, p.position.z, p.thickness, p.x_axis_angle
            );
        }
        DecodedEntity::Ellipse(el) => {
            println!(
                "[{i}] ELLIPSE  center=({:.6}, {:.6}, {:.6}) axis_ratio={:.6} \
                 start_param={:.6} end_param={:.6}",
                el.center.x,
                el.center.y,
                el.center.z,
                el.axis_ratio,
                el.start_param,
                el.end_param
            );
            println!(
                "         major_axis=({:.6}, {:.6}, {:.6})",
                el.major_axis.x, el.major_axis.y, el.major_axis.z
            );
        }
        DecodedEntity::Text(t) => {
            println!(
                "[{i}] TEXT  insertion=({:.6}, {:.6}) elevation={:.6} height={:.6} \
                 rotation={:.6}rad text={:?}",
                t.insertion_point.x,
                t.insertion_point.y,
                t.elevation,
                t.height,
                t.rotation_angle,
                truncate(&t.text, 80)
            );
        }
        DecodedEntity::LwPolyline(lp) => {
            println!(
                "[{i}] LWPOLYLINE  flag=0x{:04X} closed={} vertex_count={} bulge_count={} \
                 elev={:?} thickness={:?}",
                lp.flag,
                lp.closed,
                lp.vertices.len(),
                lp.bulges.len(),
                lp.elevation,
                lp.thickness
            );
            for (vi, vp) in lp.vertices.iter().enumerate().take(4) {
                println!("       vertex[{vi}]=({:.6}, {:.6})", vp.x, vp.y);
            }
            if lp.vertices.len() > 4 {
                println!("       ... ({} more)", lp.vertices.len() - 4);
            }
        }
        DecodedEntity::Dimension(d) => {
            let kind = match d {
                dwg::entities::dimension::Dimension::Ordinate(_) => "Ordinate",
                dwg::entities::dimension::Dimension::Linear(_) => "Linear",
                dwg::entities::dimension::Dimension::Aligned(_) => "Aligned",
                dwg::entities::dimension::Dimension::Angular3Pt(_) => "Angular3Pt",
                dwg::entities::dimension::Dimension::Angular2Line(_) => "Angular2Line",
                dwg::entities::dimension::Dimension::Radius(_) => "Radius",
                dwg::entities::dimension::Dimension::Diameter(_) => "Diameter",
            };
            println!("[{i}] DIMENSION  subtype={kind}");
        }
        DecodedEntity::Block(b) => {
            println!("[{i}] BLOCK  name={:?}", truncate(&b.name, 64));
        }
        DecodedEntity::EndBlk(_) => {
            println!("[{i}] ENDBLK");
        }
        DecodedEntity::Insert(ins) => {
            println!(
                "[{i}] INSERT  insertion=({:.6}, {:.6}, {:.6}) scale=({:.6}, {:.6}, {:.6}) \
                 rotation={:.6}rad has_attribs={}",
                ins.insertion_point.x,
                ins.insertion_point.y,
                ins.insertion_point.z,
                ins.scale.x,
                ins.scale.y,
                ins.scale.z,
                ins.rotation,
                ins.has_attribs
            );
        }
        DecodedEntity::Spline(s) => {
            let knots_n = s.control.as_ref().map(|c| c.knots.len()).unwrap_or(0);
            let ctrl_n = s
                .control
                .as_ref()
                .map(|c| c.control_points.len())
                .unwrap_or(0);
            let fit_n = s.fit.as_ref().map(|f| f.fit_points.len()).unwrap_or(0);
            println!(
                "[{i}] SPLINE  scenario={} degree={:.1} knots={} control_pts={} fit_pts={}",
                s.scenario, s.degree, knots_n, ctrl_n, fit_n
            );
        }
        DecodedEntity::Solid(s) => {
            println!(
                "[{i}] SOLID  c1=({:.3},{:.3}) c2=({:.3},{:.3}) c3=({:.3},{:.3}) c4=({:.3},{:.3}) \
                 elevation={:.3}",
                s.corners[0].x,
                s.corners[0].y,
                s.corners[1].x,
                s.corners[1].y,
                s.corners[2].x,
                s.corners[2].y,
                s.corners[3].x,
                s.corners[3].y,
                s.elevation
            );
        }
        DecodedEntity::ThreeDFace(f) => {
            println!(
                "[{i}] 3DFACE  is_triangle={} invisible_edges=0x{:04X}",
                f.is_triangle, f.invisible_edges
            );
            for (ci, corner) in f.corners.iter().enumerate() {
                println!(
                    "        corner[{ci}]=({:.3}, {:.3}, {:.3})",
                    corner.x, corner.y, corner.z
                );
            }
        }
        DecodedEntity::Trace(t) => {
            // Trace is a newtype wrapper around Solid.
            println!(
                "[{i}] TRACE  c1=({:.3},{:.3}) c2=({:.3},{:.3}) c3=({:.3},{:.3}) c4=({:.3},{:.3}) \
                 elevation={:.3}",
                t.0.corners[0].x,
                t.0.corners[0].y,
                t.0.corners[1].x,
                t.0.corners[1].y,
                t.0.corners[2].x,
                t.0.corners[2].y,
                t.0.corners[3].x,
                t.0.corners[3].y,
                t.0.elevation
            );
        }
        DecodedEntity::Ray(r) => {
            println!(
                "[{i}] RAY  start=({:.3},{:.3},{:.3}) direction=({:.3},{:.3},{:.3})",
                r.start.x, r.start.y, r.start.z, r.direction.x, r.direction.y, r.direction.z
            );
        }
        DecodedEntity::XLine(xl) => {
            println!(
                "[{i}] XLINE  point=({:.3},{:.3},{:.3}) direction=({:.3},{:.3},{:.3})",
                xl.point.x, xl.point.y, xl.point.z, xl.direction.x, xl.direction.y, xl.direction.z
            );
        }
        DecodedEntity::MText(m) => {
            println!(
                "[{i}] MTEXT  insertion=({:.3},{:.3},{:.3}) rect_width={:.3} text={:?}",
                m.insertion_point.x,
                m.insertion_point.y,
                m.insertion_point.z,
                m.rect_width,
                truncate(&m.text, 80)
            );
        }
        DecodedEntity::Attrib(a) => {
            println!(
                "[{i}] ATTRIB  tag={:?} value={:?} invisible={}",
                a.tag,
                truncate(&a.text.text, 40),
                a.is_invisible()
            );
        }
        DecodedEntity::AttDef(ad) => {
            println!(
                "[{i}] ATTDEF  tag={:?} prompt={:?}",
                ad.tag,
                truncate(&ad.prompt, 40)
            );
        }
        DecodedEntity::Polyline(p) => {
            println!(
                "[{i}] POLYLINE  flag=0x{:04X} elevation={:.3} closed={} is_3d={} polyface={}",
                p.flag,
                p.elevation,
                p.is_closed(),
                p.is_3d(),
                p.is_polyface()
            );
        }
        DecodedEntity::Vertex(v) => {
            println!(
                "[{i}] VERTEX  location=({:.3},{:.3},{:.3}) flag=0x{:02X} bulge={:.3}",
                v.location.x, v.location.y, v.location.z, v.flag, v.bulge
            );
        }
        DecodedEntity::Leader(l) => {
            println!(
                "[{i}] LEADER  annot_type={} path_type={} points={}",
                l.annot_type,
                l.path_type,
                l.points.len()
            );
        }
        DecodedEntity::Image(_) => {
            println!("[{i}] IMAGE (raster image custom class)");
        }
        DecodedEntity::Hatch(_) => {
            println!("[{i}] HATCH");
        }
        DecodedEntity::MLeader(_) => {
            println!("[{i}] MLEADER (multileader custom class)");
        }
        DecodedEntity::Viewport(_) => {
            println!("[{i}] VIEWPORT (stub)");
        }
        DecodedEntity::Unhandled { type_code, kind } => {
            println!("[{i}] UNHANDLED  type_code=0x{type_code:04X} kind={kind:?}");
        }
        DecodedEntity::Error {
            type_code,
            kind,
            message,
        } => {
            println!("[{i}] ERROR  type_code=0x{type_code:04X} kind={kind:?} message={message}");
        }
        _ => {
            // DecodedEntity is #[non_exhaustive]; any future variant shows up here.
            println!("[{i}] <unknown-variant>");
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n).collect();
        out.push('…');
        out
    }
}
