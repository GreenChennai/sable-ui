//! 基础按钮族(迭代审查报告 2026-10-04 CMP-01 批 1,§5.6 组件矩阵 #1/#2):
//! [`Button`](文本/图标+文本)+ [`IconButton`](纯图标)。
//!
//! # 两种形态(同一套视觉规格,单一实现源)
//!
//! - **Entity 形态**(规范形态,NumberField 同款 builder + `cx.new`):
//!   自持跨帧交互态——press 缩放弹簧([`PRESS_SCALE`] + SNAPPY 回弹,anim
//!   引擎,reduced_motion 直通)、120ms hover 插值
//!   ([`crate::interact::HoverState`])、焦点环(`track_focus` + accent
//!   描边)。宿主一次创建、随视图驻留:
//!   `cx.new(|_| Button::new("ok", "确定").variant(ButtonVariant::Primary))`;
//! - **内联形态**([`button_element`]/[`icon_button_element`]):把同一规格
//!   直接渲染为元素,**无跨帧状态**——hover/press 走 gpui 即时伪类
//!   (state-layer 即时切换,本仓 RenderOnce 面板同款),无弹簧缩放、
//!   无焦点环(焦点体系 = 第 4 组 A11Y 批次)。供 RenderOnce 面板与
//!   `uniform_list` 行等"逐帧重建"场景使用:逐帧/逐行建实体是反模式
//!   (CMP-06),实体驻留亦无意义,故 CMP-11 四处收口点全部走本形态。
//!
//! # 视觉规格 = 纯函数(可测,TOK 纪律)
//!
//! 状态 → 样式的映射全部独立为纯函数:[`button_style`](Idle/Hover/Pressed/
//! FocusRing/Disabled 的 bg/fg/border 裁决)、[`button_height`](派生制:
//! `max(档位下限, 行高 + 2×垂直 padding)`,CJK/大字号/DPI 安全)、
//! [`solid_variant_foreground`](实心变体前景:在两枚极性相反的令牌候选中
//! 取对比度更高者,**零新增令牌**)。所有颜色取自 [`ColorTokens`](零硬编码
//! 色),状态叠加一律走 [`crate::interact::state_layer`](TOK-04)。
//!
//! # 禁用态(TOK-07,报告 §5.9)
//!
//! **禁用 = 仅前景降级、容器不变**:[`button_style`] 的
//! [`InteractState::Disabled`] 分支容器(bg/border/几何)与 Idle **逐位
//! 相同**,前景经 [`crate::interact::disabled_foreground`] 降到
//! `text_disabled`;不挂任何监听(无热区、无 hover/press 反馈)。
//! 门禁:TC-CMP-BTN-01 的容器逐位断言。
//!
//! # 尺寸与内容(§5.6 #1/#2 规格要点)
//!
//! - [`ButtonSize`] 三档 Compact/Default/Roomy,高度走
//!   [`crate::tokens::control_height`] 派生制;文字档 Compact/Default =
//!   [`TextSize::LABEL`],Roomy = [`TextSize::BODY`];
//! - 内容支持 文本 / 图标+文本(图标 = 文字字形占位,SVG 图标系统 =
//!   §5.5 后续批次;`Button::icon` 槽位已定型),图标与文本间距 XS;
//! - [`IconButtonSize`] 两档 20/24([`ICON_BUTTON_SM_PX`]/[`ICON_BUTTON_LG_PX`]),
//!   方形、无文字;**tooltip 槽位现在定型**([`IconButton::tooltip`] +
//!   [`IconButton::tooltip_shortcut`],`SharedString` 存储):本批经 gpui
//!   原生 tooltip 机制挂最简浮层占位(禁用态同样显示——§5.9"禁用态
//!   tooltip 说明原因"),批 2 的 `Tooltip` 组件接管渲染(延迟 400ms/
//!   L4 材质/`key_badge_text` 单源)时**不改此签名**。

use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Div, ElementId, FocusHandle, FontWeight,
    Hsla, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Render,
    SharedString, Stateful, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::anim::{Spring, lerp_hsla, reduced_motion};
use crate::interact::{
    self, HoverState, InteractState, Semantic, SemanticRole, disabled_foreground, hit_size,
    semantic_slot, state_layer,
};
use crate::theme::theme;
use crate::tokens::{
    ColorTokens, HEIGHT_COMPACT, HEIGHT_DEFAULT, HEIGHT_LOOSE, RadiusTokens, SpacingTokens,
    TextSize, contrast_ratio, control_height, h_flex,
};

// ---------------------------------------------------------------------------
// 尺寸单点(常量域;颜色一律令牌,几何一律 tokens/本文件常量,零魔法数)
// ---------------------------------------------------------------------------

/// press 缩放下限(§5.6 #1:press scale 0.94,SNAPPY 弹簧回弹到 1.0)。
pub const PRESS_SCALE: f32 = 0.94;
/// Compact 档垂直内边距(`HEIGHT_COMPACT 22 = LABEL 行高 18 + 2×2`)。
const V_PAD_COMPACT: f32 = 2.0;
/// Default 档垂直内边距(`HEIGHT_DEFAULT 26 = 18 + 2×4`;= XS 档)。
const V_PAD_DEFAULT: f32 = SpacingTokens::XS;
/// Roomy 档垂直内边距(`HEIGHT_LOOSE 32 = BODY 行高 20 + 2×6`)。
const V_PAD_ROOMY: f32 = 6.0;
/// IconButton 小档边长(20px;§5.6 #2 规格)。
pub const ICON_BUTTON_SM_PX: f32 = 20.0;
/// IconButton 大档边长(24px)。
pub const ICON_BUTTON_LG_PX: f32 = 24.0;
/// 弹簧"已落定"的位移阈值(单位弹簧值与目标的距离小于它即停帧;
/// 0.005 ≈ 半个像素,视觉静止)。
const SPRING_SETTLE_EPS: f64 = 0.005;

// ---------------------------------------------------------------------------
// 变体 / 尺寸 / 回调
// ---------------------------------------------------------------------------

/// 按钮变体(§5.6 #1:primary/secondary/ghost/danger;色全部语义令牌)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ButtonVariant {
    /// 实心强调(accent 底;主行动/选中)
    Primary,
    /// 常规(surface_3 凸起底 + 细描边;工具行/面板默认)
    Secondary,
    /// 幽灵(透明底,hover 叠 state-layer;行内/工具位)
    Ghost,
    /// 危险(danger 底;删除/破坏性操作)
    Danger,
}

/// Button 三尺寸(§5.6 #1:高度派生制,见 [`button_height`])。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ButtonSize {
    /// 紧凑(行内小件;下限 22)
    Compact,
    /// 默认(工具行/表单;下限 26)
    Default,
    /// 宽松(对话框主行动;下限 32)
    Roomy,
}

/// IconButton 两尺寸(§5.6 #2:20/24 方形)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconButtonSize {
    /// 20px(行内/工具条密集位)
    Icon20,
    /// 24px(独立工具位)
    Icon24,
}

/// 按下回调(gpui `on_click` 同形,ClickEvent 含 down/up 两次事件):
/// **启用态**且按下/释放都落在按钮内时触发;禁用永不触发。
pub type PressFn = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

// ---------------------------------------------------------------------------
// 视觉规格(纯函数;TC-CMP-BTN-01 的断言面)
// ---------------------------------------------------------------------------

/// 按钮解析样式(纯数据;由 [`button_style`] 从令牌裁决)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonStyle {
    /// 容器底色(含状态叠加)
    pub bg: Hsla,
    /// 前景色(文字/图标;禁用 = `text_disabled`)
    pub fg: Hsla,
    /// 描边(FocusRing 态 = accent;其余按变体)
    pub border: Hsla,
    /// 容器高度(派生制,[`button_height`])
    pub height: f32,
    /// 文字档(字号/行高/字重)
    pub text: TextSize,
    /// 水平内边距
    pub h_padding: f32,
    /// 圆角
    pub radius: f32,
}

/// 尺寸 → 文字档(Compact/Default = LABEL 12/18/500,Roomy = BODY 13/20/400)。
#[must_use]
pub fn button_text(size: ButtonSize) -> TextSize {
    match size {
        ButtonSize::Compact | ButtonSize::Default => TextSize::LABEL,
        ButtonSize::Roomy => TextSize::BODY,
    }
}

/// 按钮高度(派生制,AGENTS.md §3.4):`max(档位下限, 文本行高 + 2×垂直
/// padding)`。档位只是下限——CJK/大字号/DPI 缩放下由内容顶高,固定值会压字。
#[must_use]
pub fn button_height(size: ButtonSize, text: TextSize) -> f32 {
    match size {
        ButtonSize::Compact => control_height(HEIGHT_COMPACT, text.line_height, V_PAD_COMPACT),
        ButtonSize::Default => control_height(HEIGHT_DEFAULT, text.line_height, V_PAD_DEFAULT),
        ButtonSize::Roomy => control_height(HEIGHT_LOOSE, text.line_height, V_PAD_ROOMY),
    }
}

/// 实心变体(Primary/Danger)的前景裁决(纯函数):容器是高饱和功能色,
/// 文字取**极性相反**的两枚令牌候选(`text_strong` 与 `surface_0`)中
/// 对比度更高者——深色主题的 accent/danger 上白字不过 AA,黑字(surface_0
/// = N0)轻松通过;浅色主题相反。两候选都是既有令牌,**零新增令牌**
/// (TOK-08 纪律;AA 达标断言见 TC-CMP-BTN-01 的对比度用例)。
#[must_use]
pub fn solid_variant_foreground(container: Hsla, colors: &ColorTokens) -> Hsla {
    let candidates = [colors.text_strong, colors.surface_0];
    candidates
        .into_iter()
        .max_by(|a, b| contrast_ratio(*a, container).total_cmp(&contrast_ratio(*b, container)))
        .unwrap_or(colors.text_strong)
}

/// 按钮状态 → 样式(核心纯函数;TOK-04 state-layer 全量接线):
///
/// - bg = 变体底色经 [`state_layer`](Hover/Press 中性叠加,深色叠白/浅色叠
///   黑,极性按底色亮度自动取向);
/// - FocusRing 态:描边 = accent(accent 描边;完整 focus-ring 体系 = 第 4
///   组 A11Y 批次,本批先落按钮单点);
/// - Disabled 态:**容器与 Idle 逐位相同**(TOK-07,容器不变),仅前景降级
///   ([`disabled_foreground`] → `text_disabled`)。
#[must_use]
pub fn button_style(
    variant: ButtonVariant,
    size: ButtonSize,
    state: InteractState,
    colors: &ColorTokens,
) -> ButtonStyle {
    let text = button_text(size);
    let h_padding = match size {
        ButtonSize::Compact => SpacingTokens::XS,
        ButtonSize::Default => SpacingTokens::SM,
        ButtonSize::Roomy => SpacingTokens::MD,
    };
    let radius = match size {
        ButtonSize::Compact => RadiusTokens::SM,
        ButtonSize::Default | ButtonSize::Roomy => RadiusTokens::MD,
    };
    // 变体基色(Idle):底/前景/描边三件全部取令牌,零字面量
    let (base_bg, fg_idle, border_idle) = match variant {
        ButtonVariant::Primary => (
            colors.accent,
            solid_variant_foreground(colors.accent, colors),
            transparent(),
        ),
        ButtonVariant::Secondary => (colors.surface_3, colors.text_primary, colors.border_subtle),
        ButtonVariant::Ghost => (transparent(), colors.text_secondary, transparent()),
        ButtonVariant::Danger => (
            colors.danger,
            solid_variant_foreground(colors.danger, colors),
            transparent(),
        ),
    };
    let border = if state == InteractState::FocusRing {
        colors.accent
    } else {
        border_idle
    };
    let fg = if state == InteractState::Disabled {
        disabled_foreground(fg_idle, colors.text_disabled)
    } else {
        fg_idle
    };
    ButtonStyle {
        bg: state_layer(base_bg, state, colors.accent),
        fg,
        border,
        height: button_height(size, text),
        text,
        h_padding,
        radius,
    }
}

/// IconButton 解析样式:颜色/圆角规则与 [`button_style`] 同源(Compact 档),
/// 仅两处按图标规格覆写——文字档(20 → LABEL / 24 → BODY)与方形边长。
#[must_use]
pub fn icon_button_style(
    size: IconButtonSize,
    variant: ButtonVariant,
    state: InteractState,
    colors: &ColorTokens,
) -> ButtonStyle {
    let mut style = button_style(variant, ButtonSize::Compact, state, colors);
    style.text = match size {
        IconButtonSize::Icon20 => TextSize::LABEL,
        IconButtonSize::Icon24 => TextSize::BODY,
    };
    style.radius = RadiusTokens::SM;
    style
}

/// IconButton 边长(§5.6 #2:20/24 方形;图标无文字行高,固定边长即派生)。
#[must_use]
pub fn icon_button_side(size: IconButtonSize) -> f32 {
    match size {
        IconButtonSize::Icon20 => ICON_BUTTON_SM_PX,
        IconButtonSize::Icon24 => ICON_BUTTON_LG_PX,
    }
}

/// 透明底(Ghost 底色/实心变体的无描边槽位;常量直通,非颜色字面量)。
fn transparent() -> Hsla {
    Hsla::transparent_black()
}

/// f64 动画值 → f32(GPU 域收口,panels 同款惯例;缩放系数为小量,截断无碍)。
#[allow(clippy::cast_possible_truncation)]
fn f32v(v: f64) -> f32 {
    v as f32
}

// ---------------------------------------------------------------------------
// press 缩放弹簧(anim 引擎;reduced_motion 直通)
// ---------------------------------------------------------------------------

/// 一次 press 缩放过渡:`from → to`(单位 = 缩放系数),`started_ms` 为
/// 调用方时钟(`interact::now_ms` 同源)。解析弹簧随时可求任意时刻,无步进态。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PressAnim {
    from: f32,
    to: f32,
    started_ms: f64,
}

/// press 缩放值(纯函数):`from + (to - from) × SNAPPY 单位弹簧(t)`。
/// reduced_motion 直通 `to`(按下即 [`PRESS_SCALE`],松开即 1.0,无动画)。
#[must_use]
pub fn press_scale_at(anim: &PressAnim, now_ms: f64, reduced: bool) -> f32 {
    if reduced {
        return anim.to;
    }
    let t_sec = ((now_ms - anim.started_ms) / 1000.0).max(0.0);
    anim.from + (anim.to - anim.from) * f32v(Spring::SNAPPY.solve(t_sec))
}

/// 弹簧落定时长(秒,纯函数):包络 `e^(-ζωt)` 衰减到 [`SPRING_SETTLE_EPS`]
/// 的时刻——由弹簧参数推出,不另立魔法时长。阻尼/刚度非正(非法参数)返回
/// 0(立即落定,与 [`Spring::solve_with_velocity`] 的防御一致)。
#[must_use]
fn spring_settle_secs(spring: Spring) -> f64 {
    // 防御 NaN:字段是 pub 值,非法参数(刚度/质量/阻尼 ≤ 0 或 NaN)在这里
    // 一并挡下(NaN 的比较恒假,否定比较才拦得住——与
    // Spring::solve_with_velocity 的防御同款),不 panic、立即落定。
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(spring.stiffness > 0.0) || !(spring.mass > 0.0) {
        return 0.0;
    }
    let omega = (spring.stiffness / spring.mass).sqrt();
    let zeta = spring.damping / (2.0 * (spring.stiffness * spring.mass).sqrt());
    let decay_rate = zeta * omega;
    // 阻尼非正/NaN 时 decay_rate 非正(NaN 的比较恒假,否定比较才拦得住,
    // 属性与 Spring::solve_with_velocity 同款):立即落定。
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(decay_rate > 0.0) {
        return 0.0;
    }
    (1.0 / SPRING_SETTLE_EPS).ln() / decay_rate
}

/// press 过渡是否已落定(纯函数;为真时组件停帧)。
#[must_use]
pub fn press_anim_settled(anim: &PressAnim, now_ms: f64, reduced: bool) -> bool {
    if reduced {
        return true;
    }
    let elapsed = (now_ms - anim.started_ms).max(0.0);
    elapsed >= spring_settle_secs(Spring::SNAPPY)
}

/// 发起一次 press 过渡:从 `current`(打断接续,无跳变)前往 `to`。
fn press_anim_to(current: f32, to: f32, now_ms: f64) -> PressAnim {
    PressAnim {
        from: current,
        to,
        started_ms: now_ms,
    }
}

/// 当前缩放值(`None` = 静止 1.0;组件内部共用)。
fn current_press_scale(press: &Option<PressAnim>, now_ms: f64) -> f32 {
    press
        .as_ref()
        .map(|a| press_scale_at(a, now_ms, reduced_motion()))
        .unwrap_or(1.0)
}

/// 按下开始(Entity 形态共用):从当前值向 [`PRESS_SCALE`] 弹簧;禁用忽略。
fn press_down(press: &mut Option<PressAnim>, disabled: bool) {
    if disabled {
        return; // TOK-07:禁用无热区
    }
    let now = interact::now_ms();
    *press = Some(press_anim_to(
        current_press_scale(press, now),
        PRESS_SCALE,
        now,
    ));
}

/// 回弹到 1.0(Entity 形态共用;从当前值接续)。
fn spring_back(press: &mut Option<PressAnim>) {
    let now = interact::now_ms();
    *press = Some(press_anim_to(current_press_scale(press, now), 1.0, now));
}

/// hover 进出(Entity 形态共用;禁用忽略,进行中的过渡由停帧自然沉降)。
fn hover_changed(hover: &mut HoverState, hovered: &bool, disabled: bool) {
    if disabled {
        return;
    }
    let now = interact::now_ms();
    if *hovered {
        hover.on_enter(now);
    } else {
        hover.on_leave(now);
    }
}

// ---------------------------------------------------------------------------
// Button(Entity 形态,规范 API)
// ---------------------------------------------------------------------------

/// 文本/图标+文本按钮(§5.6 #1)。规范用法 = Entity 形态:
///
/// ```ignore
/// cx.new(|_| {
///     Button::new("export", "导出")
///         .variant(ButtonVariant::Primary)
///         .on_press(|_ev, _win, cx| { /* ... */ })
/// })
/// ```
///
/// 内联(RenderOnce 面板/列表行)用法见 [`button_element`]。
pub struct Button {
    id: ElementId,
    label: SharedString,
    icon: Option<SharedString>,
    variant: ButtonVariant,
    size: ButtonSize,
    disabled: bool,
    on_press: Option<PressFn>,
    // —— 跨帧交互态(Entity 形态;内联形态不建实体,保持默认即可)——
    press: Option<PressAnim>,
    hover: HoverState,
    focus: Option<FocusHandle>,
    /// A11Y-02 语义槽(可访问名缺省 = 可见文本,见 resolved_semantic)
    semantic: Semantic,
}

impl Button {
    /// 文本按钮;默认 Secondary 变体 + Default 尺寸 + 启用态。
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Button {
            id: id.into(),
            label: label.into(),
            icon: None,
            variant: ButtonVariant::Secondary,
            size: ButtonSize::Default,
            disabled: false,
            on_press: None,
            press: None,
            hover: HoverState::new(),
            focus: None,
            semantic: Semantic::new(),
        }
    }

    /// 变体(默认 [`ButtonVariant::Secondary`])。
    #[must_use]
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// 尺寸(默认 [`ButtonSize::Default`])。
    #[must_use]
    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    /// 图标(文字字形占位,§5.5 SVG 图标系统 = 后续批次;槽位已定型)。
    /// 与文本并排,间距 XS。
    #[must_use]
    pub fn icon(mut self, glyph: impl Into<SharedString>) -> Self {
        self.icon = Some(glyph.into());
        self
    }

    /// 禁用态(TOK-07):仅前景降级、容器不变、不响应任何交互。
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 按下回调(启用态下按下并在按钮内释放时触发)。
    #[must_use]
    pub fn on_press(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_press = Some(Rc::new(f));
        self
    }

    /// 当前 press 缩放值(渲染期求值;宿主调试可读)。
    #[must_use]
    pub fn press_scale(&self, now_ms: f64) -> f32 {
        current_press_scale(&self.press, now_ms)
    }

    // —— 事件(Entity 形态;内联形态的伪类路径见 button_element)——

    fn on_press_down(
        &mut self,
        _ev: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        press_down(&mut self.press, self.disabled);
        cx.notify();
    }

    /// 释放(点击成立):弹簧回弹 + 触发回调(禁用不触发)。
    fn on_clicked(&mut self, ev: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.press.take();
        spring_back(&mut self.press);
        if !self.disabled
            && let Some(cb) = self.on_press.clone()
        {
            cb(ev, window, cx);
        }
        cx.notify();
    }

    /// 按下后拖出按钮释放:只回弹,不触发回调(标准按钮语义)。
    fn on_release_out(
        &mut self,
        _ev: &gpui::MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.press.take().is_some() {
            spring_back(&mut self.press);
            cx.notify();
        }
    }

    fn on_hover_changed(&mut self, hovered: &bool, _window: &mut Window, cx: &mut Context<Self>) {
        hover_changed(&mut self.hover, hovered, self.disabled);
        cx.notify();
    }
}

// A11Y-02 语义槽(label/role/semantic 三件):Button 可访问名缺省回落
// 可见文本(见 resolved_semantic)。
semantic_slot!(Button);

impl Button {
    /// 解析语义(A11Y-02):显式 `.label(...)` 优先,缺省 = 可见文本;
    /// role 默认 [`SemanticRole::Button`]。
    #[must_use]
    pub fn resolved_semantic(&self) -> Semantic {
        let mut sem = Semantic::new().with_label(self.label.clone());
        if let Some(over) = self.semantic.label() {
            sem = sem.with_label(over.clone());
        }
        sem.with_role(self.semantic.role().unwrap_or(SemanticRole::Button))
    }
}

impl Render for Button {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let now = interact::now_ms();
        let reduced = reduced_motion();
        let disabled = self.disabled;
        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let focused = !disabled && focus.is_focused(window);
        let pressed = !disabled && self.press.as_ref().is_some_and(|a| a.to < 1.0);
        let hover_progress = if disabled {
            0.0
        } else {
            f32v(self.hover.progress_at(now))
        };
        let scale = current_press_scale(&self.press, now);
        let animating = self
            .press
            .as_ref()
            .is_some_and(|a| !press_anim_settled(a, now, reduced));

        // 四态裁决:Disabled > Pressed > Idle,hover 进度在静止底上插值
        // (NumberField 同款);焦点只把描边覆盖为 accent。
        let state = if disabled {
            InteractState::Disabled
        } else if pressed {
            InteractState::Pressed
        } else {
            InteractState::Idle
        };
        let mut style = button_style(self.variant, self.size, state, &colors);
        if hover_progress > 0.0 && !pressed {
            let hover_bg = button_style(self.variant, self.size, InteractState::Hover, &colors).bg;
            style.bg = lerp_hsla(style.bg, hover_bg, f64::from(hover_progress));
        }
        if focused {
            style.border = colors.accent;
        }

        let height = style.height;
        let quad = button_quad(&style, self.icon.as_ref(), Some(&self.label), scale);
        let mut root = button_root(
            self.id.clone(),
            quad,
            hit_size(height),
            None,
            &focus,
            disabled,
        )
        .on_hover(cx.listener(Self::on_hover_changed));
        if !disabled {
            root = root
                .on_mouse_down(MouseButton::Left, cx.listener(Self::on_press_down))
                .on_click(cx.listener(Self::on_clicked))
                .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_release_out));
        }

        // 动画运行才请求帧(hover 插值 / press 弹簧;静止零帧提交,§4.4)
        let hover_running = !disabled && self.hover.is_running(now);
        if hover_running || animating {
            window.request_animation_frame();
        }
        root
    }
}

// ---------------------------------------------------------------------------
// IconButton(Entity 形态,规范 API)
// ---------------------------------------------------------------------------

/// 图标按钮(§5.6 #2:20/24 两档方形、无文字、**tooltip 槽位定型**)。
///
/// ```ignore
/// cx.new(|_| {
///     IconButton::new("eye-3", "◉")
///         .size(IconButtonSize::Icon20)
///         .tooltip("切换效果启用")
///         .on_press(|_ev, _win, cx| { /* ... */ })
/// })
/// ```
///
/// 图标 = 文字字形占位(SVG 图标系统 = §5.5 后续批次)。tooltip 经 gpui
/// 原生机制挂最简浮层(禁用态同样显示);批 2 `Tooltip` 接管渲染时以
/// [`Self::tooltip_label`]/[`Self::tooltip_shortcut`] 为数据源,签名不变。
pub struct IconButton {
    id: ElementId,
    icon: SharedString,
    size: IconButtonSize,
    variant: ButtonVariant,
    disabled: bool,
    tooltip_label: Option<SharedString>,
    tooltip_shortcut: Option<SharedString>,
    on_press: Option<PressFn>,
    press: Option<PressAnim>,
    hover: HoverState,
    focus: Option<FocusHandle>,
    /// A11Y-02 语义槽(可访问名缺省 = tooltip 文案,见 resolved_semantic)
    semantic: Semantic,
}

impl IconButton {
    /// 图标按钮;默认 20 档 + Ghost 变体 + 启用态。
    pub fn new(id: impl Into<ElementId>, icon: impl Into<SharedString>) -> Self {
        IconButton {
            id: id.into(),
            icon: icon.into(),
            size: IconButtonSize::Icon20,
            variant: ButtonVariant::Ghost,
            disabled: false,
            tooltip_label: None,
            tooltip_shortcut: None,
            on_press: None,
            press: None,
            hover: HoverState::new(),
            focus: None,
            semantic: Semantic::new(),
        }
    }

    /// 尺寸(默认 [`IconButtonSize::Icon20`])。
    #[must_use]
    pub fn size(mut self, size: IconButtonSize) -> Self {
        self.size = size;
        self
    }

    /// 变体(默认 [`ButtonVariant::Ghost`];工具位需要凸起底用 Secondary)。
    #[must_use]
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// 禁用态(TOK-07):仅前景降级、容器不变、不响应交互;tooltip 保留。
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// tooltip 主文案槽(批 2 `Tooltip` 的数据源;`名称` 部分)。
    #[must_use]
    pub fn tooltip(mut self, label: impl Into<SharedString>) -> Self {
        self.tooltip_label = Some(label.into());
        self
    }

    /// tooltip 快捷键说明槽(批 2 `key_badge_text` 的 `快捷键` 部分)。
    #[must_use]
    pub fn tooltip_shortcut(mut self, keys: impl Into<SharedString>) -> Self {
        self.tooltip_shortcut = Some(keys.into());
        self
    }

    /// tooltip 主文案(批 2 Tooltip 组件消费;签名定型)。
    #[must_use]
    pub fn tooltip_label(&self) -> Option<&SharedString> {
        self.tooltip_label.as_ref()
    }

    /// tooltip 快捷键说明(批 2 `key_badge_text` 消费;签名定型)。
    #[must_use]
    pub fn tooltip_shortcut_text(&self) -> Option<&SharedString> {
        self.tooltip_shortcut.as_ref()
    }

    /// 按下回调(启用态下按下并在按钮内释放时触发)。
    #[must_use]
    pub fn on_press(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_press = Some(Rc::new(f));
        self
    }

    /// 当前 press 缩放值(渲染期求值;宿主调试可读)。
    #[must_use]
    pub fn press_scale(&self, now_ms: f64) -> f32 {
        current_press_scale(&self.press, now_ms)
    }

    // —— 事件(Entity 形态)——

    fn on_press_down(
        &mut self,
        _ev: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        press_down(&mut self.press, self.disabled);
        cx.notify();
    }

    fn on_clicked(&mut self, ev: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.press.take();
        spring_back(&mut self.press);
        if !self.disabled
            && let Some(cb) = self.on_press.clone()
        {
            cb(ev, window, cx);
        }
        cx.notify();
    }

    fn on_release_out(
        &mut self,
        _ev: &gpui::MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.press.take().is_some() {
            spring_back(&mut self.press);
            cx.notify();
        }
    }

    fn on_hover_changed(&mut self, hovered: &bool, _window: &mut Window, cx: &mut Context<Self>) {
        hover_changed(&mut self.hover, hovered, self.disabled);
        cx.notify();
    }
}

// A11Y-02 语义槽:IconButton 无可见文本,可访问名缺省回落 tooltip 文案
// (见 resolved_semantic)。
semantic_slot!(IconButton);

impl IconButton {
    /// 解析语义(A11Y-02):显式 `.label(...)` 优先,缺省 = tooltip 文案;
    /// role 默认 [`SemanticRole::IconButton`]。
    #[must_use]
    pub fn resolved_semantic(&self) -> Semantic {
        let mut sem = match &self.tooltip_label {
            Some(text) => Semantic::new().with_label(text.clone()),
            None => Semantic::new(),
        };
        if let Some(label) = self.semantic.label() {
            sem = sem.with_label(label.clone());
        }
        sem.with_role(self.semantic.role().unwrap_or(SemanticRole::IconButton))
    }
}

impl Render for IconButton {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let now = interact::now_ms();
        let reduced = reduced_motion();
        let disabled = self.disabled;
        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let focused = !disabled && focus.is_focused(window);
        let pressed = !disabled && self.press.as_ref().is_some_and(|a| a.to < 1.0);
        let hover_progress = if disabled {
            0.0
        } else {
            f32v(self.hover.progress_at(now))
        };
        let scale = current_press_scale(&self.press, now);
        let animating = self
            .press
            .as_ref()
            .is_some_and(|a| !press_anim_settled(a, now, reduced));

        let state = if disabled {
            InteractState::Disabled
        } else if pressed {
            InteractState::Pressed
        } else {
            InteractState::Idle
        };
        let mut style = icon_button_style(self.size, self.variant, state, &colors);
        if hover_progress > 0.0 && !pressed {
            let hover_bg =
                icon_button_style(self.size, self.variant, InteractState::Hover, &colors).bg;
            style.bg = lerp_hsla(style.bg, hover_bg, f64::from(hover_progress));
        }
        if focused {
            style.border = colors.accent;
        }

        let side = icon_button_side(self.size);
        let quad = icon_button_quad(&style, &self.icon, side, scale);
        let mut root = button_root(
            self.id.clone(),
            quad,
            hit_size(side),
            Some(hit_size(side)),
            &focus,
            disabled,
        )
        .on_hover(cx.listener(Self::on_hover_changed));
        if !disabled {
            root = root
                .on_mouse_down(MouseButton::Left, cx.listener(Self::on_press_down))
                .on_click(cx.listener(Self::on_clicked))
                .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_release_out));
        }

        // tooltip 槽(gpui 原生机制,hover 惰性建视图;禁用态同样显示):
        // 本批最简浮层占位,批 2 Tooltip 接管渲染,槽位签名不变。
        if let Some(label) = self.tooltip_label.clone() {
            let shortcut = self.tooltip_shortcut.clone();
            root = root.tooltip(move |_window, cx| {
                cx.new(|_| IconButtonTooltipView {
                    label: label.clone(),
                    shortcut: shortcut.clone(),
                })
                .into()
            });
        }

        let hover_running = !disabled && self.hover.is_running(now);
        if hover_running || animating {
            window.request_animation_frame();
        }
        root
    }
}

/// IconButton 的 tooltip 占位视图(最简浮层:凸起底 + caption 文案;
/// `label (shortcut)` 拼接 = 上游"名称 (快捷键)"语义的 v0 占位,批 2 由
/// `Tooltip` + `key_badge_text` 单源接管)。
struct IconButtonTooltipView {
    label: SharedString,
    shortcut: Option<SharedString>,
}

impl Render for IconButtonTooltipView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let text = match &self.shortcut {
            Some(keys) => format!("{} ({})", self.label, keys).into(),
            None => self.label.clone(),
        };
        div()
            .px(px(SpacingTokens::SM))
            .py(px(SpacingTokens::XS))
            .rounded(px(RadiusTokens::MD))
            .bg(colors.surface_4)
            .border_1()
            .border_color(colors.border_strong)
            .text_size(px(TextSize::CAPTION.size))
            .text_color(colors.text_strong)
            .child(text)
    }
}

// ---------------------------------------------------------------------------
// 共用装配(quad / 热区根)+ 内联形态适配器
// ---------------------------------------------------------------------------

/// 按钮 quad(共用):变体样式 + 缩放系数(press 弹簧的视觉缩放只缩
/// quad,布局由外层热区根固定,不推挤邻居)。
fn button_quad(
    style: &ButtonStyle,
    icon: Option<&SharedString>,
    label: Option<&SharedString>,
    scale: f32,
) -> Div {
    let mut quad = h_flex()
        .justify_center()
        .items_center()
        .gap(px(SpacingTokens::XS * scale))
        .h(px(style.height * scale))
        .px(px(style.h_padding * scale))
        .rounded(px(style.radius * scale))
        .border_1()
        .border_color(style.border)
        .bg(style.bg)
        .text_size(px(style.text.size * scale))
        .font_weight(FontWeight(style.text.weight))
        .text_color(style.fg);
    if let Some(glyph) = icon {
        quad = quad.child(div().child(glyph.clone()));
    }
    if let Some(text) = label {
        quad = quad.child(div().child(text.clone()));
    }
    quad
}

/// IconButton quad(共用):方形 + 居中字形。
fn icon_button_quad(style: &ButtonStyle, glyph: &SharedString, side: f32, scale: f32) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(side * scale))
        .rounded(px(style.radius * scale))
        .border_1()
        .border_color(style.border)
        .bg(style.bg)
        .text_size(px(style.text.size * scale))
        .font_weight(FontWeight(style.text.weight))
        .text_color(style.fg)
        .child(glyph.clone())
}

/// 热区根(共用):固定外尺寸(press 缩放不推布局)+ 焦点接入;禁用态
/// 无指针样式(监听器由调用方按需挂)。`outer_size` 由调用方传
/// [`hit_size`](A11Y-03:命中区 ≥24px——视觉 quad 居中不变,热区外扩),
/// `min_w` 为方形件(IconButton)的宽度下限。
fn button_root(
    id: ElementId,
    quad: Div,
    outer_size: f32,
    min_w: Option<f32>,
    focus: &FocusHandle,
    disabled: bool,
) -> Stateful<Div> {
    let root = h_flex()
        .id(id)
        .h(px(outer_size))
        .justify_center()
        .items_center()
        .child(quad)
        .track_focus(focus);
    let root = match min_w {
        Some(w) => root.min_w(px(w)),
        None => root,
    };
    if disabled {
        root
    } else {
        root.cursor_pointer()
    }
}

/// [`Button`] 的**内联形态**(`CMP-11` 收口与 story 列表行用):把规格直接
/// 渲染为元素,hover/press 走 gpui 即时伪类(state-layer 即时切换),无
/// 弹簧缩放、无焦点环(两形态的规格同源 [`button_style`])。逐帧重建
/// 场景请勿为每行建 Entity(反模式 CMP-06)。
#[must_use]
pub fn button_element(button: Button, cx: &App) -> AnyElement {
    let colors = theme(cx).colors;
    let disabled = button.disabled;
    let state = if disabled {
        InteractState::Disabled
    } else {
        InteractState::Idle
    };
    let style = button_style(button.variant, button.size, state, &colors);
    let quad = button_quad(&style, button.icon.as_ref(), Some(&button.label), 1.0);
    if disabled {
        return quad.id(button.id).into_any_element(); // TOK-07:无热区,仅前景降级
    }
    // A11Y-06:三态落在视觉 quad(hover/active 即时伪类,视觉规格同源);
    // quad 持原 id(Stateful:active 可用)。
    let hover_bg = button_style(button.variant, button.size, InteractState::Hover, &colors).bg;
    let press_bg = button_style(button.variant, button.size, InteractState::Pressed, &colors).bg;
    let quad = quad
        .id(button.id.clone())
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .active(move |s| s.bg(press_bg));
    // A11Y-03:命中区 ≥24px——视觉 quad 居中不变,Stateful 命中容器外扩
    //(容器持派生 id,点击监听只在容器:padding 区可点且不双触发)
    let mut hit = h_flex()
        .id(ElementId::NamedChild(
            Box::new(button.id.clone()),
            "hit".into(),
        ))
        .h(px(hit_size(style.height)))
        .justify_center()
        .items_center()
        .cursor_pointer()
        .child(quad);
    if let Some(cb) = button.on_press {
        hit = hit.on_click(move |ev, window, cx| cb(ev, window, cx));
    }
    hit.into_any_element()
}

/// [`IconButton`] 的**内联形态**(规格同源 [`icon_button_style`];tooltip
/// 槽同样生效,禁用态保留)。语义与 [`button_element`] 一致。
#[must_use]
pub fn icon_button_element(button: IconButton, cx: &App) -> AnyElement {
    let colors = theme(cx).colors;
    let disabled = button.disabled;
    let state = if disabled {
        InteractState::Disabled
    } else {
        InteractState::Idle
    };
    let style = icon_button_style(button.size, button.variant, state, &colors);
    let mut quad = icon_button_quad(&style, &button.icon, icon_button_side(button.size), 1.0)
        .id(button.id.clone()); // Stateful:tooltip/active 可用
    if let Some(label) = button.tooltip_label.clone() {
        let shortcut = button.tooltip_shortcut.clone();
        quad = quad.tooltip(move |_window, cx| {
            cx.new(|_| IconButtonTooltipView {
                label: label.clone(),
                shortcut: shortcut.clone(),
            })
            .into()
        });
    }
    if disabled {
        return quad.into_any_element();
    }
    // A11Y-06:三态落在视觉 quad(hover/active 即时伪类);quad 持原 id
    let hover_bg = icon_button_style(button.size, button.variant, InteractState::Hover, &colors).bg;
    let press_bg =
        icon_button_style(button.size, button.variant, InteractState::Pressed, &colors).bg;
    let quad = quad
        .id(button.id.clone())
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .active(move |s| s.bg(press_bg));
    // A11Y-03:视觉 20px、命中 ≥24px(quad 居中不变,命中容器外扩;
    // 容器持派生 id,点击监听只在容器:padding 区可点且不双触发)
    let mut hit = h_flex()
        .id(ElementId::NamedChild(
            Box::new(button.id.clone()),
            "hit".into(),
        ))
        .size(px(hit_size(icon_button_side(button.size))))
        .justify_center()
        .items_center()
        .cursor_pointer()
        .child(quad);
    if let Some(cb) = button.on_press {
        hit = hit.on_click(move |ev, window, cx| cb(ev, window, cx));
    }
    hit.into_any_element()
}

// ---------------------------------------------------------------------------
// TC-CMP-BTN-01(三尺寸高度 / 四态映射 / 禁用容器逐位不变)+ 弹簧与
// 对比度纯函数用例(状态→样式的映射全部不经 GUI 断言,NumberField 同款)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::composite_over;

    #[test]
    fn tc_cmp_btn_01_size_heights_are_token_derived() {
        // 三档下限生效(文字 = 各档默认档,行高不顶破下限)
        assert_eq!(
            button_height(ButtonSize::Compact, button_text(ButtonSize::Compact)),
            HEIGHT_COMPACT,
            "compact = 22 下限"
        );
        assert_eq!(
            button_height(ButtonSize::Default, button_text(ButtonSize::Default)),
            HEIGHT_DEFAULT,
            "default = 26 下限"
        );
        assert_eq!(
            button_height(ButtonSize::Roomy, button_text(ButtonSize::Roomy)),
            HEIGHT_LOOSE,
            "roomy = 32 下限"
        );
        // 内容派生:CJK 大行高把高度顶起(固定档会压字,AGENTS.md §3.4)
        let cjk = TextSize {
            size: 12.0,
            line_height: 24.0,
            weight: 500.0,
        };
        assert_eq!(
            button_height(ButtonSize::Compact, cjk),
            24.0 + 2.0 * V_PAD_COMPACT,
            "compact 被 24px 行高顶高"
        );
        assert_eq!(
            button_height(ButtonSize::Default, cjk),
            24.0 + 2.0 * V_PAD_DEFAULT,
            "default 被 24px 行高顶高"
        );
        // roomy 的文字档 = BODY(行高 20),恰好落在下限
        assert_eq!(
            button_text(ButtonSize::Roomy).line_height,
            TextSize::BODY.line_height
        );
        assert_eq!(button_text(ButtonSize::Default), TextSize::LABEL);
    }

    #[test]
    fn tc_cmp_btn_01_four_state_mapping_and_focus_ring() {
        for tokens in [ColorTokens::dark(), ColorTokens::light()] {
            let theme_name = if tokens.surface_3.l < 0.5 {
                "dark"
            } else {
                "light"
            };
            for variant in [
                ButtonVariant::Primary,
                ButtonVariant::Secondary,
                ButtonVariant::Ghost,
                ButtonVariant::Danger,
            ] {
                let size = ButtonSize::Default;
                let idle = button_style(variant, ButtonSize::Default, InteractState::Idle, &tokens);
                let hover = button_style(variant, size, InteractState::Hover, &tokens);
                let press = button_style(variant, size, InteractState::Pressed, &tokens);
                let ring = button_style(variant, size, InteractState::FocusRing, &tokens);
                // hover 可见且方向符合 state-layer 极性(TOK-04:按**基色
                // 亮度**取向——深色表面叠白变亮、浅色表面叠黑变暗;实心
                // 变体在深色主题的 accent/danger 底 l>0.5,因此同样叠黑,
                // 这是设计语义而非缺陷)
                let base_is_light = idle.bg.l >= 0.5;
                if base_is_light {
                    assert!(
                        hover.bg.l < idle.bg.l,
                        "{theme_name}/{variant:?} hover 应压暗(浅基色)"
                    );
                } else {
                    assert!(
                        hover.bg.l > idle.bg.l,
                        "{theme_name}/{variant:?} hover 应提亮(深基色)"
                    );
                }
                // press 强于 hover(叠加强度 press > hover)
                assert_ne!(press.bg, hover.bg, "{theme_name}/{variant:?} press ≠ hover");
                // 焦点环 = accent 描边(两主题一致规则)
                assert_eq!(
                    ring.border, tokens.accent,
                    "{theme_name}/{variant:?} 焦点描边"
                );
                assert_ne!(idle.border, tokens.accent, "未聚焦不得出现 accent 描边");
                // 几何与文字档跨态不变(状态只裁决色)
                assert_eq!(hover.height, idle.height);
                assert_eq!(hover.text, idle.text);
                assert_eq!(hover.h_padding, idle.h_padding);
                assert_eq!(hover.radius, idle.radius);
            }
            // Ghost 透明底:hover = 纯叠加层(alpha = 状态 alpha,TC-TOK-STATE 同源)。
            // 注意极性按**基色**亮度取向:透明底 l=0 → 恒取深色(白)叠加层,
            // 与主题无关(interact::state_layer 的透明底语义)。
            let ghost_idle = button_style(
                ButtonVariant::Ghost,
                ButtonSize::Default,
                InteractState::Idle,
                &tokens,
            );
            let ghost_hover = button_style(
                ButtonVariant::Ghost,
                ButtonSize::Default,
                InteractState::Hover,
                &tokens,
            );
            assert_eq!(ghost_idle.bg.a, 0.0, "ghost 静止全透明");
            let expect_alpha = crate::tokens::StateLayerTokens::dark().hover;
            assert!(
                (ghost_hover.bg.a - expect_alpha).abs() < 1e-6,
                "ghost hover = 纯 state-layer 叠加层(透明底取白色叠加),得 {}",
                ghost_hover.bg.a
            );
        }
    }

    #[test]
    fn tc_cmp_btn_01_disabled_container_bitwise_unchanged() {
        for tokens in [ColorTokens::dark(), ColorTokens::light()] {
            for variant in [
                ButtonVariant::Primary,
                ButtonVariant::Secondary,
                ButtonVariant::Ghost,
                ButtonVariant::Danger,
            ] {
                for size in [ButtonSize::Compact, ButtonSize::Default, ButtonSize::Roomy] {
                    let idle = button_style(variant, size, InteractState::Idle, &tokens);
                    let dis = button_style(variant, size, InteractState::Disabled, &tokens);
                    // 容器逐位不变(TOK-07:bg/描边/几何;fg 是唯一降级位)
                    assert_eq!(dis.bg, idle.bg, "{variant:?}/{size:?} 禁用 bg 必须逐位不变");
                    assert_eq!(
                        dis.border, idle.border,
                        "{variant:?}/{size:?} 禁用描边逐位不变"
                    );
                    assert_eq!(dis.height, idle.height);
                    assert_eq!(dis.h_padding, idle.h_padding);
                    assert_eq!(dis.radius, idle.radius);
                    assert_eq!(dis.text, idle.text);
                    // 前景 = disabled_foreground(text_disabled),且确实降级
                    assert_eq!(
                        dis.fg,
                        disabled_foreground(idle.fg, tokens.text_disabled),
                        "禁用前景必须走 disabled_foreground 单点"
                    );
                    assert_ne!(dis.fg, idle.fg, "禁用前景必须可见降级");
                }
            }
        }
    }

    #[test]
    fn solid_variant_foreground_meets_aa_in_both_themes() {
        // 实心变体(Primary/Danger)前景对容器 ≥ 4.5:1(深浅两主题;
        // 候选只有 text_strong/surface_0 两枚既有令牌,零新增)
        for tokens in [ColorTokens::dark(), ColorTokens::light()] {
            for container in [tokens.accent, tokens.danger] {
                let fg = solid_variant_foreground(container, &tokens);
                let ratio = contrast_ratio(composite_over(fg, container), container);
                assert!(
                    ratio >= 4.5,
                    "实心变体前景对容器对比度 {ratio:.2} < 4.5(容器 {container:?})"
                );
                // 裁决确实是"取对比度更高者"(与手排两候选的最优一致)
                let mut candidates = [tokens.text_strong, tokens.surface_0];
                candidates.sort_by(|a, b| {
                    contrast_ratio(*b, container).total_cmp(&contrast_ratio(*a, container))
                });
                assert_eq!(fg, candidates[0], "必须选中对比度更高的候选");
            }
        }
    }

    #[test]
    fn press_spring_moves_and_settles_at_target() {
        let down = press_anim_to(1.0, PRESS_SCALE, 1000.0);
        // 起步向 0.94 推进(60ms 时已明显离开 1.0)
        let mid = press_scale_at(&down, 1060.0, false);
        assert!(
            mid < 1.0 && mid > PRESS_SCALE,
            "60ms 处于 0.94→1 弹簧途中:{mid}"
        );
        // 落定后精确在 0.94,且停帧
        let settle_ms = spring_settle_secs(Spring::SNAPPY) * 1000.0;
        let done = press_scale_at(&down, 1000.0 + settle_ms, false);
        assert!(
            (done - PRESS_SCALE).abs() < 0.005,
            "落定值收敛到 press 缩放:{done}"
        );
        assert!(press_anim_settled(&down, 1000.0 + settle_ms, false));
        // 回弹:从 0.94 → 1.0,允许轻微过冲(SNAPPY 弱欠阻尼)后收敛
        let up = press_anim_to(PRESS_SCALE, 1.0, 2000.0);
        let back = press_scale_at(&up, 2000.0 + settle_ms, false);
        assert!((back - 1.0).abs() < 0.005, "回弹收敛到 1.0:{back}");
        assert!(press_anim_settled(&up, 2000.0 + settle_ms, false));
        // 时钟回拨:按 0 处理(起步值)
        let restart = press_scale_at(&down, 500.0, false);
        assert!((restart - 1.0).abs() < 1e-6, "早于起点 = from 值");
    }

    #[test]
    fn press_reduced_motion_passes_through_target() {
        let down = press_anim_to(1.0, PRESS_SCALE, 1000.0);
        assert_eq!(
            press_scale_at(&down, 1001.0, true),
            PRESS_SCALE,
            "按下直通 0.94"
        );
        assert!(press_anim_settled(&down, 1001.0, true), "减弱动态:立即停帧");
        let up = press_anim_to(PRESS_SCALE, 1.0, 1000.0);
        assert_eq!(press_scale_at(&up, 1001.0, true), 1.0, "松开直通 1.0");
    }

    #[test]
    fn spring_settle_is_derived_from_params_not_magic() {
        let settle = spring_settle_secs(Spring::SNAPPY);
        // SNAPPY(ζ=0.7, ω=20):包络衰减到 0.005 的解析时刻,量级合理
        assert!(
            settle > 0.2 && settle < 0.8,
            "SNAPPY 落定时长应在 0.2~0.8s:{settle}"
        );
        // 非法弹簧参数:立即落定(防御,与 solve 的防御一致,不 panic)
        assert_eq!(
            spring_settle_secs(Spring {
                stiffness: 0.0,
                damping: 0.0,
                mass: 1.0
            }),
            0.0
        );
    }

    #[test]
    fn icon_button_sizes_and_styles_follow_spec() {
        assert_eq!(icon_button_side(IconButtonSize::Icon20), 20.0);
        assert_eq!(icon_button_side(IconButtonSize::Icon24), 24.0);
        for tokens in [ColorTokens::dark(), ColorTokens::light()] {
            for icon_size in [IconButtonSize::Icon20, IconButtonSize::Icon24] {
                let idle = icon_button_style(
                    icon_size,
                    ButtonVariant::Ghost,
                    InteractState::Idle,
                    &tokens,
                );
                let dis = icon_button_style(
                    icon_size,
                    ButtonVariant::Ghost,
                    InteractState::Disabled,
                    &tokens,
                );
                // 容器不变(与 Button 同一条 TOK-07 规则)
                assert_eq!(dis.bg, idle.bg);
                assert_eq!(dis.border, idle.border);
                assert_ne!(dis.fg, idle.fg);
                assert_eq!(dis.fg, disabled_foreground(idle.fg, tokens.text_disabled));
            }
        }
        // 20/24 的文字档映射(LABEL / BODY)
        let dark = ColorTokens::dark();
        assert_eq!(
            icon_button_style(
                IconButtonSize::Icon20,
                ButtonVariant::Ghost,
                InteractState::Idle,
                &dark
            )
            .text,
            TextSize::LABEL
        );
        assert_eq!(
            icon_button_style(
                IconButtonSize::Icon24,
                ButtonVariant::Ghost,
                InteractState::Idle,
                &dark
            )
            .text,
            TextSize::BODY
        );
    }

    #[test]
    fn icon_button_tooltip_slots_are_typed_and_independent() {
        let mut btn = IconButton::new("tip", "◉").tooltip("切换效果启用");
        assert_eq!(
            btn.tooltip_label(),
            Some(&SharedString::from("切换效果启用"))
        );
        assert_eq!(btn.tooltip_shortcut_text(), None, "快捷键槽独立可选");
        btn = btn.tooltip_shortcut("Mod+E");
        assert_eq!(
            btn.tooltip_shortcut_text(),
            Some(&SharedString::from("Mod+E"))
        );
        // 覆写语义:后写胜(槽位是字段,不是追加)
        btn = btn.tooltip("新文案");
        assert_eq!(btn.tooltip_label(), Some(&SharedString::from("新文案")));
    }

    /// A11Y-02 语义槽 + A11Y-03 命中区 + TC-A11Y-TRISTATE-01(IconButton
    /// 半边,三态互异):Button 可访问名缺省可见文本、IconButton 缺省
    /// tooltip;热区 = max(视觉, 24)。
    #[test]
    fn a11y_semantic_slots_hit_expansion_and_icon_tristate() {
        // Button:可见文本即可访问名,label 覆写优先
        let btn = Button::new("b", "确定");
        assert_eq!(
            btn.resolved_semantic().label().map(|s| s.as_ref()),
            Some("确定")
        );
        let named = Button::new("b2", "确定").label("确认导出");
        assert_eq!(
            named.resolved_semantic().label().map(|s| s.as_ref()),
            Some("确认导出"),
            "显式 label 优先"
        );
        assert_eq!(named.resolved_semantic().role(), Some(SemanticRole::Button));
        // IconButton:tooltip 兜底可访问名
        let ib = IconButton::new("i", "◉").tooltip("切换效果启用");
        assert_eq!(
            ib.resolved_semantic().label().map(|s| s.as_ref()),
            Some("切换效果启用")
        );
        assert_eq!(
            ib.resolved_semantic().role(),
            Some(SemanticRole::IconButton)
        );
        let named_ib = ib.label("效果启用开关");
        assert_eq!(
            named_ib.resolved_semantic().label().map(|s| s.as_ref()),
            Some("效果启用开关")
        );
        // 命中区(A11Y-03):Entity 热区 = max(视觉, 24)
        assert!(
            hit_size(button_height(
                ButtonSize::Compact,
                button_text(ButtonSize::Compact)
            )) >= crate::interact::MIN_HIT_PX
        );
        assert!(hit_size(icon_button_side(IconButtonSize::Icon20)) >= crate::interact::MIN_HIT_PX);
        // IconButton 三态互异(hover/press 底可辨、focus = accent 描边)
        for colors in [ColorTokens::dark(), ColorTokens::light()] {
            let idle = icon_button_style(
                IconButtonSize::Icon24,
                ButtonVariant::Secondary,
                InteractState::Idle,
                &colors,
            );
            let hover = icon_button_style(
                IconButtonSize::Icon24,
                ButtonVariant::Secondary,
                InteractState::Hover,
                &colors,
            );
            let press = icon_button_style(
                IconButtonSize::Icon24,
                ButtonVariant::Secondary,
                InteractState::Pressed,
                &colors,
            );
            let ring = icon_button_style(
                IconButtonSize::Icon24,
                ButtonVariant::Secondary,
                InteractState::FocusRing,
                &colors,
            );
            assert_ne!(hover.bg, idle.bg, "hover 可见");
            assert_ne!(press.bg, hover.bg, "press 区别于 hover");
            assert_eq!(ring.border, colors.accent, "focus 描边 = accent");
            assert_ne!(ring.border, idle.border, "未聚焦无 accent 描边");
        }
    }

    #[test]
    fn button_builder_defaults_and_chain() {
        let btn = Button::new("b", "确定");
        assert_eq!(btn.variant, ButtonVariant::Secondary, "默认 Secondary");
        assert_eq!(btn.size, ButtonSize::Default);
        assert!(!btn.disabled);
        assert!(btn.icon.is_none());
        assert_eq!(btn.press_scale(0.0), 1.0, "静止缩放 = 1");
        // builder 链式覆写
        let btn = btn
            .variant(ButtonVariant::Danger)
            .size(ButtonSize::Roomy)
            .disabled(true);
        assert_eq!(btn.variant, ButtonVariant::Danger);
        assert_eq!(btn.size, ButtonSize::Roomy);
        assert!(btn.disabled);
    }
}
