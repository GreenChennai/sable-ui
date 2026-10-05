//! 批 3:Slider(§5.6 #20)——轨道 + 圆点手柄;拖拽/键盘(←→ 步进、
//! Home/End)/双击复位;受控协议(value + on_change 回执,宿主回写)。
//!
//! 命中/键盘状态机全部纯函数可测;渲染 = canvas 捕获轨道 bounds(对齐
//! 标尺手法)+ `paint_quad` 三段(底轨/填充/手柄)。

use std::rc::Rc;

use gpui::{
    App, Context, ElementId, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render,
    StatefulInteractiveElement, Styled, Window, canvas, div, px,
};

use crate::tokens::SpacingTokens;

/// 轨道高度(px)。
pub const TRACK_H_PX: f32 = 4.0;
/// 手柄直径(px);行容器 py 外扩后命中 ≥24px(A11Y-03)。
pub const HANDLE_D_PX: f32 = 14.0;
/// 行上下命中外扩(G-UI-F:尺寸常量单点)。
pub const SLIDER_HIT_PAD_PX: f32 = 5.0;

/// 回调形态。
pub type SliderChangeFn = Rc<dyn Fn(f64, &mut App)>;

/// 滑块(有状态 Entity,受控)。
pub struct Slider {
    value: f64,
    range: (f64, f64),
    step: f64,
    reset_to: f64,
    id: ElementId,
    on_change: SliderChangeFn,
    focus: Option<FocusHandle>,
    /// 轨道 bounds(prepaint 回写;命中换算基准)。
    track_bounds: Rc<std::cell::Cell<gpui::Bounds<Pixels>>>,
    semantic: crate::interact::Semantic,
}

/// 值 → 手柄中心比例 0..=1(纯函数;范围退化回落 0)。
#[must_use]
pub fn value_fraction(value: f64, range: (f64, f64)) -> f64 {
    let (lo, hi) = range;
    if hi <= lo {
        return 0.0;
    }
    ((value - lo) / (hi - lo)).clamp(0.0, 1.0)
}

/// 键盘步进纯函数(←↓ 减 / →↑ 加 / Home 起 / End 止;非法键 None)。
#[must_use]
pub fn slider_key(value: f64, key: &str, step: f64, range: (f64, f64)) -> Option<f64> {
    let next = match key {
        "left" | "down" => value - step,
        "right" | "up" => value + step,
        "home" => range.0,
        "end" => range.1,
        _ => return None,
    };
    Some(next.clamp(range.0, range.1))
}

/// 点击/拖拽 x 坐标 → 值(纯函数;bounds 为轨道屏区)。
#[must_use]
pub fn value_at_x(x: f64, bounds_w: f64, range: (f64, f64)) -> f64 {
    let (lo, hi) = range;
    if bounds_w <= 0.0 || hi <= lo {
        return lo;
    }
    let t = (x / bounds_w).clamp(0.0, 1.0);
    lo + t * (hi - lo)
}

impl Slider {
    /// 构造(值钳入默认 0..=100)。
    pub fn new(id: impl Into<ElementId>, value: f64) -> Self {
        Slider {
            value: value.clamp(0.0, 100.0),
            range: (0.0, 100.0),
            step: 1.0,
            reset_to: value.clamp(0.0, 100.0),
            id: id.into(),
            on_change: Rc::new(|_, _| {}),
            focus: None,
            track_bounds: Rc::new(std::cell::Cell::new(gpui::Bounds::default())),
            semantic: crate::interact::Semantic::new(),
        }
    }

    /// 取值范围。
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.range = (min, max);
        self.value = self.value.clamp(min, max);
        self
    }

    /// 键盘步长。
    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// 双击复位值(缺省 = 构造值)。
    pub fn reset_to(mut self, v: f64) -> Self {
        self.reset_to = v;
        self
    }

    /// 变化回执。
    pub fn on_change(mut self, f: impl Fn(f64, &mut App) + 'static) -> Self {
        self.on_change = Rc::new(f);
        self
    }

    /// 语义槽(A11Y-02;role 默认 Slider)。
    pub fn label_slot(mut self, label: impl Into<SharedString2>) -> Self {
        self.semantic = self.semantic.with_label(label.into().0);
        self
    }

    /// 宿主回写。
    pub fn set_value(&mut self, v: f64, cx: &mut Context<Self>) {
        let v = v.clamp(self.range.0, self.range.1);
        if v != self.value {
            self.value = v;
            cx.notify();
        }
    }

    /// 当前值。
    pub fn value(&self) -> f64 {
        self.value
    }

    fn emit(&self, cx: &mut Context<Self>) {
        let cb = self.on_change.clone();
        let v = self.value;
        cb(v, cx);
    }

    fn apply_point(&mut self, x: Pixels, window: &mut Window, cx: &mut Context<Self>) {
        let b = self.track_bounds.get();
        let x = f64::from(x - b.origin.x);
        let w = f64::from(b.size.width);
        let v = value_at_x(x, w, self.range);
        self.set_value(v, cx);
        self.emit(cx);
        window.prevent_default();
    }

    fn on_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_point(ev.position.x, window, cx);
    }

    fn on_drag(&mut self, ev: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if ev.dragging() {
            self.apply_point(ev.position.x, window, cx);
        }
    }

    fn on_up(&mut self, _ev: &MouseUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn on_click(&mut self, ev: &gpui::ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // 双击复位(click_count 判定;单击不动作——按下即设值由 on_down 承担)
        if ev.click_count() == 2 {
            self.set_value(self.reset_to, cx);
            self.emit(cx);
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(next) = slider_key(self.value, ev.keystroke.key.as_str(), self.step, self.range)
            && next != self.value
        {
            self.set_value(next, cx);
            self.emit(cx);
        }
    }
}

/// 语义标签包装(`label_slot` 入参;避免与 `SharedString` 直接重导出冲突)。
pub struct SharedString2(gpui::SharedString);
impl From<&'static str> for SharedString2 {
    fn from(s: &'static str) -> Self {
        Self(gpui::SharedString::from(s))
    }
}
impl From<String> for SharedString2 {
    fn from(s: String) -> Self {
        Self(gpui::SharedString::from(s))
    }
}

impl Render for Slider {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = crate::theme::theme(cx).colors;
        let (accent, surface_3) = (colors.accent, colors.surface_3);
        let frac = value_fraction(self.value, self.range) as f32;
        let track_bounds = self.track_bounds.clone();
        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let semantic = self
            .semantic
            .clone()
            .with_role(crate::interact::SemanticRole::Slider);

        let row = div()
            .id(self.id.clone())
            .w_full()
            .py(px(SLIDER_HIT_PAD_PX)) // 命中外扩(A11Y-03:pad+手柄 ≥24px 热区)
            .track_focus(&focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_down))
            .on_mouse_move(cx.listener(Self::on_drag))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up))
            .on_click(cx.listener(Self::on_click))
            .child(
                canvas(
                    {
                        let tb = track_bounds.clone();
                        move |b: gpui::Bounds<Pixels>, _, _| tb.set(b)
                    },
                    move |b: gpui::Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
                        // 底轨
                        window.paint_quad(gpui::fill(
                            gpui::Bounds {
                                origin: gpui::Point {
                                    x: b.origin.x,
                                    y: b.origin.y
                                        + (px(f32::from(b.size.height) / 2.0 - TRACK_H_PX / 2.0)),
                                },
                                size: gpui::Size {
                                    width: b.size.width,
                                    height: px(TRACK_H_PX),
                                },
                            },
                            surface_3,
                        ));
                        // 填充(accent,左 → 手柄)
                        let fill_w = px(f32::from(b.size.width) * frac);
                        window.paint_quad(gpui::fill(
                            gpui::Bounds {
                                origin: gpui::Point {
                                    x: b.origin.x,
                                    y: b.origin.y
                                        + (px(f32::from(b.size.height) / 2.0 - TRACK_H_PX / 2.0)),
                                },
                                size: gpui::Size {
                                    width: fill_w,
                                    height: px(TRACK_H_PX),
                                },
                            },
                            accent,
                        ));
                        // 手柄(圆点,中心在 frac)
                        let cxp = b.origin.x + px(f32::from(b.size.width) * frac);
                        let cyp = b.origin.y + px(f32::from(b.size.height) / 2.0);
                        window.paint_quad(gpui::fill(
                            gpui::Bounds {
                                origin: gpui::Point {
                                    x: cxp - px(HANDLE_D_PX / 2.0),
                                    y: cyp - px(HANDLE_D_PX / 2.0),
                                },
                                size: gpui::Size {
                                    width: px(HANDLE_D_PX),
                                    height: px(HANDLE_D_PX),
                                },
                            },
                            accent,
                        ));
                    },
                )
                .h(px(HANDLE_D_PX))
                .w_full(),
            );
        crate::interact::attach_semantics(
            div()
                .w_full()
                .child(row)
                .child(div().h(px(SpacingTokens::XS))),
            &semantic,
        )
    }
}
