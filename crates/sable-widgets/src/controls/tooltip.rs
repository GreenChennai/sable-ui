//! Tooltip 浮层 + 键位徽章文案单源(迭代审查报告 2026-10-04 CMP-02,§5.6 组件
//! 矩阵 #7、§5.5)。
//!
//! # 语义来源(只对齐语义,零源码复制,ACL-1.0 红线)
//!
//! 上游 VellumBench `key_badge_text`(docs/upstream/02 §4.2:components.rs
//! :102-108)是「名称 (键位)」的**唯一写法**,tooltip/菜单/命令面板共用;
//! ToolButton tooltip 强制「名称 (快捷键)」零成本学习。本模块以纯 gpui 落地
//! 同一语义:[`key_badge_text`] 是文案单源,快捷键**解析对齐 gpui registry**
//! ([`gpui::Keystroke::parse`]:`ctrl-k`/`shift-enter`/`secondary-k`/`cmd-k`
//! 与 keymap 绑定同一种写法),**渲染用惯用键名**(`Ctrl+K`/`Shift+Enter`)。
//!
//! # 三层 API
//!
//! - **文案单源**:[`key_badge_text`] / [`render_shortcut`] /
//!   [`format_keystroke`]——纯函数,菜单/命令面板(批 2)直接复用;
//! - **内容构造**:[`tooltip_view`](L4 材质:`theme::elevated(4)` 阴影 +
//!   92% 不透明表面 + caption 字号)与 [`tooltip_slot`]——宿主把
//!   [`IconButton`](crate::controls::button::IconButton) 的 gpui 原生
//!   tooltip 槽一行接上:
//!   `.tooltip(tooltip_slot(TooltipSpec::new("导出").with_shortcut("ctrl-e")))`
//!   (不改 button.rs 的任何签名);
//! - **Entity 形态**:[`TooltipHost`] 挂在宿主根,程序化 show/hide,延迟/
//!   淡入/边界避让全套生效;content provider 协议
//!   ([`TooltipContentFn`])允许宿主替换内容渲染(默认 = [`tooltip_view`])。
//!
//! # 时序规格(§5.6 #7)
//!
//! 延迟 [`TOOLTIP_DELAY_MS`](400ms)出现 → HOVER 档
//! ([`MotionTokens::DUR_HOVER_MS`],80ms)淡入;隐藏淡出同档。
//! 状态机 [`TooltipClock`] 纯函数、时间显式注入(`now_ms`),reduced_motion
//! 直入直出(调用方显式传 `reduced`,无全局读取——纯函数可测、无竞态)。
//!
//! # 边界避让(纯函数,可测)
//!
//! [`place_tooltip`]:anchor 触发区 + tooltip 尺寸 + 视口 → 落位矩形。
//! 垂直向偏好侧放不下**翻边**、两侧都放不下钳在视口内;水平向中心对齐
//! anchor 并钳制。四边行为 = TC-CMP-TIP-01 的断言面。
//!
//! # v0.1 已知限制(如实声明)
//!
//! [`estimated_tooltip_size`] 以字符数 × 安全字宽估计浮层尺寸(真字体度量
//! 需布局后才知道,GPUI 无同步测量口);估计**偏宽**(CJK 安全方向),
//! 极端长文案下水平钳制可能有像素级偏差,垂直翻边(高度 = 单行精确值)
//! 不受影响。

use std::rc::Rc;

use gpui::{
    AnyElement, AnyView, App, AppContext as _, Context, Hsla, IntoElement, Keystroke,
    ParentElement, Render, SharedString, Styled, Window, div, px,
};
use kurbo::{Rect, Size};

use crate::anim::reduced_motion;
use crate::interact::now_ms;
use crate::theme::{elevated, theme};
use crate::tokens::{MotionTokens, RadiusTokens, SpacingTokens, TextSize, UI_FONT};

// ---------------------------------------------------------------------------
// 常量域(几何/时序具名常量;颜色一律令牌,零字面量)
// ---------------------------------------------------------------------------

/// 延迟出现时长(§5.6 #7 规格:400ms;动效令牌表尚无此档,先落组件具名
/// 常量,ANI-06 弹簧 token 化同轮可把它提升进 `MotionTokens`)。
pub const TOOLTIP_DELAY_MS: f64 = 400.0;
/// 淡入/淡出时长 = 动效四档的 HOVER 档(80ms,§5.6 #7"HOVER 80ms 淡入")。
pub const TOOLTIP_FADE_MS: f64 = MotionTokens::DUR_HOVER_MS;
/// L4 提示表面不透明度(报告 §5.3.2 材质阶梯:L4 = N7 92% 不透明)。
pub const L4_SURFACE_OPACITY: f32 = 0.92;
/// anchor 与浮层的间距 = 间距令牌 XS(4px,4 网格纪律)。
pub const TOOLTIP_GAP_PX: f64 = SpacingTokens::XS as f64;
/// 浮层距视口边的安全边距 = 间距令牌 SM(8px)。
pub const VIEWPORT_MARGIN_PX: f64 = SpacingTokens::SM as f64;
/// 浮层最大宽(280px,4 网格;长文案换行,不横跨整个视口)。
pub const TOOLTIP_MAX_WIDTH_PX: f32 = 280.0;
/// 尺寸估计的单字符安全字宽(CJK caption 11px 下的偏宽估计,v0.1 限制见
/// 模块 doc)。
const ESTIMATED_CHAR_WIDTH_PX: f64 = 12.0;

/// 平台键名(gpui `Modifiers::platform` 的惯用展示名,随编译目标平台):
/// mac = Cmd / windows = Win / 其余 = Super——与 gpui `display_modifiers`
/// 的 ⌘ / ⊞ / ❖ 三符号一一对应。
#[cfg(target_os = "macos")]
const PLATFORM_KEY_NAME: &str = "Cmd";
/// 平台键名(Windows)。
#[cfg(target_os = "windows")]
const PLATFORM_KEY_NAME: &str = "Win";
/// 平台键名(Linux/其他)。
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const PLATFORM_KEY_NAME: &str = "Super";

/// 常用命名键的惯用展示名(未列出的键取首字母大写,如 `f5` → `F5`)。
const PRETTY_KEY_NAMES: &[(&str, &str)] = &[
    ("return", "Enter"),
    ("enter", "Enter"),
    ("escape", "Esc"),
    ("tab", "Tab"),
    ("space", "Space"),
    ("backspace", "Backspace"),
    ("delete", "Del"),
    ("insert", "Ins"),
    ("pageup", "Page Up"),
    ("pagedown", "Page Down"),
    ("home", "Home"),
    ("end", "End"),
    ("up", "Up"),
    ("down", "Down"),
    ("left", "Left"),
    ("right", "Right"),
    ("minus", "-"),
    ("plus", "+"),
    ("equal", "="),
];

// ---------------------------------------------------------------------------
// 键位徽章文案单源(§5.5;上游 key_badge_text 语义,零源码复制)
// ---------------------------------------------------------------------------

/// 键位徽章文案(**唯一写法**,§5.5/CMP-02):`名称 (快捷键)`;无快捷键
/// 只名称;名为空只剩快捷键文本(不带括号);两者皆空得空串。
///
/// `shortcut` 是 **gpui keystroke 语法**(与 keymap 绑定同源,可空格分隔
/// 多段 chord,如 `"ctrl-k ctrl-c"`);解析失败的旧式串(如 `"Mod+E"`)
/// 整串原样兜底,不丢文案。
#[must_use]
pub fn key_badge_text(name: &str, shortcut: Option<&str>) -> String {
    let name = name.trim();
    let shortcut = shortcut.map(str::trim).filter(|s| !s.is_empty());
    match (name.is_empty(), shortcut) {
        (true, None) => String::new(),
        (true, Some(keys)) => render_shortcut(keys),
        (false, None) => name.to_string(),
        (false, Some(keys)) => format!("{name} ({})", render_shortcut(keys)),
    }
}

/// 快捷键串 → 惯用键名(纯函数):空格分隔的多段 chord 逐段经
/// [`Keystroke::parse`](gpui registry 同款解析)后以惯用名连接;`+` 不是
/// gpui keystroke 语法(分隔符是 `-`),含 `+` 的人写惯用形(`Ctrl+K`、
/// 旧式 `Mod+E`)先归一为 `-` 再解析;任一段解析失败则整串原样返回
/// (兜底不丢信息)。
#[must_use]
pub fn render_shortcut(spec: &str) -> String {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut rendered: Vec<String> = Vec::new();
    for token in trimmed.split_ascii_whitespace() {
        let normalized = token.replace('+', "-");
        match Keystroke::parse(&normalized) {
            Ok(keystroke) => rendered.push(format_keystroke(&keystroke)),
            Err(_) => return trimmed.to_string(),
        }
    }
    rendered.join(" ")
}

/// 单个 keystroke → 惯用键名(纯函数):修饰键序 = Fn / Ctrl / Alt / 平台键
/// / Shift(gpui `Display` 的输出序,`Fn` 同 `unparse` 置首),`+` 连接;
/// 单字符键大写(`k` → `K`),命名键取惯用名([`PRETTY_KEY_NAMES`],未列出的
/// 首字母大写)。
#[must_use]
pub fn format_keystroke(keystroke: &Keystroke) -> String {
    let m = &keystroke.modifiers;
    let mut out = String::new();
    if m.function {
        out.push_str("Fn+");
    }
    if m.control {
        out.push_str("Ctrl+");
    }
    if m.alt {
        out.push_str("Alt+");
    }
    if m.platform {
        out.push_str(PLATFORM_KEY_NAME);
        out.push('+');
    }
    if m.shift {
        out.push_str("Shift+");
    }
    out.push_str(&display_key(&keystroke.key));
    out
}

/// 键名展示(纯函数):单字符大写;命名键查表([`PRETTY_KEY_NAMES`]),
/// 未列出者首字母大写(`f5` → `F5`、`shift` → `Shift`)。
#[must_use]
fn display_key(key: &str) -> String {
    for (name, pretty) in PRETTY_KEY_NAMES {
        if *name == key {
            return (*pretty).to_string();
        }
    }
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(first), None) => first.to_ascii_uppercase().to_string(),
        (Some(first), Some(_)) => {
            first.to_uppercase().collect::<String>() + &key[first.len_utf8()..]
        }
        (None, _) => String::new(),
    }
}

// ---------------------------------------------------------------------------
// 显隐状态机(纯函数;TC-CMP-TIP-01/02 的被测单点)
// ---------------------------------------------------------------------------

/// Tooltip 相位(纯状态机,时间显式注入)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TooltipPhase {
    /// 隐藏(静止态,零帧)
    #[default]
    Hidden,
    /// 延迟等待中(400ms 内,尚不可见)
    Delaying,
    /// 淡入中(80ms)
    Showing,
    /// 完全可见(静止态,零帧)
    Visible,
    /// 淡出中(80ms)
    Hiding,
}

/// Tooltip 显隐时钟(纯状态机):`show` 进入 400ms 延迟 → 80ms 淡入 →
/// 可见;`hide` → 80ms 淡出 → 隐藏(延迟期直接取消,不淡出)。
///
/// [`phase_at`](读取即推进,沉降式)与 [`Self::opacity_at`] 都收显式
/// `reduced` 参数(reduced_motion **直入直出**:相位立即落终态、不透明度
/// 只有 0/1),调用方在组件边界传 `reduced_motion()`,纯函数不读全局态。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TooltipClock {
    phase: TooltipPhase,
    started_ms: f64,
}

impl TooltipClock {
    /// 隐藏态新时钟。
    pub fn new() -> Self {
        TooltipClock::default()
    }

    /// 当前相位(不推进;渲染帧间的快照)。
    #[must_use]
    pub fn phase(&self) -> TooltipPhase {
        self.phase
    }

    /// 是否处于静止态(Hidden/Visible):为真时宿主无需续帧。
    #[must_use]
    pub fn is_settled(&self) -> bool {
        matches!(self.phase, TooltipPhase::Hidden | TooltipPhase::Visible)
    }

    /// 请求显示(指针进入):进入 400ms 延迟期(重复调用即重置计时)。
    /// 非有限时间戳忽略(防御,不 panic)。
    pub fn show(&mut self, now_ms: f64) {
        if !now_ms.is_finite() {
            return;
        }
        self.phase = TooltipPhase::Delaying;
        self.started_ms = now_ms;
    }

    /// 请求隐藏(指针离开):可见/淡入中 → 80ms 淡出;延迟期 → 直接取消。
    pub fn hide(&mut self, now_ms: f64) {
        if !now_ms.is_finite() {
            return;
        }
        self.phase = match self.phase {
            TooltipPhase::Visible | TooltipPhase::Showing => TooltipPhase::Hiding,
            _ => TooltipPhase::Hidden,
        };
        self.started_ms = now_ms;
    }

    /// 推进并返回当前相位(沉降式:到点自动迁移,读取即清理)。
    /// `reduced` 为真:任何中间相位立即落终态(直入直出)。
    pub fn phase_at(&mut self, now_ms: f64, reduced: bool) -> TooltipPhase {
        if !now_ms.is_finite() {
            return self.phase;
        }
        let elapsed = now_ms - self.started_ms;
        self.phase = match self.phase {
            TooltipPhase::Delaying => {
                if reduced {
                    TooltipPhase::Visible
                } else if elapsed >= TOOLTIP_DELAY_MS {
                    // 淡入窗口自延迟期满起算(重定起始时刻)
                    self.started_ms = now_ms;
                    TooltipPhase::Showing
                } else {
                    TooltipPhase::Delaying
                }
            }
            TooltipPhase::Showing => {
                if reduced || elapsed >= TOOLTIP_FADE_MS {
                    TooltipPhase::Visible
                } else {
                    TooltipPhase::Showing
                }
            }
            TooltipPhase::Hiding => {
                if reduced || elapsed >= TOOLTIP_FADE_MS {
                    TooltipPhase::Hidden
                } else {
                    TooltipPhase::Hiding
                }
            }
            settled => settled,
        };
        self.phase
    }

    /// 不透明度 0..1(线性淡变;`reduced` 直入直出,只有 0/1 两值)。
    pub fn opacity_at(&mut self, now_ms: f64, reduced: bool) -> f64 {
        match self.phase_at(now_ms, reduced) {
            TooltipPhase::Hidden | TooltipPhase::Delaying => 0.0,
            TooltipPhase::Visible => 1.0,
            TooltipPhase::Showing => ((now_ms - self.started_ms) / TOOLTIP_FADE_MS).clamp(0.0, 1.0),
            TooltipPhase::Hiding => {
                1.0 - ((now_ms - self.started_ms) / TOOLTIP_FADE_MS).clamp(0.0, 1.0)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 边界避让(纯函数,几何;TC-CMP-TIP-01 四边断言面)
// ---------------------------------------------------------------------------

/// Tooltip 落位(纯函数):anchor 触发区 + 浮层尺寸 + 视口 → 落位矩形
/// (左上角原点坐标系,px)。
///
/// - **垂直**:偏好侧(`prefer_below` = anchor 下方)放得下即落偏好侧;
///   放不下**翻边**;两侧都放不下取剩余空间多的一侧并钳在视口边距内;
/// - **水平**:中心对齐 anchor,钳制在 `[视口左+边距, 视口右-边距-宽]`
///   (浮层宽于视口时钉在左边距,不收缩)。
///
/// 参数非有限(NaN)时返回视口左上角零尺寸矩形(防御,不 panic)。
#[must_use]
pub fn place_tooltip(
    anchor: Rect,
    tip: Size,
    viewport: Rect,
    gap: f64,
    prefer_below: bool,
) -> Rect {
    if !(anchor.is_finite() && tip.is_finite() && viewport.is_finite()) {
        return Rect::new(viewport.x0, viewport.y0, viewport.x0, viewport.y0);
    }
    let w = tip.width.max(0.0);
    let h = tip.height.max(0.0);
    let lower = viewport.y0 + VIEWPORT_MARGIN_PX;
    let upper = viewport.y1 - VIEWPORT_MARGIN_PX;

    let below_y = anchor.y1 + gap;
    let above_y = anchor.y0 - gap - h;
    let below_fits = below_y + h <= upper;
    let above_fits = above_y >= lower;
    let y0 = if prefer_below {
        if below_fits {
            below_y
        } else if above_fits {
            above_y
        } else {
            cramped_vertical(below_y, above_y, lower, upper, h)
        }
    } else if above_fits {
        above_y
    } else if below_fits {
        below_y
    } else {
        cramped_vertical(below_y, above_y, lower, upper, h)
    };

    // 水平:中心对齐 + 钳制(clamp 上界取两者较大者,视口窄于浮层时不回卷)
    let center_x = (anchor.x0 + anchor.x1) / 2.0;
    let x_low = viewport.x0 + VIEWPORT_MARGIN_PX;
    let x_high = (viewport.x1 - VIEWPORT_MARGIN_PX - w).max(x_low);
    let x0 = (center_x - w / 2.0).clamp(x_low, x_high);

    Rect::new(x0, y0, x0 + w, y0 + h)
}

/// 两侧都放不下的垂直裁决:剩余空间多的一侧 + 钳在视口边距内(纯函数,
/// [`place_tooltip`] 的分支单点)。
#[must_use]
fn cramped_vertical(below_y: f64, above_y: f64, lower: f64, upper: f64, h: f64) -> f64 {
    let room_below = upper - below_y;
    let room_above = above_y - lower;
    if room_below >= room_above {
        (upper - h).max(lower)
    } else {
        lower
    }
}

/// 浮层尺寸估计(纯函数,v0.1 限制见模块 doc):宽 = 文案/徽章较长者的
/// 字符数 × 安全字宽 + 2×SM 内边距(钳在 [`TOOLTIP_MAX_WIDTH_PX`]),高 =
/// caption 行高 + 2×XS 内边距(单行精确值)。
#[must_use]
pub fn estimated_tooltip_size(label: &str, shortcut: Option<&str>) -> Size {
    let badge = shortcut.map(str::trim).unwrap_or("").chars().count();
    let text_len = label.trim().chars().count().max(badge);
    let width = ((text_len as f64 * ESTIMATED_CHAR_WIDTH_PX) + 2.0 * f64::from(SpacingTokens::SM))
        .min(f64::from(TOOLTIP_MAX_WIDTH_PX));
    let height = f64::from(TextSize::CAPTION.line_height) + 2.0 * f64::from(SpacingTokens::XS);
    Size::new(width, height)
}

// ---------------------------------------------------------------------------
// 内容构造 + IconButton 桥(宿主一行接上,不改 button.rs 签名)
// ---------------------------------------------------------------------------

/// Tooltip 内容规格(名称 + 可选快捷键;[`IconButton`] 的
/// `tooltip_label()`/`tooltip_shortcut_text()` 即本结构的两槽)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TooltipSpec {
    /// 名称(主文案)
    pub label: SharedString,
    /// 快捷键(gpui keystroke 语法;`None`/空 = 只名称)
    pub shortcut: Option<SharedString>,
}

impl TooltipSpec {
    /// 只有名称的规格。
    pub fn new(label: impl Into<SharedString>) -> Self {
        TooltipSpec {
            label: label.into(),
            shortcut: None,
        }
    }

    /// 带快捷键的规格(键位徽章 = [`key_badge_text`] 单源)。
    #[must_use]
    pub fn with_shortcut(mut self, keys: impl Into<SharedString>) -> Self {
        let keys = keys.into();
        self.shortcut = if keys.trim().is_empty() {
            None
        } else {
            Some(keys)
        };
        self
    }

    /// 「名称 (快捷键)」文案([`key_badge_text`] 的方法形态)。
    #[must_use]
    pub fn key_badge_text(&self) -> String {
        key_badge_text(
            self.label.as_ref(),
            self.shortcut.as_ref().map(SharedString::as_ref),
        )
    }
}

/// Tooltip 内容构造(L4 材质,§5.6 #7 / 报告 §5.3.2):`theme::elevated(4)`
/// 垫 L4 海拔阴影(`shadow_quads(4)`)+ **92% 不透明** L4 表面 + 细描边 +
/// caption 字号 + 「名称 (快捷键)」单源文案。宿主/浮层系统直接取用。
#[must_use]
pub fn tooltip_view(spec: &TooltipSpec, cx: &App) -> AnyElement {
    let colors = &theme(cx).colors;
    // L4 表面 = surface_4(N7)@ 92% 不透明;alpha 覆写不是颜色字面量
    let surface = Hsla {
        a: L4_SURFACE_OPACITY,
        ..colors.surface_4
    };
    let body = div()
        .max_w(px(TOOLTIP_MAX_WIDTH_PX))
        .px(px(SpacingTokens::SM))
        .py(px(SpacingTokens::XS))
        .rounded(px(RadiusTokens::MD))
        .bg(surface)
        .border_1()
        .border_color(colors.border_strong)
        .font_family(UI_FONT)
        .text_size(px(TextSize::CAPTION.size))
        .text_color(colors.text_strong)
        .child(spec.key_badge_text());
    // L4 海拔阴影垫底(elevated 要求内容近不透明;92% 为规格定值)
    elevated(4, body).into_any_element()
}

/// gpui 原生 tooltip 槽的视图壳(渲染 [`tooltip_view`])。
struct TooltipSlotView {
    spec: TooltipSpec,
}

impl Render for TooltipSlotView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tooltip_view(&self.spec, cx)
    }
}

/// **一行接入** gpui 原生 tooltip 槽的桥(返回值直接喂 `.tooltip(..)`,
/// 与 `IconButton` 现有槽签名吻合,不改 button.rs):
///
/// ```ignore
/// use sable_widgets::controls::tooltip::{TooltipSpec, tooltip_slot};
/// // 宿主侧一行接上 IconButton 的 gpui 原生 tooltip 槽:
/// IconButton::new("export", "⇪")
///     .tooltip(tooltip_slot(TooltipSpec::new("导出").with_shortcut("ctrl-e")))
/// ```
pub fn tooltip_slot(spec: TooltipSpec) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_window, cx| cx.new(|_| TooltipSlotView { spec: spec.clone() }).into()
}

// ---------------------------------------------------------------------------
// TooltipHost(Entity 形态,挂宿主根;content provider 协议)
// ---------------------------------------------------------------------------

/// 内容 provider 协议:宿主可整体替换 Tooltip 内容渲染(默认 =
/// [`tooltip_view`])。菜单/命令面板(批 2)复用本协议挂各自的浮层内容。
pub type TooltipContentFn = Rc<dyn Fn(&TooltipSpec, &mut Window, &mut App) -> AnyElement>;

/// Tooltip 宿主(Entity 形态):挂在视图根,程序化 `show`/`hide`,内部持有
/// [`TooltipClock`] 全套时序(400ms 延迟/80ms 淡入/reduced 直入直出)与
/// [`place_tooltip`] 边界避让,以绝对定位浮层渲染。
///
/// ```ignore
/// let host = cx.new(|_| TooltipHost::new());
/// // 指针进入某触发区时:
/// host.update(cx, |host, cx| {
///     host.show(TooltipSpec::new("导出").with_shortcut("ctrl-e"),
///               anchor_rect, viewport_rect, cx)
/// });
/// ```
pub struct TooltipHost {
    spec: Option<TooltipSpec>,
    anchor: Rect,
    viewport: Rect,
    clock: TooltipClock,
    provider: Option<TooltipContentFn>,
}

impl Default for TooltipHost {
    fn default() -> Self {
        Self::new()
    }
}

impl TooltipHost {
    /// 隐藏态新宿主。
    pub fn new() -> Self {
        TooltipHost {
            spec: None,
            anchor: Rect::ZERO,
            viewport: Rect::ZERO,
            clock: TooltipClock::new(),
            provider: None,
        }
    }

    /// 替换内容渲染(链式;缺省用内置 [`tooltip_view`])。
    #[must_use]
    pub fn with_content_provider(mut self, provider: TooltipContentFn) -> Self {
        self.provider = Some(provider);
        self
    }

    /// 显示请求:记规格/触发区/视口并启动延迟时钟(重复调用即重置)。
    pub fn show(
        &mut self,
        spec: TooltipSpec,
        anchor: Rect,
        viewport: Rect,
        cx: &mut Context<Self>,
    ) {
        self.spec = Some(spec);
        self.anchor = anchor;
        self.viewport = viewport;
        self.clock.show(now_ms());
        cx.notify();
    }

    /// 隐藏请求(指针离开触发区)。
    pub fn hide(&mut self, cx: &mut Context<Self>) {
        self.clock.hide(now_ms());
        cx.notify();
    }

    /// 显隐时钟(宿主/测试可读;组件边界外的纯状态)。
    #[must_use]
    pub fn clock(&self) -> &TooltipClock {
        &self.clock
    }
}

impl Render for TooltipHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = now_ms();
        let reduced = reduced_motion();
        let phase = self.clock.phase_at(now, reduced);
        // 隐藏/延迟期:空占位(浮层不可见,布局零影响)
        if matches!(phase, TooltipPhase::Hidden | TooltipPhase::Delaying) {
            return div().into_any_element();
        }
        let Some(spec) = self.spec.clone() else {
            return div().into_any_element();
        };
        let opacity = f32v(self.clock.opacity_at(now, reduced));
        let tip = estimated_tooltip_size(
            spec.label.as_ref(),
            spec.shortcut.as_ref().map(SharedString::as_ref),
        );
        let placed = place_tooltip(self.anchor, tip, self.viewport, TOOLTIP_GAP_PX, true);
        let content = match &self.provider {
            Some(provider) => provider(&spec, window, cx),
            None => tooltip_view(&spec, cx),
        };
        div()
            .absolute()
            .left(px(f32v(placed.x0)))
            .top(px(f32v(placed.y0)))
            .opacity(opacity)
            .child(content)
            .into_any_element()
    }
}

/// 帧泵裁决:时钟未落定(Delaying/Showing/Hiding)就续帧(静止零帧提交)。
/// 宿主在包住 [`TooltipHost`] 的视图里每帧调用(与 theme 过渡帧泵同款)。
#[must_use]
pub fn tooltip_host_needs_frame(host: &TooltipHost) -> bool {
    !host.clock.is_settled()
}

// ---------------------------------------------------------------------------
// 工具(f64 → f32 收口,button.rs 同款惯例)
// ---------------------------------------------------------------------------

#[allow(clippy::cast_possible_truncation)]
fn f32v(v: f64) -> f32 {
    v as f32
}

// ---------------------------------------------------------------------------
// TC-CMP-TIP-01(文案格式 + 边界避让四边 + 延迟/淡入状态机)
// TC-CMP-TIP-02(reduced_motion 直入直出)
// 全部纯函数断言,不经 GUI、不触全局开关(reduced 以参数显式注入)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_cmp_tip_01_key_badge_text_format() {
        // 有快捷键:「名称 (快捷键)」——解析对齐 gpui registry,渲染惯用键名
        assert_eq!(key_badge_text("导出", Some("ctrl-e")), "导出 (Ctrl+E)");
        assert_eq!(
            key_badge_text("命令面板", Some("ctrl-shift-p")),
            "命令面板 (Ctrl+Shift+P)"
        );
        assert_eq!(
            key_badge_text("换行", Some("shift-enter")),
            "换行 (Shift+Enter)"
        );
        // 无快捷键:只名称
        assert_eq!(key_badge_text("切换效果启用", None), "切换效果启用");
        // 空串/纯空白快捷键视同无快捷键
        assert_eq!(key_badge_text("删除", Some("")), "删除");
        assert_eq!(key_badge_text("删除", Some("   ")), "删除");
        // 空名:只剩快捷键文本(不带括号,不出「 (Ctrl+K)」残缺格式)
        assert_eq!(key_badge_text("", Some("ctrl-k")), "Ctrl+K");
        assert_eq!(key_badge_text("  ", Some("ctrl-k")), "Ctrl+K");
        // 两者皆空:空串
        assert_eq!(key_badge_text("", None), "");
        assert_eq!(key_badge_text("", Some("  ")), "");
        // 多段 chord(空格分隔)逐段渲染
        assert_eq!(
            key_badge_text("注释", Some("ctrl-k ctrl-c")),
            "注释 (Ctrl+K Ctrl+C)"
        );
        // 解析失败的旧式串:整串原样兜底,不丢文案
        assert_eq!(key_badge_text("导出", Some("Mod+E")), "导出 (Mod+E)");
        // secondary 前缀 = gpui 语义的"次级修饰键"(非 macOS = Ctrl)
        #[cfg(not(target_os = "macos"))]
        assert_eq!(key_badge_text("复制", Some("secondary-c")), "复制 (Ctrl+C)");
        // 命名键惯用名 + 命名键大写规则
        assert_eq!(render_shortcut("ctrl-pageup"), "Ctrl+Page Up");
        assert_eq!(render_shortcut("f5"), "F5");
        assert_eq!(render_shortcut("shift"), "Shift");
        // 空输入
        assert_eq!(render_shortcut(""), "");
        assert_eq!(render_shortcut("   "), "");
    }

    #[test]
    fn tc_cmp_tip_01_tooltip_spec_shares_single_source() {
        let spec = TooltipSpec::new("导出").with_shortcut("ctrl-e");
        assert_eq!(
            spec.key_badge_text(),
            key_badge_text("导出", Some("ctrl-e")),
            "TooltipSpec 的文案必须与自由函数单源"
        );
        assert_eq!(
            TooltipSpec::new("切换效果启用").key_badge_text(),
            "切换效果启用"
        );
        // 空白快捷键归一为 None
        assert_eq!(TooltipSpec::new("删除").with_shortcut("  ").shortcut, None);
    }

    #[test]
    fn tc_cmp_tip_01_place_tooltip_flips_and_clamps_all_edges() {
        let vp = Rect::new(0.0, 0.0, 800.0, 600.0);
        let tip = Size::new(160.0, 24.0);
        let gap = TOOLTIP_GAP_PX;
        let margin = VIEWPORT_MARGIN_PX;

        // 视口中部 + 偏好下方:落下方(y = anchor.y1 + gap),水平中心对齐
        let mid = Rect::new(300.0, 200.0, 340.0, 224.0);
        let r = place_tooltip(mid, tip, vp, gap, true);
        assert_eq!(r.y0, 224.0 + gap, "中部:下方落位");
        assert_eq!(r.x0 + r.width() / 2.0, 320.0, "水平中心对齐 anchor");

        // 底边:下方越界 → 翻上方(浮层底 = anchor.y0 - gap)
        let bottom = Rect::new(300.0, 570.0, 340.0, 595.0);
        let r = place_tooltip(bottom, tip, vp, gap, true);
        assert_eq!(r.y1, 570.0 - gap, "底边:翻边到上方");

        // 顶边 + 偏好上方:上方越界 → 翻下方(浮层顶 = anchor.y1 + gap)
        let top = Rect::new(300.0, 0.0, 340.0, 16.0);
        let r = place_tooltip(top, tip, vp, gap, false);
        assert_eq!(r.y0, 16.0 + gap, "顶边:翻边到下方");

        // 两侧都放不下:取空间多的一侧并钳在视口边距内(浮层高于视口时钉边)
        let small_vp = Rect::new(0.0, 0.0, 800.0, 30.0);
        let anchor = Rect::new(300.0, 10.0, 340.0, 20.0);
        let r = place_tooltip(anchor, tip, small_vp, gap, true);
        assert_eq!(r.y0, margin, "双侧越界:钉在视口上边距");
        assert_eq!(r.height(), tip.height, "垂直钳制不改浮层高");

        // 左边缘:水平钳制到左边距
        let left = Rect::new(2.0, 200.0, 30.0, 224.0);
        let r = place_tooltip(left, tip, vp, gap, true);
        assert_eq!(r.x0, margin, "左边缘钳制");

        // 右边缘:水平钳制到右边距
        let right = Rect::new(772.0, 200.0, 798.0, 224.0);
        let r = place_tooltip(right, tip, vp, gap, true);
        assert_eq!(r.x1, 800.0 - margin, "右边缘钳制");

        // 浮层宽于视口:钉在左边距,不回卷不 panic
        let wide = Size::new(1200.0, 24.0);
        let r = place_tooltip(mid, wide, vp, gap, true);
        assert_eq!(r.x0, margin, "超宽浮层钉左边距");
        // 浮层宽于视口:右侧溢出不可避免(钳制不收缩浮层),只保证钉在左边距

        // NaN 防御:返回视口左上角,不 panic
        let nan_anchor = Rect::new(f64::NAN, 0.0, 10.0, 10.0);
        let r = place_tooltip(nan_anchor, tip, vp, gap, true);
        assert_eq!(r, Rect::new(vp.x0, vp.y0, vp.x0, vp.y0));
    }

    #[test]
    fn tc_cmp_tip_01_delay_and_fade_state_machine() {
        let t0 = 1000.0;
        let mut clock = TooltipClock::new();
        assert_eq!(clock.phase(), TooltipPhase::Hidden);
        assert!(clock.is_settled(), "初始静止(Hidden)");
        assert_eq!(clock.opacity_at(t0, false), 0.0);

        // 延迟期:400ms 内不可见、不透明度 0
        clock.show(t0);
        assert_eq!(clock.phase(), TooltipPhase::Delaying);
        assert!(!clock.is_settled(), "延迟期需要续帧");
        assert_eq!(clock.phase_at(t0 + 399.0, false), TooltipPhase::Delaying);
        assert_eq!(clock.opacity_at(t0 + 399.0, false), 0.0, "延迟期不提前显形");

        // 400ms 到点进入淡入;80ms 内线性爬升;不透明度单调
        assert_eq!(
            clock.phase_at(t0 + TOOLTIP_DELAY_MS, false),
            TooltipPhase::Showing
        );
        let mut prev = 0.0;
        for step in 1..=3 {
            let opacity = clock.opacity_at(t0 + TOOLTIP_DELAY_MS + 20.0 * f64::from(step), false);
            assert!(opacity > prev, "淡入应单调增:{opacity} ≤ {prev}");
            assert!(opacity < 1.0, "淡入中不满透明:{opacity}");
            prev = opacity;
        }
        // 80ms 淡入完成 → Visible,不透明度精确 1,静止(停帧)
        assert_eq!(
            clock.phase_at(t0 + TOOLTIP_DELAY_MS + TOOLTIP_FADE_MS, false),
            TooltipPhase::Visible
        );
        assert_eq!(
            clock.opacity_at(t0 + TOOLTIP_DELAY_MS + TOOLTIP_FADE_MS, false),
            1.0
        );
        assert!(clock.is_settled(), "可见态静止(停帧)");
        assert_eq!(clock.opacity_at(t0 + 9_999.0, false), 1.0, "超时钳在 1");

        // 淡出:80ms 内线性回落,归零后 Hidden 且停帧
        clock.hide(t0 + 2_000.0);
        assert_eq!(clock.phase(), TooltipPhase::Hiding);
        assert!(!clock.is_settled());
        let mid = clock.opacity_at(t0 + 2_000.0 + TOOLTIP_FADE_MS / 2.0, false);
        assert!((mid - 0.5).abs() < 1e-9, "淡出半程 = 0.5,得 {mid}");
        assert_eq!(
            clock.phase_at(t0 + 2_000.0 + TOOLTIP_FADE_MS, false),
            TooltipPhase::Hidden
        );
        assert_eq!(clock.opacity_at(t0 + 2_000.0 + TOOLTIP_FADE_MS, false), 0.0);
        assert!(clock.is_settled());

        // 延迟期直接取消:hide 不经过淡出
        clock.show(t0 + 3_000.0);
        clock.hide(t0 + 3_010.0);
        assert_eq!(clock.phase(), TooltipPhase::Hidden, "延迟期取消不淡出");
        assert!(clock.is_settled());

        // 淡出中重新 show:回延迟期(重置计时)
        clock.show(t0 + 4_000.0);
        clock.phase_at(t0 + 4_000.0 + TOOLTIP_DELAY_MS, false);
        clock.hide(t0 + 4_010.0);
        clock.show(t0 + 4_020.0);
        assert_eq!(
            clock.phase(),
            TooltipPhase::Delaying,
            "淡出中重 show 回延迟期"
        );

        // 非有限时间戳:忽略(防御,不 panic、不变相)
        clock.show(f64::NAN);
        assert_eq!(clock.phase(), TooltipPhase::Delaying);
        clock.hide(f64::INFINITY);
        assert_eq!(clock.phase(), TooltipPhase::Delaying);
    }

    /// TC-CMP-TIP-02:reduced_motion 直入直出——相位立即落终态,不透明度
    /// 只有 0/1 两值,无任何中间灰度(reduced 以参数显式注入,不触全局态)。
    #[test]
    fn tc_cmp_tip_02_reduced_motion_enters_and_exits_directly() {
        let t0 = 1000.0;
        let mut clock = TooltipClock::new();
        clock.show(t0);
        // 延迟期任意时刻直入 Visible
        for t in [t0 + 1.0, t0 + 100.0, t0 + TOOLTIP_DELAY_MS] {
            assert_eq!(clock.phase_at(t, true), TooltipPhase::Visible, "t={t}");
            assert_eq!(clock.opacity_at(t, true), 1.0, "t={t}");
        }
        // 直出:hide 后任意时刻立即 Hidden
        clock.hide(t0 + 2_000.0);
        for t in [t0 + 2_001.0, t0 + 2_040.0, t0 + 3_000.0] {
            assert_eq!(clock.phase_at(t, true), TooltipPhase::Hidden, "t={t}");
            assert_eq!(clock.opacity_at(t, true), 0.0, "t={t}");
        }
        // 全程无中间灰度:淡入窗口内密集采样也只有 0/1
        clock.show(t0 + 4_000.0);
        for step in 0..=80 {
            let t = t0 + 4_000.0 + f64::from(step);
            let opacity = clock.opacity_at(t, true);
            assert!(
                opacity == 0.0 || opacity == 1.0,
                "减弱动态不得出现中间灰度:t={t} opacity={opacity}"
            );
        }
    }

    #[test]
    fn tc_cmp_tip_01_host_shape_and_frame_pump_contract() {
        // Entity 形态的纯数据面:默认隐藏/静止、provider 存位、帧泵契约
        let host = TooltipHost::new();
        assert_eq!(host.clock().phase(), TooltipPhase::Hidden);
        assert!(host.clock().is_settled());
        assert!(!tooltip_host_needs_frame(&host), "隐藏态不续帧");
        let with_provider = TooltipHost::new().with_content_provider(Rc::new(
            |_spec: &TooltipSpec, _window, _cx| div().into_any_element(),
        ));
        assert!(with_provider.provider.is_some(), "provider 存位");
        // 尺寸估计:高度 = caption 行高 + 2×XS(单行精确),宽度受最大宽钳制
        let tip = estimated_tooltip_size("导出", Some("ctrl-e"));
        assert_eq!(
            tip.height,
            f64::from(TextSize::CAPTION.line_height) + 2.0 * f64::from(SpacingTokens::XS)
        );
        assert!(tip.width > 0.0 && tip.width <= f64::from(TOOLTIP_MAX_WIDTH_PX));
    }
}
