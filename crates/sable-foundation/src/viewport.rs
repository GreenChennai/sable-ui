//! 视口:世界坐标 ↔ 屏幕坐标的全部换算,画布内核的第一性原理。
//!
//! 源自项目设计文档 docs/02 §2(代码照抄;仅按 kurbo 0.13 实际签名微调
//! `Affine::translate(self.pan)`,并补充 `PartialEq` 与单元测试)。
//!
//! 工程要点(原文):永远不要存"屏幕坐标"进文档模型。文档模型(形状、锚点)
//! 一律世界坐标;屏幕坐标只在事件处理和绘制瞬间存在。

use kurbo::{Affine, Point, Vec2};

/// 视口:管理世界坐标 ↔ 屏幕坐标的全部换算
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// 缩放因子,1.0 = 100%
    pub zoom: f64,
    /// 世界原点在屏幕上的偏移(平移量)
    pub pan: Vec2,
}

impl Viewport {
    pub const MIN_ZOOM: f64 = 0.01; // 1%
    pub const MAX_ZOOM: f64 = 64.0; // 6400%

    /// 世界 → 屏幕:先缩放,再平移
    pub fn world_to_viewport(&self) -> Affine {
        Affine::translate(self.pan) * Affine::scale(self.zoom)
    }

    /// 屏幕 → 世界(逆变换,Affine 可逆是 kurbo 的免费午餐)
    pub fn viewport_to_world(&self) -> Affine {
        self.world_to_viewport().inverse()
    }

    pub fn screen_to_world(&self, screen: Point) -> Point {
        self.viewport_to_world() * screen
    }

    pub fn world_to_screen(&self, world: Point) -> Point {
        self.world_to_viewport() * world
    }

    /// 以光标为锚点缩放(滚轮缩放的关键体验:光标下的点不动)
    pub fn zoom_at(&mut self, screen_anchor: Point, factor: f64) {
        let world_anchor = self.screen_to_world(screen_anchor);
        self.zoom = (self.zoom * factor).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        // 重新计算 pan,使 world_anchor 仍落在 screen_anchor 上
        let new_screen = self.world_to_screen(world_anchor);
        self.pan += screen_anchor - new_screen;
    }

    pub fn pan_by(&mut self, delta_screen: Vec2) {
        self.pan += delta_screen;
    }
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

    /// 缩放锚点不漂移:zoom_at 前后,锚点屏幕位置对应的世界坐标不变
    /// (验收清单 docs/02 §9 第一条)
    #[test]
    fn zoom_at_anchor_does_not_drift() {
        let mut vp = viewport(1.0, (0.0, 0.0));
        let anchor = Point::new(300.0, 200.0);
        let world_before = vp.screen_to_world(anchor);
        vp.zoom_at(anchor, 1.25);
        let world_after = vp.screen_to_world(anchor);
        assert!((world_before - world_after).hypot() < 1e-9);

        // 连续缩放多步,锚点依然不动(浮点余差远小于 1 屏幕像素)
        for factor in [0.8, 1.5, 2.0, 0.33] {
            vp.zoom_at(anchor, factor);
            assert!((world_before - vp.screen_to_world(anchor)).hypot() < 1e-9);
        }
    }

    /// 往返换算:screen → world → screen 恒等(任意 zoom/pan 组合)
    #[test]
    fn screen_world_roundtrip() {
        for zoom in [Viewport::MIN_ZOOM, 0.5, 1.0, 3.7, Viewport::MAX_ZOOM] {
            for pan in [(0.0, 0.0), (-120.5, 80.25), (1e4, -1e4)] {
                let vp = viewport(zoom, pan);
                let screen = Point::new(960.0, -540.0);
                let world = vp.screen_to_world(screen);
                let back = vp.world_to_screen(world);
                assert!(
                    (back - screen).hypot() < 1e-6,
                    "zoom={zoom} pan={pan:?}: {back:?} != {screen:?}"
                );
            }
        }
    }

    /// world_to_viewport 仿射矩阵与逐点换算一致
    #[test]
    fn affine_matches_pointwise_conversion() {
        let vp = viewport(2.5, (33.0, -17.0));
        let world = Point::new(12.0, 8.0);
        let via_affine = vp.world_to_viewport() * world;
        let via_point = vp.world_to_screen(world);
        assert_eq!(via_affine, via_point);
        // 互逆
        let back = vp.viewport_to_world() * via_affine;
        assert!((back - world).hypot() < 1e-12);
    }

    /// clamp 边界:极端缩放因子被夹在 [MIN_ZOOM, MAX_ZOOM]
    #[test]
    fn zoom_clamped_to_bounds() {
        let mut vp = viewport(8.0, (0.0, 0.0));
        let anchor = Point::new(50.0, 60.0);
        vp.zoom_at(anchor, 1e12);
        assert_eq!(vp.zoom, Viewport::MAX_ZOOM);
        // 即使撞到上限,锚点依然不漂移
        let world = vp.screen_to_world(anchor);
        vp.zoom_at(anchor, 0.5);
        assert!((world - vp.screen_to_world(anchor)).hypot() < 1e-9);

        vp.zoom_at(anchor, 1e-12);
        assert_eq!(vp.zoom, Viewport::MIN_ZOOM);

        // 正常范围内按因子缩放
        let mut vp = viewport(1.0, (0.0, 0.0));
        vp.zoom_at(Point::ZERO, 1.1);
        assert!((vp.zoom - 1.1).abs() < 1e-12);
    }

    /// 平移:屏幕增量直接进 pan
    #[test]
    fn pan_by_moves_screen_origin() {
        let mut vp = viewport(1.0, (0.0, 0.0));
        let world = Point::new(7.0, 9.0);
        let before = vp.world_to_screen(world);
        let delta = Vec2::new(-30.0, 12.0);
        vp.pan_by(delta);
        let after = vp.world_to_screen(world);
        assert_eq!(after - before, delta);
        // 平移不改变换算语义:screen→world→screen 仍恒等
        let screen = Point::new(111.0, 222.0);
        let back = vp.world_to_screen(vp.screen_to_world(screen));
        assert!((back - screen).hypot() < 1e-9);
    }
}
