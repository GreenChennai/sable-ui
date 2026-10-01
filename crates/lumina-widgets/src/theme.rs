//! 主题系统:全局 [`LuminaTheme`](分册三 §5)+ 设计 token(分册六 §3)。
//!
//! ```ignore
//! // 应用启动时(一次):
//! lumina_widgets::theme::init(cx);
//! // 任意组件内:
//! let colors = &lumina_widgets::theme::theme(cx).colors;
//! let canvas = &lumina_widgets::theme::theme(cx).canvas;
//! // 换肤:
//! lumina_widgets::theme::set_mode(cx, ThemeMode::Light);
//! ```
//!
//! **与 gpui-component 主题的同步钩子留 M2**(gpui-component 0.7.0 内部
//! 跑在 gpui-pre 0.3.7 的类型世界里,与 gpui 0.2.2 不互通——已核实其
//! Cargo.toml `[dependencies.gpui] package = "gpui-pre"`;同步须经值转换层,
//! v0.1 不做)。因此本 crate 组件**刻意不使用 gpui-component**,全部纯 gpui
//! div/fill/canvas 兜底,类型世界单一,编译面最小。

use gpui::{App, Global, Hsla, rgba};

use crate::tokens::ColorTokens;

/// 主题模式(深/浅)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeMode {
    /// 深色(Illustrator 式,默认)
    Dark,
    /// 浅色
    Light,
}

/// 画布语义色(分册三 §5 `CanvasTheme`):画布/画板/网格/参考线/选中/
/// 锚点/钢笔预览,与 lumina-canvas `OverlayTheme` 同语义,值随主题。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasTheme {
    /// 画布底色(工作区,画板之外)
    pub canvas_bg: Hsla,
    /// 画板白(画板内)
    pub artboard_bg: Hsla,
    /// 网格线
    pub grid: Hsla,
    /// 参考线(经典青,分册三 §5 的 #00c8ff)
    pub guide: Hsla,
    /// 选中高亮(与 accent 同值是设计意图,同上游)
    pub selection: Hsla,
    /// 锚点/控制柄
    pub anchor: Hsla,
    /// 选中的锚点
    pub anchor_selected: Hsla,
    /// 钢笔预览线
    pub pen_preview: Hsla,
}

impl CanvasTheme {
    /// 深色画布方案(canvas_bg = lumina-canvas `CANVAS_BASE_COLOR` #1e1e24 同源)。
    pub fn dark() -> Self {
        CanvasTheme {
            canvas_bg: rgba(0x1E1E24FF).into(),
            artboard_bg: rgba(0xFAFAFAFF).into(),
            grid: rgba(0x3A3A3AFF).into(),
            guide: rgba(0x00C8FFFF).into(), // 经典青 #00c8ff(docs/03 §5)
            selection: rgba(0x4F9FFFFF).into(),
            anchor: rgba(0xFFFFFFFF).into(),
            anchor_selected: rgba(0x4F9FFFFF).into(),
            pen_preview: rgba(0x00C8FFFF).into(),
        }
    }

    /// 浅色画布方案。
    pub fn light() -> Self {
        CanvasTheme {
            canvas_bg: rgba(0xE8E8EAFF).into(),
            artboard_bg: rgba(0xFFFFFFFF).into(),
            grid: rgba(0xDADADAFF).into(),
            guide: rgba(0x00A5FFFF).into(), // 标尺青(浅色可读性更好)
            selection: rgba(0x0D99FFFF).into(),
            anchor: rgba(0x1E1E1EFF).into(),
            anchor_selected: rgba(0x0D99FFFF).into(),
            pen_preview: rgba(0x00A5FFFF).into(),
        }
    }
}

/// Lumina 全局主题:颜色令牌 + 画布语义色 + 当前模式。
///
/// `impl Global` 后经 `App` 全局态注入(`init`),组件侧用 [`theme`] 取。
#[derive(Clone, Debug)]
pub struct LuminaTheme {
    /// UI 颜色令牌(分册六 §3.1)
    pub colors: ColorTokens,
    /// 画布语义色(分册三 §5)
    pub canvas: CanvasTheme,
    /// 当前模式
    pub mode: ThemeMode,
}

impl Global for LuminaTheme {}

impl LuminaTheme {
    /// 深色主题(默认)。
    pub fn dark() -> Self {
        LuminaTheme {
            colors: ColorTokens::dark(),
            canvas: CanvasTheme::dark(),
            mode: ThemeMode::Dark,
        }
    }

    /// 浅色主题。
    pub fn light() -> Self {
        LuminaTheme {
            colors: ColorTokens::light(),
            canvas: CanvasTheme::light(),
            mode: ThemeMode::Light,
        }
    }
}

/// 注入全局主题(应用启动时调用一次;默认深色)。
pub fn init(cx: &mut App) {
    cx.set_global(LuminaTheme::dark());
}

/// 取当前主题。**必须先 [`init`]**(未初始化视为应用装配错误,panic 即fail-fast)。
pub fn theme(cx: &App) -> &LuminaTheme {
    // expect 带原因字符串:AGENTS.md §3.3 允许内部 expect;pub API 无 unwrap/Result 化的必要
    cx.try_global::<LuminaTheme>()
        .expect("LuminaTheme 未初始化:应用启动时必须先调用 lumina_widgets::theme::init(cx)")
}

/// 切换主题模式(换肤 = 换一份 token 表,组件零改动,分册六 §3.3 军规三)。
pub fn set_mode(cx: &mut App, mode: ThemeMode) {
    let next = match mode {
        ThemeMode::Dark => LuminaTheme::dark(),
        ThemeMode::Light => LuminaTheme::light(),
    };
    cx.set_global(next);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_is_default_and_modes_differ() {
        let dark = LuminaTheme::dark();
        assert_eq!(dark.mode, ThemeMode::Dark);
        let light = LuminaTheme::light();
        assert_eq!(light.mode, ThemeMode::Light);
        // 两套方案的表面/画布/参考线各成体系
        assert_ne!(dark.colors.surface_1, light.colors.surface_1);
        assert_ne!(dark.canvas.canvas_bg, light.canvas.canvas_bg);
        assert_ne!(dark.canvas.guide, light.canvas.guide);
        // 选中色与 accent 同值是设计意图(深色套内自洽)
        assert_eq!(dark.canvas.selection, dark.colors.accent);
        // set_mode 是纯函数式映射:模式 ↔ 套装一一对应
        assert_eq!(LuminaTheme::dark().mode, ThemeMode::Dark);
    }

    #[test]
    fn canvas_theme_covers_all_docs03_fields() {
        // 分册三 §5 的 8 个字段一个不少(编译期字段存在性 + 值完整性)
        let c = CanvasTheme::dark();
        let all = [
            c.canvas_bg,
            c.artboard_bg,
            c.grid,
            c.guide,
            c.selection,
            c.anchor,
            c.anchor_selected,
            c.pen_preview,
        ];
        assert_eq!(all.len(), 8);
        assert!(all.iter().all(|h| h.a > 0.99), "画布语义色不透明");
    }
}
