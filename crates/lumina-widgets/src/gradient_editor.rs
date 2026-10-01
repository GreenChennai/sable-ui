//! 渐变编辑器 GradientEditor(分册四 §5 的 v0.1 子集)。
//!
//! 绑定 `Binding<Paint>`(lumina-core 场景绘制源):只编辑
//! `Paint::LinearGradient` 的色标;`Paint::Solid` 视为两端同色的退化渐变
//! (首个编辑动作把它升格为水平 LinearGradient);`RadialGradient` 仅预览、
//! 色标编辑同样作用于其 stops(几何字段不动,doc 注明)。
//!
//! # v0.1 交互(与分册四 §5 的差异已注明)
//!
//! - 预览条 = 24 段**手动插值**色块(与 peniko/vello 的插值可能有 ≤1/255
//!   舍入差;分册四 §5 的"预览与画布同源零色差" = M2,需 vello 直填);
//! - 色标芯片左右拖 = 改 offset(Binding::set,撤销由调用方 merge);
//! - 点击芯片 = 选中;"+ 色标" = 在最大空档中点插入插值色;"− 删除" = 删选中
//!   (保底 2 个色标);
//! - **选中色标的取色浮窗 = M2**(需要 popup 层;v0.1 提供
//!   [`GradientEditor::set_stop_color`] 供应用层接 ColorWheel,模块内
//!   ColorWell 仅展示当前色)。

use std::cell::Cell;
use std::rc::Rc;

use gpui::DefiniteLength;
use gpui::{
    App, Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Render, Styled, Window, canvas, div, px,
};
use lumina_core::scene::{GradientStop, Paint, Rgba8};

use crate::binding::Binding;
use crate::color::ColorWell;
use crate::theme::theme;
use crate::tokens::{RadiusTokens, SpacingTokens, control_height, h_flex, lerp_rgba8, v_flex};

/// 预览条采样段数(分册四 §5 契约:24 段)。
pub const PREVIEW_SAMPLES: usize = 24;
/// 预览条高度(= 间距 token 的 LG 档 16px,视觉上与属性行协调)。
pub const PREVIEW_HEIGHT_PX: f32 = SpacingTokens::LG;
/// 色标芯片宽(命名固化,非 token 域)。
const STOP_CHIP_W_PX: f32 = 12.0;
/// 色标条高度(派生制:`control_height(HEIGHT_COMPACT, 12, 5)`;因
/// `f32::max` 非 const,此处按派生公式固化为常量,测试守恒等)。
const STRIP_H_PX: f32 = 22.0;

/// 渐变编辑器(有状态 Entity):`cx.new(|_| GradientEditor::new(binding))`。
pub struct GradientEditor {
    binding: Binding<Paint>,
    selected: Option<usize>,
    /// 色标拖拽中:(芯片下标, 窗口 x 起点, 起始 offset)
    drag: Option<(usize, f64, f32)>,
    /// 色标条的元素 bounds(prepaint 回写;拖拽换算像素 → offset 用)
    strip_bounds: Rc<Cell<gpui::Bounds<gpui::Pixels>>>,
}

impl GradientEditor {
    /// 绑定 Paint 的渐变编辑器。
    pub fn new(binding: Binding<Paint>) -> Self {
        GradientEditor {
            binding,
            selected: None,
            drag: None,
            strip_bounds: Rc::new(Cell::new(gpui::Bounds::default())),
        }
    }

    fn paint(&self, cx: &App) -> Paint {
        self.binding.get(cx)
    }

    fn commit_stops(&self, stops: Vec<GradientStop>, cx: &mut Context<Self>) {
        let next = with_stops(&self.paint(cx), stops);
        if next != self.paint(cx) {
            self.binding.set(next, cx);
            cx.notify();
        }
    }

    /// 在最大空档的中点插入插值色标(空档 = 相邻色标 offset 差最大处)。
    pub fn add_stop(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let mut stops = stops_of(&self.paint(cx));
        if stops.is_empty() {
            stops = vec![GradientStop {
                offset: 0.0,
                color: [0, 0, 0, 255],
            }];
        }
        let (index, offset) = largest_gap(&stops);
        let color = interpolate_at(&stops, offset);
        stops.insert(index, GradientStop { offset, color });
        self.selected = Some(index);
        self.commit_stops(stops, cx);
    }

    /// 删除选中色标(保底 2 个;Solid 升格后同样适用)。
    pub fn remove_selected(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(sel) = self.selected else { return };
        let mut stops = stops_of(&self.paint(cx));
        if stops.len() <= 2 || sel >= stops.len() {
            return;
        }
        stops.remove(sel);
        self.selected = None;
        self.commit_stops(stops, cx);
    }

    /// 设置选中(或指定)色标的颜色——应用层接 ColorWheel 的入口;
    /// 取色浮窗 = M2(模块 doc)。
    pub fn set_stop_color(&mut self, index: usize, color: Rgba8, cx: &mut Context<Self>) {
        let mut stops = stops_of(&self.paint(cx));
        if index >= stops.len() {
            return;
        }
        stops[index].color = color;
        self.commit_stops(stops, cx);
    }

    fn on_chip_down(
        &mut self,
        index: usize,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left {
            return;
        }
        self.selected = Some(index);
        let offset = stops_of(&self.paint(cx))
            .get(index)
            .map(|s| s.offset)
            .unwrap_or(0.0);
        self.drag = Some((index, f64::from(event.position.x), offset));
        cx.notify();
    }

    fn on_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some((index, start_x, start_offset)) = self.drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let width = f32::from(self.strip_bounds.get().size.width).max(1.0);
        let dx = f64::from(event.position.x) - start_x;
        let mut stops = stops_of(&self.paint(cx));
        let clamped = clamp_offset(stops.as_slice(), index, start_offset + (dx as f32) / width);
        if clamped != stops[index].offset {
            stops[index].offset = clamped;
            self.commit_stops(stops, cx);
        }
    }

    fn on_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        self.drag = None;
    }
}

impl Render for GradientEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let paint = self.paint(cx);
        let stops = stops_of(&paint);
        let selected = self.selected;
        let strip_bounds = self.strip_bounds.clone();

        // 预览条:24 段手动插值色块
        let mut preview = h_flex()
            .h(px(PREVIEW_HEIGHT_PX))
            .rounded(px(RadiusTokens::SM))
            .border_1()
            .border_color(t.colors.border_subtle)
            .overflow_hidden();
        for i in 0..PREVIEW_SAMPLES {
            let t01 = i as f32 / (PREVIEW_SAMPLES - 1) as f32;
            let color = interpolate_at(&stops, t01);
            preview = preview.child(
                div()
                    .flex_1()
                    .h_full()
                    .bg(crate::tokens::hsla_from_rgba8(color)),
            );
        }

        // 色标条:相对容器 + 绝对定位芯片(百分比 left)
        let mut strip = div()
            .relative()
            .h(px(STRIP_H_PX))
            .w_full()
            .child(
                canvas(
                    move |bounds: gpui::Bounds<gpui::Pixels>, _w: &mut Window, _cx: &mut App| {
                        strip_bounds.set(bounds);
                    },
                    |_paint_bounds: gpui::Bounds<gpui::Pixels>,
                     _state: (),
                     _window: &mut Window,
                     _cx: &mut App| {},
                )
                .size_full(),
            )
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up))
            .on_mouse_move(cx.listener(Self::on_move));
        for (i, stop) in stops.iter().enumerate() {
            let is_sel = selected == Some(i);
            let chip = div()
                .absolute()
                .left(DefiniteLength::Fraction(stop.offset.clamp(0.0, 1.0)))
                .top(px(3.0))
                .size(px(STOP_CHIP_W_PX))
                .ml(px(-STOP_CHIP_W_PX / 2.0))
                .rounded(px(RadiusTokens::SM))
                .border_1()
                .border_color(if is_sel {
                    t.colors.accent
                } else {
                    t.colors.border_strong
                })
                .bg(crate::tokens::hsla_from_rgba8(stop.color))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, win, cx| {
                        this.on_chip_down(i, ev, win, cx)
                    }),
                );
            strip = strip.child(chip);
        }

        // 工具行:加/删色标 + 选中色标的颜色展示(取色浮窗 = M2)
        let selected_well = selected
            .filter(|i| *i < stops.len())
            .map(|i| stops[i].color);
        v_flex()
            .gap(px(SpacingTokens::XS))
            .child(preview)
            .child(strip)
            .child(
                h_flex()
                    .gap(px(SpacingTokens::SM))
                    .child(text_button(
                        "+ 色标",
                        t.colors,
                        cx.listener(|this, _ev, win, cx| {
                            this.add_stop(win, cx);
                        }),
                    ))
                    .child(text_button(
                        "− 删除",
                        t.colors,
                        cx.listener(|this, _ev, win, cx| {
                            this.remove_selected(win, cx);
                        }),
                    ))
                    .children(selected_well.map(ColorWell::new)),
            )
    }
}

/// 小文字按钮(theme 语义色;v0.1 无 hover 动效,动效 = M2 动画引擎接线)。
fn text_button(
    label: &'static str,
    colors: crate::tokens::ColorTokens,
    on_click: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    div()
        .px(px(SpacingTokens::SM))
        .h(px(control_height(crate::tokens::HEIGHT_COMPACT, 14.0, 4.0)))
        .rounded(px(RadiusTokens::SM))
        .bg(colors.surface_3)
        .text_color(colors.text_secondary)
        .cursor_pointer()
        .child(label)
        .on_mouse_down(MouseButton::Left, on_click)
        .into_any_element()
}

// —— 纯函数(单测覆盖)——

/// [`Paint`] → 可编辑色标序列:Linear/Radial 取其 stops;Solid = 两端同色。
pub fn stops_of(paint: &Paint) -> Vec<GradientStop> {
    match paint {
        Paint::LinearGradient { stops, .. } | Paint::RadialGradient { stops, .. } => stops.clone(),
        Paint::Solid(c) => vec![
            GradientStop {
                offset: 0.0,
                color: *c,
            },
            GradientStop {
                offset: 1.0,
                color: *c,
            },
        ],
    }
}

/// 把编辑后的色标序列写回 [`Paint`]:Linear/Radial 保持几何字段只换 stops;
/// Solid 被首次编辑时升格为水平 LinearGradient(0,0)→(1,0)。
pub fn with_stops(paint: &Paint, stops: Vec<GradientStop>) -> Paint {
    match paint {
        Paint::LinearGradient { start, end, .. } => Paint::LinearGradient {
            start: *start,
            end: *end,
            stops,
        },
        Paint::RadialGradient { center, radius, .. } => Paint::RadialGradient {
            center: *center,
            radius: *radius,
            stops,
        },
        Paint::Solid(_) => Paint::LinearGradient {
            start: [0.0, 0.0],
            end: [1.0, 0.0],
            stops,
        },
    }
}

/// 色标序列在 t ∈ [0,1] 处的手动插值色(预览条与插入色标共用)。
/// 假定升序(与 Timeline 不变量同款约定);t 越界钳制。
pub fn interpolate_at(stops: &[GradientStop], t: f32) -> Rgba8 {
    if stops.is_empty() {
        return [0, 0, 0, 255];
    }
    let t = t.clamp(0.0, 1.0);
    let first = &stops[0];
    if t <= first.offset {
        return first.color;
    }
    let last = stops[stops.len() - 1];
    if t >= last.offset {
        return last.color;
    }
    // 右侧第一个 ≥ t 的色标,与其左侧邻居插值
    let split = stops.partition_point(|s| s.offset < t);
    let (a, b) = (&stops[split - 1], &stops[split]);
    if b.offset <= a.offset {
        return b.color; // 零宽段:按跳变处理
    }
    let local = (t - a.offset) / (b.offset - a.offset);
    lerp_rgba8(a.color, b.color, local)
}

/// 最大空档(含头部 [0→首点] 与尾部 [末点→1])的中点与插入下标;
/// 空表返回 (0, 0.5)。
pub fn largest_gap(stops: &[GradientStop]) -> (usize, f32) {
    let mut best_gap = 0.0_f32;
    let mut best = (stops.len(), 0.5_f32);
    let mut prev_offset = 0.0_f32;
    for (i, s) in stops.iter().enumerate() {
        let gap = s.offset - prev_offset;
        if gap > best_gap {
            best_gap = gap;
            best = (i, (prev_offset + s.offset) / 2.0);
        }
        prev_offset = s.offset;
    }
    let tail = 1.0 - prev_offset;
    if tail > best_gap {
        best = (stops.len(), (prev_offset + 1.0) / 2.0);
    }
    (best.0, best.1.clamp(0.0, 1.0))
}

/// 改第 `index` 个色标的 offset,钳在左右邻居之间(不重排,保持下标稳定)。
pub fn clamp_offset(stops: &[GradientStop], index: usize, offset: f32) -> f32 {
    let mut offset = offset.clamp(0.0, 1.0);
    if let Some(prev) = index.checked_sub(1).and_then(|i| stops.get(i)) {
        offset = offset.max(prev.offset);
    }
    if let Some(next) = stops.get(index + 1) {
        offset = offset.min(next.offset);
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(offset: f32, color: Rgba8) -> GradientStop {
        GradientStop { offset, color }
    }

    #[test]
    fn stops_of_solid_is_degenerate_two_ends() {
        let stops = stops_of(&Paint::Solid([10, 20, 30, 255]));
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].color, [10, 20, 30, 255]);
        assert_eq!(stops[1].color, [10, 20, 30, 255]);
        assert_eq!((stops[0].offset, stops[1].offset), (0.0, 1.0));
    }

    #[test]
    fn with_stops_preserves_geometry_and_upgrades_solid() {
        let linear = Paint::LinearGradient {
            start: [1.0, 2.0],
            end: [3.0, 4.0],
            stops: vec![stop(0.0, [255, 0, 0, 255]), stop(1.0, [0, 0, 255, 255])],
        };
        let new_stops = vec![stop(0.2, [0, 255, 0, 255])];
        match with_stops(&linear, new_stops.clone()) {
            Paint::LinearGradient { start, end, stops } => {
                assert_eq!(start, [1.0, 2.0], "几何字段保持");
                assert_eq!(end, [3.0, 4.0]);
                assert_eq!(stops, new_stops);
            }
            other => panic!("应保持 LinearGradient,实际 {other:?}"),
        }
        // Solid 首次编辑升格为水平线性渐变
        match with_stops(&Paint::Solid([1, 1, 1, 255]), new_stops) {
            Paint::LinearGradient { start, end, .. } => {
                assert_eq!((start, end), ([0.0, 0.0], [1.0, 0.0]));
            }
            other => panic!("Solid 应升格为 LinearGradient,实际 {other:?}"),
        }
    }

    #[test]
    fn interpolate_at_matches_ends_and_midpoints() {
        let stops = [stop(0.0, [0, 0, 0, 255]), stop(1.0, [100, 200, 40, 255])];
        assert_eq!(interpolate_at(&stops, 0.0), [0, 0, 0, 255]);
        assert_eq!(interpolate_at(&stops, 1.0), [100, 200, 40, 255]);
        assert_eq!(interpolate_at(&stops, 0.5), [50, 100, 20, 255]);
        // 越界钳制 + 三色标中段取右段插值
        assert_eq!(interpolate_at(&stops, -1.0), [0, 0, 0, 255]);
        let three = [
            stop(0.0, [0, 0, 0, 255]),
            stop(0.5, [255, 255, 255, 255]),
            stop(1.0, [0, 0, 0, 255]),
        ];
        assert_eq!(interpolate_at(&three, 0.75), [128, 128, 128, 255]);
    }

    #[test]
    fn largest_gap_finds_widest_hole_including_tails() {
        // [0, 0.2, 0.3]:尾部空档 0.3→1.0 最宽 → 插在下标 3、中点 0.65
        let stops = [
            stop(0.0, [0, 0, 0, 255]),
            stop(0.2, [1, 1, 1, 255]),
            stop(0.3, [2, 2, 2, 255]),
        ];
        assert_eq!(largest_gap(&stops), (3, 0.65));
        // [0, 0.9]:头部(0→0)无、中档 0.9、尾 0.1 → 插下标 1 中点 0.45
        let two = [stop(0.0, [0, 0, 0, 255]), stop(0.9, [9, 9, 9, 255])];
        assert_eq!(largest_gap(&two), (1, 0.45));
        // 头部空档最宽:[0.8, 1.0] → 插在下标 0、中点 0.4
        let head = [stop(0.8, [8, 8, 8, 255]), stop(1.0, [9, 9, 9, 255])];
        assert_eq!(largest_gap(&head), (0, 0.4));
        // 空表:中点占位
        assert_eq!(largest_gap(&[]), (0, 0.5));
    }

    #[test]
    fn clamp_offset_keeps_neighbors() {
        let stops = [
            stop(0.0, [0, 0, 0, 255]),
            stop(0.5, [9, 9, 9, 255]),
            stop(1.0, [9, 9, 9, 255]),
        ];
        assert_eq!(clamp_offset(&stops, 1, 0.2), 0.2);
        assert_eq!(clamp_offset(&stops, 1, 0.1), 0.1, "高于下邻 0.0 即可");
        assert_eq!(clamp_offset(&stops, 1, -0.5), 0.0, "钳 [0,1] 后不低于下邻");
        assert_eq!(clamp_offset(&stops, 1, 0.9), 0.9);
        assert_eq!(clamp_offset(&stops, 1, 0.8), 0.8);
        assert_eq!(clamp_offset(&stops, 0, 0.8), 0.5, "首色标被右邻 0.5 钳住");
        assert_eq!(clamp_offset(&stops, 2, 0.2), 0.5, "尾色标被左邻 0.5 托住");
    }

    #[test]
    fn strip_height_matches_derivation_formula() {
        // 常量与派生公式守恒(f32::max 非 const 的固化回归)
        assert_eq!(
            STRIP_H_PX,
            control_height(crate::tokens::HEIGHT_COMPACT, 12.0, 5.0)
        );
    }
}
