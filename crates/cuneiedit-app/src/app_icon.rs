//! The application icon, drawn in code: three white cuneiform wedges (one lying, two standing)
//! on a blue-violet rounded square.

const SAMPLES: u32 = 4;

/// Whether the point is inside the rounded square `[lo, hi]²` with corner `radius`.
fn in_rounded_square(x: f32, y: f32, lo: f32, hi: f32, radius: f32) -> bool {
    let center = (lo + hi) / 2.0;
    let half = (hi - lo) / 2.0 - radius;
    let dx = ((x - center).abs() - half).max(0.0);
    let dy = ((y - center).abs() - half).max(0.0);
    dx * dx + dy * dy <= radius * radius
}

/// Whether `p` is inside the triangle `a`, `b`, `c`.
fn in_triangle(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let side = |u: (f32, f32), v: (f32, f32)| (v.0 - u.0) * (p.1 - u.1) - (v.1 - u.1) * (p.0 - u.0);
    let (d1, d2, d3) = (side(a, b), side(b, c), side(c, a));
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(negative && positive)
}

/// A wedge as pressed into clay: a triangular head at `head`, pointing along `dir`, and a
/// thin tail that tapers to a point `length` away.
struct Wedge {
    head: (f32, f32),
    dir: (f32, f32),
    head_width: f32,
    head_length: f32,
    tail_width: f32,
    length: f32,
}

impl Wedge {
    fn contains(&self, x: f32, y: f32) -> bool {
        let (hx, hy) = self.head;
        let (dx, dy) = self.dir;
        let across = |width: f32| (-dy * width / 2.0, dx * width / 2.0);
        let at = |along: f32, (ox, oy): (f32, f32)| (hx + dx * along + ox, hy + dy * along + oy);
        let (hw, tw) = (across(self.head_width), across(self.tail_width));
        let neg = |(a, b): (f32, f32)| (-a, -b);
        in_triangle(
            (x, y),
            at(0.0, hw),
            at(0.0, neg(hw)),
            at(self.head_length, (0.0, 0.0)),
        ) || in_triangle(
            (x, y),
            at(0.0, tw),
            at(0.0, neg(tw)),
            at(self.length, (0.0, 0.0)),
        )
    }
}

const WEDGES: [Wedge; 3] = [
    Wedge {
        head: (0.27, 0.34),
        dir: (1.0, 0.0),
        head_width: 0.22,
        head_length: 0.14,
        tail_width: 0.1,
        length: 0.46,
    },
    Wedge {
        head: (0.4, 0.47),
        dir: (0.0, 1.0),
        head_width: 0.18,
        head_length: 0.13,
        tail_width: 0.085,
        length: 0.33,
    },
    Wedge {
        head: (0.6, 0.47),
        dir: (0.0, 1.0),
        head_width: 0.18,
        head_length: 0.13,
        tail_width: 0.085,
        length: 0.33,
    },
];

/// Color at a point of the unit square, or `None` outside the icon.
fn color_at(x: f32, y: f32) -> Option<[f32; 3]> {
    if !in_rounded_square(x, y, 0.1, 0.9, 0.18) {
        return None;
    }
    if WEDGES.iter().any(|wedge| wedge.contains(x, y)) {
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
    fn draws_wedges_on_a_rounded_square() {
        let size = 64;
        let pixels = rgba(size);
        assert_eq!(pixels.len(), (size * size * 4) as usize);
        let at = |x: u32, y: u32| &pixels[((y * size + x) * 4) as usize..][..4];
        assert_eq!(at(0, 0)[3], 0, "corners are transparent");
        assert_eq!(at(32, 8)[3], 255, "inside the square");
        assert_eq!(
            at(25, 35),
            &[255, 255, 255, 255],
            "a standing wedge is white"
        );
        assert_ne!(at(32, 35), &[255, 255, 255, 255], "between the wedges");
    }
}
