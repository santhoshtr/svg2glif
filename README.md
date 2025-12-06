# svg2glif

Convert SVG-based glyph drawings to UFO's GLIF format.

## Overview

`svg2glif` is a Rust library and command-line tool that converts SVG vector graphics into UFO (Unified Font Object) GLIF format, making it easy to incorporate SVG artwork into font development workflows.

## Features

- Convert SVG paths to UFO GLIF format
- Support for cubic Bézier curves
- Configurable units-per-em scaling
- Unicode codepoint assignment
- Proper coordinate system conversion (SVG top-left to UFO baseline)
- **Anchor extraction from SVG text elements** - Text nodes in your SVG are automatically converted to anchor points in the GLIF file, useful for defining attachment points for diacritics and other mark positioning
- Both library and CLI interfaces

## Installation

### As a CLI tool

```bash
cargo install svg2glif
```

### As a library

Add to your `Cargo.toml`:

```toml
[dependencies]
svg2glif = "0.1"
```

## Usage

### Command Line

```bash
svg2glif -i input.svg -o output.glif -e 1000 -d 200 -u 0041
```

**Arguments:**
- `-i, --input <INPUT>`: Input SVG file
- `-o, --output <OUTPUT>`: Output GLIF file
- `-e, --em-size <EM_SIZE>`: Units per em (typically 1000 or 2048)
- `-d, --descent <DESCENT>`: Descent value to position glyph above baseline
- `-u, --unicode <HEX>`: Optional Unicode codepoint in hex (e.g., 0041 for 'A')

### Library

```rust
use svg2glif::{convert_svg_to_glif_file, ConversionConfig};
use std::path::Path;

let config = ConversionConfig::new(1000.0, 200.0)
    .with_unicode("0041".to_string());

convert_svg_to_glif_file(
    Path::new("input.svg"),
    Path::new("output.glif"),
    &config
)?;
```

Or convert from SVG string:

```rust
use svg2glif::{convert_svg_string_to_glyph, ConversionConfig};
use std::path::Path;

let svg_data = r#"<?xml version="1.0" encoding="UTF-8"?>
<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
  <path d="M 10 10 L 90 10 L 90 90 L 10 90 Z" fill="black"/>
</svg>"#;

let config = ConversionConfig::new(1000.0, 200.0);
let glyph = convert_svg_string_to_glyph(
    svg_data,
    Path::new("example.svg"),
    &config
)?;
```

## Anchor Points

SVG text elements are automatically converted to GLIF anchors. The text content becomes the anchor name, and the position is determined by the text element's transform. This is particularly useful for defining attachment points for mark positioning in fonts.

For example, an SVG text element like:
```xml
<g transform="translate(250, 700)">
  <text>top</text>
</g>
```

Will be converted to a GLIF anchor:
```xml
<anchor x="250" y="800" name="top"/>
```

## Limitations

- Only processes `<path>` elements (shapes like circles, rectangles must be converted to paths)
- Quadratic Bézier curves are not supported (only cubic)
- Does not handle SVG transforms, strokes, or fills

## License

MIT
