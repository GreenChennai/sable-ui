//! 主题桥(迭代审查报告 §5.7.4):把 sable 设计令牌映射进 gpui-component
//! 主题,消除"dock 外观(gpui-component 内置主题)与 widgets 组件(sable
//! tokens)两套视觉语言并存"(上游 VellumBench A-08 同款病)。
//!
//! 映射面 = `ThemeColor` 的**核心语义槽**(背景/前景/边框/静音/强调/功能色);
//! 两侧行 Hsla 同类型世界(gpui 0.2.2),赋值即恒等。未映射的组件专用槽
//! (accordion/description_list 等)保持 gpui-component 预设,随升级窗口
//! 逐槽收敛。深浅由宿主先经 `Theme::change(mode)` 定模式,再调本桥覆盖
//! 色值——顺序写入宿主契约(见 [`apply_sable_colors`] doc)。

use gpui_component::Theme;
use sable_widgets::tokens::ColorTokens;

/// 把 sable 令牌覆盖进 gpui-component 主题(核心语义槽;`theme.colors`
/// 与 `ColorTokens` 同为 gpui `Hsla`,赋值恒等,零转换)。
///
/// 宿主契约:先 `gpui_component::Theme::change(mode)`(gpui-component 全局态
/// 定深浅),后调本桥覆盖色值;widgets 侧再经 `sable::widgets::theme::inject`
/// 同步。测试见 [`tc_dock_theme_bridge_maps_core_slots`](mod tests)。
pub fn apply_sable_colors(theme: &mut Theme, colors: &ColorTokens) {
    let c = colors;
    // —— 容器面 ——
    theme.colors.background = c.surface_0;
    theme.colors.foreground = c.text_primary;
    theme.colors.border = c.border_subtle;
    theme.colors.muted = c.surface_2;
    theme.colors.muted_foreground = c.text_secondary;
    theme.colors.input = c.surface_2;
    theme.colors.popover = c.surface_3;
    theme.colors.popover_foreground = c.text_primary;
    // —— 强调/主操作 ——
    theme.colors.accent = c.accent;
    theme.colors.primary = c.accent;
    // —— 功能色 ——
    theme.colors.danger = c.danger;
    theme.colors.warning = c.warning;
    theme.colors.success = c.success;
    theme.colors.info = c.info;
}

#[cfg(test)]
mod tests {
    use super::*;
    use sable_widgets::tokens::ColorTokens;

    #[test]
    fn tc_dock_theme_bridge_maps_core_slots_both_modes() {
        // 主题桥:核心槽 = sable 令牌恒等赋值(Hsla 同类型世界,零转换)
        let mut theme = Theme::default();

        apply_sable_colors(&mut theme, &ColorTokens::dark());
        let dark = ColorTokens::dark();
        assert_eq!(theme.colors.background, dark.surface_0, "背景 = surface_0");
        assert_eq!(theme.colors.foreground, dark.text_primary);
        assert_eq!(theme.colors.border, dark.border_subtle);
        assert_eq!(theme.colors.muted, dark.surface_2);
        assert_eq!(theme.colors.muted_foreground, dark.text_secondary);
        assert_eq!(theme.colors.popover, dark.surface_3);
        assert_eq!(theme.colors.accent, dark.accent, "强调 = accent");
        assert_eq!(theme.colors.primary, dark.accent, "primary = accent");
        assert_eq!(theme.colors.danger, dark.danger);
        assert_eq!(theme.colors.warning, dark.warning);
        assert_eq!(theme.colors.success, dark.success);
        assert_eq!(theme.colors.info, dark.info);

        // 浅色令牌同样成立(换表重跑即跟随)
        let light = ColorTokens::light();
        apply_sable_colors(&mut theme, &light);
        assert_eq!(theme.colors.background, light.surface_0);
        assert_eq!(theme.colors.accent, light.accent);
    }
}
