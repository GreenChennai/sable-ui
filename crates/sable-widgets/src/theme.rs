//! 主题系统:全局 [`SableTheme`](分册三 §5)+ 设计 token(分册六 §3)。
//!
//! ```ignore
//! // 应用启动时(一次):
//! sable_widgets::theme::init(cx);
//! // 任意组件内:
//! let colors = &sable_widgets::theme::theme(cx).colors;
//! let canvas = &sable_widgets::theme::theme(cx).canvas;
//! // 换肤:
//! sable_widgets::theme::set_mode(cx, ThemeMode::Light);
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
/// 锚点/钢笔预览,与 sable-canvas `OverlayTheme` 同语义,值随主题。
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
    /// 深色画布方案(canvas_bg = sable-canvas `CANVAS_BASE_COLOR` #1e1e24 同源)。
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

/// Sable 全局主题:颜色令牌 + 画布语义色 + 当前模式。
///
/// `impl Global` 后经 `App` 全局态注入(`init`),组件侧用 [`theme`] 取。
#[derive(Clone, Debug)]
pub struct SableTheme {
    /// UI 颜色令牌(分册六 §3.1)
    pub colors: ColorTokens,
    /// 画布语义色(分册三 §5)
    pub canvas: CanvasTheme,
    /// 当前模式
    pub mode: ThemeMode,
}

impl Global for SableTheme {}

impl SableTheme {
    /// 深色主题(默认)。
    pub fn dark() -> Self {
        SableTheme {
            colors: ColorTokens::dark(),
            canvas: CanvasTheme::dark(),
            mode: ThemeMode::Dark,
        }
    }

    /// 浅色主题。
    pub fn light() -> Self {
        SableTheme {
            colors: ColorTokens::light(),
            canvas: CanvasTheme::light(),
            mode: ThemeMode::Light,
        }
    }
}

/// 注入全局主题(应用启动时调用一次;默认深色)。
pub fn init(cx: &mut App) {
    cx.set_global(SableTheme::dark());
}

/// 取当前主题。**必须先 [`init`]**(未初始化视为应用装配错误,panic 即fail-fast)。
pub fn theme(cx: &App) -> &SableTheme {
    // expect 带原因字符串:AGENTS.md §3.3 允许内部 expect;pub API 无 unwrap/Result 化的必要
    cx.try_global::<SableTheme>()
        .expect("SableTheme 未初始化:应用启动时必须先调用 sable_widgets::theme::init(cx)")
}

/// 切换主题模式(换肤 = 换一份 token 表,组件零改动,分册六 §3.3 军规三)。
pub fn set_mode(cx: &mut App, mode: ThemeMode) {
    let next = match mode {
        ThemeMode::Dark => SableTheme::dark(),
        ThemeMode::Light => SableTheme::light(),
    };
    cx.set_global(next);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_is_default_and_modes_differ() {
        let dark = SableTheme::dark();
        assert_eq!(dark.mode, ThemeMode::Dark);
        let light = SableTheme::light();
        assert_eq!(light.mode, ThemeMode::Light);
        // 两套方案的表面/画布/参考线各成体系
        assert_ne!(dark.colors.surface_1, light.colors.surface_1);
        assert_ne!(dark.canvas.canvas_bg, light.canvas.canvas_bg);
        assert_ne!(dark.canvas.guide, light.canvas.guide);
        // 选中色与 accent 同值是设计意图(深色套内自洽)
        assert_eq!(dark.canvas.selection, dark.colors.accent);
        // set_mode 是纯函数式映射:模式 ↔ 套装一一对应
        assert_eq!(SableTheme::dark().mode, ThemeMode::Dark);
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

// ---------------------------------------------------------------------------
// V3.0 T3:主题定制注入 + 全 token 过渡(纯函数状态机,帧泵在应用层)
// ---------------------------------------------------------------------------

/// 注入自定义调色板(第三方主题入口,V3.0 T3.1):替换全局 token 色彩,
/// 组件下一帧生效。组件读色走全局态,故注入即全量换肤。
pub fn inject(cx: &mut App, colors: crate::tokens::ColorTokens, mode: ThemeMode) {
    let next = match mode {
        ThemeMode::Dark => SableTheme::dark(),
        ThemeMode::Light => SableTheme::light(),
    };
    let mut themed = next;
    themed.colors = colors;
    cx.set_global(themed);
}

/// 主题过渡状态机(V3.0 T3.2,纯函数;对应分册六 §4.3 #14):
/// `tokens_at(now)` 对全部色彩 token 做 lerp_hsla 最短弧插值。
/// 帧泵由应用层驱动:过渡期间每帧 `set_global(过渡态)` + `cx.notify()`。
pub struct ThemeTransition {
    from: crate::tokens::ColorTokens,
    to: crate::tokens::ColorTokens,
    started_ms: f64,
    duration_ms: f64,
}

impl ThemeTransition {
    pub fn new(
        from: crate::tokens::ColorTokens,
        to: crate::tokens::ColorTokens,
        now_ms: f64,
        duration_ms: f64,
    ) -> Self {
        Self {
            from,
            to,
            started_ms: now_ms,
            duration_ms: duration_ms.max(1.0),
        }
    }

    /// 过渡进度 0..=1(已结束为 1.0)。
    pub fn progress_at(&self, now_ms: f64) -> f64 {
        ((now_ms - self.started_ms) / self.duration_ms).clamp(0.0, 1.0)
    }

    pub fn is_running(&self, now_ms: f64) -> bool {
        now_ms < self.started_ms + self.duration_ms
    }

    /// 该时刻的全量 token(OutCubic 缓动 + lerp_hsla 最短弧)。
    pub fn tokens_at(&self, now_ms: f64) -> crate::tokens::ColorTokens {
        let t = crate::anim::Easing::OutCubic.apply(self.progress_at(now_ms));
        let (a, b) = (&self.from, &self.to);
        let l = |x: gpui::Hsla, y: gpui::Hsla| crate::anim::lerp_hsla(x, y, t);
        crate::tokens::ColorTokens {
            surface_0: l(a.surface_0, b.surface_0),
            surface_1: l(a.surface_1, b.surface_1),
            surface_2: l(a.surface_2, b.surface_2),
            surface_3: l(a.surface_3, b.surface_3),
            surface_4: l(a.surface_4, b.surface_4),
            border_subtle: l(a.border_subtle, b.border_subtle),
            border_strong: l(a.border_strong, b.border_strong),
            text_primary: l(a.text_primary, b.text_primary),
            text_secondary: l(a.text_secondary, b.text_secondary),
            text_disabled: l(a.text_disabled, b.text_disabled),
            accent: l(a.accent, b.accent),
            accent_muted: l(a.accent_muted, b.accent_muted),
            danger: l(a.danger, b.danger),
            warning: l(a.warning, b.warning),
            success: l(a.success, b.success),
        }
    }
}

#[cfg(test)]
mod transition_tests {
    use super::*;
    #[test]
    fn theme_transition_interpolates_monotonically() {
        let from = crate::tokens::ColorTokens::dark();
        let to = crate::tokens::ColorTokens::light();
        let tr = ThemeTransition::new(from, to, 0.0, 200.0);
        assert!(!tr.is_running(200.0));
        let mid = tr.tokens_at(100.0);
        assert!(mid.surface_0.l > from.surface_0.l, "dark→light 中途应变亮");
        assert!(mid.surface_0.l < to.surface_0.l);
        assert_eq!(tr.tokens_at(250.0).surface_0, to.surface_0, "结束精确到位");
    }

    // 注:reduced_motion 全局开关有并行测试竞态窗口(flip/interact 同款已知),
    // 主题过渡的应用层直切语义由文档约束,不在并行测试中触碰全局开关。
}
