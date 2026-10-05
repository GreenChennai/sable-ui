//! 批 3:ValueOverlay(§5.6 #17 ⭐)——画布浮动数值条:L4 材质 + 圆角 6
//! + caption 白字(剪映/达芬奇拖拽数值浮层同款)。纯展示,零交互。
//!
//! 渲染 = 纯函数规格(令牌)+ 单 div;定位由宿主 absolute 摆放(组件只
//! 管外观)。

use gpui::{IntoElement, ParentElement, SharedString, Styled, px};

use crate::tokens::{ColorTokens, RadiusTokens, SpacingTokens, TextSize};

/// 浮层视觉规格(纯函数):L4 = surface_3 底 + 描边 + 白字(报告 §5.3.2
/// "提示层":92% 不透明由宿主 opacity 施加——组件给足额色,避免在组件
/// 内藏状态)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ValueOverlayStyle {
    pub bg: gpui::Hsla,
    pub fg: gpui::Hsla,
    pub border: gpui::Hsla,
}

/// 令牌 → 规格(纯函数,可测;两主题各自成立)。
#[must_use]
pub fn value_overlay_style(c: &ColorTokens) -> ValueOverlayStyle {
    ValueOverlayStyle {
        bg: c.surface_3,
        fg: c.text_primary,
        border: c.border_strong,
    }
}

/// ValueOverlay 元素:`value_overlay(colors, "42px")`。
pub fn value_overlay(c: &ColorTokens, text: impl Into<SharedString>) -> impl IntoElement {
    let s = value_overlay_style(c);
    gpui::div()
        .px(px(SpacingTokens::XS))
        .py(px(2.0))
        .rounded(px(RadiusTokens::MD))
        .bg(s.bg)
        .border_1()
        .border_color(s.border)
        .text_size(px(TextSize::CAPTION.size))
        .text_color(s.fg)
        .child(text.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_cmp_value_overlay_l4_material_both_modes() {
        // 批 3 ⭐:L4 = surface_3 + border_strong + text_primary,两主题
        for colors in [ColorTokens::dark(), ColorTokens::light()] {
            let s = value_overlay_style(&colors);
            assert_eq!(s.bg, colors.surface_3, "L4 材质 = surface_3");
            assert_eq!(s.border, colors.border_strong);
            assert_eq!(s.fg, colors.text_primary);
        }
    }
}
