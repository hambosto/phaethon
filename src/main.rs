mod cluster;
mod color;
mod palette;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use image::imageops::FilterType;

use crate::cluster::ClusteringResult;
use crate::color::Color;
use crate::palette::Palette;

#[derive(Parser, Debug)]
#[command(name = "phaethon", version, about = "Extract base16 color palettes from images using Oklch perceptual clustering")]
struct Cli {
    /// Path to source image
    #[arg(short, long)]
    image: PathBuf,

    /// Contrast 0.0..1.0 (higher = more saturated/brighter accents)
    #[arg(short, long, default_value_t = 0.5)]
    contrast: f64,

    /// Resize image to NxN before clustering (0 = full resolution)
    #[arg(long, default_value_t = 256)]
    resize: u32,

    /// Write JSON to file instead of stdout
    #[arg(short, long)]
    output: Option<PathBuf>,
}

fn load_pixels(path: &Path, resize: u32) -> Result<Vec<Color>> {
    let img = image::open(path).context("failed to open image")?;
    let rgb = if resize == 0 { img.into_rgb8() } else { img.resize_exact(resize, resize, FilterType::Nearest).into_rgb8() };
    let pixels = rgb.pixels().map(|p| Color::from_srgb(p[0], p[1], p[2])).collect();

    Ok(pixels)
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let pixels = load_pixels(&args.image, args.resize)?;
    let result = ClusteringResult::from_pixels(&pixels);
    let palette = Palette::from_clusters(&result, args.contrast);
    let json = palette.to_json_string().context("failed to serialize palette")?;

    match args.output {
        Some(out) => std::fs::write(&out, json).context("failed to write output")?,
        None => println!("{json}"),
    }

    Ok(())
}
