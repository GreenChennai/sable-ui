//! IME 输入法适配层(迭代审查报告 2026-10-04 §4.3 A11Y-08,第 4 组收口)。
//!
//! # 解决的问题(报告原话:"封装 `InputMethodAdapter` 隔离 IME 调用点")
//!
//! gpui 0.2.2 的平台文本输入 = [`gpui::EntityInputHandler`] 全 8 方法,此前
//! TextField 与 NumberField **各自**实现一套:UTF-16 ↔ 字节换算、组合区间
//! 分支、平台替换路径在两个文件里各写一份(且 NumberField 的组合文本直插、
//! 不追踪 marked 区间,与 TextField 的组合一等状态不同源)。本模块把
//! **IME 侧事件**与**控件侧回调**的接线收敛为单点:
//!
//! ```text
//! 平台(gpui EntityInputHandler) ──四事件──▶ InputMethodAdapter(本模块)
//!                                               │ UTF-16 ↔ 字节换算 + 路由
//!                                               ▼
//!                                   ImeEditTarget(控件侧回调面)
//!                                    ├─ TextField        (组合区间一等状态)
//!                                    └─ NumberField 编辑态(直插,见"边界")
//! ```
//!
//! 控件只实现回调面,平台只发事件;换算与分支只此一份(含 UTF-16 下标
//! 换算 [`utf16_to_byte`]/[`byte_to_utf16`],text_field/number_field 的旧
//! 私有副本已删,单源纪律 COUP-R5)。
//!
//! # 事件模型(组合生命周期四事件 + 解除标记)
//!
//! gpui 0.2.2 实况(registry 源码核实):组合开始与更新共用
//! `replace_and_mark_text_in_range` 一个入口(Windows 恒传全量组合串、
//! 无删除区间;部分平台先删区间再组合),最终提交走 `replace_text_in_range`,
//! 组合态查询走 `marked_text_range`,解除标记走 `unmark_text`。据此:
//!
//! - [`ImeEvent::CompositionBegin`] / [`ImeEvent::CompositionUpdate`]:
//!   事件分立(生命周期可显式表达、测试可逐段断言),默认实现将两者路由到
//!   同一回调 [`ImeEditTarget::ime_composition_update`](缓冲按既有组合区间
//!   自行区分"插入点开始组合"与"原位替换组合"——与平台同入口的实况一致);
//! - [`ImeEvent::Commit`]:提交(组合最终文本,`GCS_RESULTSTR` 路径)或带
//!   区间的直接替换;
//! - [`ImeEvent::CancelComposition`]:取消组合(组合文本移除);
//! - [`ImeEvent::Unmark`]:解除组合标记(组合文本保留为普通文本)。
//!
//! # 门控契约(逐位对齐 TextField 现行为)
//!
//! - **Update/Commit 受 [`ImeEditTarget::ime_editable`] 门控**:禁用/只读
//!   时事件拒收(`route` 返回 `false`,组件据此跳过重绘)——TextField 的
//!   `disabled || read_only` 早退语义由此单点表达;
//! - **Cancel / Unmark 不受门控**:只读态仍可取消组合、平台解除标记不问
//!   可编辑(与 TextField 收敛前行为一致);禁用门控在组件键盘入口先行。
//!
//! # 边界(如实,不许虚标)
//!
//! - 候选窗字符级定位 = M2(需字形测量);TextField/NumberField 的
//!   `bounds_for_range` 均返回控件 bounds,`character_index_for_point`
//!   TextField 真实字形命中、NumberField 回报光标(近似);
//! - **NumberField 编辑器无选区/无组合区间追踪**:组合文本直插 buffer、
//!   `ime_composition_range()` 恒 `None`——组合取消/组合态高亮在
//!   NumberField 上不可用,完整组合语义随 NumberField 编辑器重构(M2);
//! - 中文输入法**真机走查**(DirectWrite/IMM32 全链路、候选窗、组合光标
//! 相位)= TC-A11Y-IME-01,走查表入库 `docs/a11y-notes.md §5`,本批交付
//! 为数据通路与单点收敛,全部条目 ☐ 待真机,不得预标 ✅。

use std::ops::Range;

// ---------------------------------------------------------------------------
// UTF-16 ↔ 字节下标换算(IME 协议口径;全 crate 单点,旧私有副本已删)
// ---------------------------------------------------------------------------

/// UTF-16 下标 → char 边界字节下标(纯函数):越界返回 `None`;落在多单元
/// 字符(代理对)中间时吸附到该字符**后边界**(宁多勿撕)。
#[must_use]
pub fn utf16_to_byte(text: &str, utf16_index: usize) -> Option<usize> {
    let mut utf16 = 0usize;
    for (byte, ch) in text.char_indices() {
        if utf16 >= utf16_index {
            return Some(byte);
        }
        utf16 += ch.len_utf16();
        if utf16 > utf16_index {
            return Some(byte + ch.len_utf8());
        }
    }
    (utf16 == utf16_index).then_some(text.len())
}

/// char 边界字节下标 → UTF-16 下标(纯函数):越界钳到末尾。
#[must_use]
pub fn byte_to_utf16(text: &str, byte_index: usize) -> usize {
    text[..byte_index.min(text.len())]
        .chars()
        .map(char::len_utf16)
        .sum()
}

// ---------------------------------------------------------------------------
// IME 事件(平台 → 控件;组合生命周期四事件 + 解除标记)
// ---------------------------------------------------------------------------

/// 平台 IME 事件(`'a` = 事件携带的文本借用;纯数据,无 App 依赖)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeEvent<'a> {
    /// 组合**开始**(插入点起组合;`delete_range_utf16` = 平台要求先删除的
    /// 区间(UTF-16,`None` = 不删);`caret_in_new_utf16` = 平台给的组合内
    /// 光标偏移(UTF-16,`None` = 落组合文本末尾))。
    CompositionBegin {
        /// 先删除的区间(UTF-16;`None` = 不删,插入点直接起组合)
        delete_range_utf16: Option<Range<usize>>,
        /// 组合文本(平台全量组合串)
        new_text: &'a str,
        /// 组合内光标(UTF-16;`None` = 组合文本末尾)
        caret_in_new_utf16: Option<usize>,
    },
    /// 组合**更新**(既有组合原位替换;字段语义同 [`ImeEvent::CompositionBegin`]
    /// ——Windows 组合更新恒 `delete_range_utf16 = None`、全量组合串)。
    CompositionUpdate {
        /// 先删除的区间(UTF-16;`None` = 原位替换既有组合)
        delete_range_utf16: Option<Range<usize>>,
        /// 组合文本(平台全量组合串)
        new_text: &'a str,
        /// 组合内光标(UTF-16;`None` = 组合文本末尾)
        caret_in_new_utf16: Option<usize>,
    },
    /// **提交/替换**:`range_utf16 = None` 时为组合最终文本提交(`None` 区间 +
    /// 组合中 = 替换组合;`None` 区间 + 非组合 = 光标处插入);`Some(range)`
    /// 为带区间的直接替换(替换区覆盖组合区时组合结束)。
    Commit {
        /// 替换区间(UTF-16;`None` = 组合提交/光标处插入)
        range_utf16: Option<Range<usize>>,
        /// 最终文本
        text: &'a str,
    },
    /// **取消组合**(组合文本移除,光标回落组合起点;不受可编辑门控)。
    CancelComposition,
    /// **解除组合标记**(组合文本保留为普通文本;平台 `unmark_text`;
    /// 不受可编辑门控)。
    Unmark,
}

// ---------------------------------------------------------------------------
// 控件侧回调面(TextField / NumberField 编辑态各自实现)
// ---------------------------------------------------------------------------

/// 控件侧回调面(IME 组合态查询 + 文本变更):Adapter 把平台事件翻译成
/// 本 trait 的调用;下标口径除注明 UTF-16 外均为 **char 边界字节偏移**
/// (控件缓冲的不变量,与平台 UTF-16 协议的换算收敛在 Adapter 单点)。
pub trait ImeEditTarget {
    /// 当前缓冲文本(`None` = 无编辑态,如 NumberField 展示态——查询类
    /// 事件据此返回 `None`,与"平台未进编辑态"语义一致)。
    fn ime_text(&self) -> Option<&str>;

    /// 是否可编辑(禁用/只读 = `false`):Update/Commit 事件的门控单点
    /// (Cancel/Unmark 不查,见模块 doc"门控契约")。
    fn ime_editable(&self) -> bool;

    /// 光标(字节偏移;`None` = 无编辑态)。
    fn ime_caret(&self) -> Option<usize>;

    /// 当前选区(字节偏移,start ≤ end;`None` = 无选区 → 查询方向回落
    /// 光标空选)。
    fn ime_selection_range(&self) -> Option<Range<usize>>;

    /// IME 组合(marked)区间(字节偏移;`None` = 非组合中/不追踪)。
    fn ime_composition_range(&self) -> Option<Range<usize>>;

    /// 光标处插入(有选区先替换选区——由控件缓冲语义保证)。
    fn ime_insert(&mut self, text: &str);

    /// 区间替换(字节偏移;控件缓冲负责边界防御)。
    fn ime_replace_range(&mut self, range: Range<usize>, text: &str);

    /// 组合开始/更新(平台全量组合串 + 组合内光标 UTF-16 偏移;缓冲按
    /// 既有组合区间区分插入点起组合与原位替换)。
    fn ime_composition_update(&mut self, new_text: &str, caret_in_new_utf16: Option<usize>);

    /// 组合提交(最终文本替换组合区间)。
    fn ime_composition_commit(&mut self, final_text: &str);

    /// 取消组合。返回是否发生了取消(无组合 = `false`)。
    fn ime_composition_cancel(&mut self) -> bool;

    /// 解除组合标记。返回是否清除了标记(无标记 = `false`)。
    fn ime_unmark(&mut self) -> bool;
}

// ---------------------------------------------------------------------------
// InputMethodAdapter(单点路由;默认实现 = UTF-16 平台协议版)
// ---------------------------------------------------------------------------

/// IME 适配器:平台事件 → 控件回调的**单点路由器**。默认实现把全部
/// UTF-16 ↔ 字节换算、区间归一、组合覆盖判定收敛于此;宿主可换实现
/// (如接入系统原生输入的降级路径),控件回调面不变。
pub trait InputMethodAdapter {
    /// 路由一个事件。返回是否产生了状态变更(`false` = 被门控拒收;
    /// 组件据此决定重绘/相位重置——Unmark/无组合取消等"未变更"路径
    /// 组件可自行忽略)。
    fn route(&self, target: &mut dyn ImeEditTarget, event: ImeEvent) -> bool {
        match event {
            ImeEvent::CompositionBegin {
                delete_range_utf16,
                new_text,
                caret_in_new_utf16,
            }
            | ImeEvent::CompositionUpdate {
                delete_range_utf16,
                new_text,
                caret_in_new_utf16,
            } => {
                if !target.ime_editable() {
                    return false; // 禁用/只读:不进组合态
                }
                if let Some(range) = delete_range_utf16 {
                    // 快照先行:换算基准 = 删除前的缓冲文本与光标
                    let snapshot = target.ime_text().unwrap_or_default().to_string();
                    let caret = target.ime_caret().unwrap_or(0);
                    let (start, end) = utf16_byte_pair(&snapshot, range, caret);
                    target.ime_replace_range(start..end, "");
                }
                target.ime_composition_update(new_text, caret_in_new_utf16);
                true
            }
            ImeEvent::Commit { range_utf16, text } => {
                if !target.ime_editable() {
                    return false; // 禁用/只读:平台替换路径拒收
                }
                let Some(range) = range_utf16 else {
                    // None 区间:组合中 = 提交(GCS_RESULTSTR);非组合 = 插入
                    if target.ime_composition_range().is_some() {
                        target.ime_composition_commit(text);
                    } else {
                        target.ime_insert(text);
                    }
                    return true;
                };
                let snapshot = target.ime_text().unwrap_or_default().to_string();
                let caret = target.ime_caret().unwrap_or(0);
                let (start, end) = utf16_byte_pair(&snapshot, range, caret);
                target.ime_replace_range(start..end, text);
                // 替换区覆盖组合区(平台最终提交形态)→ 组合结束
                if let Some(comp) = target.ime_composition_range() {
                    let (s, e) = (start.min(end), start.max(end));
                    if s <= comp.end && e >= comp.start {
                        target.ime_unmark();
                    }
                }
                true
            }
            ImeEvent::CancelComposition => target.ime_composition_cancel(),
            ImeEvent::Unmark => target.ime_unmark(),
        }
    }

    /// 组合(marked)区间(UTF-16 口径;非组合/不追踪 = `None`)。
    fn marked_range_utf16(&self, target: &dyn ImeEditTarget) -> Option<Range<usize>> {
        let range = target.ime_composition_range()?;
        let text = target.ime_text()?;
        Some(byte_to_utf16(text, range.start)..byte_to_utf16(text, range.end))
    }

    /// 区间取文(UTF-16 入;返回文本 + 按 trait 契约回调的**调整后
    /// UTF-16 区间**)。无编辑态/区间越界 = `None`。
    fn text_for_range_utf16(
        &self,
        target: &dyn ImeEditTarget,
        range_utf16: Range<usize>,
    ) -> Option<(String, Range<usize>)> {
        let text = target.ime_text()?;
        let start = utf16_to_byte(text, range_utf16.start)?;
        let end = utf16_to_byte(text, range_utf16.end)?;
        let adjusted = byte_to_utf16(text, start)..byte_to_utf16(text, end);
        Some((text[start..end].to_string(), adjusted))
    }

    /// 选区(UTF-16 口径 + reversed;无编辑态 = `None`;无选区 = 光标处
    /// 空选,reversed = `false`)。
    fn selection_utf16(&self, target: &dyn ImeEditTarget) -> Option<(Range<usize>, bool)> {
        let text = target.ime_text()?;
        match target.ime_selection_range() {
            Some(sel) => {
                let reversed = target.ime_caret().is_some_and(|caret| caret < sel.start);
                Some((
                    byte_to_utf16(text, sel.start)..byte_to_utf16(text, sel.end),
                    reversed,
                ))
            }
            None => {
                let caret = target.ime_caret().map_or(0, |c| byte_to_utf16(text, c));
                Some((caret..caret, false))
            }
        }
    }
}

/// UTF-16 区间 → 字节区间(换算失败回落 `fallback` 光标;start/end 归一为
/// start ≤ end——`EditBuffer::replace_range` 对逆序区间是无操作,归一保证
/// 两类缓冲语义一致)。
fn utf16_byte_pair(text: &str, range: Range<usize>, fallback: usize) -> (usize, usize) {
    let start = utf16_to_byte(text, range.start).unwrap_or(fallback);
    let end = utf16_to_byte(text, range.end).unwrap_or(fallback);
    (start.min(end), start.max(end))
}

/// 默认实现(无状态单例;零字段零开销)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Utf16InputMethodAdapter;

impl InputMethodAdapter for Utf16InputMethodAdapter {}

/// 默认适配器实例(控件直接用;零尺寸 const,无运行时构造)。
pub const UTF16_ADAPTER: Utf16InputMethodAdapter = Utf16InputMethodAdapter;

// ---------------------------------------------------------------------------
// 测试:Adapter 四事件 → 控件状态迁移(纯函数;TC-A11Y-IME-01 的可机检面
// ——真机走查见 docs/a11y-notes.md §5,此处断言路由与门控语义)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用回调面:真实状态 + 调用流水(断言"哪个回调被调、哪个没被调"
    /// ——组合态不误提交等语义的断言面)。
    #[derive(Default)]
    struct Target {
        text: String,
        caret: usize,
        composition: Option<Range<usize>>,
        editable: bool,
        calls: Vec<&'static str>,
    }

    impl Target {
        fn editable(text: &str) -> Self {
            Target {
                text: text.to_string(),
                caret: text.len(),
                editable: true,
                ..Target::default()
            }
        }

        fn locked(text: &str) -> Self {
            Target {
                text: text.to_string(),
                caret: text.len(),
                editable: false,
                ..Target::default()
            }
        }
    }

    impl ImeEditTarget for Target {
        fn ime_text(&self) -> Option<&str> {
            Some(&self.text)
        }

        fn ime_editable(&self) -> bool {
            self.editable
        }

        fn ime_caret(&self) -> Option<usize> {
            Some(self.caret)
        }

        fn ime_selection_range(&self) -> Option<Range<usize>> {
            None
        }

        fn ime_composition_range(&self) -> Option<Range<usize>> {
            self.composition.clone()
        }

        fn ime_insert(&mut self, text: &str) {
            self.calls.push("insert");
            self.text.insert_str(self.caret.min(self.text.len()), text);
            self.caret += text.len();
        }

        fn ime_replace_range(&mut self, range: Range<usize>, text: &str) {
            self.calls.push("replace_range");
            let (start, end) = (
                range.start.min(self.text.len()),
                range.end.min(self.text.len()),
            );
            let (start, end) = (start.min(end), start.max(end));
            self.text.replace_range(start..end, text);
            self.caret = start + text.len();
        }

        fn ime_composition_update(&mut self, new_text: &str, caret_in_new_utf16: Option<usize>) {
            self.calls.push("composition_update");
            // 与 TextFieldBuffer 同款:既有组合原位替换,否则插入点起组合
            let start = match self.composition.take() {
                Some(range) => {
                    self.text.replace_range(range.clone(), "");
                    self.caret = range.start;
                    range.start
                }
                None => self.caret,
            };
            self.text.insert_str(start, new_text);
            let caret_in_new = caret_in_new_utf16.unwrap_or(new_text.len());
            self.caret = start + caret_in_new;
            self.composition = Some(start..start + new_text.len());
        }

        fn ime_composition_commit(&mut self, final_text: &str) {
            self.calls.push("composition_commit");
            if let Some(range) = self.composition.take() {
                self.text.replace_range(range.start..range.end, final_text);
                self.caret = range.start + final_text.len();
            }
        }

        fn ime_composition_cancel(&mut self) -> bool {
            self.calls.push("composition_cancel");
            match self.composition.take() {
                Some(range) => {
                    self.text.replace_range(range.start..range.end, "");
                    self.caret = range.start;
                    true
                }
                None => false,
            }
        }

        fn ime_unmark(&mut self) -> bool {
            self.calls.push("unmark");
            self.composition.take().is_some()
        }
    }

    // —— 四事件:开始 → 更新 → 提交 → 取消(组合生命周期逐段断言)——

    #[test]
    fn tc_a11y_ime_01_adapter_lifecycle_begin_update_commit() {
        let mut t = Target::editable("ab");
        t.caret = 1;
        // 开始:插入点起组合
        assert!(UTF16_ADAPTER.route(
            &mut t,
            ImeEvent::CompositionBegin {
                delete_range_utf16: None,
                new_text: "ni",
                caret_in_new_utf16: Some(2),
            }
        ));
        assert_eq!(t.text, "anib");
        assert_eq!(t.composition, Some(1..3));
        assert_eq!(t.caret, 3, "组合内光标取平台偏移");
        // 更新:原位替换(全量组合串)
        assert!(UTF16_ADAPTER.route(
            &mut t,
            ImeEvent::CompositionUpdate {
                delete_range_utf16: None,
                new_text: "nihao",
                caret_in_new_utf16: None,
            }
        ));
        assert_eq!(t.text, "anihaob", "更新 = 原位替换组合串");
        assert_eq!(t.composition, Some(1..6));
        // 提交:最终文本替换组合,组合区间清空
        assert!(UTF16_ADAPTER.route(
            &mut t,
            ImeEvent::Commit {
                range_utf16: None,
                text: "你好",
            }
        ));
        assert_eq!(t.text, "a你好b");
        assert_eq!(t.composition, None, "提交后组合区间清空");
        assert_eq!(t.caret, "a你好".len(), "光标落最终文本之后");
        // 调用流水:两次 composition_update + 一次 composition_commit,
        // **零 insert**(组合态提交不得走插入路径 = "不误提交"的接线断言)
        assert_eq!(
            t.calls,
            vec![
                "composition_update",
                "composition_update",
                "composition_commit"
            ]
        );
    }

    #[test]
    fn tc_a11y_ime_01_adapter_cancel_and_unmark_transitions() {
        // 取消:组合文本移除,光标回落组合起点
        let mut t = Target::editable("ab");
        t.caret = 1;
        UTF16_ADAPTER.route(
            &mut t,
            ImeEvent::CompositionBegin {
                delete_range_utf16: None,
                new_text: "shi",
                caret_in_new_utf16: None,
            },
        );
        assert!(UTF16_ADAPTER.route(&mut t, ImeEvent::CancelComposition));
        assert_eq!(t.text, "ab", "组合文本移除");
        assert_eq!(t.composition, None);
        assert_eq!(t.caret, 1, "光标回落组合起点");
        // 无组合时取消:返回 false,状态不动
        assert!(!UTF16_ADAPTER.route(&mut t, ImeEvent::CancelComposition));
        assert_eq!(t.text, "ab");
        // 解除标记:文本保留为普通文本
        UTF16_ADAPTER.route(
            &mut t,
            ImeEvent::CompositionBegin {
                delete_range_utf16: None,
                new_text: "de",
                caret_in_new_utf16: None,
            },
        );
        assert!(UTF16_ADAPTER.route(&mut t, ImeEvent::Unmark));
        assert_eq!(t.text, "adeb", "unmark 文本保留");
        assert_eq!(t.composition, None, "标记清除");
        // 无标记可解除:false
        assert!(!UTF16_ADAPTER.route(&mut t, ImeEvent::Unmark));
    }

    // —— 门控契约:Update/Commit 拒收、Cancel/Unmark 不门控 ——

    #[test]
    fn tc_a11y_ime_01_adapter_gating_matches_contract() {
        // 禁用/只读:组合与提交拒收(route = false,零回调)
        let mut t = Target::locked("v1");
        let events = [
            ImeEvent::CompositionBegin {
                delete_range_utf16: None,
                new_text: "x",
                caret_in_new_utf16: None,
            },
            ImeEvent::CompositionUpdate {
                delete_range_utf16: None,
                new_text: "x",
                caret_in_new_utf16: None,
            },
            ImeEvent::Commit {
                range_utf16: None,
                text: "x",
            },
            ImeEvent::Commit {
                range_utf16: Some(0..1),
                text: "x",
            },
        ];
        for event in events {
            let what = format!("{event:?}");
            let applied = UTF16_ADAPTER.route(&mut t, event);
            assert!(!applied, "{what} 应被门控拒收");
        }
        assert!(t.calls.is_empty(), "被拒收事件不得触达控件回调");
        assert_eq!(t.text, "v1");
        // Cancel/Unmark 不受门控(契约:只读态可取消组合、平台解除标记放行)
        assert!(!UTF16_ADAPTER.route(&mut t, ImeEvent::CancelComposition));
        assert!(!UTF16_ADAPTER.route(&mut t, ImeEvent::Unmark));
    }

    // —— Commit 分支:带区间替换 / 替换区覆盖组合区 → 组合结束 ——

    #[test]
    fn tc_a11y_ime_01_adapter_commit_with_range_and_overlap_unmark() {
        // 带区间替换(非组合)= 直接替换路径
        let mut t = Target::editable("abc");
        assert!(UTF16_ADAPTER.route(
            &mut t,
            ImeEvent::Commit {
                range_utf16: Some(0..2),
                text: "X",
            }
        ));
        assert_eq!(t.text, "Xc");
        assert_eq!(t.calls, vec!["replace_range"], "非组合带区间 = 纯替换");
        // 替换区覆盖组合区 → unmark(平台最终提交形态)
        let mut c = Target::editable("ab");
        c.caret = 2;
        UTF16_ADAPTER.route(
            &mut c,
            ImeEvent::CompositionBegin {
                delete_range_utf16: None,
                new_text: "ni",
                caret_in_new_utf16: None,
            },
        );
        assert!(c.composition.is_some());
        assert!(UTF16_ADAPTER.route(
            &mut c,
            ImeEvent::Commit {
                range_utf16: Some(2..4),
                text: "你好",
            }
        ));
        assert_eq!(c.text, "ab你好");
        assert_eq!(c.composition, None, "替换区覆盖组合区 → 组合结束");
        // UTF-16 口径:区间按 UTF-16 换算(每 CJK 字 1 单位 = 3 字节)
        let mut u = Target::editable("12厘米");
        assert!(UTF16_ADAPTER.route(
            &mut u,
            ImeEvent::Commit {
                range_utf16: Some(2..4),
                text: "cm",
            }
        ));
        assert_eq!(u.text, "12cm", "UTF-16 区间 2..4 = 厘米(整段替换)");
    }

    // —— 查询方向:marked / text_for_range / selection(UTF-16 协议口径)——

    #[test]
    fn tc_a11y_ime_01_adapter_queries_report_utf16_protocol() {
        let mut t = Target::editable("a你b");
        t.caret = "a你".len();
        // marked:字节组合区间 → UTF-16 回报(你 = 1 单位/3 字节)
        t.composition = Some("a".len().."a你".len());
        assert_eq!(
            UTF16_ADAPTER.marked_range_utf16(&t),
            Some(1..2),
            "UTF-16 口径:你 = 1 单位(3 字节)"
        );
        // text_for_range:UTF-16 入,调整后区间回 UTF-16
        let (text, adjusted) = UTF16_ADAPTER
            .text_for_range_utf16(&t, 1..2)
            .expect("合法区间必有文本");
        assert_eq!(text, "你");
        assert_eq!(adjusted, 1..2);
        assert!(
            UTF16_ADAPTER.text_for_range_utf16(&t, 0..99).is_none(),
            "越界 None"
        );
        // selection:无选区 → 光标空选(UTF-16 口径,reversed = false)
        assert_eq!(UTF16_ADAPTER.selection_utf16(&t), Some((2..2, false)));
    }

    // —— 换算单点:旧 text_field/number_field 私有副本已删,行为不变 ——

    #[test]
    fn utf16_byte_conversion_single_point_round_trip() {
        let text = "12厘米e";
        assert_eq!(utf16_to_byte(text, 2), Some(2));
        assert_eq!(byte_to_utf16(text, 2), 2);
        assert_eq!(utf16_to_byte(text, 5), Some(text.len()));
        assert_eq!(utf16_to_byte(text, 6), None);
        assert_eq!(byte_to_utf16(text, text.len()), 5, "越界钳末尾");
        // 代理对内部吸附后边界
        let astral = "1🎉";
        assert_eq!(utf16_to_byte(astral, 2), Some(astral.len()));
    }
}
