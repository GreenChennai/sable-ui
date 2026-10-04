//! G-UI-C / TOK-03:WCAG AA 对比度门禁(迭代审查报告 2026-10-04 §5.10.4
//! 可访问性 4 + §5.11 G-UI-C;TC-TOK-A11Y-01 / TC-TOK-A11Y-02)。
//!
//! # 门禁矩阵(阈值按报告口径,不做 WCAG 豁免)
//!
//! - 深浅两主题 × 文字档 {strong, primary, secondary, tertiary, disabled,
//!   placeholder} × 表面 {surface_0/1/2} → 合成后 ≥ **4.5:1**。**disabled
//!   同样按 4.5 断言**(比 WCAG 对禁用态的豁免更严,报告明确要求"照做");
//!   半透明文字档先经 [`composite_over`] 落到实际表面再测——直接拿原始色
//!   算会在 alpha 上失真;
//! - 功能色文字 {accent, danger, warning, success, info} 前景用法 × 表面
//!   {surface_0/1/2} → ≥ 4.5:1;
//! - 焦点环(accent,非文本)× 全部表面 {surface_0..4} → ≥ **3:1**(WCAG
//!   1.4.11 非文本对比度);
//! - 选中底(accent @ selected alpha 叠加 surface_1)单列:其上的文字档
//!   {strong, primary, secondary} 仍 ≥ 4.5:1(选中不得摧毁可读性)。
//!
//! # 历史(TOK-03"不达标当场改值")
//!
//! 2026-10-04 首次运行本矩阵:13 个组合不达标(旧值最低 2.0:1),当场改值
//! 而非调阈值——新旧对照见 tokens.rs 注释与 sable-tokens.json `$description`
//! (深色弱文字档改亮;浅色弱文字档/accent/danger/warning 加深)。本文件
//! 是回归门禁:今后任何一侧改值跌破线即红。
//!
//! # 反面测试(TC-TOK-A11Y-02)
//!
//! 用**内联变换**注入低对比色(改 token 集合的内存副本,零文件),证明
//! 裁决函数会红并指认到具体令牌与表面对。

use gpui::{Hsla, rgba};

use sable_widgets::tokens::{ColorTokens, StateLayerTokens, composite_over, contrast_ratio};

/// WCAG AA 正文阈值(报告口径,禁用/占位不豁免)。
const AA_TEXT: f32 = 4.5;
/// 非文本(UI 组件边界/焦点环)阈值。
const NON_TEXT: f32 = 3.0;

/// 文字档(6 档;disabled 在列)。
fn text_pairs(t: &ColorTokens) -> Vec<(&'static str, Hsla)> {
    vec![
        ("text-strong", t.text_strong),
        ("text-primary", t.text_primary),
        ("text-secondary", t.text_secondary),
        ("text-tertiary", t.text_tertiary),
        ("text-disabled", t.text_disabled),
        ("text-placeholder", t.text_placeholder),
    ]
}

/// 功能色(前景用法)。
fn functional_pairs(t: &ColorTokens) -> Vec<(&'static str, Hsla)> {
    vec![
        ("accent", t.accent),
        ("danger", t.danger),
        ("warning", t.warning),
        ("success", t.success),
        ("info", t.info),
    ]
}

/// 文字承载表面(0..2;更亮/更暗的 hover/pressed 态由 3:1 焦点环列覆盖)。
fn surface_pairs(t: &ColorTokens) -> Vec<(&'static str, Hsla)> {
    vec![
        ("surface-0", t.surface_0),
        ("surface-1", t.surface_1),
        ("surface-2", t.surface_2),
    ]
}

/// 对比度矩阵裁决(纯函数):返回违例清单(空 = 全绿)。TC-TOK-A11Y-02
/// 复用同一裁决注入低对比色,证明门禁会红。
fn contrast_violations(theme: &str, t: &ColorTokens) -> Vec<String> {
    let mut bad = Vec::new();
    let surfaces = surface_pairs(t);
    for (t_name, t_color) in text_pairs(t).iter().chain(functional_pairs(t).iter()) {
        for (s_name, s_color) in &surfaces {
            let ratio = contrast_ratio(composite_over(*t_color, *s_color), *s_color);
            if ratio < AA_TEXT {
                bad.push(format!(
                    "{theme}.{t_name} on {s_name}: {ratio:.2}:1 < {AA_TEXT}:1"
                ));
            }
        }
    }
    bad
}

/// TC-TOK-A11Y-01(门禁本体):两主题对比度矩阵全绿。
#[test]
fn tc_tok_a11y_01_contrast_matrix_both_themes_pass() {
    for (theme, t) in [
        ("dark", ColorTokens::dark()),
        ("light", ColorTokens::light()),
    ] {
        let bad = contrast_violations(theme, &t);
        assert!(
            bad.is_empty(),
            "TOK-03 对比度矩阵 {theme} 主题违例 {} 处(不达标必须改值,禁止调阈值):\n  - {}",
            bad.len(),
            bad.join("\n  - ")
        );
    }
}

/// TC-TOK-A11Y-01(焦点环/选中底单列):焦点环(accent,非文本)对全部
/// 5 级表面 ≥ 3:1;选中底(accent @ 14% 叠 surface_1)上的文字档仍 ≥ 4.5:1。
#[test]
fn tc_tok_a11y_01_focus_ring_and_selected_columns_pass() {
    for (theme, t) in [
        ("dark", ColorTokens::dark()),
        ("light", ColorTokens::light()),
    ] {
        // 焦点环:accent 是焦点环的实色来源(StateLayerTokens::focus_ring
        // 的绘制接线为 accent 不透明度 1.0),对所有表面非文本 3:1
        let surfaces = [
            ("surface-0", t.surface_0),
            ("surface-1", t.surface_1),
            ("surface-2", t.surface_2),
            ("surface-3", t.surface_3),
            ("surface-4", t.surface_4),
        ];
        for (s_name, s_color) in &surfaces {
            let ratio = contrast_ratio(t.accent, *s_color);
            assert!(
                ratio >= NON_TEXT,
                "{theme} 焦点环(accent)on {s_name}: {ratio:.2}:1 < {NON_TEXT}:1"
            );
        }
        // 选中底:surface_1 + accent @ selected alpha(StateLayerTokens 单点,
        // 深浅同比例 = TC-TOK-STATE-02);行内文字在选中态仍须可读
        let selected_alpha = StateLayerTokens::dark().selected;
        assert_eq!(selected_alpha, StateLayerTokens::light().selected);
        let selected_bg = composite_over(with_alpha(t.accent, selected_alpha), t.surface_1);
        for (t_name, t_color) in [
            ("text-strong", t.text_strong),
            ("text-primary", t.text_primary),
            ("text-secondary", t.text_secondary),
        ] {
            let ratio = contrast_ratio(composite_over(t_color, selected_bg), selected_bg);
            assert!(
                ratio >= AA_TEXT,
                "{theme} 选中底上的 {t_name}: {ratio:.2}:1 < {AA_TEXT}:1"
            );
        }
    }
}

/// 复制颜色并替换 alpha(Hsla 字段直改;8-bit 量化在 composite_over 内发生,
/// 与渲染侧 rgba8 路径一致)。
fn with_alpha(c: Hsla, a: f32) -> Hsla {
    Hsla { a, ..c }
}

/// TC-TOK-A11Y-02(反面):注入低对比色必须红,且违例消息指认到令牌。
/// 内联变换 token 集合(内存副本,零文件),同一裁决函数复用。
#[test]
fn tc_tok_a11y_02_low_contrast_injection_is_red() {
    // 前置:原版全绿
    assert!(contrast_violations("dark", &ColorTokens::dark()).is_empty());
    assert!(contrast_violations("light", &ColorTokens::light()).is_empty());

    // 注入 1:浅色次文字改低对比浅灰(旧病复现:白底 ~1.6:1)
    let light_bad = ColorTokens {
        text_secondary: rgba(0xC8C8C8FF).into(),
        ..ColorTokens::light()
    };
    let bad = contrast_violations("light", &light_bad);
    assert!(
        !bad.is_empty(),
        "TC-TOK-A11Y-02:低对比注入必须红(浅色 text-secondary)"
    );
    assert!(
        bad.iter().all(|m| m.contains("text-secondary")),
        "违例应指认 text-secondary:{bad:?}"
    );

    // 注入 2:深色占位文字改回旧病值(white 28%,凸起面 ~2.4:1)
    let dark_bad = ColorTokens {
        text_placeholder: rgba(0xFFFFFF47).into(),
        ..ColorTokens::dark()
    };
    let bad = contrast_violations("dark", &dark_bad);
    assert!(
        bad.iter()
            .any(|m| m.contains("text-placeholder") && m.contains("surface-2")),
        "违例应指认 text-placeholder on surface-2:{bad:?}"
    );

    // 注入 3:功能色文字侧——浅色 accent 注入旧病值(#4f9fff,白底 2.7:1)
    let accent_bad = ColorTokens {
        accent: rgba(0x4F9FFFFF).into(),
        ..ColorTokens::light()
    };
    let bad = contrast_violations("light", &accent_bad);
    assert!(
        bad.iter().any(|m| m.contains("accent")),
        "功能色文字注入必须红:{bad:?}"
    );
}
