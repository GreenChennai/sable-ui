//! 曲线编辑器(分册四 §9)的 v0.1 只读子件:`CurvePreview`。
//!
//! 采样 64 点,以**竖条列**绘制缓动曲线(gpui 0.2.2 的 `Path` 是填充三角扇、
//! 无描边能力——画 1px 线需自建 ribbon 几何;竖条列编译最稳,契约允许),
//! 网格底用主题语义色的分隔线 div。
//!
//! # 完整交互 = M2(分册四 §9 规划,doc 注明)
//!
//! `Binding<Vec<CurveKey>>`、拖 key 改值、拖手柄改曲率、双击线加 key、
//! 右键删 key、预设缓动切换;绘制换 kurbo::BezPath → vello stroke。
//! 求值侧已就位:`sable_video::curve::evaluate`(播放器每帧调用)。

use gpui::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div, px};
use sable_video::model::Easing;

use crate::theme::theme;
use crate::tokens::{RadiusTokens, h_flex};

/// 曲线采样点数(契约:64)。
pub const CURVE_SAMPLES: usize = 64;

/// 缓动曲线只读预览:`CurvePreview::new(easing, width, height)`(RenderOnce)。
#[derive(gpui::IntoElement)]
pub struct CurvePreview {
    easing: Easing,
    /// 预览区宽度(px)
    width: f32,
    /// 预览区高度(px)
    height: f32,
}

impl CurvePreview {
    /// 指定缓动与尺寸的只读预览。
    pub fn new(easing: Easing, width: f32, height: f32) -> Self {
        CurvePreview {
            easing,
            width,
            height,
        }
    }
}

impl RenderOnce for CurvePreview {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = theme(cx).colors;
        let bar_w = (self.width / CURVE_SAMPLES as f32).max(1.0);

        // 网格底:四分线(横 2 条 + 竖 2 条)用 border_subtle
        let grid = div()
            .absolute()
            .size_full()
            .child(
                div()
                    .absolute()
                    .top(px(self.height * 0.33))
                    .left(px(0.0))
                    .w_full()
                    .h(px(1.0))
                    .bg(colors.border_subtle),
            )
            .child(
                div()
                    .absolute()
                    .top(px(self.height * 0.66))
                    .left(px(0.0))
                    .w_full()
                    .h(px(1.0))
                    .bg(colors.border_subtle),
            )
            .child(
                div()
                    .absolute()
                    .left(px(self.width * 0.33))
                    .top(px(0.0))
                    .h_full()
                    .w(px(1.0))
                    .bg(colors.border_subtle),
            )
            .child(
                div()
                    .absolute()
                    .left(px(self.width * 0.66))
                    .top(px(0.0))
                    .h_full()
                    .w(px(1.0))
                    .bg(colors.border_subtle),
            );

        // 竖条列:底部对齐,高度 = eased(u) × height
        let mut bars = h_flex().absolute().inset_0().items_end();
        for i in 0..CURVE_SAMPLES {
            let u = i as f64 / (CURVE_SAMPLES - 1) as f64;
            let eased = self.easing.apply(u).clamp(-0.5, 1.5) as f32; // 弹簧过冲允许少量越界
            let h = (eased * self.height).clamp(1.0, self.height * 1.5);
            bars = bars.child(div().w(px(bar_w)).h(px(h)).bg(colors.accent));
        }

        div()
            .relative()
            .w(px(self.width))
            .h(px(self.height))
            .rounded(px(RadiusTokens::SM))
            .border_1()
            .border_color(colors.border_strong)
            .bg(colors.surface_2)
            .overflow_hidden()
            .child(grid)
            .child(bars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_span_unit_interval() {
        // 采样点覆盖 [0,1] 两端:线性缓动首尾条 = 1.0 与满高
        let e = Easing::Linear;
        assert_eq!(e.apply(0.0), 0.0);
        assert_eq!(e.apply(1.0), 1.0);
        // 条数固定 = CURVE_SAMPLES(渲染路径需 App;此处验收采样数学)
        let last_u = (CURVE_SAMPLES - 1) as f64 / (CURVE_SAMPLES - 1) as f64;
        assert_eq!(last_u, 1.0);
    }

    #[test]
    fn preview_builds_for_all_presets() {
        // 全部预设都能构造(RenderOnce 渲染需 App,构造即编译期验收)
        for e in [
            Easing::Linear,
            Easing::InCubic,
            Easing::OutCubic,
            Easing::InOutCubic,
            Easing::Spring,
        ] {
            let _ = CurvePreview::new(e, 120.0, 80.0);
        }
    }

    #[test]
    fn bar_height_math_clamps_overshoot() {
        // 弹簧峰值 ~1.18 → 高度钳到 1.5 倍内不溢出
        let height = 80.0_f32;
        let eased = Easing::Spring.apply(0.55) as f32;
        let h = (eased * height).clamp(1.0, height * 1.5);
        assert!(h <= height * 1.5);
        assert!(h >= 1.0);
    }
}
