mod cluster;
mod color;
mod palette;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use cluster::ClusteringResult;
use color::image_to_oklch_pixels;
use image::imageops::FilterType;
use palette::Palette;

#[derive(Parser)]
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

fn load_pixels(path: &Path, resize: u32) -> Result<Vec<[f64; 3]>> {
    let image = image::open(path).context("failed to open image")?;
    let resized = image.resize_exact(resize, resize, FilterType::Nearest);
    let rgb = resized.into_rgb8();
    let pixels = image_to_oklch_pixels(&rgb);

    Ok(pixels)
}

pub fn generate_palette(path: &Path, contrast: f64, resize: u32) -> Result<String> {
    let pixels = load_pixels(path, resize)?;
    let clustering = ClusteringResult::from_pixels(pixels)?;
    let palette = Palette::from_clusters(&clustering, contrast)?;
    let json = serde_json::to_string_pretty(&palette.to_map())?;

    Ok(json)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if !cli.image.is_file() {
        anyhow::bail!("no such file: {}", cli.image.display());
    }

    if !(0.0..=1.0).contains(&cli.contrast) {
        anyhow::bail!("contrast must be between 0.0 and 1.0");
    }

    if cli.resize == 0 {
        anyhow::bail!("resize must be a positive integer");
    }

    let output = generate_palette(&cli.image, cli.contrast, cli.resize)?;
    match &cli.output {
        Some(path) => std::fs::write(&path, output).context("failed to write output")?,
        None => println!("{output}"),
    }

    Ok(())
}
