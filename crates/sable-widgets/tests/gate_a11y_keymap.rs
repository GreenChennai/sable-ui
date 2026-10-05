//! TC-A11Y-KEY-01:键位体系门禁(迭代审查报告 2026-10-04 §4.3 A11Y-04)。
//!
//! # 验收口径(报告原文)
//!
//! "Undo/Redo/工具切换/命令面板可用;**键位可从注册表生成速查表**"。
//! 本库按上游 §4.7 纪律只做**绑定层**(宿主注册表为单源,`gpui::App::
//! bind_keys` 属宿主),故"可用"的库侧断言面 = 建议清单四件齐 +
//! 注册表→速查表可生成 + chord 展示单源。
//!
//! # 断言面
//!
//! - `tc_a11y_key_01_registry_produces_cheat_sheet`:注册表(含建议清单
//!   整组 + 自定义动作)→ [`cheat_sheet`] 分节/顺序/文案断言,空注册表
//!   与未定键位的兜底(反面用例,GATE-03 纪律②);
//! - `tc_a11y_key_01_chord_display_single_source`:`chord_display` 与
//!   `controls::tooltip` 的 `render_shortcut`/`format_keystroke` **逐例
//!   同源**(禁第二份格式化实现)——含多段 chord、惯用名化、解析失败兜底。

use sable_widgets::controls::tooltip::{format_keystroke, render_shortcut};
use sable_widgets::keymap::{
    ActionSpec, CATEGORY_EDIT, CATEGORY_TOOLS, CATEGORY_VIEW, KeymapRegistry,
    SUGGESTED_CORE_ACTIONS, cheat_sheet, chord_display,
};

/// TC-A11Y-KEY-01(其一):键位可从注册表生成速查表。
#[test]
fn tc_a11y_key_01_registry_produces_cheat_sheet() {
    // 注册表 = 建议清单整组 + 自定义动作(新分组在后,同分组穿插)
    let registry = KeymapRegistry::new()
        .register_all(SUGGESTED_CORE_ACTIONS.iter().copied())
        .register(ActionSpec::new("paste", "粘贴", "ctrl-v", CATEGORY_EDIT))
        .register(ActionSpec::new(
            "zoom-fit",
            "缩放到适应",
            "shift-1",
            CATEGORY_VIEW,
        ));

    let sheet = cheat_sheet(&registry);
    // 分节顺序 = category 首次出现顺序(编辑 → 工具 → 视图)
    let categories: Vec<_> = sheet.iter().map(|(c, _)| *c).collect();
    assert_eq!(
        categories,
        vec![CATEGORY_EDIT, CATEGORY_TOOLS, CATEGORY_VIEW]
    );
    // 节内顺序 = 注册顺序;文案与 chord(展示形态)逐项断言
    let (edit, tools, view) = (&sheet[0].1, &sheet[1].1, &sheet[2].1);
    assert_eq!(
        edit.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
        vec!["撤销", "重做", "粘贴"],
        "编辑节按注册顺序"
    );
    assert_eq!(edit[0].1, "Ctrl+Z", "undo 键位 = 惯用展示形态");
    assert_eq!(
        edit[1].1, "Ctrl+Shift+Z",
        "redo 修饰键序 = Ctrl 在 Shift 前"
    );
    assert_eq!(edit[2].1, "Ctrl+V");
    // 工具切换:无默认键 → 空 chord 兜底(宿主自行标注"未绑定")
    assert_eq!(tools.len(), 1, "工具节仅循环切换工具一条");
    assert_eq!(tools[0].0, "循环切换工具");
    assert_eq!(tools[0].1, "", "未定键位 = 空 chord");
    // 视图节:命令面板 + 自定义
    assert_eq!(view[0].0, "命令面板");
    assert_eq!(view[0].1, "Ctrl+K");
    assert_eq!(view[1].0, "缩放到适应");
    assert_eq!(view[1].1, "Shift+1");
    // 单字符键大写、命名键惯用名化
    let misc = KeymapRegistry::new()
        .register(ActionSpec::new("tool-brush", "画笔", "b", CATEGORY_TOOLS))
        .register(ActionSpec::new(
            "toggle-ui",
            "显隐界面",
            "tab",
            CATEGORY_VIEW,
        ));
    let sheet = cheat_sheet(&misc);
    assert_eq!(sheet[0].1[0].1, "B", "单字符键大写");
    assert_eq!(sheet[1].1[0].1, "Tab", "命名键查惯用名表");
}

/// TC-A11Y-KEY-01(反面用例):空注册表 → 空速查表,不 panic。
#[test]
fn tc_a11y_key_01_empty_registry_yields_empty_sheet() {
    let registry = KeymapRegistry::new();
    assert!(registry.is_empty());
    assert!(cheat_sheet(&registry).is_empty(), "空注册表速查表为空");
}

/// TC-A11Y-KEY-01(其二):chord 展示单源——`chord_display` 与
/// `tooltip::render_shortcut` 逐例相同,且与 gpui `Keystroke::parse` +
/// `format_keystroke` 管线一致(同一份解析/格式化,无第二实现)。
#[test]
fn tc_a11y_key_01_chord_display_single_source() {
    let cases = [
        "ctrl-z",
        "shift-ctrl-z",
        "ctrl-k",
        "b",
        "tab",
        "shift-1",
        "alt-f4",
        "fn-f5",
        "ctrl-k ctrl-c",           // 多段 chord(空格分隔)
        "win-ctrl-k",              // 平台键
        "mod+e",                   // 人写惯用形:+ 先归一再解析
        "not-a-valid-keystroke+?", // 解析失败 → 原样兜底
    ];
    for spec in cases {
        assert_eq!(
            chord_display(spec),
            render_shortcut(spec),
            "chord_display 必须与 tooltip::render_shortcut 同源:{spec}"
        );
    }
    // 管线一致性:解析成功的 keystroke 与 format_keystroke 输出逐位相同
    assert_eq!(chord_display("ctrl-k"), "Ctrl+K");
    assert_eq!(chord_display("shift-ctrl-z"), "Ctrl+Shift+Z");
    assert_eq!(chord_display("ctrl-k ctrl-c"), "Ctrl+K Ctrl+C");
    assert_eq!(
        chord_display("mod+e"),
        "mod+e",
        "解析失败原样兜底(不丢文案)"
    );
    // 未定键位(空串)= 空展示
    assert_eq!(chord_display(""), String::new());
    // 与 gpui 解析管线直连同源:同一 keystroke,tooltip::format_keystroke
    // 与 chord_display 输出逐位相同(禁第二份格式化实现的决定性断言)
    let parsed = gpui::Keystroke::parse("shift-ctrl-z").expect("keystroke 语法必可解析");
    assert_eq!(
        format_keystroke(&parsed),
        chord_display("shift-ctrl-z"),
        "chord 展示与 tooltip/gpui 管线同源"
    );
}
