use std::collections::BTreeMap;

use anyhow::{Context, Result};

use crate::cluster::{ClusteringResult, Swatch};
use crate::color::Color;

const NUM_CLUSTERS: usize = 8;
const ZONE_MERGE_THRESHOLD: f64 = 50.0;
const BG_LIGHTNESS: [f64; 6] = [0.24, 0.22, 0.32, 0.40, 0.48, 0.88];
const FG_LIGHTNESS: f64 = 0.90;
const BRIGHT_BG_LIGHTNESS: f64 = 0.60;
const MIN_LIGHTNESS_FLOOR: f64 = 0.6;
const LABELS: [&str; 16] = ["base00", "base01", "base02", "base03", "base04", "base05", "base06", "base07", "base08", "base09", "base0A", "base0B", "base0C", "base0D", "base0E", "base0F"];

struct DominantZones {
    color1: Color,
    color2: Color,
    dominant: Color,
}

fn remap_lightness(colors: &[Color], lo: f64, hi: f64) -> Vec<Color> {
    let span = hi - lo;
    if span <= 0.0 {
        return colors.iter().map(|c| Color::new(MIN_LIGHTNESS_FLOOR, c.chroma, c.hue)).collect();
    }
    colors
        .iter()
        .map(|c| {
            let l = (c.l - lo) / span * (1.0 - MIN_LIGHTNESS_FLOOR) + MIN_LIGHTNESS_FLOOR;
            Color::new(l, c.chroma, c.hue)
        })
        .collect()
}

fn build_accent_colors(result: &ClusteringResult, contrast: f64) -> Vec<Color> {
    let contrast = contrast.clamp(0.0, 1.0);
    let mut colors: Vec<Color> = result.swatches.iter().map(|s| s.color).collect();

    let scale = 1.0 + contrast * 3.0;
    if scale != 1.0 {
        let mean_chroma: f64 = colors.iter().map(|c| c.chroma).sum::<f64>() / NUM_CLUSTERS as f64;
        if mean_chroma > 0.0 {
            let factor = (result.avg_chroma * scale) / mean_chroma;
            colors = colors.iter().map(|c| Color::new(c.l, c.chroma * factor, c.hue)).collect();
        }
    }

    let target = 0.7 + contrast * 0.3;
    let lo = colors.iter().map(|c| c.l).fold(f64::INFINITY, f64::min);
    let hi = colors.iter().map(|c| c.l).fold(f64::NEG_INFINITY, f64::max);

    if lo >= MIN_LIGHTNESS_FLOOR {
        return colors;
    }
    if target <= lo {
        return remap_lightness(&colors, lo, hi);
    }

    let mut alpha = (MIN_LIGHTNESS_FLOOR - lo) / (target - lo);
    let projected = hi + alpha * (target - hi);
    if projected > 1.0 {
        if target >= hi {
            return remap_lightness(&colors, lo, hi);
        }
        alpha = alpha.min((1.0 - hi) / (target - hi));
    }

    colors.iter().map(|c| Color::new(c.l + alpha * (target - c.l), c.chroma, c.hue)).collect()
}

fn zone_weighted_average(zone: &[(usize, Swatch)]) -> Color {
    let total: usize = zone.iter().map(|(_, s)| s.pixel_count).sum();
    if total == 0 {
        return Color::new(0.5, 0.0, 0.0);
    }

    let total = total as f64;
    let mut l = 0.0;
    let mut chroma = 0.0;
    let mut sin_sum = 0.0;
    let mut cos_sum = 0.0;

    for (_, swatch) in zone {
        let weight = swatch.pixel_count as f64 / total;
        l += swatch.color.l * weight;
        chroma += swatch.color.chroma * weight;

        let rad = swatch.color.hue.to_radians();
        sin_sum += rad.sin() * weight;
        cos_sum += rad.cos() * weight;
    }
    let hue = sin_sum.atan2(cos_sum).to_degrees().rem_euclid(360.0);

    Color::new(l, chroma, hue)
}

fn extract_multi_zones(active: &[(usize, Swatch)]) -> Result<DominantZones> {
    let mut zones: Vec<Vec<(usize, Swatch)>> = Vec::new();
    let mut current_zone: Vec<(usize, Swatch)> = Vec::new();
    let mut last_hue: Option<f64> = None;

    for &item in active {
        let starts_new_zone = match last_hue {
            Some(hue) => item.1.color.hue - hue > ZONE_MERGE_THRESHOLD,
            None => false,
        };
        if starts_new_zone {
            let finished_zone = std::mem::take(&mut current_zone);
            zones.push(finished_zone);
        }
        current_zone.push(item);
        last_hue = Some(item.1.color.hue);
    }
    zones.push(current_zone);

    if zones.len() > 1 {
        let last_zone = zones.last().context("zone list is unexpectedly empty")?;
        let last_item = last_zone.last().context("zone is unexpectedly empty")?;
        let first_zone = zones.first().context("zone list is unexpectedly empty")?;
        let first_item = first_zone.first().context("zone is unexpectedly empty")?;
        let wraps_around = (360.0 - last_item.1.color.hue) + first_item.1.color.hue <= ZONE_MERGE_THRESHOLD;

        if wraps_around {
            let wrapped_tail = zones.pop().context("zone list is unexpectedly empty")?;
            let mut wrapped = wrapped_tail;
            wrapped.extend_from_slice(&zones[0]);
            zones[0] = wrapped;
        }
    }

    if zones.len() == 1 {
        let zone = &zones[0];
        let first_item = zone.first().context("zone is unexpectedly empty")?;
        let last_item = zone.last().context("zone is unexpectedly empty")?;
        return Ok(DominantZones { color1: first_item.1.color, color2: last_item.1.color, dominant: first_item.1.color });
    }

    zones.sort_by_key(|zone| std::cmp::Reverse(zone.iter().map(|(_, s)| s.pixel_count).sum::<usize>()));

    let zone1 = zones.first().context("expected at least two zones")?;
    let zone2 = zones.get(1).context("expected at least two zones")?;
    let color1 = zone_weighted_average(zone1);
    let color2 = zone_weighted_average(zone2);
    let weight1: usize = zone1.iter().map(|(_, s)| s.pixel_count).sum();
    let weight2: usize = zone2.iter().map(|(_, s)| s.pixel_count).sum();
    let dominant = if weight1 >= weight2 { color1 } else { color2 };

    Ok(DominantZones { color1, color2, dominant })
}

fn extract_dominant_zones(result: &ClusteringResult) -> Result<DominantZones> {
    let mut active: Vec<(usize, Swatch)> = result.swatches.iter().enumerate().filter(|(_, s)| s.active).map(|(i, &s)| (i, s)).collect();
    active.sort_by(|a, b| a.1.color.hue.total_cmp(&b.1.color.hue));

    if active.is_empty() {
        let neutral = Color::new(0.5, 0.0, 0.0);
        return Ok(DominantZones { color1: neutral, color2: neutral, dominant: neutral });
    }

    if active.len() == 1 {
        let (idx, swatch) = active[0];
        let lightness: Vec<f64> = result.pixels.iter().zip(&result.labels).filter(|(_, label)| **label as usize == idx).map(|(p, _)| p[0]).collect();

        let (lo, hi) = if lightness.is_empty() {
            (swatch.color.l, swatch.color.l)
        } else {
            let lo = lightness.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = lightness.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            (lo, hi)
        };

        return Ok(DominantZones {
            color1: Color::new(lo, swatch.color.chroma, swatch.color.hue),
            color2: Color::new(hi, swatch.color.chroma, swatch.color.hue),
            dominant: Color::new(lo, swatch.color.chroma, swatch.color.hue),
        });
    }

    extract_multi_zones(&active)
}

pub struct Palette {
    colors: [Color; 16],
}

impl Palette {
    pub fn from_clusters(result: &ClusteringResult, contrast: f64) -> Result<Self> {
        let accents = build_accent_colors(result, contrast);
        let zones = extract_dominant_zones(result)?;
        let (lighter, darker) = if zones.color1.l > zones.color2.l { (zones.color1, zones.color2) } else { (zones.color2, zones.color1) };

        let mut colors = [Color::new(0.0, 0.0, 0.0); 16];
        for (i, &l) in BG_LIGHTNESS.iter().enumerate() {
            colors[i] = Color::new(l, zones.dominant.chroma / 2.0, zones.dominant.hue);
        }

        colors[6] = Color::new(FG_LIGHTNESS, lighter.chroma / 2.0, lighter.hue);
        colors[7] = Color::new(BRIGHT_BG_LIGHTNESS, darker.chroma / 2.0, darker.hue);

        for (i, &accent) in accents.iter().enumerate() {
            colors[8 + i] = accent;
        }

        Ok(Self { colors })
    }

    pub fn to_map(&self) -> BTreeMap<&'static str, String> {
        LABELS.iter().zip(self.colors.iter()).map(|(&label, &color)| (label, color.to_hex())).collect()
    }
}
