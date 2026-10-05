//! 批 3:Badge/Chip(§5.6 #18)——pill 圆角 + caption 字,零交互纯展示。
//!
//! 变体四色(中性/强调/成功/危险),文本经令牌取反色(对比度由
//! gate_contrast 全局保证的 text_strong/surface 系)。

use crate::tokens::{ColorTokens, SpacingTokens, TextSize};
use gpui::{IntoElement, ParentElement, SharedString, Styled, px};

/// 徽章语义色(取令牌功能色;中性 = surface_3/text_secondary)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeVariant {
    /// 中性(灰)。
    Neutral,
    /// 强调(accent)。
    Accent,
    /// 成功。
    Success,
    /// 危险。
    Danger,
}

/// 徽章视觉规格(纯函数;状态无——Badge 是纯展示件)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BadgeStyle {
    pub bg: gpui::Hsla,
    pub fg: gpui::Hsla,
}

/// 变体 + 令牌 → 视觉(纯函数,可测)。
#[must_use]
pub fn badge_style(variant: BadgeVariant, c: &ColorTokens) -> BadgeStyle {
    match variant {
        BadgeVariant::Neutral => BadgeStyle {
            bg: c.surface_3,
            fg: c.text_secondary,
        },
        BadgeVariant::Accent => BadgeStyle {
            bg: c.accent,
            fg: c.text_strong,
        },
        BadgeVariant::Success => BadgeStyle {
            bg: c.success,
            fg: c.text_strong,
        },
        BadgeVariant::Danger => BadgeStyle {
            bg: c.danger,
            fg: c.text_strong,
        },
    }
}

/// Badge 元素:`badge(variant, colors, "标签")`。
pub fn badge(
    variant: BadgeVariant,
    c: &ColorTokens,
    label: impl Into<SharedString>,
) -> impl IntoElement {
    let s = badge_style(variant, c);
    gpui::div()
        .px(px(SpacingTokens::XS))
        .py(px(1.0))
        .rounded_full() // pill = 高度一半(gpui 内建,非令牌数值)
        .bg(s.bg)
        .text_size(px(TextSize::CAPTION.size))
        .text_color(s.fg)
        .child(label.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_cmp_badge_variants_map_tokens() {
        // 批 3:四变体色 = 令牌恒等(Hsla 同类型世界,零转换);
        // Accent/Success/Danger 前景 = text_strong(对比度由门禁保证)。
        for colors in [ColorTokens::dark(), ColorTokens::light()] {
            let n = badge_style(BadgeVariant::Neutral, &colors);
            assert_eq!(n.bg, colors.surface_3);
            let a = badge_style(BadgeVariant::Accent, &colors);
            assert_eq!(a.bg, colors.accent);
            assert_eq!(a.fg, colors.text_strong);
            assert_eq!(badge_style(BadgeVariant::Danger, &colors).bg, colors.danger);
        }
    }
}
