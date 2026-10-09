pub const ACCENT_RGB: [u8; 3] = [255, 51, 85];

pub fn rgba(size: usize) -> Vec<u8> {
    const SS: usize = 4;
    let unit = size as f32 / 64.0;
    let radius = 14.0 * unit;
    let n = size as f32;
    let mut out = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let (mut inside, mut tri) = (0u32, 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let fx = x as f32 + (sx as f32 + 0.5) / SS as f32;
                    let fy = y as f32 + (sy as f32 + 0.5) / SS as f32;
                    let cx = fx.clamp(radius, n - radius);
                    let cy = fy.clamp(radius, n - radius);
                    if (fx - cx).powi(2) + (fy - cy).powi(2) <= radius * radius {
                        inside += 1;
                        if in_triangle(fx / unit, fy / unit) {
                            tri += 1;
                        }
                    }
                }
            }
            if inside == 0 {
                continue;
            }
            let t = tri as f32 / inside as f32;
            let mix = |c: u8| (c as f32 + (255.0 - c as f32) * t).round() as u8;
            let alpha = (255.0 * inside as f32 / (SS * SS) as f32).round() as u8;
            let i = (y * size + x) * 4;
            out[i..i + 4].copy_from_slice(&[mix(ACCENT_RGB[0]), mix(ACCENT_RGB[1]), mix(ACCENT_RGB[2]), alpha]);
        }
    }
    out
}

fn in_triangle(x: f32, y: f32) -> bool {
    x >= 24.0 && (y - 32.0).abs() <= (44.0 - x) * 0.58
}
