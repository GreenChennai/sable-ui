//! 错误状态件(迭代审查报告 2026-10-04 §5.6 组件矩阵 #16 / §5.9 状态设计表,
//! CMP-05):[`ErrorBar`](内联错误条)/[`ErrorVariant::Card`](错误卡)。
//!
//! ```ignore
//! // 媒体加载失败(内联错误条:左 danger 描边 + 图标 + 消息 + 重试):
//! cx.new(|_| {
//!     ErrorBar::new("media-err", "素材加载失败")
//!         .details("文件不存在或已损坏:shot_04.mov(错误 404)")
//!         .code("E-MEDIA-404")
//!         .on_retry(|_cx| { /* 重新加载 */ })
//! })
//! ```
//!
//! # 规格(§5.6 #16)
//!
//! 左 danger 描边([`ERROR_EDGE_W_PX`] 竖条,绝对定位实现——gpui 0.2.2 无
//! 单侧描边 API)+ 图标 + 消息 + **可展开详情** + 重试按钮(回调)+
//! 错误码槽(mono 徽章)。条(Bar)= 面板底 + 圆角 MD 的行内形态;卡
//! (Card)= 凸起底 + hairline 描边 + 圆角 LG 的独立卡片形态;色裁决单点
//! 在 [`error_visual`](纯函数,深浅两主题逐项断言)。
//!
//! # 展开/收起状态机(可测,TC 的断言面)
//!
//! - 意图纯函数 [`detail_toggle_intent`]:enter/space 翻转、escape 只收不展
//!   (逐级退出语义,§5.10.1)、无详情键位无意图;
//! - 进度纯函数 [`expand_progress_at`]:200ms
//!   ([`EXPAND_DURATION_MS`] = [`MotionTokens::DUR_PANEL_MS`] 面板档)
//!   InOutCubic;`reduced_motion` 直通目标值(展即全显、收即全隐);
//! - 只有存在详情时才有切换头(键盘 focus 只在可展开时入 Tab 序)。
//!
//! # 语义(A11Y-02)
//!
//! 可访问名缺省 = 错误消息、role 缺省 = Group;`.label(...)`/`.role(...)`
//! 槽可覆写。展开头为 [`crate::controls::button::IconButton`](tooltip 兜底
//! 可访问名"展开/收起详情");重试复用 [`Button`](Primary 变体 = 错误态的
//! 主行动)。重试回调 [`RetryFn`] 经 [`ErrorBar::retry_route`](纯函数路由)
//! 装配,未装配则不渲染按钮。

use std::rc::Rc;

use gpui::{
    App, Context, ElementId, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, Styled, Window, div, px,
};

use super::button::{Button, ButtonVariant, IconButton, button_element, icon_button_element};
use crate::anim::{Easing, reduced_motion};
use crate::interact::{self, Semantic, SemanticRole, semantic_slot};
use crate::theme::theme;
use crate::tokens::{
    ColorTokens, MotionTokens, RadiusTokens, SpacingTokens, TextSize, h_flex, v_flex,
};

/// 重试回调(`&mut App` 形态,与 `effect_stack`/`empty_state` 面板回调同惯例)。
pub type RetryFn = Rc<dyn Fn(&mut App)>;

// ---------------------------------------------------------------------------
// 常量与规格(具名单点;颜色一律令牌)
// ---------------------------------------------------------------------------

/// 展开/收起时长(毫秒)= 动效四档的 PANEL 档(200ms;详情块是面板级内容,
/// §5.8)。reduced 直通(A8)。
pub const EXPAND_DURATION_MS: f64 = MotionTokens::DUR_PANEL_MS;

/// 左 danger 描边宽(px)。描边类尺寸(与 [`crate::interact::RING_BORDER_PX`]
/// 同类)不落 4px 间距网格;3px 在条形态上可辨且不挤行高。
pub const ERROR_EDGE_W_PX: f32 = 3.0;

/// 错误图标字形(§5.5 图标系统落地前的文字字形占位,Button.icon 同纪律)。
const ERROR_GLYPH: &str = "⚠";
/// 展开头字形(收起态)。
const EXPAND_GLYPH: &str = "▸";
/// 展开头字形(展开态)。
const COLLAPSE_GLYPH: &str = "▾";
/// 重试按钮缺省文案。
const DEFAULT_RETRY_LABEL: &str = "重试";

// ---------------------------------------------------------------------------
// 纯函数层(视觉裁决 / 键盘意图 / 展开进度;TC 的被测单点)
// ---------------------------------------------------------------------------

/// 错误件形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorVariant {
    /// 内联错误条:面板底 + 圆角 MD,行内(列表行下/面板底部)
    Bar,
    /// 错误卡:凸起底 + hairline 描边 + 圆角 LG,独立卡片(面板整块)
    Card,
}

/// 错误件一帧的完整视觉裁决(纯数据;[`error_visual`] 的输出)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ErrorVisual {
    /// 容器底色
    pub bg: gpui::Hsla,
    /// 容器整体描边(Bar = 无;Card = border_subtle hairline)
    pub border: Option<gpui::Hsla>,
    /// 左 danger 描边(恒 danger 令牌,两主题一致)
    pub stripe: gpui::Hsla,
    /// 图标色(恒 danger 令牌)
    pub icon: gpui::Hsla,
    /// 消息文字(恒 text_primary)
    pub message: gpui::Hsla,
    /// 详情文字(text_secondary)
    pub details: gpui::Hsla,
    /// 错误码徽章文字(text_secondary)
    pub code: gpui::Hsla,
    /// 容器圆角(Bar = MD,Card = LG)
    pub radius: f32,
}

/// 视觉裁决(纯函数):条纹/图标恒 danger、消息恒 text_primary(错误语义
/// 不随形态漂移);形态只裁决容器(bg/描边/圆角)。深浅两主题逐项断言见
/// TC 测试。
#[must_use]
pub fn error_visual(variant: ErrorVariant, colors: &ColorTokens) -> ErrorVisual {
    match variant {
        ErrorVariant::Bar => ErrorVisual {
            bg: colors.surface_1,
            border: None,
            stripe: colors.danger,
            icon: colors.danger,
            message: colors.text_primary,
            details: colors.text_secondary,
            code: colors.text_secondary,
            radius: RadiusTokens::MD,
        },
        ErrorVariant::Card => ErrorVisual {
            bg: colors.surface_2,
            border: Some(colors.border_subtle),
            stripe: colors.danger,
            icon: colors.danger,
            message: colors.text_primary,
            details: colors.text_secondary,
            code: colors.text_secondary,
            radius: RadiusTokens::LG,
        },
    }
}

/// 展开/收起键位意图(纯函数,状态机单点):`enter`/`space` 翻转;
/// `escape` 只收不展(§5.10.1 逐级退出;已收起时无意图);其余键 `None`。
#[must_use]
pub fn detail_toggle_intent(key: &str, expanded: bool) -> Option<bool> {
    match key {
        "enter" | "space" => Some(!expanded),
        "escape" if expanded => Some(false),
        _ => None,
    }
}

/// 展开进度(纯函数):`started_ms` = 最近一次切换时刻(`None` = 从未切换,
/// 直落当前态);200ms InOutCubic 从切换前的状态滑向目标;`reduced` 直通
/// 目标值(展即全显、收即全隐,A8)。
#[must_use]
pub fn expand_progress_at(
    started_ms: Option<f64>,
    now_ms: f64,
    expanded: bool,
    reduced: bool,
) -> f64 {
    let target = if expanded { 1.0 } else { 0.0 };
    if reduced {
        return target;
    }
    let Some(started) = started_ms else {
        return target;
    };
    let elapsed = (now_ms - started).max(0.0);
    if elapsed >= EXPAND_DURATION_MS {
        return target;
    }
    let eased = Easing::InOutCubic.apply(elapsed / EXPAND_DURATION_MS);
    let from = if expanded { 0.0 } else { 1.0 };
    from + (target - from) * eased
}

/// 展开动画是否进行中(纯函数;为真时宿主续帧,reduced 恒假即停帧)。
#[must_use]
pub fn expand_animating(started_ms: Option<f64>, now_ms: f64, reduced: bool) -> bool {
    if reduced {
        return false;
    }
    started_ms.is_some_and(|started| (now_ms - started).max(0.0) < EXPAND_DURATION_MS)
}

// ---------------------------------------------------------------------------
// ErrorBar(Entity:展开态 + 动画相位是跨帧状态)
// ---------------------------------------------------------------------------

/// 错误条/错误卡(§5.6 #16;Entity 形态——展开态跨帧驻留)。
pub struct ErrorBar {
    /// 元素 id 命名空间(内部按钮 id 前缀;同屏多实例各给唯一值)
    id_ns: ElementId,
    message: SharedString,
    details: Option<SharedString>,
    code: Option<SharedString>,
    retry_label: Option<SharedString>,
    on_retry: Option<RetryFn>,
    variant: ErrorVariant,
    expanded: bool,
    /// 最近一次展开/收起切换时刻(`None` = 从未切换)
    expand_started_ms: Option<f64>,
    /// 焦点句柄(可展开时惰性创建,入 Tab 序)
    focus: Option<FocusHandle>,
    /// A11Y-02 语义槽(可访问名缺省 = 消息,见 resolved_semantic)
    semantic: Semantic,
}

impl ErrorBar {
    /// 错误件:`id_ns` 为内部按钮的 id 命名空间,`message` 同时是可访问名
    /// 的缺省值。
    pub fn new(id_ns: impl Into<ElementId>, message: impl Into<SharedString>) -> Self {
        ErrorBar {
            id_ns: id_ns.into(),
            message: message.into(),
            details: None,
            code: None,
            retry_label: None,
            on_retry: None,
            variant: ErrorVariant::Bar,
            expanded: false,
            expand_started_ms: None,
            focus: None,
            semantic: Semantic::new(),
        }
    }

    /// 可展开详情(文本;设置后出现展开头并进入 Tab 序)。
    #[must_use]
    pub fn details(mut self, text: impl Into<SharedString>) -> Self {
        self.details = Some(text.into());
        self
    }

    /// 错误码槽(mono 徽章,头部右侧;如 `E-MEDIA-404`)。
    #[must_use]
    pub fn code(mut self, code: impl Into<SharedString>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// 形态(默认 [`ErrorVariant::Bar`];整块面板用 [`ErrorVariant::Card`])。
    #[must_use]
    pub fn variant(mut self, variant: ErrorVariant) -> Self {
        self.variant = variant;
        self
    }

    /// 重试回调(未装配则不渲染重试按钮)。
    pub fn on_retry(mut self, f: impl Fn(&mut App) + 'static) -> Self {
        self.on_retry = Some(Rc::new(f));
        self
    }

    /// 重试按钮文案(缺省 = "重试")。
    #[must_use]
    pub fn retry_label(mut self, label: impl Into<SharedString>) -> Self {
        self.retry_label = Some(label.into());
        self
    }

    /// 初始展开态(构造用;运行时切换走 [`Self::toggle`]/[`Self::set_expanded`])。
    #[must_use]
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// 是否存在可展开详情(展开头是否渲染的裁决)。
    #[must_use]
    pub fn has_details(&self) -> bool {
        self.details.is_some()
    }

    /// 当前展开态。
    #[must_use]
    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// 重试回调路由(纯函数,TC 的被测单点):已装配则返回回调克隆——
    /// 渲染层把它接进 Button 的 `on_press`;未装配 `None`(不渲染按钮)。
    #[must_use]
    pub fn retry_route(&self) -> Option<RetryFn> {
        self.on_retry.as_ref().map(Rc::clone)
    }

    /// 重试文案(缺省回落"重试")。
    #[must_use]
    pub fn resolved_retry_label(&self) -> SharedString {
        self.retry_label
            .clone()
            .unwrap_or_else(|| SharedString::from(DEFAULT_RETRY_LABEL))
    }

    /// 切换展开/收起(无详情时为 no-op:没有展开头也就没有切换入口)。
    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.details.is_some() {
            self.set_expanded(!self.expanded, cx);
        }
    }

    /// 受控写入展开态:记录切换时刻(动画相位从当前态滑向目标)、通知。
    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        if expanded == self.expanded {
            return;
        }
        self.expanded = expanded;
        self.expand_started_ms = Some(interact::now_ms());
        cx.notify();
    }

    /// 当前展开进度(渲染期求值;宿主调试可读)。
    #[must_use]
    pub fn expand_progress(&self, now_ms: f64, reduced: bool) -> f64 {
        expand_progress_at(self.expand_started_ms, now_ms, self.expanded, reduced)
    }

    /// 键盘意图落地(enter/space 翻转、escape 收起;无详情时全忽略)。
    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(next) = detail_toggle_intent(&event.keystroke.key, self.expanded) {
            self.set_expanded(next, cx);
        }
    }

    /// 内部按钮的内联 id(命名空间派生,button.rs 的 NamedChild 惯例)。
    fn action_id(&self, slot: &'static str) -> ElementId {
        ElementId::NamedChild(Box::new(self.id_ns.clone()), SharedString::from(slot))
    }

    /// 解析语义(A11Y-02):显式 `.label(...)` 优先,缺省 = 错误消息;
    /// role 默认 [`SemanticRole::Group`]。
    #[must_use]
    pub fn resolved_semantic(&self) -> Semantic {
        let mut sem = Semantic::new().with_label(self.message.clone());
        if let Some(over) = self.semantic.label() {
            sem = sem.with_label(over.clone());
        }
        sem.with_role(self.semantic.role().unwrap_or(SemanticRole::Group))
    }
}

// A11Y-02 语义槽(label/role/semantic 三件):可访问名缺省回落错误消息
// (见 resolved_semantic)。
semantic_slot!(ErrorBar);

impl Render for ErrorBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let vis = error_visual(self.variant, &colors);
        let now = interact::now_ms();
        let reduced = reduced_motion();
        let progress = self.expand_progress(now, reduced);
        let animating = expand_animating(self.expand_started_ms, now, reduced);
        let has_details = self.has_details();

        // 可展开时才入 Tab 序(A11Y-01:焦点可达 = 可交互)
        let focus = if has_details {
            Some(
                self.focus
                    .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
                    .clone(),
            )
        } else {
            None
        };

        // 容器(形态裁决:条 = 面板底圆角 MD;卡 = 凸起底 + hairline 圆角 LG;
        // overflow_hidden 裁剪左描边条贴合圆角)
        let mut container = v_flex()
            .relative()
            .overflow_hidden()
            .bg(vis.bg)
            .rounded(px(vis.radius))
            .p(px(SpacingTokens::MD));
        if let Some(border) = vis.border {
            container = container.border_1().border_color(border);
        }

        // 头部行:图标 + 消息(flex_1)+ 错误码徽章 + 重试 + 展开头
        let mut header = h_flex().gap(px(SpacingTokens::SM));
        header = header.child(
            div()
                .flex_shrink_0()
                .text_size(px(TextSize::TITLE.size))
                .text_color(vis.icon)
                .child(ERROR_GLYPH),
        );
        header = header.child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(TextSize::BODY_STRONG.size))
                .font_weight(gpui::FontWeight(TextSize::BODY_STRONG.weight))
                .text_color(vis.message)
                .child(self.message.clone()),
        );
        if let Some(code) = self.code.clone() {
            header = header.child(
                div()
                    .flex_shrink_0()
                    .px(px(SpacingTokens::XS))
                    .py(px(SpacingTokens::XS / 2.0))
                    .rounded(px(RadiusTokens::SM))
                    .bg(colors.surface_3)
                    .text_size(px(TextSize::MONO.size))
                    .text_color(vis.code)
                    .child(code),
            );
        }
        if let Some(cb) = self.retry_route() {
            header = header.child(button_element(
                Button::new(self.action_id("retry"), self.resolved_retry_label())
                    .variant(ButtonVariant::Primary)
                    .on_press(move |_ev, _win, cx| cb(cx)),
                cx,
            ));
        }
        if has_details {
            // 展开头(IconButton 内联形态):字形随态翻转,tooltip 兜底
            // 可访问名(A11Y-02);切换经实体句柄回写(实体自持展开态)
            let entity = cx.entity();
            let (glyph, tip) = if self.expanded {
                (COLLAPSE_GLYPH, "收起详情")
            } else {
                (EXPAND_GLYPH, "展开详情")
            };
            header = header.child(icon_button_element(
                IconButton::new(self.action_id("toggle"), glyph)
                    .tooltip(tip)
                    .on_press(move |_ev, _win, cx| {
                        entity.update(cx, |bar, cx| bar.toggle(cx));
                    }),
                cx,
            ));
        }
        container = container.child(header);

        // 详情块(展开进度驱动:容器随进度出现、文字随进度淡入)
        if progress > 0.0
            && let Some(details) = self.details.clone()
        {
            container = container.child(
                div()
                    .mt(px(SpacingTokens::SM))
                    .text_size(px(TextSize::BODY.size))
                    .text_color(vis.details)
                    .opacity(f32_clamp01(progress))
                    .child(details),
            );
        }

        let mut root = container;
        if let Some(focus) = &focus {
            root = root
                .track_focus(focus)
                .on_key_down(cx.listener(Self::on_key_down));
        }
        if animating {
            window.request_animation_frame();
        }
        // A11Y-02:语义挂接单点透传(可访问名/role 见 resolved_semantic)
        let semantic = self.resolved_semantic();
        interact::attach_semantics(root, &semantic)
    }
}

/// f64 进度 → f32 透明度(钳制 [0,1];GPU 域收口)。
#[allow(clippy::cast_possible_truncation)]
fn f32_clamp01(v: f64) -> f32 {
    if v.is_finite() {
        (v as f32).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: f64 = 5_000.0;

    /// TC-CMP-STATE-ERROR-BAR(展开/收起状态机·意图层):enter/space 翻转、
    /// escape 只收不展、无详情/其它键无意图。
    #[test]
    fn tc_cmp_state_error_bar_toggle_intent_state_machine() {
        // 翻转(两态对称)
        assert_eq!(detail_toggle_intent("enter", false), Some(true));
        assert_eq!(detail_toggle_intent("enter", true), Some(false));
        assert_eq!(detail_toggle_intent("space", false), Some(true));
        assert_eq!(detail_toggle_intent("space", true), Some(false));
        // escape:只收不展(逐级退出语义)
        assert_eq!(detail_toggle_intent("escape", true), Some(false));
        assert_eq!(detail_toggle_intent("escape", false), None, "已收起无意图");
        // 其余键无意图
        for key in ["up", "down", "left", "tab", "a", ""] {
            assert_eq!(detail_toggle_intent(key, false), None, "{key} 无意图");
            assert_eq!(detail_toggle_intent(key, true), None, "{key} 无意图");
        }
    }

    /// TC-CMP-STATE-ERROR-BAR(展开/收起状态机·进度层):200ms PANEL 档
    /// InOutCubic、两端精确、reduced 直通;从未切换直落当前态。
    #[test]
    fn tc_cmp_state_error_bar_expand_progress_and_reduced() {
        assert_eq!(
            EXPAND_DURATION_MS,
            MotionTokens::DUR_PANEL_MS,
            "面板档 200ms"
        );
        assert_eq!(EXPAND_DURATION_MS, 200.0);
        // 从未切换:直落当前态
        assert_eq!(expand_progress_at(None, T0, true, false), 1.0);
        assert_eq!(expand_progress_at(None, T0, false, false), 0.0);
        // 展开:0 → 200ms 从 0 滑到 1(半程 InOutCubic 中点恰半)
        assert_eq!(expand_progress_at(Some(T0), T0, true, false), 0.0, "起步");
        let mid = expand_progress_at(Some(T0), T0 + 100.0, true, false);
        assert!((mid - 0.5).abs() < 1e-9, "半程恰半:{mid}");
        assert_eq!(
            expand_progress_at(Some(T0), T0 + 200.0, true, false),
            1.0,
            "到位"
        );
        assert_eq!(
            expand_progress_at(Some(T0), T0 + 1_000.0, true, false),
            1.0,
            "超时钳满"
        );
        // 收起:从 1 滑回 0(对称)
        let mid_close = expand_progress_at(Some(T0), T0 + 100.0, false, false);
        assert!((mid_close - 0.5).abs() < 1e-9);
        assert_eq!(expand_progress_at(Some(T0), T0 + 200.0, false, false), 0.0);
        // 前半程单调上行
        let mut prev = -1.0;
        for i in 0..=10 {
            let p = expand_progress_at(Some(T0), T0 + f64::from(i) * 10.0, true, false);
            assert!(p > prev, "展开单调:{p} ≤ {prev}");
            prev = p;
        }
        // reduced 直通:任意时刻 = 目标值
        for dt in [0.0, 1.0, 100.0, 199.0] {
            assert_eq!(expand_progress_at(Some(T0), T0 + dt, true, true), 1.0);
            assert_eq!(expand_progress_at(Some(T0), T0 + dt, false, true), 0.0);
        }
        // 动画进行中判定:中途真、到位/末尾假、reduced 恒假(停帧)
        assert!(expand_animating(Some(T0), T0 + 100.0, false));
        assert!(!expand_animating(Some(T0), T0 + 200.0, false));
        assert!(!expand_animating(Some(T0), T0 + 1.0, true));
        assert!(!expand_animating(None, T0, false));
    }

    /// TC-CMP-STATE-ERROR-BAR(重试回调装配):retry_route 返回同一回调
    /// (渲染层接进 on_press 的就是它)、文案缺省"重试"可覆写、未装配则
    /// 路由为空(渲染层不产按钮)。回调体执行需活 App(gpui 无测试头台),
    /// 闭包行为与按钮热区由 `controls::button` 单点保证(本仓回调测试纪律)。
    #[test]
    fn tc_cmp_state_error_bar_retry_route_wired() {
        let bar = ErrorBar::new("err", "素材加载失败")
            .code("E-MEDIA-404")
            .on_retry(|_cx: &mut App| {});
        let route = bar.retry_route().expect("重试应已装配");
        assert!(Rc::ptr_eq(&route, bar.on_retry.as_ref().expect("存位")));
        assert_eq!(bar.resolved_retry_label().as_ref(), "重试", "缺省文案单点");
        let custom = bar.retry_label("重新加载");
        assert_eq!(custom.resolved_retry_label().as_ref(), "重新加载");
        // 未装配:路由空 → 渲染层不渲染重试按钮
        let bare = ErrorBar::new("err2", "加载失败");
        assert!(bare.retry_route().is_none());
        assert_eq!(
            bare.resolved_retry_label().as_ref(),
            "重试",
            "无回调也有缺省文案"
        );
    }

    /// TC-CMP-STATE-ERROR-BAR(语义 role + 视觉裁决):可访问名缺省 = 消息、
    /// role 缺省 Group、显式覆写优先;Bar/Card 两形态在深浅两主题的容器
    /// 裁决(条纹/图标恒 danger、消息恒 text_primary,圆角档位随形态)。
    #[test]
    fn tc_cmp_state_error_bar_semantic_role_and_visual() {
        // 语义
        let bar = ErrorBar::new("e", "素材加载失败");
        assert_eq!(
            bar.resolved_semantic().label().map(|s| s.as_ref()),
            Some("素材加载失败"),
            "可访问名缺省 = 消息"
        );
        assert_eq!(bar.resolved_semantic().role(), Some(SemanticRole::Group));
        let named = bar.label("媒体加载错误").role(SemanticRole::List);
        assert_eq!(
            named.resolved_semantic().label().map(|s| s.as_ref()),
            Some("媒体加载错误"),
            "显式 label 优先"
        );
        assert_eq!(named.resolved_semantic().role(), Some(SemanticRole::List));
        // 状态机装配:details 有无 → has_details;expanded 构造位
        let plain = ErrorBar::new("p", "m");
        assert!(!plain.has_details());
        assert!(!plain.is_expanded());
        let open = plain.details("详情文本").expanded(true);
        assert!(open.has_details() && open.is_expanded());
        // 视觉裁决:深浅两主题 × 两形态
        for colors in [ColorTokens::dark(), ColorTokens::light()] {
            let bar_vis = error_visual(ErrorVariant::Bar, &colors);
            assert_eq!(bar_vis.bg, colors.surface_1, "条 = 面板底");
            assert_eq!(bar_vis.border, None, "条无整体描边");
            assert_eq!(bar_vis.radius, RadiusTokens::MD);
            let card_vis = error_visual(ErrorVariant::Card, &colors);
            assert_eq!(card_vis.bg, colors.surface_2, "卡 = 凸起底");
            assert_eq!(card_vis.border, Some(colors.border_subtle), "卡 = hairline");
            assert_eq!(card_vis.radius, RadiusTokens::LG);
            for vis in [bar_vis, card_vis] {
                assert_eq!(vis.stripe, colors.danger, "左描边恒 danger");
                assert_eq!(vis.icon, colors.danger, "图标恒 danger");
                assert_eq!(vis.message, colors.text_primary, "消息恒正文档");
                assert_eq!(vis.details, colors.text_secondary);
                assert_eq!(vis.code, colors.text_secondary);
            }
        }
    }
}
