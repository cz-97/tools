//! 主页方格布局的尺寸计算。
//!
//! 方格是等宽的方块，边宽（下称「边长」）有上下限：宽度变大时先让方格变大，够到上限
//! 就加一列、边长回到下限。这个「先长后翻页」的行为由两条边界夹出来：
//!
//! - 边长不能超过 [`CELL_MAX`]：要满足它，列数至少是 `ceil((W + gap) / (max + gap))`。
//! - 边长不能小于 [`CELL_MIN`]：要满足它，列数至多是 `floor((W + gap) / (min + gap))`。
//!
//! 取前者的最小值（也就是「不超上限的最少列数」），再用可用宽度把边长铺满。这两个
//! 条件在 `min <= max` 时总是相容的，结果一定落在 `[min, max]` 里——比「先定列数再
//! 截断边长」那种写法省掉了右侧永远补不齐的空白。

/// 方格边长的下限，逻辑像素。
pub(crate) const CELL_MIN: f32 = 72.0;
/// 方格边长的上限。
pub(crate) const CELL_MAX: f32 = 112.0;
/// 方格之间的间距。
pub(crate) const CELL_GAP: f32 = 8.0;
/// 网格两侧留的空白。
pub(crate) const CELL_PADDING: f32 = 8.0;

/// 按窗口宽度算方格尺寸（扣掉两侧留白）。
///
/// 绘制和方向键都要问它：两边算出来的列数必须一致，否则「按一下右箭头」跳到的位置和
/// 屏幕上看到的那一格对不上。
pub(crate) fn metrics_for(viewport_width: f32) -> GridMetrics {
    GridMetrics::solve(
        viewport_width - CELL_PADDING * 2.0,
        CELL_GAP,
        CELL_MIN,
        CELL_MAX,
    )
}

/// 一屏方格排出来是什么样。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GridMetrics {
    /// 一行放几个。
    pub(crate) columns: usize,
    /// 边长（正方形，宽高相同）。
    pub(crate) cell: f32,
}

impl GridMetrics {
    /// 按可用宽度算列数和边长。
    ///
    /// `width` 是内容区宽度（不含两侧留白），`gap` 是方格之间的间距。
    pub(crate) fn solve(width: f32, gap: f32, min: f32, max: f32) -> Self {
        let (min, max) = (min.max(1.0), max.max(min.max(1.0)));
        let width = width.max(min);
        // 不超上限的最少列数 / 不小于下限的最多列数，两者夹一下。
        let fewest_by_max = ((width + gap) / (max + gap)).ceil().max(1.0);
        let most_by_min = ((width + gap) / (min + gap)).floor().max(1.0);
        let columns = fewest_by_max.min(most_by_min).max(1.0) as usize;
        let span = width - gap * columns.saturating_sub(1) as f32;
        let cell = (span / columns as f32).clamp(min, max);
        Self { columns, cell }
    }

    /// 第 `row` 行第 `column` 列方格的左上角，相对于网格内容区。
    pub(crate) fn corner(&self, row: usize, column: usize, gap: f32) -> (f32, f32) {
        let step = self.cell + gap;
        (column as f32 * step, row as f32 * step)
    }

    /// 一整行（含行间距）有多高。
    pub(crate) fn row_height(&self, gap: f32) -> f32 {
        self.cell + gap
    }
}

#[cfg(test)]
mod tests {
    use super::{CELL_GAP, CELL_MAX, CELL_MIN, GridMetrics};

    fn solve(width: f32) -> GridMetrics {
        GridMetrics::solve(width, CELL_GAP, CELL_MIN, CELL_MAX)
    }

    #[test]
    fn the_cell_never_leaves_its_bounds() {
        let mut width = 60.0;
        while width < 2000.0 {
            let metrics = solve(width);
            assert!(
                metrics.cell >= CELL_MIN - 0.01 && metrics.cell <= CELL_MAX + 0.01,
                "宽度 {width} 时边长 {} 越界",
                metrics.cell
            );
            assert!(metrics.columns >= 1);
            width += 7.0;
        }
    }

    #[test]
    fn cells_grow_until_the_cap_then_a_column_is_added() {
        // 一条列数固定的区间里，边长应当随宽度单调变大；跨过列数变化点就掉回下限附近。
        let mut previous = solve(200.0);
        let mut width = 200.0;
        while width < 1400.0 {
            let metrics = solve(width);
            if metrics.columns == previous.columns {
                assert!(
                    metrics.cell >= previous.cell - 0.01,
                    "宽度 {width} 变宽了边长反而变小"
                );
                assert!(metrics.cell <= CELL_MAX + 0.01);
            } else {
                assert_eq!(metrics.columns, previous.columns + 1, "列数应当一次只加一");
                assert!(
                    metrics.cell >= CELL_MIN - 0.01,
                    "加列之后边长该回到下限附近，实际 {}",
                    metrics.cell
                );
            }
            previous = metrics;
            width += 4.0;
        }
    }

    #[test]
    fn a_narrow_panel_gets_one_column() {
        assert_eq!(solve(CELL_MIN).columns, 1);
        // 恰好装得下一个上限宽的方格时仍是一列。
        assert_eq!(solve(CELL_MAX).columns, 1);
        // 一列能撑到上限之后，得等到「两个下限宽的方格 + 中间那道缝」才够并排。
        let two_columns = CELL_MIN * 2.0 + CELL_GAP;
        assert_eq!(solve(two_columns - 1.0).columns, 1);
        assert_eq!(solve(two_columns).columns, 2);
        assert_eq!(solve(two_columns).cell, CELL_MIN);
    }

    #[test]
    fn corners_step_by_the_cell_size() {
        let metrics = solve(400.0);
        let step = metrics.cell + CELL_GAP;
        assert_eq!(metrics.corner(0, 0, CELL_GAP), (0.0, 0.0));
        assert_eq!(metrics.corner(0, 1, CELL_GAP), (step, 0.0));
        assert_eq!(metrics.corner(2, 3, CELL_GAP), (step * 3.0, step * 2.0));
        assert_eq!(metrics.row_height(CELL_GAP), step);
    }
}

/// 把过长的名字折成最多两行，**按像素宽度对齐**，不是按字数。
///
/// 需求是「8 个字折成 4+4、9 个字折成 5+4、两行各自居中」。中英文和标点宽度不等，所以
/// 按字数劈一定会歪：这里拿 `measure` 去量真实宽度，把「两行宽度差最小」的断点找出来。
///
/// 规则：
/// 1. 一行放得下就一行。
/// 2. 放不下时，两行都塞得进的最大均衡断点；尽量不把英文单词劈开。
/// 3. 两行都塞不下（太长），第一行塞满，第二行截断并补省略号——所以最多显示两行。
///
/// 返回 `(第一行, 第二行)`；只排得下一行时第二行是空串。
pub(crate) fn split_balanced(
    text: &str,
    available: f32,
    measure: &dyn Fn(&str) -> f32,
    ellipsis_width: f32,
) -> (String, String) {
    let total = measure(text);
    if total <= available || available <= 0.0 {
        return (text.to_string(), String::new());
    }

    // 能同时满足两行的断点范围是 first ∈ [total - available, available]，
    // 中间那条最均衡。
    let lowest = (total - available).max(0.0);
    let highest = available;
    if lowest > highest {
        return truncate_two_lines(text, available, measure, ellipsis_width);
    }
    let target = (total / 2.0).clamp(lowest, highest);

    let mut best: Option<(f32, usize)> = None;
    for (index, _) in text.char_indices().skip(1) {
        let (head, _) = text.split_at(index);
        let width = measure(head);
        // 两半都得塞得进，且尽量贴近对半分。
        if width > highest {
            continue;
        }
        let tail_width = total - width;
        if tail_width > available {
            continue;
        }
        let skew = (width - target).abs();
        // 一样平的时候取靠后的断点：9 个字要 5+4，不是 4+5。
        if best.is_none_or(|(current, _)| skew <= current) {
            best = Some((skew, index));
        }
    }

    let Some((_, index)) = best else {
        return truncate_two_lines(text, available, measure, ellipsis_width);
    };
    let index = avoid_splitting_a_word(text, index, available, measure);
    let (head, tail) = text.split_at(index);
    // 断点正好落在空格上时，别把那个空格画进行尾（居中时会看成缩进）。
    (head.trim_end().to_string(), tail.trim_start().to_string())
}

/// 断点正好落在英文单词中间时，往前退到最近的空格（别把 `Environment` 劈成 `Envi`）。
///
/// 退太多就没意义了：只在第一行还剩一半以上宽度时才退。
fn avoid_splitting_a_word(
    text: &str,
    index: usize,
    available: f32,
    measure: &dyn Fn(&str) -> f32,
) -> usize {
    let (head, tail) = text.split_at(index);
    let joins_word = |byte: Option<char>| {
        byte.is_some_and(|character| character.is_ascii_alphanumeric() && character.is_ascii())
    };
    if !joins_word(head.chars().next_back()) || !joins_word(tail.chars().next()) {
        return index;
    }
    let Some(space) = head.rfind(' ') else {
        return index;
    };
    let first_line = measure(&head[..space]);
    let second_line = measure(&text[space..]);
    // 退得太狠就不要退了：第一行只剩一小截，或者退完第二行反而塞不下。
    if space == 0 || first_line < available * 0.3 || second_line > available {
        return index;
    }
    space
}

/// 两行都塞不下：第一行塞满，第二行截断加省略号。
fn truncate_two_lines(
    text: &str,
    available: f32,
    measure: &dyn Fn(&str) -> f32,
    ellipsis_width: f32,
) -> (String, String) {
    let cut = widest_prefix(text, available, measure);
    let (head, tail) = text.split_at(cut);
    let room = (available - ellipsis_width).max(0.0);
    let keep = widest_prefix(tail, room, measure);
    let (shown, _) = tail.split_at(keep);
    (head.to_string(), format!("{}…", shown.trim_end()))
}

/// 最长的一段前缀，使它的宽度不超过 `limit`。
fn widest_prefix(text: &str, limit: f32, measure: &dyn Fn(&str) -> f32) -> usize {
    let mut best = 0;
    for (index, _) in text.char_indices().skip(1) {
        if measure(&text[..index]) > limit {
            break;
        }
        best = index;
    }
    if best == 0 && !text.is_empty() && measure(text) <= limit {
        return text.len();
    }
    best
}

#[cfg(test)]
mod wrap_tests {
    use super::split_balanced;

    /// 假宽度：中日韩和全角标点按 2 份，其余按 1 份。
    fn wide(character: char) -> f32 {
        let code = character as u32;
        let cjk = (0x2e80..=0x9fff).contains(&code) || (0xff00..=0xffef).contains(&code);
        if cjk { 2.0 } else { 1.0 }
    }

    fn measure(text: &str) -> f32 {
        text.chars().map(wide).sum::<f32>() * 10.0
    }

    #[test]
    fn a_short_name_stays_on_one_line() {
        let (head, tail) = split_balanced("打开设置", 100.0, &measure, 10.0);
        assert_eq!(head, "打开设置");
        assert!(tail.is_empty());
    }

    #[test]
    fn eight_characters_split_four_and_four() {
        // 一行最多放 7 个（70px）。
        let (head, tail) = split_balanced("abcdefgh", 70.0, &measure, 10.0);
        assert_eq!((head.as_str(), tail.as_str()), ("abcd", "efgh"));
    }

    #[test]
    fn nine_characters_split_five_and_four() {
        let (head, tail) = split_balanced("abcdefghi", 70.0, &measure, 10.0);
        assert_eq!(head.chars().count(), 5);
        assert_eq!(tail.chars().count(), 4);
    }

    #[test]
    fn mixed_width_text_splits_by_pixels_not_by_characters() {
        // 「中文」= 40px，一行 70px：按字数会劈成 3+3（中文a / bcd），按宽度是 中文ab + cdef。
        let (head, tail) = split_balanced("中文abcdef", 70.0, &measure, 10.0);
        assert_eq!((head.as_str(), tail.as_str()), ("中文a", "bcdef"));
    }

    #[test]
    fn too_long_gets_two_lines_and_an_ellipsis() {
        let (head, tail) = split_balanced("abcdefghijklmnop", 70.0, &measure, 10.0);
        assert_eq!(head.chars().count(), 7);
        assert!(tail.ends_with('…'), "第二行该带省略号：{tail}");
        assert!(measure(&tail) <= 70.0 + 0.01);
    }

    #[test]
    fn a_latin_word_is_not_cut_in_the_middle() {
        // 96px 一行：「open spac」是 9 个字（90px），正好劈在单词中间，应该退到空格。
        let (head, tail) = split_balanced("open space", 96.0, &measure, 10.0);
        assert_eq!((head.as_str(), tail.as_str()), ("open", "space"));
    }
}
