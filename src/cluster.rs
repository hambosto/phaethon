use anyhow::{Result, bail};

use crate::color::{CHROMA_THRESHOLD, Color};

const NUM_CLUSTERS: usize = 8;
const BASE_HUES: [f64; NUM_CLUSTERS] = [0.0, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0];
const HUE_OFFSETS: [f64; 3] = [0.0, 15.0, 30.0];
const HUE_SECTOR_HALF_WIDTH: f64 = 22.5;
const MAX_ITERS: usize = 100;
const TOLERANCE_SQ: f64 = 1e-4 * 1e-4;
const UNASSIGNED: u8 = 255;

#[derive(Clone, Copy)]
pub struct Swatch {
    pub color: Color,
    pub pixel_count: usize,
    pub active: bool,
}

struct NormStats {
    mean: [f64; 3],
    std: [f64; 3],
}

impl NormStats {
    fn denormalize(&self, v: [f64; 3]) -> [f64; 3] {
        [v[0] * self.std[0] + self.mean[0], v[1] * self.std[1] + self.mean[1], v[2] * self.std[2] + self.mean[2]]
    }

    fn normalize_hue(&self, hue: f64) -> f64 {
        (hue - self.mean[2]) / self.std[2]
    }
}

pub struct ClusteringResult {
    pub swatches: Vec<Swatch>,
    pub labels: Vec<u8>,
    pub avg_chroma: f64,
    pub pixels: Vec<[f64; 3]>,
}

impl ClusteringResult {
    pub fn from_pixels(pixels: Vec<[f64; 3]>) -> Result<Self> {
        let chromatic: Vec<[f64; 3]> = pixels.iter().copied().filter(|p| p[1] >= CHROMA_THRESHOLD).collect();

        let (pixels, avg_chroma) = if !chromatic.is_empty() {
            let avg_chroma = chromatic.iter().map(|p| p[1]).sum::<f64>() / chromatic.len() as f64;
            (chromatic, avg_chroma)
        } else if !pixels.is_empty() {
            (pixels, 0.0)
        } else {
            (vec![[0.5, 0.0, 0.0]], 0.0)
        };

        let mut normalized = pixels.clone();
        let stats = z_normalize(&mut normalized);
        let clustered = select_best_offset(&normalized, &stats)?;
        let (swatches, labels) = clustered;

        Ok(Self { swatches, labels, avg_chroma, pixels })
    }
}

fn z_normalize(points: &mut [[f64; 3]]) -> NormStats {
    let n = points.len() as f64;
    let mut mean = [0.0; 3];
    for p in points.iter() {
        for j in 0..3 {
            mean[j] += p[j];
        }
    }
    for m in &mut mean {
        *m /= n;
    }

    let mut variance = [0.0; 3];
    for p in points.iter() {
        for j in 0..3 {
            let d = p[j] - mean[j];
            variance[j] += d * d;
        }
    }
    let mut std = [0.0; 3];
    for j in 0..3 {
        std[j] = (variance[j] / n).sqrt().max(1e-8);
    }

    for p in points.iter_mut() {
        for j in 0..3 {
            p[j] = (p[j] - mean[j]) / std[j];
        }
    }

    NormStats { mean, std }
}

fn score(swatches: &[Swatch], total: usize) -> (usize, f64) {
    let active_count = swatches.iter().filter(|s| s.active).count();
    let covered: usize = swatches.iter().filter(|s| s.active).map(|s| s.pixel_count).sum();
    let coverage = if total > 0 { covered as f64 / total as f64 } else { 0.0 };
    (active_count, coverage)
}

fn select_best_offset(normalized: &[[f64; 3]], stats: &NormStats) -> Result<(Vec<Swatch>, Vec<u8>)> {
    let first = cluster_with_offset(normalized, stats, HUE_OFFSETS[0])?;
    let (mut best_swatches, mut best_labels) = first;
    let mut best_score = score(&best_swatches, normalized.len());

    for &offset in &HUE_OFFSETS[1..] {
        let candidate = cluster_with_offset(normalized, stats, offset)?;
        let (swatches, labels) = candidate;
        let candidate_score = score(&swatches, normalized.len());
        if candidate_score > best_score {
            best_swatches = swatches;
            best_labels = labels;
            best_score = candidate_score;
        }
    }

    Ok((best_swatches, best_labels))
}

fn cluster_with_offset(normalized: &[[f64; 3]], stats: &NormStats, offset: f64) -> Result<(Vec<Swatch>, Vec<u8>)> {
    let mut centers = [[0.0; 3]; NUM_CLUSTERS];
    let mut sectors = [[0.0; 2]; NUM_CLUSTERS];
    for i in 0..NUM_CLUSTERS {
        let hue = (BASE_HUES[i] + offset).rem_euclid(360.0);
        centers[i][2] = stats.normalize_hue(hue);
        sectors[i][0] = stats.normalize_hue((hue - HUE_SECTOR_HALF_WIDTH).rem_euclid(360.0));
        sectors[i][1] = stats.normalize_hue((hue + HUE_SECTOR_HALF_WIDTH).rem_euclid(360.0));
    }

    let mut active = [true; NUM_CLUSTERS];
    let labels = run_constrained_kmeans(normalized, &mut centers, &sectors, &mut active)?;
    let swatches = build_swatches(&centers, &active, &labels, stats);
    Ok((swatches, labels))
}

fn hue_in_sector(hue: f64, sector: [f64; 2]) -> bool {
    let (lo, hi) = (sector[0], sector[1]);
    if lo < hi { hue >= lo && hue < hi } else { hue >= lo || hue < hi }
}

fn clamp_to_sector(hue: f64, sector: [f64; 2]) -> f64 {
    let (lo, hi) = (sector[0], sector[1]);
    if lo < hi {
        return if hue < lo { lo } else { hi };
    }
    let d_lo = (hue - lo).abs().min(360.0 - (hue - lo).abs());
    let d_hi = (hue - hi).abs().min(360.0 - (hue - hi).abs());
    if d_lo < d_hi { lo } else { hi }
}

fn nearest_active(points: &[[f64; 3]], centers: &[[f64; 3]; NUM_CLUSTERS], active: &[bool; NUM_CLUSTERS]) -> Result<Vec<u8>> {
    let active_ids: Vec<u8> = (0..NUM_CLUSTERS as u8).filter(|&k| active[k as usize]).collect();
    if active_ids.is_empty() {
        bail!("no active clusters remain, cannot assign nearest center");
    }

    let mut result = Vec::with_capacity(points.len());
    for p in points {
        let mut best_id = active_ids[0];
        let mut best_dist = f64::INFINITY;
        for &k in &active_ids {
            let c = centers[k as usize];
            let dx = p[0] - c[0];
            let dy = p[1] - c[1];
            let dz = p[2] - c[2];
            let dist = dx * dx + dy * dy + dz * dz;
            if dist < best_dist {
                best_dist = dist;
                best_id = k;
            }
        }
        result.push(best_id);
    }
    Ok(result)
}

fn run_constrained_kmeans(normalized: &[[f64; 3]], centers: &mut [[f64; 3]; NUM_CLUSTERS], sectors: &[[f64; 2]; NUM_CLUSTERS], active: &mut [bool; NUM_CLUSTERS]) -> Result<Vec<u8>> {
    let n = normalized.len();
    let mut labels = vec![UNASSIGNED; n];

    for _ in 0..MAX_ITERS {
        if !active.iter().any(|&a| a) {
            break;
        }

        let unassigned_idx: Vec<usize> = (0..n).filter(|&i| labels[i] == UNASSIGNED).collect();
        let mut assignments = labels.clone();

        let mut sums = [[0.0; 3]; NUM_CLUSTERS];
        let mut counts = [0usize; NUM_CLUSTERS];

        if !unassigned_idx.is_empty() {
            let pts: Vec<[f64; 3]> = unassigned_idx.iter().map(|&i| normalized[i]).collect();
            let nearest = nearest_active(&pts, centers, active)?;
            for (offset, &i) in unassigned_idx.iter().enumerate() {
                assignments[i] = nearest[offset];
            }
            for (&k, p) in nearest.iter().zip(pts.iter()) {
                let k = k as usize;
                sums[k][0] += p[0];
                sums[k][1] += p[1];
                sums[k][2] += p[2];
                counts[k] += 1;
            }
        }

        let converged = update_and_clamp(centers, sectors, active, &sums, &counts, &mut labels, &assignments);
        if converged {
            break;
        }
    }

    let remaining: Vec<usize> = (0..n).filter(|&i| labels[i] == UNASSIGNED).collect();
    if !remaining.is_empty() {
        let pts: Vec<[f64; 3]> = remaining.iter().map(|&i| normalized[i]).collect();
        let nearest = nearest_active(&pts, centers, active)?;
        for (&i, &k) in remaining.iter().zip(nearest.iter()) {
            labels[i] = k;
        }
    }

    Ok(labels)
}

fn update_and_clamp(
    centers: &mut [[f64; 3]; NUM_CLUSTERS], sectors: &[[f64; 2]; NUM_CLUSTERS], active: &mut [bool; NUM_CLUSTERS], sums: &[[f64; 3]; NUM_CLUSTERS], counts: &[usize; NUM_CLUSTERS], labels: &mut [u8],
    assignments: &[u8],
) -> bool {
    let mut converged = true;

    for k in 0..NUM_CLUSTERS {
        if !active[k] || counts[k] == 0 {
            continue;
        }

        let count = counts[k] as f64;
        let new_center = [sums[k][0] / count, sums[k][1] / count, sums[k][2] / count];

        if hue_in_sector(new_center[2], sectors[k]) {
            let dx = new_center[0] - centers[k][0];
            let dy = new_center[1] - centers[k][1];
            let dz = new_center[2] - centers[k][2];
            if dx * dx + dy * dy + dz * dz > TOLERANCE_SQ {
                converged = false;
            }
            centers[k] = new_center;
            continue;
        }

        centers[k] = [new_center[0], new_center[1], clamp_to_sector(new_center[2], sectors[k])];
        active[k] = false;
        converged = false;

        let k_u8 = k as u8;
        for (i, label) in labels.iter_mut().enumerate() {
            if *label == UNASSIGNED && assignments[i] == k_u8 {
                *label = k_u8;
            }
        }
    }

    converged
}

fn build_swatches(centers: &[[f64; 3]; NUM_CLUSTERS], active: &[bool; NUM_CLUSTERS], labels: &[u8], stats: &NormStats) -> Vec<Swatch> {
    let mut counts = [0usize; NUM_CLUSTERS];
    for &label in labels {
        counts[label as usize] += 1;
    }

    (0..NUM_CLUSTERS)
        .map(|k| {
            let denormalized = stats.denormalize(centers[k]);
            Swatch { color: Color::new(denormalized[0], denormalized[1], denormalized[2]), pixel_count: counts[k], active: active[k] }
        })
        .collect()
}
