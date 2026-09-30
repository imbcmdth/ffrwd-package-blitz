//! Fonts: DejaVu Sans behind every generic family, and `@font-face` fonts
//! from `data:` URIs in TrueType, WOFF and WOFF2.
//!
//! BoxTest (fixtures/make_boxfont.py) draws every letter as a box two ems
//! wide, so a line set in it is far wider than the same line in DejaVu
//! Sans, which is what any family falls back to.

use crate::common::*;

const BOXFONT_TTF: &[u8] = include_bytes!("../fixtures/boxfont.ttf");
const BOXFONT_WOFF: &[u8] = include_bytes!("../fixtures/boxfont.woff");
const BOXFONT_WOFF2: &[u8] = include_bytes!("../fixtures/boxfont.woff2");

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= c.len() {
                s.push(T[(n >> (18 - 6 * k)) as usize & 63] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

/// The laid-out width of "AAAA" at 20px in `family`, with `face_css` added.
fn width_of(family: &str, face_css: &str) -> f32 {
    let html = doc(
        &format!(
            "{face_css}\n#t {{ position: absolute; left: 0; top: 0; font-family: {family}; font-size: 20px; white-space: nowrap; }}"
        ),
        r#"<span id="t">AAAA</span>"#,
    );
    let mut s = session(&html, 400, 100, 1);
    // The font is registered when its load is taken in, at a resolve.
    render(&mut s, 400, 100, 0.0, 0);
    render(&mut s, 400, 100, 1.0 / 30.0, 1);
    s.compositor().rect_of("#t").unwrap().2
}

#[test]
fn every_generic_family_is_dejavu_sans() {
    let sans = width_of("sans-serif", "");
    for family in [
        "serif",
        "monospace",
        "cursive",
        "fantasy",
        "system-ui",
        "NoSuchFont",
    ] {
        let w = width_of(family, "");
        assert!((w - sans).abs() < 0.01, "{family}: {w} against {sans}");
    }
    assert!((sans - 160.0).abs() > 50.0, "{sans}");
}

fn face(mime: &str, bytes: &[u8], hint: &str) -> String {
    format!(
        "@font-face {{ font-family: BoxTest; src: url(data:{mime};base64,{}) {hint}; }}",
        base64(bytes)
    )
}

#[test]
fn font_face_loads_data_uris_in_every_format_with_a_format_hint() {
    for (mime, bytes, hints) in [
        (
            "font/ttf",
            BOXFONT_TTF,
            &["format(truetype)", "format(opentype)", "format(\"ttf\")"][..],
        ),
        ("font/woff", BOXFONT_WOFF, &["format(woff)"][..]),
        (
            "font/woff2",
            BOXFONT_WOFF2,
            &["format(woff2)", "format(\"woff2\")"][..],
        ),
    ] {
        for hint in hints {
            let w = width_of("BoxTest", &face(mime, bytes, hint));
            assert!((w - 160.0).abs() < 0.5, "{mime} {hint}: {w}");
        }
    }
}

/// Blitz learns a font's type from the format hint, or else from the URL's
/// file extension, which a `data:` URI has none of; and it reads only some
/// quoted spellings of the hint. Without one it skips the source, and the
/// text falls back to DejaVu Sans.
#[test]
fn font_face_skips_a_data_uri_without_a_hint_it_reads() {
    let fallback = width_of("sans-serif", "");
    for (mime, bytes, hint) in [
        ("font/ttf", BOXFONT_TTF, ""),
        ("font/ttf", BOXFONT_TTF, "format(\"truetype\")"),
        ("font/woff", BOXFONT_WOFF, "format(\"woff\")"),
        ("font/woff2", BOXFONT_WOFF2, ""),
    ] {
        let w = width_of("BoxTest", &face(mime, bytes, hint));
        assert!((w - fallback).abs() < 0.01, "{mime} {hint:?}: {w}");
    }
}
