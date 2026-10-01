//! 主题取色隔离层:示例中一切颜色经此取自 lumina-widgets 主题
//! (AGENTS.md §3.4:禁止散落硬编码色)。
//!
//! 本文件是**唯一的** widgets 主题字段消费点:并行落地期间若 LuminaTheme/
//! ColorTokens/CanvasTheme 的字段名与设计文档(分册三 §5、分册六 §3)有出入,
//! 只需改这一个文件。

use gpui::App;
use gpui::Hsla;
use lumina::gpui;

/// 取色入口(字段全部来自主题 token)。
pub struct Palette {
    /// 应用底色
    pub surface_0: Hsla,
    /// 面板底
    pub surface_1: Hsla,
    /// 卡片/输入框(监视区底)
    pub surface_2: Hsla,
    /// 主文本
    pub text_primary: Hsla,
    /// 次级文本
    pub text_secondary: Hsla,
    /// 强调色
    #[allow(dead_code)] // M2 工具栏接线预留
    pub accent: Hsla,
    /// 选中背景
    #[allow(dead_code)] // M2 工具栏接线预留
    pub accent_muted: Hsla,
}

impl Palette {
    /// 从 lumina-widgets 全局主题取色(须在 `lumina::dock::init` 之后调用)。
    pub fn get(cx: &App) -> Self {
        // widgets 真实入口:`lumina_widgets::theme::theme(cx)`(theme 模块内
        // 自由函数;顶层无 init/theme,见 crates/lumina-widgets/src/theme.rs)
        let lumina_theme = lumina::widgets::theme::theme(cx);
        let colors = &lumina_theme.colors;
        Palette {
            surface_0: colors.surface_0,
            surface_1: colors.surface_1,
            surface_2: colors.surface_2,
            text_primary: colors.text_primary,
            text_secondary: colors.text_secondary,
            accent: colors.accent,
            accent_muted: colors.accent_muted,
        }
    }
}
