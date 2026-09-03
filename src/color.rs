#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub l: f64,
    pub chroma: f64,
    pub hue: f64,
}

impl Default for Color {
    fn default() -> Self {
        Self { l: 0.5, chroma: 0.0, hue: 0.0 }
    }
}

impl Color {
    #[inline]
    pub fn new(l: f64, chroma: f64, hue: f64) -> Self {
        Self { l, chroma, hue: hue.rem_euclid(360.0) }
    }

    pub fn from_srgb(r: u8, g: u8, b: u8) -> Self {
        let (l, c, h) = srgb_to_oklch(r, g, b);
        Self::new(l, c, h)
    }

    pub fn to_srgb(self) -> (u8, u8, u8) {
        oklch_to_srgb(self.l, self.chroma, self.hue)
    }

    pub fn to_hex(self) -> String {
        let (r, g, b) = self.to_srgb();
        format!("{r:02x}{g:02x}{b:02x}")
    }
}

const M_RGB_TO_XYZ: [[f64; 3]; 3] = [[0.4124564, 0.3575761, 0.1804375], [0.2126729, 0.7151522, 0.0721750], [0.0193339, 0.1191920, 0.9503041]];

const M_XYZ_TO_LMS: [[f64; 3]; 3] = [[0.8189330101, 0.3618667424, -0.1288597137], [0.0329845436, 0.9293118715, 0.0361456387], [0.0482003018, 0.2643662691, 0.6338517070]];

const M_LMS_TO_OKLAB: [[f64; 3]; 3] = [[0.2104542553, 0.7936177850, -0.0040720468], [1.9779984951, -2.4285922050, 0.4505937099], [0.0259040371, 0.7827717662, -0.8086757660]];

const M_OKLAB_TO_LMS: [[f64; 3]; 3] = [[1.0, 0.3963377774, 0.2158037573], [1.0, -0.1055613458, -0.0638541728], [1.0, -0.0894841775, -1.2914855480]];

const M_LMS_TO_XYZ: [[f64; 3]; 3] = [[1.2270138511, -0.5577999807, 0.2812561490], [-0.0405801784, 1.1122568696, -0.0716766787], [-0.0763812845, -0.4214819784, 1.5861632204]];

const M_XYZ_TO_RGB: [[f64; 3]; 3] = [[3.2404542, -1.5371385, -0.4985314], [-0.9692660, 1.8760108, 0.0415560], [0.0556434, -0.2040259, 1.0572252]];

fn mat_vec_mul(m: &[[f64; 3]; 3], v: &[f64; 3]) -> [f64; 3] {
    [m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2], m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2], m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2]]
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.max(0.0).powf(1.0 / 2.4) - 0.055 }
}

fn srgb_to_oklch(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let rgb_lin = [srgb_to_linear(f64::from(r) / 255.0), srgb_to_linear(f64::from(g) / 255.0), srgb_to_linear(f64::from(b) / 255.0)];
    let xyz = mat_vec_mul(&M_RGB_TO_XYZ, &rgb_lin);
    let lms = mat_vec_mul(&M_XYZ_TO_LMS, &xyz);

    let lms_cbrt = [lms[0].cbrt(), lms[1].cbrt(), lms[2].cbrt()];
    let oklab = mat_vec_mul(&M_LMS_TO_OKLAB, &lms_cbrt);

    let l = oklab[0];
    let a = oklab[1];
    let b = oklab[2];
    let c = a.hypot(b);

    let mut h = b.atan2(a).to_degrees();
    if h < 0.0 {
        h += 360.0;
    }

    (l, c, h)
}

fn oklch_to_srgb(l: f64, c: f64, h: f64) -> (u8, u8, u8) {
    let rad = h.to_radians();
    let oklab = [l, c * rad.cos(), c * rad.sin()];
    let lms_prime = mat_vec_mul(&M_OKLAB_TO_LMS, &oklab);

    let lms3 = [lms_prime[0].powi(3), lms_prime[1].powi(3), lms_prime[2].powi(3)];

    let xyz = mat_vec_mul(&M_LMS_TO_XYZ, &lms3);
    let lin = mat_vec_mul(&M_XYZ_TO_RGB, &xyz);

    let r = linear_to_srgb(lin[0]).clamp(0.0, 1.0) * 255.0;
    let g = linear_to_srgb(lin[1]).clamp(0.0, 1.0) * 255.0;
    let b = linear_to_srgb(lin[2]).clamp(0.0, 1.0) * 255.0;

    (r as u8, g as u8, b as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_black_white() {
        let black = Color::from_srgb(0, 0, 0);
        assert!(black.l < 0.01);

        let back = black.to_srgb();
        assert_eq!(back, (0, 0, 0));

        let white = Color::from_srgb(255, 255, 255);
        assert!((white.l - 1.0).abs() < 0.01);
        assert!(white.chroma < 0.01);
    }

    #[test]
    fn pure_red_hue() {
        let red = Color::from_srgb(255, 0, 0);
        assert!(red.chroma > 0.1);
    }
}
