//! 输入换算:鼠标/滚轮事件 → 视口操作的纯函数层(源自 docs/02 §8 "输入处理")。
//!
//! 上层(gpui_element 或示例)只负责把框架事件拆成
//!"屏幕坐标 + 增量 + 修饰键",全部换算数学在这里,可直接单测。

use kurbo::{Point, Vec2};

use sable_foundation::viewport::Viewport;

/// Ctrl+滚轮 单阶缩放因子(docs/02 §8 原文 1.1)。
pub const ZOOM_STEP: f64 = 1.1;
/// 点选容差(屏幕像素,docs/02 §5:"屏幕 4px / zoom 得到")。
pub const HIT_TOLERANCE_PX: f64 = 4.0;

/// 滚轮事件 → 视口更新(docs/02 §8 `on_scroll` 原语义)。
///
/// - `ctrl = true`:以光标为锚缩放,每阶 ±[`ZOOM_STEP`](1.1 倍),锚点不漂移;
/// - `ctrl = false`:普通滚轮 = 平移(触控板双指手势同此路径)。
///
/// `cursor_screen` 为事件点的**屏幕**坐标(相对画布元素),`delta` 为屏幕像素增量。
pub fn on_scroll(viewport: &mut Viewport, cursor_screen: Point, delta: Vec2, ctrl: bool) {
    if ctrl {
        // Ctrl+滚轮 = 以光标为锚缩放(也可换触控板 pinch)
        let factor = if delta.y > 0.0 {
            ZOOM_STEP
        } else {
            1.0 / ZOOM_STEP
        };
        viewport.zoom_at(cursor_screen, factor);
    } else {
        viewport.pan_by(delta); // 普通滚轮 = 平移
    }
}

/// 点选容差换算:屏幕 `4px` 对应的世界容差(docs/02 §5 原式 `4.0 / zoom`)。
pub fn screen_tolerance(zoom: f64) -> f64 {
    HIT_TOLERANCE_PX / zoom
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport(zoom: f64, pan: (f64, f64)) -> Viewport {
        Viewport {
            zoom,
            pan: Vec2::new(pan.0, pan.1),
        }
    }

    #[test]
    fn ctrl_scroll_zooms_at_cursor_without_drift() {
        let mut vp = viewport(1.0, (0.0, 0.0));
        let cursor = Point::new(320.0, 240.0);
        let world_anchor = vp.screen_to_world(cursor);

        // 向上滚 = 放大 1.1 倍/阶
        on_scroll(&mut vp, cursor, Vec2::new(0.0, 120.0), true);
        assert!((vp.zoom - 1.1).abs() < 1e-12, "单阶应放大 1.1 倍");
        assert!(
            (vp.screen_to_world(cursor) - world_anchor).hypot() < 1e-9,
            "光标锚点下的世界点不得漂移"
        );

        // 向下滚 = 缩小
        on_scroll(&mut vp, cursor, Vec2::new(0.0, -120.0), true);
        assert!((vp.zoom - 1.0).abs() < 1e-12);
        assert!((vp.screen_to_world(cursor) - world_anchor).hypot() < 1e-9);
    }

    #[test]
    fn ctrl_scroll_clamps_at_zoom_limits() {
        let mut vp = viewport(Viewport::MAX_ZOOM, (0.0, 0.0));
        on_scroll(&mut vp, Point::ZERO, Vec2::new(0.0, 10.0), true);
        assert_eq!(vp.zoom, Viewport::MAX_ZOOM, "超出上限被夹住");

        let mut vp = viewport(Viewport::MIN_ZOOM, (0.0, 0.0));
        on_scroll(&mut vp, Point::ZERO, Vec2::new(0.0, -10.0), true);
        assert_eq!(vp.zoom, Viewport::MIN_ZOOM);
    }

    #[test]
    fn plain_scroll_pans_by_exact_delta() {
        let mut vp = viewport(2.0, (10.0, 20.0));
        on_scroll(
            &mut vp,
            Point::new(999.0, 999.0),
            Vec2::new(-30.0, 12.5),
            false,
        );
        assert_eq!(vp.pan, Vec2::new(-20.0, 32.5), "普通滚轮 = pan 加增量");
        assert_eq!(vp.zoom, 2.0, "普通滚轮不改缩放");

        // 光标位置对平移无意义(平移是全屏一致移动)
        on_scroll(&mut vp, Point::ZERO, Vec2::new(30.0, -12.5), false);
        assert_eq!(vp.pan, Vec2::new(10.0, 20.0));
    }

    #[test]
    fn screen_tolerance_is_four_px_over_zoom() {
        assert_eq!(screen_tolerance(1.0), 4.0);
        assert_eq!(screen_tolerance(4.0), 1.0);
        assert_eq!(screen_tolerance(0.5), 8.0);
        assert!((screen_tolerance(0.01) - 400.0).abs() < 1e-9);
        assert!((screen_tolerance(64.0) - 0.0625).abs() < 1e-12);
    }
}
