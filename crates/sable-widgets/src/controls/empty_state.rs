//! 空态(迭代审查报告 2026-10-04 §5.6 组件矩阵 #13 / §5.9 状态设计表,
//! CMP-05):图标槽 + `display` 标题 + `body` 引导文案 + 主行动按钮 +
//! 可选次行动。
//!
//! ```ignore
//! // 媒体面板空态(§5.9:图层空 → 图标 + "导入或拖入素材" + 主按钮):
//! EmptyState::new("media-empty", "还没有素材")
//!     .icon("◌")
//!     .body("导入或拖入素材开始创作")
//!     .primary("导入素材", move |_cx| { /* 打开文件对话框 */ })
//!     .secondary("查看模板", move |_cx| { /* ... */ })
//! ```
//!
//! # 形态(RenderOnce,规格同源 button 单点)
//!
//! 无跨帧状态(空态是静态布局),逐帧重建安全(CMP-06 纪律);主/次行动
//! 复用 [`crate::controls::button::button_element`] 内联形态(规格/禁用
//! 语义/命中区全部由 `controls::button` 单点保证),不再各写各的按钮。
//!
//! # 视觉规格 = 纯函数(可测,§5.6 #13)
//!
//! 排版单点在 [`empty_state_spec`]:标题 = [`TextSize::DISPLAY`](报告 §5.4:
//! "空态主标题"档)、引导 = [`TextSize::BODY`]、行动按钮 =
//! [`ButtonSize::Default`]——逐项断言见 TC-CMP-STATE-01。颜色一律令牌
//! (图标盒 = `surface_2`、图标 = `text_secondary`、标题 = `text_strong`、
//! 引导 = `text_secondary`),零硬编码色。
//!
//! # 语义(A11Y-02)
//!
//! 可访问名缺省 = 标题文本、role 缺省 = Group;`.label(...)`/`.role(...)`
//! 槽可覆写。空态主行动**可点**:回调经 [`EmptyState::press_route`](纯函数
//! 路由)装配进 Button 的 `on_press`,未装配时路由为空(渲染层不产按钮)。
//!
//! # 减弱动态(A8)
//!
//! 空态无入场动画(装饰性动效对空态是负担而非反馈,§5.2 克制动效),
//! reduced 与正常路径逐位一致——天然直通。

use std::rc::Rc;

use gpui::{
    App, ElementId, FontWeight, IntoElement, ParentElement, RenderOnce, SharedString, Styled,
    Window, div, px,
};

use super::button::{Button, ButtonSize, ButtonVariant, button_element};
use crate::interact::{Semantic, SemanticRole, semantic_slot};
use crate::theme::theme;
use crate::tokens::{RadiusTokens, SpacingTokens, TextSize, h_flex, v_flex};

/// 行动回调(点击后上报宿主;`&mut App` 形态与 `effect_stack` 面板回调
/// 同惯例,宿主在闭包里自取实体句柄)。
pub type EmptyActionFn = Rc<dyn Fn(&mut App)>;

// ---------------------------------------------------------------------------
// 视觉规格(纯函数;TC-CMP-STATE-01 的断言面)
// ---------------------------------------------------------------------------

/// 空态排版/布局规格(纯数据;[`empty_state_spec`] 的输出)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmptyStateSpec {
    /// 图标盒边长 px(48,4 网格)
    pub icon_box_px: f32,
    /// 图标字形字号档(= DISPLAY 的字号,§5.4 已有的最大档,不另立)
    pub icon_text: TextSize,
    /// 标题排版档(§5.6 #13:display)
    pub title: TextSize,
    /// 引导文案排版档(§5.6 #13:body)
    pub body: TextSize,
    /// 行动按钮尺寸(§5.6 #13:主行动 = 默认档)
    pub action_size: ButtonSize,
    /// 引导文案最大宽度 px(320,4 网格;防长句拉满面板)
    pub body_max_w_px: f32,
}

/// 空态规格(纯函数,单一真相):标题 = DISPLAY、引导 = BODY、主行动 =
/// Default 按钮——报告 §5.6 #13 的三档逐项落值。
#[must_use]
pub fn empty_state_spec() -> EmptyStateSpec {
    EmptyStateSpec {
        icon_box_px: 48.0,
        icon_text: TextSize::DISPLAY,
        title: TextSize::DISPLAY,
        body: TextSize::BODY,
        action_size: ButtonSize::Default,
        body_max_w_px: 320.0,
    }
}

/// 行动槽变体映射(纯函数):主 = Primary(§5.9 空态主按钮)、
/// 次 = Ghost(弱化的替代路径,不与主行动争夺视觉权重)。
#[must_use]
pub fn action_variant(primary: bool) -> ButtonVariant {
    if primary {
        ButtonVariant::Primary
    } else {
        ButtonVariant::Ghost
    }
}

// ---------------------------------------------------------------------------
// EmptyState(RenderOnce)
// ---------------------------------------------------------------------------

/// 空态占位(§5.6 #13:首次使用不白板)。
///
/// 图标槽可留空(无图标 = 纯文字空态);主行动可省(纯引导);次行动可省。
#[derive(gpui::IntoElement)]
pub struct EmptyState {
    /// 元素 id 命名空间(内联行动按钮的 id 前缀;同屏多实例各给唯一值)
    id_ns: ElementId,
    icon: Option<SharedString>,
    title: SharedString,
    body: Option<SharedString>,
    /// 主行动(标签, 回调)
    primary: Option<(SharedString, EmptyActionFn)>,
    /// 次行动(标签, 回调)
    secondary: Option<(SharedString, EmptyActionFn)>,
    /// A11Y-02 语义槽(可访问名缺省 = 标题,见 resolved_semantic)
    semantic: Semantic,
}

impl EmptyState {
    /// 空态:`id_ns` 为行动按钮的 id 命名空间(同屏多实例各给唯一值),
    /// `title` 同时是可访问名的缺省值。
    pub fn new(id_ns: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        EmptyState {
            id_ns: id_ns.into(),
            icon: None,
            title: title.into(),
            body: None,
            primary: None,
            secondary: None,
            semantic: Semantic::new(),
        }
    }

    /// 图标槽(文字字形占位,§5.5 SVG 图标系统 = 后续批次;留空 = 纯文字)。
    #[must_use]
    pub fn icon(mut self, glyph: impl Into<SharedString>) -> Self {
        self.icon = Some(glyph.into());
        self
    }

    /// 引导文案(正文档,一行以内最佳)。
    #[must_use]
    pub fn body(mut self, text: impl Into<SharedString>) -> Self {
        self.body = Some(text.into());
        self
    }

    /// 主行动(标签 + 点击回调;Primary 变体,§5.9 空态主按钮)。
    /// 同槽覆写:后写胜。
    pub fn primary(
        mut self,
        label: impl Into<SharedString>,
        f: impl Fn(&mut App) + 'static,
    ) -> Self {
        self.primary = Some((label.into(), Rc::new(f)));
        self
    }

    /// 次行动(标签 + 点击回调;Ghost 变体,可选)。同槽覆写:后写胜。
    pub fn secondary(
        mut self,
        label: impl Into<SharedString>,
        f: impl Fn(&mut App) + 'static,
    ) -> Self {
        self.secondary = Some((label.into(), Rc::new(f)));
        self
    }

    /// 主行动槽只读(测试断言面):`Some((标签, 回调))` = 可点。
    #[must_use]
    pub fn primary_action(&self) -> Option<&(SharedString, EmptyActionFn)> {
        self.primary.as_ref()
    }

    /// 次行动槽只读(测试断言面)。
    #[must_use]
    pub fn secondary_action(&self) -> Option<&(SharedString, EmptyActionFn)> {
        self.secondary.as_ref()
    }

    /// 主行动按压路由(纯函数,TC-CMP-STATE-01"主行动可点"的被测单点):
    /// 已装配则返回回调克隆——渲染层把它原样接进 Button 的 `on_press`
    /// ([`button_element`]),未装配则 `None`(渲染层跳过按钮)。
    #[must_use]
    pub fn press_route(&self) -> Option<EmptyActionFn> {
        self.primary.as_ref().map(|(_, cb)| Rc::clone(cb))
    }

    /// 行动按钮的内联 id(命名空间派生,同 button.rs 的 NamedChild 惯例)。
    fn action_id(&self, slot: &'static str) -> ElementId {
        ElementId::NamedChild(Box::new(self.id_ns.clone()), SharedString::from(slot))
    }

    /// 解析语义(A11Y-02):显式 `.label(...)` 优先,缺省 = 标题;
    /// role 默认 [`SemanticRole::Group`]。
    #[must_use]
    pub fn resolved_semantic(&self) -> Semantic {
        let mut sem = Semantic::new().with_label(self.title.clone());
        if let Some(over) = self.semantic.label() {
            sem = sem.with_label(over.clone());
        }
        sem.with_role(self.semantic.role().unwrap_or(SemanticRole::Group))
    }
}

// A11Y-02 语义槽(label/role/semantic 三件):可访问名缺省回落标题
// (见 resolved_semantic)。
semantic_slot!(EmptyState);

impl RenderOnce for EmptyState {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = theme(cx).colors;
        let spec = empty_state_spec();
        let semantic = self.resolved_semantic();

        let mut root = v_flex()
            .items_center()
            .gap(px(SpacingTokens::MD))
            .px(px(SpacingTokens::XL))
            .py(px(SpacingTokens::LG));
        if let Some(glyph) = self.icon.clone() {
            root = root.child(
                h_flex()
                    .justify_center()
                    .items_center()
                    .size(px(spec.icon_box_px))
                    .rounded(px(RadiusTokens::XL))
                    .bg(colors.surface_2)
                    .text_size(px(spec.icon_text.size))
                    .text_color(colors.text_secondary)
                    .child(glyph),
            );
        }
        root = root.child(
            div()
                .text_size(px(spec.title.size))
                .font_weight(FontWeight(spec.title.weight))
                .text_color(colors.text_strong)
                .child(self.title.clone()),
        );
        if let Some(body) = self.body.clone() {
            root = root.child(
                div()
                    .max_w(px(spec.body_max_w_px))
                    .text_size(px(spec.body.size))
                    .text_color(colors.text_secondary)
                    .child(body),
            );
        }

        // 行动行:主(Primary)/次(Ghost);规格同源 button 单点,回调经
        // press_route 的同源闭包装配(可点性由 Button 组件的热区保证)
        let has_actions = self.primary.is_some() || self.secondary.is_some();
        if has_actions {
            let mut actions = h_flex().gap(px(SpacingTokens::SM));
            if let Some((label, cb)) = self.primary.as_ref() {
                let cb = Rc::clone(cb);
                actions = actions.child(button_element(
                    Button::new(self.action_id("primary"), label.clone())
                        .variant(action_variant(true))
                        .size(spec.action_size)
                        .on_press(move |_ev, _win, cx| cb(cx)),
                    cx,
                ));
            }
            if let Some((label, cb)) = self.secondary.as_ref() {
                let cb = Rc::clone(cb);
                actions = actions.child(button_element(
                    Button::new(self.action_id("secondary"), label.clone())
                        .variant(action_variant(false))
                        .size(spec.action_size)
                        .on_press(move |_ev, _win, cx| cb(cx)),
                    cx,
                ));
            }
            root = root.child(actions);
        }
        crate::interact::attach_semantics(root, &semantic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TC-CMP-STATE-01(§5.6 #13 规格):标题 = DISPLAY、引导 = BODY、
    /// 主行动 = Default 档按钮;图标盒 4 网格;引导宽度上限守网格。
    #[test]
    fn tc_cmp_state_01_spec_matches_report_5_6_13() {
        let spec = empty_state_spec();
        assert_eq!(
            spec.title,
            TextSize::DISPLAY,
            "标题 = display 档(报告 §5.4)"
        );
        assert_eq!(spec.body, TextSize::BODY, "引导 = body 档");
        assert_eq!(spec.action_size, ButtonSize::Default, "主行动 = 默认档按钮");
        assert_eq!(spec.icon_text, TextSize::DISPLAY, "图标字形 = 既有最大档");
        assert_eq!(spec.icon_box_px % 4.0, 0.0, "图标盒在 4 网格");
        assert_eq!(spec.body_max_w_px % 4.0, 0.0, "引导宽度上限在 4 网格");
        assert_eq!(spec.icon_box_px, 48.0);
    }

    /// TC-CMP-STATE-01(主行动可点,回调装配):primary 槽存位、
    /// press_route 返回**同一**回调(渲染层装配进 on_press 的就是它)、
    /// 变体映射 Primary/Ghost、次行动独立、未装配时路由为空(渲染层据此
    /// 跳过按钮)。回调体执行需活 App(gpui 无测试头台),闭包行为与按钮
    /// 热区语义由 `controls::button` 的单点规格保证(与本仓 tabs/select
    /// 的回调测试纪律一致)。
    #[test]
    fn tc_cmp_state_01_primary_action_wired_and_routed() {
        let state = EmptyState::new("media-empty", "还没有素材")
            .icon("◌")
            .body("导入或拖入素材开始创作")
            .primary("导入素材", |_cx: &mut App| {})
            .secondary("查看模板", |_cx: &mut App| {});
        // 主行动存位且标签正确
        let (label, _) = state.primary_action().expect("主行动应已装配");
        assert_eq!(label.as_ref(), "导入素材");
        // 路由 = 同一回调实例(Rc 指针相等):渲染层接进 on_press 的就是它
        let route = state.press_route().expect("路由应为 Some(可点)");
        assert!(Rc::ptr_eq(&route, &state.primary_action().expect("存位").1));
        // 变体映射:主 = Primary、次 = Ghost(§5.9 空态主按钮)
        assert_eq!(action_variant(true), ButtonVariant::Primary);
        assert_eq!(action_variant(false), ButtonVariant::Ghost);
        // 次行动独立存位
        let (label2, _) = state.secondary_action().expect("次行动应已装配");
        assert_eq!(label2.as_ref(), "查看模板");
        // 未装配时路由为空(纯引导空态,渲染层不产按钮)
        let bare = EmptyState::new("bare", "暂无数据");
        assert!(bare.press_route().is_none());
        assert!(bare.primary_action().is_none());
        assert!(bare.secondary_action().is_none());
        // 槽位覆写语义:后写胜(字段不是追加)
        let twice = EmptyState::new("e", "a")
            .primary("first", |_cx: &mut App| {})
            .primary("second", |_cx: &mut App| {});
        assert_eq!(
            twice.primary_action().expect("存位").0.as_ref(),
            "second",
            "同槽后写胜"
        );
    }

    /// TC-CMP-STATE-01(语义槽):可访问名缺省 = 标题、role 缺省 Group;
    /// `.label(...)`/`.role(...)` 显式覆写优先(A11Y-02)。
    #[test]
    fn tc_cmp_state_01_semantic_slot_defaults_and_overrides() {
        let state = EmptyState::new("e", "还没有素材");
        assert_eq!(
            state.resolved_semantic().label().map(|s| s.as_ref()),
            Some("还没有素材"),
            "可访问名缺省 = 标题"
        );
        assert_eq!(
            state.resolved_semantic().role(),
            Some(SemanticRole::Group),
            "role 缺省 = Group"
        );
        let named = state.label("媒体库空态").role(SemanticRole::List);
        assert_eq!(
            named.resolved_semantic().label().map(|s| s.as_ref()),
            Some("媒体库空态"),
            "显式 label 优先"
        );
        assert_eq!(named.resolved_semantic().role(), Some(SemanticRole::List));
        // 槽位覆写语义:后写胜(字段不是追加)
        let twice = EmptyState::new("e2", "a").icon("i").icon("j");
        assert_eq!(twice.icon.as_ref().map(SharedString::as_ref), Some("j"));
    }
}
