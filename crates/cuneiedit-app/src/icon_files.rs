//! Icon files written at build time (macOS `.iconset` PNGs, the Windows `.exe` icon); used by
//! `build.rs` and `examples/app_icon.rs`, not by the app itself.

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ 0xEDB8_8320
            };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// An RGBA PNG with uncompressed ("stored") deflate blocks; small enough for icons.
pub fn png(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(rgba.len() + size as usize);
    for row in rgba.chunks(size as usize * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (index, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(index + 1 == blocks.len()));
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut header = Vec::new();
    header.extend_from_slice(&size.to_be_bytes());
    header.extend_from_slice(&size.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;

/// One entry of a compiled resource (`.res`) file with a numeric type and name.
fn resource(out: &mut Vec<u8>, kind: u16, id: u16, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    for value in [0xFFFF, kind, 0xFFFF, id] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes()); // data version
    out.extend_from_slice(&0x1030u16.to_le_bytes()); // moveable, pure, discardable
    out.extend_from_slice(&0x0409u16.to_le_bytes()); // English (United States)
    out.extend_from_slice(&[0; 8]); // version, characteristics
    out.extend_from_slice(data);
    out.resize(out.len().next_multiple_of(4), 0);
}

/// A compiled resource file (`.res`, as the MSVC linker reads it) with the application icon:
/// one PNG image per size, grouped as icon 1, which Windows shows for the `.exe`.
pub fn windows_res(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    // A `.res` file starts with an empty entry.
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    out.extend_from_slice(&[0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 0]);
    out.extend_from_slice(&[0; 16]);

    let mut group = Vec::new();
    for value in [0u16, 1, images.len() as u16] {
        group.extend_from_slice(&value.to_le_bytes());
    }
    for (index, (size, image)) in images.iter().enumerate() {
        let id = index as u16 + 1;
        resource(&mut out, RT_ICON, id, image);
        // Width and height 0 mean 256.
        let side = if *size >= 256 { 0 } else { *size as u8 };
        group.extend_from_slice(&[side, side, 0, 0]);
        group.extend_from_slice(&1u16.to_le_bytes());
        group.extend_from_slice(&32u16.to_le_bytes());
        group.extend_from_slice(&(image.len() as u32).to_le_bytes());
        group.extend_from_slice(&id.to_le_bytes());
    }
    resource(&mut out, RT_GROUP_ICON, 1, &group);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_pngs() {
        let image = png(2, &[255; 16]);
        assert!(image.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(
            image.ends_with(&[0xAE, 0x42, 0x60, 0x82]),
            "IEND with its CRC"
        );
    }

    #[test]
    fn writes_windows_icon_resources() {
        let res = windows_res(&[(16, vec![1, 2, 3]), (256, vec![4; 8])]);
        let u32_at = |at: usize| u32::from_le_bytes(res[at..at + 4].try_into().unwrap());
        let u16_at = |at: usize| u16::from_le_bytes(res[at..at + 2].try_into().unwrap());
        assert_eq!((u32_at(0), u32_at(4)), (0, 32), "empty first entry");
        // First icon: 3 bytes of data, padded to 4.
        assert_eq!((u32_at(32), u16_at(42), u16_at(46)), (3, RT_ICON, 1));
        assert_eq!(&res[64..67], &[1, 2, 3]);
        // Second icon, then the group with two entries.
        assert_eq!((u32_at(68), u16_at(78), u16_at(82)), (8, RT_ICON, 2));
        let group = 68 + 32 + 8;
        assert_eq!(
            (u32_at(group), u16_at(group + 10)),
            (6 + 2 * 14, RT_GROUP_ICON)
        );
        let entries = group + 32;
        assert_eq!(u16_at(entries + 4), 2, "image count");
        assert_eq!(res[entries + 6 + 14], 0, "256 is written as 0");
        assert_eq!(res.len() % 4, 0);
    }
}
