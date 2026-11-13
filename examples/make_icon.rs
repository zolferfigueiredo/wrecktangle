//! Rebuilds `assets/wrecktangle.ico`, `assets/wrecktangle-256.png`, the
//! Linux window icon PNGs (`assets/wrecktangle-32.png` to `-64.png`) and the
//! Linux tray PNGs (`assets/wrecktangle-tray-white-16.png` to
//! `-black-64.png`) from the logo SVGs in `assets/`:
//! `cargo run --example make_icon`.

fn main() {
    use resvg::{tiny_skia, usvg};

    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    let draw = |file: &str, size: u32| -> tiny_skia::Pixmap {
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
        pixmap
    };
    let render = |file: &str, size: u32| draw(file, size).encode_png().expect("PNG encoding");
    let write = |file: &str, png: &[u8]| {
        std::fs::write(assets.join(file), png).unwrap_or_else(|e| panic!("{file}: {e}"));
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

    // The Settings window's icon on Linux is sent as pixels.
    for (size, png) in frames
        .iter()
        .filter(|(size, _)| [32, 48, 64].contains(size))
    {
        write(&format!("wrecktangle-{size}.png"), png);
    }

    // The Linux tray icon is one color, drawn black: white for dark
    // panels is the same coverage with every channel at full. The small
    // sizes have their own pixel-snapped drawings; the scalable one is a
    // 16 px grid, so 48 and 64 land on whole pixels too.
    for size in [16, 20, 24, 32, 48, 64] {
        let file = match size {
            16 | 20 | 24 | 32 => format!("wrecktangle-tray-{size}.svg"),
            _ => "wrecktangle-tray.svg".to_string(),
        };
        let mut pixmap = draw(&file, size);
        write(
            &format!("wrecktangle-tray-black-{size}.png"),
            &pixmap.encode_png().expect("PNG encoding"),
        );
        // Premultiplied, so white is each channel equal to the alpha.
        for pixel in pixmap.data_mut().as_chunks_mut::<4>().0 {
            let alpha = pixel[3];
            *pixel = [alpha, alpha, alpha, alpha];
        }
        write(
            &format!("wrecktangle-tray-white-{size}.png"),
            &pixmap.encode_png().expect("PNG encoding"),
        );
    }
}
