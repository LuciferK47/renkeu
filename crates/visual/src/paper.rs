//! Tone mapping for the compiled page, so it can sit on a dark ground without
//! either blinding the reader or lying about the document's colours.
//!
//! Every other LaTeX tool solves this with a CSS filter, because a browser-side
//! viewer has no other lever: `filter: invert()` over the whole element. That
//! is comfortable and wrong — inversion moves hue, so a red admonition box
//! arrives cyan and a blue link arrives orange, and you cannot check a figure
//! you are about to publish. `Chameleon` (CHI 2025, arXiv:2503.06489) makes the
//! same argument for charts: adapting a light-mode visualisation to dark mode
//! has to preserve the semantic meaning of its colours.
//!
//! We rasterize server-side, so we own the pixels and can do it properly:
//! convert to `OKLab`, compress **lightness only** into a narrower band, and put
//! `a` and `b` back untouched. Red stays red; the page stops being a lamp.
//!
//! Contrast-polarity research (arXiv:2404.11454) found the better polarity
//! varies per individual, so the band is a setting rather than a constant we
//! pick for everyone.

use image::{DynamicImage, Rgba, RgbaImage};
use std::collections::HashMap;

/// How the page should be toned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaperTone {
    /// Lightness of paper white, 0.6..=1.0. At 1.0 the page is untouched.
    pub lightness: f32,
    /// Warm shift applied to `OKLab` `b`, 0.0..=40.0 in the same units the UI
    /// exposes. Paper is warm and screens are not; this is the only place in
    /// the interface where warmth is deliberate.
    pub warmth: f32,
}

impl Default for PaperTone {
    fn default() -> Self {
        // Comfortable for a long session without crushing the page into the
        // panel beside it. Matches the frontend default so a request that omits
        // the parameters renders the same image the UI would have asked for.
        Self {
            lightness: 0.88,
            warmth: 18.0,
        }
    }
}

impl PaperTone {
    /// Clamp caller-supplied values into the range the mapping is defined for.
    #[must_use]
    pub fn new(lightness: f32, warmth: f32) -> Self {
        Self {
            lightness: lightness.clamp(0.6, 1.0),
            warmth: warmth.clamp(0.0, 40.0),
        }
    }

    /// Whether this tone would leave the image byte-identical.
    #[must_use]
    pub fn is_identity(self) -> bool {
        self.lightness >= 1.0
    }
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// `sRGB` (0..=255) to `OKLab`.
#[allow(clippy::many_single_char_names)] // matches the published OKLab formulae
fn rgb_to_oklab(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = srgb_to_linear(f32::from(r) / 255.0);
    let g = srgb_to_linear(f32::from(g) / 255.0);
    let b = srgb_to_linear(f32::from(b) / 255.0);

    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();

    (
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    )
}

/// `OKLab` back to `sRGB` (0..=255), clamped into gamut.
#[allow(clippy::many_single_char_names)] // matches the published OKLab formulae
fn oklab_to_rgb(lightness: f32, a: f32, b: f32) -> (u8, u8, u8) {
    let l = (lightness + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m = (lightness - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s = (lightness - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);

    let r = 4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s;
    let g = -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s;
    let bb = -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s;

    let to_u8 = |v: f32| -> u8 {
        let c = linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            c.round().clamp(0.0, 255.0) as u8
        }
    };
    (to_u8(r), to_u8(g), to_u8(bb))
}

/// Map one colour. Lightness is compressed into `[floor, top]`; `a` and `b`
/// ride through unchanged apart from the deliberate warm shift, which is what
/// keeps hue intact.
#[allow(clippy::many_single_char_names)]
fn map_pixel(r: u8, g: u8, b: u8, tone: PaperTone) -> (u8, u8, u8) {
    let (lightness, a, bb) = rgb_to_oklab(r, g, b);
    let top = tone.lightness;
    // Lift pure black off the floor so body text reads as ink on paper rather
    // than as holes punched in it.
    let floor = 0.10 * top;
    let mapped = floor + lightness * (top - floor);
    // Scale the warm shift by lightness: paper warms, ink stays neutral.
    let warm = (tone.warmth / 1000.0) * lightness;
    oklab_to_rgb(mapped, a, bb + warm)
}

/// Apply `tone` to every pixel of `img`.
///
/// A rendered page has very few distinct colours — paper, ink, and the
/// antialiasing between them — while having millions of pixels, so results are
/// memoised per colour. Without that this is 3.7M `OKLab` round-trips for one
/// letter page at 150 dpi.
#[must_use]
pub fn tone_map(img: DynamicImage, tone: PaperTone) -> DynamicImage {
    if tone.is_identity() {
        return img;
    }

    let mut rgba: RgbaImage = img.to_rgba8();
    let mut memo: HashMap<[u8; 3], [u8; 3]> = HashMap::new();

    for Rgba(px) in rgba.pixels_mut() {
        let key = [px[0], px[1], px[2]];
        let out = *memo.entry(key).or_insert_with(|| {
            let (r, g, b) = map_pixel(key[0], key[1], key[2], tone);
            [r, g, b]
        });
        px[0] = out[0];
        px[1] = out[1];
        px[2] = out[2];
    }

    DynamicImage::ImageRgba8(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn img_of(pixels: &[[u8; 3]]) -> DynamicImage {
        let mut i = RgbaImage::new(u32::try_from(pixels.len()).unwrap_or(1), 1);
        for (x, p) in pixels.iter().enumerate() {
            i.put_pixel(
                u32::try_from(x).unwrap_or(0),
                0,
                Rgba([p[0], p[1], p[2], 255]),
            );
        }
        DynamicImage::ImageRgba8(i)
    }

    fn px(img: &DynamicImage, x: u32) -> [u8; 3] {
        let p = img.to_rgba8();
        let q = p.get_pixel(x, 0).0;
        [q[0], q[1], q[2]]
    }

    /// `OKLab` hue angle, the quantity inversion destroys and this must not.
    fn hue(rgb: [u8; 3]) -> f32 {
        let (_, a, b) = rgb_to_oklab(rgb[0], rgb[1], rgb[2]);
        b.atan2(a)
    }

    fn chroma(rgb: [u8; 3]) -> f32 {
        let (_, a, b) = rgb_to_oklab(rgb[0], rgb[1], rgb[2]);
        a.hypot(b)
    }

    #[test]
    fn lightness_one_leaves_the_page_untouched() {
        let src = [[255, 255, 255], [0, 0, 0], [192, 57, 43]];
        let out = tone_map(img_of(&src), PaperTone::new(1.0, 40.0));
        for (i, p) in src.iter().enumerate() {
            assert_eq!(
                px(&out, u32::try_from(i).unwrap_or(0)),
                *p,
                "pixel {i} should be unchanged"
            );
        }
    }

    #[test]
    fn paper_white_is_dimmed_but_stays_light() {
        let out = tone_map(img_of(&[[255, 255, 255]]), PaperTone::new(0.88, 0.0));
        let w = px(&out, 0);
        assert!(
            w[0] < 255,
            "paper must come down from pure white, got {w:?}"
        );
        assert!(
            w[0] > 200,
            "paper must stay clearly lighter than the UI around it, got {w:?}"
        );
    }

    #[test]
    fn hue_survives_the_mapping() {
        // The thesis: a red box stays red, a blue link stays blue. Inversion
        // fails this by roughly pi radians; we allow a hair of rounding.
        let src = [[192, 57, 43], [26, 95, 180], [30, 142, 78], [230, 126, 34]];
        let out = tone_map(img_of(&src), PaperTone::new(0.88, 18.0));
        for (i, original) in src.iter().enumerate() {
            let got = px(&out, u32::try_from(i).unwrap_or(0));
            let delta = (hue(*original) - hue(got)).abs();
            assert!(
                delta < 0.12,
                "hue drifted {delta:.3} rad for {original:?} -> {got:?}"
            );
        }
    }

    #[test]
    fn inversion_would_have_failed_that_hue_test() {
        // Guards the test above from becoming vacuous: if the assertion were
        // loose enough to pass for `filter: invert()`, it would prove nothing.
        let red = [192u8, 57, 43];
        let inverted = [255 - red[0], 255 - red[1], 255 - red[2]];
        let delta = (hue(red) - hue(inverted)).abs();
        assert!(
            delta > 1.0,
            "inversion should move hue a long way, moved {delta:.3} rad"
        );
    }

    #[test]
    fn saturated_colour_keeps_its_chroma() {
        let red = [192u8, 57, 43];
        let out = tone_map(img_of(&[red]), PaperTone::new(0.88, 18.0));
        let got = px(&out, 0);
        let ratio = chroma(got) / chroma(red);
        assert!(
            (0.75..=1.25).contains(&ratio),
            "chroma changed by {ratio:.2}x: {red:?} -> {got:?}"
        );
    }

    #[test]
    fn ink_stays_darker_than_paper() {
        let out = tone_map(img_of(&[[255, 255, 255], [0, 0, 0]]), PaperTone::default());
        let paper = px(&out, 0);
        let ink = px(&out, 1);
        assert!(
            i32::from(paper[0]) - i32::from(ink[0]) > 150,
            "ink and paper must stay far apart: {ink:?} vs {paper:?}"
        );
    }

    #[test]
    fn warmth_pushes_paper_yellow_not_blue() {
        let cool = tone_map(img_of(&[[255, 255, 255]]), PaperTone::new(0.88, 0.0));
        let warm = tone_map(img_of(&[[255, 255, 255]]), PaperTone::new(0.88, 40.0));
        let c = px(&cool, 0);
        let w = px(&warm, 0);
        assert!(
            w[2] < c[2],
            "warmth should reduce blue: cool {c:?} vs warm {w:?}"
        );
        assert!(
            w[0] >= c[0],
            "warmth should not reduce red: cool {c:?} vs warm {w:?}"
        );
    }

    #[test]
    fn alpha_is_preserved() {
        let mut i = RgbaImage::new(1, 1);
        i.put_pixel(0, 0, Rgba([255, 255, 255, 128]));
        let out = tone_map(DynamicImage::ImageRgba8(i), PaperTone::default());
        assert_eq!(out.to_rgba8().get_pixel(0, 0).0[3], 128);
    }

    #[test]
    fn values_are_clamped_into_range() {
        let t = PaperTone::new(-5.0, 900.0);
        assert!((t.lightness - 0.6).abs() < f32::EPSILON);
        assert!((t.warmth - 40.0).abs() < f32::EPSILON);
    }

    #[test]
    fn mapping_is_deterministic() {
        let src = [[255, 255, 255], [17, 17, 17], [192, 57, 43]];
        let a = tone_map(img_of(&src), PaperTone::default());
        let b = tone_map(img_of(&src), PaperTone::default());
        assert_eq!(a.to_rgba8().into_raw(), b.to_rgba8().into_raw());
    }
}
