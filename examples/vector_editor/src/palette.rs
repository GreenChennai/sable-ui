//! 主题取色隔离层:示例中一切颜色经此取自 sable-widgets 主题
//! (AGENTS.md §3.4:禁止散落硬编码色)。
//!
//! 本文件是**唯一的** widgets 主题字段消费点:并行落地期间若 SableTheme/
//! ColorTokens/CanvasTheme 的字段名与设计文档(分册三 §5、分册六 §3)有出入,
//! 只需改这一个文件。

use gpui::{App, Hsla};
use sable::core::scene::Rgba8;
use sable::gpui;

/// 取色入口(字段全部来自主题 token,构造时一次性换算好 RGBA 形态)。
pub struct Palette {
    // —— 表面/文本(gpui::Hsla,直接给 .bg()/.text_color() 用)——
    /// 应用底色
    pub surface_0: Hsla,
    /// 面板底
    pub surface_1: Hsla,
    /// 卡片/输入框
    pub surface_2: Hsla,
    /// 主文本
    pub text_primary: Hsla,
    /// 次级文本
    pub text_secondary: Hsla,
    /// 强调色
    #[allow(dead_code)] // M2 工具栏接线预留
    pub accent: Hsla,
    /// 选中背景(accent 低透明)
    #[allow(dead_code)] // M2 工具栏接线预留
    pub accent_muted: Hsla,
    // —— RGBA8 形态(给 Scene 的 Paint::Solid / 画布底色用)——
    /// 画布底色
    pub canvas_bg: Rgba8,
    /// 强调色(矩形 A 填充)
    pub accent_solid: Rgba8,
    /// 功能色:成功(矩形 B 填充)
    pub success_solid: Rgba8,
    /// 功能色:警示(圆角路径填充)
    pub warning_solid: Rgba8,
}

impl Palette {
    /// 从 sable-widgets 全局主题取色(须在 `sable::dock::init` 之后调用)。
    pub fn get(cx: &App) -> Self {
        // widgets 真实入口:`sable_widgets::theme::theme(cx)`(theme 模块内
        // 自由函数;顶层无 init/theme,见 crates/sable-widgets/src/theme.rs)
        let sable_theme = sable::widgets::theme::theme(cx);
        let colors = &sable_theme.colors;
        Palette {
            surface_0: colors.surface_0,
            surface_1: colors.surface_1,
            surface_2: colors.surface_2,
            text_primary: colors.text_primary,
            text_secondary: colors.text_secondary,
            accent: colors.accent,
            accent_muted: colors.accent_muted,
            canvas_bg: hsla_to_rgba8(sable_theme.canvas.canvas_bg),
            accent_solid: hsla_to_rgba8(colors.accent),
            success_solid: hsla_to_rgba8(colors.success),
            warning_solid: hsla_to_rgba8(colors.warning),
        }
    }
}

/// `gpui::Hsla` → 0-255 RGBA(标准 HSL→RGB;gpui 的 h/s/l/a 均为 0..1 f32)。
pub fn hsla_to_rgba8(c: Hsla) -> Rgba8 {
    let h = c.h.fract() * 6.0;
    let s = c.s.clamp(0.0, 1.0);
    let l = c.l.clamp(0.0, 1.0);
    let (r, g, b) = if s <= 0.0 {
        (l, l, l)
    } else {
        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;
        let hue = |t: f32| {
            let mut t = t;
            if t < 0.0 {
                t += 1.0;
            }
            if t > 1.0 {
                t -= 1.0;
            }
            if t < 1.0 / 6.0 {
                p + (q - p) * 6.0 * t
            } else if t < 0.5 {
                q
            } else if t < 2.0 / 3.0 {
                p + (q - p) * (2.0 / 3.0 - t) * 6.0
            } else {
                p
            }
        };
        (hue(h), hue(h - 2.0), hue(h - 4.0))
    };
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [byte(r), byte(g), byte(b), byte(c.a.clamp(0.0, 1.0))]
}
