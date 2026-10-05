//! 批 3:Card/GroupBox(§5.6 #19)——hairline 描边 + LG 圆角 + L2 材质
//! (surface_2 底)+ 内边距 S4;纯容器,内容为子元素闭包。

use crate::tokens::{ColorTokens, RadiusTokens, SpacingTokens};
use gpui::{IntoElement, Styled, px};

/// 卡片视觉规格(纯函数):L2 凸起材质。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardStyle {
    /// 底色 = surface_2(L2 凸起,TOK-01 elevation 阶梯)。
    pub bg: gpui::Hsla,
    /// 描边 = border_subtle(hairline)。
    pub border: gpui::Hsla,
}

/// 令牌 → 卡片规格(纯函数,可测)。
#[must_use]
pub fn card_style(c: &ColorTokens) -> CardStyle {
    CardStyle {
        bg: c.surface_2,
        border: c.border_subtle,
    }
}

/// Card 元素:`card(colors, |div| div.child(...))`——内容闭包拿裸 div
/// 自行排版(容器只管材质/圆角/内边距)。
pub fn card(c: &ColorTokens, content: impl FnOnce(gpui::Div) -> gpui::Div) -> impl IntoElement {
    let s = card_style(c);
    content(
        gpui::div()
            .w_full()
            .bg(s.bg)
            .border_1()
            .border_color(s.border)
            .rounded(px(RadiusTokens::LG))
            .p(px(SpacingTokens::MD)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_cmp_card_l2_material_from_tokens() {
        // 批 3:卡片 = surface_2(L2)+ border_subtle,两主题取值随令牌
        for colors in [ColorTokens::dark(), ColorTokens::light()] {
            let s = card_style(&colors);
            assert_eq!(s.bg, colors.surface_2, "L2 凸起材质");
            assert_eq!(s.border, colors.border_subtle);
        }
    }
}
