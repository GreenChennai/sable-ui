//! 数值拖拽框 NumberField(分册四 §6,设计软件的灵魂小件)。
//!
//! # v0.1 交互矩阵(与分册四 §6 的差异均已注明)
//!
//! | 输入 | 行为 | 状态 |
//! |---|---|---|
//! | 按住左右拖 | `start + dx × step × 修饰键倍率`,钳制 range | ✅ 完整(拖拽路径必须完整) |
//! | Shift 拖 | ×10 | ✅ |
//! | Alt 拖 | ×0.1(**Alt 优先**,上游 NumField 同款) | ✅ |
//! | 滚轮 | ±1 步进(Shift ×10) | ✅ |
//! | ↑/↓ 键 | ±1 步进(Shift ×10) | ⚠️ 已挂 `on_key_down`,但 gpui 0.2.2 的
//!   元素级键盘事件只在元素持有焦点时派发,v0.1 未接焦点系统(track_focus)——
//!   **键盘步进的可用性 = M2 接焦点后生效**(TODO-M2) |
//! | 双击 | 进入文本编辑态(v0.1 简化:仅展示当前值 + 强调描边) | ⚠️ **简化**:
//!   gpui-component 0.7.0 的 Input 跑在 gpui-pre 0.3.7 类型世界(已核实其
//!   Cargo.toml),与 gpui 0.2.2 不互通,接入成本过高——真文本输入 = TODO-M2 |
//!
//! # 撤销与节流
//!
//! 每次 scrub 都走 [`Binding::set`];widgets 层不做 16ms 节流,**连续 set 由
//! 调用方 set 闭包里的命令 `merge` 语义兜底**(分册三 §2 拖动范式,契约允许
//! 二选一);值未变化时跳过 set(PartialEq 短路)。
//!
//! # A7 微交互(hover/press 三态,分册六 §4.3 #1)
//!
//! 底色 = `surface_2` 基色上插值:hover 时经 [`hover_tint`](crate::interact::hover_tint)
//! 加亮 4%(120ms ease-out,由 [`HoverState`](crate::interact::HoverState) 驱动),
//! 按下(scrub 拖拽中)直接取 `pressed_tint` 加亮 8%。动画插值需要 hover
//! 进出事件(`on_hover` 只存在于 Stateful 元素),故**同屏多个实例时必须
//! 经 [`.element_id`](Self::element_id) 给唯一 id**;未给 id 时退化为 gpui
//! hover 样式即时切换(无动画,不破缺省构造)。减弱动态(A8)下插值被
//! [`reduced_motion`](crate::anim::reduced_motion) 短路,进度直通 0/1。

use gpui::{
    Context, ElementId, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Render, ScrollWheelEvent, SharedString,
    StatefulInteractiveElement, Styled, Window, px,
};

use crate::anim::lerp_hsla;
use crate::binding::Binding;
use crate::interact::{self, HoverState};
use crate::theme::theme;
use crate::tokens::{
    FONT_SIZE_BODY, HEIGHT_COMPACT, RadiusTokens, SpacingTokens, control_height, h_flex,
};

/// 面板正文的估行高(11~12px 字号,数值用等宽观感)。
const LINE_HEIGHT_PX: f32 = 14.0;
/// 控件的垂直内边距。
const V_PADDING_PX: f32 = 4.0;
/// 滚轮一行折算像素(与 sable-canvas 同款惯例)。
const SCROLL_LINE_PX: f32 = 24.0;

/// 数值框(有状态 Entity):`cx.new(|_| NumberField::new(binding).range(0.0, 100.0))`。
pub struct NumberField {
    binding: Binding<f64>,
    range: (f64, f64),
    step: f64,
    unit: &'static str,
    /// 拖拽中:窗口 x 起点 + 起始值
    drag: Option<ScrubDrag>,
    /// 文本编辑态(v0.1 简化,见模块 doc)
    editing: bool,
    /// A7 悬停进度(120ms ease-out;拖拽 = pressed 态,复用 ScrubDrag 判定)
    hover: HoverState,
    /// 悬停事件跟踪用的元素 id(`on_hover` 需要 Stateful 元素;同屏多实例
    /// 必须各给唯一 id,未给则退化为即时 hover 样式)
    element_id: Option<ElementId>,
}

#[derive(Clone, Copy, Debug)]
struct ScrubDrag {
    start_x: f64,
    start_val: f64,
}

impl NumberField {
    /// 绑定驱动的数值框;默认范围 0..=100、步长 1、无单位。
    pub fn new(binding: Binding<f64>) -> Self {
        NumberField {
            binding,
            range: (0.0, 100.0),
            step: 1.0,
            unit: "",
            drag: None,
            editing: false,
            hover: HoverState::new(),
            element_id: None,
        }
    }

    /// 元素 id(悬停动画需要 Stateful 元素;同屏多个数值框各给唯一 id,
    /// 如 `ElementId::named_usize("inspector-num", ix)`)。
    pub fn element_id(mut self, id: impl Into<ElementId>) -> Self {
        self.element_id = Some(id.into());
        self
    }

    /// 取值范围(拖拽/步进钳制)。
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.range = (min, max);
        self
    }

    /// 基础步长(拖拽 1px 的值变化;Shift ×10 / Alt ×0.1 另算)。
    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// 单位后缀("px" / "°" / "%")。
    pub fn unit(mut self, unit: &'static str) -> Self {
        self.unit = unit;
        self
    }

    /// 实际控件高度(派生制):max(22, 14 + 2×4) = 22。
    pub fn control_height() -> f32 {
        control_height(HEIGHT_COMPACT, LINE_HEIGHT_PX, V_PADDING_PX)
    }

    // —— 事件(gpui 事件坐标一律窗口坐标,dx 与 bounds 无关)——

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left {
            return;
        }
        // 双击 → 文本编辑态(v0.1 简化展示;真输入 = TODO-M2)
        self.editing = event.click_count >= 2;
        // gpui 0.2.2 的 Pixels 字段 crate 私有(已核实),公开通道是 From<Pixels> for f64
        self.drag = Some(ScrubDrag {
            start_x: f64::from(event.position.x),
            start_val: self.binding.get(cx),
        });
        cx.notify();
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.drag else { return };
        if event.pressed_button != Some(MouseButton::Left) {
            return; // 拖出元素后松键的场景由 on_mouse_up_out 兜底(M2)
        }
        let scale = modifier_scale(event.modifiers.alt, event.modifiers.shift);
        let dx = f64::from(event.position.x) - drag.start_x;
        let next = scrub(drag.start_val, dx, self.step, scale, self.range);
        if next != self.binding.get(cx) {
            // 每次 set 由调用方闭包的 merge 语义合并为一步撤销(模块 doc)
            self.binding.set(next, cx);
            cx.notify();
        }
    }

    fn on_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.drag = None;
        // 不 notify:值未变时无需重绘(editing 已在 down 时 notify)
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dy = f64::from(event.delta.pixel_delta(px(SCROLL_LINE_PX)).y);
        if dy == 0.0 {
            return;
        }
        let scale = modifier_scale(false, event.modifiers.shift);
        let current = self.binding.get(cx);
        // 滚轮向下 = 减(与面板惯例一致)
        let next = scrub(current, -dy.signum(), self.step, scale, self.range);
        if next != current {
            self.binding.set(next, cx);
            cx.notify();
        }
    }

    fn on_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.keystroke.key.as_str() {
            "up" | "down" => {
                // 焦点系统接好后生效(模块 doc:TODO-M2);逻辑先行落地
                let dir: f64 = if event.keystroke.key == "up" {
                    1.0
                } else {
                    -1.0
                };
                let scale = modifier_scale(false, event.keystroke.modifiers.shift);
                let current = self.binding.get(cx);
                let next = scrub(current, dir, self.step, scale, self.range);
                if next != current {
                    self.binding.set(next, cx);
                    cx.notify();
                }
            }
            "enter" | "escape" if self.editing => {
                self.editing = false;
                cx.notify();
            }
            _ => {}
        }
    }

    /// A7 悬停进出:驱动 [`HoverState`] 过渡并请求重绘(动画帧由 render 里
    /// 的 `request_animation_frame` 续)。
    fn on_hover_changed(&mut self, hovered: &bool, _window: &mut Window, cx: &mut Context<Self>) {
        let now = interact::now_ms();
        if *hovered {
            self.hover.on_enter(now);
        } else {
            self.hover.on_leave(now);
        }
        cx.notify();
    }
}

impl Render for NumberField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &theme(cx).colors;
        let value = self.binding.get(cx);
        let dragging = self.drag.is_some();
        let display: SharedString = format!("{}{}", format_value(value), self.unit).into();

        // A7 三态底色:静止/悬停 = surface_2 → hover_tint 插值;按下 = pressed_tint
        let base_bg = colors.surface_2;
        let now = interact::now_ms();
        let hover_progress = self.hover.progress_at(now);
        let bg = if dragging {
            interact::pressed_tint(base_bg)
        } else {
            lerp_hsla(base_bg, interact::hover_tint(base_bg), hover_progress)
        };

        let root = h_flex()
            .justify_center()
            .h(px(Self::control_height()))
            .min_w_0()
            .px(px(SpacingTokens::SM))
            .rounded(px(RadiusTokens::SM))
            .border_1()
            .border_color(if dragging || self.editing {
                colors.border_strong
            } else {
                colors.border_subtle
            })
            .bg(bg)
            .text_size(px(FONT_SIZE_BODY))
            .text_color(if dragging || self.editing {
                colors.text_primary
            } else {
                colors.text_secondary
            })
            .cursor_pointer()
            .child(display)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .on_key_down(cx.listener(Self::on_key_down));

        // 悬停动画在跑就续帧(静止零帧提交,分册六 §4.4)
        if self.hover.is_running(now) {
            window.request_animation_frame();
        }
        match self.element_id.clone() {
            // 有 id:on_hover 驱动 120ms 插值(见模块 doc)
            Some(id) => root
                .id(id)
                .on_hover(cx.listener(Self::on_hover_changed))
                .into_any_element(),
            // 无 id:退化为 gpui hover 样式即时切换(无动画)
            None => root
                .hover(move |style| style.bg(interact::hover_tint(base_bg)))
                .into_any_element(),
        }
    }
}

/// 修饰键倍率(纯函数):**Alt 优先 ×0.1,其次 Shift ×10**(上游 NumField
/// docs/upstream/02 §4.2 的 scrubby 公式同款)。
pub fn modifier_scale(alt: bool, shift: bool) -> f64 {
    if alt {
        0.1
    } else if shift {
        10.0
    } else {
        1.0
    }
}

/// 拖拽换算(纯函数):`start + dx × step × scale` 钳制到 range。
pub fn scrub(start_val: f64, dx_px: f64, step: f64, scale: f64, range: (f64, f64)) -> f64 {
    clamp_range(start_val + dx_px * step * scale, range)
}

/// 步进(键/滚轮共用):dir = ±1。
pub fn step_by(current: f64, dir: f64, step: f64, scale: f64, range: (f64, f64)) -> f64 {
    clamp_range(current + dir * step * scale, range)
}

/// 钳制到闭区间(min > max 时交换,防御调用方笔误)。
pub fn clamp_range(v: f64, range: (f64, f64)) -> f64 {
    let (lo, hi) = if range.0 <= range.1 {
        range
    } else {
        (range.1, range.0)
    };
    v.clamp(lo, hi)
}

/// 数值显示(纯函数):最多 2 位小数、去尾零;整数不带小数点。
pub fn format_value(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let rounded = (v * 100.0).round() / 100.0;
    if (rounded - rounded.trunc()).abs() < f64::EPSILON {
        format!("{}", rounded as i64)
    } else {
        let s = format!("{rounded:.2}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_scale_alt_takes_priority_over_shift() {
        assert_eq!(modifier_scale(false, false), 1.0);
        assert_eq!(modifier_scale(false, true), 10.0);
        assert_eq!(modifier_scale(true, false), 0.1);
        assert_eq!(modifier_scale(true, true), 0.1, "Alt 优先");
    }

    #[test]
    fn scrub_moves_by_dx_times_step_and_clamps() {
        assert_eq!(scrub(50.0, 10.0, 1.0, 1.0, (0.0, 100.0)), 60.0);
        assert_eq!(
            scrub(50.0, 10.0, 1.0, 10.0, (0.0, 100.0)),
            100.0,
            "Shift×10 钳上限"
        );
        // Alt×0.1 细调(无钳制路径):5 - 20×1.0×0.1 = 3
        assert_eq!(
            scrub(5.0, -20.0, 1.0, 0.1, (0.0, 100.0)),
            3.0,
            "Alt×0.1 细调"
        );
        // 同参数在贴下限处触发钳制:0.5 - 2.0 → 0
        assert_eq!(
            scrub(0.5, -20.0, 1.0, 0.1, (0.0, 100.0)),
            0.0,
            "细调仍受 range 钳制"
        );
        assert_eq!(scrub(0.0, -50.0, 1.0, 1.0, (0.0, 100.0)), 0.0, "钳下限");
        // 负范围与小步长(角度 0.5°/px)
        assert_eq!(scrub(-90.0, 4.0, 0.5, 1.0, (-180.0, 180.0)), -88.0);
    }

    #[test]
    fn step_by_walks_range_bounds() {
        assert_eq!(step_by(99.0, 1.0, 1.0, 1.0, (0.0, 100.0)), 100.0);
        assert_eq!(
            step_by(100.0, 1.0, 1.0, 1.0, (0.0, 100.0)),
            100.0,
            "到顶不再越界"
        );
        assert_eq!(step_by(1.0, -1.0, 1.0, 1.0, (0.0, 100.0)), 0.0);
        assert_eq!(step_by(5.0, 1.0, 1.0, 10.0, (0.0, 100.0)), 15.0);
    }

    #[test]
    fn clamp_range_swaps_reversed_bounds() {
        assert_eq!(clamp_range(5.0, (10.0, 0.0)), 5.0);
        assert_eq!(clamp_range(99.0, (10.0, 0.0)), 10.0, "反写范围被防御性交换");
        assert_eq!(clamp_range(-99.0, (10.0, 0.0)), 0.0);
    }

    #[test]
    fn format_value_trims_and_limits_decimals() {
        assert_eq!(format_value(10.0), "10");
        assert_eq!(format_value(-3.0), "-3");
        assert_eq!(format_value(0.5), "0.5");
        assert_eq!(format_value(1.25), "1.25");
        assert_eq!(format_value(1.234), "1.23", "最多 2 位小数(四舍五入)");
        assert_eq!(format_value(1.250001), "1.25", "去尾零");
        assert_eq!(format_value(f64::NAN), "0", "非有限值防御");
        assert_eq!(format_value(f64::INFINITY), "0");
    }

    #[test]
    fn control_height_fills_tier_floor() {
        // 14 行高 + 8 padding = 22,恰好紧凑档下限
        assert_eq!(NumberField::control_height(), HEIGHT_COMPACT);
    }
}
