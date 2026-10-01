//! 无限画布自适应背景网格(源自 docs/02 §4.3)。
//!
//! 缩放越小网格越疏,始终保证屏幕间距落在 48~96px 档内(2 的幂跳档,
//! 缩放时平滑跳变,验收项 docs/02 §9 "网格随缩放自动跳档")。
//!
//! 线宽按屏幕等宽处理:`0.5 / zoom` 的世界线宽经视口变换后恰为 0.5 屏幕像素
//! (docs/02 §4.2 关键技巧 "屏幕等宽描边",只适用于覆盖层/辅助线)。

use kurbo::{BezPath, Rect};
use lumina_core::scene::{Paint, Rgba8, StrokeStyle};

use lumina_core::viewport::Viewport;
use lumina_paint::sink::PaintSink;

/// 网格目标屏幕间距(px,docs/02 §4.3 原文)。
pub const TARGET_SCREEN_STEP: f64 = 48.0;
/// 网格线屏幕宽度(px),世界线宽 = 0.5 / zoom。
pub const GRID_LINE_WIDTH_PX: f64 = 0.5;
/// 网格线颜色(docs/02 §4.3 原文 `rgba8(0xff, 0xff, 0xff, 12)`)。
/// 默认值供无 token 场景;UI 侧应从 lumina-widgets tokens 取主题色。
pub const GRID_COLOR: Rgba8 = [0xff, 0xff, 0xff, 0x0c];

/// 自适应网格:返回给定缩放下网格线的**世界坐标**步长。
///
/// 缩放越小网格越疏,始终保证屏幕间距在 24~96px 之间
/// (公式 docs/02 §4.3 原样:2 的幂跳档)。
pub fn grid_step_world(zoom: f64) -> f64 {
    let target_screen = TARGET_SCREEN_STEP; // 目标屏幕间距
    let raw = target_screen / zoom; // 世界坐标下的原始步长
    let pow = raw.log2().ceil(); // 取 2 的幂,缩放时平滑跳档
    2f64.powf(pow)
}

/// 把网格线画进 sink(在场景对象**之前**调用,网格是背景层)。
///
/// `visible_world_rect` 为当前视口可见的世界矩形(见
/// [`crate::render::visible_world_rect`]);全部竖线/横线合并为一条
/// `BezPath`,单次 stroke 调用(docs/02 §4.3 逐线画法的等价批处理)。
pub fn draw_grid(sink: &mut dyn PaintSink, viewport: &Viewport, visible_world_rect: Rect) {
    let step = grid_step_world(viewport.zoom);
    if !(step.is_finite() && step > 0.0) {
        return;
    }
    // 间距档保护:可见范围内的线数约等于 screen/48,天然有界;仍防御非有限可见区
    let r = visible_world_rect;
    if !(r.x0.is_finite() && r.x1.is_finite() && r.y0.is_finite() && r.y1.is_finite()) {
        return;
    }

    let mut lines = BezPath::new();
    let x0 = (visible_world_rect.x0 / step).floor() * step;
    let y0 = (visible_world_rect.y0 / step).floor() * step;
    let mut x = x0;
    while x <= visible_world_rect.x1 {
        lines.move_to((x, visible_world_rect.y0));
        lines.line_to((x, visible_world_rect.y1));
        x += step;
    }
    let mut y = y0;
    while y <= visible_world_rect.y1 {
        lines.move_to((visible_world_rect.x0, y));
        lines.line_to((visible_world_rect.x1, y));
        y += step;
    }

    // 线宽 0.5/zoom:经视口变换后 = 恒定 0.5 屏幕像素
    let style = StrokeStyle {
        paint: Paint::Solid(GRID_COLOR),
        width: GRID_LINE_WIDTH_PX / viewport.zoom,
    };
    sink.stroke(&style, viewport.world_to_viewport(), &lines);
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Affine;

    /// 只数 stroke 调用并记录线宽的 RecordingSink(测试专用)。
    struct CountingSink {
        strokes: Vec<f64>,
    }

    impl CountingSink {
        fn new() -> Self {
            CountingSink {
                strokes: Vec::new(),
            }
        }
    }

    impl PaintSink for CountingSink {
        fn fill(&mut self, _paint: &Paint, _transform: Affine, _path: &BezPath) {}
        fn stroke(&mut self, style: &StrokeStyle, _transform: Affine, _path: &BezPath) {
            self.strokes.push(style.width);
        }
    }

    #[test]
    fn step_lands_in_24_to_96_screen_band() {
        // 验收(docs/02 §9):网格随缩放自动跳档,屏幕间距始终在 24~96px 之间
        for zoom in [
            Viewport::MIN_ZOOM,
            0.01,
            0.3,
            1.0,
            2.5,
            64.0,
            Viewport::MAX_ZOOM,
        ] {
            let step = grid_step_world(zoom);
            let screen = step * zoom;
            assert!(
                (24.0..=96.0).contains(&screen),
                "zoom={zoom}: 屏幕间距 {screen}px 应落在 [24, 96]"
            );
        }
        // 任务契约指定三档抽查
        for zoom in [1.0, 0.01, 64.0] {
            let screen = grid_step_world(zoom) * zoom;
            assert!((24.0..=96.0).contains(&screen));
        }
    }

    #[test]
    fn step_is_power_of_two() {
        for zoom in [0.01, 0.7, 3.0, 16.0, 64.0] {
            let step = grid_step_world(zoom);
            let log2 = step.log2();
            assert!(
                (log2 - log2.round()).abs() < 1e-9,
                "step={step} 应为 2 的整数幂"
            );
        }
    }

    #[test]
    fn draw_grid_strokes_once_with_screen_constant_width() {
        let viewport = Viewport {
            zoom: 1.0,
            pan: kurbo::Vec2::ZERO,
        };
        let visible = crate::render::visible_world_rect(&viewport, (640.0, 480.0));
        let mut sink = CountingSink::new();
        draw_grid(&mut sink, &viewport, visible);
        assert_eq!(sink.strokes.len(), 1, "全部网格线合并为单次 stroke");
        let width = sink.strokes[0];
        assert!(
            (width - 0.5).abs() < 1e-9,
            "zoom=1 时世界线宽应恰为 0.5(= 屏幕等宽 0.5px)"
        );

        // zoom=8:世界线宽 = 0.5/8,屏幕恒 0.5px
        let zoomed = Viewport {
            zoom: 8.0,
            pan: kurbo::Vec2::ZERO,
        };
        let mut sink = CountingSink::new();
        draw_grid(
            &mut sink,
            &zoomed,
            crate::render::visible_world_rect(&zoomed, (640.0, 480.0)),
        );
        assert!((sink.strokes[0] - 0.5 / 8.0).abs() < 1e-12);
    }

    #[test]
    fn draw_grid_skips_non_finite_visible_rect() {
        let viewport = Viewport {
            zoom: 1.0,
            pan: kurbo::Vec2::ZERO,
        };
        let infinite = Rect::new(
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::INFINITY,
        );
        let mut sink = CountingSink::new();
        draw_grid(&mut sink, &viewport, infinite);
        assert!(sink.strokes.is_empty(), "非有限可见区不应生成网格(防御)");
    }
}
