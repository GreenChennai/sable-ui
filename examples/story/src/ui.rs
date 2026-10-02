//! story 页面小件:本地按钮 + 卡片容器(全部走 token,零硬编码)。
//!
//! 刻意不引 gpui-component `Button`:其一,本示例只演示 sable 自有组件;
//! 其二,gpui-component 按钮配色走它自己的主题体系,与本仓 token 纪律
//! ("一切颜色走 tokens",AGENTS.md §3.4)不同源。

use sable::gpui::{
    App, Div, InteractiveElement as _, IntoElement, ParentElement, Stateful, Styled, div, px,
};
use sable::widgets::prelude::{RadiusTokens, SpacingTokens, h_flex, v_flex};
use sable::widgets::theme::theme;
use sable::widgets::tokens::{
    FONT_SIZE_BODY, FONT_SIZE_CAPTION, FONT_SIZE_HEADING, HEIGHT_DEFAULT,
};

/// 本地按钮(Stateful div):调用方继续链 `.on_click(...)`。
/// (ColorTokens 是 Copy:复制出值,避免把 cx 借用拖进 hover 闭包。)
pub fn story_button(cx: &App, id: &'static str, label: &'static str) -> Stateful<Div> {
    let c = theme(cx).colors;
    div()
        .id(id)
        .h(px(HEIGHT_DEFAULT))
        .px(px(SpacingTokens::MD))
        .rounded(px(RadiusTokens::MD))
        .bg(c.surface_3)
        .border_1()
        .border_color(c.border_strong)
        .text_size(px(FONT_SIZE_BODY))
        .text_color(c.text_primary)
        .cursor_pointer()
        .hover(move |style| style.bg(c.surface_4))
        .child(label)
}

/// 演示卡片:标题 + 说明文字 + 内容(分册五 §3"每组件一卡"的容器)。
pub fn card(cx: &App, title: &'static str, desc: &'static str, content: impl IntoElement) -> Div {
    let c = theme(cx).colors;
    v_flex()
        .w_full()
        .gap(px(SpacingTokens::MD))
        .p(px(SpacingTokens::LG))
        .rounded(px(RadiusTokens::LG))
        .bg(c.surface_1)
        .border_1()
        .border_color(c.border_subtle)
        .child(
            div()
                .text_size(px(FONT_SIZE_HEADING))
                .text_color(c.text_primary)
                .child(title),
        )
        .child(
            div()
                .text_size(px(FONT_SIZE_CAPTION))
                .text_color(c.text_secondary)
                .child(desc),
        )
        .child(content)
}

/// 分组标题行(页面级)。
pub fn group_title(cx: &App, text: &'static str) -> Div {
    let c = theme(cx).colors;
    h_flex().w_full().mt(px(SpacingTokens::SM)).child(
        div()
            .text_size(px(FONT_SIZE_HEADING))
            .text_color(c.text_secondary)
            .child(text),
    )
}
