//! 键位注册绑定层(迭代审查报告 2026-10-04 §4.3 A11Y-04,TC-A11Y-KEY-01)。
//!
//! # 契约:键位以宿主注册表为单源,库只做绑定层
//!
//! 上游纪律(docs/upstream/00 §4.7 / 01 §7.5 / 03 §集成键位):CutForge
//! 45 条可重绑定快捷键注册表等**宿主注册表是命令 ID 与键位的唯一真相**,
//! Sable keymap 只做绑定层——本模块因此**不含任何全局监听/绑定注册**,
//! 只提供三件绑定层元数据:
//!
//! 1. [`ActionSpec`] / [`KeymapRegistry`]:动作规格与注册表(宿主把动作
//!    登记进来,持久化/命令面板/菜单可从同一注册表取);
//! 2. [`chord_display`] / [`cheat_sheet`]:键位展示与速查表生成——展示
//!    **单源复用** [`crate::controls::tooltip::render_shortcut`] /
//!    `format_keystroke`(禁第二份实现,TC-A11Y-KEY-01 断言同源);
//! 3. [`SUGGESTED_CORE_ACTIONS`]:常用动作**建议清单**(Undo/Redo/工具
//!    切换/命令面板),宿主可选用、可覆盖——键位是建议值不是注册值。
//!
//! # gpui 0.2.2 实况(registry 源码核实,2026-10)
//!
//! - `gpui::actions!(namespace, [Undo, Redo, …])`:宿主声明动作(unit
//!   struct + `Action` derive,TypeId 分发);
//! - `gpui::KeyBinding::new(keystrokes, action, context)`:构造绑定;
//!   keystroke 语法 = 空格分隔多段 chord、`-` 分隔修饰键(`ctrl`/`alt`/
//!   `shift`/`fn`/`cmd`|`super`|`win`/`secondary`,如 `"ctrl-k"`、
//!   `"shift-ctrl-z"`);解析失败 `KeyBinding::new` 会 panic,宿主注册前
//!   可先用本模块 [`chord_display`] 试解析(失败原样返回不 panic);
//! - `App::bind_keys(bindings)`:把绑定装进 App 级 `gpui::Keymap`——
//!   **注册属宿主**,这正是"库不做全局监听"的平台依据;
//! - 元素侧 `.on_action(cx.listener(...))` 消费动作。
//!
//! 典型宿主接线:`actions!` 声明动作 → `ActionSpec` 登记注册表(命令
//! 面板/速查表共用)→ `KeyBinding::new(spec.default_keystroke, …)` +
//! `cx.bind_keys` 落键位。

use crate::controls::tooltip::render_shortcut;

// ---------------------------------------------------------------------------
// 动作规格与注册表
// ---------------------------------------------------------------------------

/// 动作规格(绑定层元数据;全 `&'static str` 使建议清单可用 `const` 声明
/// ——命令 ID 与键位的真相在宿主注册表,本结构只是登记载体)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ActionSpec {
    /// 动作稳定 id(命令面板/持久化键;建议与宿主 `actions!` 动作名对应)
    pub id: &'static str,
    /// 展示名(tooltip/菜单/命令面板/速查表共用)
    pub label: &'static str,
    /// 默认键位(gpui keystroke 语法;空串 = 未定默认,宿主注册时定夺)
    pub default_keystroke: &'static str,
    /// 分组(速查表分节,如"编辑"/"工具"/"视图")
    pub category: &'static str,
}

impl ActionSpec {
    /// const 构造(宿主/建议清单的常量声明入口)。
    #[must_use]
    pub const fn new(
        id: &'static str,
        label: &'static str,
        default_keystroke: &'static str,
        category: &'static str,
    ) -> Self {
        ActionSpec {
            id,
            label,
            default_keystroke,
            category,
        }
    }
}

/// 动作注册表(宿主把动作登记进来;命令面板/菜单/速查表从同一注册表取
/// ——"注册表为单源"的库侧落点)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeymapRegistry {
    actions: Vec<ActionSpec>,
}

impl KeymapRegistry {
    /// 空注册表。
    #[must_use]
    pub const fn new() -> Self {
        KeymapRegistry {
            actions: Vec::new(),
        }
    }

    /// 登记一个动作(builder 风格;重复 id 不去重,`find` 取先登记者)。
    #[must_use]
    pub fn register(mut self, action: ActionSpec) -> Self {
        self.actions.push(action);
        self
    }

    /// 批量登记(如 [`SUGGESTED_CORE_ACTIONS`] 整组搬入)。
    #[must_use]
    pub fn register_all(mut self, actions: impl IntoIterator<Item = ActionSpec>) -> Self {
        self.actions.extend(actions);
        self
    }

    /// 已登记动作(注册顺序)。
    #[must_use]
    pub fn actions(&self) -> &[ActionSpec] {
        &self.actions
    }

    /// 按 id 查找(取先登记者;未登记 = `None`)。
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&ActionSpec> {
        self.actions.iter().find(|a| a.id == id)
    }

    /// 已登记动作数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 键位展示与速查表(chord 展示单源 = tooltip::render_shortcut 转发)
// ---------------------------------------------------------------------------

/// 键位展示(渲染 "Ctrl+K" 惯用形态):**唯一实现 = 转发**
/// [`render_shortcut`](crate::controls::tooltip::render_shortcut)(其内部
/// 经 gpui `Keystroke::parse` + `format_keystroke` 惯用名化)——本函数
/// 只是把单点暴露成 keymap 语义入口,**禁第二份格式化实现**(TC-A11Y-KEY-01
/// 断言与 tooltip 同源)。多段 chord 以空格连接;解析失败的串原样兜底;
/// 空串(未定默认键)返回空串,宿主自行渲染"未绑定"。
#[must_use]
pub fn chord_display(keystroke: &str) -> String {
    render_shortcut(keystroke)
}

/// 速查表(TC-A11Y-KEY-01:"键位可从注册表生成速查表"):按 category 分节
/// (首次出现顺序),节内按注册顺序;chord 为 [`chord_display`] 展示形态。
/// 空注册表 → 空速查表。
#[must_use]
pub fn cheat_sheet(registry: &KeymapRegistry) -> Vec<(&'static str, Vec<(&'static str, String)>)> {
    let mut sections: Vec<(&'static str, Vec<(&'static str, String)>)> = Vec::new();
    for action in registry.actions() {
        let entry = (action.label, chord_display(action.default_keystroke));
        match sections
            .iter_mut()
            .find(|(category, _)| *category == action.category)
        {
            Some((_, entries)) => entries.push(entry),
            None => sections.push((action.category, vec![entry])),
        }
    }
    sections
}

// ---------------------------------------------------------------------------
// 常用动作建议清单(宿主可选用/覆盖;键位是建议值不是注册值)
// ---------------------------------------------------------------------------

/// 动作分组:编辑。
pub const CATEGORY_EDIT: &str = "编辑";
/// 动作分组:工具。
pub const CATEGORY_TOOLS: &str = "工具";
/// 动作分组:视图。
pub const CATEGORY_VIEW: &str = "视图";

/// 建议:撤销(gpui keystroke 语法 `"ctrl-z"`;宿主可覆盖)。
pub const ACTION_UNDO: ActionSpec = ActionSpec::new("undo", "撤销", "ctrl-z", CATEGORY_EDIT);
/// 建议:重做(`"shift-ctrl-z"`;宿主可另绑 `"ctrl-y"`)。
pub const ACTION_REDO: ActionSpec = ActionSpec::new("redo", "重做", "shift-ctrl-z", CATEGORY_EDIT);
/// 建议:循环切换工具。**无默认键**——工具集与顺序属宿主(画布工具注册表
/// 为单源),空串键位在速查表渲染为空,宿主自行标注"未绑定"。
pub const ACTION_CYCLE_TOOLS: ActionSpec =
    ActionSpec::new("cycle-tools", "循环切换工具", "", CATEGORY_TOOLS);
/// 建议:命令面板(报告 CMP-03 规格键位 `Mod+K`;Windows/非 macOS 落
/// `"ctrl-k"`)。
pub const ACTION_COMMAND_PALETTE: ActionSpec =
    ActionSpec::new("command-palette", "命令面板", "ctrl-k", CATEGORY_VIEW);

/// 常用动作建议清单(A11Y-04:Undo/Redo/工具切换/命令面板;宿主
/// `register_all` 整组搬入或按 id 挑选)。
pub const SUGGESTED_CORE_ACTIONS: &[ActionSpec] = &[
    ACTION_UNDO,
    ACTION_REDO,
    ACTION_CYCLE_TOOLS,
    ACTION_COMMAND_PALETTE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_register_and_find_keeps_insertion_order() {
        let registry = KeymapRegistry::new()
            .register(ACTION_UNDO)
            .register(ACTION_REDO)
            .register(ActionSpec::new("cut", "剪切", "ctrl-x", CATEGORY_EDIT));
        assert_eq!(registry.len(), 3);
        assert!(!registry.is_empty());
        // 注册顺序保持(速查表节内顺序的依据)
        let ids: Vec<_> = registry.actions().iter().map(|a| a.id).collect();
        assert_eq!(ids, vec!["undo", "redo", "cut"]);
        // find:先登记者胜;未登记 None
        assert_eq!(registry.find("redo"), Some(&ACTION_REDO));
        let dup = registry.register(ACTION_UNDO);
        assert_eq!(dup.find("undo"), Some(&ACTION_UNDO));
        assert!(KeymapRegistry::new().find("undo").is_none());
        // 空注册表
        assert!(KeymapRegistry::new().is_empty());
    }

    #[test]
    fn suggested_core_actions_cover_a11y_04_minimum_set() {
        // TC-A11Y-KEY-01 的"Undo/Redo/工具切换/命令面板可用":建议清单四件齐
        assert_eq!(SUGGESTED_CORE_ACTIONS.len(), 4);
        for id in ["undo", "redo", "cycle-tools", "command-palette"] {
            assert!(
                SUGGESTED_CORE_ACTIONS.iter().any(|a| a.id == id),
                "建议清单缺 {id}"
            );
        }
        // 全部字段非空(工具循环的键位除外——宿主定夺的显式空串)
        for action in SUGGESTED_CORE_ACTIONS {
            assert!(!action.label.is_empty());
            assert!(!action.category.is_empty());
        }
        assert!(ACTION_UNDO.default_keystroke.starts_with("ctrl-"));
        assert_eq!(ACTION_CYCLE_TOOLS.default_keystroke, "", "工具键位留给宿主");
    }
}
