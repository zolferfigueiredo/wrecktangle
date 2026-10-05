//! Rebuilds `assets/wrecktangle.ico` and `assets/wrecktangle-256.png` from the
//! logo SVGs in `assets/`: `cargo run --example make_icon`.

#[cfg(windows)]
fn main() {
    use resvg::{tiny_skia, usvg};

    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    let render = |file: &str, size: u32| -> Vec<u8> {
        let data = std::fs::read(assets.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        let tree = usvg::Tree::from_data(&data, &usvg::Options::default())
            .unwrap_or_else(|e| panic!("{file}: {e}"));
        let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("non-zero size");
        let scale = size as f32 / tree.size().width();
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        pixmap.encode_png().expect("PNG encoding")
    };

    // The tray and title bar sizes have their own pixel-snapped drawings;
    // the full artwork turns to mush below 40 px.
    let frames: Vec<(u32, Vec<u8>)> = [16, 20, 24, 32, 40, 48, 64, 256]
        .into_iter()
        .map(|size| {
            let file = match size {
                16 | 20 | 24 | 32 => format!("wrecktangle-{size}.svg"),
                _ => "wrecktangle.svg".to_string(),
            };
            (size, render(&file, size))
        })
        .collect();

    let mut ico = vec![0, 0, 1, 0];
    ico.extend_from_slice(&(frames.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * frames.len() as u32;
    for (size, png) in &frames {
        // An ICO directory entry stores 256 as 0.
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        ico.extend_from_slice(&[dim, dim, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in &frames {
        ico.extend_from_slice(png);
    }

    std::fs::write(assets.join("wrecktangle.ico"), ico).expect("write wrecktangle.ico");
    std::fs::write(
        assets.join("wrecktangle-256.png"),
        &frames[frames.len() - 1].1,
    )
    .expect("write wrecktangle-256.png");
}

#[cfg(not(windows))]
fn main() {}
