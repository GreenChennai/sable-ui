//! 属性行与分组标题(分册三 §3 组件三件套的"受控组件"容器)。
//!
//! 检查器里所有行的统一版式:28px 行高(派生制,见下)+ 72px 固定宽标签
//! (左对齐,多字段左缘自动成线——与上游 field_row 规格同语义)+ 右侧控件
//! 区。行本身无状态(RenderOnce);状态在控件里(NumberField 等 Entity 或
//! 外部 Binding)。
//!
//! ```ignore
//! PropertyRow::new("X").control(NumberField::new(x_binding))
//! PropertyRow::new("填充").control(ColorWell::new(color))
//! ```
//!
//! # 零硬编码纪律
//! 颜色一律 `theme(cx)` 语义色;尺寸全部来自 tokens(行高用
//! [`control_height`] 派生制:默认档 26 只是下限,正文 12px 行高约 16 +
//! 2×6 padding = 28 实际生效,不压 CJK 字形)。
//!
//! # A7 微交互(hover 态,分册六 §4.3 #1)
//!
//! 行悬停时底色 = [`hover_tint`](crate::interact::hover_tint)(surface_1 基色
//! 加亮 4%,静止时不着色、视觉零变化)。**即时切换而非 120ms 插值**:本组件
//! 是 RenderOnce(无跨帧状态可持 [`HoverState`](crate::interact::HoverState)),
//! 插值版接线示范见 NumberField(有状态 Entity);把属性行包进有状态容器后
//! 可用同款状态机升级 = TODO-M2。行本身不可点击,不设 pressed 态。

use gpui::{
    App, Div, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString, Styled,
    Window, div, px,
};

use crate::interact;
use crate::theme::theme;
use crate::tokens::{
    FONT_SIZE_BODY, FONT_SIZE_HEADING, SpacingTokens, control_height, h_flex, v_flex,
};

/// 属性行标签列宽(分册三 §3 示例的 72px 固定宽)。
pub const LABEL_WIDTH_PX: f32 = 72.0;
/// 面板正文的估行高(CJK 派生基准:12px 字号 ≈ 1.33 倍行高,取 16)。
pub const LABEL_LINE_HEIGHT_PX: f32 = 16.0;
/// 行的垂直内边距(上下各 6px;16 + 12 = 28 为实际行高)。
pub const ROW_V_PADDING_PX: f32 = 6.0;

/// 属性行:`PropertyRow::new("X").control(number_field)`(分册三 §3 用法)。
#[derive(gpui::IntoElement)]
pub struct PropertyRow {
    label: SharedString,
    control: Option<gpui::AnyElement>,
}

impl PropertyRow {
    /// 新属性行(标签左对齐,固定 [`LABEL_WIDTH_PX`] 宽);控件经
    /// [`.control`](Self::control) 追加。
    pub fn new(label: impl Into<SharedString>) -> Self {
        PropertyRow {
            label: label.into(),
            control: None,
        }
    }

    /// 挂右侧控件(任意 IntoElement:NumberField Entity / ColorWell / 文本)。
    pub fn control(mut self, el: impl IntoElement) -> Self {
        self.control = Some(el.into_any_element());
        self
    }

    /// 实际行高(派生制):`max(26, 行高 16 + 2×6)` = 28。
    pub fn row_height() -> f32 {
        control_height(
            crate::tokens::HEIGHT_DEFAULT,
            LABEL_LINE_HEIGHT_PX,
            ROW_V_PADDING_PX,
        )
    }
}

impl RenderOnce for PropertyRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &theme(cx).colors;
        // gpui 0.2.2 没有 impl IntoElement for Option(已核实),None 用空占位
        let control = self.control.unwrap_or_else(|| div().into_any_element());
        // A7:hover 底色 = surface_1 加亮 4%(即时切换,理由见模块 doc)
        let hover_bg = interact::hover_tint(colors.surface_1);
        h_flex()
            .h(px(PropertyRow::row_height()))
            .gap(px(SpacingTokens::SM))
            .hover(move |style| style.bg(hover_bg))
            .child(
                div()
                    .w(px(LABEL_WIDTH_PX))
                    .flex_shrink_0()
                    .text_size(px(FONT_SIZE_BODY))
                    .text_color(colors.text_secondary)
                    .child(self.label),
            )
            .child(div().flex_1().min_w_0().child(control))
    }
}

/// 分组标题 + 内容(v0.1 静态展示,无折叠交互;折叠头动效 = M2,对齐上游
/// SectionHeader 规格 docs/upstream/02 §4.2)。
///
/// 头部刻意**不着色**(标题文字颜色继承父链,避免无 cx 入口却写死颜色;
/// 视觉层级由字号 13/加粗/行高承担),内容区紧随其下由调用方布局。
pub fn section(title: impl Into<SharedString>, content: impl IntoElement) -> gpui::AnyElement {
    // 显式定目标类型:`title.into()` 直塞 child(impl IntoElement) 会让
    // .into() 的目标类型多义(E0283/E0308),先落成 SharedString
    let title: SharedString = title.into();
    let header: Div = h_flex().child(
        div()
            .text_size(px(FONT_SIZE_HEADING))
            .font_weight(gpui::FontWeight::MEDIUM)
            .child(title),
    );
    v_flex()
        .gap(px(SpacingTokens::XS))
        .child(header)
        .child(content.into_any_element())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_height_is_content_derived_above_default_tier() {
        // 16 行高 + 12 padding = 28 > 默认档 26:派生制生效
        assert_eq!(PropertyRow::row_height(), 28.0);
        // 与 tokens::control_height 的口径一致
        assert_eq!(
            PropertyRow::row_height(),
            control_height(
                crate::tokens::HEIGHT_DEFAULT,
                LABEL_LINE_HEIGHT_PX,
                ROW_V_PADDING_PX
            )
        );
    }

    #[test]
    fn property_row_builder_accepts_any_element() {
        let row = PropertyRow::new("X").control(div().child("1.0"));
        let _ = row; // 构造成功即编译期验收(渲染路径需 App,不在此测)
        let empty = PropertyRow::new("无控件");
        let _ = empty;
    }
}
