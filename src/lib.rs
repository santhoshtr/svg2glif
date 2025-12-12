use anyhow::{Context, Result};
use norad::{Anchor, Codepoints, Contour, ContourPoint, Glyph, Name, PointType};
use std::fs;
use std::path::Path;
use usvg::Node;

/// Configuration for SVG to GLIF conversion
pub struct ConversionConfig {
    /// Units per em (typically 1000 or 2048)
    pub em_size: f32,
    /// Descent value to place the glyph above baseline
    pub descent: f32,
    /// Optional Unicode codepoint in hex format
    pub unicode: Option<String>,
    /// Optional name for the glyph, if not given, filename will be used.
    pub name: Option<String>,
}

impl ConversionConfig {
    /// Create a new conversion configuration
    pub fn new(em_size: f32, descent: f32) -> Self {
        Self {
            em_size,
            descent,
            unicode: None,
            name: None,
        }
    }

    /// Set the unicode codepoint (in hex format, e.g., "0041")
    pub fn with_unicode(mut self, unicode: String) -> Self {
        self.unicode = Some(unicode);
        self
    }
}

/// Convert an SVG file to a GLIF glyph
pub fn convert_svg_to_glyph(svg_path: &Path, config: &ConversionConfig) -> Result<Glyph> {
    // Load SVG
    let svg_data = fs::read_to_string(svg_path).context("reading input svg")?;
    convert_svg_string_to_glyph(&svg_data, svg_path, config)
}

/// Convert SVG string data to a GLIF glyph
///
/// # Examples
///
/// ```
/// use svg2glif::{convert_svg_string_to_glyph, ConversionConfig};
/// use std::path::Path;
///
/// let svg_data = r#"<?xml version="1.0" encoding="UTF-8"?>
/// <svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
///   <path d="M 10 10 L 90 10 L 90 90 L 10 90 Z" fill="black"/>
/// </svg>"#;
///
/// let config = ConversionConfig::new(1000.0, 200.0)
///     .with_unicode("0041".to_string());
///
/// let glyph = convert_svg_string_to_glyph(
///     svg_data,
///     Path::new("example.svg"),
///     &config
/// ).unwrap();
///
/// ```
pub fn convert_svg_string_to_glyph(
    svg_data: &str,
    svg_path: &Path,
    config: &ConversionConfig,
) -> Result<Glyph> {
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let rtree = usvg::Tree::from_str(svg_data, &options).context("parsing svg")?;

    // Get SVG dimensions
    let svg_size = rtree.size();
    let svg_width: f32 = svg_size.width();
    let svg_height: f32 = svg_size.height();

    // Scale to font units
    let scale = config.em_size / svg_height;
    let advance_width = (svg_width * scale).round();
    let advance_height = (svg_height * scale).round();

    let glyph_name = config
        .name
        .as_deref()
        .or_else(|| svg_path.file_stem().and_then(|s| s.to_str()))
        .unwrap_or("svgglyph")
        .to_string();

    // Create glyph
    let mut glyph = Glyph::new(glyph_name.as_str());
    glyph.width = advance_width as f64;
    glyph.height = advance_height as f64;

    // Add unicode if provided
    if let Some(ref unicode_hex) = config.unicode
        && let Ok(codepoint) = u32::from_str_radix(unicode_hex, 16)
        && let Some(c) = char::from_u32(codepoint)
    {
        let codepoints = Codepoints::new([c]);
        glyph.codepoints = codepoints;
    }

    // Process paths
    for node in rtree.root().children() {
        process_node(node, &mut glyph, svg_height, config.descent, scale);
    }

    Ok(glyph)
}

/// Convert an SVG file to GLIF and write to output file
///
/// # Examples
///
/// ```no_run
/// use svg2glif::{convert_svg_to_glif_file, ConversionConfig};
/// use std::path::Path;
///
/// let config = ConversionConfig::new(1000.0, 200.0)
///     .with_unicode("0041".to_string());
///
/// convert_svg_to_glif_file(
///     Path::new("input.svg"),
///     Path::new("output.glif"),
///     &config
/// ).unwrap();
/// ```
pub fn convert_svg_to_glif_file(
    svg_path: &Path,
    glif_path: &Path,
    config: &ConversionConfig,
) -> Result<()> {
    let glyph = convert_svg_to_glyph(svg_path, config)?;
    let glif_data = glyph.encode_xml()?;
    fs::write(glif_path, glif_data)?;
    Ok(())
}

fn process_node(node: &usvg::Node, glyph: &mut Glyph, svg_height: f32, descent: f32, scale: f32) {
    match *node {
        Node::Path(ref path) => {
            let contours = process_path(path, svg_height, descent, scale);
            if !glyph.contours.is_empty() {
                glyph.contours.extend(contours);
            } else {
                glyph.contours = contours;
            }
        }
        Node::Text(ref text) => {
            if let Some(anchor) = process_text_as_anchor(text, node, svg_height, descent, scale) {
                glyph.anchors.push(anchor);
            }
        }
        Node::Group(ref group) => {
            for child in group.children() {
                process_node(child, glyph, svg_height, descent, scale);
            }
        }
        _ => {}
    }
}

fn process_path(path: &usvg::Path, svg_height: f32, descent: f32, scale: f32) -> Vec<Contour> {
    let mut contours = Vec::new();
    let mut current_contour: Vec<ContourPoint> = Vec::new();
    let path_data: &usvg::tiny_skia_path::Path = path.data();
    let segments: usvg::tiny_skia_path::PathSegmentsIter<'_> = path_data.segments();

    for seg in segments {
        match seg {
            usvg::tiny_skia_path::PathSegment::MoveTo(p) => {
                // Start new contour
                if !current_contour.is_empty() {
                    contours.push(Contour::new(current_contour, None));
                    current_contour = Vec::new();
                }
                let (x, y) = svg_to_ufo(p.x, p.y, svg_height, descent, scale);
                current_contour.push(ContourPoint::new(x, y, PointType::Curve, true, None, None));
            }
            usvg::tiny_skia_path::PathSegment::LineTo(p) => {
                let (x, y) = svg_to_ufo(p.x, p.y, svg_height, descent, scale);
                current_contour.push(ContourPoint::new(x, y, PointType::Line, false, None, None));
            }
            usvg::tiny_skia_path::PathSegment::CubicTo(p1, p2, p) => {
                // Add two off-curve control points
                let (cx1, cy1) = svg_to_ufo(p1.x, p1.y, svg_height, descent, scale);
                current_contour.push(ContourPoint::new(
                    cx1,
                    cy1,
                    PointType::OffCurve,
                    false,
                    None,
                    None,
                ));

                let (cx2, cy2) = svg_to_ufo(p2.x, p2.y, svg_height, descent, scale);
                current_contour.push(ContourPoint::new(
                    cx2,
                    cy2,
                    PointType::OffCurve,
                    false,
                    None,
                    None,
                ));

                // Add on-curve point
                let (px, py) = svg_to_ufo(p.x, p.y, svg_height, descent, scale);
                current_contour.push(ContourPoint::new(
                    px,
                    py,
                    PointType::Curve,
                    true, // smooth
                    None,
                    None,
                ));
            }
            usvg::tiny_skia_path::PathSegment::Close => {
                // Finish current contour
                if !current_contour.is_empty() {
                    contours.push(Contour::new(current_contour, None));
                    current_contour = Vec::new();
                }
            }
            usvg::tiny_skia_path::PathSegment::QuadTo(_, _) => {
                // Skip quadratic curves as requested
            }
        }
    }

    // Add any remaining contour
    if !current_contour.is_empty() {
        contours.push(Contour::new(current_contour, None));
    }

    // Remove duplicate last point if it matches the first point
    for contour in &mut contours {
        if contour.points.len() > 1 {
            let first = &contour.points[0];
            let last = &contour.points[contour.points.len() - 1];

            if first.x == last.x && first.y == last.y {
                contour.points.pop();
            }
        }
    }

    contours
}

fn process_text_as_anchor(
    text: &usvg::Text,
    node: &usvg::Node,
    svg_height: f32,
    descent: f32,
    scale: f32,
) -> Option<Anchor> {
    // Get the text content as the anchor name
    let mut anchor_name = String::new();
    for chunk in text.chunks() {
        anchor_name.push_str(chunk.text());
    }

    if anchor_name.is_empty() {
        return None;
    }

    // Get position from transform

    // Check parent for transform
    let transform = node.abs_transform();
    let x = transform.tx;
    let y = transform.ty;

    // Convert to UFO coordinates
    let (ufo_x, ufo_y) = svg_to_ufo(x, y, svg_height, descent, scale);
    let anchor_name = Name::new(anchor_name.as_str()).unwrap();
    Some(Anchor::new(ufo_x, ufo_y, Some(anchor_name), None, None))
}
fn svg_to_ufo(sx: f32, sy: f32, svg_height: f32, descent: f32, scale: f32) -> (f64, f64) {
    // Flip Y (SVG origin is top-left; UFO origin baseline is bottom-left)
    let x = sx * scale;
    let y = (svg_height - descent - sy) * scale;
    (x.round() as f64, y.round() as f64)
}
