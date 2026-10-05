//! 数值拖拽框 NumberField(分册四 §6,设计软件的灵魂小件)。
//!
//! # v1.0 交互矩阵(08 迭代计划 S3 3.3;分册四 §6 验收表逐行对应)
//!
//! | 输入 | 行为 | 状态 |
//! |---|---|---|
//! | 按住左右拖 | `start + dx × step × 修饰键倍率`,钳制 range | ✅ 完整 |
//! | Shift 拖 | ×10 | ✅ |
//! | Alt 拖 | ×0.1(**Alt 优先**,上游 NumField 同款) | ✅ |
//! | 滚轮 | ±1 步进(Shift ×10) | ✅ |
//! | ↑/↓ 键 | ±1 步进(Shift ×10);需焦点(元素恒 `track_focus`,点击/Tab 即得焦点) | ✅ |
//! | 双击 / Enter | 进入文本编辑态(**真编辑**,v1.0 升级:双击"简化展示"已移除) | ✅ |
//! | 编辑态键入 | 字符 0-9 `.` `e` `E` `+` `-` 插入 buffer | ✅ |
//! | Backspace / Delete | 删光标前/后字符 | ✅ |
//! | ←/→ / Home / End | 移动光标(选择区间 = M2,`EditBuffer` 已留扩展位) | ✅ |
//! | Enter | 提交(parse f64 → 钳制 range → [`Binding::set`] **一次**) | ✅ |
//! | Esc | 取消,回落旧值(不产生命令) | ✅ |
//! | Tab | 提交 + `window.focus_next()` 跳下一字段(gpui 0.2.2 已核实存在
//!   `Window::focus_next`,按渲染序的 tab stop 环游) | ✅ |
//! | 失焦(点击别处) | 渲染帧检测 `editing && !focused` → 自动提交(与 Enter 同路) | ✅ |
//! | IME | [`EntityInputHandler`] 实现挂 `track_focus` 元素,`Window::handle_input`
//!   在 paint 阶段注册(见下方"IME 钩子");文本事件经
//!   [`crate::input_method::InputMethodAdapter`] 单点路由(A11Y-08) | ✅ 接口全通(组合语义 = M2) |
//!
//! # 禁用态(TOK-07,报告 §5.9)
//!
//! `.disabled(true)` 门控上表**全部**交互路径(拖拽/滚轮/键盘/双击编辑/
//! 悬停动画);视觉走 [`disabled_visual`]:**仅前景降级**(`text_disabled`),
//! 容器背景/描边与静止态逐位相同——不整体降饱和、不变色(规则与门禁
//! TC-TOK-DISABLED-01 见 [`crate::interact`] 模块 doc)。
//!
//! # 编辑态状态机
//!
//! ```text
//! Display ──双击/Enter/键入数字──▶ Editing { buffer, caret } ──Enter/Tab/失焦──▶ Display(提交)
//!                                     │──────Esc───────────────▶ Display(回落旧值)
//! ```
//!
//! 编辑态渲染 = buffer 分段着色(光标前文本 + 1px 光标条 + 光标后文本),
//! 走 flex 布局天然定位,不做字形测量(gpui 0.2.2 下最稳方案)。
//!
//! # 撤销与节流
//!
//! **提交一次 = [`Binding::set`] 一次**(编辑中的每个 keystroke 只改本地
//! buffer,不产生命令);拖拽/滚轮/步进仍逐次 set,由调用方 set 闭包里的
//! 命令 `merge` 语义兜底(分册三 §2 拖动范式)。widgets 层不做 16ms 节流,
//! 值未变化时跳过 set(PartialEq 短路)。
//!
//! # IME 钩子(2026-10 gpui 0.2.2 源码核实;A11Y-08 第 4 组收口)
//!
//! gpui 0.2.2 **有** `InputHandler` 平台文本输入 trait(`src/platform.rs:995`)
//! 与视图侧 `EntityInputHandler`(`src/input.rs`)+ paint 阶段注册点
//! `Window::handle_input`(`src/window.rs:3403`,内部断言 Paint 阶段——
//! 故经 `canvas()` 元素的 paint 闭包挂载,而非 render/prepaint)。本组件
//! 实现全部 8 个方法,文本事件**全部经
//! [`crate::input_method::InputMethodAdapter`] 单点路由**——UTF-16 ↔ 字节
//! 换算、区间归一、组合/提交分支与 TextField 同源(该模块 doc),本文件
//! 不再持有第二份(旧私有 `utf16_to_byte`/`byte_to_utf16` 副本已删)。
//!
//! **真实边界(如实,不虚标)**:编辑器 v0.1 无选区、**组合(marked)区间
//! 不追踪**——组合文本直插 buffer(Adapter 的 `ime_composition_update`
//! 落 [`EditBuffer::insert`]),组合取消/组合态高亮在 NumberField 上
//! **不可用**;`marked_text_range` 恒 `None`。中文输入法的候选/提交链路 =
//! TC-A11Y-IME-01 **真机走查**(走查表 `docs/a11y-notes.md §5`,全部 ☐
//! 待真机);完整组合语义随 NumberField 编辑器重构(M2)接入。
//!
//! # 焦点系统(gpui 0.2.2 源码核实)
//!
//! `FocusHandle`(`cx.focus_handle()`)、`InteractiveElement::track_focus`、
//! `Window::focus`/`focus_next` 均存在;track_focus 元素被点击时自动接管
//! 焦点(`elements/div.rs` paint 期的 mousedown 派发)。构造函数无 cx
//! (受 [`crate::inspector`] 调用形态约束),焦点句柄在首帧 render 惰性
//! 创建并以 `tab_stop(true)` 进 Tab 环游序。
//!
//! # A7 微交互(hover/press 三态,分册六 §4.3 #1 + TOK-04 state-layer)
//!
//! 底色 = `surface_2` 凸起材质(N5)上做状态层混合:hover =
//! [`state_layer`](crate::interact::state_layer)(深色叠白 6%/浅色叠黑 4%,
//! 120ms ease-out,由 [`HoverState`](crate::interact::HoverState) 驱动),
//! 按下(scrub 拖拽中)= `InteractState::Pressed`。同屏多个实例时必须
//! 经 [`.element_id`](Self::element_id) 给唯一 id;未给 id 时退化为 gpui
//! hover 样式即时切换(无动画,不破缺省构造)。减弱动态(A8)下插值被
//! [`reduced_motion`](crate::anim::reduced_motion) 短路,进度直通 0/1;
//! 编辑态光标为常亮竖线(不闪烁),与 A8 无关。
//!
//! # 海拔材质(TOK-01,报告 §5.3.2)
//!
//! 输入是"L2 凸起"材质:渲染时经 [`theme::elevated`](crate::theme::elevated)
//! 垫 [`ELEVATIONS[2]`](crate::tokens::ELEVATIONS) 的 quad 近似阴影
//! (GPUI 0.2.2 无 box-shadow)。内容层保持原事件语义(监听器全部在外层
//! 容器上)。

use gpui::{
    Context, ElementId, EntityInputHandler, Hsla, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Render, ScrollWheelEvent,
    SharedString, StatefulInteractiveElement, Styled, Window, canvas, px,
};

use crate::anim::lerp_hsla;
use crate::binding::Binding;
use crate::input_method::{
    ImeEditTarget, ImeEvent, InputMethodAdapter, UTF16_ADAPTER, byte_to_utf16,
};
use crate::interact::{self, HoverState, Semantic, SemanticRole, semantic_slot};
use crate::theme::theme;
use crate::tokens::{
    ColorTokens, HEIGHT_COMPACT, MONO_FONT, RadiusTokens, SpacingTokens, TextSize, control_height,
    h_flex,
};

/// 控内布局估行高(12px 等宽数值的紧凑估;高度派生用下限,与
/// [`TextSize::MONO`] 的排版行高 18 无冲突——控件高度走 [`control_height`])。
const LINE_HEIGHT_PX: f32 = 14.0;
/// 控件的垂直内边距。
const V_PADDING_PX: f32 = 4.0;
/// 滚轮一行折算像素(与 sable-canvas 同款惯例)。
const SCROLL_LINE_PX: f32 = 24.0;
/// 编辑态光标条宽(1px 竖线,token 取 accent 色)。
const CARET_WIDTH_PX: f32 = 1.0;

/// 数值显示字体族(mono 族令牌,TOK-02 等宽数字):展示/编辑两态的所有
/// 文本都经 render 挂此族——等宽字形天然满足 tabular-nums 语义,拖动时
/// 字符不逐个跳动。TC-TOK-TYPE-02 的断言点(渲染路径与令牌单点绑定)。
#[must_use]
pub fn value_font_family() -> &'static str {
    MONO_FONT
}

/// 数值框的禁用态视觉(TOK-07):容器不变、仅前景降级。
///
/// 三色全部单一源自 [`ColorTokens`](底 = `surface_2`、描边 = `border_subtle`
/// ——与**静止态**容器逐位同源;前景经
/// [`interact::disabled_foreground`](crate::interact::disabled_foreground)
/// 收敛 `text_disabled`),渲染与门禁(TC-TOK-DISABLED-01)共用本结构,
/// 不许组件另调灰。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisabledVisual {
    /// 容器底(= 静止态 `surface_2`,逐位不变)
    pub bg: Hsla,
    /// 容器描边(= 静止态 `border_subtle`,逐位不变)
    pub border: Hsla,
    /// 禁用前景(= `text_disabled`)
    pub fg: Hsla,
}

/// 禁用态视觉裁决(纯函数,TOK-07 规则在 NumberField 的落地):禁用 =
/// 仅前景降级,容器背景/描边与静止态**逐位相同**(不整体降饱和/变色)。
/// render 的 `disabled` 分支与 TC-TOK-DISABLED-01 共用本单点。
#[must_use]
pub fn disabled_visual(colors: &ColorTokens) -> DisabledVisual {
    DisabledVisual {
        bg: colors.surface_2,
        border: colors.border_subtle,
        fg: interact::disabled_foreground(colors.text_secondary, colors.text_disabled),
    }
}

/// 数值框(有状态 Entity):`cx.new(|_| NumberField::new(binding).range(0.0, 100.0))`。
pub struct NumberField {
    binding: Binding<f64>,
    range: (f64, f64),
    step: f64,
    unit: &'static str,
    /// 拖拽中:窗口 x 起点 + 起始值
    drag: Option<ScrubDrag>,
    /// 文本编辑态(v1.0 真编辑:buffer + 光标;`None` = 展示态)
    editing: Option<EditBuffer>,
    /// 焦点句柄(首帧惰性创建,`tab_stop(true)` 进 Tab 环游;见模块 doc)
    focus: Option<gpui::FocusHandle>,
    /// A7 悬停进度(120ms ease-out;拖拽 = pressed 态,复用 ScrubDrag 判定)
    hover: HoverState,
    /// 悬停事件跟踪用的元素 id(`on_hover` 需要 Stateful 元素;同屏多实例
    /// 必须各给唯一 id,未给则退化为即时 hover 样式)
    element_id: Option<ElementId>,
    /// 禁用态(TOK-07):真 = 交互全门控(拖拽/滚轮/键盘/悬停动画)且视觉
    /// 走 [`disabled_visual`](仅前景降级,容器不变)
    disabled: bool,
    /// A11Y-02 语义槽(可访问名;role 默认 TextField——编辑态即文本输入)
    semantic: Semantic,
    /// PERF-01 展示文本缓存:`(value.to_bits(), 文本)`。值未变的帧(悬停/
    /// 按压动画帧、外部状态触发的重绘帧)零 `format!` 零堆分配;值变化时
    /// 恰一次分配。经 [`cached_display`] 纯函数操作。
    display_cache: Option<(u64, SharedString)>,
}

/// PERF-01:展示文本(值 + 单位)缓存纯函数。命中即 clone(`SharedString`
/// 为 Arc 计数,clone 是引用计数自增,不分配);未命中恰格式化一次。
/// 键用 `to_bits()`:NaN 载荷与 ±0.0 各自区分,宁可多格式化一次也不误用
/// 旧文本(0.0 与 -0.0 文本相同,多算一次无碍)。
fn cached_display(
    cache: &mut Option<(u64, SharedString)>,
    value: f64,
    unit: &'static str,
) -> SharedString {
    let bits = value.to_bits();
    if let Some((cached_bits, text)) = cache {
        if *cached_bits == bits {
            return text.clone();
        }
    }
    let text = SharedString::from(format!("{}{}", format_value(value), unit));
    *cache = Some((bits, text.clone()));
    text
}

#[derive(Clone, Copy, Debug)]
struct ScrubDrag {
    start_x: f64,
    start_val: f64,
}

/// 文本编辑态缓冲:`caret` 是 **char 边界上的字节偏移**(数值输入全 ASCII,
/// IME 插入的 CJK 按整字符推进,不变量恒成立)。
///
/// 纯数据 + 纯函数操作,单测覆盖插入/删除/光标边界(无 App 依赖)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditBuffer {
    text: String,
    caret: usize,
}

impl EditBuffer {
    /// 从展示值构造(光标在末尾)。
    pub fn new(initial: &str) -> Self {
        EditBuffer {
            caret: initial.len(),
            text: initial.to_string(),
        }
    }

    /// 光标前插入文本,光标推进 `text.len()`(调用方保证 caret 在边界上;
    /// 本类型全部公开操作恒维持该不变量)。
    pub fn insert(&mut self, text: &str) {
        self.text.insert_str(self.caret, text);
        self.caret += text.len();
    }

    /// 删除光标前一个字符;光标在起点时无操作。返回是否发生删除。
    pub fn backspace(&mut self) -> bool {
        if self.caret == 0 {
            return false;
        }
        let prev = self.text[..self.caret]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or_default();
        self.text.replace_range(prev..self.caret, "");
        self.caret = prev;
        true
    }

    /// 删除光标后一个字符;光标在终点时无操作。返回是否发生删除。
    pub fn delete(&mut self) -> bool {
        if self.caret >= self.text.len() {
            return false;
        }
        let end = self.text[self.caret..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or_default();
        self.text.replace_range(self.caret..self.caret + end, "");
        true
    }

    /// 光标移动(纯函数语义:越界钳到 0/len)。返回位置是否变化。
    pub fn move_caret(&mut self, direction: CaretMove) -> bool {
        let next = match direction {
            CaretMove::Left => self.text[..self.caret]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0),
            CaretMove::Right => self.text[self.caret..]
                .chars()
                .next()
                .map(char::len_utf8)
                .map(|n| self.caret + n)
                .unwrap_or(self.caret),
            CaretMove::Home => 0,
            CaretMove::End => self.text.len(),
        };
        if next == self.caret {
            return false;
        }
        self.caret = next;
        true
    }

    /// 区间替换(IME `replace_text_in_range` 路径):start/end 一律**向下取**
    /// char 边界(平台发来撕裂的 UTF-16 下标时只删整字符,宁少勿多),
    /// 替换后光标落在插入文本之后。
    pub fn replace_range(&mut self, range: std::ops::Range<usize>, text: &str) {
        let start = self.boundary_floor(range.start.min(self.text.len()));
        let end = self.boundary_floor(range.end.min(self.text.len()));
        if start > end {
            return; // 防御:非法区间原样保留
        }
        self.text.replace_range(start..end, text);
        self.caret = start + text.len();
    }

    /// 直接移动光标(IME 提交后的定位;越界钳到 [0, len] 的 char 边界)。
    pub fn set_caret(&mut self, byte_index: usize) {
        self.caret = self.boundary_floor(byte_index.min(self.text.len()));
    }

    /// ≤ index 的最近 char 边界(IME/平台下标防御)。
    fn boundary_floor(&self, index: usize) -> usize {
        let mut i = index.min(self.text.len());
        while i > 0 && !self.text.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    /// 光标两侧文本(渲染分段:左段 + 光标条 + 右段)。
    pub fn segments(&self) -> (&str, &str) {
        (&self.text[..self.caret], &self.text[self.caret..])
    }

    /// 光标位置(字节偏移,char 边界)。
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// 缓冲文本。
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// 光标移动方向(编辑态 ←/→/Home/End;shift 选择区间 = M2)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaretMove {
    /// 左移一个字符(起点钳 0)。
    Left,
    /// 右移一个字符(终点钳 len)。
    Right,
    /// 行首。
    Home,
    /// 行尾。
    End,
}

/// 编辑字符白名单:数字/小数点/科学计数 e/正负号(任务 3.3 #2)。
pub fn is_editable_char(c: char) -> bool {
    c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')
}

/// 提交结果(纯函数 [`commit_value`] 的输出;便于无 App 单测"提交一次命令"
/// 的语义——`Value` 才走 [`Binding::set`],`Invalid` 回落旧值)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CommitOutcome {
    /// 合法数值(已钳制到 range;调用方执行一次 set)。
    Value(f64),
    /// 非数值(空串/非法串):回落旧值,不产生命令。
    Invalid,
}

/// 提交校验(纯函数):trim 后 parse f64;合法值钳制到 range。
pub fn commit_value(text: &str, range: (f64, f64)) -> CommitOutcome {
    match text.trim().parse::<f64>() {
        Ok(v) if v.is_finite() => CommitOutcome::Value(clamp_range(v, range)),
        _ => CommitOutcome::Invalid,
    }
}

impl NumberField {
    /// 绑定驱动的数值框;默认范围 0..=100、步长 1、无单位。
    pub fn new(binding: Binding<f64>) -> Self {
        NumberField {
            binding,
            range: (0.0, 100.0),
            step: 1.0,
            unit: "",
            drag: None,
            editing: None,
            focus: None,
            hover: HoverState::new(),
            element_id: None,
            disabled: false,
            semantic: Semantic::new(),
            display_cache: None,
        }
    }

    /// PERF-02:重定向绑定(检查器实体池复用)。焦点句柄保留(修复规格
    /// 每帧重建导致编辑态/焦点丢失的旧病);进行中的拖拽与编辑缓冲取消
    /// (绑定目标可能已换);展示文本缓存失效(下次渲染按新值重建)。
    pub(crate) fn rebind(&mut self, binding: Binding<f64>) {
        self.binding = binding;
        self.drag = None;
        self.editing = None;
        self.display_cache = None;
    }

    /// 禁用态(TOK-07):禁用即交互全门控(点击拖拽/双击编辑/滚轮/键盘/
    /// 悬停动画),视觉经 [`disabled_visual`]——**仅前景降级**
    /// (`text_disabled`),容器背景/描边与静止态逐位相同。builder 风格,
    /// 与 [`Self::range`]/[`Self::step`] 同链。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 元素 id(悬停动画需要 Stateful 元素;同屏多个数值框各给唯一 id,
    /// 如 `ElementId::named_usize("inspector-num", ix)`)。
    pub fn element_id(mut self, id: impl Into<ElementId>) -> Self {
        self.element_id = Some(id.into());
        self
    }

    /// 取值范围(拖拽/步进钳制)。
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.range = (min, max);
        self
    }

    /// 基础步长(拖拽 1px 的值变化;Shift ×10 / Alt ×0.1 另算)。
    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// 单位后缀("px" / "°" / "%")。
    pub fn unit(mut self, unit: &'static str) -> Self {
        self.unit = unit;
        self
    }

    /// 实际控件高度(派生制):max(22, 14 + 2×4) = 22。
    pub fn control_height() -> f32 {
        control_height(HEIGHT_COMPACT, LINE_HEIGHT_PX, V_PADDING_PX)
    }

    /// 进入编辑态:以当前值展示形式播 buffer,光标在末尾,并接管焦点。
    fn begin_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.binding.get(cx);
        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        self.editing = Some(EditBuffer::new(&format_value(value)));
        focus.focus(window);
        cx.notify();
    }

    /// 提交:合法 → `Binding::set` **一次**;非法 → 回落旧值(无命令)。
    /// Enter / Tab / 失焦三路共用(任务 3.3 #3/#4)。
    fn commit(&mut self, cx: &mut Context<Self>) {
        let Some(buffer) = self.editing.take() else {
            return;
        };
        if let CommitOutcome::Value(v) = commit_value(buffer.text(), self.range) {
            if v != self.binding.get(cx) {
                // 提交一次 = 命令一次(编辑中 keystroke 不产生命令,模块 doc)
                self.binding.set(v, cx);
            }
        }
        cx.notify();
    }

    /// 取消(Esc):丢弃 buffer,回落旧值,不产生命令。
    fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.editing.take().is_some() {
            cx.notify();
        }
    }

    // —— 事件(gpui 事件坐标一律窗口坐标,dx 与 bounds 无关)——

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return; // TOK-07:禁用即交互门控(不拖拽/不进编辑)
        }
        if event.button != MouseButton::Left {
            return;
        }
        // 编辑态禁用拖拽(任务 3.3 #7);双击进入编辑态(真编辑,v1.0)
        if self.editing.is_some() {
            return;
        }
        if event.click_count >= 2 {
            self.begin_editing(window, cx);
            return;
        }
        // gpui 0.2.2 的 Pixels 字段 crate 私有(已核实),公开通道是 From<Pixels> for f64
        self.drag = Some(ScrubDrag {
            start_x: f64::from(event.position.x),
            start_val: self.binding.get(cx),
        });
        cx.notify();
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return; // TOK-07:禁用即交互门控(拖拽不推进)
        }
        if self.editing.is_some() {
            return; // 编辑态时拖拽禁用(任务 3.3 #7)
        }
        let Some(drag) = self.drag else { return };
        if event.pressed_button != Some(MouseButton::Left) {
            return; // 拖出元素后松键的场景由 on_mouse_up_out 兜底(M2)
        }
        let scale = modifier_scale(event.modifiers.alt, event.modifiers.shift);
        let dx = f64::from(event.position.x) - drag.start_x;
        let next = scrub(drag.start_val, dx, self.step, scale, self.range);
        if next != self.binding.get(cx) {
            // 每次 set 由调用方闭包的 merge 语义合并为一步撤销(模块 doc)
            self.binding.set(next, cx);
            cx.notify();
        }
    }

    fn on_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.drag = None;
        // 不 notify:值未变时无需重绘(editing 已在 down 时 notify)
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return; // TOK-07:禁用即交互门控(滚轮不步进)
        }
        if self.editing.is_some() {
            return; // 编辑态滚轮不步进(文本正在编辑,语义冲突)
        }
        let dy = f64::from(event.delta.pixel_delta(px(SCROLL_LINE_PX)).y);
        if dy == 0.0 {
            return;
        }
        let scale = modifier_scale(false, event.modifiers.shift);
        let current = self.binding.get(cx);
        // 滚轮向下 = 减(与面板惯例一致)
        let next = scrub(current, -dy.signum(), self.step, scale, self.range);
        if next != current {
            self.binding.set(next, cx);
            cx.notify();
        }
    }

    fn on_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return; // TOK-07:禁用即交互门控(步进/编辑/键入全关)
        }
        let keystroke = &event.keystroke;
        // —— 编辑态:控制键先行(提交/取消/跳格),再进 buffer 状态机
        // (先分流控制键,避免与 `self.editing.as_mut()` 的借用交叠)——
        if self.editing.is_some() {
            match keystroke.key.as_str() {
                "enter" => {
                    self.commit(cx);
                    return;
                }
                "escape" => {
                    self.cancel(cx); // 取消回旧值,不产生命令
                    return;
                }
                "tab" => {
                    // 提交 + Tab 跳下一字段(gpui 0.2.2 的 Window::focus_next,
                    // 按渲染序在 tab stop 间环游——已核实存在)
                    self.commit(cx);
                    window.focus_next();
                    return;
                }
                _ => {}
            }
            let Some(buffer) = self.editing.as_mut() else {
                return;
            };
            match keystroke.key.as_str() {
                "backspace" => {
                    buffer.backspace();
                    cx.notify();
                }
                "delete" => {
                    buffer.delete();
                    cx.notify();
                }
                "left" => {
                    buffer.move_caret(CaretMove::Left);
                    cx.notify();
                }
                "right" => {
                    buffer.move_caret(CaretMove::Right);
                    cx.notify();
                }
                "home" => {
                    buffer.move_caret(CaretMove::Home);
                    cx.notify();
                }
                "end" => {
                    buffer.move_caret(CaretMove::End);
                    cx.notify();
                }
                _ => {
                    // 字符输入:优先平台给出的实际键入字符(IME/非美式布局),
                    // 回退单字符 key;经白名单过滤(任务 3.3 #2)
                    let ch = keystroke
                        .key_char
                        .as_deref()
                        .filter(|s| s.chars().count() == 1)
                        .and_then(|s| s.chars().next())
                        .or_else(|| single_char(keystroke.key.as_str()));
                    if let Some(ch) = ch.filter(|c| is_editable_char(*c)) {
                        buffer.insert(&ch.to_string());
                        cx.notify();
                    }
                }
            }
            return;
        }

        // —— 展示态:步进 / 进入编辑 ——
        match keystroke.key.as_str() {
            "up" | "down" => {
                let dir: f64 = if keystroke.key == "up" { 1.0 } else { -1.0 };
                let scale = modifier_scale(false, keystroke.modifiers.shift);
                let current = self.binding.get(cx);
                let next = step_by(current, dir, self.step, scale, self.range);
                if next != current {
                    self.binding.set(next, cx);
                    cx.notify();
                }
            }
            "enter" => {
                self.begin_editing(window, cx);
            }
            _ => {
                // 聚焦状态下直接键入数字/负号等:立即进入编辑态并带入该字符
                if let Some(ch) = single_char(keystroke.key.as_str()).filter(|c| {
                    is_editable_char(*c) && !keystroke.modifiers.control && !keystroke.modifiers.alt
                }) {
                    self.begin_editing(window, cx);
                    if let Some(buffer) = self.editing.as_mut() {
                        *buffer = EditBuffer::new(&ch.to_string());
                    }
                    cx.notify();
                }
            }
        }
    }

    /// A7 悬停进出:驱动 [`HoverState`] 过渡并请求重绘(动画帧由 render 里
    /// 的 `request_animation_frame` 续)。禁用态忽略(TOK-07:容器不变,
    /// 无悬停反馈;进行中的过渡由 render 停泵后自然沉降)。
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

    /// 失焦自动提交:渲染帧检测"编辑中但焦点已走"(点击别处/切窗口),
    /// 与 Enter 同一条提交路径(任务书"Enter=提交"的失焦等价语义)。
    fn commit_if_blurred(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self
            .focus
            .as_ref()
            .is_some_and(|handle| handle.is_focused(window));
        if self.editing.is_some() && !focused {
            self.commit(cx);
        }
    }
}

// A11Y-02 语义槽(label/role/semantic 三件;role 默认 TextField——编辑态
// 即文本输入语义)。
semantic_slot!(NumberField);

impl NumberField {
    /// 解析语义(A11Y-02):显式 `.label(...)` 优先;role 默认 TextField。
    #[must_use]
    pub fn resolved_semantic(&self) -> Semantic {
        let sem = Semantic::new();
        let sem = match self.semantic.label() {
            Some(text) => sem.with_label(text.clone()),
            None => sem,
        };
        sem.with_role(self.semantic.role().unwrap_or(SemanticRole::TextField))
    }
}

impl Render for NumberField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.commit_if_blurred(window, cx);

        let colors = &theme(cx).colors;
        let editing = self.editing.clone();
        let dragging = self.drag.is_some();
        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();

        // 展示文本:编辑态 = buffer 分段;展示态 = 值 + 单位
        // TOK-02 接线:等宽族 + TextSize::MONO 三值(M2 的"字体 token 化"已落)
        let display = match &editing {
            Some(buffer) => {
                let (left, right) = buffer.segments();
                // 分段着色 + 1px 光标条(flex 流式定位,不做字形测量)
                h_flex()
                    .child(gpui::div().child(SharedString::from(left.to_string())))
                    .child(
                        gpui::div()
                            .w(px(CARET_WIDTH_PX))
                            .h(px(LINE_HEIGHT_PX))
                            .bg(colors.accent),
                    )
                    .child(gpui::div().child(SharedString::from(right.to_string())))
                    .into_any_element()
            }
            None => {
                let value = self.binding.get(cx);
                let text = cached_display(&mut self.display_cache, value, self.unit);
                gpui::div().child(text).into_any_element()
            }
        };

        // A7/TOK-04 三态底色:静止/悬停 = surface_2 → state_layer(Hover)
        // 插值(深色叠白/浅色叠黑);按下 = state_layer(Pressed)。
        // TOK-07 禁用分支:容器(bg/描边)= 静止态**逐位不变**,仅前景经
        // [`disabled_visual`] 降级到 text_disabled(不整体降饱和,不响应
        // hover/press)。
        let disabled = self.disabled;
        let base_bg = colors.surface_2;
        let accent = colors.accent;
        let now = interact::now_ms();
        let hover_progress = if disabled {
            0.0
        } else {
            self.hover.progress_at(now)
        };
        let (bg, border, fg) = if disabled {
            let v = disabled_visual(colors);
            (v.bg, v.border, v.fg)
        } else if dragging || editing.is_some() {
            (
                interact::state_layer(base_bg, interact::InteractState::Pressed, accent),
                colors.border_strong,
                colors.text_primary,
            )
        } else {
            (
                lerp_hsla(
                    base_bg,
                    interact::state_layer(base_bg, interact::InteractState::Hover, accent),
                    hover_progress,
                ),
                colors.border_subtle,
                colors.text_secondary,
            )
        };

        // IME 钩子:paint 阶段注册平台输入处理器(Window::handle_input 断言
        // Paint 阶段,故经 canvas() 的 paint 闭包挂载;见模块 doc)
        let ime_focus = focus.clone();
        let ime_entity = cx.entity();

        // 内容层:视觉样式(底/描边/文字)+ 显示文本 + IME 钩子 canvas
        let mut inner = h_flex()
            .justify_center()
            .w_full()
            .h(px(Self::control_height()))
            .px(px(SpacingTokens::SM))
            .rounded(px(RadiusTokens::SM))
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_size(px(TextSize::MONO.size))
            .font_family(value_font_family())
            .font_weight(gpui::FontWeight(TextSize::MONO.weight))
            .text_color(fg)
            .cursor_pointer()
            .child(display)
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        // focus 已聚焦时注册输入处理器(handle_input 内部同样
                        // 有 is_focused 断言,双保险;实体消亡后闭包不再被装帧)
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
        // 无 id:退化为 gpui hover 样式即时切换(无动画)。hover 底色必须挂
        // 内容层(阴影容器 bg 会被 quad 与内容盖住,挂上去等于没有反馈)。
        // 禁用态不挂 hover 样式(TOK-07:容器不变,无交互反馈)。
        if !disabled && self.element_id.is_none() {
            let hover_bg = interact::state_layer(base_bg, interact::InteractState::Hover, accent);
            inner = inner.hover(move |style| style.bg(hover_bg));
        }

        // TOK-01 消费点 1:输入凸起 = L2 海拔(theme::elevated 内部取
        // tokens::ELEVATIONS[2] 的 quad 近似阴影;不透明底盖住重叠区)。
        // 事件语义不变:id/焦点/监听器全部挂在外层容器。
        let root = crate::theme::elevated(2, inner)
            .min_w_0()
            .track_focus(&focus)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .on_key_down(cx.listener(Self::on_key_down));

        // 悬停动画在跑就续帧(静止零帧提交,分册六 §4.4);禁用态悬停进度
        // 冻结(渲染已忽略),不再续帧
        if !disabled && self.hover.is_running(now) {
            window.request_animation_frame();
        }
        match self.element_id.clone() {
            // 有 id:on_hover 驱动 120ms 插值(见模块 doc)
            Some(id) => root
                .id(id)
                .on_hover(cx.listener(Self::on_hover_changed))
                .into_any_element(),
            None => root.into_any_element(),
        }
    }
}

/// 控件侧回调面(Adapter 路由的落点;A11Y-08 单点收敛):
///
/// - `ime_editable` = **编辑会话存在**(`editing.is_some()`)——禁用门控
///   在组件入口先行(见 `replace_text_in_range`),此处不再重复判定;
/// - `ime_composition_range()` 恒 `None`:组合区间不追踪(模块 doc"真实
///   边界"),故 Adapter 的组合事件全部落直插路径——这是如实声明的能力
///   边界,不是遗漏;
/// - 无选区(选区 = M2),`ime_selection_range()` 恒 `None`。
impl ImeEditTarget for NumberField {
    fn ime_text(&self) -> Option<&str> {
        Some(self.editing.as_ref()?.text())
    }

    fn ime_editable(&self) -> bool {
        self.editing.is_some()
    }

    fn ime_caret(&self) -> Option<usize> {
        Some(self.editing.as_ref()?.caret())
    }

    fn ime_selection_range(&self) -> Option<std::ops::Range<usize>> {
        None // 选区 = M2(EditBuffer 已留扩展位)
    }

    fn ime_composition_range(&self) -> Option<std::ops::Range<usize>> {
        None // 组合区间不追踪(真实边界,模块 doc)
    }

    fn ime_insert(&mut self, text: &str) {
        if let Some(buffer) = self.editing.as_mut() {
            buffer.insert(text);
        }
    }

    fn ime_replace_range(&mut self, range: std::ops::Range<usize>, text: &str) {
        if let Some(buffer) = self.editing.as_mut() {
            buffer.replace_range(range, text);
        }
    }

    fn ime_composition_update(&mut self, new_text: &str, _caret_in_new_utf16: Option<usize>) {
        // 直插语义(v0.1 边界):组合文本不追踪 marked,光标推进插入文本
        self.ime_insert(new_text);
    }

    fn ime_composition_commit(&mut self, final_text: &str) {
        self.ime_insert(final_text);
    }

    fn ime_composition_cancel(&mut self) -> bool {
        false // 无组合区间可取消(边界同上)
    }

    fn ime_unmark(&mut self) -> bool {
        false // 无标记可解除
    }
}

/// IME / 平台文本输入(gpui 0.2.2 `EntityInputHandler`,见模块 doc"IME 钩子")。
///
/// 文本事件经 [`UTF16_ADAPTER`](crate::input_method 单点)路由:trait 文档
/// 以 **UTF-16** 计,数值 buffer 全 ASCII 时 UTF-16 下标与字节下标一致,
/// 含 CJK 的中间态由 Adapter 统一换算(本文件无第二份换算)。composing
/// (预提交)文本 v0.1 直插 buffer、不追踪 marked 区间——真机走查
/// (TC-A11Y-IME-01,`docs/a11y-notes.md §5`)若发现双写,再引入 marked
/// 追踪(随 M2 编辑器重构)。
impl EntityInputHandler for NumberField {
    fn text_for_range(
        &mut self,
        range_utf16: std::ops::Range<usize>,
        adjusted_range: &mut Option<std::ops::Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let (text, adjusted) = UTF16_ADAPTER.text_for_range_utf16(self, range_utf16)?;
        // adjusted_range 按 trait 契约回 UTF-16 口径
        adjusted_range.replace(adjusted);
        Some(text)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::UTF16Selection> {
        // 无编辑态 = None;编辑态无选区 = 光标处空选择(IME 由此定位插入点)
        let (range, reversed) = UTF16_ADAPTER.selection_utf16(self)?;
        Some(gpui::UTF16Selection { range, reversed })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        // 组合区间不追踪(真实边界,模块 doc;TC-A11Y-IME-01 真机项)→ None
        UTF16_ADAPTER.marked_range_utf16(self)
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        // 无 marked 区间需要解除(模块 doc);Adapter Unmark 事件 no-op
        UTF16_ADAPTER.route(self, ImeEvent::Unmark);
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<std::ops::Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            // TOK-07 对齐:禁用即交互全门控,IME 路径同样拒收。(收敛前此
            // 路径无门控——如实注明的行为修正,非兼容面;可编辑态语义不变)
            return;
        }
        // 平台未先经过 begin_editing 就发文本(极路径):以当前值播种 buffer
        if self.editing.is_none() {
            let value = self.binding.get(cx);
            self.editing = Some(EditBuffer::new(&format_value(value)));
        }
        // None 区间 + 无组合(EditBuffer 恒无组合)→ Adapter 落光标处插入;
        // Some 区间 → Adapter 归一后区间替换。与收敛前行为一致(极路径播种
        // 保留),仅路由/换算收敛到单点。
        let applied = UTF16_ADAPTER.route(self, ImeEvent::Commit { range_utf16, text });
        if applied {
            cx.notify();
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<std::ops::Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<std::ops::Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return; // 同 replace_text_in_range(TOK-07 对齐)
        }
        if self.editing.is_none() {
            let value = self.binding.get(cx);
            self.editing = Some(EditBuffer::new(&format_value(value)));
        }
        // composing 文本直插(不追踪 marked,组合内光标偏移不适用——直插后
        // 光标落插入文本之后);最终提交再走 replace_text_in_range。恒
        // CompositionBegin:组合区间不追踪,"更新"语义由直插覆盖。
        let applied = UTF16_ADAPTER.route(
            self,
            ImeEvent::CompositionBegin {
                delete_range_utf16: range_utf16,
                new_text,
                caret_in_new_utf16: None,
            },
        );
        if applied {
            cx.notify();
        }
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: std::ops::Range<usize>,
        element_bounds: gpui::Bounds<gpui::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        // 候选窗定位到控件本身(字符级定位 = M2:需字形测量)
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<gpui::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        // 点选定位 = M2(需命中字形);返回当前光标(近似,文档已注明)
        let buffer = self.editing.as_ref()?;
        Some(byte_to_utf16(buffer.text(), buffer.caret()))
    }
}

/// 单字符 key 判定("a" → 'a',"shift+2" 场景 key 恒单字符;命名键返回 None)。
fn single_char(key: &str) -> Option<char> {
    let mut chars = key.chars();
    let ch = chars.next()?;
    chars.next().is_none().then_some(ch)
}

// UTF-16 ↔ 字节下标换算已收敛到 crate::input_method 单点(A11Y-08:
// utf16_to_byte / byte_to_utf16,旧私有副本已删;测试经 use 引入)。

/// 修饰键倍率(纯函数):**Alt 优先 ×0.1,其次 Shift ×10**(上游 NumField
/// docs/upstream/02 §4.2 的 scrubby 公式同款)。
pub fn modifier_scale(alt: bool, shift: bool) -> f64 {
    if alt {
        0.1
    } else if shift {
        10.0
    } else {
        1.0
    }
}

/// 拖拽换算(纯函数):`start + dx × step × scale` 钳制到 range。
pub fn scrub(start_val: f64, dx_px: f64, step: f64, scale: f64, range: (f64, f64)) -> f64 {
    clamp_range(start_val + dx_px * step * scale, range)
}

/// 步进(键/滚轮共用):dir = ±1。
pub fn step_by(current: f64, dir: f64, step: f64, scale: f64, range: (f64, f64)) -> f64 {
    clamp_range(current + dir * step * scale, range)
}

/// 钳制到闭区间(min > max 时交换,防御调用方笔误)。
pub fn clamp_range(v: f64, range: (f64, f64)) -> f64 {
    let (lo, hi) = if range.0 <= range.1 {
        range
    } else {
        (range.1, range.0)
    };
    v.clamp(lo, hi)
}

/// 数值显示(纯函数):最多 2 位小数、去尾零;整数不带小数点。
pub fn format_value(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let rounded = (v * 100.0).round() / 100.0;
    if (rounded - rounded.trunc()).abs() < f64::EPSILON {
        format!("{}", rounded as i64)
    } else {
        let s = format!("{rounded:.2}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_method::utf16_to_byte;

    /// TOK-07:builder 存位 + 禁用视觉单点(容器 = 静止态容器、前景 =
    /// text_disabled;TC-TOK-DISABLED-01 的组件侧主体验收在
    /// tests/gate_inject_disabled.rs,此处锁 builder → 字段链路)。
    #[test]
    fn disabled_builder_sets_flag_and_visual_follows_rule() {
        let field = NumberField::new(Binding::new(|_cx| 0.0, |_v, _cx| {})).disabled(true);
        assert!(field.disabled, "builder 应存位");
        assert!(!NumberField::new(Binding::new(|_cx| 0.0, |_v, _cx| {})).disabled);
        let colors = ColorTokens::dark();
        let v = disabled_visual(&colors);
        assert_eq!(
            v,
            DisabledVisual {
                bg: colors.surface_2,
                border: colors.border_subtle,
                fg: colors.text_disabled,
            }
        );
    }

    #[test]
    fn modifier_scale_alt_takes_priority_over_shift() {
        assert_eq!(modifier_scale(false, false), 1.0);
        assert_eq!(modifier_scale(false, true), 10.0);
        assert_eq!(modifier_scale(true, false), 0.1);
        assert_eq!(modifier_scale(true, true), 0.1, "Alt 优先");
    }

    #[test]
    fn scrub_moves_by_dx_times_step_and_clamps() {
        assert_eq!(scrub(50.0, 10.0, 1.0, 1.0, (0.0, 100.0)), 60.0);
        assert_eq!(
            scrub(50.0, 10.0, 1.0, 10.0, (0.0, 100.0)),
            100.0,
            "Shift×10 钳上限"
        );
        // Alt×0.1 细调(无钳制路径):5 - 20×1.0×0.1 = 3
        assert_eq!(
            scrub(5.0, -20.0, 1.0, 0.1, (0.0, 100.0)),
            3.0,
            "Alt×0.1 细调"
        );
        // 同参数在贴下限处触发钳制:0.5 - 2.0 → 0
        assert_eq!(
            scrub(0.5, -20.0, 1.0, 0.1, (0.0, 100.0)),
            0.0,
            "细调仍受 range 钳制"
        );
        assert_eq!(scrub(0.0, -50.0, 1.0, 1.0, (0.0, 100.0)), 0.0, "钳下限");
        // 负范围与小步长(角度 0.5°/px)
        assert_eq!(scrub(-90.0, 4.0, 0.5, 1.0, (-180.0, 180.0)), -88.0);
    }

    #[test]
    fn step_by_walks_range_bounds() {
        assert_eq!(step_by(99.0, 1.0, 1.0, 1.0, (0.0, 100.0)), 100.0);
        assert_eq!(
            step_by(100.0, 1.0, 1.0, 1.0, (0.0, 100.0)),
            100.0,
            "到顶不再越界"
        );
        assert_eq!(step_by(1.0, -1.0, 1.0, 1.0, (0.0, 100.0)), 0.0);
        assert_eq!(step_by(5.0, 1.0, 1.0, 10.0, (0.0, 100.0)), 15.0);
    }

    #[test]
    fn clamp_range_swaps_reversed_bounds() {
        assert_eq!(clamp_range(5.0, (10.0, 0.0)), 5.0);
        assert_eq!(clamp_range(99.0, (10.0, 0.0)), 10.0, "反写范围被防御性交换");
        assert_eq!(clamp_range(-99.0, (10.0, 0.0)), 0.0);
    }

    #[test]
    fn format_value_trims_and_limits_decimals() {
        assert_eq!(format_value(10.0), "10");
        assert_eq!(format_value(-3.0), "-3");
        assert_eq!(format_value(0.5), "0.5");
        assert_eq!(format_value(1.25), "1.25");
        assert_eq!(format_value(1.234), "1.23", "最多 2 位小数(四舍五入)");
        assert_eq!(format_value(1.250001), "1.25", "去尾零");
        assert_eq!(format_value(f64::NAN), "0", "非有限值防御");
        assert_eq!(format_value(f64::INFINITY), "0");
    }

    #[test]
    fn control_height_fills_tier_floor() {
        // 14 行高 + 8 padding = 22,恰好紧凑档下限
        assert_eq!(NumberField::control_height(), HEIGHT_COMPACT);
    }

    /// TC-TOK-TYPE-02(TOK-02):数值文本渲染族 = mono 族令牌。渲染路径
    /// (render 的 `font_family(value_font_family())`)与 tokens 单点绑定;
    /// 族名值本身由 JSON typography 表经 TC-TOK-TYPE-01 对拍锁定。
    #[test]
    fn tc_tok_type_02_number_field_value_text_uses_mono_family() {
        assert_eq!(value_font_family(), crate::tokens::MONO_FONT);
        assert_eq!(value_font_family(), "JetBrains Mono");
        // 数值档三值(mono 档):render 的 text_size/font_weight 消费同源
        assert_eq!(TextSize::MONO.size, 12.0);
        assert_eq!(TextSize::MONO.weight, 400.0);
    }

    // —— v1.0 编辑态:buffer 状态机(任务 3.3 测试清单)——

    #[test]
    fn buffer_insert_advances_caret_and_keeps_boundary() {
        let mut b = EditBuffer::new("12");
        b.insert(".5");
        assert_eq!(b.text(), "12.5");
        assert_eq!(b.caret(), 4, "光标推进插入长度");
        b.insert("e");
        assert_eq!(b.text(), "12.5e");
        assert_eq!(b.segments(), ("12.5e", ""), "光标在末尾");
    }

    #[test]
    fn buffer_backspace_and_delete_respect_caret_bounds() {
        let mut b = EditBuffer::new("123");
        b.move_caret(CaretMove::Home);
        assert!(!b.backspace(), "起点 backspace 无操作");
        assert!(b.delete(), "删除光标后字符:123 → 23");
        assert_eq!(b.text(), "23");
        assert_eq!(b.caret(), 0);
        // 标准 caret 语义:delete = 删光标**后**字符(0 处仍有 '2' 可删)
        assert!(b.delete(), "再次 delete:23 → 3");
        assert_eq!(b.text(), "3");
        assert_eq!(b.caret(), 0);
        assert!(!b.backspace(), "光标在起点,backspace 无操作");
        assert_eq!(b.text(), "3");
        // 光标移到终点再 backspace:删前字符 3 → ""
        assert!(b.move_caret(CaretMove::Right));
        assert!(b.backspace(), "终点 backspace 删前字符");
        assert_eq!(b.text(), "");
        assert_eq!(b.caret(), 0);
    }

    #[test]
    fn buffer_caret_moves_and_clamps() {
        let mut b = EditBuffer::new("42");
        assert!(b.move_caret(CaretMove::Left));
        assert_eq!(b.caret(), 1);
        assert!(b.move_caret(CaretMove::Left));
        assert!(!b.move_caret(CaretMove::Left), "起点钳 0");
        assert!(b.move_caret(CaretMove::End));
        assert_eq!(b.caret(), 2, "Home/End 走到头");
        assert!(!b.move_caret(CaretMove::Right), "终点钳 len");
        assert!(b.move_caret(CaretMove::Home));
        assert_eq!(b.caret(), 0);
    }

    #[test]
    fn buffer_handles_multibyte_chars_on_boundaries() {
        // IME 插入 CJK 后 backspace 必须按字符边界删(不撕开 UTF-8)
        let mut b = EditBuffer::new("12");
        b.insert("厘米");
        assert_eq!(b.text(), "12厘米");
        assert!(b.backspace());
        assert_eq!(b.text(), "12厘", "整字符删除");
        assert_eq!(b.caret(), "12厘".len(), "字节偏移落在 char 边界");
    }

    #[test]
    fn editable_char_whitelist() {
        for c in '0'..='9' {
            assert!(is_editable_char(c));
        }
        assert!(is_editable_char('.'));
        assert!(is_editable_char('e'));
        assert!(is_editable_char('E'));
        assert!(is_editable_char('-'));
        assert!(is_editable_char('+'));
        assert!(!is_editable_char('x'));
        assert!(!is_editable_char(' '));
        assert!(!is_editable_char(';'));
    }

    #[test]
    fn commit_value_parses_and_falls_back_on_garbage() {
        assert_eq!(commit_value("42", (0.0, 100.0)), CommitOutcome::Value(42.0));
        assert_eq!(
            commit_value(" 3.5 ", (0.0, 100.0)),
            CommitOutcome::Value(3.5),
            "首尾空白容忍"
        );
        assert_eq!(
            commit_value("1e2", (0.0, 1000.0)),
            CommitOutcome::Value(100.0),
            "科学计数"
        );
        assert_eq!(commit_value("", (0.0, 100.0)), CommitOutcome::Invalid);
        assert_eq!(commit_value("abc", (0.0, 100.0)), CommitOutcome::Invalid);
        assert_eq!(commit_value("1..2", (0.0, 100.0)), CommitOutcome::Invalid);
        assert_eq!(
            commit_value("150", (0.0, 100.0)),
            CommitOutcome::Value(100.0),
            "提交值钳制 range"
        );
        assert_eq!(
            commit_value("-5", (-10.0, 10.0)),
            CommitOutcome::Value(-5.0),
            "负值合法(负范围)"
        );
        assert_eq!(
            commit_value("nan", (0.0, 100.0)),
            CommitOutcome::Invalid,
            "非有限回落旧值"
        );
    }

    #[test]
    fn buffer_segments_split_at_caret() {
        let mut b = EditBuffer::new("12.5");
        b.move_caret(CaretMove::Left);
        assert_eq!(b.segments(), ("12.", "5"));
        b.move_caret(CaretMove::Home);
        assert_eq!(b.segments(), ("", "12.5"));
        b.move_caret(CaretMove::End);
        assert_eq!(b.segments(), ("12.5", ""));
    }

    #[test]
    fn utf16_byte_mapping_round_trip_with_cjk() {
        let text = "12厘米e";
        let byte_of_cjk = "12".len();
        let byte_of_e = byte_of_cjk + "厘米".len();
        // ASCII 前缀:两套下标一致
        assert_eq!(utf16_to_byte(text, 2), Some(2));
        assert_eq!(byte_to_utf16(text, 2), 2);
        // CJK(BMP):utf16 每字 1 单位、utf8 每字 3 字节
        assert_eq!(byte_to_utf16(text, byte_of_cjk), 2);
        assert_eq!(utf16_to_byte(text, 2), Some(byte_of_cjk));
        assert_eq!(utf16_to_byte(text, 4), Some(byte_of_e));
        assert_eq!(byte_to_utf16(text, byte_of_e), 4);
        // 末尾与越界
        assert_eq!(utf16_to_byte(text, 5), Some(text.len()));
        assert_eq!(utf16_to_byte(text, 6), None);
        assert_eq!(byte_to_utf16(text, text.len()), 5, "字节下标越界钳到末尾");
    }

    #[test]
    fn utf16_index_inside_surrogate_pair_snaps_forward() {
        // '🎉' 是 4 字节 / 2 个 utf16 单位:落在其间的下标吸附到字符后边界
        let text = "1🎉";
        assert_eq!(utf16_to_byte(text, 0), Some(0));
        assert_eq!(utf16_to_byte(text, 1), Some(1));
        assert_eq!(
            utf16_to_byte(text, 2),
            Some(text.len()),
            "吸附到星体面后边界"
        );
        assert_eq!(utf16_to_byte(text, 3), Some(text.len()));
        assert_eq!(utf16_to_byte(text, 4), None, "越界");
    }

    #[test]
    fn replace_range_clamps_to_char_boundaries() {
        let mut b = EditBuffer::new("12厘米");
        // 平台可能发来撕裂 CJK 的下标(字节 3 = '厘' 中间):一律向下取边界,
        // 只删整字符(宁少勿多)
        b.replace_range(3..6, "X");
        assert_eq!(b.text(), "12X米");
        assert_eq!(b.caret(), "12X".len(), "光标落在插入文本之后");
        // caret 钳制到边界后插入:set_caret(9) 越界 → 钳到 3,落在尾部
        // (replace_range 不自动推进 caret,二次插入前显式重设)
        let mut c = EditBuffer::new("abc");
        c.set_caret(9);
        c.replace_range(3..3, "!");
        c.set_caret(9);
        c.replace_range(4..4, "X");
        assert_eq!(c.text(), "abc!X");
    }

    #[test]
    fn set_caret_clamps_to_boundary() {
        let mut b = EditBuffer::new("12厘米");
        b.set_caret(999);
        assert_eq!(b.caret(), b.text().len(), "越界钳末尾");
        b.set_caret(3);
        assert_eq!(b.caret(), 2, "撕裂下标向下取整到边界");
    }

    #[test]
    fn single_char_keys_only() {
        assert_eq!(single_char("5"), Some('5'));
        assert_eq!(single_char("-"), Some('-'));
        assert_eq!(single_char("enter"), None);
        assert_eq!(single_char(""), None);
    }
    #[test]
    fn tc_perf_nf_01_display_cache_zero_alloc_on_unchanged_value() {
        // PERF-01:值未变的帧(悬停/按压动画帧)必须零格式化零分配——
        // 证明:命中时返回的 SharedString 与缓存内是同一 Arc 分配(指针同一)。
        let mut cache = None;
        let first = cached_display(&mut cache, 1.5, "px");
        assert_eq!(first.as_ref(), "1.5px");
        let hit = cached_display(&mut cache, 1.5, "px");
        assert_eq!(hit.as_ref(), "1.5px");
        assert_eq!(first.as_ptr(), hit.as_ptr(), "命中帧不得产生新分配");

        // 值变化 → 文本更新(允许重新分配,内容必须正确)
        let changed = cached_display(&mut cache, -3.25, "px");
        assert_eq!(changed.as_ref(), "-3.25px");
        assert_ne!(first.as_ptr(), changed.as_ptr());

        // ±0.0 文本相同是既有口径(format_value 走 i64 截断),键按位区分:
        // -0.0 必然未命中并重格式化(指针可不同),但文本必须正确、不得误用。
        let zero = cached_display(&mut cache, 0.0, "");
        assert_eq!(zero.as_ref(), "0");
        let neg_zero = cached_display(&mut cache, -0.0, "");
        assert_eq!(neg_zero.as_ref(), "0");
    }
}
