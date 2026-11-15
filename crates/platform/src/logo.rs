//! 应用标志：黑底白字的花体「T」。
//!
//! 图形是算出来的，不随包附带图片：托盘图标、窗口图标、exe 自己的图标都来自这一份
//! 描述，需要多大就画多大，二进制里也不用带图片解码器。`build.rs` 会包含本文件，
//! 用它写出交给资源编译器的 `.ico`。

/// exe 图标里带的尺寸，单位像素。
pub const ICO_SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 128];

/// 深色版本：底板的颜色。
const PLATE_DARK: [f32; 3] = [11., 11., 12.];
/// 深色版本：字的颜色。
const LETTER_DARK: [f32; 3] = [255., 255., 255.];
/// 浅色版本：底板的颜色。
const PLATE_LIGHT: [f32; 3] = [244., 244., 246.];
/// 浅色版本：字的颜色。
const LETTER_LIGHT: [f32; 3] = [17., 17., 20.];

/// 底板圆角，按标志宽度的比例算。
const PLATE_RADIUS: f32 = 0.2;

/// 每个像素在两个方向上的采样数。4 已经够斜边看着像边、而不像台阶，所有尺寸都适用。
const SAMPLES: u32 = 4;

/// 字的横杠：左端是直角，外上方切一刀，让这一臂收成斜口而不是方块。
const BAR: [(f32, f32); 5] = [
    (0.150, 0.250),
    (0.735, 0.250),
    (0.850, 0.365),
    (0.850, 0.415),
    (0.150, 0.415),
];

/// 字的竖杆：与横杠平接，脚部切一刀，与横杠呼应。
const STEM: [(f32, f32); 5] = [
    (0.405, 0.415),
    (0.585, 0.415),
    (0.585, 0.820),
    (0.470, 0.820),
    (0.405, 0.735),
];

/// 深色版本，直通 alpha 的 RGBA，从顶行开始逐行排列。
pub fn rgba(size: u32) -> Vec<u8> {
    mark(size, PLATE_DARK, LETTER_DARK)
}

/// 托盘用的版本：深色主题配深底白字，浅色主题配浅底黑字。
///
/// 任务栏的底色跟着主题走，图标只有跟着反过来才在两个主题里都清楚。
pub fn tray_rgba(size: u32, dark: bool) -> Vec<u8> {
    if dark {
        mark(size, PLATE_DARK, LETTER_DARK)
    } else {
        mark(size, PLATE_LIGHT, LETTER_LIGHT)
    }
}

/// 按给定的底板与字色，把标志画成直通 alpha 的 RGBA。
fn mark(size: u32, plate_color: [f32; 3], letter_color: [f32; 3]) -> Vec<u8> {
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
                    // 取每个子格的中心采样。
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
            // 底板盖住的像素是不透明的，字压在底板之上，所以存的是按覆盖率加权后的颜色。
            let alpha = plate as f32 / samples;
            let ink = letter as f32 / samples;
            let base = alpha - ink;
            let at = ((y as usize * size as usize) + x as usize) * 4;
            for channel in 0..3 {
                let colour = (letter_color[channel] * ink + plate_color[channel] * base) / alpha;
                pixels[at + channel] = colour.round().clamp(0.0, 255.0) as u8;
            }
            pixels[at + 3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    pixels
}

/// 单位坐标下的点是否落在圆角底板内。
fn inside_plate(u: f32, v: f32) -> bool {
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return false;
    }
    // 先夹到内矩形，判定就变成「离最近的圆角圆心不超过半径」，直边也一并覆盖。
    let cu = u.clamp(PLATE_RADIUS, 1.0 - PLATE_RADIUS);
    let cv = v.clamp(PLATE_RADIUS, 1.0 - PLATE_RADIUS);
    let du = u - cu;
    let dv = v - cv;
    du * du + dv * dv <= PLATE_RADIUS * PLATE_RADIUS
}

/// 用交点计数法判断点是否落在凸多边形内。
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

/// 把标志打包成多尺寸的 Windows 图标文件。
pub fn ico() -> Vec<u8> {
    let images: Vec<(u32, Vec<u8>)> = ICO_SIZES.iter().map(|&size| (size, dib(size))).collect();

    let mut icon = Vec::new();
    icon.extend_from_slice(&0u16.to_le_bytes()); // 保留字节
    icon.extend_from_slice(&1u16.to_le_bytes()); // 类型：图标
    icon.extend_from_slice(&(images.len() as u16).to_le_bytes());

    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in &images {
        // 256 塞不进一个字节，按规范写成 0。
        let edge = if *size >= 256 { 0 } else { *size as u8 };
        icon.push(edge);
        icon.push(edge);
        icon.push(0); // 调色板项数
        icon.push(0); // 保留字节
        icon.extend_from_slice(&1u16.to_le_bytes()); // 色彩平面
        icon.extend_from_slice(&32u16.to_le_bytes()); // 每像素位数
        icon.extend_from_slice(&(data.len() as u32).to_le_bytes());
        icon.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &images {
        icon.extend_from_slice(data);
    }
    icon
}

/// 单个尺寸的图像，按 `.ico` 的存法编成 32 位自下而上的 DIB。
fn dib(size: u32) -> Vec<u8> {
    let rgba = rgba(size);
    let stride = mask_row(size);
    let mut image = Vec::with_capacity((40 + size * size * 4 + size * stride) as usize);

    image.extend_from_slice(&40u32.to_le_bytes()); // 头长度
    image.extend_from_slice(&(size as i32).to_le_bytes()); // 宽
    image.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // 彩色 + 掩码
    image.extend_from_slice(&1u16.to_le_bytes()); // 平面数
    image.extend_from_slice(&32u16.to_le_bytes()); // 每像素位数
    image.extend_from_slice(&0u32.to_le_bytes()); // 不压缩
    image.extend_from_slice(&(size * size * 4).to_le_bytes()); // 图像长度
    image.extend_from_slice(&[0; 16]); // 分辨率、调色板数

    for y in (0..size).rev() {
        for x in 0..size {
            let at = ((y * size + x) * 4) as usize;
            image.extend_from_slice(&[rgba[at + 2], rgba[at + 1], rgba[at], rgba[at + 3]]);
        }
    }
    // 形状由 alpha 通道承担，掩码只要存在即可。
    image.extend(std::iter::repeat_n(0u8, (size * stride) as usize));
    image
}

/// 一行 AND 掩码的宽度：每像素一位，补齐到四字节。
fn mask_row(size: u32) -> u32 {
    size.div_ceil(32) * 4
}

#[cfg(test)]
mod tests {
    use super::{ICO_SIZES, dib, ico, rgba, tray_rgba};

    #[test]
    fn the_letter_is_white_in_the_middle_of_the_mark() {
        let size = 32;
        let pixels = rgba(size);
        let at = ((size / 2 * size + size / 2) * 4) as usize;
        assert_eq!(&pixels[at..at + 3], &[255, 255, 255]);
        assert_eq!(pixels[at + 3], 255);
    }

    #[test]
    fn the_light_variant_puts_a_dark_letter_on_a_light_plate() {
        let size = 32;
        let pixels = tray_rgba(size, false);
        let at = ((size / 2 * size + size / 2) * 4) as usize;
        assert!(pixels[at] < 40, "浅色版本的字应该是深色：{}", pixels[at]);
        let plate = ((28 * size + 6) * 4) as usize;
        assert!(
            pixels[plate] > 200,
            "浅色版本的底应该是浅色：{}",
            pixels[plate]
        );
    }

    #[test]
    fn the_dark_variant_stays_white_on_black() {
        let size = 32;
        let pixels = tray_rgba(size, true);
        let at = ((size / 2 * size + size / 2) * 4) as usize;
        assert_eq!(&pixels[at..at + 3], &[255, 255, 255]);
        let plate = ((28 * size + 6) * 4) as usize;
        assert!(
            pixels[plate] < 20,
            "深色版本的底应该接近黑色：{}",
            pixels[plate]
        );
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
        // 底板上的一个点：竖杆左边、横杠下方。
        let at = ((28 * size + 6) * 4) as usize;
        assert_eq!(pixels[at + 3], 255);
        assert!(pixels[at] < 20, "底板接近黑色：{}", pixels[at]);
    }

    #[test]
    fn every_stored_size_has_the_bytes_an_icon_entry_promises() {
        for size in ICO_SIZES {
            let data = dib(size);
            let stride = size.div_ceil(32) * 4;
            assert_eq!(
                data.len() as u32,
                40 + size * size * 4 + size * stride,
                "尺寸 {size}"
            );
        }
    }

    #[test]
    fn the_icon_file_lists_every_size() {
        let icon = ico();
        // 保留、类型、数量，之后每张图一条 16 字节的目录项
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
