use anyhow::{Context, Result, anyhow};
use norad::{Anchor, Codepoints, Contour, ContourPoint, Glyph, Name, PointType};
use std::fs;
use std::path::Path;
use std::str::FromStr;
use svgtypes::{Length, LengthUnit, SimplePathSegment, SimplifyingPathParser, Transform};

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

    /// Set the name
    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
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
    let doc = roxmltree::Document::parse(svg_data).context("parsing svg")?;
    let root = doc.root_element();

    // Get SVG dimensions
    let svg_width = parse_length(root.attribute("width").unwrap_or("100"))?;
    let svg_height = parse_length(root.attribute("height").unwrap_or("100"))?;

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

    // Process nodes
    process_svg_node(
        &root,
        &mut glyph,
        svg_height,
        config.descent,
        scale,
        &Transform::default(),
    )?;

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

fn multiply(ts1: &Transform, ts2: &Transform) -> Transform {
    Transform {
        a: ts1.a * ts2.a + ts1.c * ts2.b,
        b: ts1.b * ts2.a + ts1.d * ts2.b,
        c: ts1.a * ts2.c + ts1.c * ts2.d,
        d: ts1.b * ts2.c + ts1.d * ts2.d,
        e: ts1.a * ts2.e + ts1.c * ts2.f + ts1.e,
        f: ts1.b * ts2.e + ts1.d * ts2.f + ts1.f,
    }
}

fn apply_transform(transform: &Transform, x: f32, y: f32) -> (f32, f32) {
    let new_x = transform.a as f32 * x + transform.c as f32 * y + transform.e as f32;
    let new_y = transform.b as f32 * x + transform.d as f32 * y + transform.f as f32;
    (new_x, new_y)
}

fn process_svg_node(
    node: &roxmltree::Node,
    glyph: &mut Glyph,
    svg_height: f32,
    descent: f32,
    scale: f32,
    parent_transform: &Transform,
) -> Result<()> {
    // Compute current transform
    let current_transform = if let Some(transform_str) = node.attribute("transform") {
        let transform = Transform::from_str(transform_str).context("parsing transform")?;
        multiply(parent_transform, &transform)
    } else {
        *parent_transform
    };

    match node.tag_name().name() {
        "path" => {
            if let Some(d) = node.attribute("d") {
                let contours =
                    process_path_data(d, svg_height, descent, scale, &current_transform)?;
                if !glyph.contours.is_empty() {
                    glyph.contours.extend(contours);
                } else {
                    glyph.contours = contours;
                }
            }
        }
        "text" => {
            if let Some(anchor) =
                process_text_as_anchor(node, svg_height, descent, scale, &current_transform)
            {
                glyph.anchors.push(anchor);
            }
        }
        "g" | "svg" => {
            // Process children
            for child in node.children() {
                if child.is_element() {
                    process_svg_node(
                        &child,
                        glyph,
                        svg_height,
                        descent,
                        scale,
                        &current_transform,
                    )?;
                }
            }
        }
        _ => {
            // Process children for unknown elements too
            for child in node.children() {
                if child.is_element() {
                    process_svg_node(
                        &child,
                        glyph,
                        svg_height,
                        descent,
                        scale,
                        &current_transform,
                    )?;
                }
            }
        }
    }

    Ok(())
}

fn parse_length(length_str: &str) -> Result<f32> {
    let length = Length::from_str(length_str).context("parsing length")?;
    match length.unit {
        LengthUnit::None | LengthUnit::Px => Ok(length.number as f32),
        _ => Err(anyhow!("unsupported length unit: {:?}", length.unit)),
    }
}

fn process_path_data(
    path_data: &str,
    svg_height: f32,
    descent: f32,
    scale: f32,
    transform: &Transform,
) -> Result<Vec<Contour>> {
    let mut contours = Vec::new();
    let mut current_contour: Vec<ContourPoint> = Vec::new();

    // Use SimplifyingPathParser - all coordinates are absolute!
    // This automatically handles:
    // - Relative to absolute conversion
    // - H/V line to LineTo conversion
    // - SmoothCurveTo control point reflection
    // - Arc to Bezier curve conversion
    for segment in SimplifyingPathParser::from(path_data) {
        let segment = segment.context("parsing path segment")?;

        match segment {
            SimplePathSegment::MoveTo { x, y } => {
                // Start new contour
                if !current_contour.is_empty() {
                    contours.push(Contour::new(current_contour, None));
                    current_contour = Vec::new();
                }
                let (tx, ty) = apply_transform(transform, x as f32, y as f32);
                let (ux, uy) = svg_to_ufo(tx, ty, svg_height, descent, scale);
                current_contour.push(ContourPoint::new(
                    ux,
                    uy,
                    PointType::Curve,
                    true,
                    None,
                    None,
                ));
            }
            SimplePathSegment::LineTo { x, y } => {
                let (tx, ty) = apply_transform(transform, x as f32, y as f32);
                let (ux, uy) = svg_to_ufo(tx, ty, svg_height, descent, scale);
                current_contour.push(ContourPoint::new(
                    ux,
                    uy,
                    PointType::Line,
                    false,
                    None,
                    None,
                ));
            }
            SimplePathSegment::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                // Add two off-curve control points
                let (tx1, ty1) = apply_transform(transform, x1 as f32, y1 as f32);
                let (ux1, uy1) = svg_to_ufo(tx1, ty1, svg_height, descent, scale);
                current_contour.push(ContourPoint::new(
                    ux1,
                    uy1,
                    PointType::OffCurve,
                    false,
                    None,
                    None,
                ));

                let (tx2, ty2) = apply_transform(transform, x2 as f32, y2 as f32);
                let (ux2, uy2) = svg_to_ufo(tx2, ty2, svg_height, descent, scale);
                current_contour.push(ContourPoint::new(
                    ux2,
                    uy2,
                    PointType::OffCurve,
                    false,
                    None,
                    None,
                ));

                // Add on-curve point
                let (tx, ty) = apply_transform(transform, x as f32, y as f32);
                let (ux, uy) = svg_to_ufo(tx, ty, svg_height, descent, scale);
                current_contour.push(ContourPoint::new(
                    ux,
                    uy,
                    PointType::Curve,
                    true,
                    None,
                    None,
                ));
            }
            SimplePathSegment::Quadratic { .. } => {
                // Skip quadratic curves as they're not supported in UFO/GLIF
                // If needed, they could be converted to cubic Bezier curves
            }
            SimplePathSegment::ClosePath => {
                // Finish current contour
                if !current_contour.is_empty() {
                    contours.push(Contour::new(current_contour, None));
                    current_contour = Vec::new();
                }
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

    Ok(contours)
}

fn process_text_as_anchor(
    node: &roxmltree::Node,
    svg_height: f32,
    descent: f32,
    scale: f32,
    transform: &Transform,
) -> Option<Anchor> {
    // Get the text content as the anchor name
    let anchor_name = node.text()?;

    if anchor_name.trim().is_empty() {
        return None;
    }

    // Get position from x, y attributes (default to 0, 0)
    let x = node
        .attribute("x")
        .and_then(|s| parse_length(s).ok())
        .unwrap_or(0.0);
    let y = node
        .attribute("y")
        .and_then(|s| parse_length(s).ok())
        .unwrap_or(0.0);

    // Apply transform
    let (tx, ty) = apply_transform(transform, x, y);

    // Convert to UFO coordinates
    let (ufo_x, ufo_y) = svg_to_ufo(tx, ty, svg_height, descent, scale);
    let anchor_name = Name::new(anchor_name.trim()).ok()?;
    Some(Anchor::new(ufo_x, ufo_y, Some(anchor_name), None, None))
}

fn svg_to_ufo(sx: f32, sy: f32, svg_height: f32, descent: f32, scale: f32) -> (f64, f64) {
    // Flip Y (SVG origin is top-left; UFO origin baseline is bottom-left)
    let x = sx * scale;
    let y = (svg_height - descent - sy) * scale;
    (x.round() as f64, y.round() as f64)
}
