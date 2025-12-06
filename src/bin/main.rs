use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use svg2glif::{ConversionConfig, convert_svg_to_glif_file};

/// Convert SVG glyphs to UFO GLIF format
#[derive(Parser, Debug)]
#[command(name = "svg2glif")]
#[command(about = "Convert SVG-based glyph drawings to UFO's GLIF format", long_about = None)]
struct Args {
    /// Input SVG file
    #[arg(short, long, value_name = "INPUT")]
    input: PathBuf,

    /// Output GLIF file
    #[arg(short, long, value_name = "OUTPUT")]
    output: PathBuf,

    /// Units per em (typically 1000 or 2048)
    #[arg(short, long, value_name = "EM_SIZE")]
    em_size: f32,

    /// SVG Does not have the concept of ascent-descent, give descent
    /// value to place the glyph above descent
    #[arg(short, long, value_name = "DESCENT")]
    descent: f32,

    /// Unicode codepoint in hex (e.g., 0041 for 'A')
    #[arg(short, long, value_name = "HEX")]
    unicode: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut config = ConversionConfig::new(args.em_size, args.descent);
    if let Some(unicode) = args.unicode {
        config = config.with_unicode(unicode);
    }

    convert_svg_to_glif_file(&args.input, &args.output, &config)?;
    println!("Wrote glif to {}", args.output.display());
    Ok(())
}
