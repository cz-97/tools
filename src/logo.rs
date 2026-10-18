//! The application's mark: an art-deco "T" in white on a black plate.
//!
//! The mark is drawn here rather than shipped as an image so that the tray
//! icon, the window icon and the executable's own icon all come from one
//! description, at whatever size the caller needs, with no image decoder in the
//! binary. `build.rs` includes this file to write the `.ico` that resource
//! compiler embeds.

/// The squares the executable's icon carries, in pixels.
pub const ICO_SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 128];

/// The colour of the plate the letter sits on.
const PLATE: [f32; 3] = [11., 11., 12.];
/// The colour of the letter.
const LETTER: [f32; 3] = [255., 255., 255.];
/// The corner radius of the plate, as a fraction of the mark's width.
const PLATE_RADIUS: f32 = 0.2;

/// Samples taken per axis in each pixel. Four is enough for the diagonals to
/// read as edges rather than as stairs, at every size the icon ships in.
const SAMPLES: u32 = 4;

/// The bar of the letter: square at the left, bevelled at the outer top corner
/// so the arm ends in a diagonal rather than a block.
const BAR: [(f32, f32); 5] = [
    (0.150, 0.250),
    (0.735, 0.250),
    (0.850, 0.365),
    (0.850, 0.415),
    (0.150, 0.415),
];

/// The stem: joined square to the bar, bevelled at the foot, mirroring the bar.
const STEM: [(f32, f32); 5] = [
    (0.405, 0.415),
    (0.585, 0.415),
    (0.585, 0.820),
    (0.470, 0.820),
    (0.405, 0.735),
];

/// The mark as straight-alpha RGBA, row by row from the top.
pub fn rgba(size: u32) -> Vec<u8> {
    let mut pixels = vec![0; (size as usize) * (size as usize) * 4];
    if size == 0 {
        return pixels;
    }
    let samples = (SAMPLES * SAMPLES) as f32;
    let edge = size as f32;

    for y in 0..size {
        for x in 0..size {
            let mut plate = 0u32;
            let mut letter = 0u32;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    // Sample at the centre of each sub-cell.
                    let u = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / edge;
                    let v = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / edge;
                    if !inside_plate(u, v) {
                        continue;
                    }
                    plate += 1;
                    if inside(&BAR, u, v) || inside(&STEM, u, v) {
                        letter += 1;
                    }
                }
            }
            if plate == 0 {
                continue;
            }
            // The plate is opaque where it covers, and the letter is opaque on
            // top of it, so the stored colour is the coverage-weighted mix.
            let alpha = plate as f32 / samples;
            let ink = letter as f32 / samples;
            let base = alpha - ink;
            let at = ((y as usize * size as usize) + x as usize) * 4;
            for channel in 0..3 {
                let colour = (LETTER[channel] * ink + PLATE[channel] * base) / alpha;
                pixels[at + channel] = colour.round().clamp(0.0, 255.0) as u8;
            }
            pixels[at + 3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    pixels
}

/// Whether a point in unit coordinates falls inside the rounded plate.
fn inside_plate(u: f32, v: f32) -> bool {
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return false;
    }
    // Clamping to the inner rect turns the test into "within the radius of the
    // nearest corner centre", which also covers the straight edges.
    let cu = u.clamp(PLATE_RADIUS, 1.0 - PLATE_RADIUS);
    let cv = v.clamp(PLATE_RADIUS, 1.0 - PLATE_RADIUS);
    let du = u - cu;
    let dv = v - cv;
    du * du + dv * dv <= PLATE_RADIUS * PLATE_RADIUS
}

/// Whether a point falls inside a convex polygon, by the crossing count.
fn inside(polygon: &[(f32, f32)], u: f32, v: f32) -> bool {
    let mut inside = false;
    let mut previous = polygon.len() - 1;
    for current in 0..polygon.len() {
        let (cu, cv) = polygon[current];
        let (pu, pv) = polygon[previous];
        if (cv > v) != (pv > v) && u < (pu - cu) * (v - cv) / (pv - cv) + cu {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

/// The mark packed as a multi-size Windows icon file.
pub fn ico() -> Vec<u8> {
    let images: Vec<(u32, Vec<u8>)> = ICO_SIZES.iter().map(|&size| (size, dib(size))).collect();

    let mut icon = Vec::new();
    icon.extend_from_slice(&0u16.to_le_bytes()); // reserved
    icon.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    icon.extend_from_slice(&(images.len() as u16).to_le_bytes());

    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in &images {
        // A stored 256 would not fit the byte, and is written as zero.
        let edge = if *size >= 256 { 0 } else { *size as u8 };
        icon.push(edge);
        icon.push(edge);
        icon.push(0); // palette entries
        icon.push(0); // reserved
        icon.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        icon.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        icon.extend_from_slice(&(data.len() as u32).to_le_bytes());
        icon.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &images {
        icon.extend_from_slice(data);
    }
    icon
}

/// One icon image as a 32-bit bottom-up DIB, the way an `.ico` stores it.
fn dib(size: u32) -> Vec<u8> {
    let rgba = rgba(size);
    let stride = mask_row(size);
    let mut image = Vec::with_capacity((40 + size * size * 4 + size * stride) as usize);

    image.extend_from_slice(&40u32.to_le_bytes()); // header size
    image.extend_from_slice(&(size as i32).to_le_bytes()); // width
    image.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // colour + mask
    image.extend_from_slice(&1u16.to_le_bytes()); // planes
    image.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    image.extend_from_slice(&0u32.to_le_bytes()); // no compression
    image.extend_from_slice(&(size * size * 4).to_le_bytes()); // image size
    image.extend_from_slice(&[0; 16]); // resolution, palette counts

    for y in (0..size).rev() {
        for x in 0..size {
            let at = ((y * size + x) * 4) as usize;
            image.extend_from_slice(&[rgba[at + 2], rgba[at + 1], rgba[at], rgba[at + 3]]);
        }
    }
    // The alpha channel carries the shape, so the mask only has to exist.
    image.extend(std::iter::repeat_n(0u8, (size * stride) as usize));
    image
}

/// The width of one AND-mask row: one bit per pixel, padded to four bytes.
fn mask_row(size: u32) -> u32 {
    size.div_ceil(32) * 4
}

#[cfg(test)]
mod tests {
    use super::{ICO_SIZES, dib, ico, rgba};

    #[test]
    fn the_letter_is_white_in_the_middle_of_the_mark() {
        let size = 32;
        let pixels = rgba(size);
        let at = ((size / 2 * size + size / 2) * 4) as usize;
        assert_eq!(&pixels[at..at + 3], &[255, 255, 255]);
        assert_eq!(pixels[at + 3], 255);
    }

    #[test]
    fn the_corners_of_the_plate_are_transparent() {
        let pixels = rgba(32);
        assert_eq!(pixels[3], 0);
        assert_eq!(pixels[pixels.len() - 1], 0);
    }

    #[test]
    fn the_plate_is_black_away_from_the_letter() {
        let size = 32;
        let pixels = rgba(size);
        // A point on the plate, left of the stem and below the bar.
        let at = ((28 * size + 6) * 4) as usize;
        assert_eq!(pixels[at + 3], 255);
        assert!(pixels[at] < 20, "plate is near black: {}", pixels[at]);
    }

    #[test]
    fn every_stored_size_has_the_bytes_an_icon_entry_promises() {
        for size in ICO_SIZES {
            let data = dib(size);
            let stride = size.div_ceil(32) * 4;
            assert_eq!(
                data.len() as u32,
                40 + size * size * 4 + size * stride,
                "size {size}"
            );
        }
    }

    #[test]
    fn the_icon_file_lists_every_size() {
        let icon = ico();
        // reserved, type, count, then one 16-byte entry per image
        assert_eq!(u16::from_le_bytes([icon[2], icon[3]]), 1);
        assert_eq!(
            u16::from_le_bytes([icon[4], icon[5]]),
            ICO_SIZES.len() as u16
        );
        let entries = ICO_SIZES.len() * 16;
        assert_eq!(icon[6], ICO_SIZES[0] as u8);
        assert!(icon.len() > 6 + entries);
    }
}
