//! Regenerates the committed icon assets from `src/icon.rs`:
//! `cargo run --example icons`

#[allow(dead_code)]
#[path = "../src/icon.rs"]
mod icon;

use std::io::Cursor;
use std::path::Path;

fn png(size: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_raw(size, size, icon::rgba(size as usize)).expect("icon buffer size");
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).expect("encode png");
    out.into_inner()
}

fn icns() -> Vec<u8> {
    let entries: [(&[u8; 4], u32); 10] = [
        (b"icp4", 16),
        (b"icp5", 32),
        (b"ic11", 32),
        (b"ic12", 64),
        (b"ic07", 128),
        (b"ic13", 256),
        (b"ic08", 256),
        (b"ic14", 512),
        (b"ic09", 512),
        (b"ic10", 1024),
    ];
    let mut body = Vec::new();
    for (kind, size) in entries {
        let data = png(size);
        body.extend_from_slice(kind);
        body.extend_from_slice(&(data.len() as u32 + 8).to_be_bytes());
        body.extend_from_slice(&data);
    }
    let mut out = b"icns".to_vec();
    out.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
    out.extend_from_slice(&body);
    out
}

fn main() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(assets.join("icon.png"), png(256)).unwrap();
    std::fs::write(assets.join("fastyt.icns"), icns()).unwrap();
    println!("wrote {}", assets.display());
}
