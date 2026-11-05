use std::fs;

use resvg::tiny_skia;
use resvg::usvg;

fn main() {
    // A clean vector cat face instead of the traced logo: the traced path has
    // a square backing that showed up as a thin frame on the taskbar, and its
    // details disappeared at 16 px. This design is a rounded gradient tile
    // with a soft white cat, no outline, and large eyes that survive scaling.
    let wrapped = r##"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256">
<defs>
  <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">
    <stop offset="0" stop-color="#a855f7"/>
    <stop offset="0.55" stop-color="#d946ef"/>
    <stop offset="1" stop-color="#ec4899"/>
  </linearGradient>
  <linearGradient id="fur" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#ffffff"/>
    <stop offset="1" stop-color="#fdf4ff"/>
  </linearGradient>
</defs>
<rect width="256" height="256" rx="60" fill="url(#bg)"/>
<path d="M64 110 L52 38 Q50 29 58 34 L120 72 Z" fill="url(#fur)"/>
<path d="M192 110 L204 38 Q206 29 198 34 L136 72 Z" fill="url(#fur)"/>
<path d="M128 60 C178 60 214 92 214 138 C214 184 178 212 128 212 C78 212 42 184 42 138 C42 92 78 60 128 60 Z" fill="url(#fur)"/>
<ellipse cx="96" cy="132" rx="14" ry="18" fill="#3b0764"/>
<ellipse cx="160" cy="132" rx="14" ry="18" fill="#3b0764"/>
<path d="M128 158 L139 169 L117 169 Z" fill="#f472b6"/>
</svg>"##;

    let tree = usvg::Tree::from_data(wrapped.as_bytes(), &usvg::Options::default())
        .expect("parse icon svg");

    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let mut frames = Vec::new();

    for size in sizes {
        let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("pixmap");
        let scale = size as f32 / 256.0;
        resvg::render(
            &tree,
            usvg::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        let png = pixmap.encode_png().expect("png");
        if size == 256 {
            fs::write("assets/logo.png", &png).expect("write logo.png");
        }
        frames.push((size, png));
    }

    let mut output = Vec::new();
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&(frames.len() as u16).to_le_bytes());

    let mut offset = 6 + 16 * frames.len() as u32;
    for (size, png) in &frames {
        output.push(if *size >= 256 { 0 } else { *size as u8 });
        output.push(if *size >= 256 { 0 } else { *size as u8 });
        output.push(0);
        output.push(0);
        output.extend_from_slice(&1u16.to_le_bytes());
        output.extend_from_slice(&32u16.to_le_bytes());
        output.extend_from_slice(&(png.len() as u32).to_le_bytes());
        output.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in &frames {
        output.extend_from_slice(png);
    }

    fs::write("assets/logo.ico", output).expect("write logo.ico");
}
