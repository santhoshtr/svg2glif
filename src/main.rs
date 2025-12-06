use anyhow::{Context, Result};
use quick_xml::Writer;
use quick_xml::events::{BytesStart, Event};
use std::fs;
use std::io::Cursor;
use std::path::Path;

// usvg for parsing SVG files and getting path segments
use usvg::{NodeKind, PathSegment};

fn main() -> Result<()> {
    // Simple CLI args: input svg, output glif, units_per_em or scale
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("Usage: {} input.svg output.glif units_per_em", args[0]);
        std::process::exit(2);
    }
    let input = &args[1];
    let output = &args[2];
    let units_per_em: f64 = args[3].parse().context("units_per_em must be a number")?;

    // Load SVG
    let svg_data = fs::read_to_string(input).context("reading input svg")?;
    // Parse with usvg
    let opt = usvg::Options::default();
    let rtree = usvg::Tree::from_str(&svg_data, &opt.to_ref()).context("parsing svg")?;

    // Determine SVG height for y-flip (use viewBox or svg size)
    let svg_height = rtree.svg_node().borrow().size.height();

    // Decide scale: map svg units (px) -> font units. A common mapping:
    // scale = units_per_em / svg_height
    let scale = units_per_em / svg_height;

    // We'll accumulate contours discovered from path nodes
    let mut contours = Vec::new();

    // Walk nodes looking for path nodes
    for node in rtree.root().descendants() {
        if let NodeKind::Path(ref path) = *node.borrow() {
            // Each path has a vector of segments in path.data
            let mut contour_points: Vec<GlifEvent> = Vec::new();
            for seg in path.data.segments() {
                match seg {
                    PathSegment::MoveTo(p) => {
                        let (x, y) = svg_to_ufo(p.x(), p.y(), svg_height, scale);
                        contour_points.push(GlifEvent::MoveTo(x, y));
                    }
                    PathSegment::LineTo(p) => {
                        let (x, y) = svg_to_ufo(p.x(), p.y(), svg_height, scale);
                        contour_points.push(GlifEvent::LineTo(x, y));
                    }
                    PathSegment::CurveTo(c1, c2, p) => {
                        let (x1, y1) = svg_to_ufo(c1.x(), c1.y(), svg_height, scale);
                        let (x2, y2) = svg_to_ufo(c2.x(), c2.y(), svg_height, scale);
                        let (x, y) = svg_to_ufo(p.x(), p.y(), svg_height, scale);
                        contour_points.push(GlifEvent::CurveTo((x1, y1), (x2, y2), (x, y)));
                    }
                    PathSegment::ClosePath => {
                        contour_points.push(GlifEvent::ClosePath);
                    }
                    _ => {
                        // Handle other segment types if present
                    }
                }
            }
            // Convert segments into glif contour(s).
            // Note: usvg's path.data typically encodes one sequence which may contain multiple MoveTo start points.
            let contours_from_path = segments_to_contours(contour_points);
            contours.extend(contours_from_path);
        }
    }

    // Write glif file. Minimal header: glyph name "svgglyph"
    let glyph_name = Path::new(input)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("svgglyph");

    let glif = build_glif(glyph_name, units_per_em as i32, &contours)?;
    fs::write(output, glif)?;
    println!("Wrote glif to {}", output);
    Ok(())
}

// Small enum to hold extracted path segment events
enum GlifEvent {
    MoveTo(i32, i32),
    LineTo(i32, i32),
    CurveTo((i32, i32), (i32, i32), (i32, i32)), // c1, c2, to
    ClosePath,
}

// Convert segments (with Move/Line/Curve/Close items) into one or more closed contours.
// Assumes MoveTo starts a new contour.
fn segments_to_contours(events: Vec<GlifEvent>) -> Vec<Vec<GlifEvent>> {
    let mut contours: Vec<Vec<GlifEvent>> = Vec::new();
    let mut current: Vec<GlifEvent> = Vec::new();
    for ev in events {
        match ev {
            GlifEvent::MoveTo(_, _) => {
                if !current.is_empty() {
                    contours.push(current);
                    current = Vec::new();
                }
                current.push(ev);
            }
            GlifEvent::ClosePath => {
                current.push(ev);
                // finalize contour
                contours.push(current);
                current = Vec::new();
            }
            _ => {
                current.push(ev);
            }
        }
    }
    if !current.is_empty() {
        contours.push(current);
    }
    contours
}

// Convert SVG coordinates to UFO coordinates (integer font units)
fn svg_to_ufo(sx: f64, sy: f64, svg_height: f64, scale: f64) -> (i32, i32) {
    // Flip Y (SVG origin is top-left; UFO origin baseline is bottom-left)
    let x = (sx * scale).round() as i32;
    let y = ((svg_height - sy) * scale).round() as i32;
    (x, y)
}

// Build glif string with quick-xml
fn build_glif(name: &str, units_per_em: i32, contours: &[Vec<GlifEvent>]) -> Result<String> {
    let mut writer = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);

    // XML header + <glyph> start
    let mut glyph_start = BytesStart::new("glyph");
    glyph_start.push_attribute(("name", name));
    glyph_start.push_attribute(("format", "2"));
    writer.write_event(Event::Decl(
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>".into(),
    ))?;
    writer.write_event(Event::Start(glyph_start.to_borrowed()))?;

    // Note: you can add <advance> etc here if desired
    // <outline>
    writer.write_event(Event::Start(BytesStart::new("outline")))?;

    for contour in contours {
        writer.write_event(Event::Start(BytesStart::new("contour")))?;
        // We must emit points. We will convert sequence of events into point elements:
        // - MoveTo -> <point type="move" x=".." y=".."/>
        // - LineTo -> <point type="line" x=".." y=".."/>
        // - CurveTo with c1,c2,to -> emit two offcurve points then a curve (oncurve)
        // - ClosePath -> nothing special; glif contour is implicitly closed when points end
        for ev in contour {
            match ev {
                GlifEvent::MoveTo(x, y) => {
                    let mut p = BytesStart::new("point");
                    p.push_attribute(("x", &x.to_string()[..]));
                    p.push_attribute(("y", &y.to_string()[..]));
                    p.push_attribute(("type", "move"));
                    writer.write_event(Event::Empty(p))?;
                }
                GlifEvent::LineTo(x, y) => {
                    let mut p = BytesStart::new("point");
                    p.push_attribute(("x", &x.to_string()[..]));
                    p.push_attribute(("y", &y.to_string()[..]));
                    p.push_attribute(("type", "line"));
                    writer.write_event(Event::Empty(p))?;
                }
                GlifEvent::CurveTo((x1, y1), (x2, y2), (x, y)) => {
                    let mut p1 = BytesStart::new("point");
                    p1.push_attribute(("x", &x1.to_string()[..]));
                    p1.push_attribute(("y", &y1.to_string()[..]));
                    p1.push_attribute(("type", "offcurve"));
                    writer.write_event(Event::Empty(p1))?;

                    let mut p2 = BytesStart::new("point");
                    p2.push_attribute(("x", &x2.to_string()[..]));
                    p2.push_attribute(("y", &y2.to_string()[..]));
                    p2.push_attribute(("type", "offcurve"));
                    writer.write_event(Event::Empty(p2))?;

                    let mut p = BytesStart::new("point");
                    p.push_attribute(("x", &x.to_string()[..]));
                    p.push_attribute(("y", &y.to_string()[..]));
                    p.push_attribute(("type", "curve"));
                    writer.write_event(Event::Empty(p))?;
                }
                GlifEvent::ClosePath => {
                    // nothing special here - glif contours are closed by default
                }
            }
        }

        writer.write_event(Event::End(BytesStart::new("contour").to_end()))?;
    }

    writer.write_event(Event::End(BytesStart::new("outline").to_end()))?;

    // Optionally add <lib> etc.

    writer.write_event(Event::End(BytesStart::new("glyph").to_end()))?;

    let result = writer.into_inner().into_inner();
    let s = String::from_utf8(result)?;
    Ok(s)
}
