//! Images: `data:` URIs render on the frame they are inserted; other
//! schemes load nothing; SVG is drawn as one picture.

use crate::common::*;

const W: u32 = 200;
const H: u32 = 100;

/// A 2x2 PNG, every pixel `rgb`, as a data: URI (stored deflate blocks, so
/// the test needs no encoder).
fn png_uri(rgb: [u8; 3]) -> String {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for b in data {
            c ^= *b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    fn adler(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for x in data {
            a = (a + *x as u32) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend(data);
        out.extend(&c);
        out.extend(crc(&c).to_be_bytes());
    }
    let raw: Vec<u8> = (0..2)
        .flat_map(|_| [0u8].into_iter().chain(rgb.repeat(2)))
        .collect();
    let mut z = vec![0x78, 0x01, 0x01];
    z.extend((raw.len() as u16).to_le_bytes());
    z.extend((!(raw.len() as u16)).to_le_bytes());
    z.extend(&raw);
    z.extend(adler(&raw).to_be_bytes());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend(2u32.to_be_bytes());
    ihdr.extend(2u32.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &z);
    chunk(&mut png, b"IEND", &[]);
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::from("data:image/png;base64,");
    for c in png.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            s.push(if k <= c.len() {
                T[(n >> (18 - 6 * k)) as usize & 63] as char
            } else {
                '='
            });
        }
    }
    s
}

fn rgb(f: &[u8], x: u32, y: u32) -> [u8; 3] {
    let p = px(f, W, x, y);
    [p[0], p[1], p[2]]
}

fn after_insert(markup: &str) -> Vec<u8> {
    let html = doc(
        "html, body { background: #000; } #slot > * { position: absolute; left: 10px; top: 10px; width: 50px; height: 50px; }",
        r#"<div id="slot"></div>"#,
    );
    let mut s = session(&html, W, H, 1);
    render(&mut s, W, H, 0.0, 0);
    let row = serde_json::json!({"at": 0.02, "select": "#slot", "html": markup}).to_string();
    s.fold(0.0, &[row]);
    bytes(render(&mut s, W, H, ft(1), 1).0)
}

#[test]
fn a_data_png_shows_on_the_frame_it_is_inserted() {
    let f = after_insert(&format!(r#"<img src="{}">"#, png_uri([255, 128, 0])));
    assert_eq!(rgb(&f, 35, 35), [255, 128, 0]);
}

#[test]
fn a_data_png_background_shows_on_the_frame_it_is_inserted() {
    let f = after_insert(&format!(
        r#"<div style="background-image: url('{}'); background-size: 100% 100%"></div>"#,
        png_uri([0, 128, 255])
    ));
    assert_eq!(rgb(&f, 35, 35), [0, 128, 255]);
}

#[test]
fn other_schemes_load_nothing() {
    for src in [
        "http://127.0.0.1:9/x.png",
        "https://example.invalid/x.png",
        "file:///C:/Windows/win.ini",
    ] {
        let f = after_insert(&format!(r#"<img src="{src}">"#));
        assert_eq!(rgb(&f, 35, 35), [0, 0, 0], "{src}");
    }
}

#[test]
fn inline_and_data_svg_render() {
    let svg = r##"<svg width="50" height="50" viewBox="0 0 50 50"><rect x="0" y="0" width="50" height="50" fill="#00ff00"/></svg>"##;
    let f = after_insert(svg);
    assert_eq!(rgb(&f, 35, 35), [0, 255, 0]);
    let uri = format!(
        "data:image/svg+xml,{}",
        svg.replace('#', "%23")
            .replace('<', "%3C")
            .replace('>', "%3E")
            .replace('"', "'")
    );
    let f = after_insert(&format!(r#"<img src="{uri}">"#));
    assert_eq!(rgb(&f, 35, 35), [0, 255, 0]);
}

#[test]
fn page_css_does_not_reach_inside_an_svg() {
    let html = doc(
        "html, body { background: #000; } svg rect { fill: #ff0000 !important; } svg { position: absolute; left: 10px; top: 10px; }",
        r##"<svg width="50" height="50"><rect x="0" y="0" width="50" height="50" fill="#00ff00"/></svg>"##,
    );
    let mut s = session(&html, W, H, 1);
    let f = bytes(render(&mut s, W, H, 0.0, 0).0);
    assert_eq!(rgb(&f, 35, 35), [0, 255, 0]);
}
