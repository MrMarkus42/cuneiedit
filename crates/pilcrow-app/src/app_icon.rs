//! The application icon, drawn in code: a white pilcrow on a blue-violet rounded square.

const SAMPLES: u32 = 4;

/// Whether the point is inside the rounded square `[lo, hi]²` with corner `radius`.
fn in_rounded_square(x: f32, y: f32, lo: f32, hi: f32, radius: f32) -> bool {
    let center = (lo + hi) / 2.0;
    let half = (hi - lo) / 2.0 - radius;
    let dx = ((x - center).abs() - half).max(0.0);
    let dy = ((y - center).abs() - half).max(0.0);
    dx * dx + dy * dy <= radius * radius
}

fn in_box(x: f32, y: f32, (x0, y0, x1, y1): (f32, f32, f32, f32)) -> bool {
    (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
}

fn in_pilcrow(x: f32, y: f32) -> bool {
    let (cx, cy, r) = (0.43, 0.39, 0.135);
    let bowl = x <= 0.5 && ((x - cx).powi(2) + (y - cy).powi(2) <= r * r);
    bowl || in_box(x, y, (0.43, 0.255, 0.5, 0.525))
        || in_box(x, y, (0.48, 0.255, 0.545, 0.78))
        || in_box(x, y, (0.6, 0.255, 0.665, 0.78))
        || in_box(x, y, (0.43, 0.255, 0.665, 0.315))
}

/// Color at a point of the unit square, or `None` outside the icon.
fn color_at(x: f32, y: f32) -> Option<[f32; 3]> {
    if !in_rounded_square(x, y, 0.1, 0.9, 0.18) {
        return None;
    }
    if in_pilcrow(x, y) {
        return Some([255.0; 3]);
    }
    let t = ((y - 0.1) / 0.8 * 0.8 + (x - 0.1) / 0.8 * 0.2).clamp(0.0, 1.0);
    let mix = |top: f32, bottom: f32| top + (bottom - top) * t;
    Some([mix(79.0, 124.0), mix(141.0, 77.0), mix(247.0, 232.0)])
}

/// The icon as `size` × `size` RGBA pixels (not premultiplied), anti-aliased.
pub fn rgba(size: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    let samples = (SAMPLES * SAMPLES) as f32;
    for py in 0..size {
        for px in 0..size {
            let mut sum = [0.0f32; 3];
            let mut covered = 0.0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let x = (px as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / size as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / size as f32;
                    if let Some(color) = color_at(x, y) {
                        for (total, channel) in sum.iter_mut().zip(color) {
                            *total += channel;
                        }
                        covered += 1.0;
                    }
                }
            }
            if covered > 0.0 {
                pixels.extend(sum.map(|total| (total / covered).round() as u8));
                pixels.push((covered / samples * 255.0).round() as u8);
            } else {
                pixels.extend([0; 4]);
            }
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_a_glyph_on_a_rounded_square() {
        let size = 64;
        let pixels = rgba(size);
        assert_eq!(pixels.len(), (size * size * 4) as usize);
        let at = |x: u32, y: u32| &pixels[((y * size + x) * 4) as usize..][..4];
        assert_eq!(at(0, 0)[3], 0, "corners are transparent");
        assert_eq!(at(32, 8)[3], 255, "inside the square");
        assert_eq!(at(33, 32), &[255, 255, 255, 255], "the stem is white");
    }
}
