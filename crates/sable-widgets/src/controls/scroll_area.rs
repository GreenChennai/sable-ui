//! ScrollArea 统一滚动容器(迭代审查报告 2026-10-04 CMP-04,§5.6 组件矩阵 #8)。
//!
//! # 形态(纯 gpui 自绘,零 gpui-component)
//!
//! [`ScrollArea`] = 细滚动条(轨道 + 胶囊滑块,**hover 显形**)+ 滚轮 +
//! 滑块拖拽;viewport 根元素 `overflow_hidden` 裁剪,内容为**子元素闭包**
//! (每帧重建,RenderOnce 面板同款;逐行建实体是反模式 CMP-06)。轴向二选一
//! ([`ScrollAxis::Vertical`] / [`ScrollAxis::Horizontal`]),LayerPanel/
//! Inspector 用竖向、Timeline 用横向。
//!
//! # 物理(复用 [`ScrollPhysics`],零另立物理值)
//!
//! 惯性/橡皮筋**全部**来自 [`crate::anim::scroll::ScrollPhysics`](第 8 组
//! ANI 项的地盘,本模块只消费):
//!
//! ```text
//! 滚轮:  base = physics.overscroll(delta, current, 0, max)   // 立即应用+橡皮筋
//! 每帧:  render = base + physics.offset_at(now - anchor)      // fling/回弹位移
//! ```
//!
//! 时间常量(衰减 τ、橡皮筋阻尼/饱和)单一源自 anim/scroll.rs,本文件没有任何
//! 新物理值;惯性入口 [`ScrollState::fling`] 收宿主给定的初速度(拖拽释放/
//! 手势),滚轮路径不做 px→速度换算(不另立手感系数)。
//!
//! # 状态机(纯函数,可测;TC-CMP-SCROLL-01 的断言面)
//!
//! [`ScrollState`]:offset 钳制([`clamp_offset`])/滑块几何
//! ([`thumb_geometry`])/拖拽映射([`offset_from_drag`])/惯性推进
//! ([`ScrollState::render_offset`] + [`ScrollState::settle`])全部纯函数、
//! 时间显式注入。**边界语义**:滚轮越界 = 橡皮筋伸长(base 在界外,回弹由
//! `offset_at` 收敛回边界);惯性越界 = 撞墙即停(立即落界终止滑行,弹跳
//! 视觉专属橡皮筋路径)。reduced_motion 由 [`ScrollPhysics`] 内部直通收敛
//! (瞬时滑行/回弹)。
//!
//! 滚动条三态显形([`ScrollbarState`] → 目标透明度 + [`ScrollbarFade`]
//! STATE 档 120ms 淡变):Idle 隐藏 / Hovered 半显 / Dragging 全显。
//!
//! # v0.1 尺寸契约(如实声明)
//!
//! 视口/内容沿轴长度由宿主在构造时声明(`viewport_len`/`content_len`,本
//! 组件据此外层定尺寸):GPUI 无同步内容测量口,面板场景(行数 × 行高)
//! 宿主可知;内容尺寸变化时调 [`ScrollArea::set_content_len`] 重钳。
//! 真测量接入 = 后续批次(接 gpui 布局回调)。

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, Context, ElementId, FocusHandle, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render,
    ScrollWheelEvent, StatefulInteractiveElement as _, Styled, Window, canvas, div, px,
};

use crate::anim::ScrollPhysics;
use crate::anim::reduced_motion;
use crate::interact::{self, Semantic, SemanticRole, now_ms, semantic_slot};
use crate::theme::theme;
use crate::tokens::{ColorTokens, MotionTokens, RadiusTokens};

// ---------------------------------------------------------------------------
// 常量域(几何具名常量;颜色一律令牌;物理值一律 ScrollPhysics)
// ---------------------------------------------------------------------------

/// 滚动条命中条宽(8px;视觉滑块细于它,命中区更宽,A11Y-03 精神)。
pub const TRACK_WIDTH_PX: f32 = 8.0;
/// 滑块视觉宽(4px,细滚动条规格,§5.6 #8)。
pub const THUMB_WIDTH_PX: f32 = 4.0;
/// 滑块最小长(24px:长内容下滑块保持可抓握)。
pub const MIN_THUMB_LEN_PX: f64 = 24.0;
/// 滚动条三态 · 静止:隐藏。
pub const SCROLLBAR_IDLE_OPACITY: f64 = 0.0;
/// 滚动条三态 · 悬停:半显。
pub const SCROLLBAR_HOVER_OPACITY: f64 = 0.7;
/// 滚动条三态 · 拖拽:全显。
pub const SCROLLBAR_DRAG_OPACITY: f64 = 1.0;
/// 静止判定阈值(px):滑行残余 < 它视为停(0.25px ≈ 视觉静止;非物理值,
/// 停帧判据)。
const SETTLE_EPS_PX: f64 = 0.25;
/// 滚轮线步长(px/行;NumberField 的 SCROLL_LINE_PX 同值,行高步长惯例)。
const SCROLL_LINE_PX: f32 = 24.0;

// ---------------------------------------------------------------------------
// 纯函数层(offset/滑块几何/拖拽映射;TC-CMP-SCROLL-01 的被测单点)
// ---------------------------------------------------------------------------

/// 可滚动最大偏移(纯函数):`max(0, content - viewport)`;非有限入参 → 0。
#[must_use]
pub fn max_offset(content_len: f64, viewport_len: f64) -> f64 {
    if !content_len.is_finite() || !viewport_len.is_finite() {
        return 0.0;
    }
    (content_len - viewport_len).max(0.0)
}

/// offset 钳制(纯函数):钳入 `[0, max_offset]`;NaN → 0(防御,不 panic)。
#[must_use]
pub fn clamp_offset(offset: f64, content_len: f64, viewport_len: f64) -> f64 {
    if !offset.is_finite() {
        return 0.0;
    }
    offset.clamp(0.0, max_offset(content_len, viewport_len))
}

/// 滑块几何(纯函数):`(沿轴位置, 滑块长)` px。滑块长 = 视口占比 × 轨道长,
/// 钳在 `[MIN_THUMB_LEN_PX, 轨道长]`;无溢出/非法轨道 → `(0, 0)`(隐藏)。
#[must_use]
pub fn thumb_geometry(
    offset: f64,
    content_len: f64,
    viewport_len: f64,
    track_len: f64,
) -> (f64, f64) {
    let max = max_offset(content_len, viewport_len);
    if !track_len.is_finite() || track_len <= 0.0 || max <= 0.0 {
        return (0.0, 0.0);
    }
    let len = (track_len * viewport_len / content_len)
        .max(MIN_THUMB_LEN_PX)
        .min(track_len);
    let pos = (offset / max).clamp(0.0, 1.0) * (track_len - len);
    (pos, len)
}

/// 拖拽映射(纯函数):指针沿轴位置 → offset。抓取点保持(grab = 按下时
/// 指针相对滑块头的偏移),映射比 = `max / (轨道长 - 滑块长)`,两端钳制;
/// 轨道-滑块 ≤ 1px 时按 1px 防除零。无溢出 → 0。
#[must_use]
pub fn offset_from_drag(
    pointer_along_track: f64,
    grab_in_thumb: f64,
    thumb_len: f64,
    track_len: f64,
    content_len: f64,
    viewport_len: f64,
) -> f64 {
    let max = max_offset(content_len, viewport_len);
    if max <= 0.0 {
        return 0.0;
    }
    let span = (track_len - thumb_len).max(1.0);
    let frac = ((pointer_along_track - grab_in_thumb) / span).clamp(0.0, 1.0);
    frac * max
}

/// 滚动条显形三态(§5.6 #8:hover 显形)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollbarState {
    /// 静止:隐藏
    Idle,
    /// 悬停于滚动区:半显
    Hovered,
    /// 拖拽滑块:全显
    Dragging,
}

/// 三态 → 目标透明度(纯函数,值域单调:Idle < Hovered < Dragging)。
#[must_use]
pub fn scrollbar_target_opacity(state: ScrollbarState) -> f64 {
    match state {
        ScrollbarState::Idle => SCROLLBAR_IDLE_OPACITY,
        ScrollbarState::Hovered => SCROLLBAR_HOVER_OPACITY,
        ScrollbarState::Dragging => SCROLLBAR_DRAG_OPACITY,
    }
}

/// 滚动条淡变状态机(纯函数):STATE 档([`MotionTokens::DUR_STATE_MS`],
/// 120ms)线性淡变;`reduced` 直通目标值(显式参数注入,不读全局态)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarFade {
    value: f64,
    target: f64,
    started_ms: f64,
}

impl Default for ScrollbarFade {
    fn default() -> Self {
        ScrollbarFade {
            value: SCROLLBAR_IDLE_OPACITY,
            target: SCROLLBAR_IDLE_OPACITY,
            started_ms: 0.0,
        }
    }
}

impl ScrollbarFade {
    /// 隐藏态新状态机。
    pub fn new() -> Self {
        Self::default()
    }

    /// 目标透明度(渲染期比较用,避免重复发起淡变)。
    #[must_use]
    pub fn target(&self) -> f64 {
        self.target
    }

    /// 发起淡变(同值幂等)。
    pub fn set_target(&mut self, target: f64, now_ms: f64) {
        if !target.is_finite() || !now_ms.is_finite() {
            return;
        }
        self.target = target.clamp(0.0, 1.0);
        self.started_ms = now_ms;
    }

    /// 当前透明度(沉降式:到点落定;`reduced` 直通目标)。
    pub fn opacity_at(&mut self, now_ms: f64, reduced: bool) -> f64 {
        if !now_ms.is_finite() {
            return self.value;
        }
        let elapsed = now_ms - self.started_ms;
        if reduced || elapsed >= MotionTokens::DUR_STATE_MS {
            self.value = self.target;
            return self.target;
        }
        self.value + (self.target - self.value) * (elapsed / MotionTokens::DUR_STATE_MS)
    }

    /// 淡变是否进行中(为真时宿主应续帧)。
    #[must_use]
    pub fn is_running_at(&self, now_ms: f64) -> bool {
        now_ms.is_finite()
            && self.value != self.target
            && (now_ms - self.started_ms) < MotionTokens::DUR_STATE_MS
    }
}

/// 滑块拖拽会话(抓取点保持映射的参数快照)。
#[derive(Clone, Copy, Debug, PartialEq)]
struct ThumbDrag {
    grab_in_thumb: f64,
    thumb_len: f64,
    track_len: f64,
}

/// 滚动位置状态机(纯函数核,时间显式注入):
///
/// - `base` = 滚轮 [`ScrollPhysics::overscroll`] 立即应用后的基准(界内值,
///   或橡皮筋伸长后的**界外值**);
/// - 每帧渲染 = `base + physics.offset_at(now - anchor)`;基准在界内时对
///   结果钳制(惯性越界 = 撞墙即停),基准在界外(橡皮筋伸长期)放行
///   (回弹由 `offset_at` 收敛回边界);
/// - [`Self::settle`] 每帧调用:撞墙立即落界;滑行/回弹收敛带内
///   ([`SETTLE_EPS_PX`])重定基、清速度并落界——宿主随后停帧。
#[derive(Clone, Debug, Default)]
pub struct ScrollState {
    base: f64,
    anchor_ms: f64,
    physics: ScrollPhysics,
    drag: Option<ThumbDrag>,
}

impl ScrollState {
    /// 静止在顶部的状态机。
    pub fn new() -> Self {
        ScrollState::default()
    }

    /// 静止基准(测试/宿主可读;渲染值请用 [`Self::render_offset`])。
    #[must_use]
    pub fn offset(&self) -> f64 {
        self.base
    }

    /// 内部速度透传(px/s;宿主调试/手势接续可读)。
    #[must_use]
    pub fn velocity(&self) -> f64 {
        self.physics.velocity()
    }

    /// 是否拖拽滑块中。
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// 一次滚轮步(立即应用 + 橡皮筋):以当前渲染位置为基准走
    /// [`ScrollPhysics::overscroll`] 并重定锚点。`delta_px` 带符号
    /// (正 = offset 增大 = 露出更下方/右方内容)。无可滚动量时不动作。
    pub fn wheel_step(&mut self, delta_px: f64, now_ms: f64, content_len: f64, viewport_len: f64) {
        if !delta_px.is_finite() || !now_ms.is_finite() {
            return;
        }
        if max_offset(content_len, viewport_len) <= 0.0 {
            self.base = 0.0; // 无溢出:钉顶(不产生橡皮筋)
            return;
        }
        let current = self.render_offset(now_ms, content_len, viewport_len);
        let max = max_offset(content_len, viewport_len);
        self.base = self.physics.overscroll(delta_px, current, 0.0, max);
        self.anchor_ms = now_ms;
    }

    /// 惯性入口(px/s,宿主给定初速度:拖拽释放/手势接续)。滚轮路径**不**
    /// 自动调用(不另立 px→速度手感系数,物理值全部单一源自 ScrollPhysics)。
    pub fn fling(&mut self, velocity_px_per_s: f64, now_ms: f64) {
        if !velocity_px_per_s.is_finite() || !now_ms.is_finite() {
            return;
        }
        self.physics.push_scroll_delta(velocity_px_per_s);
        self.anchor_ms = now_ms;
    }

    /// 该时刻的渲染 offset(纯函数):基准在界内 → 钳制(惯性越界撞墙);
    /// 基准在界外(橡皮筋伸长期)→ 放行(回弹收敛回边界)。
    #[must_use]
    pub fn render_offset(&self, now_ms: f64, content_len: f64, viewport_len: f64) -> f64 {
        let max = max_offset(content_len, viewport_len);
        let residual = self.physics.offset_at(now_ms - self.anchor_ms);
        let raw = self.base + residual;
        if self.base < 0.0 || self.base > max {
            raw
        } else {
            clamp_offset(raw, content_len, viewport_len)
        }
    }

    /// 每帧收口(组件渲染循环调用):撞墙立即落界;滑行/回弹收敛带内重定基、
    /// 清速度;返回应渲染的 offset。
    pub fn settle(&mut self, now_ms: f64, content_len: f64, viewport_len: f64) -> f64 {
        if !now_ms.is_finite() {
            return self.render_offset(now_ms, content_len, viewport_len);
        }
        let max = max_offset(content_len, viewport_len);
        let raw = self.base + self.physics.offset_at(now_ms - self.anchor_ms);
        let clamped = clamp_offset(raw, content_len, viewport_len);
        let base_in_bounds = self.base >= 0.0 && self.base <= max;
        // 惯性撞墙:界内基准的 raw 越界 → 立即落界终止滑行(弹跳专属橡皮筋)
        if base_in_bounds && (raw < -SETTLE_EPS_PX || raw > max + SETTLE_EPS_PX) {
            self.base = clamped;
            self.physics.reset();
            self.anchor_ms = now_ms;
            return clamped;
        }
        // 滑行/回弹收敛(残余 ≤ 阈值):重定基、停帧
        if self.residual_settled(now_ms) {
            self.base = clamped;
            self.physics.reset();
            self.anchor_ms = now_ms;
            return clamped;
        }
        self.render_offset(now_ms, content_len, viewport_len)
    }

    /// 是否仍在滑行/回弹(为真时宿主应续帧;reduced_motion 下恒假——
    /// ScrollPhysics 直通收敛位移,一步到位)。
    #[must_use]
    pub fn is_flowing(&self, now_ms: f64) -> bool {
        !self.residual_settled(now_ms)
    }

    /// 程序化定位:丢弃进行中的滑行/伸长,直接钳位到 `offset`。
    pub fn snap_to(&mut self, offset: f64, content_len: f64, viewport_len: f64) {
        self.base = clamp_offset(offset, content_len, viewport_len);
        self.physics.reset();
        self.drag = None;
    }

    /// 开始滑块拖拽:丢弃进行中的滑行/伸长(从钳制位起步),记录抓取映射。
    /// `thumb` = [`thumb_geometry`] 的 `(沿轴位置, 滑块长)`。
    pub fn begin_thumb_drag(
        &mut self,
        pointer_along_track: f64,
        thumb: (f64, f64),
        track_len: f64,
        now_ms: f64,
        content_len: f64,
        viewport_len: f64,
    ) {
        let (thumb_pos, thumb_len) = thumb;
        if !pointer_along_track.is_finite() || !now_ms.is_finite() {
            return;
        }
        self.base = clamp_offset(self.base, content_len, viewport_len);
        self.physics.reset();
        self.anchor_ms = now_ms;
        self.drag = Some(ThumbDrag {
            grab_in_thumb: (pointer_along_track - thumb_pos).clamp(0.0, thumb_len.max(0.0)),
            thumb_len,
            track_len,
        });
    }

    /// 拖拽推进:指针沿轴位置 → 新 offset(抓取点保持,两端钳制)。
    /// 返回新 offset;非拖拽态原样返回基准。
    pub fn drag_thumb(
        &mut self,
        pointer_along_track: f64,
        content_len: f64,
        viewport_len: f64,
    ) -> f64 {
        let Some(drag) = self.drag else {
            return self.base;
        };
        if !pointer_along_track.is_finite() {
            return self.base;
        }
        self.base = offset_from_drag(
            pointer_along_track,
            drag.grab_in_thumb,
            drag.thumb_len,
            drag.track_len,
            content_len,
            viewport_len,
        );
        self.base
    }

    /// 结束拖拽(返回真 = 有会话被结束)。
    pub fn end_thumb_drag(&mut self) -> bool {
        self.drag.take().is_some()
    }

    /// 滑行/回弹是否已收敛(残余位移与总位移之差 ≤ [`SETTLE_EPS_PX`])。
    #[must_use]
    fn residual_settled(&self, now_ms: f64) -> bool {
        if !now_ms.is_finite() {
            return true;
        }
        let total = self.physics.offset_at(1e9); // = v₀·τ(收敛总位移)
        let residual = self.physics.offset_at((now_ms - self.anchor_ms).max(0.0));
        (total - residual).abs() <= SETTLE_EPS_PX
    }
}

// ---------------------------------------------------------------------------
// 组件(builder + Entity)
// ---------------------------------------------------------------------------

/// 滚动轴向(二选一;竖向面板 / 横向时间轴)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollAxis {
    /// 竖向(滚轮 y 分量;滚动条贴右缘)
    Vertical,
    /// 横向(滚轮 x 分量;滚动条贴下缘)
    Horizontal,
}

/// 内容子元素闭包(每帧重建;宿主把当前数据投影为元素,受控面板同款)。
pub type ScrollContentFn = Box<dyn Fn() -> AnyElement>;

/// 统一滚动容器(§5.6 #8)。宿主声明沿轴视口/内容长度(px,v0.1 契约见
/// 模块 doc),内容为子元素闭包:
///
/// ```ignore
/// cx.new(|_| ScrollArea::new("layers", 240.0, rows * 28.0, move || {
///     div().child("…行…").into_any_element()
/// }))
/// ```
pub struct ScrollArea {
    id: ElementId,
    axis: ScrollAxis,
    viewport_len: f32,
    content_len: f32,
    content: ScrollContentFn,
    state: ScrollState,
    fade: ScrollbarFade,
    hovered: bool,
    /// 本元素 painted bounds(布局期经 canvas 捕获;鼠标坐标 → 轨道坐标用)
    bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
    /// A11Y-01:键盘滚动焦点(容器 track_focus;轴向对应的方向键滚动)
    focus: Option<FocusHandle>,
    /// A11Y-02 语义槽(可访问名;role 默认 ScrollRegion)
    semantic: Semantic,
}

impl ScrollArea {
    /// 构造:元素 id + 沿轴视口长 + 沿轴内容长(px)+ 内容闭包。默认竖向。
    pub fn new(
        id: impl Into<ElementId>,
        viewport_len: f32,
        content_len: f32,
        content: impl Fn() -> AnyElement + 'static,
    ) -> Self {
        ScrollArea {
            id: id.into(),
            axis: ScrollAxis::Vertical,
            viewport_len: viewport_len.max(0.0),
            content_len: content_len.max(0.0),
            content: Box::new(content),
            state: ScrollState::new(),
            fade: ScrollbarFade::new(),
            hovered: false,
            bounds: Rc::new(RefCell::new(None)),
            focus: None,
            semantic: Semantic::new(),
        }
    }

    /// 轴向(默认 [`ScrollAxis::Vertical`])。
    #[must_use]
    pub fn axis(mut self, axis: ScrollAxis) -> Self {
        self.axis = axis;
        self
    }

    /// 内容长度变化(行增删/数据刷新)时更新并重钳偏移。
    pub fn set_content_len(&mut self, content_len: f32, cx: &mut Context<Self>) {
        self.content_len = content_len.max(0.0);
        self.state
            .snap_to(self.state.offset(), self.content_len(), self.viewport_len());
        cx.notify();
    }

    /// 程序化定位(钳制;丢弃进行中的滑行)。
    pub fn scroll_to(&mut self, offset: f64, cx: &mut Context<Self>) {
        self.state
            .snap_to(offset, self.content_len(), self.viewport_len());
        cx.notify();
    }

    /// 当前渲染 offset(宿主调试可读)。
    #[must_use]
    pub fn scroll_offset(&self) -> f64 {
        self.state.offset()
    }

    /// 键盘滚动(A11Y-01):聚焦后按轴向消费方向键(竖向 ↑↓ / 横向 ←→),
    /// 一次一行([`SCROLL_LINE_PX`];物理与滚轮同路 [`ScrollState::wheel_step`],
    /// reduced_motion 由 ScrollPhysics 直通收敛)。Home/End = 滚到顶/底。
    fn on_key_scroll(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let now = now_ms();
        let (content, viewport) = (self.content_len(), self.viewport_len());
        let step = f64::from(SCROLL_LINE_PX);
        let vertical = self.axis == ScrollAxis::Vertical;
        let intent = match key {
            "up" if vertical => Some(step),
            "down" if vertical => Some(-step),
            "left" if !vertical => Some(step),
            "right" if !vertical => Some(-step),
            "home" => Some(f64::MAX / 4.0),
            "end" => Some(-(f64::MAX / 4.0)),
            _ => None,
        };
        if let Some(delta) = intent {
            self.state.wheel_step(delta, now, content, viewport);
            cx.notify();
        }
    }

    fn content_len(&self) -> f64 {
        f64::from(self.content_len)
    }

    fn viewport_len(&self) -> f64 {
        f64::from(self.viewport_len)
    }

    // —— 事件(cx.listener 形态;NumberField 同款接线)——

    fn on_hover_changed(&mut self, hovered: &bool, _window: &mut Window, cx: &mut Context<Self>) {
        if self.hovered != *hovered {
            self.hovered = *hovered;
            cx.notify();
        }
    }

    fn on_wheel(&mut self, event: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // winit 惯例:滚轮向下 delta.y < 0 → offset 增大(露出更下/右内容)
        let delta = event.delta.pixel_delta(px(SCROLL_LINE_PX));
        let d = match self.axis {
            ScrollAxis::Vertical => -f64::from(delta.y),
            ScrollAxis::Horizontal => -f64::from(delta.x),
        };
        if d == 0.0 {
            return;
        }
        self.state
            .wheel_step(d, now_ms(), self.content_len(), self.viewport_len());
        cx.notify();
    }

    fn on_thumb_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != gpui::MouseButton::Left {
            return;
        }
        let Some(bounds) = self.bounds.borrow().as_ref().map(|b| *b) else {
            return;
        };
        let along = self.along_axis(
            f64::from(event.position.y),
            f64::from(event.position.x),
            &bounds,
        );
        let (thumb_pos, thumb_len) = thumb_geometry(
            self.state
                .render_offset(now_ms(), self.content_len(), self.viewport_len()),
            self.content_len(),
            self.viewport_len(),
            self.viewport_len(),
        );
        self.state.begin_thumb_drag(
            along,
            (thumb_pos, thumb_len),
            self.viewport_len(),
            now_ms(),
            self.content_len(),
            self.viewport_len(),
        );
        cx.notify();
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.state.is_dragging() {
            return;
        }
        if event.pressed_button != Some(gpui::MouseButton::Left) {
            return; // 拖出元素后松键由 on_mouse_up_out 兜底(NumberField 同款)
        }
        let Some(bounds) = self.bounds.borrow().as_ref().map(|b| *b) else {
            return;
        };
        let along = self.along_axis(
            f64::from(event.position.y),
            f64::from(event.position.x),
            &bounds,
        );
        self.state
            .drag_thumb(along, self.content_len(), self.viewport_len());
        cx.notify();
    }

    fn on_mouse_up_out(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.end_thumb_drag() {
            cx.notify();
        }
    }

    /// 指针位置 → 沿轴轨道坐标(px;以 painted bounds 原点为基准)。
    fn along_axis(&self, y: f64, x: f64, bounds: &Bounds<Pixels>) -> f64 {
        match self.axis {
            ScrollAxis::Vertical => y - f64::from(bounds.origin.y),
            ScrollAxis::Horizontal => x - f64::from(bounds.origin.x),
        }
    }
}

impl Render for ScrollArea {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors: &ColorTokens = &theme(cx).colors;
        let now = now_ms();
        let reduced = reduced_motion();
        let content_len = self.content_len();
        let viewport_len = self.viewport_len();

        // 状态机收口(撞墙落界/收敛重定基)+ 当前渲染 offset
        let offset = self.state.settle(now, content_len, viewport_len);
        let scrollable = max_offset(content_len, viewport_len) > 0.0;

        // 滚动条三态:拖拽 > 悬停 > 静止;目标透明度变化才发起淡变
        let bar_state = if self.state.is_dragging() {
            ScrollbarState::Dragging
        } else if self.hovered {
            ScrollbarState::Hovered
        } else {
            ScrollbarState::Idle
        };
        let target = scrollbar_target_opacity(bar_state);
        if (self.fade.target() - target).abs() > 1e-9 {
            self.fade.set_target(target, now);
        }
        let bar_opacity = f32v(self.fade.opacity_at(now, reduced));
        let dragging = self.state.is_dragging();
        let (thumb_pos, thumb_len) =
            thumb_geometry(offset, content_len, viewport_len, viewport_len);

        // 内容闭包(每帧重建)+ 视口位移(绝对定位负偏移,根元素裁剪)
        let content_child = (self.content)();
        let shifted = match self.axis {
            ScrollAxis::Vertical => div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(-f32v(offset)))
                .child(content_child),
            ScrollAxis::Horizontal => div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(-f32v(offset)))
                .child(content_child),
        };

        // 布局期 bounds 捕获(鼠标坐标 → 轨道坐标;canvas 兜底,零绘制)
        let bounds_rc = Rc::clone(&self.bounds);
        let bounds_recorder = canvas(
            move |bounds, _window, _cx| {
                *bounds_rc.borrow_mut() = Some(bounds);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0();

        // 根 = 视口(id + 相对定位 + 裁剪)+ 悬停/滚轮/拖拽接线
        let mut root = match self.axis {
            ScrollAxis::Vertical => div()
                .id(self.id.clone())
                .relative()
                .w_full()
                .h(px(self.viewport_len))
                .overflow_hidden(),
            ScrollAxis::Horizontal => div()
                .id(self.id.clone())
                .relative()
                .h_full()
                .w(px(self.viewport_len))
                .overflow_hidden(),
        }
        .on_hover(cx.listener(Self::on_hover_changed))
        .on_scroll_wheel(cx.listener(Self::on_wheel))
        .on_mouse_move(cx.listener(Self::on_mouse_move))
        .on_mouse_up_out(gpui::MouseButton::Left, cx.listener(Self::on_mouse_up_out))
        .child(bounds_recorder)
        .child(shifted);

        // 自绘滚动条(细:轨道半透 + 4px 胶囊滑块;三态淡变;拖拽接线)
        if scrollable {
            let thumb_bg = if dragging {
                colors.text_disabled
            } else {
                colors.border_strong
            };
            let half = (TRACK_WIDTH_PX - THUMB_WIDTH_PX) / 2.0;
            let thumb = match self.axis {
                ScrollAxis::Vertical => div()
                    .absolute()
                    .right(px(half))
                    .top(px(f32v(thumb_pos)))
                    .w(px(THUMB_WIDTH_PX))
                    .h(px(f32v(thumb_len.max(0.0))))
                    .rounded(px(THUMB_WIDTH_PX / 2.0))
                    .bg(thumb_bg),
                ScrollAxis::Horizontal => div()
                    .absolute()
                    .bottom(px(half))
                    .left(px(f32v(thumb_pos)))
                    .h(px(THUMB_WIDTH_PX))
                    .w(px(f32v(thumb_len.max(0.0))))
                    .rounded(px(THUMB_WIDTH_PX / 2.0))
                    .bg(thumb_bg),
            }
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(Self::on_thumb_down));
            let track = match self.axis {
                ScrollAxis::Vertical => div()
                    .absolute()
                    .right_0()
                    .top_0()
                    .bottom_0()
                    .w(px(TRACK_WIDTH_PX)),
                ScrollAxis::Horizontal => div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(px(TRACK_WIDTH_PX)),
            }
            .bg(colors.border_subtle)
            .opacity(bar_opacity)
            .child(thumb);
            root = root.child(track);
        }

        // A11Y-01:键盘滚动焦点(tab_stop 进 Tab 环游)+ 焦点环
        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let focused = focus.is_focused(window);
        let mut root = root
            .track_focus(&focus)
            .on_key_down(cx.listener(Self::on_key_scroll));
        if focused {
            root = root
                .relative()
                .children(interact::focus_ring(colors.accent, RadiusTokens::SM));
        }

        // 帧泵:滑行/回弹或淡变进行中才续帧(静止零帧提交,分册六 §4.4)
        if self.state.is_flowing(now) || self.fade.is_running_at(now) {
            window.request_animation_frame();
        }

        root
    }
}

// A11Y-02 语义槽:可访问名缺省"滚动区"、role 缺省 ScrollRegion。
semantic_slot!(ScrollArea);

impl ScrollArea {
    /// 解析语义(A11Y-02):显式 `.label(...)`/`.role(...)` 优先,缺省 =
    /// ("滚动区", ScrollRegion)。
    #[must_use]
    pub fn resolved_semantic(&self) -> Semantic {
        let sem = match self.semantic.label() {
            Some(_) => self.semantic.clone(),
            None => self.semantic.clone().with_label("滚动区"),
        };
        let role = sem.role().unwrap_or(SemanticRole::ScrollRegion);
        sem.with_role(role)
    }
}

// ---------------------------------------------------------------------------
// 工具
// ---------------------------------------------------------------------------

#[allow(clippy::cast_possible_truncation)]
fn f32v(v: f64) -> f32 {
    v as f32
}

// ---------------------------------------------------------------------------
// TC-CMP-SCROLL-01:offset 钳制 + 拖拽映射 + 惯性推进(复用 ScrollPhysics
// 同一物理常量断言)+ 滚动条三态显形。全部纯函数断言,不经 GUI。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const CONTENT: f64 = 1000.0;
    const VIEWPORT: f64 = 200.0;

    #[test]
    fn tc_cmp_scroll_01_offset_clamp_and_thumb_geometry() {
        // max_offset / clamp_offset
        assert_eq!(max_offset(CONTENT, VIEWPORT), 800.0);
        assert_eq!(max_offset(100.0, 200.0), 0.0, "无溢出 → 0");
        assert_eq!(clamp_offset(-50.0, CONTENT, VIEWPORT), 0.0, "负向钳顶");
        assert_eq!(clamp_offset(900.0, CONTENT, VIEWPORT), 800.0, "越界钳底");
        assert_eq!(clamp_offset(424.0, CONTENT, VIEWPORT), 424.0, "界内原样");
        assert_eq!(clamp_offset(f64::NAN, CONTENT, VIEWPORT), 0.0, "NaN 防御");
        assert_eq!(clamp_offset(50.0, f64::NAN, VIEWPORT), 0.0, "非法区间钳 0");

        // 滑块几何:占比长度、比例位置、最小长、隐藏
        let (pos, len) = thumb_geometry(0.0, CONTENT, VIEWPORT, VIEWPORT);
        assert_eq!(pos, 0.0, "顶部滑块贴顶");
        assert!(
            (len - VIEWPORT * VIEWPORT / CONTENT).abs() < 1e-9,
            "长 = 视口占比"
        );
        let (pos_end, _) = thumb_geometry(800.0, CONTENT, VIEWPORT, VIEWPORT);
        assert!(
            (pos_end + len - VIEWPORT).abs() < 1e-9,
            "底部滑块贴底(位置 + 长 = 轨道长)"
        );
        // 极长内容:滑块长钳最小值,位置仍成比例
        let (pos, len) = thumb_geometry(9976.0, 100_000.0, 200.0, 200.0);
        assert_eq!(len, MIN_THUMB_LEN_PX, "极长内容滑块 = 最小长");
        let expect_pos = (9976.0 / 99_800.0) * (200.0 - MIN_THUMB_LEN_PX);
        assert!((pos - expect_pos).abs() < 1e-9, "最小长下位置仍成比例");
        assert_eq!(
            thumb_geometry(0.0, 100.0, 200.0, 200.0),
            (0.0, 0.0),
            "无溢出隐藏"
        );
        assert_eq!(
            thumb_geometry(0.0, CONTENT, VIEWPORT, 0.0),
            (0.0, 0.0),
            "零轨道隐藏"
        );
        // 往返:offset → 滑块位置 → 拖拽映射恢复同一 offset(抓取点 = 0)
        for offset in [0.0, 123.0, 800.0] {
            let (pos, len) = thumb_geometry(offset, CONTENT, VIEWPORT, VIEWPORT);
            let back = offset_from_drag(pos, 0.0, len, VIEWPORT, CONTENT, VIEWPORT);
            assert!(
                (back - offset).abs() < 1e-9,
                "几何-拖拽往返一致:{offset} → {pos} → {back}"
            );
        }
    }

    #[test]
    fn tc_cmp_scroll_01_drag_mapping_keeps_grab_point() {
        let track = 200.0;
        let (thumb_pos, thumb_len) = thumb_geometry(100.0, CONTENT, VIEWPORT, track);
        // 在滑块中点按下:grab = thumb_len/2
        let press = thumb_pos + thumb_len / 2.0;
        let mut state = ScrollState::new();
        state.snap_to(100.0, CONTENT, VIEWPORT);
        state.begin_thumb_drag(press, (thumb_pos, thumb_len), track, 0.0, CONTENT, VIEWPORT);
        assert!(state.is_dragging());
        // 指针移动 Δ → offset 移动 Δ × (max / (轨道 - 滑块)),抓取点不跳
        let ratio = 800.0 / (track - thumb_len);
        let moved = state.drag_thumb(press + 10.0, CONTENT, VIEWPORT);
        assert!(
            (moved - 100.0 - 10.0 * ratio).abs() < 1e-9,
            "映射比线性:{moved}"
        );
        // 两端钳制
        let top = state.drag_thumb(0.0, CONTENT, VIEWPORT);
        assert_eq!(top, 0.0, "拖过头钳顶");
        let bottom = state.drag_thumb(track + 500.0, CONTENT, VIEWPORT);
        assert_eq!(bottom, 800.0, "拖过头钳底");
        assert!(state.end_thumb_drag(), "结束返回真");
        assert!(!state.is_dragging());
        assert_eq!(
            state.drag_thumb(50.0, CONTENT, VIEWPORT),
            bottom,
            "无会话不动作"
        );
    }

    #[test]
    fn tc_cmp_scroll_01_inertia_uses_scrollphysics_constants() {
        // 惯性推进与 ScrollPhysics 同一物理常量:同一初速度下,ScrollState 的
        // 渲染 offset 序列 = base + ScrollPhysics::offset_at(t)(独立实例对照)
        let v0 = 1000.0;
        let mut state = ScrollState::new();
        state.snap_to(0.0, CONTENT, VIEWPORT);
        state.fling(v0, 1000.0);
        let mut reference = ScrollPhysics::new();
        reference.push_scroll_delta(v0);
        let mut prev = 0.0;
        for step in 1..=20 {
            let t = f64::from(step) * 20.0;
            let rendered = state.render_offset(1000.0 + t, CONTENT, VIEWPORT);
            let expect = clamp_offset(reference.offset_at(t), CONTENT, VIEWPORT);
            assert!(
                (rendered - expect).abs() < 1e-9,
                "t={t}: {rendered} != ScrollPhysics 对照 {expect}"
            );
            assert!(rendered >= prev, "滑行单调递增:t={t}");
            prev = rendered;
        }
        // 总位移收敛于 v₀·τ(物理常量,τ 单一源自 anim/scroll.rs)
        assert!(state.is_flowing(1000.0 + 16.0), "滑行中需续帧");
        let settled_offset = state.settle(1000.0 + 60_000.0, CONTENT, VIEWPORT);
        assert!(
            (settled_offset - v0 * 0.35).abs() < 1.0,
            "收敛到 v₀·τ 附近(钳制带内)"
        );
        assert_eq!(state.velocity(), 0.0, "收敛后速度清零(停帧)");
        assert!(!state.is_flowing(1000.0 + 60_000.0));

        // 惯性撞墙:界内基准滑行越界 → 立即落界,不放行(弹跳专属橡皮筋)
        let mut wall = ScrollState::new();
        wall.snap_to(750.0, CONTENT, VIEWPORT);
        wall.fling(5000.0, 2000.0);
        let stopped = wall.settle(2000.0 + 16.0, CONTENT, VIEWPORT);
        assert_eq!(stopped, 800.0, "撞墙立即落底");
        assert!(!wall.is_flowing(2000.0 + 17.0), "撞墙后停帧");
    }

    #[test]
    fn tc_cmp_scroll_01_wheel_rubber_band_reuses_physics_curve() {
        // 滚轮过界:伸长量逐位等于 ScrollPhysics::rubber_band(同一物理常量),
        // 回弹由 offset_at 收敛回边界
        let mut state = ScrollState::new();
        state.snap_to(800.0, CONTENT, VIEWPORT);
        state.wheel_step(40.0, 1000.0, CONTENT, VIEWPORT);
        let rendered = state.render_offset(1000.0, CONTENT, VIEWPORT);
        let expect_stretch = ScrollPhysics::rubber_band(40.0);
        assert!(
            (rendered - (800.0 + expect_stretch)).abs() < 1e-9,
            "伸长量 = 橡皮筋曲线原值:{rendered} vs {expect_stretch}"
        );
        // 回弹单调逼近边界,收敛后重定基、停帧
        let mut prev = rendered;
        for step in 1..=10 {
            let t = 1000.0 + f64::from(step) * 100.0;
            let r = state.render_offset(t, CONTENT, VIEWPORT);
            assert!(r < prev, "回弹应朝边界单调:t={t} {r} ≥ {prev}");
            prev = r;
        }
        let done = state.settle(1000.0 + 60_000.0, CONTENT, VIEWPORT);
        assert!((done - 800.0).abs() < SETTLE_EPS_PX, "回弹落底:{done}");
        assert_eq!(state.offset(), 800.0, "重定基到边界");
        assert!(!state.is_flowing(1000.0 + 60_001.0));
        // 界内滚轮:1:1 跟手,不引入速度(overscroll 界内直通语义)
        let mut plain = ScrollState::new();
        plain.wheel_step(50.0, 2000.0, CONTENT, VIEWPORT);
        assert_eq!(plain.offset(), 50.0);
        assert_eq!(plain.velocity(), 0.0, "界内滚轮不动速度");
        // 无溢出:滚轮钉顶,不产橡皮筋
        let mut flat = ScrollState::new();
        flat.wheel_step(40.0, 3000.0, 100.0, VIEWPORT);
        assert_eq!(flat.offset(), 0.0);
    }

    #[test]
    fn tc_cmp_scroll_01_reduced_motion_is_instant() {
        // reduced_motion 全局开关(测试单线程纪律下安全,anim/scroll.rs 同款):
        // 滑行/回弹一步收敛到位
        crate::anim::set_reduced_motion(true);
        let mut state = ScrollState::new();
        state.fling(1000.0, 1000.0);
        let rendered = state.render_offset(1001.0, CONTENT, VIEWPORT);
        assert!(
            (rendered - 1000.0 * 0.35).abs() < 1e-9,
            "减弱动态:滑行瞬时收敛到 v₀·τ,得 {rendered}"
        );
        assert!(!state.is_flowing(1001.0), "减弱动态不续帧");
        // 橡皮筋瞬时回弹
        let mut bounce = ScrollState::new();
        bounce.snap_to(800.0, CONTENT, VIEWPORT);
        bounce.wheel_step(40.0, 2000.0, CONTENT, VIEWPORT);
        let done = bounce.settle(2000.0, CONTENT, VIEWPORT);
        assert!(
            (done - 800.0).abs() < SETTLE_EPS_PX,
            "减弱动态:回弹瞬时到位:{done}"
        );
        crate::anim::set_reduced_motion(false);
    }

    #[test]
    fn tc_cmp_scroll_01_scrollbar_three_states_and_fade() {
        // 三态目标透明度:Idle 隐藏 < Hovered 半显 < Dragging 全显
        let idle = scrollbar_target_opacity(ScrollbarState::Idle);
        let hovered = scrollbar_target_opacity(ScrollbarState::Hovered);
        let dragging = scrollbar_target_opacity(ScrollbarState::Dragging);
        assert_eq!(idle, 0.0, "静止隐藏");
        assert!(hovered > idle, "悬停显形");
        assert_eq!(dragging, 1.0, "拖拽全显");

        // STATE 档(120ms)淡变:半程介于两态之间,120ms 精确落定
        assert_eq!(MotionTokens::DUR_STATE_MS, 120.0);
        let mut fade = ScrollbarFade::new();
        assert_eq!(fade.opacity_at(1000.0, false), 0.0, "初始隐藏");
        fade.set_target(hovered, 1000.0);
        let mid = fade.opacity_at(1000.0 + 60.0, false);
        assert!(mid > 0.0 && mid < hovered, "半程介于 0 与目标:{mid}");
        assert!(
            (fade.opacity_at(1000.0 + 120.0, false) - hovered).abs() < 1e-9,
            "120ms 落定"
        );
        assert!(!fade.is_running_at(1000.0 + 120.0), "落定停帧");
        // 淡出回隐藏
        fade.set_target(idle, 2000.0);
        assert!((fade.opacity_at(2000.0 + 120.0, false) - idle).abs() < 1e-9);
        // reduced 直通:目标立即生效,无中间值
        let mut direct = ScrollbarFade::new();
        direct.set_target(dragging, 3000.0);
        assert_eq!(
            direct.opacity_at(3000.0 + 1.0, true),
            dragging,
            "减弱动态直通"
        );
        assert!(!direct.is_running_at(3000.0 + 1.0), "减弱动态不续帧");
    }
}
