#[allow(dead_code)]
#[path = "src/icon.rs"]
mod icon;

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let ico = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("fastyt.ico");
    write_ico(&ico, &[16, 24, 32, 48, 64, 128, 256]);
    winresource::WindowsResource::new()
        .set_icon(ico.to_str().unwrap())
        .set("FileDescription", "fast.yt")
        .set("ProductName", "fast.yt")
        .compile()
        .expect("embed Windows resources");
}

fn write_ico(path: &Path, sizes: &[usize]) {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&n| bmp_entry(n)).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&n, img) in sizes.iter().zip(&images) {
        let dim = if n >= 256 { 0 } else { n as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in &images {
        out.extend_from_slice(img);
    }
    std::fs::write(path, out).unwrap();
}

fn bmp_entry(n: usize) -> Vec<u8> {
    let px = icon::rgba(n);
    let mut out = Vec::new();
    for v in [40u32, n as u32, 2 * n as u32] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]);
    for y in (0..n).rev() {
        for x in 0..n {
            let i = (y * n + x) * 4;
            out.extend_from_slice(&[px[i + 2], px[i + 1], px[i], px[i + 3]]);
        }
    }
    let mask_row = n.div_ceil(32) * 4;
    out.extend(std::iter::repeat_n(0u8, mask_row * n));
    out
}
