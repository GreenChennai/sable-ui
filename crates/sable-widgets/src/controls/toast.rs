//! Toast 通知系统(迭代审查报告 2026-10-04 CMP-03,§5.6 组件矩阵 #10、
//! §5.8 动效、ANI-01 #5"Toast 滑入滑出")。
//!
//! # 形态(宿主根挂载,程序化入队)
//!
//! [`ToastHost`] 是 **Entity 形态浮层宿主**(TooltipHost 同款):挂在应用
//! 根视图,宿主经 [`ToastHost::toast`] 一行入队:
//!
//! ```ignore
//! let toasts = cx.new(|_| ToastHost::new());
//! // 任意业务点:
//! toasts.update(cx, |host, cx| {
//!     host.toast(ToastSpec::new(ToastLevel::Error, "导出失败:磁盘已满"), cx)
//! });
//! ```
//!
//! 渲染 = 右下角(默认,[`OverlayCorner`] 可配)自下而上堆叠的绝对定位
//! 浮层;**堆叠上限 [`TOAST_MAX_VISIBLE`](4),超限挤掉最旧**
//! ([`evict_oldest`] 纯函数单点)。
//!
//! # TTL 分级(§5.6 #10,常量单点)
//!
//! [`ToastLevel`] 三级:`Info` 2.5s / `Warning` 5s / `Error` 10s
//! ([`TOAST_TTL_INFO_MS`] 等三常量 + [`ToastLevel::ttl_ms`] 单一映射)。
//! 到点自动进入离场([`expired_ids`] 纯函数裁决;宿主 render 每帧扫一次,
//! [`ToastHost::expire_due`] 落到各时钟)。
//!
//! # 错误可复制(§5.6 #10;简实现,如实声明)
//!
//! **选简实现 = "复制"钮**而非文本 selectable:gpui 0.2.2 无
//! `text_selectable` 一类文本选区 API(源码核实,2026-10),Error 级 toast
//! 尾部追加"复制"钮,点击把消息原文写入系统剪贴板
//! (`App::write_to_clipboard`),再回调宿主 [`ToastHost::on_copy`] 钩子
//! (宿主可用来给"已复制"反馈)。文本选区式复制待 gpui 升级后评估。
//!
//! # 动效(§5.8 / ANI-01 #5)
//!
//! 滑入 = STATE 档([`MotionTokens::DUR_STATE_MS`],120ms)+ 8px 位移
//! ([`TOAST_SLIDE_PX`],单源复用 select 下拉的 8px 上浮常量),滑出对称
//! (离场 120ms 反向 8px)。状态机 [`ToastClock`] 纯函数、时间显式注入,
//! `reduced_motion` **直入直出**(相位直落终态、透明度只有 0/1)。
//!
//! # 手动关闭
//!
//! 每条 toast 尾部一枚关闭钮([`IconButton`] 语义 + "关闭"可访问名),
//! 点击发起对称滑出;浮层逐帧重建,按钮走**内联形态**(逐行建 Entity 是
//! 反模式 CMP-06)。
//!
//! # 共享浮层锚位(三件浮层的公共小协议,本文件单点定义)
//!
//! 报告把 Toast/Dialog/CommandPalette 归入统一"浮层系统";本模块提供
//! [`OverlayCorner`](角位枚举)+ [`place_overlay`](角落落位纯函数)+
//! [`stacked_top`](堆叠第 n 层偏移)+ [`overlay_scrim`](背板,`surface_0`
//! 半透明遮罩,Dialog/命令面板共用)+ [`OVERLAY_SCRIM_OPACITY`] /
//! [`OVERLAY_MARGIN_PX`] 常量。取舍:角落堆叠型落位只有 Toast 需要,但
//! **背板与边距是三件共用的材质语义**,放在三件中最"浮层"的 toast.rs
//! 单点定义、另两件 `use super::toast::…` 复用(避免各写一份遮罩构造);
//! Dialog(居中)与命令面板(顶部居中)的一次性居中算术不抽协议——
//! 三行代码的重复低于一层间接的代价。

use std::rc::Rc;

use gpui::{
    App, ClipboardItem, Context, Div, ElementId, Entity, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, Styled, Window, div, px,
};
use kurbo::Size;

use crate::anim::reduced_motion;
use crate::controls::button::{IconButton, IconButtonSize, icon_button_element};
use crate::interact::{InteractState, Semantic, SemanticRole, now_ms, semantic_slot, state_layer};
use crate::theme::{elevated, theme};
use crate::tokens::{
    ColorTokens, MotionTokens, RadiusTokens, SpacingTokens, TextSize, UI_FONT, h_flex,
};

// ---------------------------------------------------------------------------
// 共享浮层锚位(三件浮层公共协议;取舍见模块 doc)
// ---------------------------------------------------------------------------

/// 浮层角位([`ToastHost`] 的堆叠锚;默认右下)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OverlayCorner {
    /// 右下(默认)
    #[default]
    BottomRight,
    /// 左下
    BottomLeft,
    /// 右上
    TopRight,
    /// 左上
    TopLeft,
}

impl OverlayCorner {
    /// 是否贴底(堆叠方向裁决用)。
    #[must_use]
    pub fn is_bottom(self) -> bool {
        matches!(self, OverlayCorner::BottomRight | OverlayCorner::BottomLeft)
    }

    /// 是否贴右(滑入位移方向裁决用:贴右从右侧滑入,贴左从左侧)。
    #[must_use]
    pub fn is_right(self) -> bool {
        matches!(self, OverlayCorner::BottomRight | OverlayCorner::TopRight)
    }
}

/// 浮层距视口边的安全边距(px)= 间距令牌 SM(单源复用 tooltip 的视口边距
/// 常量,同一"浮层不贴边"语义)。
pub const OVERLAY_MARGIN_PX: f64 = super::tooltip::VIEWPORT_MARGIN_PX;

/// 浮层落位(纯函数):角位 + 浮层尺寸 + 视口尺寸 + 边距 → 浮层左上角
/// `(left, top)`(px,宿主根坐标系)。宽高非有限时返回 `(边距, 边距)`
/// (防御,不 panic)。堆叠第 n 层的 top 见 [`stacked_top`]。
#[must_use]
pub fn place_overlay(corner: OverlayCorner, size: Size, viewport: Size, margin: f64) -> (f64, f64) {
    if !(size.width.is_finite() && size.height.is_finite()) {
        return (margin, margin);
    }
    let w = size.width.max(0.0);
    let h = size.height.max(0.0);
    let left = if corner.is_right() {
        viewport.width - margin - w
    } else {
        margin
    };
    let top = if corner.is_bottom() {
        viewport.height - margin - h
    } else {
        margin
    };
    (left, top)
}

/// 堆叠第 `index` 层的 top(纯函数;`index` 0 = 贴角最旧一条,层距 =
/// `pitch`)。贴底角位向上堆(top 递减),贴顶角位向下堆(top 递增)。
#[must_use]
pub fn stacked_top(base_top: f64, index: usize, pitch: f64, corner: OverlayCorner) -> f64 {
    let offset = index as f64 * pitch;
    if corner.is_bottom() {
        base_top - offset
    } else {
        base_top + offset
    }
}

/// 背板不透明度(三件浮层共用材质定值;令牌纪律:色取 `surface_0`,alpha
/// 是材质规格常量,同 tooltip `L4_SURFACE_OPACITY` 的先例)。
pub const OVERLAY_SCRIM_OPACITY: f32 = 0.5;

/// 浮层背板(scrim,三件共用;**零硬编码色**):`surface_0` @
/// [`OVERLAY_SCRIM_OPACITY`] 的绝对定位全屏遮罩——深色主题压暗、浅色主题
/// 提亮(令牌自适性),不参与内容布局。Dialog/命令面板直接取用。
#[must_use]
pub fn overlay_scrim(colors: &ColorTokens) -> Div {
    let scrim = gpui::Hsla {
        a: OVERLAY_SCRIM_OPACITY,
        ..colors.surface_0
    };
    div().absolute().inset_0().bg(scrim)
}

// ---------------------------------------------------------------------------
// 常量域(TTL 分级 / 动效 / 几何;颜色一律令牌)
// ---------------------------------------------------------------------------

/// Info 级 TTL(§5.6 #10:信息 2.5s)。
pub const TOAST_TTL_INFO_MS: f64 = 2500.0;
/// Warning 级 TTL(告警 5s)。
pub const TOAST_TTL_WARNING_MS: f64 = 5000.0;
/// Error 级 TTL(错误 10s;错误可复制,留足阅读时间)。
pub const TOAST_TTL_ERROR_MS: f64 = 10000.0;
/// 堆叠层数上限(超出挤掉最旧)。
pub const TOAST_MAX_VISIBLE: usize = 4;
/// 滑入/滑出时长 = 动效四档的 STATE 档(120ms,§5.8/ANI-01 #5)。
pub const TOAST_SLIDE_MS: f64 = MotionTokens::DUR_STATE_MS;
/// 滑入/滑出位移(px,§5.6 #10"8px 位移";单源复用 select 下拉的 8px
/// 上浮常量——同一"浮层位移"语义)。
pub const TOAST_SLIDE_PX: f32 = super::select::MENU_OPEN_OFFSET_PX;
/// 相邻两条 toast 的层间 gap = 间距令牌 SM(8px,4 网格)。
pub const TOAST_GAP_PX: f32 = SpacingTokens::SM;
/// toast 宽(px,4 网格;长文案截断,不横跨视口)。
pub const TOAST_WIDTH_PX: f32 = 320.0;

/// toast 高(px,派生制):文字行高 + 2×SM 内边距(≥24px 命中线)。
#[must_use]
pub fn toast_height() -> f32 {
    TextSize::LABEL.line_height + 2.0 * SpacingTokens::SM
}

/// 堆叠层距(px,纯函数):toast 高 + 层间 gap([`stacked_top`] 的 pitch)。
#[must_use]
pub fn toast_pitch() -> f64 {
    f64::from(toast_height() + TOAST_GAP_PX)
}

// ---------------------------------------------------------------------------
// 级别与 TTL 分级(常量单点)
// ---------------------------------------------------------------------------

/// Toast 级别(§5.6 #10:TTL 分级 + 语义色标记)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToastLevel {
    /// 信息(2.5s;`info` 令牌)
    #[default]
    Info,
    /// 告警(5s;`warning` 令牌)
    Warning,
    /// 错误(10s;`danger` 令牌 + "复制"钮)
    Error,
}

impl ToastLevel {
    /// TTL(毫秒;§5.6 #10 分级单点:Info 2500 / Warning 5000 / Error 10000)。
    #[must_use]
    pub fn ttl_ms(self) -> f64 {
        match self {
            ToastLevel::Info => TOAST_TTL_INFO_MS,
            ToastLevel::Warning => TOAST_TTL_WARNING_MS,
            ToastLevel::Error => TOAST_TTL_ERROR_MS,
        }
    }

    /// 级别语义色(零硬编码:info/warning/danger 三枚令牌)。
    #[must_use]
    pub fn color(self, colors: &ColorTokens) -> gpui::Hsla {
        match self {
            ToastLevel::Info => colors.info,
            ToastLevel::Warning => colors.warning,
            ToastLevel::Error => colors.danger,
        }
    }
}

// ---------------------------------------------------------------------------
// 显隐状态机(纯函数;TC-CMP-TOAST-01 的被测单点)
// ---------------------------------------------------------------------------

/// Toast 相位(纯状态机,时间显式注入):入队即 [`ToastPhase::Entering`] →
/// 120ms 滑入 → [`ToastPhase::Visible`](至 TTL 或手动关闭)→
/// [`ToastPhase::Leaving`] 120ms 滑出 → [`ToastPhase::Gone`]。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToastPhase {
    /// 滑入中(120ms + 8px 位移)
    #[default]
    Entering,
    /// 可见(静止态,零帧)
    Visible,
    /// 滑出中(120ms + 8px 位移,对称反向)
    Leaving,
    /// 已消失(宿主可回收)
    Gone,
}

/// Toast 显隐时钟(纯状态机):`dismiss` 把 Entering/Visible 推入 Leaving;
/// [`phase_at`](读取即推进,沉降式)到点自动迁移;`reduced` **直入直出**
/// (Entering 立即 Visible、Leaving 立即 Gone,透明度只有 0/1)。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ToastClock {
    phase: ToastPhase,
    started_ms: f64,
}

impl ToastClock {
    /// 入队新时钟(从 Entering 起算;`now_ms` = 入队时刻)。
    pub fn new(now_ms: f64) -> Self {
        ToastClock {
            phase: ToastPhase::Entering,
            started_ms: now_ms,
        }
    }

    /// 当前相位(不推进;渲染帧间的快照)。
    #[must_use]
    pub fn phase(&self) -> ToastPhase {
        self.phase
    }

    /// 是否静止(Visible/Gone):为真时宿主无需为该条续帧。
    #[must_use]
    pub fn is_settled(&self) -> bool {
        matches!(self.phase, ToastPhase::Visible | ToastPhase::Gone)
    }

    /// 发起滑出(TTL 到点与手动关闭共用;Gone/Leaving 后无操作,幂等)。
    /// 非有限时间戳忽略(防御,不 panic)。
    pub fn dismiss(&mut self, now_ms: f64) {
        if !now_ms.is_finite() {
            return;
        }
        if matches!(self.phase, ToastPhase::Entering | ToastPhase::Visible) {
            self.phase = ToastPhase::Leaving;
            self.started_ms = now_ms;
        }
    }

    /// 推进并返回当前相位(沉降式)。`reduced` 直入直出。
    pub fn phase_at(&mut self, now_ms: f64, reduced: bool) -> ToastPhase {
        if !now_ms.is_finite() {
            return self.phase;
        }
        let elapsed = now_ms - self.started_ms;
        self.phase = match self.phase {
            ToastPhase::Entering => {
                if reduced || elapsed >= TOAST_SLIDE_MS {
                    ToastPhase::Visible
                } else {
                    ToastPhase::Entering
                }
            }
            ToastPhase::Leaving => {
                if reduced || elapsed >= TOAST_SLIDE_MS {
                    ToastPhase::Gone
                } else {
                    ToastPhase::Leaving
                }
            }
            settled => settled,
        };
        self.phase
    }

    /// 透明度 0..1(线性;`reduced` 只有 0/1):Entering 从 0 爬到 1、
    /// Visible 恒 1、Leaving 从 1 落到 0、Gone 0。
    #[must_use]
    pub fn opacity_at(&self, now_ms: f64, reduced: bool) -> f64 {
        let elapsed = now_ms - self.started_ms;
        match self.phase {
            ToastPhase::Entering => {
                if reduced {
                    return 1.0;
                }
                (elapsed / TOAST_SLIDE_MS).clamp(0.0, 1.0)
            }
            ToastPhase::Visible => 1.0,
            ToastPhase::Leaving => {
                if reduced {
                    return 0.0;
                }
                1.0 - (elapsed / TOAST_SLIDE_MS).clamp(0.0, 1.0)
            }
            ToastPhase::Gone => 0.0,
        }
    }

    /// 滑动位移(px,OutCubic 缓动):Entering 从"向角外侧 8px"归零(从
    /// 角外侧滑入),Visible 恒 0,Leaving 从 0 长回"向角外侧 8px"(与滑入
    /// 对称)。`toward_outside` = 角位是否贴右(贴右向 +x 滑,贴左向 −x)。
    /// `reduced` 恒 0(直入直出,无位移)。
    #[must_use]
    pub fn slide_offset_at(&self, now_ms: f64, reduced: bool, toward_outside: bool) -> f64 {
        if reduced {
            return 0.0;
        }
        let sign = if toward_outside { 1.0 } else { -1.0 };
        let elapsed = now_ms - self.started_ms;
        match self.phase {
            ToastPhase::Entering => {
                let p = (elapsed / TOAST_SLIDE_MS).clamp(0.0, 1.0);
                sign * (1.0 - crate::anim::Easing::OutCubic.apply(p)) * f64::from(TOAST_SLIDE_PX)
            }
            ToastPhase::Leaving => {
                let q = (elapsed / TOAST_SLIDE_MS).clamp(0.0, 1.0);
                sign * crate::anim::Easing::OutCubic.apply(q) * f64::from(TOAST_SLIDE_PX)
            }
            _ => 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// 队列纯函数层(入队/计时/出队;TC-CMP-TOAST-01 断言面)
// ---------------------------------------------------------------------------

/// 一条 toast(宿主队列成员;字段私有,读经 [`ToastEntry::id`] /
/// [`ToastEntry::spec`] / [`ToastEntry::clock`])。
pub struct ToastEntry {
    id: u64,
    spec: ToastSpec,
    clock: ToastClock,
    /// TTL 到点(`入队时刻 + 级别 TTL`;[`expired_ids`] 的裁决依据)。
    expires_ms: f64,
}

impl ToastEntry {
    /// 到点时刻 = 入队时刻 + 级别 TTL(纯计算)。
    #[must_use]
    pub fn expires_at(enqueued_ms: f64, level: ToastLevel) -> f64 {
        enqueued_ms + level.ttl_ms()
    }

    /// 队列 id(入队顺序;测试/调试定位)。
    #[must_use]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// 内容规格。
    #[must_use]
    pub fn spec(&self) -> &ToastSpec {
        &self.spec
    }

    /// 显隐时钟(测试读相位)。
    #[must_use]
    pub fn clock(&self) -> &ToastClock {
        &self.clock
    }
}

/// Toast 内容规格(级别 + 消息)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToastSpec {
    /// 级别(TTL 分级 + 语义色 + Error 复制钮的裁决依据)
    pub level: ToastLevel,
    /// 消息正文(单行,长文案截断)
    pub message: SharedString,
}

impl ToastSpec {
    /// 指定级别与消息的规格。
    pub fn new(level: ToastLevel, message: impl Into<SharedString>) -> Self {
        ToastSpec {
            level,
            message: message.into(),
        }
    }
}

/// 到期扫描(纯函数):`now_ms` 已过到期点且尚未 Gone/Leaving 的条目 id
/// 列表(按队列序)。宿主对返回的每一条发起 [`ToastClock::dismiss`](滑出),
/// "TTL 分级到点自动离场"因此是纯函数裁决,时间显式注入可测。
#[must_use]
pub fn expired_ids(entries: &[ToastEntry], now_ms: f64) -> Vec<u64> {
    if !now_ms.is_finite() {
        return Vec::new();
    }
    entries
        .iter()
        .filter(|entry| {
            now_ms >= entry.expires_ms
                && !matches!(
                    entry.clock().phase(),
                    ToastPhase::Gone | ToastPhase::Leaving
                )
        })
        .map(ToastEntry::id)
        .collect()
}

/// 堆叠上限裁决(纯函数):`entries` 超过 `max` 时从最旧端(`entries[0]`,
/// 队列头部)逐条挤出,返回被挤出的 id 列表(保序);未超限返回空表。
/// 入队路径单点调用,不散落容量判断。
#[must_use]
pub fn evict_oldest(entries: &mut Vec<ToastEntry>, max: usize) -> Vec<u64> {
    let mut evicted = Vec::new();
    while entries.len() > max {
        let entry = entries.remove(0);
        evicted.push(entry.id);
    }
    evicted
}

/// 复制反馈钩子(宿主可选;Error toast 的"复制"钮把消息写入剪贴板后
/// 调用,宿主可借此给"已复制"反馈)。
pub type ToastCopyFn = Rc<dyn Fn(&str, &mut App)>;

// ---------------------------------------------------------------------------
// ToastHost(Entity 形态,挂宿主根)
// ---------------------------------------------------------------------------

/// Toast 宿主(Entity 形态):挂在视图根,程序化 [`Self::toast`] 入队,
/// 右下堆叠(角位可配)、TTL 分级、滑入滑出、手动关闭与 Error 复制全套。
///
/// 计时入口两轨:`toast`/`dismiss` 用库时钟([`now_ms`]),`toast_at`/
/// `dismiss_at` 显式注入(测试/确定性场景)——两轨落到同一套纯状态步进
/// ([`Self::enqueue`]/[`Self::begin_dismiss`]/[`Self::expire_due`])。
pub struct ToastHost {
    entries: Vec<ToastEntry>,
    next_id: u64,
    corner: OverlayCorner,
    copy_hook: Option<ToastCopyFn>,
    /// A11Y-02 语义槽(浮层宿主,非交互件;role 缺省 Decoration)
    semantic: Semantic,
}

// A11Y-02 语义槽:ToastHost 是程序化浮层宿主(非交互件,不进 Tab 序);
// label/role 为接口面,role 缺省 Decoration。
semantic_slot!(ToastHost);

impl Default for ToastHost {
    fn default() -> Self {
        Self::new()
    }
}

impl ToastHost {
    /// 空宿主(右下角位;零在队条目)。
    pub fn new() -> Self {
        ToastHost {
            entries: Vec::new(),
            next_id: 0,
            corner: OverlayCorner::BottomRight,
            copy_hook: None,
            semantic: Semantic::new(),
        }
    }

    /// 角位(链式;默认 [`OverlayCorner::BottomRight`])。
    #[must_use]
    pub fn corner(mut self, corner: OverlayCorner) -> Self {
        self.corner = corner;
        self
    }

    /// 复制反馈钩子(Error toast"复制"钮的宿主回执;链式)。
    #[must_use]
    pub fn on_copy(mut self, f: impl Fn(&str, &mut App) + 'static) -> Self {
        self.copy_hook = Some(Rc::new(f));
        self
    }

    /// 解析语义(A11Y-02):显式 `.label(...)`/`.role(...)` 优先;role 缺省
    /// Decoration。
    #[must_use]
    pub fn resolved_semantic(&self) -> Semantic {
        let mut sem = Semantic::new();
        if let Some(label) = self.semantic.label() {
            sem = sem.with_label(label.clone());
        }
        sem.with_role(self.semantic.role().unwrap_or(SemanticRole::Decoration))
    }

    /// 在队条目(队列序,最旧在前;宿主调试/测试读)。
    #[must_use]
    pub fn entries(&self) -> &[ToastEntry] {
        &self.entries
    }

    /// 在队条目数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否无在队条目。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 入队(§5.6 #10 `toast(host, spec)` 语义):分配 id、启动滑入、按
    /// 级别落 TTL 到点;**超 [`TOAST_MAX_VISIBLE`] 挤掉最旧**并返回被挤
    /// id([`evict_oldest`] 单点)。非有限时间防御为立即到期的 Visible。
    pub fn enqueue(&mut self, spec: ToastSpec, now_ms: f64) -> Vec<u64> {
        let id = self.next_id;
        self.next_id += 1;
        let clock = if now_ms.is_finite() {
            ToastClock::new(now_ms)
        } else {
            // 非有限时间:直接 Visible 终态(零位移),到期点视为 0(立即到期)
            ToastClock {
                phase: ToastPhase::Visible,
                started_ms: 0.0,
            }
        };
        let expires_ms = ToastEntry::expires_at(now_ms.max(0.0), spec.level);
        self.entries.push(ToastEntry {
            id,
            spec,
            clock,
            expires_ms,
        });
        evict_oldest(&mut self.entries, TOAST_MAX_VISIBLE)
    }

    /// 入队(库时钟;通知重渲染)。
    pub fn toast(&mut self, spec: ToastSpec, cx: &mut Context<Self>) {
        self.enqueue(spec, now_ms());
        cx.notify();
    }

    /// 手动关闭的纯状态步进(对称滑出;未知 id/已离场幂等,返回是否发起)。
    pub fn begin_dismiss(&mut self, id: u64, now_ms: f64) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        let phase = entry.clock.phase();
        if matches!(phase, ToastPhase::Entering | ToastPhase::Visible) {
            entry.clock.dismiss(now_ms);
            true
        } else {
            false
        }
    }

    /// 手动关闭(库时钟;通知重渲染)。
    pub fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.begin_dismiss(id, now_ms()) {
            cx.notify();
        }
    }

    /// TTL 到点扫描 + 滑出发起(纯状态步进;渲染每帧前置,把 [`expired_ids`]
    /// 的裁决落到各时钟)。返回本帧新发起滑出的 id。
    pub fn expire_due(&mut self, now_ms: f64) -> Vec<u64> {
        let mut started = Vec::new();
        for id in expired_ids(&self.entries, now_ms) {
            if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
                entry.clock.dismiss(now_ms);
                started.push(id);
            }
        }
        started
    }

    /// 推进全部在队时钟(纯状态步进;渲染每帧前置,沉降式迁移相位)。
    pub fn advance(&mut self, now_ms: f64, reduced: bool) {
        for entry in &mut self.entries {
            entry.clock.phase_at(now_ms, reduced);
        }
    }

    /// 回收 Gone 条目(渲染每帧收尾;纯状态步进)。
    pub fn prune(&mut self) {
        self.entries
            .retain(|entry| entry.clock.phase() != ToastPhase::Gone);
    }

    /// 是否需要续帧(任一在队时钟未静止;静止零帧提交)。
    #[must_use]
    pub fn needs_frame(&self) -> bool {
        self.entries.iter().any(|entry| !entry.clock.is_settled())
    }
}

impl Render for ToastHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = now_ms();
        let reduced = reduced_motion();
        // 每帧前置:TTL 到点滑出 → 推进相位 → 回收 Gone
        self.expire_due(now);
        self.advance(now, reduced);
        self.prune();
        if self.entries.is_empty() {
            return div().into_any_element();
        }

        let viewport = window.viewport_size();
        let vp_size = Size::new(
            f64::from(f32::from(viewport.width)),
            f64::from(f32::from(viewport.height)),
        );
        let margin = OVERLAY_MARGIN_PX;
        // 宽度钳进视口(窄窗安全);高 = 派生制
        let width = f64::from(TOAST_WIDTH_PX).min((vp_size.width - 2.0 * margin).max(0.0));
        let height = f64::from(toast_height());
        let (base_left, base_top) =
            place_overlay(self.corner, Size::new(width, height), vp_size, margin);
        let pitch = toast_pitch();
        let toward_outside = self.corner.is_right();
        let entity = cx.entity();

        let mut stack = div().absolute().inset_0();
        for (index, entry) in self.entries.iter().enumerate() {
            let slide = entry.clock.slide_offset_at(now, reduced, toward_outside);
            let opacity = f32v(entry.clock.opacity_at(now, reduced));
            let offset = if toward_outside { slide } else { -slide };
            let top = stacked_top(base_top, index, pitch, self.corner);
            stack = stack.child(self.render_toast(
                index,
                entry,
                ToastFrame {
                    left: base_left + offset,
                    top,
                    width,
                    opacity,
                },
                entity.clone(),
                cx,
            ));
        }

        if self.needs_frame() {
            window.request_animation_frame();
        }
        stack.into_any_element()
    }
}

/// 一帧的单条 toast 落位(渲染内聚的参数包;收敛 [`ToastHost::render_toast`]
/// 的参数面)。
struct ToastFrame {
    left: f64,
    top: f64,
    width: f64,
    opacity: f32,
}

impl ToastHost {
    /// 单条 toast 装配:L4 材质浮层(surface_1 底 + border_strong 描边)+
    /// 级别色点 + 消息 + (Error)"复制"钮 + 关闭钮。按钮走内联形态
    /// (浮层逐帧重建,逐行建 Entity 是反模式 CMP-06)。
    fn render_toast(
        &self,
        index: usize,
        entry: &ToastEntry,
        frame: ToastFrame,
        entity: Entity<Self>,
        cx: &App,
    ) -> gpui::AnyElement {
        let ToastFrame {
            left,
            top,
            width,
            opacity,
        } = frame;
        let id = entry.id;
        let level = entry.spec.level;
        let message = entry.spec.message.clone();
        let colors = &theme(cx).colors;
        let close_host = entity.clone();
        let copy_host = entity.clone();
        let copy_message = message.clone();

        // 关闭钮(IconButton 语义:Ghost 内联形态 + "关闭"可访问名)
        let close = icon_button_element(
            IconButton::new(ElementId::NamedInteger("toast-close".into(), id), "✕")
                .size(IconButtonSize::Icon20)
                .tooltip("关闭")
                .on_press(move |_ev, _window, cx| {
                    close_host.update(cx, |host, cx| host.dismiss(id, cx));
                }),
            cx,
        );

        // Error 级"复制"钮(§5.6 #10 错误可复制的简实现;写剪贴板 + 宿主钩子)
        let copy_button = (level == ToastLevel::Error).then(|| {
            icon_button_element(
                IconButton::new(ElementId::NamedInteger("toast-copy".into(), id), "⧉")
                    .size(IconButtonSize::Icon20)
                    .tooltip("复制错误信息")
                    .on_press(move |_ev, _window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_message.to_string()));
                        copy_host.update(cx, |host, cx| {
                            if let Some(hook) = host.copy_hook.clone() {
                                hook(copy_message.as_ref(), cx);
                            }
                        });
                    }),
                cx,
            )
        });

        let row = h_flex()
            .min_w_0()
            .flex_1()
            .gap(px(SpacingTokens::XS))
            .child(
                div()
                    .size(px(RadiusTokens::SM))
                    .rounded_full()
                    .bg(level.color(colors)),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .font_family(UI_FONT)
                    .text_size(px(TextSize::LABEL.size))
                    .text_color(colors.text_primary)
                    .child(message),
            )
            .children(copy_button)
            .child(close);

        let quad = h_flex()
            .justify_between()
            .w(px(f32v(width)))
            .min_h(px(toast_height()))
            .px(px(SpacingTokens::SM))
            .py(px(SpacingTokens::XS))
            .gap(px(SpacingTokens::SM))
            .rounded(px(RadiusTokens::MD))
            .border_1()
            .border_color(if level == ToastLevel::Error {
                colors.danger // 错误级:danger 描边强化(令牌)
            } else {
                colors.border_strong
            })
            .bg(state_layer(
                colors.surface_1,
                InteractState::Idle,
                colors.accent,
            ))
            .child(row);

        elevated(4, quad)
            .absolute()
            .left(px(f32v(left)))
            .top(px(f32v(top)))
            .opacity(opacity)
            .id(ElementId::NamedInteger(
                "toast-layer".into(),
                u64::try_from(index).unwrap_or(0),
            ))
            .into_any_element()
    }
}

/// 帧泵裁决:宿主是否需要续帧(与 `tooltip_host_needs_frame` 同款契约;
/// 宿主根视图包住 [`ToastHost`] 时每帧调用)。
#[must_use]
pub fn toast_host_needs_frame(host: &ToastHost) -> bool {
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
// TC-CMP-TOAST-01(TTL 分级 + 堆叠上限 + 滑入滑出相位)
// TC-CMP-TOAST-02(reduced_motion 直入直出)
// 全部纯函数断言,不经 GUI、不触全局开关(reduced 以参数显式注入)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, level: ToastLevel, now: f64) -> ToastEntry {
        ToastEntry {
            id,
            spec: ToastSpec::new(level, "消息"),
            clock: ToastClock::new(now),
            expires_ms: ToastEntry::expires_at(now, level),
        }
    }

    // —— TC-CMP-TOAST-01:TTL 分级(常量单点) ——

    #[test]
    fn tc_cmp_toast_01_ttl_levels_are_graduated() {
        assert_eq!(TOAST_TTL_INFO_MS, 2500.0, "信息 2.5s");
        assert_eq!(TOAST_TTL_WARNING_MS, 5000.0, "告警 5s");
        assert_eq!(TOAST_TTL_ERROR_MS, 10000.0, "错误 10s");
        // 单点映射:级别 → TTL 严格递增
        assert_eq!(ToastLevel::Info.ttl_ms(), TOAST_TTL_INFO_MS);
        assert_eq!(ToastLevel::Warning.ttl_ms(), TOAST_TTL_WARNING_MS);
        assert_eq!(ToastLevel::Error.ttl_ms(), TOAST_TTL_ERROR_MS);
        assert!(ToastLevel::Info.ttl_ms() < ToastLevel::Warning.ttl_ms());
        assert!(ToastLevel::Warning.ttl_ms() < ToastLevel::Error.ttl_ms());
        // 到点 = 入队 + 级别 TTL
        assert_eq!(ToastEntry::expires_at(1000.0, ToastLevel::Info), 3500.0);
        assert_eq!(ToastEntry::expires_at(1000.0, ToastLevel::Error), 11000.0);
    }

    #[test]
    fn tc_cmp_toast_01_expired_scan_respects_ttl() {
        // Info 2.5s:到 2.4s 未到期,到 2.5s(含)到期
        let entries = vec![entry(1, ToastLevel::Info, 1000.0)];
        assert!(expired_ids(&entries, 1000.0 + 2400.0).is_empty(), "未到期");
        assert_eq!(expired_ids(&entries, 1000.0 + TOAST_TTL_INFO_MS), vec![1]);
        // Warning 5s:同一时刻 Info 已到期而 Warning 未到
        let mixed = vec![
            entry(1, ToastLevel::Info, 1000.0),
            entry(2, ToastLevel::Warning, 1000.0),
        ];
        assert_eq!(expired_ids(&mixed, 1000.0 + TOAST_TTL_INFO_MS), vec![1]);
        assert_eq!(
            expired_ids(&mixed, 1000.0 + TOAST_TTL_WARNING_MS),
            vec![1, 2],
            "Warning 到点后两条都到期"
        );
        // Error 10s:5s 时仍未到期
        let err = vec![entry(3, ToastLevel::Error, 0.0)];
        assert!(expired_ids(&err, TOAST_TTL_WARNING_MS).is_empty());
        assert_eq!(expired_ids(&err, TOAST_TTL_ERROR_MS), vec![3]);
        // 已在滑出的不重复发起;非有限时间戳安全
        let mut leaving = vec![entry(4, ToastLevel::Info, 0.0)];
        leaving[0].clock.dismiss(TOAST_TTL_INFO_MS / 2.0);
        assert!(
            expired_ids(&leaving, TOAST_TTL_ERROR_MS * 2.0).is_empty(),
            "离场中不重复"
        );
        assert!(expired_ids(&entries, f64::NAN).is_empty());
    }

    // —— TC-CMP-TOAST-01:堆叠上限(超限挤掉最旧) ——

    #[test]
    fn tc_cmp_toast_01_stack_cap_evicts_oldest() {
        assert_eq!(TOAST_MAX_VISIBLE, 4);
        let mut entries: Vec<ToastEntry> =
            (0..6).map(|i| entry(i, ToastLevel::Info, 0.0)).collect();
        let evicted = evict_oldest(&mut entries, TOAST_MAX_VISIBLE);
        assert_eq!(evicted, vec![0, 1], "挤掉最旧两条(队列头部)");
        assert_eq!(entries.len(), TOAST_MAX_VISIBLE);
        let remaining: Vec<u64> = entries.iter().map(ToastEntry::id).collect();
        assert_eq!(remaining, vec![2, 3, 4, 5], "保序:新条目在队尾");
        // 未超限:空表返回,队列不动
        assert!(evict_oldest(&mut entries, TOAST_MAX_VISIBLE).is_empty());
        assert_eq!(entries.len(), 4);
        // 宿主入队路径走同一裁决(纯状态步进,时间注入)
        let mut host = ToastHost::new();
        for i in 0..6 {
            host.enqueue(ToastSpec::new(ToastLevel::Info, "n"), f64::from(i));
        }
        assert_eq!(host.len(), TOAST_MAX_VISIBLE, "宿主入队超限收敛到 4");
        let ids: Vec<u64> = host.entries().iter().map(ToastEntry::id).collect();
        assert_eq!(ids, vec![2, 3, 4, 5], "最旧两条(0/1)被挤掉");
    }

    #[test]
    fn tc_cmp_toast_01_slide_phases_are_symmetric() {
        let t0 = 1000.0;
        let mut clock = ToastClock::new(t0);
        assert_eq!(clock.phase(), ToastPhase::Entering);
        assert!(!clock.is_settled(), "滑入中需要续帧");

        // 滑入 120ms:起点位移 8px(角外侧)、不透明度 0;中程单调爬升
        assert!((clock.slide_offset_at(t0, false, true) - f64::from(TOAST_SLIDE_PX)).abs() < 1e-9);
        assert_eq!(clock.opacity_at(t0, false), 0.0);
        assert_eq!(
            TOAST_SLIDE_MS,
            MotionTokens::DUR_STATE_MS,
            "滑入 = STATE 档"
        );
        let mid = t0 + TOAST_SLIDE_MS / 2.0;
        let mid_offset = clock.slide_offset_at(mid, false, true);
        assert!(
            mid_offset > 0.0 && mid_offset < f64::from(TOAST_SLIDE_PX),
            "滑入中程位移在 (0, 8) 内:{mid_offset}"
        );
        let mid_opacity = clock.opacity_at(mid, false);
        assert!(
            mid_opacity > 0.0 && mid_opacity < 1.0,
            "滑入中程透明度在 (0,1)"
        );
        // OutCubic:前半程位移消去过半(快出)
        assert!(
            mid_offset < f64::from(TOAST_SLIDE_PX) / 2.0,
            "OutCubic 前半程已走过大半位移:{mid_offset}"
        );
        // 120ms 到点:Visible、位移归零、透明度 1、静止(停帧)
        assert_eq!(
            clock.phase_at(t0 + TOAST_SLIDE_MS, false),
            ToastPhase::Visible
        );
        assert_eq!(clock.slide_offset_at(t0 + TOAST_SLIDE_MS, false, true), 0.0);
        assert_eq!(clock.opacity_at(t0 + TOAST_SLIDE_MS, false), 1.0);
        assert!(clock.is_settled(), "可见态静止");

        // 手动/TTL 关闭 → 对称滑出:起点位移 0,终点位移 8px、透明度 0
        let td = t0 + 2000.0;
        clock.dismiss(td);
        assert_eq!(clock.phase(), ToastPhase::Leaving);
        assert!(!clock.is_settled());
        assert_eq!(
            clock.slide_offset_at(td, false, true),
            0.0,
            "滑出起点位移 0(对称)"
        );
        let out_mid = clock.slide_offset_at(td + TOAST_SLIDE_MS / 2.0, false, true);
        assert!(
            out_mid > 0.0 && out_mid < f64::from(TOAST_SLIDE_PX),
            "滑出中程位移在 (0, 8) 内:{out_mid}"
        );
        assert_eq!(clock.phase_at(td + TOAST_SLIDE_MS, false), ToastPhase::Gone);
        assert_eq!(clock.opacity_at(td + TOAST_SLIDE_MS, false), 0.0);
        assert!(clock.is_settled(), "Gone 静止");

        // 方向参数:贴左角位位移取负(向左外侧),幅度一致
        let left_clock = ToastClock::new(t0);
        let left_offset = left_clock.slide_offset_at(t0, false, false);
        assert!(left_offset < 0.0, "贴左向 −x 滑:{left_offset}");
        assert!((left_offset.abs() - f64::from(TOAST_SLIDE_PX)).abs() < 1e-9);
        // 重复 dismiss 幂等;非有限时间戳:忽略(防御,不 panic、不变相)
        clock.dismiss(td + TOAST_SLIDE_MS * 2.0);
        assert_eq!(clock.phase(), ToastPhase::Gone);
        let mut fresh = ToastClock::new(t0);
        fresh.dismiss(f64::NAN);
        assert_eq!(fresh.phase(), ToastPhase::Entering);
    }

    #[test]
    fn tc_cmp_toast_01_host_lifecycle_enqueue_dismiss_expire() {
        let mut host = ToastHost::new();
        host.enqueue(ToastSpec::new(ToastLevel::Info, "a"), 0.0);
        host.enqueue(ToastSpec::new(ToastLevel::Error, "b"), 100.0);
        assert_eq!(host.len(), 2);
        assert!(host.needs_frame(), "有滑入中的条目");
        // 手动关闭:发起滑出;未知 id 幂等假
        assert!(host.begin_dismiss(0, 200.0));
        assert!(!host.begin_dismiss(99, 200.0), "未知 id 无操作");
        assert_eq!(host.entries()[0].clock().phase(), ToastPhase::Leaving);
        // 已在滑出的再次关闭:假(幂等)
        assert!(!host.begin_dismiss(0, 300.0), "离场中不重复发起");
        // TTL 到点扫描:Error 条目 10s 到点才发起
        assert!(host.expire_due(5000.0).is_empty(), "10s 级未到点");
        assert_eq!(host.expire_due(100.0 + TOAST_TTL_ERROR_MS), vec![1]);
        assert!(
            host.expire_due(100.0 + TOAST_TTL_ERROR_MS).is_empty(),
            "重复扫描不重复发起"
        );
        // 滑出完成 → Gone → prune 回收
        host.advance(100.0 + TOAST_TTL_ERROR_MS + TOAST_SLIDE_MS, false);
        assert_eq!(host.entries()[1].clock().phase(), ToastPhase::Gone);
        host.prune();
        assert!(host.is_empty(), "Gone 全部回收");
        assert!(!host.needs_frame(), "空宿主停帧");
    }

    // —— TC-CMP-TOAST-02:reduced_motion 直入直出 ——

    #[test]
    fn tc_cmp_toast_02_reduced_motion_passes_through() {
        let t0 = 1000.0;
        let mut clock = ToastClock::new(t0);
        // 直入:入队任意时刻立即 Visible、透明度 1、零位移
        for t in [t0, t0 + 1.0, t0 + TOAST_SLIDE_MS / 2.0] {
            assert_eq!(clock.phase_at(t, true), ToastPhase::Visible, "t={t}");
            assert_eq!(clock.opacity_at(t, true), 1.0);
            assert_eq!(clock.slide_offset_at(t, true, true), 0.0, "无位移");
        }
        assert!(clock.is_settled(), "减弱动态:入队即静止,不续帧");
        // 直出:dismiss 后任意时刻立即 Gone、透明度 0
        clock.dismiss(t0 + 100.0);
        for t in [t0 + 100.0, t0 + 160.0, t0 + 500.0] {
            assert_eq!(clock.phase_at(t, true), ToastPhase::Gone, "t={t}");
            assert_eq!(clock.opacity_at(t, true), 0.0);
        }
        // 全程无中间灰度:滑入/滑出窗口内密集采样也只有 0/1
        let mut clock2 = ToastClock::new(t0);
        for step in 0..=120 {
            let t = t0 + f64::from(step);
            let opacity = clock2.opacity_at(t, true);
            assert!(
                opacity == 0.0 || opacity == 1.0,
                "减弱动态不得出现中间灰度:t={t} opacity={opacity}"
            );
            clock2.phase_at(t, true);
        }
        clock2.dismiss(t0 + 1000.0);
        for step in 0..=120 {
            let opacity = clock2.opacity_at(t0 + 1000.0 + f64::from(step), true);
            assert!(opacity == 0.0 || opacity == 1.0);
        }
    }

    // —— 共享浮层锚位(OverlayCorner / place_overlay / stacked_top / scrim) ——

    #[test]
    fn tc_cmp_toast_01_overlay_anchor_places_all_corners() {
        let size = Size::new(320.0, 34.0);
        let vp = Size::new(800.0, 600.0);
        let m = OVERLAY_MARGIN_PX;
        assert_eq!(m, f64::from(SpacingTokens::SM), "边距 = SM 令牌单源");
        // 右下:右缘/下缘留边距
        let (l, t) = place_overlay(OverlayCorner::BottomRight, size, vp, m);
        assert_eq!(l, 800.0 - m - 320.0);
        assert_eq!(t, 600.0 - m - 34.0);
        // 左下:左缘贴边距
        let (l, t_bottom) = place_overlay(OverlayCorner::BottomLeft, size, vp, m);
        assert_eq!(l, m);
        assert_eq!(t_bottom, 600.0 - m - 34.0);
        // 右上:top 贴边距
        let (_, t_top) = place_overlay(OverlayCorner::TopRight, size, vp, m);
        assert_eq!(t_top, m);
        // 堆叠:贴底向上(top 递减),贴顶向下(top 递增)
        let pitch = toast_pitch();
        assert_eq!(
            stacked_top(t_top, 1, pitch, OverlayCorner::TopRight),
            m + pitch,
            "贴顶向下堆"
        );
        assert_eq!(
            stacked_top(t_bottom, 2, pitch, OverlayCorner::BottomRight),
            t_bottom - 2.0 * pitch,
            "贴底向上堆"
        );
        // 非有限尺寸防御:回落边距点
        let bad = Size::new(f64::NAN, 34.0);
        assert_eq!(
            place_overlay(OverlayCorner::BottomRight, bad, vp, m),
            (m, m)
        );
    }

    #[test]
    fn tc_cmp_toast_01_scrim_is_surface0_token_at_spec_opacity() {
        // 背板 = surface_0 令牌 @ 材质定值 alpha(色相/饱和度逐位来自令牌,
        // 零硬编码色;Dialog/命令面板共用同一构造)
        for colors in [ColorTokens::dark(), ColorTokens::light()] {
            let scrim = gpui::Hsla {
                a: OVERLAY_SCRIM_OPACITY,
                ..colors.surface_0
            };
            assert_eq!(scrim.h, colors.surface_0.h);
            assert_eq!(scrim.s, colors.surface_0.s);
            assert_eq!(scrim.l, colors.surface_0.l);
            assert!((scrim.a - OVERLAY_SCRIM_OPACITY).abs() < 1e-6);
        }
        assert_eq!(OVERLAY_SCRIM_OPACITY, 0.5);
    }

    // —— 宿主形态与几何(纯数据面) ——

    #[test]
    fn tc_cmp_toast_01_host_shape_and_geometry() {
        let host = ToastHost::new();
        assert_eq!(host.corner, OverlayCorner::BottomRight, "默认右下");
        assert!(host.is_empty());
        assert_eq!(host.len(), 0);
        assert!(!toast_host_needs_frame(&host), "空宿主不续帧");
        assert_eq!(
            host.resolved_semantic().role(),
            Some(SemanticRole::Decoration)
        );
        // 几何派生:高 = 行高 + 2×SM,≥24 命中线;层距 = 高 + gap
        assert_eq!(
            toast_height(),
            TextSize::LABEL.line_height + 2.0 * SpacingTokens::SM
        );
        assert!(toast_height() >= crate::interact::MIN_HIT_PX);
        assert!((toast_pitch() - f64::from(toast_height() + TOAST_GAP_PX)).abs() < 1e-9);
        assert_eq!(
            TOAST_SLIDE_PX,
            crate::controls::select::MENU_OPEN_OFFSET_PX,
            "8px 位移单源"
        );
        assert_eq!(TOAST_GAP_PX, SpacingTokens::SM);
        assert_eq!(TOAST_WIDTH_PX, 320.0);
        // 级别色全部来自令牌(深浅两主题下均与对应令牌逐位相同)
        for colors in [ColorTokens::dark(), ColorTokens::light()] {
            assert_eq!(ToastLevel::Info.color(&colors), colors.info);
            assert_eq!(ToastLevel::Warning.color(&colors), colors.warning);
            assert_eq!(ToastLevel::Error.color(&colors), colors.danger);
        }
    }

    #[test]
    fn toast_spec_defaults_and_builder() {
        let spec = ToastSpec::default();
        assert_eq!(spec.level, ToastLevel::Info);
        assert_eq!(spec.message, "");
        let named = ToastSpec::new(ToastLevel::Error, "导出失败");
        assert_eq!(named.message, "导出失败");
        // OutCubic 快出:1/4 时程内位移已消去过半
        let clock = ToastClock::new(0.0);
        let quarter = clock.slide_offset_at(TOAST_SLIDE_MS / 4.0, false, true);
        assert!(quarter < f64::from(TOAST_SLIDE_PX) * 0.5, "{quarter}");
        // 角位辅助谓词
        assert!(OverlayCorner::BottomRight.is_bottom() && OverlayCorner::BottomRight.is_right());
        assert!(!OverlayCorner::TopLeft.is_bottom() && !OverlayCorner::TopLeft.is_right());
    }
}
