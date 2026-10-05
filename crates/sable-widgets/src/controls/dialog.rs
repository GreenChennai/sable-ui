//! Dialog / Modal 对话框(迭代审查报告 2026-10-04 CMP-03,§5.6 组件矩阵
//! #11、§5.8 动效、ANI-01 #4"对话框进出场")。
//!
//! # 形态(Entity 宿主 + spec 打开)
//!
//! [`DialogHost`] 是 **Entity 形态浮层宿主**:挂在应用根视图,宿主经
//! [`DialogHost::open`] 打开:
//!
//! ```ignore
//! let dialogs = cx.new(|_| DialogHost::new());
//! // 任意业务点(删除确认):
//! dialogs.update(cx, |host, cx| {
//!     host.open(
//!         DialogSpec::new("删除图层?", |_win, _cx| {
//!             div().child("该操作不可撤销。").into_any_element()
//!         })
//!         .button(DialogButton::cancel("取消"))
//!         .button(DialogButton::confirm("删除").on_press(|_win, _cx| { /* 删除 */ })),
//!         Some(opener_focus),
//!         window,
//!         cx,
//!     );
//! });
//! ```
//!
//! # 结构(§5.6 #11)
//!
//! 标题([`TextSize::TITLE`] 字号)/ 内容(闭包,宿主渲染)/ 底部按钮
//! **右对齐**(按钮走 [`button_element`](crate::controls::button) 内联形态
//! ——浮层逐帧重建,逐行建 Entity 是反模式 CMP-06);背板(scrim)=
//! [`overlay_scrim`](共享单点,`surface_0` @ [`OVERLAY_SCRIM_OPACITY`],
//! 零硬编码色);面板 = `surface_1` 底 + `border_strong` 描边 +
//! [`RadiusTokens::LG`] 圆角,经 [`elevated(4, …)`](crate::theme::elevated)
//! 垫 L4 海拔阴影(ELEVATIONS[4]"对话框"档,TOK-01 消费点)。
//!
//! # 键盘(TC-CMP-DLG-01 的断言面)
//!
//! **Esc = 取消、Enter = 确认**([`dialog_key`] 纯函数唯一裁决);Tab 被
//! 拦截为焦点 trap([`focus_trap_next`] 纯函数 + 面板根单停靠点形态,见
//! 下"边界");关闭时**焦点恢复给打开者**(`open` 记下 `opener` 句柄,
//! `begin_close` 立即 `window.focus(&opener)`——select.rs Esc 还焦点的
//! 既有做法)。
//!
//! # 焦点 trap(v0.1 边界,如实声明)
//!
//! gpui 0.2.2 无 focus-trap API;本组件的 trap 是**面板根单停靠点**形态:
//! 打开时焦点移入面板根(子树内唯一的 tab stop——按钮走内联形态无焦点
//! 句柄),Tab 按键被 [`dialog_key`] 拦截为 [`DialogKeyIntent::TrapFocus`],
//! 渲染层把它落成"重新聚焦面板根",焦点因此不出对话框子树。若宿主在
//! body 里塞入自带焦点句柄的自定义控件,请自管其 Tab 行为(宿主控件
//! 进入 Tab 序后 root 单停靠点不再完备)。升级真 Tab 环游待 gpui 焦点
//! 陷阱原语。
//!
//! # 动效(§5.8 / ANI-01 #4)
//!
//! 出场 = STATE 档([`MotionTokens::DUR_STATE_MS`],120ms)+ 8px 上浮
//! ([`DIALOG_RISE_PX`],单源复用 select 下拉的 8px 位移常量)+ 同窗淡入
//! (面板与背板同进度);收场对称。状态机 [`DialogClock`] 纯函数、时间
//! 显式注入,`reduced_motion` **直切**(相位直落终态、透明度只有 0/1)。
//!
//! # A11Y(如实)
//!
//! [`interact::SemanticRole`] 目前**没有 `Dialog` 变体**(interact.rs 本轮
//! 禁改),语义槽 role 缺省回落 [`SemanticRole::Group`](最接近的容器角色),
//! 宿主可用 `.role(...)` 显式覆写;label 缺省回落对话框标题。`Dialog` 变体
//! 落地后应把缺省改挂其上。

use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, Styled, Window, div, px,
};

use crate::anim::reduced_motion;
use crate::controls::button::{Button, ButtonSize, ButtonVariant, button_element};
use crate::controls::toast::{OVERLAY_MARGIN_PX, overlay_scrim};
use crate::interact::{InteractState, Semantic, SemanticRole, now_ms, semantic_slot, state_layer};
use crate::theme::{elevated, theme};
use crate::tokens::{MotionTokens, RadiusTokens, SpacingTokens, TextSize, UI_FONT, h_flex, v_flex};

// ---------------------------------------------------------------------------
// 常量域(几何/时序具名常量;颜色一律令牌,零字面量)
// ---------------------------------------------------------------------------

/// 出场/收场时长 = 动效四档的 STATE 档(120ms,§5.6 #11/ANI-01 #4)。
pub const DIALOG_ANIM_MS: f64 = MotionTokens::DUR_STATE_MS;
/// 出场上浮位移(px,§5.6 #11"8px";单源复用 select 下拉的 8px 位移常量)。
pub const DIALOG_RISE_PX: f32 = super::select::MENU_OPEN_OFFSET_PX;
/// 面板宽(px,4 网格;窄窗由渲染钳进视口)。
pub const DIALOG_WIDTH_PX: f32 = 420.0;
/// 面板内边距 = 间距令牌 LG(16px,4 网格)。
pub const DIALOG_PADDING_PX: f32 = SpacingTokens::LG;
/// 面板圆角 = 圆角令牌 LG(8px)。
pub const DIALOG_RADIUS_PX: f32 = RadiusTokens::LG;

// 背板 = surface_0 @ OVERLAY_SCRIM_OPACITY(共享单点 toast::overlay_scrim,
// 本文件不重复定义材质)。

// ---------------------------------------------------------------------------
// 纯函数层(键盘裁决 / 焦点 trap / 按钮解析;可无 App 单测)
// ---------------------------------------------------------------------------

/// 对话框键盘意图([`dialog_key`] 的输出)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DialogKeyIntent {
    /// Enter:确认(仅存在 Confirm 类按钮时)
    Confirm,
    /// Esc:取消
    Cancel,
    /// Tab:焦点 trap(重新聚焦面板根,不出子树)
    TrapFocus,
    /// 其余情形:无意图
    None,
}

/// 对话框键盘裁决(纯函数,唯一键盘语义单点):**Esc = 取消、Enter =
/// 确认**(§5.6 #11);Tab 拦截为焦点 trap;`has_confirm` 为假时 Enter
/// 无意图(无确认钮的对话框不得凭空"确认")。
#[must_use]
pub fn dialog_key(key: &str, has_confirm: bool) -> DialogKeyIntent {
    match key {
        "escape" => DialogKeyIntent::Cancel,
        "enter" if has_confirm => DialogKeyIntent::Confirm,
        "tab" => DialogKeyIntent::TrapFocus,
        _ => DialogKeyIntent::None,
    }
}

/// 焦点 trap 环游(纯函数):`count` 个停靠点内,从 `current` 前进/后退
/// 一步,**到端点回绕**(trap 语义:焦点永不离开子树)。`count` 0 → 0;
/// 越界 `current` 钳入界内(宿主未及时归位的防御)。
#[must_use]
pub fn focus_trap_next(current: usize, count: usize, forward: bool) -> usize {
    if count == 0 {
        return 0;
    }
    let current = current.min(count - 1);
    if forward {
        (current + 1) % count
    } else {
        (current + count - 1) % count
    }
}

/// 按钮语义类(裁决 Enter/Esc 接线与默认变体)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DialogButtonKind {
    /// 确认(Enter 接线;默认 Primary 变体)
    #[default]
    Confirm,
    /// 取消(Esc 接线;默认 Secondary 变体)
    Cancel,
    /// 中性动作(点击后同样关对话框,不接键盘)
    Neutral,
}

/// 按钮动作回调(`fn(&mut Window, &mut App)`;宿主业务落点)。
pub type DialogActionFn = Rc<dyn Fn(&mut Window, &mut App)>;

/// 对话框底部按钮(§5.6 #11;渲染走
/// [`button_element`](crate::controls::button) 内联形态)。
#[derive(Clone, Default)]
pub struct DialogButton {
    /// 按钮文案
    pub label: SharedString,
    /// 变体(缺省按 kind:Confirm = Primary,Cancel = Secondary,Neutral =
    /// Secondary;显式 `variant` 覆写)
    pub variant: Option<ButtonVariant>,
    /// 语义类(Enter/Esc 接线依据)
    pub kind: DialogButtonKind,
    /// 点击回调(宿主业务;确认/取消在回调后自动关对话框)
    pub on_press: Option<DialogActionFn>,
}

impl DialogButton {
    /// 确认钮(Primary 变体;Enter 接线)。
    pub fn confirm(label: impl Into<SharedString>) -> Self {
        DialogButton {
            label: label.into(),
            variant: Some(ButtonVariant::Primary),
            kind: DialogButtonKind::Confirm,
            on_press: None,
        }
    }

    /// 取消钮(Secondary 变体;Esc 接线)。
    pub fn cancel(label: impl Into<SharedString>) -> Self {
        DialogButton {
            label: label.into(),
            variant: Some(ButtonVariant::Secondary),
            kind: DialogButtonKind::Cancel,
            on_press: None,
        }
    }

    /// 中性钮(变体显式给;点击关对话框,不接键盘)。
    pub fn neutral(label: impl Into<SharedString>, variant: ButtonVariant) -> Self {
        DialogButton {
            label: label.into(),
            variant: Some(variant),
            kind: DialogButtonKind::Neutral,
            on_press: None,
        }
    }

    /// 点击回调(链式)。
    #[must_use]
    pub fn on_press(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_press = Some(Rc::new(f));
        self
    }
}

/// 第一枚指定语义类按钮的下标(纯函数;Enter/Esc 接线解析单点)。
#[must_use]
pub fn first_button_of_kind(buttons: &[DialogButton], kind: DialogButtonKind) -> Option<usize> {
    buttons.iter().position(|button| button.kind == kind)
}

/// 对话框内容 provider(宿主渲染对话框正文;`fn(&mut Window, &mut App)`
/// → 任意元素)。
pub type DialogBodyFn = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

/// 对话框规格(标题 / 内容 / 底部按钮,§5.6 #11)。
#[derive(Clone, Default)]
pub struct DialogSpec {
    /// 标题(title 字号 = [`TextSize::TITLE`])
    pub title: SharedString,
    /// 内容 provider(每帧调用,宿主闭包内自取状态)
    pub body: Option<DialogBodyFn>,
    /// 底部按钮(渲染序 = 加入序;右对齐)
    pub buttons: Vec<DialogButton>,
}

impl DialogSpec {
    /// 标题 + 内容闭包构造(无按钮;`DialogButton::confirm`/`cancel` 链式
    /// 追加)。
    pub fn new(
        title: impl Into<SharedString>,
        body: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        DialogSpec {
            title: title.into(),
            body: Some(Rc::new(body)),
            buttons: Vec::new(),
        }
    }

    /// 追加底部按钮(链式)。
    #[must_use]
    pub fn button(mut self, button: DialogButton) -> Self {
        self.buttons.push(button);
        self
    }

    /// 是否存在确认钮(Enter 接线前提)。
    #[must_use]
    pub fn has_confirm(&self) -> bool {
        first_button_of_kind(&self.buttons, DialogButtonKind::Confirm).is_some()
    }

    /// 是否存在取消钮(Esc 关闭后是否回执取消钮回调)。
    #[must_use]
    pub fn has_cancel(&self) -> bool {
        first_button_of_kind(&self.buttons, DialogButtonKind::Cancel).is_some()
    }
}

// ---------------------------------------------------------------------------
// 显隐状态机(纯函数;TC-CMP-DLG-01 的被测单点)
// ---------------------------------------------------------------------------

/// 对话框相位(纯状态机,时间显式注入)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DialogPhase {
    /// 隐藏(静止态,零帧)
    #[default]
    Hidden,
    /// 出场中(120ms + 8px 上浮 + 淡入)
    Showing,
    /// 打开(静止态,零帧)
    Open,
    /// 收场中(120ms,对称)
    Hiding,
}

/// 对话框显隐时钟(纯状态机):`show` → 120ms 出场 → 打开;`close` →
/// 120ms 收场 → 隐藏。`reduced` **直切**(相位立即落终态)。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DialogClock {
    phase: DialogPhase,
    started_ms: f64,
}

impl DialogClock {
    /// 隐藏态新时钟。
    pub fn new() -> Self {
        DialogClock::default()
    }

    /// 当前相位(不推进;渲染帧间的快照)。
    #[must_use]
    pub fn phase(&self) -> DialogPhase {
        self.phase
    }

    /// 是否静止(Hidden/Open):为真时宿主无需续帧。
    #[must_use]
    pub fn is_settled(&self) -> bool {
        matches!(self.phase, DialogPhase::Hidden | DialogPhase::Open)
    }

    /// 请求显示(打开;进行中收场则反向重入)。非有限时间戳忽略(防御)。
    pub fn show(&mut self, now_ms: f64) {
        if !now_ms.is_finite() {
            return;
        }
        self.phase = DialogPhase::Showing;
        self.started_ms = now_ms;
    }

    /// 请求关闭(Hidden 后无操作,幂等)。非有限时间戳忽略(防御)。
    pub fn close(&mut self, now_ms: f64) {
        if !now_ms.is_finite() {
            return;
        }
        if matches!(self.phase, DialogPhase::Showing | DialogPhase::Open) {
            self.phase = DialogPhase::Hiding;
            self.started_ms = now_ms;
        }
    }

    /// 推进并返回当前相位(沉降式)。`reduced` 直切。
    pub fn phase_at(&mut self, now_ms: f64, reduced: bool) -> DialogPhase {
        if !now_ms.is_finite() {
            return self.phase;
        }
        let elapsed = now_ms - self.started_ms;
        self.phase = match self.phase {
            DialogPhase::Showing => {
                if reduced || elapsed >= DIALOG_ANIM_MS {
                    DialogPhase::Open
                } else {
                    DialogPhase::Showing
                }
            }
            DialogPhase::Hiding => {
                if reduced || elapsed >= DIALOG_ANIM_MS {
                    DialogPhase::Hidden
                } else {
                    DialogPhase::Hiding
                }
            }
            settled => settled,
        };
        self.phase
    }

    /// 透明度 0..1(线性;面板与背板同进度;`reduced` 只有 0/1)。
    #[must_use]
    pub fn opacity_at(&self, now_ms: f64, reduced: bool) -> f64 {
        let elapsed = now_ms - self.started_ms;
        match self.phase {
            DialogPhase::Showing => {
                if reduced {
                    return 1.0;
                }
                (elapsed / DIALOG_ANIM_MS).clamp(0.0, 1.0)
            }
            DialogPhase::Open => 1.0,
            DialogPhase::Hiding => {
                if reduced {
                    return 0.0;
                }
                1.0 - (elapsed / DIALOG_ANIM_MS).clamp(0.0, 1.0)
            }
            DialogPhase::Hidden => 0.0,
        }
    }

    /// 上浮位移(px,OutCubic):出场从"下方 8px"归零,收场对称沉回;
    /// `reduced` 恒 0(直切,无位移)。
    #[must_use]
    pub fn rise_offset_at(&self, now_ms: f64, reduced: bool) -> f64 {
        if reduced {
            return 0.0;
        }
        let elapsed = now_ms - self.started_ms;
        match self.phase {
            DialogPhase::Showing => {
                let p = (elapsed / DIALOG_ANIM_MS).clamp(0.0, 1.0);
                (1.0 - crate::anim::Easing::OutCubic.apply(p)) * f64::from(DIALOG_RISE_PX)
            }
            DialogPhase::Hiding => {
                let q = (elapsed / DIALOG_ANIM_MS).clamp(0.0, 1.0);
                crate::anim::Easing::OutCubic.apply(q) * f64::from(DIALOG_RISE_PX)
            }
            _ => 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// DialogHost(Entity 形态,挂宿主根)
// ---------------------------------------------------------------------------

/// 对话框宿主(Entity 形态):挂在视图根,程序化 [`Self::open`] 打开,
/// 背板 + 居中面板 + 右对齐按钮 + Esc/Enter 接线 + 焦点恢复全套。
pub struct DialogHost {
    spec: Option<DialogSpec>,
    clock: DialogClock,
    /// 面板根焦点句柄(打开时惰性创建并聚焦;trap 的单停靠点)
    focus: Option<FocusHandle>,
    /// 打开者焦点句柄(关闭时恢复焦点给它)
    opener: Option<FocusHandle>,
    /// A11Y-02 语义槽(label 缺省回落标题;role 缺省 Group,见模块 doc)
    semantic: Semantic,
}

// A11Y-02 语义槽(可访问名缺省回落对话框标题;role 无 Dialog 变体,
// 缺省 Group,宿主可覆写——见模块 doc"如实")。
semantic_slot!(DialogHost);

impl Default for DialogHost {
    fn default() -> Self {
        Self::new()
    }
}

impl DialogHost {
    /// 隐藏态新宿主。
    pub fn new() -> Self {
        DialogHost {
            spec: None,
            clock: DialogClock::new(),
            focus: None,
            opener: None,
            semantic: Semantic::new(),
        }
    }

    /// 解析语义(A11Y-02):显式 `.label(...)`/`.role(...)` 优先;label
    /// 缺省回落当前对话框标题,role 缺省 Group(无 Dialog 变体,模块 doc)。
    #[must_use]
    pub fn resolved_semantic(&self) -> Semantic {
        let mut sem = Semantic::new();
        let explicit = self.semantic.label().cloned();
        let fallback = self.spec.as_ref().map(|spec| spec.title.clone());
        if let Some(text) = explicit.or(fallback) {
            sem = sem.with_label(text);
        }
        sem.with_role(self.semantic.role().unwrap_or(SemanticRole::Group))
    }

    /// 当前规格(打开中;宿主调试/测试读)。
    #[must_use]
    pub fn spec(&self) -> Option<&DialogSpec> {
        self.spec.as_ref()
    }

    /// 显隐时钟(宿主/测试可读;组件边界外的纯状态)。
    #[must_use]
    pub fn clock(&self) -> &DialogClock {
        &self.clock
    }

    /// 是否处于打开链路(出场/打开/收场中)。
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.clock.phase() != DialogPhase::Hidden
    }

    /// 是否需要续帧(时钟未静止;静止零帧提交)。
    #[must_use]
    pub fn needs_frame(&self) -> bool {
        !self.clock.is_settled()
    }

    /// 打开(§5.6 #11):记规格与打开者焦点,启动出场动画并**把焦点移入
    /// 面板根**(trap 单停靠点)。进行中的对话框被新 spec 替换(重开语义)。
    pub fn open(
        &mut self,
        spec: DialogSpec,
        opener: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        self.spec = Some(spec);
        self.opener = opener;
        self.clock.show(now_ms());
        window.focus(&focus);
        cx.notify();
    }

    /// 确认路径(Enter/确认钮):回执第一枚 Confirm 钮的回调,然后关闭。
    pub fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let index = self
            .spec()
            .and_then(|spec| first_button_of_kind(&spec.buttons, DialogButtonKind::Confirm));
        self.fire_button(index, window, cx);
    }

    /// 取消路径(Esc/取消钮):回执第一枚 Cancel 钮的回调,然后关闭。
    pub fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let index = self
            .spec()
            .and_then(|spec| first_button_of_kind(&spec.buttons, DialogButtonKind::Cancel));
        self.fire_button(index, window, cx);
    }

    /// 按下底部第 `index` 枚按钮:回执其回调(中性钮仅回调),随后关闭。
    pub fn press_button(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.fire_button(Some(index), window, cx);
    }

    /// 关闭(纯状态步进 + 焦点恢复):启动对称收场,并**立即把焦点还给
    /// 打开者**(select.rs Esc 还焦点的既有做法;面板淡出期间键盘已在
    /// 打开者上下文)。无回调路径(程序化 dismiss);Esc/按钮路径先回执
    /// 再走这里。
    pub fn begin_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clock.close(now_ms());
        if let Some(opener) = self.opener.clone() {
            window.focus(&opener);
        }
        cx.notify();
    }

    /// 回执按钮回调并关闭(`index` `None` = 无对应钮,仅关闭——Esc 在无
    /// 取消钮的对话框上仍然关得掉)。
    fn fire_button(&mut self, index: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let callback = index
            .and_then(|index| self.spec.as_ref().and_then(|spec| spec.buttons.get(index)))
            .and_then(|button| button.on_press.clone());
        if let Some(callback) = callback {
            callback(window, cx);
        }
        self.begin_close(window, cx);
    }

    /// 键盘接线(挂面板根;焦点在子树内时到达):
    /// [`dialog_key`] 裁决 → Confirm/Cancel/TrapFocus 三路。
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let has_confirm = self.spec.as_ref().is_some_and(DialogSpec::has_confirm);
        match dialog_key(event.keystroke.key.as_str(), has_confirm) {
            DialogKeyIntent::None => {}
            DialogKeyIntent::Confirm => self.accept(window, cx),
            DialogKeyIntent::Cancel => self.cancel(window, cx),
            DialogKeyIntent::TrapFocus => {
                // 焦点 trap:重新聚焦面板根(单停靠点,见模块 doc"边界")
                if let Some(focus) = self.focus.clone() {
                    window.focus(&focus);
                }
                cx.notify();
            }
        }
    }
}

impl Render for DialogHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = now_ms();
        let reduced = reduced_motion();
        let phase = self.clock.phase_at(now, reduced);
        if phase == DialogPhase::Hidden {
            return div().into_any_element();
        }
        let Some(spec) = self.spec.clone() else {
            return div().into_any_element();
        };
        let colors = theme(cx).colors;
        let opacity = f32v(self.clock.opacity_at(now, reduced));
        let rise = f32v(self.clock.rise_offset_at(now, reduced));
        let entity = cx.entity();

        // 背板(scrim):共享单点 surface_0 半透明遮罩,随面板同窗淡变
        let scrim = overlay_scrim(&colors).opacity(opacity);

        // 标题(title 字号)
        let title = div()
            .font_family(UI_FONT)
            .text_size(px(TextSize::TITLE.size))
            .font_weight(gpui::FontWeight(TextSize::TITLE.weight))
            .text_color(colors.text_strong)
            .child(spec.title.clone());

        // 内容(provider 闭包)
        let body = match &spec.body {
            Some(provider) => provider(window, cx),
            None => div().into_any_element(),
        };

        // 底部按钮(右对齐;内联形态,CMP-06 纪律)
        let mut buttons = h_flex().justify_end().gap(px(SpacingTokens::SM));
        for (index, button) in spec.buttons.iter().enumerate() {
            let variant = button.variant.unwrap_or(match button.kind {
                DialogButtonKind::Confirm => ButtonVariant::Primary,
                DialogButtonKind::Cancel | DialogButtonKind::Neutral => ButtonVariant::Secondary,
            });
            let entity = entity.clone();
            buttons = buttons.child(button_element(
                Button::new(
                    gpui::ElementId::NamedInteger(
                        "dialog-btn".into(),
                        u64::try_from(index).unwrap_or(0),
                    ),
                    button.label.clone(),
                )
                .variant(variant)
                .size(ButtonSize::Default)
                .on_press(move |_ev, window, cx| {
                    entity.update(cx, |host, cx| host.press_button(index, window, cx));
                }),
                cx,
            ));
        }

        // 面板:L4 海拔(elevated(4)) + surface_1 底 + border_strong + LG
        // 圆角;居中层 flex 定位,出场/收场的 8px 沉浮用外边距表达
        // (rise > 0 = 面板自下方 8px 上浮落位;收场对称沉回)
        let panel_w = {
            let vp_w = f32::from(window.viewport_size().width);
            DIALOG_WIDTH_PX.min((vp_w - 2.0 * OVERLAY_MARGIN_PX as f32).max(0.0))
        };
        let panel_inner = v_flex()
            .w(px(panel_w))
            .px(px(DIALOG_PADDING_PX))
            .py(px(DIALOG_PADDING_PX))
            .gap(px(SpacingTokens::MD))
            .rounded(px(DIALOG_RADIUS_PX))
            .border_1()
            .border_color(colors.border_strong)
            .bg(state_layer(
                colors.surface_1,
                InteractState::Idle,
                colors.accent,
            ))
            .child(title)
            .child(body)
            .child(buttons);

        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let panel = elevated(4, panel_inner)
            .mt(px(rise))
            .opacity(opacity)
            .track_focus(&focus)
            .on_key_down(cx.listener(Self::on_key_down));

        div()
            .absolute()
            .inset_0()
            .child(scrim)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(panel),
            )
            .into_any_element()
    }
}

/// 帧泵裁决:宿主是否需要续帧(与 `tooltip_host_needs_frame` 同款契约)。
#[must_use]
pub fn dialog_host_needs_frame(host: &DialogHost) -> bool {
    host.needs_frame()
}

// ---------------------------------------------------------------------------
// 工具(f64 → f32 收口,button.rs 同款惯例)
// ---------------------------------------------------------------------------

#[allow(clippy::cast_possible_truncation)]
fn f32v(v: f64) -> f32 {
    v as f32
}

// ---------------------------------------------------------------------------
// TC-CMP-DLG-01(Esc/Enter 接线状态机 + 焦点 trap 纯函数 + 动画相位)
// TC-CMP-DLG-02(reduced_motion 直切)
// 全部纯函数断言,不经 GUI、不触全局开关(reduced 以参数显式注入)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::button::ButtonVariant;
    use crate::tokens::ColorTokens;

    fn sample_spec() -> DialogSpec {
        DialogSpec::new("删除图层?", |_win, _cx| div().into_any_element())
            .button(DialogButton::cancel("取消"))
            .button(DialogButton::confirm("删除"))
    }

    // —— TC-CMP-DLG-01:Esc/Enter 接线状态机 ——

    #[test]
    fn tc_cmp_dlg_01_key_wiring_escape_enter() {
        // 有确认钮:Esc=取消、Enter=确认
        assert_eq!(dialog_key("escape", true), DialogKeyIntent::Cancel);
        assert_eq!(dialog_key("enter", true), DialogKeyIntent::Confirm);
        // 无确认钮:Enter 不凭空确认(无意图)
        assert_eq!(dialog_key("enter", false), DialogKeyIntent::None);
        // Esc 永远取消(与确认钮存在无关)
        assert_eq!(dialog_key("escape", false), DialogKeyIntent::Cancel);
        // Tab 拦截为焦点 trap;其余键无意图
        assert_eq!(dialog_key("tab", true), DialogKeyIntent::TrapFocus);
        for key in ["space", "a", "up", "down", "left", "right"] {
            assert_eq!(dialog_key(key, true), DialogKeyIntent::None, "{key}");
        }
        // spec 派生的 has_confirm/has_cancel
        let spec = sample_spec();
        assert!(spec.has_confirm());
        assert!(spec.has_cancel());
        assert!(first_button_of_kind(&spec.buttons, DialogButtonKind::Confirm) == Some(1));
        assert!(first_button_of_kind(&spec.buttons, DialogButtonKind::Cancel) == Some(0));
        // 只有取消钮:Enter 无意图,Esc 仍取消
        let cancel_only = DialogSpec::new("提示?", |_w, _cx| div().into_any_element())
            .button(DialogButton::cancel("知道了"));
        assert!(!cancel_only.has_confirm());
        assert_eq!(
            dialog_key("enter", cancel_only.has_confirm()),
            DialogKeyIntent::None
        );
        // 完全无按钮:两键都不接业务,关闭仍由宿主 begin_close 兜底
        let bare = DialogSpec::new("提示", |_w, _cx| div().into_any_element());
        assert!(!bare.has_confirm() && !bare.has_cancel());
    }

    #[test]
    fn tc_cmp_dlg_01_button_kinds_and_variants() {
        // kind → 缺省变体(渲染层映射的同一规则)
        assert_eq!(DialogButton::confirm("x").kind, DialogButtonKind::Confirm);
        assert_eq!(
            DialogButton::confirm("x").variant,
            Some(ButtonVariant::Primary)
        );
        assert_eq!(
            DialogButton::cancel("x").variant,
            Some(ButtonVariant::Secondary)
        );
        assert_eq!(
            DialogButton::neutral("x", ButtonVariant::Danger).kind,
            DialogButtonKind::Neutral
        );
        // 第一枚语义钮解析(多枚同类取先)
        let spec = DialogSpec::new("t", |_w, _cx| div().into_any_element())
            .button(DialogButton::neutral("详情", ButtonVariant::Ghost))
            .button(DialogButton::confirm("好"))
            .button(DialogButton::confirm("都好"))
            .button(DialogButton::cancel("不"));
        assert_eq!(
            first_button_of_kind(&spec.buttons, DialogButtonKind::Confirm),
            Some(1),
            "多枚确认取第一枚"
        );
        assert_eq!(
            first_button_of_kind(&spec.buttons, DialogButtonKind::Neutral),
            Some(0)
        );
        assert_eq!(
            first_button_of_kind(&spec.buttons, DialogButtonKind::Cancel),
            Some(3)
        );
        assert_eq!(
            first_button_of_kind(&[], DialogButtonKind::Confirm),
            None,
            "空按钮表"
        );
        // 回调存位
        let with_cb = DialogButton::confirm("删除").on_press(|_w, _cx| {});
        assert!(with_cb.on_press.is_some());
    }

    // —— TC-CMP-DLG-01:焦点 trap 纯函数 ——

    #[test]
    fn tc_cmp_dlg_01_focus_trap_wraps_within_dialog() {
        // 前进:到末端回绕到 0(trap 语义:焦点不出子树)
        assert_eq!(focus_trap_next(0, 3, true), 1);
        assert_eq!(focus_trap_next(1, 3, true), 2);
        assert_eq!(focus_trap_next(2, 3, true), 0, "末端回绕");
        // 后退:到顶端回绕到末端
        assert_eq!(focus_trap_next(2, 3, false), 1);
        assert_eq!(focus_trap_next(0, 3, false), 2, "顶端回绕");
        // 单停靠点:前进/后退都原地(trap 退化为恒聚焦面板根)
        assert_eq!(focus_trap_next(0, 1, true), 0);
        assert_eq!(focus_trap_next(0, 1, false), 0);
        // 空停靠点与越界钳制(防御,不 panic):越界钳到末端 2,前进回绕 0、后退到 1
        assert_eq!(focus_trap_next(0, 0, true), 0);
        assert_eq!(focus_trap_next(9, 3, true), 0, "越界钳末端,前进回绕");
        assert_eq!(focus_trap_next(9, 3, false), 1, "越界钳末端,后退一步");
    }

    // —— TC-CMP-DLG-01:出场/收场动画相位(STATE 120ms + 8px 上浮) ——

    #[test]
    fn tc_cmp_dlg_01_anim_phases_rise_and_fade() {
        let t0 = 1000.0;
        let mut clock = DialogClock::new();
        assert_eq!(clock.phase(), DialogPhase::Hidden);
        assert!(clock.is_settled());
        assert_eq!(clock.opacity_at(t0, false), 0.0);

        // 出场 120ms:起点沉 8px、透明 0;中程上浮中、透明中;OutCubic 快出
        clock.show(t0);
        assert_eq!(clock.phase(), DialogPhase::Showing);
        assert!(!clock.is_settled(), "出场中需要续帧");
        assert_eq!(
            DIALOG_ANIM_MS,
            MotionTokens::DUR_STATE_MS,
            "出场 = STATE 档"
        );
        assert!((clock.rise_offset_at(t0, false) - f64::from(DIALOG_RISE_PX)).abs() < 1e-9);
        assert_eq!(clock.opacity_at(t0, false), 0.0);
        let mid = clock.rise_offset_at(t0 + DIALOG_ANIM_MS / 2.0, false);
        assert!(
            mid > 0.0 && mid < f64::from(DIALOG_RISE_PX),
            "出场中程上浮 {mid}"
        );
        assert!(mid < f64::from(DIALOG_RISE_PX) / 2.0, "OutCubic 快出:{mid}");
        let mid_opacity = clock.opacity_at(t0 + DIALOG_ANIM_MS / 2.0, false);
        assert!(mid_opacity > 0.0 && mid_opacity < 1.0);
        // 120ms 到点:Open、位移 0、透明 1、静止(停帧)
        assert_eq!(
            clock.phase_at(t0 + DIALOG_ANIM_MS, false),
            DialogPhase::Open
        );
        assert_eq!(clock.rise_offset_at(t0 + DIALOG_ANIM_MS, false), 0.0);
        assert_eq!(clock.opacity_at(t0 + DIALOG_ANIM_MS, false), 1.0);
        assert!(clock.is_settled(), "打开态静止");

        // 收场对称:起点位移 0、透明 1;终点沉回 8px、透明 0、Hidden
        let tc = t0 + 2000.0;
        clock.close(tc);
        assert_eq!(clock.phase(), DialogPhase::Hiding);
        assert!(!clock.is_settled());
        assert_eq!(clock.rise_offset_at(tc, false), 0.0, "收场起点位移 0(对称)");
        let out_mid = clock.rise_offset_at(tc + DIALOG_ANIM_MS / 2.0, false);
        assert!(out_mid > 0.0 && out_mid < f64::from(DIALOG_RISE_PX));
        assert_eq!(
            clock.phase_at(tc + DIALOG_ANIM_MS, false),
            DialogPhase::Hidden
        );
        assert_eq!(clock.opacity_at(tc + DIALOG_ANIM_MS, false), 0.0);
        assert!(clock.is_settled());
        // Hidden 后 close 幂等;收场中重 show 反向重入;非有限时间戳忽略
        clock.close(tc + 9999.0);
        assert_eq!(clock.phase(), DialogPhase::Hidden);
        clock.show(t0 + 3000.0);
        clock.close(t0 + 3010.0);
        clock.show(t0 + 3020.0);
        assert_eq!(clock.phase(), DialogPhase::Showing, "收场中重开回出场");
        let mut fresh = DialogClock::new();
        fresh.show(f64::NAN);
        assert_eq!(fresh.phase(), DialogPhase::Hidden);
        fresh.show(t0);
        fresh.close(f64::INFINITY);
        assert_eq!(fresh.phase(), DialogPhase::Showing);
    }

    #[test]
    fn tc_cmp_dlg_01_scrim_and_panel_tokens() {
        // 背板材质 = 共享单点(本文件消费 toast::overlay_scrim 与
        // toast::OVERLAY_SCRIM_OPACITY,零第二份定义;定值与令牌逐位断言在
        // toast.rs 的 TC-CMP-TOAST-01)
        assert_eq!(crate::controls::toast::OVERLAY_SCRIM_OPACITY, 0.5);
        for colors in [ColorTokens::dark(), ColorTokens::light()] {
            let scrim = gpui::Hsla {
                a: crate::controls::toast::OVERLAY_SCRIM_OPACITY,
                ..colors.surface_0
            };
            assert_eq!(scrim.h, colors.surface_0.h, "背板色相来自 surface_0 令牌");
        }
        // 面板几何:宽 4 网格、圆角/内边距 = 令牌、上浮 = select 8px 单源
        assert_eq!(DIALOG_WIDTH_PX, 420.0);
        assert_eq!(DIALOG_RADIUS_PX, RadiusTokens::LG);
        assert_eq!(DIALOG_PADDING_PX, SpacingTokens::LG);
        assert_eq!(DIALOG_RISE_PX, crate::controls::select::MENU_OPEN_OFFSET_PX);
        // L4 海拔 = ELEVATIONS[4](对话框档)
        assert_eq!(crate::theme::shadow(4), crate::tokens::ELEVATIONS[4]);
        assert_eq!(crate::tokens::ELEVATIONS[4].blur, 32.0);
    }

    #[test]
    fn tc_cmp_dlg_01_spec_builder_and_host_shape() {
        // builder 链式追加;按钮回调存位
        let spec = sample_spec();
        assert_eq!(spec.title, "删除图层?");
        assert_eq!(spec.buttons.len(), 2);
        assert!(spec.body.is_some());
        let host = DialogHost::new();
        assert_eq!(host.clock().phase(), DialogPhase::Hidden);
        assert!(!host.is_open());
        assert!(!dialog_host_needs_frame(&host), "隐藏态不续帧");
        assert!(host.spec().is_none());
        // 语义:label 缺省回落标题(有 spec 时)/ 无 spec 时无 label;
        // role 缺省 Group(无 Dialog 变体,宿主可覆写)
        assert!(host.resolved_semantic().label().is_none());
        assert_eq!(host.resolved_semantic().role(), Some(SemanticRole::Group));
        let named = DialogHost::new()
            .label("导出确认")
            .role(SemanticRole::Group);
        assert_eq!(
            named.resolved_semantic().label().map(|s| s.as_ref()),
            Some("导出确认")
        );
    }

    // —— TC-CMP-DLG-02:reduced_motion 直切 ——

    #[test]
    fn tc_cmp_dlg_02_reduced_motion_cuts_directly() {
        let t0 = 1000.0;
        let mut clock = DialogClock::new();
        clock.show(t0);
        // 出场窗口内任意时刻直切 Open:透明 1、零位移
        for t in [t0 + 1.0, t0 + 60.0, t0 + DIALOG_ANIM_MS] {
            assert_eq!(clock.phase_at(t, true), DialogPhase::Open, "t={t}");
            assert_eq!(clock.opacity_at(t, true), 1.0, "t={t}");
            assert_eq!(clock.rise_offset_at(t, true), 0.0, "无位移");
        }
        assert!(clock.is_settled(), "减弱动态:出场即静止");
        // 收场直切 Hidden:透明 0
        clock.close(t0 + 100.0);
        for t in [t0 + 100.0, t0 + 160.0, t0 + 300.0] {
            assert_eq!(clock.phase_at(t, true), DialogPhase::Hidden, "t={t}");
            assert_eq!(clock.opacity_at(t, true), 0.0, "t={t}");
        }
        // 全程无中间灰度:出场/收场窗口密集采样只有 0/1
        let mut clock2 = DialogClock::new();
        clock2.show(t0);
        for step in 0..=120 {
            let t = t0 + f64::from(step);
            let opacity = clock2.opacity_at(t, true);
            assert!(
                opacity == 0.0 || opacity == 1.0,
                "减弱动态不得出现中间灰度:t={t} opacity={opacity}"
            );
            clock2.phase_at(t, true);
        }
        clock2.close(t0 + 500.0);
        for step in 0..=120 {
            let opacity = clock2.opacity_at(t0 + 500.0 + f64::from(step), true);
            assert!(opacity == 0.0 || opacity == 1.0);
        }
    }
}
