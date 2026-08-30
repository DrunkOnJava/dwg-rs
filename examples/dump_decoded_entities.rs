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
//! Kept separate from `examples/coverage_report.rs` by design: that
//! example's audience is CI and humans wanting a corpus-wide
//! summary; this example's audience is a human spot-checking a
//! single file. Merging them would bury either signal in the
//! other's output.
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
                el.center.x, el.center.y, el.center.z, el.axis_ratio, el.start_param, el.end_param
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
        DecodedEntity::BlockRecord(b) => {
            println!(
                "[{i}] BLOCK_RECORD  name={:?} owned={:?}",
                truncate(&b.header.name, 64),
                b.num_owned_objects
            );
        }
        DecodedEntity::Ltype(l) => {
            println!(
                "[{i}] LTYPE  name={:?} desc={:?} pattern_length={:.6} dashes={}",
                truncate(&l.header.name, 64),
                truncate(&l.description, 80),
                l.pattern_length,
                l.dashes.len()
            );
        }
        DecodedEntity::Style(st) => {
            println!(
                "[{i}] STYLE  name={:?} font={:?} bigfont={:?} fixed_h={:.6} width={:.6} \
                 oblique={:.6} gen={} last_h={:.6} shape={} vertical={}",
                st.header.name,
                st.font_filename,
                st.bigfont_filename,
                st.fixed_height,
                st.width_factor,
                st.oblique_angle,
                st.generation,
                st.last_height,
                st.is_shape_file(),
                st.is_vertical()
            );
        }
        DecodedEntity::AppId(a) => {
            println!("[{i}] APPID  name={:?}", a.header.name);
        }
        DecodedEntity::Ucs(u) => {
            println!(
                "[{i}] UCS  name={:?} origin=({:.3},{:.3},{:.3}) ortho={}",
                u.header.name, u.origin.x, u.origin.y, u.origin.z, u.ortho_view_type
            );
        }
        DecodedEntity::View(v) => {
            println!(
                "[{i}] VIEW  name={:?} h={:.6} w={:.6} center=({:.6},{:.6}) \
                 dir=({:.3},{:.3},{:.3}) lens={:.3} mode={} render={} pspace={} assoc_ucs={}",
                v.header.name,
                v.view_height,
                v.view_width,
                v.view_center.x,
                v.view_center.y,
                v.view_direction.x,
                v.view_direction.y,
                v.view_direction.z,
                v.lens_length,
                v.view_mode,
                v.render_mode,
                v.is_paperspace,
                v.is_associated_ucs
            );
        }
        DecodedEntity::VPort(v) => {
            println!(
                "[{i}] VPORT  name={:?} h={:.6} aspect={:.6} center=({:.6},{:.6}) \
                 dir=({:.3},{:.3},{:.3}) lens={:.3} ll=({:.3},{:.3}) ur=({:.3},{:.3}) \
                 grid=({:.3},{:.3}) snap_spacing=({:.3},{:.3}) snap_rot={:.3} mode={} render={}",
                v.header.name,
                v.view_height,
                v.aspect_ratio,
                v.view_center.x,
                v.view_center.y,
                v.view_direction.x,
                v.view_direction.y,
                v.view_direction.z,
                v.lens_length,
                v.lower_left.x,
                v.lower_left.y,
                v.upper_right.x,
                v.upper_right.y,
                v.grid_spacing.x,
                v.grid_spacing.y,
                v.snap_spacing.x,
                v.snap_spacing.y,
                v.snap_rotation,
                v.view_mode,
                v.render_mode
            );
        }
        DecodedEntity::DimStyle(d) => {
            println!(
                "[{i}] DIMSTYLE  name={:?} dimscale={:.4} dimasz={:.4} dimtxt={:.4} \
                 dimexo={:.4} dimexe={:.4} dimcen={:.4} dimlfac={:.4} dimtad={} dimtolj={}",
                d.header.name,
                d.dimscale,
                d.dimasz,
                d.dimtxt,
                d.dimexo,
                d.dimexe,
                d.dimcen,
                d.dimlfac,
                d.dimtad,
                d.dimtolj
            );
        }
        DecodedEntity::Layer(l) => {
            println!(
                "[{i}] LAYER  name={:?} flags=0x{:04X} plot={} lineweight={} color={}",
                l.header.name, l.flags, l.plot_flag, l.lineweight, l.color_index
            );
        }
        DecodedEntity::EndBlk(_) => {
            println!("[{i}] ENDBLK");
        }
        DecodedEntity::Dictionary(d) => {
            let shown: Vec<&str> = d.keys.iter().take(6).map(String::as_str).collect();
            println!(
                "[{i}] DICTIONARY  entries={} cloning={} hard_owner={} keys={:?}{}",
                d.len(),
                d.cloning_flag,
                d.hard_owner,
                shown,
                if d.len() > shown.len() { " …" } else { "" }
            );
        }
        DecodedEntity::DictionaryVar(v) => {
            println!(
                "[{i}] DICTIONARYVAR  schema={} value={:?}",
                v.schema, v.value
            );
        }
        DecodedEntity::XRecord(x) => {
            println!(
                "[{i}] XRECORD  data_bytes={} cloning_flags={}",
                x.data.len(),
                x.cloning_flags
            );
        }
        DecodedEntity::Placeholder(p) => {
            println!("[{i}] ACDB_PLACEHOLDER  reactors={}", p.num_reactors);
        }
        DecodedEntity::Group(g) => {
            println!(
                "[{i}] GROUP  name={:?} unnamed={} selectable={} members={}",
                g.name, g.unnamed, g.selectable, g.num_members
            );
        }
        DecodedEntity::Scale(s) => {
            println!(
                "[{i}] SCALE  name={:?} paper={} drawing={} unit_scale={}",
                s.scale_name, s.paper_units, s.drawing_units, s.is_unit_scale
            );
        }
        DecodedEntity::VisualStyle(v) => {
            println!(
                "[{i}] VISUALSTYLE  description={:?} type={} internal_only={} \
                 face(model={} quality={} color_mode={} opacity={} specular={}) \
                 mono_color={:#010X}",
                v.description,
                v.internal_style_type,
                v.is_internal_use_only,
                v.face_lighting_model,
                v.face_lighting_quality,
                v.face_color_mode,
                v.face_opacity,
                v.face_specular,
                v.face_mono_color.rgb,
            );
            println!(
                "             edge(model={} color={:#010X} silhouette={:#010X} \
                 crease={} opacity={}) extended={} strings={:?}",
                v.edge_model,
                v.edge_color.rgb,
                v.edge_silhouette_color.rgb,
                v.edge_crease_angle,
                v.edge_opacity,
                v.extended.len(),
                v.trailing_strings,
            );
        }
        DecodedEntity::Layout(l) => {
            println!(
                "[{i}] LAYOUT  name={:?} tab={} flags={} viewports={} \
                 limmin=({}, {}) limmax=({}, {}) ucs_x=({}, {}, {}) ucs_y=({}, {}, {}) \
                 ortho={} elevation={}",
                l.layout_name,
                l.tab_order,
                l.flags,
                l.viewport_count,
                l.limmin.x,
                l.limmin.y,
                l.limmax.x,
                l.limmax.y,
                l.ucs_x_axis.x,
                l.ucs_x_axis.y,
                l.ucs_x_axis.z,
                l.ucs_y_axis.x,
                l.ucs_y_axis.y,
                l.ucs_y_axis.z,
                l.ucs_ortho_view_type,
                l.elevation,
            );
            print_plot_settings(&l.plot_settings, "            ");
        }
        DecodedEntity::PlotSettings(s) => {
            println!("[{i}] PLOTSETTINGS");
            print_plot_settings(s, "            ");
        }
        DecodedEntity::ImageDef(d) => {
            println!(
                "[{i}] IMAGEDEF  path={:?} size={:?} loaded={}",
                d.file_path, d.image_size_pixels, d.is_loaded
            );
        }
        DecodedEntity::Control { kind, control } => {
            println!(
                "[{i}] {kind}  entries={}{}",
                control.num_entries,
                match control.dimstyle_trailing_rc {
                    Some(rc) => format!(" trailing_rc={rc}"),
                    None => String::new(),
                }
            );
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
        DecodedEntity::SeqEnd(_) => {
            println!("[{i}] SEQEND");
        }
        DecodedEntity::VertexPoint(v) => {
            println!(
                "[{i}] VERTEX  flag=0x{:02X} location=({:.6}, {:.6}, {:.6})",
                v.flag, v.location.x, v.location.y, v.location.z
            );
        }
        DecodedEntity::VertexPfaceFace(f) => {
            println!("[{i}] VERTEX_PFACE_FACE  indices={:?}", f.vertex_indices);
        }
        DecodedEntity::Polyline3d(p) => {
            println!(
                "[{i}] POLYLINE_3D  flags={:?} owned={:?}",
                p.flags, p.num_owned_objects
            );
        }
        DecodedEntity::PolyfaceMesh(m) => {
            println!(
                "[{i}] POLYLINE_PFACE  vertices={} faces={} owned={:?}",
                m.vertex_count, m.face_count, m.num_owned_objects
            );
        }
        DecodedEntity::MLine(m) => {
            println!(
                "[{i}] MLINE  scale={:.6} justification={:?} lines={} vertices={} flags={}",
                m.scale_factor,
                m.justification,
                m.num_lines,
                m.vertices.len(),
                m.open_closed_flags
            );
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

/// Print the §20.4.84 plot-settings block a LAYOUT embeds (or a
/// standalone PLOTSETTINGS record carries).
fn print_plot_settings(s: &dwg::objects::acad_plot_settings::AcadPlotSettings, indent: &str) {
    println!(
        "{indent}plot: setup={:?} device={:?} paper={:?} {}x{} mm \
         margins(l={} b={} r={} t={}) flags={}",
        s.page_setup_name,
        truncate(&s.printer_config_name, 40),
        s.paper_size,
        s.paper_width,
        s.paper_height,
        s.margin_left,
        s.margin_bottom,
        s.margin_right,
        s.margin_top,
        s.plot_layout_flags,
    );
    println!(
        "{indent}      origin=({}, {}) units={} rotation={} type={} \
         window=({}, {})..({}, {}) scale={}/{} factor={} std_scale={} \
         shade(mode={} res={} dpi={}) stylesheet={:?}",
        s.plot_origin.x,
        s.plot_origin.y,
        s.paper_units,
        s.plot_rotation,
        s.plot_type,
        s.window_min.x,
        s.window_min.y,
        s.window_max.x,
        s.window_max.y,
        s.real_world_units,
        s.drawing_units,
        s.scale_factor,
        s.standard_scale_type,
        s.shade_plot_mode,
        s.shade_plot_resolution_level,
        s.shade_plot_custom_dpi,
        s.current_style_sheet,
    );
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
