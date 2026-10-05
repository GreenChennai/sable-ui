//! 选择控件三件套 Checkbox / Switch / Radio(迭代审查报告 §5.6 组件矩阵 #5,
//! CMP-01 批 1)。
//!
//! # 形态(kind 参数化的单一 Entity 组件)
//!
//! [`Choice`] 按 [`ChoiceKind`] 三态变体:**Checkbox** = 方框 + 对勾路径
//! (gpui [`gpui::PathBuilder`] 描边路径,非裸 gpui checkbox);**Switch** =
//! 胶囊轨道 + 滑块(位移走 STATE 120ms,`reduced_motion` 直通);**Radio** =
//! 圆环 + 圆点(同组互斥由宿主管理,组件只持自身 checked 态,键盘/点击
//! 只"选中"不"取消",宿主经 [`Choice::set_checked`] 落地互斥)。
//!
//! ```ignore
//! // 宿主侧(checkbox):
//! let cb = cx.new(|_| Choice::checkbox(false).label("自动保存").on_change(|checked, _cx| { .. }));
//! // radio 组(互斥在宿主):
//! let on_change = move |checked: bool, cx: &mut App| {
//!     if checked { /* 把同组其它 radio set_checked(false) */ }
//! };
//! ```
//!
//! # 视觉(报告 §5.3.3 / §5.9,TOK-04 / TOK-07)
//!
//! 状态→颜色映射集中在纯函数 [`choice_visual`](可测单点):选中 = accent
//! 令牌;三态 hover/press 走 [`crate::interact::state_layer`] alpha 叠加
//! (容器随 hover 以 [`crate::interact::HoverState`] 120ms 插值);焦点 =
//! accent 1.5px 外描边 + 内侧隔离环(黑 40% 经元素 `opacity` 落地,色取
//! `ELEVATION_SHADOW_TINT`,无颜色字面量);**禁用 = 仅前景降级**
//! ([`crate::interact::disabled_foreground`] → `text_disabled`),容器
//! 背景/描边与启用态逐位相同、不响应任何交互(TOK-07)。
//!
//! 对勾/滑块等图形前景由对比度裁决(纯函数 [`on_accent_glyph`] /
//! [`control_knob`]):在既有令牌候选中对 `accent`/`surface_2` 取对比度
//! 最高者,零硬编码色、深浅两主题自动成立(图形对比度 ≥3:1,TC-CMP-
//! CHOICE-01 断言)。
//!
//! # 键盘(A11Y,报告 §5.10.2)
//!
//! `track_focus` + `tab_stop(true)` 入 Tab 序;space/enter 切换(意图纯
//! 函数 [`key_checked_intent`]:Checkbox/Switch 翻转,Radio 只选不清)。
//! Radio 组内 ←/→ 巡航属宿主(组语义在宿主),组件不代管。
//!
//! # 时长(报告 §5.8 动效四档)
//!
//! 滑块位移/对勾淡入/圆点缩放 = [`crate::tokens::MotionTokens::DUR_STATE_MS`]
//! (120ms OutCubic);hover 插值 = [`crate::interact::HoverState`] 的 STATE
//! 档;`reduced_motion` 下全部直通目标值([`crate::anim::Animated`] 短路)。

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, Context, ElementId, FocusHandle, FontWeight, Hsla, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement,
    PathBuilder, Render, SharedString, StatefulInteractiveElement, Styled, Window, canvas, div,
    point, px,
};

use crate::anim::{Animated, Easing, lerp_hsla};
use crate::interact::{self, HoverState, InteractState, disabled_foreground, state_layer};
use crate::theme::theme;
use crate::tokens::{
    ColorTokens, HEIGHT_DEFAULT, RadiusTokens, SpacingTokens, TextSize, UI_FONT, contrast_ratio,
    control_height, h_flex,
};

// ---------------------------------------------------------------------------
// 几何常量(具名 + 注明依据;非令牌表的组件本体尺寸,同 number_field 惯例)
// ---------------------------------------------------------------------------

/// 方框/圆环边长(px):16 网格基准;命中区 ≥24px 由行高 [`hit_height`]
/// (派生制)保证(报告 §5.8 命中区红线)。
const CONTROL_BOX_PX: f32 = 16.0;
/// 对勾描边宽(px):对齐 §5.5 图标描边口径的加粗档(1.5 在 16px 盒内过细,
/// 取 2 保证可读)。
const CHECK_STROKE_PX: f32 = 2.0;
/// 对勾路径起笔(方框边长分数)。
const CHECK_P0: (f32, f32) = (0.25, 0.55);
/// 对勾路径折点(方框边长分数)。
const CHECK_P1: (f32, f32) = (0.42, 0.72);
/// 对勾路径收笔(方框边长分数)。
const CHECK_P2: (f32, f32) = (0.78, 0.30);
/// Switch 轨道宽(px)。
const SWITCH_TRACK_W_PX: f32 = 32.0;
/// Switch 轨道高(px)。
const SWITCH_TRACK_H_PX: f32 = 16.0;
/// Switch 滑块直径(px)。
const SWITCH_KNOB_D_PX: f32 = 12.0;
/// Radio 圆点满径(px;选中进度 1.0 时)。
const RADIO_DOT_D_PX: f32 = 8.0;
/// 选择控件变体(组件渲染与键盘意图的共同参数)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChoiceKind {
    /// 勾选框:方框 + 对勾;space/enter 翻转。
    Checkbox,
    /// 开关:胶囊轨道 + 滑块;space/enter 翻转。
    Switch,
    /// 单选:圆环 + 圆点;space/enter 只选不清(互斥由宿主管理)。
    Radio,
}

// ---------------------------------------------------------------------------
// 纯函数层(状态→视觉映射、键盘意图、几何;全部可无 App 单测)
// ---------------------------------------------------------------------------

/// 三态交互相位([`choice_visual`] 的输入;focus 单独布尔传入)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlPhase {
    /// 静止
    Idle,
    /// 悬停
    Hover,
    /// 按下
    Pressed,
}

/// 选择控件一帧的完整视觉裁决(纯函数输出)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlVisual {
    /// 容器底(方框/胶囊/圆盘):未选 = `surface_2`(L2 凸起),选中 = accent
    pub track: Hsla,
    /// 容器描边:未选 = `border_subtle`(hover/press 提到 `border_strong`),
    /// 选中 = accent
    pub border: Hsla,
    /// 图形前景(对勾/滑块/圆点):未勾选的 Checkbox/Radio 为全透明
    pub glyph: Hsla,
    /// 标签文字(启用 = `text_primary`,禁用 = `text_disabled`)
    pub label: Hsla,
    /// 焦点环(聚焦且启用时 = Some(accent);报告 §5.3.3)
    pub ring: Option<Hsla>,
}

/// 状态→视觉映射(纯函数,TOK-04/TOK-07 的组件落地单点):
///
/// - 未选容器 = `surface_2` + `border_subtle`;选中容器 = accent(底与描边);
/// - hover/press = [`state_layer`] alpha 叠加(深色叠白/浅色叠黑,深浅两主题
///   自动取向);禁用分支**恒等直通容器**(容器不变,仅前景降级);
/// - 焦点环只在"聚焦且启用"时出现;禁用永远无环(不可交互不诱导)。
#[must_use]
pub fn choice_visual(
    colors: &ColorTokens,
    kind: ChoiceKind,
    checked: bool,
    phase: ControlPhase,
    focused: bool,
    disabled: bool,
) -> ControlVisual {
    let accent = colors.accent;
    let (track_base, border_base) = if checked {
        (accent, accent)
    } else {
        (colors.surface_2, colors.border_subtle)
    };
    // 图形前景(启用态):Switch 滑块恒在;Checkbox/Radio 仅选中时出现
    let glyph_enabled = match kind {
        ChoiceKind::Checkbox => checked.then(|| on_accent_glyph(colors)),
        ChoiceKind::Switch => Some(control_knob(colors)),
        ChoiceKind::Radio => checked.then_some(accent),
    };
    if disabled {
        // TOK-07:容器不变(底/描边与静止态逐位同源),仅前景降级
        return ControlVisual {
            track: track_base,
            border: border_base,
            glyph: glyph_enabled
                .map(|g| disabled_foreground(g, colors.text_disabled))
                .unwrap_or_else(Hsla::transparent_black),
            label: disabled_foreground(colors.text_primary, colors.text_disabled),
            ring: None,
        };
    }
    let (track, border) = match phase {
        ControlPhase::Idle => (track_base, border_base),
        ControlPhase::Hover => (
            state_layer(track_base, InteractState::Hover, accent),
            if checked {
                accent
            } else {
                colors.border_strong
            },
        ),
        ControlPhase::Pressed => (
            state_layer(track_base, InteractState::Pressed, accent),
            if checked {
                accent
            } else {
                colors.border_strong
            },
        ),
    };
    ControlVisual {
        track,
        border,
        glyph: glyph_enabled.unwrap_or_else(Hsla::transparent_black),
        label: colors.text_primary,
        ring: focused.then_some(accent),
    }
}

/// 强调色上的图形前景(对勾等;纯函数):在 `surface_0`/`text_strong` 两个
/// 既有令牌中,对 accent 对比度更高者——深色主题选深极、浅色主题选浅极,
/// 保证图形 ≥3:1(深色 accent 上白对勾仅 2.7:1,故按对比度裁决而非惯例)。
#[must_use]
pub fn on_accent_glyph(colors: &ColorTokens) -> Hsla {
    pick_by_contrast([colors.surface_0, colors.text_strong], colors.accent)
}

/// 控件滑块/旋钮填充(纯函数):在 `surface_1`/`text_strong` 两个既有令牌中,
/// 对未选轨道 `surface_2` 对比度更高者——深浅两主题均裁决为"亮色滑块",
/// 滑块横跨未选灰轨与选中 accent 轨两种底,取对未选轨道的最强对比锚定形态。
#[must_use]
pub fn control_knob(colors: &ColorTokens) -> Hsla {
    pick_by_contrast([colors.surface_1, colors.text_strong], colors.surface_2)
}

/// 二选一对比度裁决(纯函数):对 `against` 对比度更高者;持平取前者。
#[must_use]
fn pick_by_contrast(candidates: [Hsla; 2], against: Hsla) -> Hsla {
    let a = contrast_ratio(candidates[0], against);
    let b = contrast_ratio(candidates[1], against);
    if a >= b { candidates[0] } else { candidates[1] }
}

/// 键盘意图(纯函数,键盘状态机单点):`space`/`enter` → 目标 checked;
/// 其余键 `None`。Checkbox/Switch = 翻转;**Radio 只选不清**(Some(true)
/// 恒成立,取消交给宿主互斥逻辑)。
#[must_use]
pub fn key_checked_intent(kind: ChoiceKind, checked: bool, key: &str) -> Option<bool> {
    if !matches!(key, "space" | "enter") {
        return None;
    }
    Some(match kind {
        ChoiceKind::Radio => true,
        ChoiceKind::Checkbox | ChoiceKind::Switch => !checked,
    })
}

/// Switch 滑块几何(纯函数):进度 0..1 → 滑块左缘 x(px)。行程 =
/// `track_width - knob - 2×inset`,两端精确落位,进度越界被钳制。
#[must_use]
pub fn switch_knob_offset(track_width: f32, knob_diameter: f32, inset: f32, progress: f64) -> f32 {
    let travel = (track_width - knob_diameter - 2.0 * inset).max(0.0);
    inset + travel * progress.clamp(0.0, 1.0) as f32
}

/// Radio 圆点直径(纯函数):选中进度 0..1 → 直径(px),越界钳制。
#[must_use]
pub fn radio_dot_diameter(full_diameter: f32, progress: f64) -> f32 {
    full_diameter * progress.clamp(0.0, 1.0) as f32
}

/// Switch 滑块内衬(px):轨道高与滑块直径的居中差的一半。
#[must_use]
pub fn switch_knob_inset() -> f32 {
    (SWITCH_TRACK_H_PX - SWITCH_KNOB_D_PX) / 2.0
}

/// 命中行高(px,派生制):`max(26, 18 + 2×4) = 26` ≥ 24px 命中区红线
/// (报告 §5.8;紧凑盒 16px 只是图形,热区以整行为准)。
#[must_use]
pub fn hit_height() -> f32 {
    control_height(
        HEIGHT_DEFAULT,
        TextSize::LABEL.line_height,
        SpacingTokens::XS,
    )
}

/// 焦点环(报告 §5.3.3,A11Y 批次):实现单点在
/// [`crate::interact::focus_ring`](内层隔离环 + accent 外描边两层),本组件
/// 只是消费方;`radius` 随变体取 [`ring_radius_for`]。
fn ring_children(ring: Option<Hsla>, radius: f32) -> impl IntoIterator<Item = AnyElement> {
    ring.map(|accent| crate::interact::focus_ring(accent, radius))
        .into_iter()
        .flatten()
}

/// 焦点环圆角随变体:方框 = SM;胶囊/圆 = LG(8 = 轨道高一半)。
#[must_use]
fn ring_radius_for(kind: ChoiceKind) -> f32 {
    match kind {
        ChoiceKind::Checkbox => RadiusTokens::SM,
        ChoiceKind::Switch | ChoiceKind::Radio => RadiusTokens::LG,
    }
}

// ---------------------------------------------------------------------------
// 组件(builder + Entity,形态对齐 NumberField)
// ---------------------------------------------------------------------------

/// 选中变化回调(受控回执,同 `effect_stack::ClickFn` 的别名惯例):
/// `fn(checked, &mut App)`——点击/键盘触发的变化都经此上报宿主。
pub type ChoiceChangeFn = Rc<dyn Fn(bool, &mut App)>;

/// 选择控件(kind 参数化的 Checkbox / Switch / Radio;Entity 组件):
/// `cx.new(|_| Choice::switch(false).label("预览").on_change(|on, _cx| { .. }))`。
pub struct Choice {
    kind: ChoiceKind,
    checked: bool,
    label: Option<SharedString>,
    disabled: bool,
    /// 按下(down 置位、up/up_out 清除)
    pressed: bool,
    /// 悬停进度(120ms ease-out;禁用冻结)
    hover: HoverState,
    /// 选中进度 0..1(STATE 120ms OutCubic;驱动对勾淡入/滑块位移/圆点缩放
    /// 的单一动画真相;`reduced_motion` 直通)
    sel: Animated<f64>,
    /// 焦点句柄(首帧惰性创建,`tab_stop(true)` 进 Tab 环游)
    focus: Option<FocusHandle>,
    /// 选中变化回调(受控回执:宿主据此落地 radio 组互斥等)
    on_change: Option<ChoiceChangeFn>,
    /// 元素 id(on_hover 动画需要 Stateful 元素;同屏多实例各给唯一 id,
    /// 未给则退化为 gpui hover 即时切换)
    element_id: Option<ElementId>,
}

impl Choice {
    /// 勾选框(初值 `checked`)。
    pub fn checkbox(checked: bool) -> Self {
        Self::new(ChoiceKind::Checkbox, checked)
    }

    /// 开关(初值 `checked`)。
    pub fn switch(checked: bool) -> Self {
        Self::new(ChoiceKind::Switch, checked)
    }

    /// 单选钮(初值 `checked`;同组互斥由宿主管理)。
    pub fn radio(checked: bool) -> Self {
        Self::new(ChoiceKind::Radio, checked)
    }

    fn new(kind: ChoiceKind, checked: bool) -> Self {
        Choice {
            kind,
            checked,
            label: None,
            disabled: false,
            pressed: false,
            hover: HoverState::new(),
            sel: Animated::new(if checked { 1.0 } else { 0.0 }),
            focus: None,
            on_change: None,
            element_id: None,
        }
    }

    /// 变体种类。
    #[must_use]
    pub fn kind(&self) -> ChoiceKind {
        self.kind
    }

    /// 当前选中态。
    #[must_use]
    pub fn is_checked(&self) -> bool {
        self.checked
    }

    /// 标签文本(可选;`TextSize::LABEL` 排版档)。**可见标签即可访问名**
    /// (A11Y-02:读屏名称槽与视觉文本单源,gpui 0.2.2 无语义树、存态消费
    /// 待 TD-01,见 [`crate::interact::Semantic`])。
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// 语义槽(A11Y-02,只读访问):label = 可见标签;role 由 kind 映射
    /// (Checkbox/Switch/Radio,[`Self::semantic_role`])。
    #[must_use]
    pub fn semantic(&self) -> crate::interact::Semantic {
        let sem = crate::interact::Semantic::new().with_role(self.semantic_role());
        match &self.label {
            Some(text) => sem.with_label(text.clone()),
            None => sem,
        }
    }

    /// 组件类型 → 语义角色映射(A11Y-02):Checkbox/Switch/Radio。
    #[must_use]
    pub fn semantic_role(&self) -> crate::interact::SemanticRole {
        match self.kind {
            ChoiceKind::Checkbox => crate::interact::SemanticRole::Checkbox,
            ChoiceKind::Switch => crate::interact::SemanticRole::Switch,
            ChoiceKind::Radio => crate::interact::SemanticRole::Radio,
        }
    }

    /// 禁用态(TOK-07):交互全门控(点击/键盘/悬停动画),视觉仅前景降级、
    /// 容器不变(见 [`choice_visual`] 的 disabled 分支)。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 元素 id(悬停动画需要 Stateful 元素;同屏多实例各给唯一 id)。
    pub fn element_id(mut self, id: impl Into<ElementId>) -> Self {
        self.element_id = Some(id.into());
        self
    }

    /// 选中变化回调:`on_change(checked, &mut App)`。点击/键盘触发的变化
    /// 都经此回执;宿主可在此落地 radio 组互斥(对同组其余实例调
    /// [`Self::set_checked`] = false)。
    pub fn on_change(mut self, f: impl Fn(bool, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
    /// 宿主受控写入(radio 组互斥的落地通道):状态变化时以 STATE 时长动画
    /// 过渡并通知;同值幂等。不触发 [`Self::on_change`](回调是"组件 → 宿主"
    /// 的上报通道,宿主回写不应回环)。
    pub fn set_checked(&mut self, checked: bool, cx: &mut Context<Self>) {
        if checked == self.checked {
            return;
        }
        self.checked = checked;
        self.sel.set(
            if checked { 1.0 } else { 0.0 },
            // STATE 档(毫秒令牌 ÷ 1000 → Duration,令牌以毫秒计)
            Duration::from_secs_f64(crate::tokens::MotionTokens::DUR_STATE_MS / 1000.0),
            Easing::OutCubic,
        );
        cx.notify();
    }

    /// 激活(点击/键盘共用):禁用门控 → 按变体语义推进状态(radio 已选中
    /// 再点无变化)→ 动画 → 上报。
    fn activate(&mut self, cx: &mut Context<Self>) {
        if self.disabled {
            return; // TOK-07:禁用即交互门控
        }
        let next = match self.kind {
            ChoiceKind::Radio if self.checked => None, // 已选中:无变化
            _ => Some(!self.checked),
        };
        if let Some(next) = next {
            self.set_checked(next, cx);
            if let Some(cb) = self.on_change.clone() {
                cb(self.checked, cx);
            }
        }
    }

    fn on_mouse_down(
        &mut self,
        _event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        self.pressed = true;
        self.activate(cx);
    }

    fn on_mouse_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.pressed {
            self.pressed = false;
            cx.notify();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if key_checked_intent(self.kind, self.checked, &event.keystroke.key).is_some() {
            self.activate(cx);
        }
    }

    /// A7 悬停进出:驱动 [`HoverState`] 过渡并请求重绘。禁用态忽略。
    fn on_hover_changed(&mut self, hovered: &bool, _window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let now = interact::now_ms();
        if *hovered {
            self.hover.on_enter(now);
        } else {
            self.hover.on_leave(now);
        }
        cx.notify();
    }
}

impl Render for Choice {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &theme(cx).colors;
        let kind = self.kind;
        let disabled = self.disabled;
        let now = interact::now_ms();
        let hover_progress = if disabled {
            0.0
        } else {
            self.hover.progress_at(now)
        };
        let sel_progress = self.sel.value_at(Instant::now());
        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let focused = focus.is_focused(window);

        // 视觉:press 即时;否则 idle↔hover 按 HoverState 进度插值(120ms)
        let idle = choice_visual(
            colors,
            kind,
            self.checked,
            ControlPhase::Idle,
            focused,
            disabled,
        );
        let vis = if disabled || self.pressed {
            choice_visual(
                colors,
                kind,
                self.checked,
                ControlPhase::Pressed,
                focused,
                disabled,
            )
        } else {
            let hover = choice_visual(
                colors,
                kind,
                self.checked,
                ControlPhase::Hover,
                focused,
                disabled,
            );
            ControlVisual {
                track: lerp_hsla(idle.track, hover.track, hover_progress),
                border: lerp_hsla(idle.border, hover.border, hover_progress),
                ..idle
            }
        };
        let glyph = vis.glyph;

        // 控件本体(kind 变体自绘;对勾经 canvas + 描边路径,滑块/圆点为 div)
        let control = match kind {
            ChoiceKind::Checkbox => {
                div()
                    .relative()
                    .w(px(CONTROL_BOX_PX))
                    .h(px(CONTROL_BOX_PX))
                    .rounded(px(RadiusTokens::SM))
                    .border_1()
                    .border_color(vis.border)
                    .bg(vis.track)
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, _| {
                                if sel_progress <= 0.0 {
                                    return; // 未选中:不画对勾
                                }
                                // 对勾路径(方框内分数坐标 → 窗口绝对坐标)
                                let ox = f32::from(bounds.origin.x);
                                let oy = f32::from(bounds.origin.y);
                                let w = f32::from(bounds.size.width);
                                let h = f32::from(bounds.size.height);
                                let pt = |fx: f32, fy: f32| point(px(ox + w * fx), px(oy + h * fy));
                                let mut builder = PathBuilder::stroke(px(CHECK_STROKE_PX));
                                builder.move_to(pt(CHECK_P0.0, CHECK_P0.1));
                                builder.line_to(pt(CHECK_P1.0, CHECK_P1.1));
                                builder.line_to(pt(CHECK_P2.0, CHECK_P2.1));
                                if let Ok(path) = builder.build() {
                                    let mut color = glyph;
                                    color.a *= sel_progress.clamp(0.0, 1.0) as f32; // 选中淡入
                                    window.paint_path(path, color);
                                }
                            },
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .children(ring_children(vis.ring, ring_radius_for(kind)))
                    .into_any_element()
            }
            ChoiceKind::Switch => {
                let inset = switch_knob_inset();
                div()
                    .relative()
                    .w(px(SWITCH_TRACK_W_PX))
                    .h(px(SWITCH_TRACK_H_PX))
                    .rounded_full()
                    .border_1()
                    .border_color(vis.border)
                    .bg(vis.track)
                    .child(
                        div()
                            .absolute()
                            .rounded_full()
                            .bg(vis.glyph)
                            .w(px(SWITCH_KNOB_D_PX))
                            .h(px(SWITCH_KNOB_D_PX))
                            .top(px(inset))
                            .left(px(switch_knob_offset(
                                SWITCH_TRACK_W_PX,
                                SWITCH_KNOB_D_PX,
                                inset,
                                sel_progress,
                            ))),
                    )
                    .children(ring_children(vis.ring, ring_radius_for(kind)))
                    .into_any_element()
            }
            ChoiceKind::Radio => {
                let dot = px(radio_dot_diameter(RADIO_DOT_D_PX, sel_progress));
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(CONTROL_BOX_PX))
                    .h(px(CONTROL_BOX_PX))
                    .rounded_full()
                    .border_1()
                    .border_color(vis.border)
                    .bg(vis.track)
                    .child(div().rounded_full().bg(vis.glyph).w(dot).h(dot))
                    .children(ring_children(vis.ring, ring_radius_for(kind)))
                    .into_any_element()
            }
        };

        // 命中行:控件 + 可选标签(LABEL 档),行高派生 ≥ 24px 命中区
        let row = h_flex()
            .relative()
            .gap(px(SpacingTokens::SM))
            .h(px(hit_height()))
            .child(control)
            .children(self.label.clone().map(|text| {
                div()
                    .font_family(UI_FONT)
                    .text_size(px(TextSize::LABEL.size))
                    .font_weight(FontWeight(TextSize::LABEL.weight))
                    .text_color(vis.label)
                    .child(text)
            }));

        // 事件与焦点(id/hover 动画策略与 NumberField 同款:无 id 退化为
        // gpui hover 即时切换;禁用不挂任何交互样式)
        let mut root = row
            .track_focus(&focus)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_key_down(cx.listener(Self::on_key_down));
        if !disabled {
            root = root.cursor_pointer();
            if self.element_id.is_none() {
                let hover_bg = state_layer(colors.surface_1, InteractState::Hover, colors.accent);
                root = root.hover(move |style| style.bg(hover_bg));
            }
        }

        // 动画帧泵:悬停插值或选中动画进行中才续帧(静止零帧提交)
        if (!disabled && self.hover.is_running(now)) || self.sel.is_running_at(Instant::now()) {
            window.request_animation_frame();
        }

        match self.element_id.clone() {
            Some(id) => root
                .id(id)
                .on_hover(cx.listener(Self::on_hover_changed))
                .into_any_element(),
            None => root.into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TC-CMP-CHOICE-01(三组件四态 + 禁用映射断言):
    /// 选中 accent / 未选凸起;hover/press 相位互异;
    /// 禁用 = 容器逐位不变、前景收敛 text_disabled、无焦点环、不响应 hover;
    /// 焦点环只在"聚焦且启用"出现且值 = accent 实色。
    #[test]
    fn tc_cmp_choice_01_phase_disabled_and_focus_mapping() {
        for colors in [ColorTokens::dark(), ColorTokens::light()] {
            for kind in [ChoiceKind::Checkbox, ChoiceKind::Switch, ChoiceKind::Radio] {
                for checked in [false, true] {
                    let idle =
                        choice_visual(&colors, kind, checked, ControlPhase::Idle, false, false);
                    let hover =
                        choice_visual(&colors, kind, checked, ControlPhase::Hover, false, false);
                    let pressed =
                        choice_visual(&colors, kind, checked, ControlPhase::Pressed, false, false);
                    // 容器档位:未选 = surface_2 + border_subtle,选中 = accent
                    assert_eq!(
                        idle.track,
                        if checked {
                            colors.accent
                        } else {
                            colors.surface_2
                        }
                    );
                    assert_eq!(
                        idle.border,
                        if checked {
                            colors.accent
                        } else {
                            colors.border_subtle
                        }
                    );
                    // 三态互异(hover 可见、press 区别于 hover)
                    assert_ne!(hover.track, idle.track, "hover 必须可见变化");
                    assert_ne!(pressed.track, hover.track, "press 必须区别于 hover");
                    // 焦点环:聚焦 + 启用才出现,值 = accent 实色
                    let focused =
                        choice_visual(&colors, kind, checked, ControlPhase::Idle, true, false);
                    assert_eq!(idle.ring, None);
                    assert_eq!(focused.ring, Some(colors.accent));
                    // 禁用:容器与启用静止态逐位相同(仅前景降级)、无环
                    let disabled =
                        choice_visual(&colors, kind, checked, ControlPhase::Idle, true, true);
                    assert_eq!(disabled.track, idle.track, "禁用容器不变");
                    assert_eq!(disabled.border, idle.border, "禁用描边不变");
                    assert_eq!(disabled.label, colors.text_disabled, "标签前景降级");
                    assert_eq!(disabled.ring, None, "禁用无焦点环");
                    let disabled_hover =
                        choice_visual(&colors, kind, checked, ControlPhase::Hover, true, true);
                    assert_eq!(disabled_hover.track, idle.track, "禁用不响应 hover");
                }
            }
        }
    }

    /// TC-CMP-CHOICE-01(图形前景语义):Checkbox 对勾 = 对比度裁决色且仅
    /// 选中出现;Switch 滑块恒在;Radio 圆点 = accent 且仅选中出现;禁用时
    /// 可见图形全部收敛 text_disabled。
    #[test]
    fn tc_cmp_choice_01_glyph_semantics_per_kind() {
        for colors in [ColorTokens::dark(), ColorTokens::light()] {
            for kind in [ChoiceKind::Checkbox, ChoiceKind::Switch, ChoiceKind::Radio] {
                let off = choice_visual(&colors, kind, false, ControlPhase::Idle, false, false);
                let on = choice_visual(&colors, kind, true, ControlPhase::Idle, false, false);
                match kind {
                    ChoiceKind::Checkbox => {
                        assert_eq!(off.glyph.a, 0.0, "未勾选无对勾");
                        assert_eq!(on.glyph, on_accent_glyph(&colors));
                    }
                    ChoiceKind::Switch => {
                        assert_eq!(off.glyph, control_knob(&colors), "滑块恒在");
                        assert_eq!(on.glyph, control_knob(&colors));
                    }
                    ChoiceKind::Radio => {
                        assert_eq!(off.glyph.a, 0.0, "未选中无圆点");
                        assert_eq!(on.glyph, colors.accent, "选中圆点 = accent");
                    }
                }
                let on_disabled =
                    choice_visual(&colors, kind, true, ControlPhase::Idle, false, true);
                assert_eq!(
                    on_disabled.glyph, colors.text_disabled,
                    "可见图形禁用后 = text_disabled"
                );
            }
        }
    }

    /// TC-CMP-CHOICE-01(键盘切换状态机):space/enter 驱动三变体;Radio
    /// 只选不清;其余键无意图。
    #[test]
    fn tc_cmp_choice_01_keyboard_toggle_state_machine() {
        for key in ["space", "enter"] {
            assert_eq!(
                key_checked_intent(ChoiceKind::Checkbox, false, key),
                Some(true),
                "Checkbox {key}: 勾上"
            );
            assert_eq!(
                key_checked_intent(ChoiceKind::Checkbox, true, key),
                Some(false),
                "Checkbox {key}: 取消"
            );
            assert_eq!(
                key_checked_intent(ChoiceKind::Switch, true, key),
                Some(false),
                "Switch {key}: 翻转"
            );
            assert_eq!(
                key_checked_intent(ChoiceKind::Radio, false, key),
                Some(true),
                "Radio {key}: 只选"
            );
            assert_eq!(
                key_checked_intent(ChoiceKind::Radio, true, key),
                Some(true),
                "Radio {key}: 已选不取消(互斥归宿主)"
            );
        }
        for key in ["up", "down", "left", "right", "escape", "tab", "a"] {
            assert_eq!(
                key_checked_intent(ChoiceKind::Checkbox, false, key),
                None,
                "{key} 无切换意图"
            );
        }
    }

    /// TC-CMP-CHOICE-01(几何状态机):Switch 滑块行程两端精确、逐进度单调、
    /// 越界钳制;Radio 圆点随进度缩放。
    #[test]
    fn tc_cmp_choice_01_switch_knob_and_radio_dot_geometry() {
        let inset = switch_knob_inset();
        assert_eq!(inset, 2.0, "(16-12)/2 = 2,胶囊内衬");
        assert_eq!(
            switch_knob_offset(SWITCH_TRACK_W_PX, SWITCH_KNOB_D_PX, inset, 0.0),
            inset,
            "进度 0 落左端"
        );
        assert_eq!(
            switch_knob_offset(SWITCH_TRACK_W_PX, SWITCH_KNOB_D_PX, inset, 1.0),
            SWITCH_TRACK_W_PX - SWITCH_KNOB_D_PX - inset,
            "进度 1 落右端(行程 16px)"
        );
        let mut prev = -1.0_f32;
        for i in 0..=20 {
            let p = f64::from(i) / 20.0;
            let x = switch_knob_offset(SWITCH_TRACK_W_PX, SWITCH_KNOB_D_PX, inset, p);
            assert!(x >= prev, "滑块位置随进度单调:{x} < {prev}");
            prev = x;
        }
        assert_eq!(
            switch_knob_offset(SWITCH_TRACK_W_PX, SWITCH_KNOB_D_PX, inset, 2.0),
            SWITCH_TRACK_W_PX - SWITCH_KNOB_D_PX - inset,
            "进度越界钳右端"
        );
        assert_eq!(
            switch_knob_offset(20.0, 24.0, 2.0, 1.0),
            2.0,
            "行程负值防御(轨道小于滑块)"
        );
        assert_eq!(radio_dot_diameter(RADIO_DOT_D_PX, 0.0), 0.0);
        assert_eq!(radio_dot_diameter(RADIO_DOT_D_PX, 1.0), RADIO_DOT_D_PX);
        assert_eq!(
            radio_dot_diameter(RADIO_DOT_D_PX, 0.5),
            RADIO_DOT_D_PX / 2.0,
            "圆点随选中进度缩放"
        );
    }

    /// TC-CMP-CHOICE-01(对比度保证):对勾对 accent、滑块对未选轨道、
    /// 圆点(accent)对轨道,深浅两主题均过图形 3:1 线;滑块对轨道 ≥4.5。
    #[test]
    fn tc_cmp_choice_01_glyph_contrast_guarantees() {
        for colors in [ColorTokens::dark(), ColorTokens::light()] {
            let glyph = on_accent_glyph(&colors);
            assert!(
                contrast_ratio(glyph, colors.accent) >= 3.0,
                "对勾对选中底 ≥3:1,得 {}",
                contrast_ratio(glyph, colors.accent)
            );
            let knob = control_knob(&colors);
            let knob_track = contrast_ratio(knob, colors.surface_2);
            assert!(knob_track >= 4.5, "滑块对未选轨道 ≥4.5:1,得 {knob_track}");
            let dot_track = contrast_ratio(colors.accent, colors.surface_2);
            assert!(dot_track >= 3.0, "Radio 圆点对轨道 ≥3:1,得 {dot_track}");
        }
    }

    /// TC-CMP-CHOICE-01(builder → 字段链路,同 NumberField 测试风格):
    /// 三构造器落 kind,disabled/label 存位,初始 checked 落位且动画目标同步。
    #[test]
    fn tc_cmp_choice_01_builders_shape_three_components() {
        let cb = Choice::checkbox(true).label("自动保存").disabled(true);
        assert_eq!(cb.kind(), ChoiceKind::Checkbox);
        assert!(cb.is_checked() && cb.disabled && cb.label.is_some());
        let sw = Choice::switch(false);
        assert_eq!(sw.kind(), ChoiceKind::Switch);
        assert!(!sw.is_checked() && !sw.disabled);
        let radio = Choice::radio(true);
        assert_eq!(radio.kind(), ChoiceKind::Radio);
        assert!(radio.is_checked());
        // 初始 checked 的动画目标从 1 起步(选中态实例不闪入场动画)
        assert_eq!(radio.sel.target(), &1.0);
        assert_eq!(sw.sel.target(), &0.0);
    }
}
