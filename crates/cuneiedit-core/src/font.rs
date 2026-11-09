//! Minimal TrueType reader: glyph lookup and metrics, for embedding fonts in PDFs.
//!
//! Font files come from the user, so every read is bounds-checked; a damaged font is
//! rejected (`None`) instead of causing a panic.

/// A TrueType font (with `glyf` outlines) from a font file or collection.
pub struct TrueType {
    /// The font as a standalone font file (extracted if it came from a collection).
    data: Vec<u8>,
    units_per_em: u16,
    ascent: i16,
    descent: i16,
    bbox: [i16; 4],
    num_glyphs: u16,
    hmtx: Table,
    num_h_metrics: u16,
    cmap: CharMap,
}

#[derive(Clone, Copy)]
struct Table {
    offset: usize,
    len: usize,
}

#[derive(Clone, Copy)]
enum CharMap {
    /// Segments of the BMP; offset of the subtable.
    Format4(usize),
    /// Groups of code points; offset of the subtable and number of groups.
    Format12(usize, usize),
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    let bytes = data.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn i16_at(data: &[u8], at: usize) -> Option<i16> {
    u16_at(data, at).map(|v| v as i16)
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Tables of the font starting at `base`: (tag, offset, length), checked against `data`.
fn tables(data: &[u8], base: usize) -> Option<Vec<([u8; 4], Table)>> {
    let version = u32_at(data, base)?;
    // TrueType outlines only; CFF fonts (`OTTO`) cannot be embedded as FontFile2.
    if version != 0x0001_0000 && version != u32::from_be_bytes(*b"true") {
        return None;
    }
    let count = usize::from(u16_at(data, base + 4)?);
    let mut tables = Vec::with_capacity(count);
    for i in 0..count {
        let record = base + 12 + 16 * i;
        let tag: [u8; 4] = data.get(record..record + 4)?.try_into().ok()?;
        let offset = u32_at(data, record + 8)? as usize;
        let len = u32_at(data, record + 12)? as usize;
        if offset.checked_add(len)? > data.len() {
            return None;
        }
        tables.push((tag, Table { offset, len }));
    }
    Some(tables)
}

/// A standalone font file containing `tables` of `data`.
fn standalone(data: &[u8], tables: &[([u8; 4], Table)]) -> Vec<u8> {
    let mut tables = tables.to_vec();
    tables.sort_by_key(|(tag, _)| *tag);
    let count = tables.len() as u16;
    let power = count.max(1).ilog2();
    let search_range = (1u16 << power) * 16;
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    for value in [count, search_range, power as u16, count * 16 - search_range] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, table) in &tables {
        let bytes = &data[table.offset..table.offset + table.len];
        let mut checksum = 0u32;
        for chunk in bytes.chunks(4) {
            let mut word = [0u8; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            checksum = checksum.wrapping_add(u32::from_be_bytes(word));
        }
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(table.len as u32).to_be_bytes());
        body.extend_from_slice(bytes);
        let padded = table.len.next_multiple_of(4);
        body.resize(body.len() + padded - table.len, 0);
        offset += padded;
    }
    out.extend_from_slice(&body);
    out
}

impl TrueType {
    /// Parses font `index` of a font file or collection; `None` if it is not a usable
    /// TrueType font.
    pub fn parse(data: &[u8], index: u32) -> Option<Self> {
        let collection = data.starts_with(b"ttcf");
        let base = if collection {
            let count = u32_at(data, 8)?;
            if index >= count {
                return None;
            }
            u32_at(data, 12 + 4 * index as usize)? as usize
        } else {
            0
        };
        let tables = tables(data, base)?;
        let find = |name: &[u8; 4]| {
            tables
                .iter()
                .find(|(tag, _)| tag == name)
                .map(|(_, table)| *table)
        };
        for required in [b"loca", b"glyf"] {
            find(required)?;
        }
        let head = find(b"head")?;
        let hhea = find(b"hhea")?;
        let maxp = find(b"maxp")?;
        let hmtx = find(b"hmtx")?;
        let cmap = find(b"cmap")?;
        let table = |table: Table| &data[table.offset..table.offset + table.len];

        let head_data = table(head);
        let units_per_em = u16_at(head_data, 18).filter(|&u| u >= 16)?;
        let bbox = [
            i16_at(head_data, 36)?,
            i16_at(head_data, 38)?,
            i16_at(head_data, 40)?,
            i16_at(head_data, 42)?,
        ];
        let hhea_data = table(hhea);
        let num_h_metrics = u16_at(hhea_data, 34).filter(|&n| n > 0)?;
        if usize::from(num_h_metrics) * 4 > hmtx.len {
            return None;
        }
        let num_glyphs = u16_at(table(maxp), 4)?;
        let cmap = Self::char_map(table(cmap))?;

        let mut font = Self {
            data: Vec::new(),
            units_per_em,
            ascent: i16_at(hhea_data, 4)?,
            descent: i16_at(hhea_data, 6)?,
            bbox,
            num_glyphs,
            hmtx,
            num_h_metrics,
            cmap: match cmap {
                CharMap::Format4(at) => CharMap::Format4(cmap_offset(&tables, at)?),
                CharMap::Format12(at, n) => CharMap::Format12(cmap_offset(&tables, at)?, n),
            },
        };
        if collection {
            let rebuilt = standalone(data, &tables);
            // Offsets now refer to the rebuilt file.
            let fresh = self::tables(&rebuilt, 0)?;
            let find_fresh = |name: &[u8; 4]| {
                fresh
                    .iter()
                    .find(|(tag, _)| tag == name)
                    .map(|(_, table)| *table)
            };
            font.hmtx = find_fresh(b"hmtx")?;
            let new_cmap = find_fresh(b"cmap")?;
            font.cmap = match Self::char_map(&rebuilt[new_cmap.offset..][..new_cmap.len])? {
                CharMap::Format4(at) => CharMap::Format4(new_cmap.offset + at),
                CharMap::Format12(at, n) => CharMap::Format12(new_cmap.offset + at, n),
            };
            font.data = rebuilt;
        } else {
            font.data = data.to_vec();
        }
        Some(font)
    }

    /// The best Unicode subtable of a `cmap` table; offsets are relative to the table.
    fn char_map(cmap: &[u8]) -> Option<CharMap> {
        let count = usize::from(u16_at(cmap, 2)?);
        let mut best: Option<(u8, CharMap)> = None;
        for i in 0..count {
            let record = 4 + 8 * i;
            let platform = u16_at(cmap, record)?;
            let encoding = u16_at(cmap, record + 2)?;
            let offset = u32_at(cmap, record + 4)? as usize;
            let unicode = platform == 0 || (platform == 3 && matches!(encoding, 1 | 10));
            if !unicode {
                continue;
            }
            let (rank, map) = match u16_at(cmap, offset) {
                Some(12) => {
                    let groups = u32_at(cmap, offset + 12)? as usize;
                    if offset + 16 + groups.checked_mul(12)? > cmap.len() {
                        continue;
                    }
                    (2, CharMap::Format12(offset, groups))
                }
                Some(4) => {
                    let seg_count = usize::from(u16_at(cmap, offset + 6)? / 2);
                    if offset + 16 + seg_count * 8 > cmap.len() {
                        continue;
                    }
                    (1, CharMap::Format4(offset))
                }
                _ => continue,
            };
            if best.is_none_or(|(r, _)| rank > r) {
                best = Some((rank, map));
            }
        }
        best.map(|(_, map)| map)
    }

    /// The standalone font file, for embedding.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    pub fn ascent(&self) -> i16 {
        self.ascent
    }

    pub fn descent(&self) -> i16 {
        self.descent
    }

    /// `[x_min, y_min, x_max, y_max]` in font units.
    pub fn bbox(&self) -> [i16; 4] {
        self.bbox
    }

    /// The glyph for `c`, if the font has one.
    pub fn glyph(&self, c: char) -> Option<u16> {
        let code = u32::from(c);
        let glyph = match self.cmap {
            CharMap::Format4(at) => self.format4(at, code)?,
            CharMap::Format12(at, groups) => self.format12(at, groups, code)?,
        };
        (glyph != 0 && glyph < self.num_glyphs).then_some(glyph)
    }

    fn format4(&self, at: usize, code: u32) -> Option<u16> {
        let code = u16::try_from(code).ok()?;
        let data = &self.data;
        let seg_x2 = usize::from(u16_at(data, at + 6)?);
        let seg_count = seg_x2 / 2;
        let ends = at + 14;
        let starts = ends + seg_x2 + 2;
        let deltas = starts + seg_x2;
        let ranges = deltas + seg_x2;
        // Segments are sorted by end code: binary search for the first end >= code.
        let (mut low, mut high) = (0, seg_count);
        while low < high {
            let mid = (low + high) / 2;
            if u16_at(data, ends + 2 * mid)? < code {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        if low == seg_count {
            return None;
        }
        let start = u16_at(data, starts + 2 * low)?;
        if code < start {
            return None;
        }
        let delta = u16_at(data, deltas + 2 * low)?;
        let range_at = ranges + 2 * low;
        let range = usize::from(u16_at(data, range_at)?);
        if range == 0 {
            return Some(code.wrapping_add(delta));
        }
        let glyph = u16_at(data, range_at + range + 2 * usize::from(code - start))?;
        (glyph != 0).then(|| glyph.wrapping_add(delta))
    }

    fn format12(&self, at: usize, groups: usize, code: u32) -> Option<u16> {
        let data = &self.data;
        let (mut low, mut high) = (0, groups);
        while low < high {
            let mid = (low + high) / 2;
            let group = at + 16 + 12 * mid;
            if u32_at(data, group + 4)? < code {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        let group = at + 16 + 12 * low;
        if low == groups || u32_at(data, group)? > code {
            return None;
        }
        let glyph = u32_at(data, group + 8)?.checked_add(code - u32_at(data, group)?)?;
        u16::try_from(glyph).ok()
    }

    /// Advance width of `glyph` in font units.
    pub fn advance(&self, glyph: u16) -> u16 {
        let index = glyph.min(self.num_h_metrics - 1);
        u16_at(&self.data, self.hmtx.offset + 4 * usize::from(index)).unwrap_or(0)
    }
}

/// Absolute offset of a `cmap` subtable at `at` within the `cmap` table.
fn cmap_offset(tables: &[([u8; 4], Table)], at: usize) -> Option<usize> {
    tables
        .iter()
        .find(|(tag, _)| tag == b"cmap")
        .map(|(_, table)| table.offset + at)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A tiny TrueType font: glyph 1 is `A` (width 600), glyph 2 is `€` (width 700) and
    /// glyph 3 is U+1F600 (width 1000, only in the format 12 character map).
    pub(crate) fn test_font() -> Vec<u8> {
        test_font_with(true)
    }

    fn test_font_with(format12: bool) -> Vec<u8> {
        let be16 = |v: u16| v.to_be_bytes();
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&be16(1000));
        head[38..40].copy_from_slice(&(-200i16).to_be_bytes());
        head[40..42].copy_from_slice(&be16(1000));
        head[42..44].copy_from_slice(&be16(800));
        let mut hhea = vec![0u8; 36];
        hhea[4..6].copy_from_slice(&be16(800));
        hhea[6..8].copy_from_slice(&(-200i16).to_be_bytes());
        hhea[34..36].copy_from_slice(&be16(4));
        let mut maxp = vec![0u8; 6];
        maxp[4..6].copy_from_slice(&be16(4));
        let hmtx: Vec<u8> = [500u16, 600, 700, 1000]
            .iter()
            .flat_map(|w| [be16(*w), be16(0)].concat())
            .collect();

        // Format 4: 'A' -> 1, '€' (U+20AC) -> 2 via the glyph array, end segment 0xFFFF.
        let mut f4 = Vec::new();
        let seg_count = 3u16;
        f4.extend(be16(4));
        f4.extend(be16(16 + 8 * seg_count + 2));
        f4.extend(be16(0));
        f4.extend(be16(seg_count * 2));
        f4.extend([0u8; 6]);
        for end in [0x41, 0x20AC, 0xFFFF] {
            f4.extend(be16(end));
        }
        f4.extend(be16(0));
        for start in [0x41, 0x20AC, 0xFFFF] {
            f4.extend(be16(start));
        }
        for delta in [1u16.wrapping_sub(0x41), 0, 1] {
            f4.extend(be16(delta));
        }
        // Second segment: range offset pointing just past the range offsets array.
        for range in [0, 4, 0] {
            f4.extend(be16(range));
        }
        f4.extend(be16(2));

        let groups = [
            [0x41u32, 0x41, 1],
            [0x20AC, 0x20AC, 2],
            [0x1F600, 0x1F600, 3],
        ];
        let mut f12 = Vec::new();
        f12.extend(be16(12));
        f12.extend(be16(0));
        f12.extend((16 + 12 * groups.len() as u32).to_be_bytes());
        f12.extend(0u32.to_be_bytes());
        f12.extend((groups.len() as u32).to_be_bytes());
        for value in groups.iter().flatten() {
            f12.extend(value.to_be_bytes());
        }

        let mut cmap = Vec::new();
        cmap.extend(be16(0));
        cmap.extend(be16(if format12 { 2 } else { 1 }));
        cmap.extend(be16(3));
        cmap.extend(be16(1));
        cmap.extend(20u32.to_be_bytes());
        cmap.extend(be16(if format12 { 3 } else { 9 }));
        cmap.extend(be16(10));
        cmap.extend((20 + f4.len() as u32).to_be_bytes());
        cmap.extend(&f4);
        cmap.extend(&f12);

        let tables: Vec<([u8; 4], Vec<u8>)> = vec![
            (*b"cmap", cmap),
            (*b"glyf", vec![0; 4]),
            (*b"head", head),
            (*b"hhea", hhea),
            (*b"hmtx", hmtx),
            (*b"loca", vec![0; 10]),
            (*b"maxp", maxp),
        ];
        let mut data = vec![0u8; 12 + 16 * tables.len()];
        data[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        data[4..6].copy_from_slice(&be16(tables.len() as u16));
        for (i, (tag, bytes)) in tables.iter().enumerate() {
            let record = 12 + 16 * i;
            let offset = data.len() as u32;
            data[record..record + 4].copy_from_slice(tag);
            data[record + 8..record + 12].copy_from_slice(&offset.to_be_bytes());
            data[record + 12..record + 16].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
            data.extend(bytes);
            data.resize(data.len().next_multiple_of(4), 0);
        }
        data
    }

    #[test]
    fn reads_glyphs_and_metrics() {
        let font = TrueType::parse(&test_font(), 0).unwrap();
        assert_eq!(font.units_per_em(), 1000);
        assert_eq!((font.ascent(), font.descent()), (800, -200));
        assert_eq!(font.glyph('A'), Some(1));
        assert_eq!(font.glyph('€'), Some(2));
        assert_eq!(font.glyph('😀'), Some(3), "format 12 is preferred");
        assert_eq!(font.glyph('B'), None);
        assert_eq!(font.advance(2), 700);
        assert_eq!(font.advance(3), 1000);

        let bmp_only = TrueType::parse(&test_font_with(false), 0).unwrap();
        assert_eq!(bmp_only.glyph('A'), Some(1));
        assert_eq!(bmp_only.glyph('€'), Some(2));
        assert_eq!(bmp_only.glyph('😀'), None);
        assert_eq!(bmp_only.glyph('\u{FFFF}'), None);
    }

    #[test]
    fn reads_fonts_from_collections() {
        let single = test_font();
        let mut collection = b"ttcf".to_vec();
        collection.extend(0x0001_0000u32.to_be_bytes());
        collection.extend(1u32.to_be_bytes());
        collection.extend(16u32.to_be_bytes());
        // Table offsets in a collection are relative to the collection file.
        let mut font = single.clone();
        let count = usize::from(u16_at(&font, 4).unwrap());
        for i in 0..count {
            let at = 12 + 16 * i + 8;
            let offset = u32_at(&font, at).unwrap() + 16;
            font[at..at + 4].copy_from_slice(&offset.to_be_bytes());
        }
        collection.extend(font);
        let font = TrueType::parse(&collection, 0).unwrap();
        assert_eq!(font.glyph('€'), Some(2));
        assert!(font.data().starts_with(&0x0001_0000u32.to_be_bytes()));
        assert!(TrueType::parse(font.data(), 0).is_some());
        assert!(TrueType::parse(&collection, 1).is_none());
    }

    #[test]
    fn rejects_damaged_fonts_without_panicking() {
        let font = test_font();
        for len in 0..font.len() {
            if let Some(parsed) = TrueType::parse(&font[..len], 0) {
                for c in ['A', '€', '😀', 'x', '\u{FFFF}'] {
                    if let Some(glyph) = parsed.glyph(c) {
                        parsed.advance(glyph);
                    }
                }
            }
        }
        let mut garbage = font.clone();
        for (i, byte) in garbage.iter_mut().enumerate().skip(12) {
            *byte = (i * 37) as u8;
        }
        if let Some(parsed) = TrueType::parse(&garbage, 0) {
            parsed.glyph('A');
        }
        assert!(TrueType::parse(b"OTTO\0\0\0\0", 0).is_none(), "CFF");
    }
}
