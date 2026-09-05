use std::collections::BTreeMap;

use anyhow::{Context, Result};

use crate::cluster::{ClusteringResult, NUM_CLUSTERS, Swatch};
use crate::color::Color;

const ZONE_MERGE_THRESHOLD: f64 = 50.0;
const BG_LIGHTNESS: [f64; 6] = [0.24, 0.22, 0.32, 0.40, 0.48, 0.88];
const FG_LIGHTNESS: f64 = 0.90;
const BRIGHT_BG_LIGHTNESS: f64 = 0.60;
const MIN_LIGHTNESS_FLOOR: f64 = 0.6;
const LABELS: [&str; 16] = ["base00", "base01", "base02", "base03", "base04", "base05", "base06", "base07", "base08", "base09", "base0A", "base0B", "base0C", "base0D", "base0E", "base0F"];

pub struct Palette(pub [Color; 16]);

impl Palette {
    pub fn from_clusters(result: &ClusteringResult, contrast: f64) -> Self {
        let contrast = contrast.clamp(0.0, 1.0);
        let accents = build_accent_colors(result, contrast);
        let zones = extract_dominant_zones(result);
        let (lighter, darker) = if zones.color1.l > zones.color2.l { (zones.color1, zones.color2) } else { (zones.color2, zones.color1) };

        let mut colors = [Color::default(); 16];
        for (slot, &l) in colors[..6].iter_mut().zip(&BG_LIGHTNESS) {
            *slot = Color::new(l, zones.dominant.chroma / 2.0, zones.dominant.hue);
        }

        colors[6] = Color::new(FG_LIGHTNESS, lighter.chroma / 2.0, lighter.hue);
        colors[7] = Color::new(BRIGHT_BG_LIGHTNESS, darker.chroma / 2.0, darker.hue);
        colors[8..].copy_from_slice(&accents);

        Self(colors)
    }

    pub fn to_json(&self) -> Result<String> {
        let map: BTreeMap<&str, String> = LABELS.iter().copied().zip(self.0.iter().copied().map(Color::to_hex)).collect();
        serde_json::to_string_pretty(&map).context("failed to serialize palette")
    }
}

fn build_accent_colors(result: &ClusteringResult, contrast: f64) -> [Color; NUM_CLUSTERS] {
    let mut colors = [Color::default(); NUM_CLUSTERS];
    for (dst, sw) in colors.iter_mut().zip(&result.swatches) {
        *dst = sw.color;
    }

    let scale = 1.0 + contrast * 3.0;
    if scale != 1.0 {
        let mean = colors.iter().map(|c| c.chroma).sum::<f64>() / NUM_CLUSTERS as f64;
        if mean > 0.0 {
            let factor = (result.avg_chroma * scale) / mean;
            for c in &mut colors {
                c.chroma *= factor;
            }
        }
    }

    let target = 0.7 + contrast * 0.3;
    let min_l = colors.iter().map(|c| c.l).fold(f64::INFINITY, f64::min);
    let max_l = colors.iter().map(|c| c.l).fold(f64::NEG_INFINITY, f64::max);

    if min_l < MIN_LIGHTNESS_FLOOR {
        if target <= min_l {
            remap_lightness(&mut colors, min_l, max_l);
        } else {
            let mut alpha = (MIN_LIGHTNESS_FLOOR - min_l) / (target - min_l);
            let proj = max_l + alpha * (target - max_l);
            if proj > 1.0 {
                if target >= max_l {
                    remap_lightness(&mut colors, min_l, max_l);
                    return colors;
                }
                alpha = alpha.min((1.0 - max_l) / (target - max_l));
            }
            for c in &mut colors {
                c.l += alpha * (target - c.l);
            }
        }
    }

    colors
}

fn remap_lightness(colors: &mut [Color; NUM_CLUSTERS], lo: f64, hi: f64) {
    let range = hi - lo;
    for c in colors.iter_mut() {
        c.l = if range > 0.0 { (c.l - lo) / range * (1.0 - MIN_LIGHTNESS_FLOOR) + MIN_LIGHTNESS_FLOOR } else { MIN_LIGHTNESS_FLOOR };
    }
}

struct DominantZones {
    color1: Color,
    color2: Color,
    dominant: Color,
}

fn extract_dominant_zones(result: &ClusteringResult) -> DominantZones {
    let neutral = Color::new(0.5, 0.0, 0.0);
    let mut active: Vec<(usize, &Swatch)> = result.swatches.iter().enumerate().filter(|(_, s)| s.active).collect();
    active.sort_by(|a, b| a.1.color.hue.total_cmp(&b.1.color.hue));

    match active.as_slice() {
        [] => DominantZones { color1: neutral, color2: neutral, dominant: neutral },
        [(idx, sw)] => {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            let mut found = false;
            for (&lab, pix) in result.labels.iter().zip(&result.pixels) {
                if lab as usize == *idx {
                    lo = lo.min(pix.l);
                    hi = hi.max(pix.l);
                    found = true;
                }
            }
            let (l1, l2) = if found { (lo, hi) } else { (sw.color.l, sw.color.l) };
            let c1 = Color::new(l1, sw.color.chroma, sw.color.hue);
            let c2 = Color::new(l2, sw.color.chroma, sw.color.hue);
            DominantZones { color1: c1, color2: c2, dominant: c1 }
        }
        _ => extract_multi_zones(&active),
    }
}

fn extract_multi_zones(active: &[(usize, &Swatch)]) -> DominantZones {
    let mut zones: Vec<Vec<(usize, &Swatch)>> = vec![vec![active[0]]];
    for &(idx, sw) in &active[1..] {
        let prev_hue = zones.last().unwrap().last().unwrap().1.color.hue;
        if sw.color.hue - prev_hue <= ZONE_MERGE_THRESHOLD {
            zones.last_mut().unwrap().push((idx, sw));
        } else {
            zones.push(vec![(idx, sw)]);
        }
    }

    if zones.len() > 1 {
        let first_hue = zones[0].first().unwrap().1.color.hue;
        let last_hue = zones.last().unwrap().last().unwrap().1.color.hue;
        if (360.0 - last_hue) + first_hue <= ZONE_MERGE_THRESHOLD
            && let Some(mut wrapped) = zones.pop()
        {
            wrapped.append(&mut zones[0]);
            zones[0] = wrapped;
        }
    }

    if zones.len() == 1 {
        let z = &zones[0];
        let c1 = z.first().unwrap().1.color;
        let c2 = z.last().unwrap().1.color;
        return DominantZones { color1: c1, color2: c2, dominant: c1 };
    }

    zones.sort_by_key(|z| std::cmp::Reverse(z.iter().map(|(_, s)| s.pixel_count).sum::<usize>()));

    let p1: usize = zones[0].iter().map(|(_, s)| s.pixel_count).sum();
    let p2: usize = zones[1].iter().map(|(_, s)| s.pixel_count).sum();

    let c1 = zone_weighted_average(&zones[0]);
    let c2 = zone_weighted_average(&zones[1]);
    let dominant = if p1 >= p2 { c1 } else { c2 };

    DominantZones { color1: c1, color2: c2, dominant }
}

fn zone_weighted_average(zone: &[(usize, &Swatch)]) -> Color {
    let total: f64 = zone.iter().map(|(_, s)| s.pixel_count as f64).sum();
    if total == 0.0 {
        return Color::new(0.5, 0.0, 0.0);
    }

    let mut l_sum = 0.0;
    let mut ch_sum = 0.0;
    let mut cos_sum = 0.0;
    let mut sin_sum = 0.0;

    for &(_, sw) in zone {
        let w = sw.pixel_count as f64 / total;
        l_sum += sw.color.l * w;
        ch_sum += sw.color.chroma * w;

        let rad = sw.color.hue.to_radians();
        cos_sum += rad.cos() * w;
        sin_sum += rad.sin() * w;
    }

    let hue = sin_sum.atan2(cos_sum).to_degrees().rem_euclid(360.0);

    Color::new(l_sum, ch_sum, hue)
}
