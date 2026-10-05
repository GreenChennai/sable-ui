//! 下拉选择 Select / ComboBox(迭代审查报告 2026-10-04 §5.6 组件矩阵 #4,
//! CMP-01 批 1)。
//!
//! # 形态(受控组件,协议对齐 Tabs)
//!
//! [`Select`] 是**受控**组件:选中项的最终裁决在宿主——点击选项/键盘
//! Enter 产生 [`Select::on_change`]`(index, &mut App)` 回执,组件**不自持
//! 选中**;宿主落自身状态后经 [`Select::set_selected`] 回写(Tabs 协议同款)。
//!
//! ```ignore
//! let select_entity = cx.new(|cx| {
//!     let weak = cx.entity().downgrade();
//!     Select::new("export-format", ["PNG", "JPEG", "WebP"])
//!         .placeholder("选择导出格式…")
//!         .on_change(move |i, cx| {
//!             weak.update(cx, |sel, cx| sel.set_selected(i, cx));
//!         })
//! });
//! ```
//!
//! # 材质(报告 §5.6 #4:L2 触发钮 + L3 下拉)
//!
//! - **触发钮 = L2 凸起材质**:视觉规格**直接复用** [`button_style`]
//!   (Secondary 变体 + Default 尺寸,见 [`trigger_style`] 纯函数——复用而非
//!   自绘的取舍:与 Button 四态/禁用规则单一真相,TC-CMP-BTN-01 的门禁
//!   自动覆盖触发钮;代价是 press 缩放弹簧不适用,以 open = Pressed 态
//!   表达"激活"),渲染时经 [`elevated`] 垫 [`ELEVATIONS[2]`](TOK-01 消费点),
//!   尾部展开箭头 = 文字字形占位(SVG 图标系统 = §5.5 后续批次,与
//!   `Button::icon` 惯例一致);
//! - **下拉 = L3 浮层**:`surface_3` 底 + `border_strong` 发丝描边 +
//!   [`RadiusTokens::LG`],经 [`elevated`] 垫 [`ELEVATIONS[3]`] quad 阴影;
//! - **出现动画**:STATE 120ms([`MotionTokens::DUR_STATE_MS`])OutCubic,
//!   opacity 与 8px 上浮([`MENU_OPEN_OFFSET_PX`])同插值;`reduced_motion`
//!   由 [`Animated`] 直通目标(无入场动画,直切)。
//!
//! # 键盘(TC-CMP-SEL-01 的断言面)
//!
//! `track_focus` + `tab_stop(true)` 入 Tab 序;唯一键盘状态机
//! [`select_nav`](纯函数):关闭态 ↑/↓/Enter/Space 打开;打开态 ↑/↓ 移动
//! 高亮(端点钳制)、Home/End 跳端、Enter 选中高亮项并关闭、Esc/Tab 关闭;
//! **打开时高亮当前选中项**([`Select::begin_open`])。Type-ahead 首字母
//! 跳转 = [`typeahead_match`](选做项已落:循环向后匹配 label 首字符,大小写
//! 不敏感;拼音首字母容错属命令面板批次,不做)。
//!
//! # 浮层定位(纯函数可测)
//!
//! 锚定触发钮下方 [`MENU_GAP_PX`],**越界自动翻边**:[`dropdown_opens_upward`]
//! (纯函数,anchor rect + viewport → 向下放不下且向上放得下才翻上)。
//! 触发钮 bounds 于 paint 期经 canvas 记录(TextField 同款),viewport 取
//! `window.viewport_size()`。
//!
//! **边界(如实)**:下拉在组件根容器内 absolute 渲染——祖先面板的
//! `overflow_hidden` 会裁剪它,不跨面板/不跨窗口;统一"浮层 host"(第 5 组
//! CMP-03 的 Toast/Dialog/CommandPalette 框架)落地时迁移,本批 API 面
//! ([`SelectNav`]/[`dropdown_opens_upward`]/受控协议)不再变。
//!
//! # 禁用态(TOK-07)
//!
//! [`trigger_style`] 强制 Disabled 映射(容器与 Idle 逐位不变,仅前景降级,
//! 复用 Button 禁用门禁);交互全门控:点击不开、键盘不开、高亮/选中回执
//! 不产生。

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    App, Context, ElementId, FocusHandle, FontWeight, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, ParentElement, Pixels, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, canvas, div, px,
};

use crate::anim::{Animated, Easing, lerp_hsla};
use crate::controls::button::{ButtonSize, ButtonVariant, button_style};
use crate::interact::{
    self, HoverState, InteractState, Semantic, SemanticRole, semantic_slot, state_layer,
};
use crate::theme::{elevated, theme};
use crate::tokens::{
    ColorTokens, HEIGHT_DEFAULT, MotionTokens, RadiusTokens, SpacingTokens, TextSize, UI_FONT,
    control_height, h_flex, v_flex,
};

// ---------------------------------------------------------------------------
// 几何常量(具名;非令牌表的组件本体尺寸,同 choice/tabs 惯例)
// ---------------------------------------------------------------------------

/// 下拉与触发钮的间距(px,= XS 4)。
pub const MENU_GAP_PX: f32 = SpacingTokens::XS;
/// 下拉最大可见行数(超出纵向滚动)。
pub const MENU_MAX_ROWS: usize = 6;
/// 出现动画上浮距离(px,报告 §5.6 #4:120ms + 8px 上浮)。
pub const MENU_OPEN_OFFSET_PX: f32 = 8.0;
/// 触发钮最小宽(px):选项文本过短时保持可点命中区。
pub const TRIGGER_MIN_W_PX: f32 = 96.0;
/// 展开箭头(文字字形占位,§5.5 SVG 图标系统 = 后续批次)。
const CHEVRON_GLYPH: &str = "▾";
/// 当前项对勾(文字字形占位,同上)。
const CHECK_GLYPH: &str = "✓";

/// 下拉行高(px,派生制):`max(26, 18 + 2×4) = 26`,与 Tabs 页签/选择控件
/// 命中行同高(面板节奏一致,≥24px 命中区红线)。
#[must_use]
pub fn menu_row_height() -> f32 {
    control_height(
        HEIGHT_DEFAULT,
        TextSize::LABEL.line_height,
        SpacingTokens::XS,
    )
}

/// 下拉面板高度(px,纯函数):`min(选项数, MENU_MAX_ROWS) × 行高`
/// (超出部分纵向滚动;空表 = 0)。
#[allow(clippy::cast_possible_truncation)]
#[must_use]
pub fn menu_height(option_count: usize) -> f32 {
    option_count.min(MENU_MAX_ROWS) as f32 * menu_row_height()
}

// ---------------------------------------------------------------------------
// 纯函数层(键盘状态机 / type-ahead / 翻边定位 / 触发钮样式;可无 App 单测)
// ---------------------------------------------------------------------------

/// 键盘导航意图([`select_nav`] 的输出)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectNav {
    /// 打开下拉(关闭态 ↑/↓/Enter/Space)
    Open,
    /// 关闭下拉(打开态 Esc/Tab;不产生选中)
    Close,
    /// 移动高亮(打开态 ↑/↓/Home/End,已钳制)
    Highlight(usize),
    /// 选中高亮项并关闭(打开态 Enter)
    Select(usize),
    /// 其余情形:无意图
    None,
}

/// 下拉键盘状态机(纯函数,唯一键盘语义单点):
///
/// - **关闭态**:`up`/`down`/`enter`/`space` → [`SelectNav::Open`]
///   (经典 combobox:先开再选,两段 Enter);
/// - **打开态**:`down`/`up` 相邻移动(端点钳制不回绕)、`home`/`end` 跳端、
///   `enter` → [`SelectNav::Select`]`(高亮项)`、`escape`/`tab` → Close;
/// - 空表只认 Esc/Tab(关闭),其余无意图;越界 `highlighted` 钳入界内
///   (宿主未及时归位的防御)。
#[must_use]
pub fn select_nav(open: bool, highlighted: usize, count: usize, key: &str) -> SelectNav {
    if count == 0 {
        return match (open, key) {
            (true, "escape" | "tab") => SelectNav::Close,
            _ => SelectNav::None,
        };
    }
    let last = count - 1;
    match (open, key) {
        (false, "up" | "down" | "enter" | "space") => SelectNav::Open,
        (true, "down") => SelectNav::Highlight(highlighted.min(last).saturating_add(1).min(last)),
        (true, "up") => SelectNav::Highlight(highlighted.min(last).saturating_sub(1)),
        (true, "home") => SelectNav::Highlight(0),
        (true, "end") => SelectNav::Highlight(last),
        (true, "enter") => SelectNav::Select(highlighted.min(last)),
        (true, "escape" | "tab") => SelectNav::Close,
        _ => SelectNav::None,
    }
}

/// Type-ahead 首字母跳转(纯函数,选做项已落):从 `from` 的下一项起循环
/// 向后找 **首字符** 匹配 `ch`(大小写不敏感)的选项。无匹配返回 `None`。
/// 拼音首字母容错不在本批(CMP-03 命令面板批次)。
#[must_use]
pub fn typeahead_match(labels: &[impl AsRef<str>], from: usize, ch: char) -> Option<usize> {
    let count = labels.len();
    if count == 0 {
        return None;
    }
    let want = ch.to_lowercase().next()?;
    (0..count).find_map(|step| {
        let index = (from + 1 + step) % count;
        labels[index]
            .as_ref()
            .chars()
            .next()
            .and_then(|c| c.to_lowercase().next())
            .filter(|first| *first == want)
            .map(|_| index)
    })
}

/// 下拉翻边裁决(纯函数,报告 §5.6 #4"越界自动翻边"):下方
/// `anchor_bottom + gap + menu` 越出 viewport 底,且上方
/// `anchor_top - gap - menu` 不越出 viewport 顶时才向上翻;其余一律向下
/// (向下放不下且向上也放不下的情形保持惯常方位)。
#[must_use]
pub fn dropdown_opens_upward(
    anchor_top: f32,
    anchor_bottom: f32,
    viewport_top: f32,
    viewport_bottom: f32,
    menu_height: f32,
    gap: f32,
) -> bool {
    let fits_below = anchor_bottom + gap + menu_height <= viewport_bottom;
    if fits_below {
        return false;
    }
    anchor_top - gap - menu_height >= viewport_top
}

/// 触发钮样式(纯函数):**复用** [`button_style`](Secondary/Default)的
/// 取舍单点——激活(open)= Pressed 态,禁用强制 Disabled 映射(容器不变
/// 仅前景降级,TOK-07)。四态/禁用规则与 Button 单一真相,不另立规格。
#[must_use]
pub fn trigger_style(
    disabled: bool,
    open: bool,
    colors: &ColorTokens,
) -> crate::controls::button::ButtonStyle {
    let state = if disabled {
        InteractState::Disabled
    } else if open {
        InteractState::Pressed
    } else {
        InteractState::Idle
    };
    button_style(ButtonVariant::Secondary, ButtonSize::Default, state, colors)
}

// ---------------------------------------------------------------------------
// Select(builder + Entity,受控协议)
// ---------------------------------------------------------------------------

/// 选中变化回调(受控回执,同 `TabChangeFn` 惯例):`fn(index, &mut App)`
/// ——点击选项与键盘 Enter 统一经此上报宿主。
pub type SelectChangeFn = Rc<dyn Fn(usize, &mut App)>;

/// 下拉选择(受控 Entity 组件):`cx.new(|_| Select::new("fmt",
/// ["PNG", "JPEG"]).selected(0).on_change(|i, _cx| { .. }))`;
/// 宿主经 [`Self::set_selected`] 回写最终选中。
pub struct Select {
    id: ElementId,
    options: Vec<SharedString>,
    /// 当前选中(`None` = 未选,显示占位符;最终裁决在宿主)
    selected: Option<usize>,
    /// 打开态键盘/悬停高亮
    highlighted: usize,
    /// 下拉开合
    open: bool,
    /// 开合动画进度 0..1(STATE 120ms OutCubic:opacity + 8px 上浮同插值;
    /// `reduced_motion` 直通)
    open_anim: Animated<f64>,
    placeholder: Option<SharedString>,
    disabled: bool,
    /// 选中变化回调(受控回执;组件不自持选中)
    on_change: Option<SelectChangeFn>,
    /// 焦点句柄(首帧惰性创建,`tab_stop(true)` 进 Tab 环游)
    focus: Option<FocusHandle>,
    /// 触发钮悬停进度(120ms ease-out;禁用冻结)
    hover: HoverState,
    /// 触发钮窗口 bounds(paint 期 canvas 记录;翻边定位用)
    anchor_bounds: Option<gpui::Bounds<Pixels>>,
    /// A11Y-02 语义槽(可访问名;缺省回落选中项/占位符)
    semantic: Semantic,
}

impl Select {
    /// 由选项构造(初始未选,显示占位符;空表合法,不可打开)。
    pub fn new(
        id: impl Into<ElementId>,
        options: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        Select {
            id: id.into(),
            options: options.into_iter().map(Into::into).collect(),
            selected: None,
            highlighted: 0,
            open: false,
            open_anim: Animated::new(0.0),
            placeholder: None,
            disabled: false,
            on_change: None,
            focus: None,
            hover: HoverState::new(),
            anchor_bounds: None,
            semantic: Semantic::new(),
        }
    }

    /// 初始选中项(越界由渲染钳制)。
    #[must_use]
    pub fn selected(mut self, index: usize) -> Self {
        self.selected = if self.options.is_empty() {
            None
        } else {
            Some(index.min(self.options.len() - 1))
        };
        self
    }

    /// 占位符(未选中时触发钮文案,`text_placeholder` 色)。
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// 禁用态(TOK-07):容器不变仅前景降级;点击/键盘/高亮/回执全门控。
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 选中变化回执:`on_change(index, &mut App)`。点击选项与键盘 Enter
    /// 统一走此;组件不自持选中——宿主落自身状态后应调
    /// [`Self::set_selected`] 回写。
    pub fn on_change(mut self, f: impl Fn(usize, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    /// 选项数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.options.len()
    }

    /// 是否无选项。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.options.is_empty()
    }

    /// 当前选中项(组件侧渲染值;最终裁决在宿主)。
    #[must_use]
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// 下拉是否打开。
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// 宿主受控回写(受控协议落地点):钳制 → 通知;同值幂等。
    /// 不触发 [`Self::on_change`](回调是"组件 → 宿主"的上报通道,宿主
    /// 回写不应回环)。
    pub fn set_selected(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.options.is_empty() {
            return;
        }
        let clamped = index.min(self.options.len() - 1);
        if self.selected == Some(clamped) {
            return;
        }
        self.selected = Some(clamped);
        cx.notify();
    }

    /// 打开下拉(纯状态步进,无 App 依赖):禁用/已开/空表返回 `false`;
    /// 成功时**高亮当前选中项**(报告 §5.6 #4),启动 STATE 120ms 出现动画。
    pub fn begin_open(&mut self) -> bool {
        if self.disabled || self.open || self.options.is_empty() {
            return false;
        }
        self.open = true;
        self.highlighted = self.selected.unwrap_or(0).min(self.options.len() - 1);
        self.anim_open(true);
        true
    }

    /// 关闭下拉(纯状态步进):未开返回 `false`;成功时启动收合动画。
    pub fn begin_close(&mut self) -> bool {
        if !self.open {
            return false;
        }
        self.open = false;
        self.anim_open(false);
        true
    }

    /// 开合动画(STATE 档,毫秒令牌 ÷ 1000 → Duration;打断从当前值接续)。
    fn anim_open(&mut self, opening: bool) {
        self.open_anim.set(
            if opening { 1.0 } else { 0.0 },
            Duration::from_secs_f64(MotionTokens::DUR_STATE_MS / 1000.0),
            Easing::OutCubic,
        );
    }

    /// 选中上报(点击/键盘共用;无宿主回调时为无操作)。
    fn emit(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(cb) = self.on_change.clone() {
            cb(index, cx);
        }
    }

    fn on_mouse_down(
        &mut self,
        _event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return; // TOK-07:禁用即交互门控
        }
        if self.open {
            self.begin_close();
        } else {
            self.begin_open();
        }
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return; // TOK-07:禁用即交互门控
        }
        let key = event.keystroke.key.as_str();
        // Type-ahead(打开态,无修饰键的单字符键;首字母跳转)
        if self.open
            && !event.keystroke.modifiers.control
            && !event.keystroke.modifiers.alt
            && !event.keystroke.modifiers.platform
            && let Some(ch) = single_char(key)
            && let Some(index) = typeahead_match(&self.options, self.highlighted, ch)
        {
            self.highlighted = index;
            cx.notify();
            return;
        }
        match select_nav(self.open, self.highlighted, self.options.len(), key) {
            SelectNav::None => {}
            SelectNav::Open => {
                self.begin_open();
                cx.notify();
            }
            SelectNav::Close => {
                self.begin_close();
                // A11Y-01 Esc 契约:关下拉、焦点还触发钮。触发钮与下拉共用
                // 根句柄(焦点本就在其上),显式 focus 一次保证宿主侧焦点态
                // 与视觉一致(浮层收起后键盘继续操作触发钮)。
                if let Some(focus) = self.focus.clone() {
                    window.focus(&focus);
                }
                cx.notify();
            }
            SelectNav::Highlight(index) => {
                self.highlighted = index;
                cx.notify();
            }
            SelectNav::Select(index) => {
                self.emit(index, cx);
                self.begin_close();
                cx.notify();
            }
        }
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
}

impl Render for Select {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &theme(cx).colors;
        let disabled = self.disabled;
        let now = interact::now_ms();
        let hover_progress = if disabled {
            0.0
        } else {
            f32_val(self.hover.progress_at(now))
        };
        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let focused = !disabled && focus.is_focused(window);

        // 触发钮:button_style 复用(Secondary/Default);open = Pressed 态,
        // hover 120ms 插值,聚焦描边 accent(§5.3.3)
        let mut style = trigger_style(disabled, self.open, colors);
        if hover_progress > 0.0 && !disabled && !self.open {
            let hover_bg = trigger_style(false, true, colors).bg;
            style.bg = lerp_hsla(style.bg, hover_bg, f64::from(hover_progress));
        }
        if focused {
            style.border = colors.accent;
        }
        let trigger_h = style.height;

        // 触发钮文案:选中项 / 占位符(text_placeholder 色)
        let label = self
            .selected
            .and_then(|i| self.options.get(i))
            .cloned()
            .or_else(|| self.placeholder.clone())
            .unwrap_or_else(|| "".into());
        let label_color = if disabled {
            colors.text_disabled
        } else if self.selected.is_none() {
            colors.text_placeholder
        } else {
            style.fg
        };

        let quad = h_flex()
            .justify_between()
            .min_w(px(TRIGGER_MIN_W_PX))
            .w_full()
            .h(px(trigger_h))
            .px(px(style.h_padding))
            .rounded(px(style.radius))
            .border_1()
            .border_color(style.border)
            .bg(style.bg)
            .text_size(px(style.text.size))
            .font_family(UI_FONT)
            .font_weight(FontWeight(style.text.weight))
            .text_color(label_color)
            .child(div().truncate().child(label))
            .child(div().text_color(label_color).child(CHEVRON_GLYPH));

        // L2 凸起 + 焦点环(环画在触发钮盒上,不圈下拉;环 = interact 单点)
        let mut trigger_box = div().relative().w_full().child(elevated(2, quad));
        if focused {
            trigger_box = trigger_box.children(interact::focus_ring(colors.accent, style.radius));
        }

        // 触发钮 bounds 记录钩子(翻边定位用;TextField 同款 paint 期采集)
        let bounds_entity = cx.entity();
        let bounds_hook = canvas(
            |_, _, _| {},
            move |bounds, _, _window, cx| {
                bounds_entity.update(cx, |this, _| this.anchor_bounds = Some(bounds));
            },
        )
        .absolute()
        .inset_0();

        let mut root = div()
            .relative()
            .w_full()
            .track_focus(&focus)
            .child(trigger_box)
            .child(bounds_hook);

        // 下拉浮层:开合动画进行中或打开时渲染(L3 材质 + 翻边定位)
        let anim_running = self.open_anim.is_running_at(Instant::now());
        if self.open || anim_running {
            let progress = self.open_anim.value_at(Instant::now());
            root = root.child(self.render_menu(progress, trigger_h, window, cx));
        }

        // 事件:键盘挂根;点击/hover 挂 Stateful 触发(禁用不挂任何交互监听)
        let root = root.on_key_down(cx.listener(Self::on_key_down));
        let element = if disabled {
            root.into_any_element()
        } else {
            root.id(self.id.clone())
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                .on_hover(cx.listener(Self::on_hover_changed))
                .into_any_element()
        };

        // 动画帧泵:开合动画进行中才续帧(静止零帧提交)
        if anim_running {
            window.request_animation_frame();
        }
        element
    }
}

impl Select {
    /// 下拉浮层装配(L3 材质 + 翻边 + STATE 动画;从 render 拆出保持可读)。
    fn render_menu(
        &self,
        progress: f64,
        trigger_h: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let colors = &theme(cx).colors;
        let progress = progress.clamp(0.0, 1.0) as f32;
        let menu_h = menu_height(self.options.len());

        // 翻边:锚 = 触发钮(paint 期记录的窗口 bounds),viewport = 窗口
        let (anchor_top, anchor_bottom) = self
            .anchor_bounds
            .map(|b| {
                (
                    f32::from(b.origin.y),
                    f32::from(b.origin.y) + f32::from(b.size.height),
                )
            })
            .unwrap_or((0.0, trigger_h));
        let viewport_bottom = f32::from(window.viewport_size().height);
        let opens_up = dropdown_opens_upward(
            anchor_top,
            anchor_bottom,
            0.0,
            viewport_bottom,
            menu_h,
            MENU_GAP_PX,
        );
        // 8px 上浮:出现进度 0 → 向下偏 8px,进度 1 → 落位(翻边时同向)
        let rise = (1.0 - progress) * MENU_OPEN_OFFSET_PX;
        let top = if opens_up {
            -(MENU_GAP_PX + menu_h) + rise
        } else {
            trigger_h + MENU_GAP_PX + rise
        };

        // 选项行:高亮 = accent 同比例 alpha(TOK-04 Selected),当前项 =
        // 对勾 + 强文字;点击 → on_change 回执(受控,不自持)
        let highlighted = self.highlighted.min(self.options.len().saturating_sub(1));
        let on_change = self.on_change.clone();
        let mut rows = v_flex().min_w_full();
        for (i, label) in self.options.iter().enumerate() {
            let is_highlighted = i == highlighted;
            let is_selected = self.selected == Some(i);
            let mut row = h_flex()
                .justify_between()
                .w_full()
                .h(px(menu_row_height()))
                .px(px(SpacingTokens::SM))
                .gap(px(SpacingTokens::SM))
                .font_family(UI_FONT)
                .text_size(px(TextSize::LABEL.size))
                .font_weight(FontWeight(TextSize::LABEL.weight))
                .text_color(if is_highlighted || is_selected {
                    colors.text_strong
                } else {
                    colors.text_secondary
                })
                .child(div().truncate().child(label.clone()))
                .child(div().text_color(colors.accent).child(if is_selected {
                    CHECK_GLYPH
                } else {
                    ""
                }));
            if is_highlighted {
                row = row.bg(state_layer(
                    colors.surface_3,
                    InteractState::Selected,
                    colors.accent,
                ));
            } else {
                let hover_bg = state_layer(colors.surface_3, InteractState::Hover, colors.accent);
                row = row.hover(move |s| s.bg(hover_bg));
            }
            if let Some(cb) = on_change.clone() {
                row = row.on_mouse_down(
                    MouseButton::Left,
                    move |_event: &MouseDownEvent, _window, cx| cb(i, cx),
                );
            }
            rows = rows.child(row);
        }

        // 行区:最大可见行数,超出纵向滚动(Stateful + overflow_y_scroll)
        let rows_id = ElementId::NamedChild(Box::new(self.id.clone()), "menu-rows".into());
        let rows_area = rows.id(rows_id).max_h(px(menu_h)).overflow_y_scroll();

        // L3 浮层壳:surface_3 + border_strong + LG 圆角 + ELEVATIONS[3] 阴影,
        // absolute 锚定触发钮;STATE 动画 = opacity + 8px 上浮同插值
        let shell = v_flex()
            .min_w_full()
            .bg(colors.surface_3)
            .border_1()
            .border_color(colors.border_strong)
            .rounded(px(RadiusTokens::LG))
            .overflow_hidden()
            .child(rows_area);
        elevated(3, shell)
            .absolute()
            .left_0()
            .right_0()
            .top(px(top))
            .opacity(progress)
            .into_any_element()
    }
}

/// 单字符 key 判定("a" → 'a';命名键/空串返回 None;NumberField 同款)。
fn single_char(key: &str) -> Option<char> {
    let mut chars = key.chars();
    let ch = chars.next()?;
    chars.next().is_none().then_some(ch)
}

// A11Y-02 语义槽(label/role/semantic 三件;role 默认 Button——下拉以触发
// 钮为语义单元;可访问名缺省回落选中项/占位符,见 resolved_semantic)。
semantic_slot!(Select);

impl Select {
    /// 解析语义(A11Y-02):显式 `.label(...)` 优先;缺省回落**当前选中项
    /// 文本**,再回落占位符——触发钮可见文案即可访问名,单源。
    #[must_use]
    pub fn resolved_semantic(&self) -> Semantic {
        // 显式 `.label(...)` 优先;缺省回落选中项 → 占位符(触发钮可见文案
        // 即可访问名,单源)
        let explicit = self.semantic.label().cloned();
        let fallback = self
            .selected
            .and_then(|i| self.options.get(i).cloned())
            .or_else(|| self.placeholder.clone());
        let mut sem = Semantic::new();
        if let Some(text) = explicit.or(fallback) {
            sem = sem.with_label(text);
        }
        sem.with_role(self.semantic.role().unwrap_or(SemanticRole::Button))
    }
}

/// f64 动画值 → f32(GPU 域收口,button/panels 同款惯例)。
#[allow(clippy::cast_possible_truncation)]
fn f32_val(v: f64) -> f32 {
    v as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ELEVATIONS;

    fn labels() -> Vec<SharedString> {
        ["PNG", "JPEG", "WebP", "BMP"]
            .iter()
            .map(|s| (*s).into())
            .collect()
    }

    // —— TC-CMP-SEL-01:键盘操作状态机 ——

    #[test]
    fn tc_cmp_sel_01_keyboard_state_machine() {
        // 关闭态:↑/↓/Enter/Space 打开
        for key in ["up", "down", "enter", "space"] {
            assert_eq!(
                select_nav(false, 0, 4, key),
                SelectNav::Open,
                "{key} 应打开"
            );
        }
        // 打开态:↑↓ 移动高亮、端点钳制不回绕
        assert_eq!(select_nav(true, 0, 4, "down"), SelectNav::Highlight(1));
        assert_eq!(
            select_nav(true, 3, 4, "down"),
            SelectNav::Highlight(3),
            "末端钳制"
        );
        assert_eq!(select_nav(true, 2, 4, "up"), SelectNav::Highlight(1));
        assert_eq!(
            select_nav(true, 0, 4, "up"),
            SelectNav::Highlight(0),
            "顶端钳制"
        );
        // Home/End 跳端
        assert_eq!(select_nav(true, 2, 4, "home"), SelectNav::Highlight(0));
        assert_eq!(select_nav(true, 0, 4, "end"), SelectNav::Highlight(3));
        // Enter 选中高亮项并关闭;Esc/Tab 关闭不选中
        assert_eq!(select_nav(true, 2, 4, "enter"), SelectNav::Select(2));
        assert_eq!(select_nav(true, 2, 4, "escape"), SelectNav::Close);
        assert_eq!(select_nav(true, 2, 4, "tab"), SelectNav::Close);
        // 关闭态其余键无意图(不含字符导航——type-ahead 是独立通道)
        assert_eq!(select_nav(false, 0, 4, "a"), SelectNav::None);
        assert_eq!(select_nav(true, 1, 4, "left"), SelectNav::None);
        // 空表:仅 Esc/Tab 关闭,其余无意图
        assert_eq!(select_nav(false, 0, 0, "down"), SelectNav::None);
        assert_eq!(select_nav(true, 0, 0, "enter"), SelectNav::None);
        assert_eq!(select_nav(true, 0, 0, "escape"), SelectNav::Close);
        // 越界 highlighted(宿主未及时归位)钳入界内
        assert_eq!(select_nav(true, 9, 4, "down"), SelectNav::Highlight(3));
        assert_eq!(select_nav(true, 9, 4, "enter"), SelectNav::Select(3));
        // 单项:down 停在 0,enter 选中 0
        assert_eq!(select_nav(true, 0, 1, "down"), SelectNav::Highlight(0));
        assert_eq!(select_nav(true, 0, 1, "enter"), SelectNav::Select(0));
    }

    #[test]
    fn tc_cmp_sel_01_open_highlights_current_selection() {
        // 打开时高亮当前项(报告 §5.6 #4;begin_open 纯状态步进)
        let mut sel = Select::new("fmt", labels()).selected(2);
        assert!(sel.begin_open());
        assert!(sel.is_open());
        assert_eq!(sel.highlighted, 2, "打开时高亮当前选中项");
        // 未选中:高亮落到第 0 项
        let mut fresh = Select::new("fmt2", labels());
        assert!(fresh.begin_open());
        assert_eq!(fresh.highlighted, 0);
        // 已开/禁用/空表:不再打开
        assert!(!fresh.begin_open(), "已开不重复打开");
        let mut off = Select::new("off", labels()).disabled(true);
        assert!(!off.begin_open(), "禁用不开");
        assert!(!off.is_open());
        let mut empty = Select::new("empty", Vec::<&str>::new());
        assert!(!empty.begin_open(), "空表不开");
        // 关闭与收合动画
        assert!(sel.begin_close());
        assert!(!sel.is_open());
        assert!(!sel.begin_close(), "未开不再关");
    }

    // —— TC-CMP-SEL-01:Type-ahead 首字母跳转 ——

    #[test]
    fn tc_cmp_sel_01_typeahead_first_letter() {
        let labels = ["PNG", "JPEG", "WebP", "BMP"];
        assert_eq!(typeahead_match(&labels, 0, 'j'), Some(1), "从下一项起匹配");
        assert_eq!(typeahead_match(&labels, 0, 'J'), Some(1), "大小写不敏感");
        assert_eq!(typeahead_match(&labels, 1, 'p'), Some(0), "循环回绕匹配");
        assert_eq!(
            typeahead_match(&labels, 2, 'w'),
            Some(2),
            "从下一项起含当前项"
        );
        assert_eq!(typeahead_match(&labels, 1, 'z'), None, "无匹配 None");
        let empty: [&str; 0] = [];
        assert_eq!(typeahead_match(&empty, 0, 'a'), None, "空表 None");
        // SharedString 选项同形(AsRef<str> 泛型)
        let owned: Vec<SharedString> = vec!["alpha".into(), "beta".into()];
        assert_eq!(typeahead_match(&owned, 0, 'b'), Some(1));
    }

    // —— TC-CMP-SEL-01:下拉开关状态 + 禁用门控 ——

    #[test]
    fn tc_cmp_sel_01_disabled_gating_and_trigger_style() {
        for colors in [ColorTokens::dark(), ColorTokens::light()] {
            // 禁用触发钮:容器与启用静止态逐位相同(复用 Button 规则),
            // 仅前景降级;open 态不改变禁用裁决
            let idle = trigger_style(false, false, &colors);
            let dis = trigger_style(true, false, &colors);
            let dis_open = trigger_style(true, true, &colors);
            assert_eq!(dis.bg, idle.bg, "禁用 bg 逐位不变");
            assert_eq!(dis.border, idle.border, "禁用描边逐位不变");
            assert_eq!(dis.height, idle.height);
            assert_eq!(dis.fg, colors.text_disabled, "禁用前景降级");
            assert_ne!(dis.fg, idle.fg, "禁用前景可见降级");
            assert_eq!(dis_open.bg, idle.bg, "禁用下 open 不改容器");
            // 启用 + open = Pressed 态(激活视觉,复用 Button press 规则)
            let open_style = trigger_style(false, true, &colors);
            assert_ne!(open_style.bg, idle.bg, "激活态可见变化");
            // 高度 = Button Default 档(复用同一派生)
            assert_eq!(
                idle.height,
                button_style(
                    ButtonVariant::Secondary,
                    ButtonSize::Default,
                    InteractState::Idle,
                    &colors
                )
                .height
            );
        }
        // builder 存位
        let sel = Select::new("s", labels())
            .selected(1)
            .placeholder("选择…")
            .disabled(true)
            .on_change(|_i, _cx| {});
        assert_eq!(sel.selected_index(), Some(1));
        assert!(sel.disabled);
        assert_eq!(sel.placeholder, Some(SharedString::from("选择…")));
        assert!(sel.on_change.is_some());
        assert_eq!(sel.len(), 4);
        assert!(!sel.is_empty());
        assert!(!sel.is_open());
        let empty = Select::new("e", Vec::<&str>::new());
        assert!(empty.is_empty());
        assert_eq!(empty.selected_index(), None, "空表无选中");
        assert_eq!(
            empty.selected(0).selected_index(),
            None,
            "空表 selected 无效"
        );
    }

    // —— TC-CMP-SEL-01:浮层定位翻边(纯函数) ——

    #[test]
    fn tc_cmp_sel_01_dropdown_flips_at_viewport_edge() {
        let menu = menu_height(4); // 4 行 × 26 = 104
        let gap = MENU_GAP_PX;
        // 视口中部:下方放得下 → 向下
        assert!(!dropdown_opens_upward(100.0, 126.0, 0.0, 800.0, menu, gap));
        // 贴近视口底:下方放不下、上方放得下 → 翻上
        assert!(dropdown_opens_upward(720.0, 746.0, 0.0, 800.0, menu, gap));
        // 顶端:上方放不下 → 向下(放不下也不翻,保持惯常方位)
        assert!(!dropdown_opens_upward(0.0, 26.0, 0.0, 800.0, menu, gap));
        // viewport_top 非零(嵌入子视口)语义
        assert!(dropdown_opens_upward(500.0, 526.0, 100.0, 600.0, menu, gap));
        // 恰好放得下(等号):向下
        let bottom = 126.0 + gap + menu;
        assert!(
            !dropdown_opens_upward(100.0, 126.0, 0.0, bottom, menu, gap),
            "等号视为放得下"
        );
        // 上下都放不下(menu 高于上下两个空档):保持向下
        // below = 226+4+300 = 530 > 460;above = 200-4-300 = -104 < 0
        assert!(!dropdown_opens_upward(200.0, 226.0, 0.0, 460.0, 300.0, gap));
    }

    #[test]
    fn tc_cmp_sel_01_menu_geometry_derived() {
        // 行高派生(26 档,与 Tabs/选择控件同高)
        assert_eq!(menu_row_height(), 26.0);
        assert_eq!(menu_height(0), 0.0);
        assert_eq!(menu_height(3), 3.0 * 26.0);
        assert_eq!(menu_height(4), 104.0);
        // 超出最大可见行数被钳制(滚动消化)
        assert_eq!(menu_height(12), MENU_MAX_ROWS as f32 * 26.0);
        assert_eq!(MENU_MAX_ROWS, 6);
    }

    // —— TC-CMP-SEL-01:出现动画(STATE 120ms;reduced_motion 直切) ——

    #[test]
    fn tc_cmp_sel_01_open_anim_state_duration_and_reduced_motion() {
        let mut sel = Select::new("fmt", labels());
        assert!(!sel.open_anim.is_running(), "初始静止");
        assert!(sel.begin_open());
        // 时长 = STATE 档 120ms(报告 §5.6 #4)
        assert_eq!(MotionTokens::DUR_STATE_MS, 120.0);
        assert!(sel.open_anim.is_running(), "打开即播出现动画");
        // 目标 1.0(半透明起点 → 不透明落位)
        assert_eq!(*sel.open_anim.target(), 1.0);
        // 关闭:目标回落 0
        assert!(sel.begin_close());
        assert_eq!(*sel.open_anim.target(), 0.0);
        // reduced_motion:直通目标(直切,无插值)
        crate::anim::set_reduced_motion(true);
        let mut rm = Select::new("rm", labels());
        assert!(rm.begin_open());
        assert_eq!(rm.open_anim.value(), 1.0, "减弱动态直切全显");
        assert!(!rm.open_anim.is_running(), "减弱动态不再续帧");
        crate::anim::set_reduced_motion(false);
    }

    /// L3 材质消费(报告 §5.6 #4:下拉 = L3):触发钮垫 L2、下拉垫 L3,
    /// 渲染层经 theme::elevated 消费 ELEVATIONS[2]/[3](海拔参数单一真相)。
    #[test]
    fn tc_cmp_sel_01_l3_material_tokens() {
        assert_eq!(ELEVATIONS[2].blur, 8.0, "L2 浮面板档");
        assert_eq!(ELEVATIONS[3].blur, 16.0, "L3 下拉档");
        assert!(ELEVATIONS[3].alpha > ELEVATIONS[2].alpha, "L3 阴影重于 L2");
        assert_eq!(MENU_GAP_PX, 4.0);
        assert_eq!(MENU_OPEN_OFFSET_PX, 8.0);
        // 渲染层经 theme::elevated(2/3) 包装触发钮/下拉(本文件 render 两处
        // 消费点;海拔参数单一源自 ELEVATIONS,与 gate_elevation_consumed 对齐)
        assert_eq!(crate::theme::shadow(3), ELEVATIONS[3]);
    }

    #[test]
    fn single_char_keys_only() {
        assert_eq!(single_char("p"), Some('p'));
        assert_eq!(single_char("enter"), None);
        assert_eq!(single_char(""), None);
        assert_eq!(single_char("ab"), None);
    }

    /// A11Y-02 语义槽:label 覆写优先;缺省回落选中项 → 占位符;role 默认
    /// Button、可覆写。
    #[test]
    fn semantic_slot_label_falls_back_to_selection_then_placeholder() {
        let sel = Select::new("s", labels()).selected(1);
        assert_eq!(
            sel.resolved_semantic().label().map(|s| s.as_ref()),
            Some("JPEG"),
            "缺省回落选中项"
        );
        let fresh = Select::new("f", labels()).placeholder("选择格式…");
        assert_eq!(
            fresh.resolved_semantic().label().map(|s| s.as_ref()),
            Some("选择格式…"),
            "未选中回落占位符"
        );
        let named = Select::new("n", labels())
            .label("导出格式")
            .role(crate::interact::SemanticRole::Select);
        assert_eq!(
            named.resolved_semantic().label().map(|s| s.as_ref()),
            Some("导出格式"),
            "显式 label 优先于选中项"
        );
        assert_eq!(
            named.resolved_semantic().role(),
            Some(crate::interact::SemanticRole::Select)
        );
    }
}
