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
