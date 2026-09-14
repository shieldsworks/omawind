//! GRIB edition 2, read from scratch from the WMO's Manual on Codes
//! (FM 92 GRIB): the sections, Lambert conformal grids, product template
//! 4.0, simple packing (template 5.0) and bitmaps. That is everything NOAA
//! sends for HRRR. Any other template is refused by number, never guessed.

use crate::grid::{Grid, Lambert};
use crate::time;

/// The most points one field may have: HRRR's whole grid has 1.9 million.
const MAX_POINTS: usize = 4_000_000;
/// The most values one file may decode to, all its fields together.
const MAX_VALUES: usize = 16_000_000;

/// One field: a parameter at one level and one forecast time.
#[derive(Clone, Debug)]
pub struct Field {
    pub discipline: u8,
    pub category: u8,
    pub number: u8,
    /// The first fixed surface (code table 4.5), like 103 for a height
    /// above ground, and its value in meters or pascals.
    pub surface: u8,
    pub level: Option<f64>,
    /// The model run, Unix seconds.
    pub reference: i64,
    /// Seconds from the run to the time the field is valid.
    pub lead: i64,
    pub grid: Grid,
    /// Row by row from the grid's first point. NaN where the bitmap says
    /// there's no value.
    pub values: Vec<f32>,
}

impl Field {
    pub fn valid(&self) -> i64 {
        self.reference + self.lead
    }

    /// The parameter's NCEP abbreviation, for people.
    pub fn name(&self) -> String {
        match (self.discipline, self.category, self.number) {
            (0, 2, 2) => "UGRD".into(),
            (0, 2, 3) => "VGRD".into(),
            (0, 2, 22) => "GUST".into(),
            (0, 3, 1) => "PRMSL".into(),
            (0, 3, 198) => "MSLMA".into(),
            (d, c, n) => format!("{d}.{c}.{n}"),
        }
    }
}

fn be(b: &[u8]) -> u64 {
    b.iter().fold(0, |v, &x| v << 8 | u64::from(x))
}

/// GRIB's signed integers are sign and magnitude, not two's complement.
fn signed(b: &[u8]) -> i64 {
    let bits = b.len() as u32 * 8;
    let v = be(b);
    let magnitude = (v & ((1 << (bits - 1)) - 1)) as i64;
    if v >> (bits - 1) == 1 {
        -magnitude
    } else {
        magnitude
    }
}

/// Every field in `bytes`: one or more whole GRIB2 messages, back to back.
pub fn parse(bytes: &[u8]) -> Result<Vec<Field>, String> {
    let mut fields = Vec::new();
    let mut budget = MAX_VALUES;
    let mut pos = 0;
    let mut n = 0;
    while pos < bytes.len() {
        n += 1;
        let rest = &bytes[pos..];
        if rest.len() < 16 || &rest[..4] != b"GRIB" {
            return Err(format!("message {n}: not GRIB at byte {pos}"));
        }
        if rest[7] != 2 {
            return Err(format!("message {n}: GRIB edition {}, not 2", rest[7]));
        }
        let len = be(&rest[8..16]);
        if len < 16 + 4 || len > rest.len() as u64 {
            return Err(format!("message {n}: cut short"));
        }
        let len = len as usize;
        message(&rest[..len], &mut fields, &mut budget).map_err(|e| format!("message {n}: {e}"))?;
        pos += len;
    }
    Ok(fields)
}

struct Product {
    category: u8,
    number: u8,
    surface: u8,
    level: Option<f64>,
    lead: i64,
}

struct Packing {
    count: usize,
    reference: f32,
    binary: i32,
    decimal: i32,
    bits: u32,
}

fn message(m: &[u8], out: &mut Vec<Field>, budget: &mut usize) -> Result<(), String> {
    let discipline = m[6];
    let mut pos = 16;
    let mut reference = None;
    let mut grid: Option<Grid> = None;
    let mut product: Option<Product> = None;
    let mut packing: Option<Packing> = None;
    // The bitmap for the field being read, and the last one defined, which
    // indicator 254 reuses.
    let mut bitmap: Option<Option<Vec<bool>>> = None;
    let mut defined: Option<Vec<bool>> = None;
    loop {
        let rest = &m[pos..];
        if rest == b"7777" {
            return Ok(());
        }
        if rest.len() < 5 {
            return Err("no end section".into());
        }
        let len = be(&rest[..4]) as usize;
        if len < 5 || len > rest.len() {
            return Err(format!("section at byte {pos} runs past the message"));
        }
        let s = &rest[..len];
        let need = |n: usize| {
            if len < n {
                Err(format!("section {} is too short", s[4]))
            } else {
                Ok(())
            }
        };
        match s[4] {
            1 => {
                need(19)?;
                let (y, mo, d, h, mi, sec) = (be(&s[12..14]), s[14], s[15], s[16], s[17], s[18]);
                let text = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{sec:02}Z");
                reference = Some(time::parse_iso(&text).ok_or("impossible reference time")?);
            }
            2 => {}
            3 => {
                grid = Some(grid_section(s)?);
                // A bitmap describes the grid before it, never a new one.
                bitmap = None;
            }
            4 => product = Some(product_section(s)?),
            5 => packing = Some(packing_section(s)?),
            6 => {
                need(6)?;
                let points = grid.as_ref().ok_or("bitmap before grid")?.len();
                bitmap = Some(match s[5] {
                    255 => None,
                    0 => {
                        let bits = &s[6..];
                        if bits.len() * 8 < points {
                            return Err("bitmap too short for the grid".into());
                        }
                        let map: Vec<bool> = (0..points)
                            .map(|i| bits[i / 8] >> (7 - i % 8) & 1 == 1)
                            .collect();
                        defined = Some(map.clone());
                        Some(map)
                    }
                    254 => {
                        let map = defined.clone().ok_or("reuses a bitmap never defined")?;
                        if map.len() != points {
                            return Err("reuses a bitmap made for another grid".into());
                        }
                        Some(map)
                    }
                    other => return Err(format!("predefined bitmap {other} isn't supported")),
                });
            }
            7 => {
                let (Some(reference), Some(g), Some(p), Some(k), Some(map)) = (
                    reference,
                    grid.as_ref(),
                    product.take(),
                    packing.take(),
                    bitmap.take(),
                ) else {
                    return Err(
                        "data before its identification, grid, product, packing or bitmap".into(),
                    );
                };
                if map.as_ref().is_some_and(|m| m.len() != g.len()) {
                    return Err("bitmap doesn't fit the grid".into());
                }
                *budget = budget
                    .checked_sub(g.len())
                    .ok_or("too many values to decode in one file")?;
                let values = unpack(&s[5..], &k, g.len(), map.as_deref())?;
                out.push(Field {
                    discipline,
                    category: p.category,
                    number: p.number,
                    surface: p.surface,
                    level: p.level,
                    reference,
                    lead: p.lead,
                    grid: g.clone(),
                    values,
                });
            }
            n => return Err(format!("unknown section {n}")),
        }
        pos += len;
    }
}

fn grid_section(s: &[u8]) -> Result<Grid, String> {
    if s.len() < 14 {
        return Err("section 3 is too short".into());
    }
    if s[5] != 0 || s[10] != 0 {
        return Err("grids defined by a table or with a list of points aren't supported".into());
    }
    let template = be(&s[12..14]);
    if template != 30 {
        return Err(format!(
            "grid template 3.{template} isn't supported (only Lambert conformal, 3.30)"
        ));
    }
    if s.len() < 81 {
        return Err("section 3 is too short for template 3.30".into());
    }
    let radius = match s[14] {
        0 => 6_367_470.0,
        6 => 6_371_229.0,
        1 => {
            let scale = signed(&s[15..16]) as i32;
            be(&s[16..20]) as f64 / 10f64.powi(scale)
        }
        other => {
            return Err(format!(
                "earth shape {other} isn't supported (only spheres)"
            ));
        }
    };
    let micro = |b: &[u8]| signed(b) as f64 * 1e-6;
    let lon = |b: &[u8]| {
        let d = be(b) as f64 * 1e-6;
        if d > 180.0 { d - 360.0 } else { d }
    };
    let (flags, center, scan) = (s[46], s[63], s[64]);
    if center != 0 {
        return Err("only Lambert grids centered on the north pole are supported".into());
    }
    // Rows may run north or south; anything but plain rows of i is refused.
    if scan & !0x40 != 0 {
        return Err(format!("scanning mode {scan:#04x} isn't supported"));
    }
    let points = be(&s[6..10]) as usize;
    let (nx, ny) = (be(&s[30..34]) as usize, be(&s[34..38]) as usize);
    if nx.checked_mul(ny).is_none_or(|n| n > MAX_POINTS) {
        return Err(format!("a {nx}×{ny} grid has too many points"));
    }
    let grid = Grid::lambert(Lambert {
        nx,
        ny,
        la1: micro(&s[38..42]),
        lo1: lon(&s[42..46]),
        lad: micro(&s[47..51]),
        lov: lon(&s[51..55]),
        dx: be(&s[55..59]) as f64 * 1e-3,
        dy: be(&s[59..63]) as f64 * 1e-3,
        latin1: micro(&s[65..69]),
        latin2: micro(&s[69..73]),
        radius,
        winds_along_grid: flags & 0x08 != 0,
        rows_north: scan & 0x40 != 0,
    })?;
    if grid.len() != points {
        return Err(format!(
            "grid is {}×{} but says it has {points} points",
            grid.nx, grid.ny
        ));
    }
    Ok(grid)
}

fn product_section(s: &[u8]) -> Result<Product, String> {
    if s.len() < 9 {
        return Err("section 4 is too short".into());
    }
    let template = be(&s[7..9]);
    if template != 0 {
        return Err(format!(
            "product template 4.{template} isn't supported (only 4.0)"
        ));
    }
    if s.len() < 34 {
        return Err("section 4 is too short for template 4.0".into());
    }
    let unit = match s[17] {
        0 => 60,
        1 => time::HOUR,
        2 => 24 * time::HOUR,
        10 => 3 * time::HOUR,
        11 => 6 * time::HOUR,
        12 => 12 * time::HOUR,
        13 => 1,
        other => return Err(format!("time unit {other} isn't supported")),
    };
    let scaled = be(&s[24..28]);
    let level = (s[23] != 0xff && scaled != 0xffff_ffff)
        .then(|| scaled as f64 / 10f64.powi(signed(&s[23..24]) as i32));
    Ok(Product {
        category: s[9],
        number: s[10],
        surface: s[22],
        level,
        lead: signed(&s[18..22]) * unit,
    })
}

fn packing_section(s: &[u8]) -> Result<Packing, String> {
    if s.len() < 11 {
        return Err("section 5 is too short".into());
    }
    let template = be(&s[9..11]);
    if template != 0 {
        return Err(format!(
            "packing template 5.{template} isn't supported (only simple packing, 5.0)"
        ));
    }
    if s.len() < 21 {
        return Err("section 5 is too short for template 5.0".into());
    }
    let bits = u32::from(s[19]);
    if bits > 32 {
        return Err(format!("{bits} bits per value isn't supported"));
    }
    Ok(Packing {
        count: be(&s[5..9]) as usize,
        reference: f32::from_bits(be(&s[11..15]) as u32),
        binary: signed(&s[15..17]) as i32,
        decimal: signed(&s[17..19]) as i32,
        bits,
    })
}

/// Simple packing: each value is `(R + X·2^E) / 10^D`, X being `bits`
/// wide, packed most significant bit first, one per point the bitmap keeps.
fn unpack(
    data: &[u8],
    k: &Packing,
    points: usize,
    bitmap: Option<&[bool]>,
) -> Result<Vec<f32>, String> {
    let present = bitmap.map_or(points, |m| m.iter().filter(|&&b| b).count());
    if k.count != present {
        return Err(format!(
            "packs {} values for {present} points with data",
            k.count
        ));
    }
    if !k.reference.is_finite() {
        return Err("reference value isn't a number".into());
    }
    let needed = (present as u64 * u64::from(k.bits)).div_ceil(8);
    if (data.len() as u64) < needed {
        return Err("data section too short".into());
    }
    let r = f64::from(k.reference);
    let e = 2f64.powi(k.binary);
    let d = 10f64.powi(-k.decimal);
    if !(e.is_finite() && d.is_finite()) {
        return Err("scale factors overflow".into());
    }
    let mut bit = 0u64;
    let mut next = || {
        if k.bits == 0 {
            return (r * d) as f32;
        }
        let byte = (bit / 8) as usize;
        let shift = (bit % 8) as u32;
        // Up to 32 bits starting mid-byte span at most five bytes.
        let mut word = 0u64;
        for i in 0..5 {
            word = word << 8 | u64::from(*data.get(byte + i).unwrap_or(&0));
        }
        let x = (word >> (40 - shift - k.bits)) & ((1u64 << k.bits) - 1);
        bit += u64::from(k.bits);
        ((r + x as f64 * e) * d) as f32
    };
    let values: Vec<f32> = match bitmap {
        None => (0..points).map(|_| next()).collect(),
        Some(map) => map
            .iter()
            .map(|&kept| if kept { next() } else { f32::NAN })
            .collect(),
    };
    // Only the bitmap's gaps may be missing: a value too big for f32 means
    // a broken file, not a gap.
    let kept = |i: usize| bitmap.is_none_or(|m| m[i]);
    if values
        .iter()
        .enumerate()
        .any(|(i, v)| kept(i) && !v.is_finite())
    {
        return Err("values overflow".into());
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sign_and_magnitude() {
        assert_eq!(signed(&[0x80, 0x04]), -4);
        assert_eq!(signed(&[0x00, 0x04]), 4);
        assert_eq!(signed(&[0x82, 0x2c, 0xb7, 0x2e]), -36_484_910);
        assert_eq!(signed(&[0x80]), 0);
    }

    fn packing(bits: u32, count: usize) -> Packing {
        Packing {
            count,
            reference: 1.5,
            binary: -1,
            decimal: 1,
            bits,
        }
    }

    #[test]
    fn unpacks_values_across_byte_boundaries() {
        // 3, 0, 7, 5 in three bits each: 011 000 111 101, then padding.
        let data = [0b0110_0011, 0b1101_0000];
        let v = unpack(&data, &packing(3, 4), 4, None).unwrap();
        // (1.5 + x/2) / 10
        assert_eq!(v, [0.3, 0.15, 0.5, 0.4]);
    }

    #[test]
    fn a_bitmap_leaves_gaps_and_zero_bits_mean_constant() {
        let data = [0b1110_0000];
        let v = unpack(&data, &packing(3, 1), 3, Some(&[false, true, false])).unwrap();
        assert!(v[0].is_nan() && v[2].is_nan());
        assert_eq!(v[1], 0.5);
        assert_eq!(unpack(&[], &packing(0, 2), 2, None).unwrap(), [0.15, 0.15]);
    }

    #[test]
    fn refuses_short_or_miscounted_data() {
        assert!(unpack(&[0xff], &packing(8, 2), 2, None).is_err());
        assert!(unpack(&[0xff, 0xff], &packing(8, 2), 3, None).is_err());
    }

    #[test]
    fn refuses_what_isnt_grib2() {
        assert!(parse(b"<!doctype html><html>Request for Future Data</html>").is_err());
        let mut edition1 = b"GRIB\0\0\0\x01".to_vec();
        edition1.extend([0; 12]);
        assert!(parse(&edition1).unwrap_err().contains("edition 1"));
    }

    // Messages built by hand: HRRR's grid cut to nx×ny, a 10 m wind field.
    fn section(number: u8, body: &[u8]) -> Vec<u8> {
        let mut s = ((body.len() + 5) as u32).to_be_bytes().to_vec();
        s.push(number);
        s.extend(body);
        s
    }

    fn identification() -> Vec<u8> {
        section(1, &[0, 7, 0, 0, 2, 1, 1, 0x07, 0xea, 9, 14, 3, 0, 0, 0, 1])
    }

    fn lambert(nx: u32, ny: u32) -> Vec<u8> {
        let mut b = vec![0];
        b.extend(nx.wrapping_mul(ny).to_be_bytes());
        b.extend([0, 0, 0, 30, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        for v in [nx, ny, 36_364_046, 236_364_977] {
            b.extend(v.to_be_bytes());
        }
        b.push(0x08);
        for v in [38_500_000u32, 262_500_000, 3_000_000, 3_000_000] {
            b.extend(v.to_be_bytes());
        }
        b.extend([0, 0x40]);
        for v in [38_500_000u32, 38_500_000, 0, 0] {
            b.extend(v.to_be_bytes());
        }
        section(3, &b)
    }

    fn product() -> Vec<u8> {
        section(
            4,
            &[
                0, 0, 0, 0, 2, 2, 2, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 103, 0, 0, 0, 0, 10, 255, 0, 0,
                0, 0, 0,
            ],
        )
    }

    fn simple(count: u32, r: f32, e: [u8; 2], d: [u8; 2], bits: u8) -> Vec<u8> {
        let mut b = count.to_be_bytes().to_vec();
        b.extend([0, 0]);
        b.extend(r.to_bits().to_be_bytes());
        b.extend(e);
        b.extend(d);
        b.extend([bits, 0]);
        section(5, &b)
    }

    fn bitmap(indicator: u8, bits: &[u8]) -> Vec<u8> {
        let mut b = vec![indicator];
        b.extend(bits);
        section(6, &b)
    }

    fn wrap(sections: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = sections.concat().into_iter().chain(*b"7777").collect();
        let mut m = b"GRIB\0\0\0\x02".to_vec();
        m.extend((16 + body.len() as u64).to_be_bytes());
        m.extend(body);
        m
    }

    #[test]
    fn decodes_a_message_built_by_hand() {
        let m = wrap(&[
            identification(),
            lambert(2, 2),
            product(),
            simple(4, 1.0, [0, 0], [0, 0], 8),
            bitmap(255, &[]),
            section(7, &[0, 1, 2, 3]),
        ]);
        let f = parse(&m).unwrap();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].values, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(
            (f[0].name().as_str(), f[0].level, f[0].lead),
            ("UGRD", Some(10.0), 3600)
        );
        assert_eq!(f[0].reference, time::unix(2026, 9, 14, 3, 0, 0));
    }

    #[test]
    fn refuses_a_grid_too_big_to_decode() {
        // A couple of hundred bytes claiming 4.3 billion points of one value.
        let m = wrap(&[
            identification(),
            lambert(65_535, 65_535),
            product(),
            simple(65_535 * 65_535, 1.0, [0, 0], [0, 0], 0),
            bitmap(255, &[]),
            section(7, &[]),
        ]);
        assert!(parse(&m).unwrap_err().contains("too many points"));
    }

    #[test]
    fn refuses_too_many_values_in_one_file() {
        let mut sections = vec![identification()];
        for _ in 0..5 {
            sections.extend([
                lambert(2000, 2000),
                product(),
                simple(4_000_000, 1.0, [0, 0], [0, 0], 0),
                bitmap(255, &[]),
                section(7, &[]),
            ]);
        }
        assert!(
            parse(&wrap(&sections))
                .unwrap_err()
                .contains("too many values")
        );
    }

    #[test]
    fn refuses_a_bitmap_reused_for_another_grid() {
        let m = wrap(&[
            identification(),
            lambert(2, 2),
            product(),
            simple(4, 1.0, [0, 0], [0, 0], 8),
            bitmap(0, &[0xf0]),
            section(7, &[0, 1, 2, 3]),
            lambert(3, 3),
            product(),
            simple(4, 1.0, [0, 0], [0, 0], 8),
            bitmap(254, &[]),
            section(7, &[0, 1, 2, 3]),
        ]);
        assert!(parse(&m).unwrap_err().contains("another grid"));
    }

    #[test]
    fn refuses_a_grid_changed_after_its_bitmap() {
        let m = wrap(&[
            identification(),
            lambert(3, 3),
            product(),
            simple(9, 1.0, [0, 0], [0, 0], 8),
            bitmap(0, &[0xff, 0x80]),
            lambert(2, 2),
            section(7, &[0; 9]),
        ]);
        assert!(parse(&m).unwrap_err().contains("bitmap"));
    }

    #[test]
    fn refuses_values_that_overflow() {
        // D = -32767: every value times 10^32767.
        let m = wrap(&[
            identification(),
            lambert(2, 2),
            product(),
            simple(4, 1.0, [0, 0], [0xff, 0xff], 0),
            bitmap(255, &[]),
            section(7, &[]),
        ]);
        assert!(parse(&m).unwrap_err().contains("overflow"));
    }
}
