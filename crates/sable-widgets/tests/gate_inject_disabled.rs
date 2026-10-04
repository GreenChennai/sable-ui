//! G-15 / TOK-06 + TOK-07:主题注入语义与禁用态统一规则门禁
//! (迭代审查报告 2026-10-04 TOK-06/COUP-04/TOK-07/§5.9;
//! TC-TOK-INJECT-01 / TC-TOK-DISABLED-01)。
//!
//! # TC-TOK-INJECT-01(注入语义与文档一致 + 可定制画布色)
//!
//! - [`injected_theme`](`None` 分支)与旧 `inject` 的组装语义**逐位一致**
//!   (深/浅两模式 × UI 色全量 + 画布 8 槽逐一对拍;历史组装在测试内原样
//!   内联作对拍基准,防"两侧一起改"漂移);
//! - `Some(自定义 CanvasTheme)` 时画布 8 槽(canvas_bg/artboard_bg/grid/
//!   guide/selection/anchor/anchor_selected/pen_preview)**全部取注入值**,
//!   UI 色与 `mode` 不受影响(第三方换肤可定制画布配色,TOK-06 修复点);
//! - 反面自检:注入后画布必须 ≠ mode 预设(防"注入未生效"假绿)。
//!
//! `inject`/`inject_with` 的全局落盘是 `cancel_transition + set_global`
//! 两行(纯组装经 [`injected_theme`] 单点);gpui `App` 无 test-support
//! 不可构造(见 binding.rs 测试注记),故对拍锚定在纯组装单点。
//!
//! # TC-TOK-DISABLED-01(禁用 = 仅前景降级、容器不变)
//!
//! - 令牌层:`StateLayerTokens::{dark,light}.disabled == 0.0`(容器叠加恒零);
//! - interact 纯函数:`state_layer(surface, Disabled, _) == surface`
//!   **逐位不变**(深/浅 × surface_0..4 全表面);
//! - `disabled_foreground`:任何启用态前景档 → `text_disabled`(六档收敛);
//! - NumberField 落地件:`disabled_visual` 的容器(底/描边)与**静止态**
//!   逐位相同(不整体降饱和),前景 == `text_disabled` 且 ≠ 启用档
//!   (反面自检:防"未降级"假绿)。

use gpui::{Hsla, hsla};

use sable_widgets::interact::{InteractState, disabled_foreground, state_layer};
use sable_widgets::number_field::disabled_visual;
use sable_widgets::theme::{CanvasTheme, SableTheme, ThemeMode, injected_theme};
use sable_widgets::tokens::{ColorTokens, StateLayerTokens};

/// 画布 8 语义槽的具名展开(逐槽对拍用,顺序 = [`CanvasTheme`] 字段序)。
fn canvas_slots(c: CanvasTheme) -> [(&'static str, Hsla); 8] {
    [
        ("canvas_bg", c.canvas_bg),
        ("artboard_bg", c.artboard_bg),
        ("grid", c.grid),
        ("guide", c.guide),
        ("selection", c.selection),
        ("anchor", c.anchor),
        ("anchor_selected", c.anchor_selected),
        ("pen_preview", c.pen_preview),
    ]
}

/// 旧 `inject`(V3.0 T3.1)的组装语义**原样内联**:TC-TOK-INJECT-01 的
/// 对拍基准。`inject_with(.., None)` 与之逐位一致是本轮的兼容性承诺。
fn legacy_inject_composition(colors: ColorTokens, mode: ThemeMode) -> SableTheme {
    let mut themed = match mode {
        ThemeMode::Dark => SableTheme::dark(),
        ThemeMode::Light => SableTheme::light(),
    };
    themed.colors = colors;
    themed
}

/// TC-TOK-INJECT-01:`inject_with` 传 `Some(自定义画布)` → 画布 8 槽生效;
/// 不传(`None`)→ 与旧 `inject` 行为逐位一致(两模式 × 全部槽位对拍)。
#[test]
fn tc_tok_inject_01_custom_canvas_applies_and_none_is_bitwise_legacy() {
    // 自定义皮肤:UI 色取深色套改 accent(异于深浅两套预设),画布 8 槽
    // 全部为测试内显式构造值(两两不同,可指认到槽)
    let mut ui = ColorTokens::dark();
    ui.accent = hsla(0.62, 0.75, 0.42, 1.0);
    let custom_canvas = CanvasTheme {
        canvas_bg: hsla(0.10, 0.20, 0.30, 1.0),
        artboard_bg: hsla(0.11, 0.20, 0.31, 1.0),
        grid: hsla(0.12, 0.20, 0.32, 1.0),
        guide: hsla(0.13, 0.20, 0.33, 1.0),
        selection: hsla(0.14, 0.20, 0.34, 1.0),
        anchor: hsla(0.15, 0.20, 0.35, 1.0),
        anchor_selected: hsla(0.16, 0.20, 0.36, 1.0),
        pen_preview: hsla(0.17, 0.20, 0.37, 1.0),
    };

    for mode in [ThemeMode::Dark, ThemeMode::Light] {
        let preset = match mode {
            ThemeMode::Dark => CanvasTheme::dark(),
            ThemeMode::Light => CanvasTheme::light(),
        };

        // (a) None:与旧 inject 组装逐位一致(V3.0/V4.0 兼容语义不变)
        let legacy = legacy_inject_composition(ui, mode);
        let via_none = injected_theme(ui, mode, None);
        assert_eq!(via_none.mode, legacy.mode, "mode 字段落为参数");
        assert_eq!(
            via_none.colors, legacy.colors,
            "UI 色全量槽位与旧 inject 逐位一致({mode:?})"
        );
        for ((name, got), (_, want)) in canvas_slots(via_none.canvas)
            .into_iter()
            .zip(canvas_slots(legacy.canvas))
        {
            assert_eq!(got, want, "canvas 槽 {name} 与旧 inject 逐位一致({mode:?})");
        }
        assert_eq!(
            via_none.canvas, preset,
            "None = mode 预设(画布重置的兼容语义,{mode:?})"
        );
        assert_eq!(via_none.colors, ui, "注入的 UI 色全量生效");

        // (b) Some(自定义画布):8 槽全部取注入值,UI 色/mode 不受影响
        let via_some = injected_theme(ui, mode, Some(custom_canvas));
        assert_ne!(
            via_some.canvas, preset,
            "反面自检:注入必须真的改了画布,否则本用例失效({mode:?})"
        );
        assert_eq!(via_some.canvas, custom_canvas, "画布整体 = 注入值");
        for ((name, got), (_, want)) in canvas_slots(via_some.canvas)
            .into_iter()
            .zip(canvas_slots(custom_canvas))
        {
            assert_eq!(
                got, want,
                "注入后画布槽 {name} 应逐位等于自定义值({mode:?})"
            );
        }
        assert_eq!(via_some.colors, ui, "画布定制不影响 UI 色注入");
        assert_eq!(via_some.mode, mode, "mode 字段落为参数");
    }
}

/// TC-TOK-DISABLED-01:禁用前后容器色逐位不变、前景收敛 `text_disabled`
/// (interact 纯函数 + NumberField 落地件,深浅两主题全覆盖)。
#[test]
fn tc_tok_disabled_01_container_bitwise_unchanged_foreground_converges() {
    // 令牌层:禁用叠加 alpha 恒 0(容器不变的令牌侧承诺)
    assert_eq!(StateLayerTokens::dark().disabled, 0.0);
    assert_eq!(StateLayerTokens::light().disabled, 0.0);

    for tokens in [ColorTokens::dark(), ColorTokens::light()] {
        let name = if tokens.text_primary.l > 0.5 {
            "dark"
        } else {
            "light"
        };

        // (1) interact 纯函数:Disabled 容器逐位不变(全部 5 级表面)
        for (ix, surface) in [
            tokens.surface_0,
            tokens.surface_1,
            tokens.surface_2,
            tokens.surface_3,
            tokens.surface_4,
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(
                state_layer(*surface, InteractState::Disabled, tokens.accent),
                *surface,
                "{name} surface_{ix} 禁用容器必须逐位不变"
            );
        }

        // (2) 前景规则:任何启用态前景档 → text_disabled(禁用档与占位档
        // 也在列,规则是"恒收敛",不特判)
        for (tier, fg) in [
            ("text_strong", tokens.text_strong),
            ("text_primary", tokens.text_primary),
            ("text_secondary", tokens.text_secondary),
            ("text_tertiary", tokens.text_tertiary),
            ("text_disabled", tokens.text_disabled),
            ("text_placeholder", tokens.text_placeholder),
        ] {
            assert_eq!(
                disabled_foreground(fg, tokens.text_disabled),
                tokens.text_disabled,
                "{name} {tier} 禁用必须收敛 text_disabled"
            );
        }

        // (3) NumberField 落地件:容器(底/描边)与静止态逐位相同(h/s/l/a
        // 四通道单列断言,"逐位"可见),前景 = text_disabled
        let v = disabled_visual(&tokens);
        assert_eq!(
            (v.bg.h, v.bg.s, v.bg.l, v.bg.a),
            (
                tokens.surface_2.h,
                tokens.surface_2.s,
                tokens.surface_2.l,
                tokens.surface_2.a
            ),
            "{name} 禁用容器 bg 必须与静止态逐位相同(不整体降饱和/变色)"
        );
        assert_eq!(
            (v.border.h, v.border.s, v.border.l, v.border.a),
            (
                tokens.border_subtle.h,
                tokens.border_subtle.s,
                tokens.border_subtle.l,
                tokens.border_subtle.a
            ),
            "{name} 禁用容器描边必须与静止态逐位相同"
        );
        assert_eq!(
            v.fg, tokens.text_disabled,
            "{name} 禁用前景 = text_disabled"
        );
        assert_ne!(
            v.fg, tokens.text_secondary,
            "{name} 反面自检:前景确实降级(防未降级假绿)"
        );
        assert_eq!(
            v.bg.l, tokens.surface_2.l,
            "{name} 容器亮度不得因禁用改变(§5.9:容器不变)"
        );
    }
}
