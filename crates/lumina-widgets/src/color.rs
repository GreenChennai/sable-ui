//! 色井 ColorWell + 色轮 ColorWheel(分册四 §4)。
//!
//! # v0.1 绘制策略(纯 gpui,零纹理)
//!
//! - 色相环 = 36 个扇形多边形([`SECTORS`]),经 `canvas` + `window.paint_path`
//!   逐扇填充(gpui 0.2.2 的 `Path` 是三角扇填充,已核实);
//! - SV 方盘 = 24 条垂直渐变色条(Figma 式方盘:x = 饱和度,y = 明度);
//! - **vello 纹理缓存版 = M2 优化**(分册四 §4 的偷懒方案:静态部分画一次进
//!   纹理,拖动只重画手柄;v0.1 每帧重画 36+24 个多边形,量级无碍)。
//!
//! # 绑定值类型
//!
//! 拖动 → `Binding<[u8; 4]>`(`lumina_core::scene::Paint::Solid` 的 Rgba8,
//! 应用层经 `Binding::map` 与 `Binding<Paint>` 互转)。色相/饱和度换算全部
//! 纯函数,可单测。
//!
//! # v0.1 边界(留 M2)
//! - 拖出窗口即丢拖动事件(gpui 无全局捕获;与 lumina-canvas 同款边界);
//! - 三角取色器(Photoshop 式)与十六进制输入未做。

use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, Bounds, Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render, Styled, Window, canvas, div,
    point, px,
};
use lumina_core::scene::Rgba8;

use crate::binding::Binding;
use crate::theme::theme;
use crate::tokens::{
    RadiusTokens, SpacingTokens, hsl_to_rgb, hsla_from_rgba8, lerp_rgba8, rgb_to_hsl, v_flex,
};

/// 色轮直径(组件布局常数;非间距/圆角 token 域,命名固化)。
pub const WHEEL_DIAMETER_PX: f32 = 160.0;
/// 色相环宽度。
pub const RING_WIDTH_PX: f32 = 16.0;
/// SV 方盘边长。
pub const SQUARE_SIDE_PX: f32 = 120.0;
/// 色相环扇形数(36 × 10°)。
const SECTORS: usize = 36;
/// SV 方盘渐变条数。
const SV_STRIPS: usize = 24;
/// 手柄半径。
const HANDLE_RADIUS_PX: f32 = 6.0;

/// 色井点击回调(应用层弹取色浮窗/取色器;收 `&mut App`)。
pub type ClickFn = Rc<dyn Fn(&mut App)>;

/// 色井(RenderOnce 色块):当前色 + 点击回调(弹取色浮窗由应用层接线;
/// v0.1 回调为空时仅展示)。
#[derive(gpui::IntoElement)]
pub struct ColorWell {
    color: Rgba8,
    on_click: Option<ClickFn>,
}

impl ColorWell {
    /// 展示一个 0~255 RGBA 色块。
    pub fn new(color: Rgba8) -> Self {
        ColorWell {
            color,
            on_click: None,
        }
    }

    /// 点击回调(应用层弹 ColorWheel 浮窗/取色器)。
    pub fn on_click(mut self, f: impl Fn(&mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl gpui::RenderOnce for ColorWell {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &theme(cx).colors;
        div()
            .size(px(SpacingTokens::XL)) // 24px 方块
            .rounded(px(RadiusTokens::SM))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(hsla_from_rgba8(self.color))
            .when_some(self.on_click, |el, click| {
                el.on_mouse_down(MouseButton::Left, move |_ev: &MouseDownEvent, _win, cx| {
                    click(cx);
                })
            })
    }
}

/// 色轮拖拽区(环 or 方盘)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WheelPart {
    Ring,
    Square,
}

/// 色轮(有状态 Entity):外环调色相、方盘调饱和度/明度,拖动实时回写绑定。
///
/// `cx.new(|_| ColorWheel::new(binding))`。
pub struct ColorWheel {
    binding: Binding<Rgba8>,
    drag: Option<WheelPart>,
    /// 环/方盘的元素 bounds(prepaint 回写;命中换算基准)
    ring_bounds: Rc<Cell<Bounds<Pixels>>>,
    square_bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl ColorWheel {
    /// 绑定 Rgba8 的色轮。
    pub fn new(binding: Binding<Rgba8>) -> Self {
        ColorWheel {
            binding,
            drag: None,
            ring_bounds: Rc::new(Cell::new(Bounds::default())),
            square_bounds: Rc::new(Cell::new(Bounds::default())),
        }
    }

    // —— 命中与换算(窗口坐标 → 元素局部)——

    fn local_pos(bounds: &Cell<Bounds<Pixels>>, position: gpui::Point<Pixels>) -> (f32, f32) {
        let b = bounds.get();
        // gpui 0.2.2 的 Pixels 字段 crate 私有,公开通道是 From<Pixels> for f32
        (
            f32::from(position.x) - f32::from(b.origin.x),
            f32::from(position.y) - f32::from(b.origin.y),
        )
    }

    fn apply_hue(&mut self, local: (f32, f32), cx: &mut Context<Self>) {
        let b = self.ring_bounds.get();
        let center = (
            f32::from(b.size.width) / 2.0,
            f32::from(b.size.height) / 2.0,
        );
        let hue = hue_at(local, center);
        let (_, s, v) = hsv_split(self.binding.get(cx));
        let next = hsv_join(hue, s, v);
        if next != self.binding.get(cx) {
            self.binding.set(next, cx);
            cx.notify();
        }
    }

    fn apply_sv(&mut self, local: (f32, f32), cx: &mut Context<Self>) {
        let b = self.square_bounds.get();
        let side = f32::from(b.size.width).max(1.0);
        let (s, v) = sv_at(local, side);
        let (h, _, _) = hsv_split(self.binding.get(cx));
        let next = hsv_join(h, s, v);
        if next != self.binding.get(cx) {
            self.binding.set(next, cx);
            cx.notify();
        }
    }

    fn on_down(
        &mut self,
        part: WheelPart,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left {
            return;
        }
        self.drag = Some(part);
        match part {
            WheelPart::Ring => {
                let local = Self::local_pos(&self.ring_bounds, event.position);
                self.apply_hue(local, cx);
            }
            WheelPart::Square => {
                let local = Self::local_pos(&self.square_bounds, event.position);
                self.apply_sv(local, cx);
            }
        }
    }

    fn on_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        match self.drag {
            Some(WheelPart::Ring) => {
                let local = Self::local_pos(&self.ring_bounds, event.position);
                self.apply_hue(local, cx);
            }
            Some(WheelPart::Square) => {
                let local = Self::local_pos(&self.square_bounds, event.position);
                self.apply_sv(local, cx);
            }
            None => {}
        }
    }

    fn on_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        self.drag = None;
    }
}

impl Render for ColorWheel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let (handle_color, handle_outline) = (t.canvas.anchor, t.canvas.selection);
        let current = self.binding.get(cx);
        let (hue, sat, val) = hsv_split(current);
        let ring_bounds = self.ring_bounds.clone();
        let square_bounds = self.square_bounds.clone();

        v_flex()
            .gap(px(SpacingTokens::SM))
            // —— 色相环 ——
            .child(
                div()
                    .size(px(WHEEL_DIAMETER_PX))
                    .rounded_full()
                    .child(
                        canvas(
                            move |bounds: Bounds<Pixels>, _w: &mut Window, _cx: &mut App| {
                                ring_bounds.set(bounds);
                            },
                            move |paint_bounds: Bounds<Pixels>,
                                  _state: (),
                                  window: &mut Window,
                                  _cx: &mut App| {
                                paint_hue_ring(
                                    window,
                                    paint_bounds,
                                    handle_color,
                                    handle_outline,
                                    hue,
                                );
                            },
                        )
                        .size_full(),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, win, cx| {
                            this.on_down(WheelPart::Ring, ev, win, cx)
                        }),
                    )
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up)),
            )
            // —— SV 方盘 ——
            .child(
                div()
                    .size(px(SQUARE_SIDE_PX))
                    .rounded(px(RadiusTokens::MD))
                    .border_1()
                    .border_color(t.colors.border_subtle)
                    .child(
                        canvas(
                            move |bounds: Bounds<Pixels>, _w: &mut Window, _cx: &mut App| {
                                square_bounds.set(bounds);
                            },
                            move |paint_bounds: Bounds<Pixels>,
                                  _state: (),
                                  window: &mut Window,
                                  _cx: &mut App| {
                                paint_sv_square(
                                    window,
                                    paint_bounds,
                                    hue,
                                    handle_color,
                                    handle_outline,
                                    sat,
                                    val,
                                );
                            },
                        )
                        .size_full(),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, win, cx| {
                            this.on_down(WheelPart::Square, ev, win, cx)
                        }),
                    )
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up)),
            )
            // mouse move 挂在根容器:拖动中鼠标移出子元素仍持续收事件
            .on_mouse_move(cx.listener(Self::on_move))
    }
}

// —— 绘制(canvas paint 侧,窗口坐标)——

/// 色相环:36 扇形 + 手柄小圆。
fn paint_hue_ring(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    handle_color: gpui::Hsla,
    handle_outline: gpui::Hsla,
    hue: f32,
) {
    let center = (
        f32::from(bounds.origin.x) + f32::from(bounds.size.width) / 2.0,
        f32::from(bounds.origin.y) + f32::from(bounds.size.height) / 2.0,
    );
    let outer = f32::from(bounds.size.width) / 2.0;
    let inner = outer - RING_WIDTH_PX;
    for i in 0..SECTORS {
        let a0 = (i as f32 / SECTORS as f32) * std::f32::consts::TAU;
        let a1 = ((i + 1) as f32 / SECTORS as f32) * std::f32::consts::TAU;
        let hue_i = i as f32 / SECTORS as f32;
        // 扇形四边形(屏幕 y 向下:取 -sin 使角度沿屏幕逆时针,与 hue_at 一致)
        let mut path = gpui::Path::new(point(
            px(center.0 + outer * a0.cos()),
            px(center.1 - outer * a0.sin()),
        ));
        path.line_to(point(
            px(center.0 + outer * a1.cos()),
            px(center.1 - outer * a1.sin()),
        ));
        path.line_to(point(
            px(center.0 + inner * a1.cos()),
            px(center.1 - inner * a1.sin()),
        ));
        path.line_to(point(
            px(center.0 + inner * a0.cos()),
            px(center.1 - inner * a0.sin()),
        ));
        window.paint_path(path, gpui::hsla(hue_i, 1.0, 0.5, 1.0));
    }
    // 手柄:当前色相角度的小圆
    let mid_r = (outer + inner) / 2.0;
    let angle = hue * std::f32::consts::TAU;
    let hx = center.0 + mid_r * angle.cos();
    let hy = center.1 - mid_r * angle.sin();
    paint_circle(
        window,
        (hx, hy),
        HANDLE_RADIUS_PX,
        handle_color,
        handle_outline,
    );
}

/// SV 方盘:垂直渐变条 + 手柄(x = 饱和度,y = 明度)。
fn paint_sv_square(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    hue: f32,
    handle_color: gpui::Hsla,
    handle_outline: gpui::Hsla,
    sat: f32,
    val: f32,
) {
    let side = f32::from(bounds.size.width);
    for i in 0..SV_STRIPS {
        let s0 = i as f32 / SV_STRIPS as f32;
        let s1 = (i + 1) as f32 / SV_STRIPS as f32;
        let x0 = f32::from(bounds.origin.x) + s0 * side;
        let x1 = f32::from(bounds.origin.x) + s1 * side;
        let y0 = f32::from(bounds.origin.y);
        let y1 = f32::from(bounds.origin.y) + side;
        let mid_y = (y0 + y1) / 2.0;
        let top = hsla_from_rgba8(square_color(hue, (s0 + s1) / 2.0, 1.0));
        let bottom = hsla_from_rgba8(square_color(hue, (s0 + s1) / 2.0, 0.0));
        // 垂直渐变用上下两段近似(24 条 × 2 段 = 48 级,视觉连续)
        let mut upper = gpui::Path::new(point(px(x0), px(y0)));
        upper.line_to(point(px(x1), px(y0)));
        upper.line_to(point(px(x1), px(mid_y)));
        upper.line_to(point(px(x0), px(mid_y)));
        window.paint_path(upper, top);
        let mut lower = gpui::Path::new(point(px(x0), px(mid_y)));
        lower.line_to(point(px(x1), px(mid_y)));
        lower.line_to(point(px(x1), px(y1)));
        lower.line_to(point(px(x0), px(y1)));
        window.paint_path(lower, bottom);
    }
    // 手柄
    let hx = f32::from(bounds.origin.x) + sat * side;
    let hy = f32::from(bounds.origin.y) + (1.0 - val) * side;
    paint_circle(
        window,
        (hx, hy),
        HANDLE_RADIUS_PX,
        handle_color,
        handle_outline,
    );
}

/// 12 边形近似小圆 + 半透描边。
fn paint_circle(
    window: &mut Window,
    center: (f32, f32),
    radius: f32,
    fill: gpui::Hsla,
    outline: gpui::Hsla,
) {
    let steps = 12;
    let mut path = gpui::Path::new(point(px(center.0 + radius), px(center.1)));
    for i in 1..=steps {
        let a = (i as f32 / steps as f32) * std::f32::consts::TAU;
        path.line_to(point(
            px(center.0 + radius * a.cos()),
            px(center.1 - radius * a.sin()),
        ));
    }
    window.paint_path(path, fill);
    // 描边:半径 +1 的同形路径,半透明
    let mut ring = gpui::Path::new(point(px(center.0 + radius + 1.0), px(center.1)));
    for i in 1..=steps {
        let a = (i as f32 / steps as f32) * std::f32::consts::TAU;
        ring.line_to(point(
            px(center.0 + (radius + 1.0) * a.cos()),
            px(center.1 - (radius + 1.0) * a.sin()),
        ));
    }
    let mut dimmed = outline;
    dimmed.a *= 0.5;
    window.paint_path(ring, dimmed);
}

// —— 纯函数(单测覆盖)——

/// 元素局部坐标 → 色相 [0,1):沿屏幕逆时针(y 向下,取 -dy),
/// 与 [`paint_hue_ring`] 的扇形角度定义一致(右 = 0,上 = 0.25)。
pub fn hue_at(local: (f32, f32), center: (f32, f32)) -> f32 {
    let (dx, dy) = (local.0 - center.0, -(local.1 - center.1));
    (dy.atan2(dx) / std::f32::consts::TAU + 1.0).rem_euclid(1.0)
}

/// 方盘局部坐标 → (饱和度, 明度)(y 向下,y=0 为最亮)。
pub fn sv_at(local: (f32, f32), side: f32) -> (f32, f32) {
    let s = (local.0 / side).clamp(0.0, 1.0);
    let v = 1.0 - (local.1 / side).clamp(0.0, 1.0);
    (s, v)
}

/// 方盘色:`lerp(黑, lerp(白 → 纯色相, s), v)`(HSV 方盘的 RGB 空间近似)。
pub fn square_color(h: f32, s: f32, v: f32) -> Rgba8 {
    let (r, g, b) = hsl_to_rgb(h, 1.0, 0.5);
    let pure = [r, g, b, 255];
    let white = [255u8, 255, 255, 255];
    let black = [0u8, 0, 0, 255];
    lerp_rgba8(black, lerp_rgba8(white, pure, s), v)
}

/// Rgba8 → (h, s, v)(HSV 的 s/v 与 HSL 的 s/l 不同;方盘语义用 V)。
pub fn hsv_split(c: Rgba8) -> (f32, f32, f32) {
    let (h, s, l) = rgb_to_hsl(c[0], c[1], c[2]);
    let v = l + s * l.min(1.0 - l);
    let s_hsv = if v.abs() < f32::EPSILON {
        0.0
    } else {
        2.0 * (1.0 - l / v)
    };
    (h, s_hsv.clamp(0.0, 1.0), v.clamp(0.0, 1.0))
}

/// (h, s, v) → Rgba8。
pub fn hsv_join(h: f32, s: f32, v: f32) -> Rgba8 {
    let l = v * (1.0 - s / 2.0);
    let s_hsl = if l.abs() < f32::EPSILON || (1.0 - l).abs() < f32::EPSILON {
        0.0
    } else {
        ((v - l) / l.min(1.0 - l)).clamp(0.0, 1.0)
    };
    let (r, g, b) = hsl_to_rgb(h, s_hsl, l);
    [r, g, b, 255]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hue_at_matches_painted_sector_direction() {
        let c = (50.0_f32, 50.0);
        // 屏幕逆时针(与 paint_hue_ring 的 -sin 一致):右 0 / 上 0.25 / 左 0.5 / 下 0.75
        assert!(
            (hue_at((c.0 + 10.0, c.1), c) - 0.0).abs() < 1e-5,
            "右侧 = 0"
        );
        assert!(
            (hue_at((c.0, c.1 - 10.0), c) - 0.25).abs() < 1e-5,
            "屏幕上方 = 0.25"
        );
        assert!(
            (hue_at((c.0 - 10.0, c.1), c) - 0.5).abs() < 1e-5,
            "左侧 = 0.5"
        );
        assert!(
            (hue_at((c.0, c.1 + 10.0), c) - 0.75).abs() < 1e-5,
            "屏幕下方 = 0.75"
        );
    }

    #[test]
    fn sv_at_corners_and_clamp() {
        let side = 100.0_f32;
        assert_eq!(sv_at((0.0, 0.0), side), (0.0, 1.0), "左上 = 白域(s=0,v=1)");
        assert_eq!(sv_at((side, 0.0), side), (1.0, 1.0), "右上 = 纯色相");
        assert_eq!(sv_at((0.0, side), side), (0.0, 0.0), "左下 = 黑域");
        assert_eq!(sv_at((side, side), side), (1.0, 0.0));
        // 越界钳制
        assert_eq!(sv_at((-30.0, 230.0), side), (0.0, 0.0));
    }

    #[test]
    fn square_color_axes_match_sv() {
        let h = 0.0_f32; // 红
        assert_eq!(
            square_color(h, 0.0, 1.0),
            [255, 255, 255, 255],
            "s=0 v=1 = 白"
        );
        assert_eq!(
            square_color(h, 1.0, 1.0),
            [255, 0, 0, 255],
            "s=1 v=1 = 纯色相"
        );
        assert_eq!(square_color(h, 0.0, 0.0), [0, 0, 0, 255], "v=0 = 黑");
        let mid = square_color(h, 1.0, 0.5);
        assert!(
            mid[0] > 100 && mid[1] == 0 && mid[2] == 0,
            "半明度纯红应偏暗红 {mid:?}"
        );
    }

    #[test]
    fn hsv_roundtrip_recovers_color() {
        for c in [
            [200u8, 40, 60, 255],
            [10, 240, 130, 255],
            [128, 128, 128, 255],
        ] {
            let (h, s, v) = hsv_split(c);
            let back = hsv_join(h, s, v);
            for ch in 0..3 {
                assert!(
                    (i16::from(c[ch]) - i16::from(back[ch])).abs() <= 2,
                    "{c:?} → hsv {h}/{s}/{v} → {back:?}"
                );
            }
        }
    }

    #[test]
    fn colorwell_builds_with_and_without_callback() {
        let plain = ColorWell::new([255, 0, 0, 255]);
        let wired = plain.on_click(|_cx: &mut App| {});
        let _ = wired; // 构造成功即编译期验收
    }
}
