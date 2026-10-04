//! 文本输入框 TextField(迭代审查报告 2026-10-04 §5.6 组件矩阵 #3,CMP-01 批 1)。
//!
//! # 形态(Entity + builder,NumberField/Button 同款)
//!
//! [`TextField`] 是**实时**单行编辑控件(区别于 NumberField 的"展示/编辑"
//! 双态):文本常驻 [`TextFieldBuffer`],Enter 经 [`TextField::on_submit`]
//! 上报宿主(`fn(&str, &mut Window, &mut App)`),Esc 还原到最近提交快照并
//! 经 [`TextField::on_cancel`] 上报。宿主经 [`TextField::set_text`]/
//! [`TextField::text`] 读写(受控友好:回调里落自身状态后回写即闭环)。
//!
//! ```ignore
//! cx.new(|_| {
//!     TextField::new("name", "默认名")
//!         .placeholder("输入图层名…")
//!         .on_submit(|text, _win, _cx| { /* … */ })
//!         .on_cancel(|_win, _cx| { /* … */ })
//! })
//! ```
//!
//! # 编辑核心 = 纯函数状态机(TC-CMP-TF-01 的断言面)
//!
//! - [`TextFieldBuffer`]:文本 + 光标(char 边界字节偏移)+ 选区锚点 +
//!   **IME 组合区间**四元组;插入/删除/选区归一/光标移动全部为独立纯函数
//!   操作,无 App 依赖可单测(NumberField 的 [`crate::number_field::
//!   EditBuffer`] 选区扩展版);
//! - [`edit_key`]:键盘 → [`EditKey`] 意图映射(唯一键盘状态机单点);
//!   **组合态(composing)下 Enter 不产生 [`EditKey::Submit`]**——IME 拥有
//!   全部按键,仅 Esc 映射 [`EditKey::CancelComposition`](取消组合而非取消
//!   编辑),防御"组合中按 Enter 误提交"(TC-CMP-TF-01 用例);
//! - [`word_range_at`]:双击选词的词边界(字母数字连串为词、空白连串为
//!   一段、标点单字符自成一段;经典编辑器三分语义,CJK 连串同为词段)
//! - [`caret_visible`]:光标闪烁(1s 周期前半亮,任意光标移动重置相位);
//!   reduced_motion 直通常亮(§5.3.3 归零规则)。
//!
//! # 鼠标选区(gpui 0.2.2 真实命中测量)
//!
//! 点击/拖选经 `WindowTextSystem::layout_line` + `LineLayout::
//! closest_index_for_x` 做 **x → char 索引**的真实字形命中(paint 期经
//! canvas 记录内容 bounds,事件期换算文本局部 x),双击选词走
//! [`word_range_at`]。字形命中需要 text system——纯函数层不含测量,组件层
//! 才接 gpui(与 NumberField `character_index_for_point` 的"M2 字形级"边界
//! 相比,本组件用 `layout_line` 把单行命中做实了)。
//!
//! # IME 组合态(A11Y-08 预埋,真机走查 = 第 4 组)
//!
//! gpui 0.2.2 的 IME 通路(本地 registry 源码核实):组合更新走
//! [`gpui::EntityInputHandler::replace_and_mark_text_in_range`],最终提交走
//! `replace_text_in_range`,平台经 `marked_text_range()` 查询组合态——
//! **Windows 平台在组合态吞掉全部 KeyDown**(platform/windows/events.rs
//! `handle_keydown_msg` 的 `is_composing` 分支,返回 0 不派发)。本组件:
//!
//! 1. 组合区间作为 buffer 的一等状态([`TextFieldBuffer::composition`]),
//!    `marked_text_range` 如实回报(组合态数据通路完整);
//! 2. 组合文本渲染 accent 下划线(`.underline()`,经典 IME 组合样式),
//!    组合中选区高亮让位(不高亮候选);
//! 3. 组件侧 [`edit_key`] 组合态门控(平台已吞键之外的纵深防御:即便
//!    平台在组合态仍派发 Enter,也不会误提交)。
//!
//! **覆盖边界(如实)**:真机中文输入法走查(DirectWrite/IMM32 全链路、
//! 候选窗定位精度、组合中光标相位)属第 4 组 A11Y-08 的 TC-A11Y-IME-01;
//! 本批交付的是**组合态数据通路 + 双重提交门控 + 组合渲染样式**,尚未覆盖
//! 真机回归与候选窗字符级定位(仍为控件 bounds 级,同 NumberField M2 边界)。
//!
//! # 视觉规格(纯函数,TOK 纪律)
//!
//! - 尺寸三档 [`TextFieldSize`] 与 Button 尺寸档**同一派生源**:高度/文字
//!   档直接取 `controls::button` 的 [`button_height`]/[`button_text`]
//!   (复用而非复制,"对齐 Button 尺寸档"由构造保证);
//! - 状态 → 样式映射集中在 [`text_field_style`]:底 = `surface_2`(L2 凸起
//!   输入材质,渲染时经 [`crate::theme::elevated`] 垫 [`ELEVATIONS[2]`] 阴影
//!   ——TOK-01 消费点),hover/press 走 [`crate::interact::state_layer`]
//!   (TOK-04);焦点 = accent 描边 + [`focus_ring_layers`](§5.3.3 两层环,
//!   与 choice/tabs 同源);
//! - 占位符 = `text_placeholder` 令牌(报告 §5.6 #3"占位色");
//! - **禁用 = 仅前景降级、容器不变**(TOK-07):[`text_field_style`] 的
//!   Disabled 分支容器与 Idle 逐位相同,前景/占位经
//!   [`crate::interact::disabled_foreground`] 降到 `text_disabled`,交互
//!   全门控(键盘/鼠标/IME 编辑路径全部拒收,焦点环不出现);
//! - **只读态**:可选可提交(Enter/导航/选区),编辑动作(插入/删除/IME
//!   替换)全部拒收;视觉与启用态一致(只读≠禁用)。

use std::rc::Rc;

use gpui::{
    App, Bounds, Context, ElementId, Entity, EntityInputHandler, FocusHandle, Font, FontWeight,
    Hsla, InteractiveElement, IntoElement, KeyDownEvent, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render, SharedString,
    StatefulInteractiveElement, Styled, TextRun, Window, canvas, div, px,
};

use crate::anim::{lerp_hsla, reduced_motion};
use crate::controls::button::{ButtonSize, button_height, button_text};
use crate::controls::choice::focus_ring_layers;
use crate::interact::{self, HoverState, InteractState, disabled_foreground, state_layer};
use crate::theme::theme;
use crate::tokens::{ColorTokens, RadiusTokens, SpacingTokens, TextSize, UI_FONT, h_flex};

// ---------------------------------------------------------------------------
// 几何常量(具名;非令牌表的组件本体尺寸,同 choice/number_field 惯例)
// ---------------------------------------------------------------------------

/// 光标条宽(1px 竖线,accent 色)。
const CARET_WIDTH_PX: f32 = 1.0;
/// 光标闪烁周期(ms):前半常亮、后半熄灭;任意光标移动重置相位。
pub const CARET_BLINK_PERIOD_MS: f64 = 1000.0;

// 组合态下划线 = accent(经 `.underline()` + `text_decoration_color`,
// 零新增令牌;渲染层接线)。

// ---------------------------------------------------------------------------
// 尺寸档(与 Button 尺寸档同一派生源:复用 button 的派生函数,零复制)
// ---------------------------------------------------------------------------

/// TextField 三尺寸(§5.6 #3"统一高度,对齐 Button 尺寸档")。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextFieldSize {
    /// 紧凑(行内小件;下限 22,同 Button Compact)
    Compact,
    /// 默认(表单/工具行;下限 26,同 Button Default)
    Default,
    /// 宽松(对话框主输入;下限 32,同 Button Roomy)
    Roomy,
}

impl TextFieldSize {
    /// 尺寸档 → Button 档(派生源统一的映射单点)。
    #[must_use]
    fn to_button(self) -> ButtonSize {
        match self {
            TextFieldSize::Compact => ButtonSize::Compact,
            TextFieldSize::Default => ButtonSize::Default,
            TextFieldSize::Roomy => ButtonSize::Roomy,
        }
    }
}

/// TextField 高度(**对齐 Button 尺寸档**的构造性保证:直接调用
/// [`button_height`] 派生——`max(档位下限, 行高 + 2×垂直 padding)`,
/// CJK/大字号/DPI 安全)。
#[must_use]
pub fn text_field_height(size: TextFieldSize) -> f32 {
    let button = size.to_button();
    button_height(button, button_text(button))
}

/// TextField 文字档(同 Button:Compact/Default = LABEL 12/18/500,Roomy =
/// BODY 13/20/400)。
#[must_use]
pub fn text_field_text(size: TextFieldSize) -> TextSize {
    button_text(size.to_button())
}

// ---------------------------------------------------------------------------
// 视觉规格(纯函数;TC-CMP-TF-01 的断言面)
// ---------------------------------------------------------------------------

/// TextField 一帧的完整样式裁决(纯数据)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextFieldStyle {
    /// 容器底(hover/press 含 state-layer 叠加)
    pub bg: Hsla,
    /// 正文前景(禁用 = `text_disabled`)
    pub fg: Hsla,
    /// 占位符前景(禁用 = `text_disabled`,启用 = `text_placeholder`)
    pub placeholder: Hsla,
    /// 描边(聚焦 = accent;hover/press = `border_strong`;静止 = `border_subtle`)
    pub border: Hsla,
    /// 容器高度([`text_field_height`])
    pub height: f32,
    /// 文字档
    pub text: TextSize,
    /// 水平内边距
    pub h_padding: f32,
    /// 圆角
    pub radius: f32,
}

/// 状态 → 样式(核心纯函数,TOK-04/TOK-07 组件落地单点):
///
/// - 容器底恒 `surface_2`(L2 凸起输入材质),hover/press 经
///   [`state_layer`] alpha 叠加(深色叠白/浅色叠黑);
/// - FocusRing 态:描边 = accent(环本体由渲染层 [`focus_ring_layers`] 绘制,
///   本函数只裁决描边色,同 Button 规则);
/// - **Disabled 分支容器与 Idle 逐位相同**(TOK-07 容器不变),仅前景降级。
#[must_use]
pub fn text_field_style(
    size: TextFieldSize,
    state: InteractState,
    colors: &ColorTokens,
) -> TextFieldStyle {
    let text = text_field_text(size);
    let h_padding = match size {
        TextFieldSize::Compact => SpacingTokens::XS,
        TextFieldSize::Default => SpacingTokens::SM,
        TextFieldSize::Roomy => SpacingTokens::MD,
    };
    let radius = match size {
        TextFieldSize::Compact => RadiusTokens::SM,
        TextFieldSize::Default | TextFieldSize::Roomy => RadiusTokens::MD,
    };
    let (bg, border) = match state {
        InteractState::Idle => (colors.surface_2, colors.border_subtle),
        InteractState::Hover => (
            state_layer(colors.surface_2, InteractState::Hover, colors.accent),
            colors.border_strong,
        ),
        InteractState::Pressed => (
            state_layer(colors.surface_2, InteractState::Pressed, colors.accent),
            colors.border_strong,
        ),
        InteractState::FocusRing | InteractState::Selected => (colors.surface_2, colors.accent),
        InteractState::Disabled => (colors.surface_2, colors.border_subtle),
    };
    let (fg, placeholder) = if state == InteractState::Disabled {
        (
            disabled_foreground(colors.text_primary, colors.text_disabled),
            disabled_foreground(colors.text_placeholder, colors.text_disabled),
        )
    } else {
        (colors.text_primary, colors.text_placeholder)
    };
    TextFieldStyle {
        bg,
        fg,
        placeholder,
        border,
        height: text_field_height(size),
        text,
        h_padding,
        radius,
    }
}

/// 光标是否点亮(纯函数):1s 周期前半亮;`reduced_motion` 直通常亮(§5.3.3
/// 装饰性闪烁归零)。`phase_started_ms` 为相位起点(任意光标移动/聚焦时重置,
/// 组件注入 [`interact::now_ms`] 时钟)。
#[must_use]
pub fn caret_visible(phase_started_ms: f64, now_ms: f64, reduced: bool) -> bool {
    if reduced {
        return true;
    }
    let elapsed = (now_ms - phase_started_ms).max(0.0);
    elapsed % CARET_BLINK_PERIOD_MS < CARET_BLINK_PERIOD_MS / 2.0
}

// ---------------------------------------------------------------------------
// 键盘意图状态机(纯函数;TC-CMP-TF-01"快捷键矩阵 + IME 不误提交"断言面)
// ---------------------------------------------------------------------------

/// 光标移动方向(←/→/Home/End)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextCaretMove {
    /// 左移一个字符(起点钳 0)
    Left,
    /// 右移一个字符(终点钳 len)
    Right,
    /// 行首(单行字段即文本首)
    Home,
    /// 行尾(单行字段即文本尾)
    End,
}

/// 键盘意图([`edit_key`] 的输出;组件按此执行 buffer 操作/回调)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKey {
    /// 光标移动(带 extend = shift 按下 → 移动选区头,否则折叠选区)
    Caret(TextCaretMove, bool),
    /// 删光标前字符/选区
    Backspace,
    /// 删光标后字符/选区
    Delete,
    /// 全选(Ctrl/⌘+A)
    SelectAll,
    /// 插入字符(实际键入字符优先,回退单字符 key)
    Insert(char),
    /// Enter:提交(组合态永不产生——见 [`edit_key`])
    Submit,
    /// Tab:提交并跳下一字段
    TabSubmit,
    /// Esc:还原到最近提交快照并回调 on_cancel(组合态不产生)
    Cancel,
    /// Esc(组合态):仅取消 IME 组合,不动编辑内容
    CancelComposition,
    /// 其余键:无意图
    Ignored,
}

/// 键盘 → 意图映射(唯一键盘状态机单点;`key`/`key_char` 取自
/// `KeyDownEvent.keystroke`,`modifiers` 同源,`composing` = IME 组合中):
///
/// - **组合态纵深防御**:composing 时除 Esc(→ [`EditKey::CancelComposition`])
///   外全部 [`EditKey::Ignored`]——`Submit`/`Insert` 不可能产生,组件即使
///   在"平台组合态仍派发按键"的平台上也不会误提交/误插入(TC-CMP-TF-01);
///   gpui 0.2.2 Windows 平台本就在组合态吞键(模块 doc"IME 组合态"),
///   本门控是第二道防线;
/// - Ctrl/Alt/⌘ 修饰的组合不落字符(保留给宿主快捷键);Ctrl/⌘+A = 全选;
/// - 字符插入优先 `key_char`(IME/非美式布局的真实键入),回退单字符 key。
#[must_use]
pub fn edit_key(
    key: &str,
    modifiers: &Modifiers,
    key_char: Option<&str>,
    composing: bool,
) -> EditKey {
    if composing {
        return match key {
            "escape" => EditKey::CancelComposition,
            _ => EditKey::Ignored,
        };
    }
    match key {
        "enter" => EditKey::Submit,
        "escape" => EditKey::Cancel,
        "tab" => EditKey::TabSubmit,
        "backspace" => EditKey::Backspace,
        "delete" => EditKey::Delete,
        "left" => EditKey::Caret(TextCaretMove::Left, modifiers.shift),
        "right" => EditKey::Caret(TextCaretMove::Right, modifiers.shift),
        "home" => EditKey::Caret(TextCaretMove::Home, modifiers.shift),
        "end" => EditKey::Caret(TextCaretMove::End, modifiers.shift),
        "a" if modifiers.control || modifiers.platform => EditKey::SelectAll,
        _ => {
            if modifiers.control || modifiers.alt || modifiers.platform {
                return EditKey::Ignored; // 修饰组合不落字符
            }
            typed_char(key, key_char).map_or(EditKey::Ignored, EditKey::Insert)
        }
    }
}

/// 实际键入字符(纯函数):`key_char` 单字符优先,回退单字符命名键("a" → 'a')。
#[must_use]
fn typed_char(key: &str, key_char: Option<&str>) -> Option<char> {
    let from_key_char = key_char.and_then(|s| {
        let mut chars = s.chars();
        let first = chars.next()?;
        chars.next().is_none().then_some(first)
    });
    from_key_char.or_else(|| {
        let mut chars = key.chars();
        let first = chars.next()?;
        chars.next().is_none().then_some(first)
    })
}

// ---------------------------------------------------------------------------
// 词边界(双击选词;纯函数)
// ---------------------------------------------------------------------------

/// 双击选词的词区间(纯函数,经典编辑器三分语义):**字母数字连串**为词
/// (Unicode `char::is_alphanumeric` 口径,CJK 连串同为一段——无分词词典)、
/// **空白连串**为一段、**标点单字符**自成一段。越界下标钳到 char 边界,
/// 空文本返回 0..0。
#[must_use]
pub fn word_range_at(text: &str, byte: usize) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }
    let byte = boundary_floor(text, byte.min(text.len()));
    let class_of = |c: char| {
        if c.is_alphanumeric() {
            0 // 词
        } else if c.is_whitespace() {
            1 // 空白段
        } else {
            2 // 标点(单字符自成一段)
        }
    };
    let Some(first) = text[byte..].chars().next() else {
        return (byte, byte);
    };
    let want = class_of(first);
    if want == 2 {
        return (byte, byte + first.len_utf8());
    }
    let mut start = byte;
    while start > 0 {
        let prev = boundary_floor(text, start - 1);
        match text[prev..].chars().next() {
            Some(c) if class_of(c) == want => start = prev,
            _ => break,
        }
    }
    let mut end = byte + first.len_utf8();
    while end < text.len() {
        match text[end..].chars().next() {
            Some(c) if class_of(c) == want => end += c.len_utf8(),
            _ => break,
        }
    }
    (start, end)
}

/// ≤ index 的最近 char 边界(平台/测量下标防御,宁少勿多)。
#[must_use]
fn boundary_floor(text: &str, index: usize) -> usize {
    let mut i = index.min(text.len());
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// char 边界字节下标 → UTF-16 下标(IME 协议口径;越界钳到末尾)。
#[must_use]
fn byte_to_utf16(text: &str, byte_index: usize) -> usize {
    text[..byte_index.min(text.len())]
        .chars()
        .map(char::len_utf16)
        .sum()
}

/// UTF-16 下标 → char 边界字节下标(越界返回 `None`;落在多单元字符中间
/// 时吸附到该字符后边界)。
#[must_use]
fn utf16_to_byte(text: &str, utf16_index: usize) -> Option<usize> {
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

// ---------------------------------------------------------------------------
// 编辑缓冲(文本 + 光标 + 选区 + IME 组合区间;纯数据 + 纯函数操作)
// ---------------------------------------------------------------------------

/// 渲染分段种类([`TextFieldRun`])。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFieldRunKind {
    /// 普通文本
    Plain,
    /// 选区高亮段
    Selected,
    /// IME 组合段(accent 下划线;组合中选区高亮让位)
    Composing,
}

/// 渲染分段(纯数据;`caret_before`/`caret_after` 标记光标条插入位)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFieldRun {
    /// 分段种类
    pub kind: TextFieldRunKind,
    /// 分段文本
    pub text: String,
    /// 光标在本段之前
    pub caret_before: bool,
    /// 光标在本段之后
    pub caret_after: bool,
}

/// 文本编辑缓冲:光标/选区锚点/IME 组合区间全部是 **char 边界字节偏移**
/// (全部公开操作恒维持该不变量;IME/平台下标经 `boundary_floor` 防御)。
///
/// 纯数据 + 纯函数操作,单测覆盖插入/删除/选区归一/组合态(NumberField
/// `EditBuffer` 的选区 + 组合扩展版,无 App 依赖)。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TextFieldBuffer {
    text: String,
    caret: usize,
    /// 选区锚点(`None` = 无选区,光标即 `caret`)
    anchor: Option<usize>,
    /// IME 组合(marked)区间
    composition: Option<std::ops::Range<usize>>,
}

impl TextFieldBuffer {
    /// 从初始文本构造(光标在末尾,无选区)。
    #[must_use]
    pub fn new(initial: &str) -> Self {
        TextFieldBuffer {
            text: initial.to_string(),
            caret: initial.len(),
            anchor: None,
            composition: None,
        }
    }

    /// 缓冲文本。
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 光标位置(char 边界字节偏移)。
    #[must_use]
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// 归一化选区(`start ≤ end`;无选区返回 `None`)。
    #[must_use]
    pub fn selection(&self) -> Option<std::ops::Range<usize>> {
        let anchor = self.anchor?;
        let (start, end) = if anchor <= self.caret {
            (anchor, self.caret)
        } else {
            (self.caret, anchor)
        };
        Some(start..end)
    }

    /// 是否有非空选区。
    #[must_use]
    pub fn has_selection(&self) -> bool {
        self.selection().is_some_and(|s| !s.is_empty())
    }

    /// IME 组合区间。
    #[must_use]
    pub fn composition(&self) -> Option<std::ops::Range<usize>> {
        self.composition.clone()
    }

    /// 是否组合中。
    #[must_use]
    pub fn is_composing(&self) -> bool {
        self.composition.is_some()
    }

    /// 整体重置(宿主 [`TextField::set_text`] / Esc 还原共用):文本替换,
    /// 光标到末尾,选区/组合清空。
    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
        self.caret = text.len();
        self.anchor = None;
        self.composition = None;
    }

    /// 在光标处插入文本(有选区先替换选区);光标落在插入文本之后。
    /// 返回是否发生变化。
    pub fn insert(&mut self, text: &str) -> bool {
        if text.is_empty() && !self.has_selection() {
            return false;
        }
        if let Some(sel) = self.selection() {
            self.text.replace_range(sel.clone(), "");
            self.caret = sel.start;
        }
        self.text.insert_str(self.caret, text);
        self.caret += text.len();
        self.anchor = None;
        true
    }

    /// 删光标前一个字符;有选区先删选区。返回是否发生删除。
    pub fn backspace(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.caret == 0 {
            return false;
        }
        let prev = boundary_floor(&self.text, self.caret - 1);
        self.text.replace_range(prev..self.caret, "");
        self.caret = prev;
        true
    }

    /// 删光标后一个字符;有选区先删选区。返回是否发生删除。
    pub fn delete(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.caret >= self.text.len() {
            return false;
        }
        let end = self.text[self.caret..]
            .chars()
            .next()
            .map_or(1, char::len_utf8);
        self.text.replace_range(self.caret..self.caret + end, "");
        true
    }

    /// 删除选区(若有)。返回是否发生了删除。
    fn delete_selection(&mut self) -> bool {
        let Some(sel) = self.selection().filter(|s| !s.is_empty()) else {
            return false;
        };
        self.text.replace_range(sel.clone(), "");
        self.caret = sel.start;
        self.anchor = None;
        true
    }

    /// 光标移动(`extend` = shift:移动选区头,锚点不动;否则折叠选区)。
    /// 返回光标是否变化。
    pub fn move_caret(&mut self, direction: TextCaretMove, extend: bool) -> bool {
        let next = match direction {
            TextCaretMove::Left => boundary_floor(&self.text, self.caret.saturating_sub(1)),
            TextCaretMove::Right => self.text[self.caret..]
                .chars()
                .next()
                .map_or(self.caret, |c| self.caret + c.len_utf8()),
            TextCaretMove::Home => 0,
            TextCaretMove::End => self.text.len(),
        };
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.caret);
            }
        } else {
            self.anchor = None;
        }
        if next == self.caret {
            return false;
        }
        self.caret = next;
        true
    }

    /// 设置光标(点击命中;折叠选区,越界/撕裂下标钳到 char 边界)。
    pub fn set_caret(&mut self, byte_index: usize) {
        self.caret = boundary_floor(&self.text, byte_index.min(self.text.len()));
        self.anchor = None;
    }

    /// 开始拖选:锚点 = 光标(点击建立选区起点;后续拖动只动 caret)。
    pub fn begin_selection_drag(&mut self) {
        self.anchor = Some(self.caret);
    }

    /// 全选。
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.text.len();
    }

    /// 双击选词:光标处词区间([`word_range_at`])。
    pub fn select_word(&mut self) {
        let (start, end) = word_range_at(&self.text, self.caret);
        self.anchor = Some(start);
        self.caret = end;
    }

    /// 区间替换(IME `replace_text_in_range` 带范围路径):start/end 一律
    /// 向下取 char 边界(宁少勿多),替换后光标落在插入文本之后。
    pub fn replace_range(&mut self, range: std::ops::Range<usize>, text: &str) {
        let start = boundary_floor(&self.text, range.start.min(self.text.len()));
        let end = boundary_floor(&self.text, range.end.min(self.text.len()));
        let (start, end) = (start.min(end), start.max(end));
        self.text.replace_range(start..end, text);
        self.caret = start + text.len();
        self.anchor = None;
    }

    // —— IME 组合态(组合区间是一等状态;见模块 doc)——

    /// 组合更新(`replace_and_mark_text_in_range` 路径;平台每次发**全量**
    /// 组合串):已有组合区间则原位**替换**,否则替换当前选区/在光标处插入;
    /// 新组合区间 = 插入文本区间,光标取平台给的组合内偏移(UTF-16 口径,
    /// 防御性钳到边界)。
    pub fn composition_update(&mut self, new_text: &str, caret_in_new_utf16: Option<usize>) {
        let start = match self.composition.take() {
            Some(range) => {
                let start = boundary_floor(&self.text, range.start.min(self.text.len()));
                let end = boundary_floor(&self.text, range.end.min(self.text.len())).max(start);
                self.text.replace_range(start..end, "");
                self.caret = start;
                start
            }
            None => {
                let sel = self.selection().filter(|s| !s.is_empty());
                let start = sel.as_ref().map_or(self.caret, |s| s.start);
                if let Some(sel) = sel {
                    self.text.replace_range(sel.clone(), "");
                }
                self.caret = boundary_floor(&self.text, start.min(self.text.len()));
                self.caret
            }
        };
        self.text.insert_str(start, new_text);
        let caret_in_new = caret_in_new_utf16
            .and_then(|u| utf16_to_byte(new_text, u))
            .unwrap_or(new_text.len());
        self.caret = start + caret_in_new;
        self.anchor = None;
        self.composition = Some(start..start + new_text.len());
    }

    /// 组合提交(`replace_text_in_range` 组合路径):组合文本替换为最终
    /// 文本,组合区间清空,光标落最终文本之后。无组合时等同普通插入。
    pub fn composition_commit(&mut self, final_text: &str) {
        let Some(range) = self.composition.take() else {
            self.insert(final_text);
            return;
        };
        self.replace_range(range, final_text);
    }

    /// 取消组合(组合态 Esc;组合文本移除,光标回落到组合起点)。
    /// 返回是否发生了取消。
    pub fn composition_cancel(&mut self) -> bool {
        let Some(range) = self.composition.take() else {
            return false;
        };
        let start = boundary_floor(&self.text, range.start.min(self.text.len()));
        let end = boundary_floor(&self.text, range.end.min(self.text.len())).max(start);
        self.text.replace_range(start..end, "");
        self.caret = start;
        self.anchor = None;
        true
    }

    /// 解除组合标记(平台 `unmark_text`;组合文本保留为普通文本)。
    pub fn unmark(&mut self) {
        if let Some(range) = self.composition.take() {
            self.caret = boundary_floor(&self.text, range.end.min(self.text.len()));
            self.anchor = None;
        }
    }

    /// 渲染分段(纯函数):在 {选区端点, 组合端点, 光标} 边界处把文本切成
    /// 有序段,每段带种类与光标标记。**组合中选区高亮让位**(段种类只出
    /// Composing,不出 Selected——"不高亮候选"的落地);切分不含空段
    /// (全空文本返回带光标标记的单空段,保证光标条可渲染)。
    #[must_use]
    pub fn render_runs(&self) -> Vec<TextFieldRun> {
        let composing = self.is_composing();
        let selection = if composing { None } else { self.selection() };
        let composition = self.composition.clone();
        let mut cuts = vec![0, self.text.len()];
        for range in [selection.as_ref(), composition.as_ref()]
            .into_iter()
            .flatten()
        {
            cuts.push(range.start);
            cuts.push(range.end);
        }
        cuts.push(self.caret);
        cuts.sort_unstable();
        cuts.dedup();
        let mut runs = Vec::new();
        for pair in cuts.windows(2) {
            let (start, end) = (pair[0], pair[1]);
            let in_composition = composition
                .as_ref()
                .is_some_and(|c| start >= c.start && end <= c.end);
            let in_selection = selection
                .as_ref()
                .is_some_and(|s| s.start < s.end && start >= s.start && end <= s.end);
            let kind = if composing && in_composition {
                TextFieldRunKind::Composing
            } else if in_selection {
                TextFieldRunKind::Selected
            } else {
                TextFieldRunKind::Plain
            };
            runs.push(TextFieldRun {
                kind,
                text: self.text[start..end].to_string(),
                caret_before: start == self.caret,
                caret_after: end == self.caret,
            });
        }
        if runs.is_empty() {
            runs.push(TextFieldRun {
                kind: TextFieldRunKind::Plain,
                text: String::new(),
                caret_before: self.caret == 0,
                caret_after: true,
            });
        }
        runs
    }
}

// ---------------------------------------------------------------------------
// TextField(Entity 形态,规范 API)
// ---------------------------------------------------------------------------

/// 提交回调:`fn(提交文本, &mut Window, &mut App)`(Enter/Tab 触发)。
pub type SubmitFn = Rc<dyn Fn(&str, &mut Window, &mut App)>;
/// 取消回调:`fn(&mut Window, &mut App)`(Esc 还原时触发)。
pub type CancelFn = Rc<dyn Fn(&mut Window, &mut App)>;

/// 单行文本输入框(§5.6 #3)。规范用法 = Entity 形态:
///
/// ```ignore
/// cx.new(|_| {
///     TextField::new("layer-name", "背景")
///         .placeholder("输入图层名…")
///         .on_submit(|text, _win, _cx| println!("{text}"))
/// })
/// ```
pub struct TextField {
    id: ElementId,
    buffer: TextFieldBuffer,
    /// 最近提交快照(Esc 还原锚;失焦时同步为当前文本)
    committed: String,
    placeholder: Option<SharedString>,
    size: TextFieldSize,
    disabled: bool,
    read_only: bool,
    /// 渲染/测量共用的字体(UI 族 + 当前文字档字重)
    font: Font,
    on_submit: Option<SubmitFn>,
    on_cancel: Option<CancelFn>,
    /// 焦点句柄(首帧惰性创建,`tab_stop(true)` 进 Tab 环游)
    focus: Option<FocusHandle>,
    /// A7 悬停进度(120ms ease-out;禁用冻结)
    hover: HoverState,
    /// 光标闪烁相位起点(任意光标移动/聚焦重置)
    caret_phase_started_ms: f64,
    /// 拖选中(按下建立锚点,拖动只动光标)
    dragging: bool,
    /// 内容区 bounds(paint 期 canvas 记录;鼠标命中换算用)
    content_bounds: Option<Bounds<Pixels>>,
}

impl TextField {
    /// 文本输入框;默认 Default 尺寸 + 启用可编辑态。
    pub fn new(id: impl Into<ElementId>, initial: impl Into<String>) -> Self {
        let initial = initial.into();
        TextField {
            id: id.into(),
            committed: initial.clone(),
            buffer: TextFieldBuffer::new(&initial),
            placeholder: None,
            size: TextFieldSize::Default,
            disabled: false,
            read_only: false,
            font: Self::font_for(TextSize::LABEL),
            on_submit: None,
            on_cancel: None,
            focus: None,
            hover: HoverState::new(),
            caret_phase_started_ms: interact::now_ms(),
            dragging: false,
            content_bounds: None,
        }
    }

    /// 当前文字档对应的渲染/测量字体(单点:UI 族 + 档位字重)。
    #[must_use]
    pub fn font_for(text: TextSize) -> Font {
        let mut font = gpui::font(UI_FONT);
        font.weight = FontWeight(text.weight);
        font
    }

    /// 占位符(空文本时显示,`text_placeholder` 色)。
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// 尺寸(默认 [`TextFieldSize::Default`];高度对齐 Button 尺寸档)。
    #[must_use]
    pub fn size(mut self, size: TextFieldSize) -> Self {
        self.size = size;
        self
    }

    /// 禁用态(TOK-07):容器不变仅前景降级,键盘/鼠标/IME 交互全门控。
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 只读态:可选区/导航/提交,编辑动作(插入/删除/IME 替换)拒收;
    /// 视觉与启用态一致。
    #[must_use]
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// 提交回调(Enter/Tab;只读态同样触发)。
    #[must_use]
    pub fn on_submit(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_submit = Some(Rc::new(f));
        self
    }

    /// 取消回调(Esc 还原时触发)。
    #[must_use]
    pub fn on_cancel(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_cancel = Some(Rc::new(f));
        self
    }

    /// 当前文本(编辑中含未提交内容)。
    #[must_use]
    pub fn text(&self) -> &str {
        self.buffer.text()
    }

    /// 是否 IME 组合中(宿主调试/测试可读)。
    #[must_use]
    pub fn is_composing(&self) -> bool {
        self.buffer.is_composing()
    }

    /// 是否聚焦(渲染期快照语义;未渲染过 = 未聚焦)。
    #[must_use]
    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus.as_ref().is_some_and(|f| f.is_focused(window))
    }

    /// 宿主受控写入:整体替换文本(编辑缓冲 + 提交快照同步),光标到末尾。
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.buffer.set_text(text);
        self.committed = text.to_string();
        cx.notify();
    }

    /// 提交(Enter/Tab):快照同步 + 回调。公开为宿主/测试入口。
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.committed = self.buffer.text().to_string();
        if let Some(cb) = self.on_submit.clone() {
            cb(self.committed.as_str(), window, cx);
        }
        cx.notify();
    }

    /// 取消(Esc):还原到提交快照 + 回调。
    pub fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.buffer.text() != self.committed {
            self.buffer.set_text(&self.committed);
        }
        if let Some(cb) = self.on_cancel.clone() {
            cb(window, cx);
        }
        cx.notify();
    }

    /// 键盘处理:`edit_key` 意图 → buffer 操作/回调(禁用全门控;只读只拦
    /// 编辑动作)。
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return; // TOK-07:禁用即交互门控
        }
        let keystroke = &event.keystroke;
        let composing = self.buffer.is_composing();
        let action = edit_key(
            keystroke.key.as_str(),
            &keystroke.modifiers,
            keystroke.key_char.as_deref(),
            composing,
        );
        let editable = !self.read_only;
        let mut moved = false;
        match action {
            EditKey::Ignored => return,
            EditKey::Caret(direction, extend) => {
                moved = self.buffer.move_caret(direction, extend);
            }
            EditKey::SelectAll => self.buffer.select_all(),
            EditKey::Submit => {
                self.submit(window, cx);
                return;
            }
            EditKey::TabSubmit => {
                self.submit(window, cx);
                window.focus_next();
                return;
            }
            EditKey::Cancel => {
                self.cancel(window, cx);
                return;
            }
            EditKey::CancelComposition => {
                self.buffer.composition_cancel();
                cx.notify();
                return;
            }
            EditKey::Backspace if editable => {
                moved = self.buffer.backspace();
            }
            EditKey::Delete if editable => {
                moved = self.buffer.delete();
            }
            EditKey::Insert(ch) if editable => {
                moved = self.buffer.insert(ch.encode_utf8(&mut [0; 4]));
            }
            // 只读态的编辑动作:吞掉(可选区/导航不受影响)
            EditKey::Backspace | EditKey::Delete | EditKey::Insert(_) => {}
        }
        if moved || self.buffer.has_selection() {
            self.reset_caret_phase();
        }
        cx.notify();
    }

    /// 光标相位重置(任意移动/点击/聚焦;闪烁从"亮"起算)。
    fn reset_caret_phase(&mut self) {
        self.caret_phase_started_ms = interact::now_ms();
    }

    /// 鼠标按下:接管焦点 + 真实命中定位(双击选词,单击建锚)。
    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return; // TOK-07:禁用即交互门控
        }
        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        focus.focus(window);
        let fg = theme(cx).colors.text_primary;
        let index = self.hit_index(window, f32::from(event.position.x), fg);
        if event.click_count >= 2 {
            self.buffer.set_caret(index);
            self.buffer.select_word();
        } else {
            self.buffer.set_caret(index);
            self.buffer.begin_selection_drag();
            self.dragging = true;
        }
        self.reset_caret_phase();
        cx.notify();
    }

    /// 拖选:光标跟随命中位置(锚点不动,选区归一在 buffer)。
    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled || !self.dragging {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            self.dragging = false;
            return;
        }
        let fg = theme(cx).colors.text_primary;
        let index = self.hit_index(window, f32::from(event.position.x), fg);
        let before = self.buffer.caret();
        self.buffer.set_caret(index);
        if self.buffer.caret() != before {
            self.reset_caret_phase();
            cx.notify();
        }
    }

    /// 拖选结束(松开在字段内/外)。
    fn on_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.dragging = false;
    }

    /// A7 悬停进出(禁用态忽略,TOK-07 无悬停反馈)。
    fn on_hover_changed(&mut self, hovered: &bool, _window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let now = interact::now_ms();
        if *hovered {
            self.hover.on_enter(now);
        } else {
            self.hover.on_leave(now);
        }
        cx.notify();
    }

    /// 失焦同步快照:编辑中失焦(点击别处/Tab)→ 提交快照跟随当前文本
    /// (失焦不触发 on_submit——提交语义只属显式 Enter/Tab)。
    fn sync_committed_if_blurred(&mut self, window: &mut Window) {
        if !self.is_focused(window) && self.buffer.text() != self.committed {
            self.committed = self.buffer.text().to_string();
        }
    }

    /// x(窗口坐标)→ 光标 char 索引:内容 bounds(paint 期记录)+ 水平
    /// padding 换算文本局部 x,经 `WindowTextSystem::layout_line` 真实字形
    /// 命中(`closest_index_for_x`),向下吸附 char 边界。bounds 未记录
    /// (首帧前)时退化为文本尾,空文本命中 0。
    fn hit_index(&self, window: &Window, window_x: f32, fg: Hsla) -> usize {
        let Some(bounds) = self.content_bounds else {
            return self.buffer.text().len();
        };
        let text = self.buffer.text();
        if text.is_empty() {
            return 0;
        }
        let h_padding = match self.size {
            TextFieldSize::Compact => SpacingTokens::XS,
            TextFieldSize::Default => SpacingTokens::SM,
            TextFieldSize::Roomy => SpacingTokens::MD,
        };
        let local_x = window_x - f32::from(bounds.origin.x) - h_padding;
        self.measured_index(window, local_x, text, fg)
            .unwrap_or(text.len())
    }

    /// 当前文字档字号(渲染与测量同源)。
    fn font_size(&self) -> f32 {
        text_field_text(self.size).size
    }
}

impl TextField {
    /// 字形测量命中(共享内部:`layout_line` + `closest_index_for_x`,
    /// 结果向下吸附 char 边界)。`fg` 仅填充 TextRun 布局无关字段(取自
    /// 调用点的主题色,零字面量)。文本系统不可用返回 `None`(调用方退化)。
    fn measured_index(&self, window: &Window, local_x: f32, text: &str, fg: Hsla) -> Option<usize> {
        let run = TextRun {
            len: text.len(),
            font: self.font.clone(),
            color: fg,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let layout = window
            .text_system()
            .layout_line(text, px(self.font_size()), &[run], None);
        Some(boundary_floor(
            text,
            layout
                .closest_index_for_x(px(local_x.max(0.0)))
                .min(text.len()),
        ))
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_committed_if_blurred(window);

        let colors = &theme(cx).colors;
        let disabled = self.disabled;
        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let focused = !disabled && focus.is_focused(window);
        let now = interact::now_ms();
        let hover_progress = if disabled {
            0.0
        } else {
            f32_val(self.hover.progress_at(now))
        };

        // 状态裁决:Disabled > Focus(聚焦优先于 hover,输入位主态)> Hover > Idle
        let state = if disabled {
            InteractState::Disabled
        } else if focused {
            InteractState::FocusRing
        } else if hover_progress > 0.0 {
            InteractState::Hover
        } else {
            InteractState::Idle
        };
        let mut style = text_field_style(self.size, state, colors);
        if hover_progress > 0.0 && !focused && !disabled {
            // hover 进度插值(静止底 → hover 底,120ms ease-out;NumberField 同款)
            let hover_bg = text_field_style(self.size, InteractState::Hover, colors).bg;
            style.bg = lerp_hsla(style.bg, hover_bg, f64::from(hover_progress));
        }

        // 文本分段(占位符 / 普通段 + 选区高亮 + 组合下划线 + 光标条)
        let text_size = px(style.text.size);
        let runs = self.buffer.render_runs();
        let empty = self.buffer.text().is_empty();
        let line = h_flex().min_w_0();
        let line = if empty {
            let placeholder = self.placeholder.clone().unwrap_or_else(|| "".into());
            line.child(
                div()
                    .font_family(UI_FONT)
                    .text_size(text_size)
                    .font_weight(FontWeight(style.text.weight))
                    .text_color(style.placeholder)
                    .truncate()
                    .child(placeholder),
            )
        } else {
            let mut line = line;
            for run in runs {
                let mut seg = div()
                    .font_family(UI_FONT)
                    .text_size(text_size)
                    .font_weight(FontWeight(style.text.weight))
                    .whitespace_normal();
                seg = match run.kind {
                    TextFieldRunKind::Plain => seg.text_color(style.fg),
                    TextFieldRunKind::Selected => {
                        // 选区底 = accent 同比例 alpha(TOK-04 Selected;深浅一致)
                        seg.text_color(style.fg).bg(state_layer(
                            colors.surface_2,
                            InteractState::Selected,
                            colors.accent,
                        ))
                    }
                    TextFieldRunKind::Composing => {
                        // IME 组合:accent 下划线(经典组合样式;组合中无选区高亮)
                        seg.text_color(style.fg)
                            .underline()
                            .text_decoration_color(colors.accent)
                    }
                };
                if run.caret_before {
                    line = line.child(caret_bar(colors, style.text.line_height));
                }
                line = line.child(seg.child(run.text));
                if run.caret_after {
                    line = line.child(caret_bar(colors, style.text.line_height));
                }
            }
            line
        };

        // 内容层:样式 + 分段 + IME/paint 钩子 canvas(记录 bounds + 注册
        // 平台输入处理器;handle_input 断言 Paint 阶段,NumberField 同款)
        let ime_focus = focus.clone();
        let ime_entity: Entity<Self> = cx.entity();
        let bounds_entity = ime_entity.clone();
        let content = h_flex()
            .w_full()
            .h(px(style.height))
            .px(px(style.h_padding))
            .rounded(px(style.radius))
            .border_1()
            .border_color(style.border)
            .bg(style.bg)
            .overflow_hidden()
            .child(line)
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        bounds_entity.update(cx, |this, _| {
                            // paint 期记录内容 bounds(鼠标命中换算;变更不
                            // notify——无需额外重绘,blink 已有帧泵)
                            this.content_bounds = Some(bounds);
                        });
                        window.handle_input(
                            &ime_focus,
                            gpui::ElementInputHandler::new(bounds, ime_entity),
                            cx,
                        );
                    },
                )
                .absolute()
                .inset_0(),
            );

        // 根:焦点 + 事件;禁用不挂任何交互监听(TOK-07)
        let mut root = crate::theme::elevated(2, content)
            .min_w_0()
            .relative()
            .track_focus(&focus);
        if focused {
            root = root.children(focus_ring_layers(colors.accent, style.radius));
        }
        let element = if disabled {
            root.into_any_element()
        } else {
            root.id(self.id.clone())
                .cursor_text()
                .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                .on_mouse_move(cx.listener(Self::on_mouse_move))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                .on_key_down(cx.listener(Self::on_key_down))
                .on_hover(cx.listener(Self::on_hover_changed))
                .into_any_element()
        };

        // 光标闪烁帧泵:聚焦且减弱动态未开才续帧(静止/禁用零帧提交)
        if focused && !reduced_motion() {
            window.request_animation_frame();
        }
        element
    }
}

/// 光标条(1px accent 竖线,行高 = 当前文字档;组合/选区渲染共用)。
fn caret_bar(colors: &ColorTokens, line_height: f32) -> gpui::Div {
    div()
        .w(px(CARET_WIDTH_PX))
        .h(px(line_height))
        .flex_shrink_0()
        .bg(colors.accent)
}

/// f64 进度 → f32(GPU 域收口,panels/button 同款惯例)。
#[allow(clippy::cast_possible_truncation)]
fn f32_val(v: f64) -> f32 {
    v as f32
}

// ---------------------------------------------------------------------------
// IME / 平台文本输入(gpui 0.2.2 `EntityInputHandler`,见模块 doc)
//
// 与 NumberField 的差别:组合(marked)区间是 buffer 一等状态,
// `marked_text_range` 如实回报 UTF-16 区间——这正是 Windows 平台在组合态
// 吞掉 KeyDown 的判定输入(数据通路的核心一环)。
// ---------------------------------------------------------------------------
impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range_utf16: std::ops::Range<usize>,
        adjusted_range: &mut Option<std::ops::Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let text = self.buffer.text();
        let start = utf16_to_byte(text, range_utf16.start)?;
        let end = utf16_to_byte(text, range_utf16.end)?;
        adjusted_range.replace(byte_to_utf16(text, start)..byte_to_utf16(text, end));
        Some(text[start..end].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::UTF16Selection> {
        let text = self.buffer.text();
        match self.buffer.selection() {
            Some(sel) => Some(gpui::UTF16Selection {
                range: byte_to_utf16(text, sel.start)..byte_to_utf16(text, sel.end),
                reversed: self.buffer.caret() < sel.start,
            }),
            None => {
                let caret = byte_to_utf16(text, self.buffer.caret());
                Some(gpui::UTF16Selection {
                    range: caret..caret,
                    reversed: false,
                })
            }
        }
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        // 组合态如实回报(UTF-16 口径)——Windows 平台据此吞掉组合中的
        // KeyDown(模块 doc"IME 组合态");本组件的组合态数据通路核心。
        let range = self.buffer.composition()?;
        let text = self.buffer.text();
        Some(byte_to_utf16(text, range.start)..byte_to_utf16(text, range.end))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.buffer.unmark();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<std::ops::Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled || self.read_only {
            return; // 只读/禁用:平台替换路径同样拒收
        }
        match range_utf16 {
            Some(range) => {
                let buffer_text = self.buffer.text().to_string();
                let start = utf16_to_byte(&buffer_text, range.start).unwrap_or(self.buffer.caret());
                let end = utf16_to_byte(&buffer_text, range.end).unwrap_or(self.buffer.caret());
                self.buffer.replace_range(start..end, text);
                // 替换区覆盖组合区(平台最终提交形态)→ 组合结束
                if let Some(comp) = self.buffer.composition() {
                    let (s, e) = (start.min(end), start.max(end));
                    if s <= comp.end && e >= comp.start {
                        self.buffer.unmark();
                    }
                }
            }
            None if self.buffer.is_composing() => {
                self.buffer.composition_commit(text); // Windows GCS_RESULTSTR 提交路径
            }
            None => {
                self.buffer.insert(text);
            }
        }
        self.reset_caret_phase();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<std::ops::Range<usize>>,
        new_text: &str,
        new_selected_range: Option<std::ops::Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled || self.read_only {
            return; // 只读/禁用:不进组合态
        }
        // Windows 组合更新恒传 None(替换既有组合/在插入点开始组合);
        // Some(range) 形态(先删指定区间再组合)按区间删除后从该点组合。
        if let Some(range) = range_utf16 {
            let buffer_text = self.buffer.text().to_string();
            let start = utf16_to_byte(&buffer_text, range.start).unwrap_or(self.buffer.caret());
            let end = utf16_to_byte(&buffer_text, range.end).unwrap_or(self.buffer.caret());
            self.buffer.replace_range(start..end, "");
        }
        let caret_in_new = new_selected_range.as_ref().map(|sel| sel.start);
        self.buffer.composition_update(new_text, caret_in_new);
        self.reset_caret_phase();
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: std::ops::Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // 候选窗定位到控件本身(字符级定位 = M2,需字形测量;边界与
        // NumberField 一致,真机走查 = 第 4 组 A11Y-08)
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        // 点选定位:真实字形命中(单行;与鼠标 hit_index 同一换算)
        let text = self.buffer.text();
        if text.is_empty() {
            return Some(0);
        }
        let bounds = self.content_bounds?;
        let fg = theme(cx).colors.text_primary;
        let measured = self.measured_index(
            window,
            f32::from(point.x) - f32::from(bounds.origin.x),
            text,
            fg,
        )?;
        Some(byte_to_utf16(text, measured))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Modifiers;

    const T0: f64 = 10_000.0;

    // —— TC-CMP-TF-01:编辑操作矩阵(插入/删除/选区)——

    #[test]
    fn tc_cmp_tf_01_edit_matrix_insert_delete_selection() {
        let mut b = TextFieldBuffer::new("abc");
        // 插入:光标在末尾
        assert!(b.insert("de"));
        assert_eq!(b.text(), "abcde");
        assert_eq!(b.caret(), 5);
        // 光标移到 b 后插入
        b.set_caret(1);
        assert!(b.insert("X"));
        assert_eq!(b.text(), "aXbcde");
        assert_eq!(b.caret(), 2);
        // backspace 删光标前字符
        assert!(b.backspace());
        assert_eq!(b.text(), "abcde");
        assert_eq!(b.caret(), 1);
        // delete 删光标后字符
        assert!(b.delete());
        assert_eq!(b.text(), "acde");
        // 边界:起点 backspace / 终点 delete 无操作
        b.set_caret(0);
        assert!(!b.backspace());
        b.set_caret(b.text().len());
        assert!(!b.delete());
        // 选区:anchor(a)→caret(c) 拖选,backspace 整体删
        b.set_caret(1);
        b.begin_selection_drag();
        b.move_caret(TextCaretMove::Right, true);
        assert_eq!(b.selection(), Some(1..2));
        assert!(b.has_selection());
        assert!(b.backspace(), "选区存在时 backspace 删选区");
        assert_eq!(b.text(), "ade");
        assert!(!b.has_selection());
        // 插入替换选区
        b.set_caret(0);
        b.select_all();
        assert!(b.insert("Q"));
        assert_eq!(b.text(), "Q", "插入替换全选内容");
        assert_eq!(b.caret(), 1);
    }

    #[test]
    fn tc_cmp_tf_01_caret_movement_and_selection_normalization() {
        let mut b = TextFieldBuffer::new("héllo");
        // é 是 2 字节:光标按 char 边界走(h → é 前 = 1,过 é = 3)
        assert!(b.move_caret(TextCaretMove::Home, false));
        assert_eq!(b.caret(), 0);
        assert!(b.move_caret(TextCaretMove::Right, false), "过 h");
        assert_eq!(b.caret(), 1);
        assert!(b.move_caret(TextCaretMove::Right, false), "跳过 2 字节的 é");
        assert_eq!(b.caret(), 3);
        assert!(b.move_caret(TextCaretMove::Left, false), "左移回 é 前");
        assert_eq!(b.caret(), 1);
        assert!(b.move_caret(TextCaretMove::Left, false));
        assert!(
            !b.move_caret(TextCaretMove::Left, false),
            "回到 0 前无操作时返回 false"
        );
        assert_eq!(b.caret(), 0);
        assert!(b.move_caret(TextCaretMove::End, false));
        assert_eq!(b.caret(), b.text().len());
        // shift 扩选 / 反向选区归一(扩选跨 h + é = 3 字节)
        b.move_caret(TextCaretMove::Home, false);
        b.move_caret(TextCaretMove::Right, true);
        b.move_caret(TextCaretMove::Right, true);
        assert_eq!(b.selection(), Some(0..3), "正向扩选跨重音字节");
        b.move_caret(TextCaretMove::Left, true);
        b.move_caret(TextCaretMove::Left, true);
        assert_eq!(b.selection(), Some(0..0), "选区折叠为空");
        // 非 extend 移动折叠选区
        b.select_all();
        assert!(b.has_selection());
        b.move_caret(TextCaretMove::Home, false);
        assert!(!b.has_selection(), "非 extend 移动折叠选区");
        assert_eq!(b.caret(), 0);
    }

    #[test]
    fn tc_cmp_tf_01_word_selection_boundaries() {
        // 字母数字连串为词
        assert_eq!(word_range_at("foo bar", 1), (0, 3));
        assert_eq!(word_range_at("foo bar", 4), (4, 7));
        assert_eq!(word_range_at("foo bar", 6), (4, 7));
        // 标点单字符自成一段(经典编辑器语义)
        assert_eq!(word_range_at("a, b", 1), (1, 2));
        assert_eq!(word_range_at("a == b", 2), (2, 3));
        // 空白连串为一段(被标点隔开的空白是两段)
        assert_eq!(word_range_at("a == b", 4), (4, 5), "单空格自成段");
        assert_eq!(word_range_at("a  b", 1), (1, 3), "双击空白选中空白段");
        // CJK 连串同为词段(is_alphanumeric 口径,无分词词典)
        assert_eq!(word_range_at("图层名", 0), (0, "图层名".len()));
        assert_eq!(word_range_at("图层名", "图层".len()), (0, "图层名".len()));
        // 混排:数字与字母同为词字类
        assert_eq!(word_range_at("v2", 0), (0, 2));
        // 边界:空文本 / 越界下标
        assert_eq!(word_range_at("", 0), (0, 0));
        assert_eq!(word_range_at("ab", 99), (2, 2), "越界钳末尾得空段");
        // 双击选词落位
        let mut b = TextFieldBuffer::new("foo bar");
        b.set_caret(1);
        b.select_word();
        assert_eq!(b.selection(), Some(0..3));
    }

    // —— TC-CMP-TF-01:快捷键矩阵(键盘意图状态机)——

    #[test]
    fn tc_cmp_tf_01_shortcut_key_matrix() {
        let none = Modifiers::default();
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        let ctrl = Modifiers {
            control: true,
            ..Default::default()
        };
        assert_eq!(edit_key("enter", &none, None, false), EditKey::Submit);
        assert_eq!(edit_key("escape", &none, None, false), EditKey::Cancel);
        assert_eq!(edit_key("tab", &none, None, false), EditKey::TabSubmit);
        assert_eq!(
            edit_key("backspace", &none, None, false),
            EditKey::Backspace
        );
        assert_eq!(edit_key("delete", &none, None, false), EditKey::Delete);
        assert_eq!(
            edit_key("left", &none, None, false),
            EditKey::Caret(TextCaretMove::Left, false)
        );
        assert_eq!(
            edit_key("right", &shift, None, false),
            EditKey::Caret(TextCaretMove::Right, true),
            "shift+→ 扩选"
        );
        assert_eq!(
            edit_key("home", &none, None, false),
            EditKey::Caret(TextCaretMove::Home, false)
        );
        assert_eq!(
            edit_key("end", &none, None, false),
            EditKey::Caret(TextCaretMove::End, false)
        );
        assert_eq!(
            edit_key("a", &ctrl, None, false),
            EditKey::SelectAll,
            "Ctrl+A 全选"
        );
        assert_eq!(edit_key("a", &none, Some("a"), false), EditKey::Insert('a'));
        // 修饰组合不落字符(保留宿主快捷键)
        assert_eq!(edit_key("c", &ctrl, Some("c"), false), EditKey::Ignored);
        assert_eq!(edit_key("f", &ctrl, None, false), EditKey::Ignored);
        // 命名键无 key_char:不落字符
        assert_eq!(edit_key("f5", &none, None, false), EditKey::Ignored);
    }

    #[test]
    fn tc_cmp_tf_01_ime_composing_never_submits() {
        let none = Modifiers::default();
        // 组合态:Enter 不得产生 Submit(平台吞键之外的组件侧纵深防御)
        for (key, key_char) in [
            ("enter", None),
            ("a", Some("a")),
            ("space", Some(" ")),
            ("tab", None),
        ] {
            let action = edit_key(key, &none, key_char, true);
            assert_ne!(action, EditKey::Submit, "组合态 {key} 不得映射提交");
            assert_ne!(action, EditKey::Insert(' '), "组合态不得组件侧插字符");
            assert_eq!(action, EditKey::Ignored, "组合态 {key} 应归 IME 所有");
        }
        // 组合态 Esc = 仅取消组合(不动编辑内容)
        assert_eq!(
            edit_key("escape", &none, None, true),
            EditKey::CancelComposition
        );
        // 非组合态 Esc = 取消编辑(还原)
        assert_eq!(edit_key("escape", &none, None, false), EditKey::Cancel);
    }

    // —— TC-CMP-TF-01:IME 组合态数据通路(buffer 层)——

    #[test]
    fn tc_cmp_tf_01_ime_composition_data_path() {
        let mut b = TextFieldBuffer::new("ab");
        b.set_caret(1);
        // 组合开始:在光标处插入组合文本
        b.composition_update("ni", Some(2));
        assert_eq!(b.text(), "anib", "组合文本插入光标处");
        assert!(b.is_composing());
        assert_eq!(b.composition(), Some(1..3));
        // 组合更新:原位替换
        b.composition_update("nihao", None);
        assert_eq!(b.text(), "anihaob");
        assert_eq!(b.composition(), Some(1..6));
        // 组合提交:最终文本替换组合区间
        b.composition_commit("你好");
        assert_eq!(b.text(), "a你好b");
        assert!(!b.is_composing(), "提交后组合区间清空");
        assert_eq!(b.caret(), "a你好".len(), "光标落最终文本之后");
        // 组合取消:文本移除、光标回落
        b.composition_update("shi", None);
        assert_eq!(b.text(), "a你好shib");
        assert!(b.composition_cancel());
        assert_eq!(b.text(), "a你好b");
        assert!(!b.is_composing());
        assert_eq!(b.caret(), "a你好".len());
        // 重复取消无操作
        assert!(!b.composition_cancel());
        // unmark:文本保留
        b.composition_update("de", None);
        b.unmark();
        assert_eq!(b.text(), "a你好deb");
        assert!(!b.is_composing(), "unmark 只清标记");
        // 空组合文本(Windows lparam=0 清组合路径)
        b.composition_update("", None);
        assert!(!b.is_composing() || b.composition().is_some_and(|r| r.is_empty()));
    }

    #[test]
    fn tc_cmp_tf_01_ime_composition_replaces_selection() {
        // 组合开始时替换既有选区(选中文本直接打字的 IME 语义)
        let mut b = TextFieldBuffer::new("hello");
        b.set_caret(1);
        b.begin_selection_drag();
        b.move_caret(TextCaretMove::Right, true);
        assert_eq!(b.selection(), Some(1..2));
        b.composition_update("X", None);
        assert_eq!(b.text(), "hXllo", "组合文本替换选区");
        assert_eq!(b.composition(), Some(1..2));
    }

    #[test]
    fn tc_cmp_tf_01_composition_render_runs_suppress_selection() {
        // 组合段带 Composing 种类(渲染 accent 下划线),组合中不出 Selected
        // ("不高亮候选")
        let mut b = TextFieldBuffer::new("ab");
        b.set_caret(2);
        b.select_all();
        b.composition_update("拼音", None);
        for run in b.render_runs() {
            assert_ne!(
                run.kind,
                TextFieldRunKind::Selected,
                "组合中选区高亮必须让位"
            );
        }
        let kinds: Vec<_> = b.render_runs().into_iter().map(|r| r.kind).collect();
        assert!(
            kinds.contains(&TextFieldRunKind::Composing),
            "组合段存在:{kinds:?}"
        );
        // 非组合:选区段带 Selected
        let mut c = TextFieldBuffer::new("abc");
        c.set_caret(0);
        c.select_all();
        let kinds: Vec<_> = c.render_runs().into_iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            vec![TextFieldRunKind::Selected],
            "全选 = 单 Selected 段"
        );
        // 光标标记:光标在中缝 → 恰两段,a|b 各带一侧光标标记
        let mut d = TextFieldBuffer::new("ab");
        d.set_caret(1);
        let runs = d.render_runs();
        let caret_marks: usize = runs
            .iter()
            .map(|r| usize::from(r.caret_before) + usize::from(r.caret_after))
            .sum();
        assert!(caret_marks >= 1, "光标条有插入位:{runs:?}");
        assert_eq!(runs.len(), 2, "中缝切分:ab → a|b 两段");
        assert!(runs[0].caret_after && runs[1].caret_before);
        // 光标在末尾:不切分,单段带 caret_after
        let e = TextFieldBuffer::new("ab");
        let runs = e.render_runs();
        assert_eq!(runs.len(), 1, "末尾光标不产生切分");
        assert!(runs[0].caret_after && !runs[0].caret_before);
    }

    // —— TC-CMP-TF-01:禁用门控(TOK-07 容器不变仅前景降级)——

    #[test]
    fn tc_cmp_tf_01_disabled_container_bitwise_unchanged() {
        for colors in [ColorTokens::dark(), ColorTokens::light()] {
            for size in [
                TextFieldSize::Compact,
                TextFieldSize::Default,
                TextFieldSize::Roomy,
            ] {
                let idle = text_field_style(size, InteractState::Idle, &colors);
                let dis = text_field_style(size, InteractState::Disabled, &colors);
                assert_eq!(dis.bg, idle.bg, "{size:?} 禁用 bg 逐位不变");
                assert_eq!(dis.border, idle.border, "{size:?} 禁用描边逐位不变");
                assert_eq!(dis.height, idle.height);
                assert_eq!(dis.h_padding, idle.h_padding);
                assert_eq!(dis.radius, idle.radius);
                assert_eq!(dis.text, idle.text);
                // 前景/占位双双降级到 text_disabled
                assert_eq!(
                    dis.fg,
                    disabled_foreground(idle.fg, colors.text_disabled),
                    "禁用正文前景走 disabled_foreground 单点"
                );
                assert_eq!(
                    dis.placeholder,
                    disabled_foreground(idle.placeholder, colors.text_disabled),
                    "禁用占位前景走 disabled_foreground 单点"
                );
                assert_ne!(dis.fg, idle.fg, "禁用前景必须可见降级");
                // 焦点态:描边 = accent,禁用永不 accent
                let ring = text_field_style(size, InteractState::FocusRing, &colors);
                assert_eq!(ring.border, colors.accent);
                assert_ne!(dis.border, colors.accent, "禁用不得出现焦点描边");
            }
        }
    }

    #[test]
    fn tc_cmp_tf_01_hover_press_visible_and_derived_height() {
        for colors in [ColorTokens::dark(), ColorTokens::light()] {
            for size in [
                TextFieldSize::Compact,
                TextFieldSize::Default,
                TextFieldSize::Roomy,
            ] {
                let idle = text_field_style(size, InteractState::Idle, &colors);
                let hover = text_field_style(size, InteractState::Hover, &colors);
                let press = text_field_style(size, InteractState::Pressed, &colors);
                // state-layer 极性:按底色亮度取向(浅色 surface 叠黑压暗)
                if idle.bg.l >= 0.5 {
                    assert!(hover.bg.l < idle.bg.l, "{size:?} 浅底 hover 压暗");
                } else {
                    assert!(hover.bg.l > idle.bg.l, "{size:?} 深底 hover 提亮");
                }
                assert_ne!(press.bg, hover.bg, "press 区别于 hover");
                // hover/press 描边提到 border_strong
                assert_eq!(hover.border, colors.border_strong);
                // 占位符弱于正文(text_placeholder 语义)
                assert_ne!(idle.placeholder, idle.fg);
            }
        }
        // 高度与 Button 尺寸档逐档相等(构造性对齐:同一派生函数)
        for (tf, btn) in [
            (TextFieldSize::Compact, ButtonSize::Compact),
            (TextFieldSize::Default, ButtonSize::Default),
            (TextFieldSize::Roomy, ButtonSize::Roomy),
        ] {
            assert_eq!(
                text_field_height(tf),
                crate::controls::button::button_height(
                    btn,
                    crate::controls::button::button_text(btn)
                ),
                "{tf:?} 高度必须与 {btn:?} 同源派生"
            );
            assert_eq!(
                text_field_text(tf),
                crate::controls::button::button_text(btn)
            );
        }
        assert_eq!(
            text_field_height(TextFieldSize::Default),
            26.0,
            "默认档下限 26"
        );
    }

    // —— TC-CMP-TF-01:光标闪烁(含 reduced_motion 直通)——

    #[test]
    fn tc_cmp_tf_01_caret_blink_phase_and_reduced_motion() {
        // 1s 周期前半亮
        assert!(caret_visible(T0, T0, false), "相位起点即亮");
        assert!(caret_visible(T0, T0 + 499.0, false));
        assert!(!caret_visible(T0, T0 + 500.0, false), "后半熄灭");
        assert!(caret_visible(T0, T0 + 1000.0, false), "进入下一周期");
        // 相位重置:任意光标移动从亮起算
        assert!(!caret_visible(T0, T0 + 800.0, false), "800ms 熄灭中");
        assert!(caret_visible(T0 + 800.0, T0 + 800.0, false), "重置即亮");
        // 时钟回拨防御:按 0 处理(亮)
        assert!(caret_visible(T0, T0 - 100.0, false));
        // reduced_motion:直通常亮
        assert!(caret_visible(T0, T0 + 700.0, true), "减弱动态常亮");
    }

    // —— TC-CMP-TF-01:UTF-16 ↔ 字节映射(IME 协议口径)——

    #[test]
    fn tc_cmp_tf_01_utf16_byte_mapping_round_trip_with_cjk() {
        let text = "12厘米e";
        let byte_of_cjk = "12".len();
        // CJK(BMP):utf16 每字 1 单位、utf8 每字 3 字节
        assert_eq!(byte_to_utf16(text, byte_of_cjk), 2);
        assert_eq!(utf16_to_byte(text, 2), Some(byte_of_cjk));
        assert_eq!(utf16_to_byte(text, 3), Some(byte_of_cjk + 3));
        assert_eq!(utf16_to_byte(text, 99), None, "越界 None");
        assert_eq!(byte_to_utf16(text, text.len()), 5);
        // 代理对内部吸附到后边界
        let astral = "1🎉";
        assert_eq!(utf16_to_byte(astral, 2), Some(astral.len()));
        assert_eq!(utf16_to_byte(astral, 3), Some(astral.len()));
    }

    // —— TC-CMP-TF-01:builder → 字段链路(受控 API 形态)——

    #[test]
    fn tc_cmp_tf_01_builder_shape_and_defaults() {
        let field = TextField::new("name", "初始");
        assert_eq!(field.text(), "初始");
        assert_eq!(field.size, TextFieldSize::Default);
        assert!(!field.disabled && !field.read_only);
        assert!(field.placeholder.is_none());
        assert!(field.on_submit.is_none() && field.on_cancel.is_none());
        let field = field
            .placeholder("输入…")
            .size(TextFieldSize::Roomy)
            .disabled(true)
            .read_only(true)
            .on_submit(|_text, _win, _cx| {})
            .on_cancel(|_win, _cx| {});
        assert_eq!(field.placeholder, Some(SharedString::from("输入…")));
        assert_eq!(field.size, TextFieldSize::Roomy);
        assert!(field.disabled && field.read_only);
        assert!(field.on_submit.is_some() && field.on_cancel.is_some());
        // set_text 语义(纯字段级,无 App):buffer 与提交快照一致
        let mut field = TextField::new("n", "old");
        field.buffer.set_text("draft");
        assert_ne!(field.buffer.text(), field.committed, "编辑中快照滞后");
        field.committed = field.buffer.text().to_string();
        assert_eq!(field.committed, "draft");
    }

    #[test]
    fn tc_cmp_tf_01_submit_and_cancel_revert_semantics() {
        // 提交:快照推进(Esc 还原锚)
        let mut field = TextField::new("n", "v1");
        field.buffer.set_text("v2");
        field.committed = field.buffer.text().to_string();
        assert_eq!(field.committed, "v2");
        // 取消:buffer 还原到快照
        field.buffer.set_text("草稿");
        assert_eq!(field.buffer.text(), "草稿");
        if field.buffer.text() != field.committed {
            field.buffer.set_text(&field.committed);
        }
        assert_eq!(field.buffer.text(), "v2", "Esc 还原到最近提交");
        // 快照与 buffer 一致时取消无变化
        assert_eq!(field.committed, field.buffer.text());
    }

    #[test]
    fn typed_char_prefers_key_char_and_falls_back() {
        assert_eq!(typed_char("a", Some("A")), Some('A'), "key_char 优先");
        assert_eq!(typed_char("2", Some("•")), Some('•'), "shift 布局真实字符");
        assert_eq!(typed_char("a", None), Some('a'), "回退单字符 key");
        assert_eq!(typed_char("enter", None), None, "命名键不落字符");
        assert_eq!(
            typed_char("x", Some("")),
            Some('x'),
            "空 key_char 回退单字符 key"
        );
    }
}
