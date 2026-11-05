use std::fs;

use resvg::tiny_skia;
use resvg::usvg;

fn main() {
    let source = fs::read_to_string("assets/logo.svg").expect("read logo.svg");
    let svg_open = source.find("<svg").expect("svg element");
    let start = source[svg_open..].find('>').expect("svg start") + svg_open + 1;
    let end = source.rfind("</svg>").expect("svg end");
    let inner = &source[start..end];

    // White cat head on a dark rounded tile: readable at 16-32 px on the taskbar.
    let wrapped = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256">
<rect width="256" height="256" rx="56" fill="#15151b"/>
<g transform="translate(27.5 27.5) scale(3.0)">{inner}</g>
</svg>"##
    );

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
