use image::RgbImage;

pub const CHROMA_THRESHOLD: f64 = 0.01;

type Mat3 = [[f64; 3]; 3];

const M_SRGB_TO_XYZ: Mat3 = [[0.4124564, 0.3575761, 0.1804375], [0.2126729, 0.7151522, 0.0721750], [0.0193339, 0.1191920, 0.9503041]];
const M_XYZ_TO_LMS: Mat3 = [[0.8189330101, 0.3618667424, -0.1288597137], [0.0329845436, 0.9293118715, 0.0361456387], [0.0482003018, 0.2643662691, 0.6338517070]];
const M_LMS_TO_OKLAB: Mat3 = [[0.2104542553, 0.7936177850, -0.0040720468], [1.9779984951, -2.4285922050, 0.4505937099], [0.0259040371, 0.7827717662, -0.8086757660]];
const M_OKLAB_TO_LMS_PRIME: Mat3 = [[1.0, 0.3963377774, 0.2158037573], [1.0, -0.1055613458, -0.0638541728], [1.0, -0.0894841775, -1.2914855480]];
const M_LMS_TO_XYZ: Mat3 = [[1.2270138511, -0.5577999807, 0.2812561490], [-0.0405801784, 1.1122568696, -0.0716766787], [-0.0763812845, -0.4214819784, 1.5861632204]];
const M_XYZ_TO_SRGB: Mat3 = [[3.2404542, -1.5371385, -0.4985314], [-0.9692660, 1.8760108, 0.0415560], [0.0556434, -0.2040259, 1.0572252]];

fn mat_vec(m: &Mat3, v: [f64; 3]) -> [f64; 3] {
    [m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2], m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2], m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2]]
}

fn srgb_to_linear(channel: u8) -> f64 {
    let c = channel as f64 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(linear: f64) -> u8 {
    let clamped = linear.max(0.0);
    let srgb = if linear <= 0.0031308 { linear * 12.92 } else { 1.055 * clamped.powf(1.0 / 2.4) - 0.055 };
    (srgb.clamp(0.0, 1.0) * 255.0) as u8
}

fn oklab_to_oklch(l: f64, a: f64, b: f64) -> (f64, f64, f64) {
    let hue = b.atan2(a).to_degrees().rem_euclid(360.0);
    (l, a.hypot(b), hue)
}

fn oklch_to_oklab(l: f64, chroma: f64, hue: f64) -> (f64, f64, f64) {
    let rad = hue.to_radians();
    (l, chroma * rad.cos(), chroma * rad.sin())
}

pub fn srgb_to_oklch(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let linear = [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b)];
    let xyz = mat_vec(&M_SRGB_TO_XYZ, linear);
    let mut lms = mat_vec(&M_XYZ_TO_LMS, xyz);
    for v in &mut lms {
        *v = v.cbrt();
    }
    let oklab = mat_vec(&M_LMS_TO_OKLAB, lms);
    oklab_to_oklch(oklab[0], oklab[1], oklab[2])
}

pub fn oklch_to_srgb(l: f64, chroma: f64, hue: f64) -> [u8; 3] {
    let (l, a, b) = oklch_to_oklab(l, chroma, hue);
    let mut lms = mat_vec(&M_OKLAB_TO_LMS_PRIME, [l, a, b]);
    for v in &mut lms {
        *v = v.powi(3);
    }
    let xyz = mat_vec(&M_LMS_TO_XYZ, lms);
    let srgb = mat_vec(&M_XYZ_TO_SRGB, xyz);
    [linear_to_srgb(srgb[0]), linear_to_srgb(srgb[1]), linear_to_srgb(srgb[2])]
}

pub fn image_to_oklch_pixels(image: &RgbImage) -> Vec<[f64; 3]> {
    let mut pixels = Vec::with_capacity(image.as_raw().len() / 3);
    for channels in image.as_raw().chunks_exact(3) {
        let (l, chroma, hue) = srgb_to_oklch(channels[0], channels[1], channels[2]);
        pixels.push([l, chroma, hue]);
    }
    pixels
}

#[derive(Clone, Copy)]
pub struct Color {
    pub l: f64,
    pub chroma: f64,
    pub hue: f64,
}

impl Color {
    pub fn new(l: f64, chroma: f64, hue: f64) -> Self {
        Self { l, chroma, hue: hue.rem_euclid(360.0) }
    }

    pub fn to_srgb(self) -> [u8; 3] {
        oklch_to_srgb(self.l, self.chroma, self.hue)
    }

    pub fn to_hex(self) -> String {
        let [r, g, b] = self.to_srgb();
        hex::encode([r, g, b])
    }
}
