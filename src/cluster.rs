use crate::color::Color;

pub const NUM_CLUSTERS: usize = 8;

const BASE_HUES: [f64; NUM_CLUSTERS] = [0.0, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0];
const HUE_OFFSETS: [f64; 3] = [0.0, 15.0, 30.0];
const HUE_SECTOR_HALF_WIDTH: f64 = 22.5;
const MAX_ITERS: usize = 100;
const TOLERANCE: f64 = 1e-4;
const UNASSIGNED: u8 = u8::MAX;
const CHROMA_THRESHOLD: f64 = 0.01;

#[derive(Clone, Copy, Debug)]
pub struct Swatch {
    pub color: Color,
    pub pixel_count: usize,
    pub active: bool,
}

pub struct ClusteringResult {
    pub swatches: [Swatch; NUM_CLUSTERS],
    pub labels: Vec<u8>,
    pub avg_chroma: f64,
    pub pixels: Vec<Color>,
}

#[derive(Clone, Copy, Debug)]
struct NormStats {
    mean: [f64; 3],
    std: [f64; 3],
}

impl NormStats {
    fn denormalize(self, v: [f64; 3]) -> [f64; 3] {
        [v[0] * self.std[0] + self.mean[0], v[1] * self.std[1] + self.mean[1], v[2] * self.std[2] + self.mean[2]]
    }

    fn normalize_hue(self, h: f64) -> f64 {
        (h - self.mean[2]) / self.std[2]
    }
}

impl ClusteringResult {
    pub fn from_pixels(pixels: &[Color]) -> Self {
        let mut filtered: Vec<Color> = pixels.iter().copied().filter(|p| p.chroma >= CHROMA_THRESHOLD).collect();

        let avg_chroma = if filtered.is_empty() {
            filtered = if pixels.is_empty() { vec![Color::new(0.5, 0.0, 0.0)] } else { pixels.to_vec() };
            0.0
        } else {
            filtered.iter().map(|p| p.chroma).sum::<f64>() / filtered.len() as f64
        };

        let mut normalized: Vec<[f64; 3]> = filtered.iter().map(|p| [p.l, p.chroma, p.hue]).collect();
        let stats = z_normalize(&mut normalized);
        let (swatches, labels) = select_best_offset(&normalized, stats);

        Self { swatches, labels, avg_chroma, pixels: filtered }
    }
}

fn z_normalize(pixels: &mut [[f64; 3]]) -> NormStats {
    let n = pixels.len() as f64;
    debug_assert!(n > 0.0);

    let mut mean = [0.0; 3];
    for p in pixels.iter() {
        for c in 0..3 {
            mean[c] += p[c];
        }
    }
    for m in &mut mean {
        *m /= n;
    }

    let mut var = [0.0; 3];
    for p in pixels.iter() {
        for c in 0..3 {
            let d = p[c] - mean[c];
            var[c] += d * d;
        }
    }

    let mut std = [0.0; 3];
    for c in 0..3 {
        std[c] = (var[c] / n).sqrt().max(1e-8);
    }

    for p in pixels.iter_mut() {
        for c in 0..3 {
            p[c] = (p[c] - mean[c]) / std[c];
        }
    }

    NormStats { mean, std }
}

fn score(swatches: &[Swatch; NUM_CLUSTERS], total: usize) -> (usize, f64) {
    let mut active_cnt = 0usize;
    let mut active_pixels = 0usize;
    for s in swatches {
        if s.active {
            active_cnt += 1;
            active_pixels += s.pixel_count;
        }
    }
    let coverage = if total == 0 { 0.0 } else { active_pixels as f64 / total as f64 };

    (active_cnt, coverage)
}

fn select_best_offset(normalized: &[[f64; 3]], stats: NormStats) -> ([Swatch; NUM_CLUSTERS], Vec<u8>) {
    let total = normalized.len();
    let mut best = cluster_with_offset(normalized, stats, HUE_OFFSETS[0]);
    let mut best_score = score(&best.0, total);

    for &off in &HUE_OFFSETS[1..] {
        let candidate = cluster_with_offset(normalized, stats, off);
        let s = score(&candidate.0, total);
        if s > best_score {
            best = candidate;
            best_score = s;
        }
    }

    best
}

fn cluster_with_offset(normalized: &[[f64; 3]], stats: NormStats, offset: f64) -> ([Swatch; NUM_CLUSTERS], Vec<u8>) {
    let mut centers = [[0.0; 3]; NUM_CLUSTERS];
    let mut sectors = [[0.0; 2]; NUM_CLUSTERS];

    for i in 0..NUM_CLUSTERS {
        let hue = (BASE_HUES[i] + offset).rem_euclid(360.0);
        centers[i][2] = stats.normalize_hue(hue);

        let lo = (hue - HUE_SECTOR_HALF_WIDTH).rem_euclid(360.0);
        let hi = (hue + HUE_SECTOR_HALF_WIDTH).rem_euclid(360.0);
        sectors[i] = [stats.normalize_hue(lo), stats.normalize_hue(hi)];
    }

    let mut active = [true; NUM_CLUSTERS];
    let labels = run_constrained_kmeans(normalized, &mut centers, &sectors, &mut active);
    let swatches = build_swatches(&centers, &active, &labels, stats);

    (swatches, labels)
}

fn hue_in_sector(h: f64, sector: [f64; 2]) -> bool {
    let (lo, hi) = (sector[0], sector[1]);
    if lo < hi { h >= lo && h < hi } else { h >= lo || h < hi }
}

fn clamp_to_sector(h: f64, sector: [f64; 2]) -> f64 {
    let (lo, hi) = (sector[0], sector[1]);
    if lo < hi {
        if h < lo { lo } else { hi }
    } else {
        let d_lo = (h - lo).abs().min(360.0 - (h - lo).abs());
        let d_hi = (h - hi).abs().min(360.0 - (h - hi).abs());
        if d_lo < d_hi { lo } else { hi }
    }
}

fn nearest_active_center(point: [f64; 3], centers: &[[f64; 3]; NUM_CLUSTERS], active: &[bool; NUM_CLUSTERS]) -> usize {
    let mut best = 0usize;
    let mut best_dist = f64::INFINITY;
    for (k, &is_active) in active.iter().enumerate() {
        if !is_active {
            continue;
        }
        let d = (point[0] - centers[k][0]).powi(2) + (point[1] - centers[k][1]).powi(2) + (point[2] - centers[k][2]).powi(2);
        if d < best_dist {
            best_dist = d;
            best = k;
        }
    }
    best
}

fn run_constrained_kmeans(normalized: &[[f64; 3]], centers: &mut [[f64; 3]; NUM_CLUSTERS], sectors: &[[f64; 2]; NUM_CLUSTERS], active: &mut [bool; NUM_CLUSTERS]) -> Vec<u8> {
    let n = normalized.len();
    let mut labels = vec![UNASSIGNED; n];

    for _ in 0..MAX_ITERS {
        if !active.iter().any(|&a| a) {
            break;
        }

        let (assignments, sums, counts) = assign_to_nearest(normalized, &labels, centers, active);
        if update_and_clamp(centers, sectors, active, &sums, &counts, &mut labels, &assignments) {
            break;
        }
    }

    for (lab, pt) in labels.iter_mut().zip(normalized) {
        if *lab == UNASSIGNED {
            *lab = nearest_active_center(*pt, centers, active) as u8;
        }
    }

    labels
}

fn assign_to_nearest(normalized: &[[f64; 3]], labels: &[u8], centers: &[[f64; 3]; NUM_CLUSTERS], active: &[bool; NUM_CLUSTERS]) -> (Vec<u8>, [[f64; 3]; NUM_CLUSTERS], [usize; NUM_CLUSTERS]) {
    let mut assignments = vec![0u8; normalized.len()];
    let mut sums = [[0.0; 3]; NUM_CLUSTERS];
    let mut counts = [0usize; NUM_CLUSTERS];

    for (i, &pt) in normalized.iter().enumerate() {
        let k = if labels[i] == UNASSIGNED {
            let k = nearest_active_center(pt, centers, active);
            for c in 0..3 {
                sums[k][c] += pt[c];
            }
            counts[k] += 1;
            k
        } else {
            labels[i] as usize
        };
        assignments[i] = k as u8;
    }

    (assignments, sums, counts)
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

        let inv = 1.0 / counts[k] as f64;
        let new_center = [sums[k][0] * inv, sums[k][1] * inv, sums[k][2] * inv];

        if hue_in_sector(new_center[2], sectors[k]) {
            let shift_sq = (new_center[0] - centers[k][0]).powi(2) + (new_center[1] - centers[k][1]).powi(2) + (new_center[2] - centers[k][2]).powi(2);
            if shift_sq > TOLERANCE * TOLERANCE {
                converged = false;
            }
            centers[k] = new_center;
            continue;
        }

        let clamped = clamp_to_sector(new_center[2], sectors[k]);
        centers[k] = [new_center[0], new_center[1], clamped];
        active[k] = false;
        converged = false;

        for (lab, &assigned) in labels.iter_mut().zip(assignments.iter()) {
            if *lab == UNASSIGNED && assigned as usize == k {
                *lab = k as u8;
            }
        }
    }

    converged
}

fn build_swatches(centers: &[[f64; 3]; NUM_CLUSTERS], active: &[bool; NUM_CLUSTERS], labels: &[u8], stats: NormStats) -> [Swatch; NUM_CLUSTERS] {
    let mut swatches = [Swatch { color: Color::default(), pixel_count: 0, active: false }; NUM_CLUSTERS];

    for k in 0..NUM_CLUSTERS {
        let [l, ch, h] = stats.denormalize(centers[k]);
        swatches[k] = Swatch { color: Color::new(l, ch, h), pixel_count: 0, active: active[k] };
    }

    for &label in labels {
        swatches[label as usize].pixel_count += 1;
    }

    swatches
}
