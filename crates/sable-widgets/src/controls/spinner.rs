//! 加载状态件:不确定态 [`Spinner`](pill 扫描 1.2s 循环)与确定态
//! [`Progress`](线性进度条 + 完成对勾 pop)。
//!
//! 迭代审查报告 2026-10-04 §5.6 组件矩阵 #14 / §5.9 状态设计表(CMP-05):
//! "不确定 pill 1.2s 循环;确定态线性 + 完成对勾 pop 120ms"。
//!
//! # ANI-01 #13 接线声明(进度条/导出)
//!
//! `interact.rs` 清单 #13"进度条/导出:线性 + 不确定态脉冲"的**组件层落点
//! 就是本模块**——确定态进度为纯函数线性直读([`progress_fill`],值即宽度,
//! 零动画队列、逐帧确定),不确定态为 1.2s 定周期循环([`SPIN_PERIOD_MS`]);
//! 宿主的导出/导入进度一律经 [`Progress::set_value`] 落地,不再自绘。
//!
//! # 周期常量的取舍(单点声明)
//!
//! [`SPIN_PERIOD_MS`] = 1200ms 是**循环周期**,与 [`crate::tokens::
//! MotionTokens`] 的四档**过渡时长**(0/80/120/200ms,语义为"从 A 到 B 的
//! 插值时长")不是同一类值;报告明写 1.2s,而 MotionTokens 表本轮不扩表
//! (tokens.rs 与 sable-tokens.json 的逐值同步门禁在另一批次统一扩),故
//! 本文件以具名常量单点承载、注明依据——后续 tokens 扩表时把消费点重指向
//! 即可,宿主零改动。
//!
//! # 纯函数规格(可测,TC-CMP-STATE-02 的断言面)
//!
//! 相位([`cycle_phase`])→ 扫描位置([`scan_progress`]/[`scan_x`])→
//! 像素;进度值([`progress_fill`] 钳制 [0,1])→ 填充宽度;完成迁移
//! ([`completion_transition`])→ 对勾 pop([`check_pop_scale`],120ms =
//! [`MotionTokens::DUR_STATE_MS`] STATE 档)。时间一律调用方注入,零
//! `Instant::now()`,全部可脱离 GUI 单测。
//!
//! # 减弱动态(A8,§5.9"加载态"降级)
//!
//! [`reduced_motion`] 为真:不确定态 = **静态弧线**(pill 停在起位,不请求
//! 动画帧);确定态 = 直接值(填充即终值),完成对勾**免 pop 直通全尺寸**。

use gpui::{
    App, Context, Div, FontWeight, IntoElement, ParentElement, Render, RenderOnce, Styled, Window,
    div, px,
};

use crate::anim::{Easing, reduced_motion};
use crate::interact::{self, Semantic, SemanticRole, semantic_slot};
use crate::theme::theme;
use crate::tokens::{MotionTokens, SpacingTokens, TextSize, h_flex};

// ---------------------------------------------------------------------------
// 周期与几何常量(具名单点;颜色一律令牌)
// ---------------------------------------------------------------------------

/// 不确定态循环周期(毫秒)。报告 §5.6 #14 明写 1.2s;取舍见模块 doc
/// ("周期常量的取舍"节)——循环周期与过渡时长(MotionTokens 四档)不同类,
/// 本文件单点承载,tokens 扩表后重指向。
pub const SPIN_PERIOD_MS: f64 = 1200.0;

/// 不确定态 pill 轨道默认宽度(px,4 网格;行内加载位)。
pub const SPINNER_TRACK_W_PX: f32 = 96.0;
/// 不确定态扫描 pill 宽度(px,轨道的 1/4 档)。
pub const SPINNER_PILL_W_PX: f32 = 24.0;
/// 确定态进度条默认宽度(px,4 网格;导出/长任务行)。
pub const PROGRESS_TRACK_W_PX: f32 = 160.0;
/// pill/进度条厚度(px,4 网格;胶囊形态的最小可辨厚度)。
pub const SPINNER_THICKNESS_PX: f32 = 4.0;
/// 扫描行程两端内衬(px):贴边扫描会与圆角打架,留 1/2 厚度呼吸位。
const PILL_INSET_PX: f32 = 2.0;
/// 完成对勾 pop 的起步缩放(从 0.5 长到 1.0,120ms OutCubic = "pop")。
const CHECK_START_SCALE: f32 = 0.5;
/// 完成对勾字形(§5.5 图标系统落地前的文字字形占位,Button.icon 同纪律)。
const CHECK_GLYPH: &str = "✓";
/// pop 的"已落定"阈值(秒):超过即停帧。
const POP_STATE_MS: f64 = MotionTokens::DUR_STATE_MS;

/// f64 动画值 → f32(GPU 域收口,button.rs 同款惯例)。
#[allow(clippy::cast_possible_truncation)]
fn f32v(v: f64) -> f32 {
    v as f32
}

// ---------------------------------------------------------------------------
// 纯函数层(相位 → 几何 → 状态迁移;TC-CMP-STATE-02 的被测单点)
// ---------------------------------------------------------------------------

/// 循环相位(纯函数):`now_ms` 折进 `[0, period)` 后归一化到 0..1。
/// 循环对纪元不敏感:负时刻经 `rem_euclid` 自然回绕(时钟回拨无损);
/// 非有限输入或非法周期(`≤ 0`/NaN)恒 0(静态,不 panic)。
#[must_use]
pub fn cycle_phase(now_ms: f64, period_ms: f64) -> f64 {
    if !period_ms.is_finite() || period_ms <= 0.0 || !now_ms.is_finite() {
        return 0.0;
    }
    let wrapped = now_ms.rem_euclid(period_ms);
    wrapped / period_ms
}

/// 扫描进度(纯函数):相位 0..1 → 0..1..0 的**往返**曲线(扫描是折返不是
/// 环绕:环绕 wrap 会在回卷帧产生位置跳变)。每半程 InOutCubic 缓动,两端
/// 自然减速,1.2s 单程一半。相位越界经 [`cycle_phase`] 语义折返处理不在此
/// 重复钳制(调用方传 0..1;越界值的行为 = 线性外推后钳制,无 panic)。
#[must_use]
pub fn scan_progress(phase: f64) -> f64 {
    // 三角波:0→1→0,周期 = 相位全长
    let tri = 1.0 - (2.0 * phase - 1.0).abs();
    let leg = tri.clamp(0.0, 1.0);
    Easing::InOutCubic.apply(leg)
}

/// 扫描 pill 的左缘 x(纯函数,几何单点):`inset + travel × scan_progress`,
/// 其中 `travel = track_w - pill_w - 2×inset`(轨道容不下 pill 时行程 0,
/// pill 恒在 inset 起位——防御不 panic)。
#[must_use]
pub fn scan_x(phase: f64, track_w: f32, pill_w: f32) -> f32 {
    let travel = (track_w - pill_w - 2.0 * PILL_INSET_PX).max(0.0);
    PILL_INSET_PX + travel * f32v(scan_progress(phase))
}

/// pill 静止位(纯函数,A8 降级的"静态弧线"落点):行程起点。
#[must_use]
pub fn spinner_rest_x(track_w: f32, pill_w: f32) -> f32 {
    scan_x(0.0, track_w, pill_w)
}

/// 不确定态 pill 位置总入口(纯函数):正常 = 1.2s 循环扫描;`reduced` =
/// 恒在静止位(任意时刻同值,宿主不请求帧)。
#[must_use]
pub fn spinner_pill_x(now_ms: f64, reduced: bool, track_w: f32, pill_w: f32) -> f32 {
    if reduced {
        return spinner_rest_x(track_w, pill_w);
    }
    scan_x(cycle_phase(now_ms, SPIN_PERIOD_MS), track_w, pill_w)
}

/// 确定态填充宽度(纯函数,ANI-01 #13 的"线性直读"):进度值钳制 [0,1]
/// 后线性映射到轨道宽。NaN(非法输入)按 0 处理(缺进度 = 空条,不 panic)。
#[must_use]
pub fn progress_fill(value: f64, track_w: f32) -> f32 {
    let v = if value.is_finite() {
        f32v(value.clamp(0.0, 1.0))
    } else {
        0.0
    };
    track_w * v
}

/// 是否完成(纯函数):钳制后的值到达 1.0。NaN 恒假。
#[must_use]
pub fn is_complete(value: f64) -> bool {
    value.is_finite() && value >= 1.0
}

/// 完成迁移意图([`completion_transition`] 的输出,Progress 的 pop 驱动)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionIntent {
    /// 刚跨入完成:起一次对勾 pop(120ms)
    Complete,
    /// 从完成回退(重试/重置):清除 pop,回到进行中
    Resume,
    /// 同侧滑动(进行中→进行中 / 完成→完成):pop 态不变
    Steady,
}

/// 完成迁移裁决(纯函数,状态机单点):只认**跨越**——prev 未完成、next
/// 完成才 Complete;反向才 Resume;其余 Steady(重复 set 1.0 不重放 pop)。
#[must_use]
pub fn completion_transition(prev: f64, next: f64) -> CompletionIntent {
    match (is_complete(prev), is_complete(next)) {
        (false, true) => CompletionIntent::Complete,
        (true, false) => CompletionIntent::Resume,
        _ => CompletionIntent::Steady,
    }
}

/// 对勾 pop 缩放(纯函数):0 → [`CHECK_START_SCALE`],120ms
/// ([`MotionTokens::DUR_STATE_MS`])OutCubic 长到 1.0,此后恒 1.0;
/// `reduced` 直通 1.0(免 pop,对勾全尺寸直现)。
#[must_use]
pub fn check_pop_scale(elapsed_ms: f64, reduced: bool) -> f32 {
    if reduced {
        return 1.0;
    }
    let elapsed = elapsed_ms.max(0.0);
    if elapsed >= POP_STATE_MS {
        return 1.0;
    }
    let u = elapsed / POP_STATE_MS;
    CHECK_START_SCALE + (1.0 - CHECK_START_SCALE) * f32v(Easing::OutCubic.apply(u))
}

// ---------------------------------------------------------------------------
// Spinner(不确定态;RenderOnce——相位是 now 的纯函数,无跨帧状态)
// ---------------------------------------------------------------------------

/// 不确定态加载件(§5.6 #14:pill 1.2s 循环扫描)。
///
/// ```ignore
/// // 正在导出(宿主行内,逐帧重建安全——RenderOnce 无跨帧状态):
/// spinner()
/// ```
///
/// 相位由 [`crate::interact::now_ms`] 在渲染期读取(时间注入边界的惯例
/// 位置),进行中**每帧续帧**;`reduced_motion` 下 pill 停在静止位、零帧
/// 提交(A8)。语义槽缺省 role = Group(宿主可用 `.label(...)`/`.role(...)`
/// 覆写为更精确的语义)。
#[derive(gpui::IntoElement)]
pub struct Spinner {
    track_w: f32,
    /// A11Y-02 语义槽
    semantic: Semantic,
}

impl Default for Spinner {
    fn default() -> Self {
        Spinner::new()
    }
}

impl Spinner {
    /// 默认宽度的不确定态加载件。
    pub fn new() -> Self {
        Spinner {
            track_w: SPINNER_TRACK_W_PX,
            semantic: Semantic::new(),
        }
    }

    /// 轨道宽度覆盖(px;下限 = pill + 两端内衬,行程为 0 时退化为静态位)。
    #[must_use]
    pub fn width(mut self, track_w: f32) -> Self {
        self.track_w = track_w.max(SPINNER_PILL_W_PX + 2.0 * PILL_INSET_PX);
        self
    }

    /// 解析语义(A11Y-02):显式 `.label(...)` 优先;role 默认 Group。
    #[must_use]
    pub fn resolved_semantic(&self) -> Semantic {
        let mut sem = Semantic::new();
        if let Some(label) = self.semantic.label() {
            sem = sem.with_label(label.clone());
        }
        sem.with_role(self.semantic.role().unwrap_or(SemanticRole::Group))
    }
}

// A11Y-02 语义槽(label/role/semantic 三件)
semantic_slot!(Spinner);

impl RenderOnce for Spinner {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = theme(cx).colors;
        let reduced = reduced_motion();
        let now = interact::now_ms();
        let x = spinner_pill_x(now, reduced, self.track_w, SPINNER_PILL_W_PX);
        if !reduced {
            // 循环动画恒续帧;reduced 静态零帧提交(A8)
            window.request_animation_frame();
        }
        let root =
            spinner_track(self.track_w, colors.surface_2).child(spinner_pill(x, colors.accent));
        interact::attach_semantics(root, &self.resolved_semantic())
    }
}

/// 轨道 quad(共用装配;[`Progress`] 同一条轨道形态)。
fn spinner_track(track_w: f32, bg: gpui::Hsla) -> Div {
    div()
        .relative()
        .w(px(track_w))
        .h(px(SPINNER_THICKNESS_PX))
        .rounded_full()
        .bg(bg)
}

/// 扫描 pill quad(绝对定位,左缘由纯函数给出)。
fn spinner_pill(x: f32, bg: gpui::Hsla) -> Div {
    div()
        .absolute()
        .top_0()
        .left(px(x))
        .w(px(SPINNER_PILL_W_PX))
        .h(px(SPINNER_THICKNESS_PX))
        .rounded_full()
        .bg(bg)
}

/// 便捷构造:`spinner()`(不确定态,默认宽度)。
#[must_use]
pub fn spinner() -> gpui::AnyElement {
    Spinner::new().into_any_element()
}

// ---------------------------------------------------------------------------
// Progress(确定态;Entity——值迁移驱动对勾 pop,有跨帧状态)
// ---------------------------------------------------------------------------

/// 确定态进度条(§5.6 #14:线性进度 + 完成对勾 pop 120ms;ANI-01 #13 的
/// 组件层落点)。
///
/// ```ignore
/// // 宿主驻留(Entity 形态;导出循环里 set_value 推进):
/// let p = cx.new(|_| Progress::new(0.0));
/// // 导出回调中:
/// p.update(cx, |p, cx| p.set_value(done / total, cx));
/// ```
///
/// 值钳制 [0,1]([`progress_fill`]);跨入 1.0 时起一次对勾 pop(120ms,
/// [`completion_transition`] 只认跨越,重复 set 1.0 不重放);`reduced_
/// motion` 下填充直读、对勾免 pop 直现。
pub struct Progress {
    /// 进度值(钳制 [0,1] 后存储)
    value: f64,
    track_w: f32,
    /// 对勾 pop 起点(`None` = 未完成/已沉降)
    pop_started_ms: Option<f64>,
    /// A11Y-02 语义槽
    semantic: Semantic,
}

impl Progress {
    /// 初值 `value` 的进度条(内部即钳制;构造 1.0 不闪 pop——pop 只认
    /// **运行中的跨越**,静止构造直落完成态)。
    pub fn new(value: f64) -> Self {
        Progress {
            value: clamp01(value),
            track_w: PROGRESS_TRACK_W_PX,
            pop_started_ms: None,
            semantic: Semantic::new(),
        }
    }

    /// 轨道宽度覆盖(px)。
    #[must_use]
    pub fn width(mut self, track_w: f32) -> Self {
        self.track_w = track_w.max(SPINNER_THICKNESS_PX);
        self
    }

    /// 当前进度值(钳制后)。
    #[must_use]
    pub fn value(&self) -> f64 {
        self.value
    }

    /// 是否处于完成态(对勾可见)。
    #[must_use]
    pub fn is_done(&self) -> bool {
        is_complete(self.value)
    }

    /// 推进进度(宿主唯一写入口):钳制 [0,1];跨越完成 → 起对勾 pop,
    /// 回退 → 清 pop;同侧滑动不动 pop 态。幂等通知(值不变不触发)。
    pub fn set_value(&mut self, value: f64, cx: &mut Context<Self>) {
        let next = clamp01(value);
        if next == self.value {
            return;
        }
        match completion_transition(self.value, next) {
            CompletionIntent::Complete => {
                self.pop_started_ms = Some(interact::now_ms());
            }
            CompletionIntent::Resume => {
                self.pop_started_ms = None;
            }
            CompletionIntent::Steady => {}
        }
        self.value = next;
        cx.notify();
    }

    /// 当前对勾缩放(渲染期求值;宿主调试可读)。
    #[must_use]
    pub fn pop_scale(&self, now_ms: f64, reduced: bool) -> f32 {
        match self.pop_started_ms {
            Some(started) => check_pop_scale(now_ms - started, reduced),
            None => {
                if self.is_done() {
                    1.0 // 静止完成态:对勾全尺寸直现
                } else {
                    0.0
                }
            }
        }
    }

    /// 解析语义(A11Y-02):显式 `.label(...)` 优先;role 默认 Group
    /// (读屏进度语义待 TD-01 语义树,见 interact 模块 doc 的如实边界)。
    #[must_use]
    pub fn resolved_semantic(&self) -> Semantic {
        let mut sem = Semantic::new();
        if let Some(label) = self.semantic.label() {
            sem = sem.with_label(label.clone());
        }
        sem.with_role(self.semantic.role().unwrap_or(SemanticRole::Group))
    }
}

// A11Y-02 语义槽(label/role/semantic 三件)
semantic_slot!(Progress);

/// f64 进度值钳制(NaN → 0;有限值钳 [0,1])。
fn clamp01(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

impl Render for Progress {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let now = interact::now_ms();
        let reduced = reduced_motion();
        let fill_w = progress_fill(self.value, self.track_w);
        let done = self.is_done();
        let pop = self.pop_scale(now, reduced);
        let popping = self
            .pop_started_ms
            .is_some_and(|s| (now - s).max(0.0) < POP_STATE_MS);

        let mut root = spinner_track(self.track_w, colors.surface_2).child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .w(px(fill_w))
                .h(px(SPINNER_THICKNESS_PX))
                .rounded_full()
                .bg(colors.accent),
        );
        if done && pop > 0.0 {
            // 完成对勾(文字字形占位,§5.5):pop 缩放落在字号上(字形居中
            // 于条尾,success 令牌取色;对 accent 条的辨识度由主题门禁保证)
            root = root.child(
                h_flex()
                    .absolute()
                    .left(px(fill_w + SpacingTokens::XS))
                    .top_0()
                    .bottom_0()
                    .child(
                        div()
                            .text_size(px(TextSize::LABEL.size * pop))
                            .font_weight(FontWeight(TextSize::LABEL.weight))
                            .text_color(colors.success)
                            .child(CHECK_GLYPH),
                    ),
            );
        }
        // pop 进行中续帧;其余(循环-free 的确定态)静止零帧提交
        if popping && !reduced {
            window.request_animation_frame();
        }
        interact::attach_semantics(root, &self.resolved_semantic())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 相位 0 对齐的测试纪元(1200 的整数倍:循环纪元本任意,取对齐值使
    /// 断言可读)。
    const T0: f64 = 12_000.0;

    /// TC-CMP-STATE-02(相位):1.2s 周期的折返与回卷;非有限/非法周期防御。
    #[test]
    fn tc_cmp_state_02_cycle_phase_wraps_and_defends() {
        assert_eq!(cycle_phase(T0, SPIN_PERIOD_MS), 0.0, "起点相位 0");
        let half = cycle_phase(T0 + 600.0, SPIN_PERIOD_MS);
        assert!((half - 0.5).abs() < 1e-9, "半程相位 0.5:{half}");
        let full = cycle_phase(T0 + SPIN_PERIOD_MS, SPIN_PERIOD_MS);
        assert!(full.abs() < 1e-9, "整周期回卷到 0:{full}");
        // 2.5 周期 = 相位 0.5(回卷确定性,长任务不漂移)
        let wrap = cycle_phase(T0 + 2.5 * SPIN_PERIOD_MS, SPIN_PERIOD_MS);
        assert!((wrap - 0.5).abs() < 1e-9);
        // 回卷语义:负时刻经 rem_euclid 自然回卷(循环对纪元不敏感,时钟
        // 回拨无损绕回),与正向同相位点一致
        assert!(
            (cycle_phase(-100.0, SPIN_PERIOD_MS) - cycle_phase(1100.0, SPIN_PERIOD_MS)).abs()
                < 1e-9,
            "负时刻回卷到周期尾部"
        );
        // 防御:非有限输入与非法周期不 panic、落 0
        assert_eq!(cycle_phase(f64::NAN, SPIN_PERIOD_MS), 0.0);
        assert_eq!(cycle_phase(f64::INFINITY, SPIN_PERIOD_MS), 0.0);
        assert_eq!(cycle_phase(0.0, 0.0), 0.0);
        assert_eq!(cycle_phase(0.0, -1.0), 0.0);
        assert_eq!(cycle_phase(0.0, f64::NAN), 0.0);
        assert_eq!(SPIN_PERIOD_MS, 1200.0, "报告 §5.6 #14:1.2s 周期单点");
    }

    /// TC-CMP-STATE-02(几何/相位):扫描往返曲线两端精确、单程单调、
    /// 对称;像素几何端点与退化轨道防御。
    #[test]
    fn tc_cmp_state_02_scan_geometry_endpoints_monotonic_symmetric() {
        // 三角波相位:0 → 0(起位)、0.25 → 半程、0.5 → 终点、0.75 → 半程
        assert!((scan_progress(0.0)).abs() < 1e-9);
        assert!((scan_progress(0.5) - 1.0).abs() < 1e-9, "相位 0.5 抵达右端");
        assert!(
            (scan_progress(0.25) - 0.5).abs() < 1e-9,
            "半程InOutCubic 中点恰半"
        );
        assert!((scan_progress(0.75) - 0.5).abs() < 1e-9, "折返对称");
        // 单程单调(前半程 0→0.5 相位)
        let mut prev = -1.0;
        for i in 0..=10 {
            let p = scan_progress(f64::from(i) / 20.0);
            assert!(p > prev, "前半程单调上行:{p} ≤ {prev}");
            prev = p;
        }
        // 像素端点:inset / track - pill - inset
        let end = SPINNER_TRACK_W_PX - SPINNER_PILL_W_PX - 2.0 * PILL_INSET_PX;
        assert_eq!(
            scan_x(0.0, SPINNER_TRACK_W_PX, SPINNER_PILL_W_PX),
            PILL_INSET_PX
        );
        assert_eq!(
            scan_x(0.5, SPINNER_TRACK_W_PX, SPINNER_PILL_W_PX),
            PILL_INSET_PX + end,
            "右端精确落位"
        );
        // 退化:轨道容不下 pill → 行程 0,pill 恒在 inset(不 panic)
        assert_eq!(
            scan_x(0.5, SPINNER_PILL_W_PX, SPINNER_PILL_W_PX),
            PILL_INSET_PX
        );
        // 静止位 = 起位(A8 的"静态弧线")
        assert_eq!(
            spinner_rest_x(SPINNER_TRACK_W_PX, SPINNER_PILL_W_PX),
            PILL_INSET_PX
        );
    }

    /// TC-CMP-STATE-02(reduced 直通):不确定态任意时刻恒在静止位;确定态
    /// 对勾免 pop 直现全尺寸。
    #[test]
    fn tc_cmp_state_02_reduced_motion_static_arc_and_direct_value() {
        // 不确定态:reduced 下任意时刻同值(静态弧线)
        let rest = spinner_pill_x(T0, true, SPINNER_TRACK_W_PX, SPINNER_PILL_W_PX);
        for dt in [0.0, 1.0, 300.0, 600.0, 1199.0] {
            assert_eq!(
                spinner_pill_x(T0 + dt, true, SPINNER_TRACK_W_PX, SPINNER_PILL_W_PX),
                rest,
                "reduced:相位无关"
            );
            assert_eq!(rest, PILL_INSET_PX, "静态弧线 = 起位");
        }
        // 正常路径:同两时刻位置可辨(确实在动)
        let a = spinner_pill_x(T0, false, SPINNER_TRACK_W_PX, SPINNER_PILL_W_PX);
        let b = spinner_pill_x(T0 + 600.0, false, SPINNER_TRACK_W_PX, SPINNER_PILL_W_PX);
        assert!(b > a, "0→600ms 扫描前进");
        // 对勾:reduced 直通 1.0
        assert_eq!(check_pop_scale(1.0, true), 1.0);
    }

    /// TC-CMP-STATE-02(确定态几何,ANI-01 #13 线性直读):值→宽度线性、
    /// 越界钳制、单调确定(同值同宽,零动画队列)。
    #[test]
    fn tc_cmp_state_02_progress_fill_linear_clamped_ani_01_13() {
        assert_eq!(progress_fill(0.0, PROGRESS_TRACK_W_PX), 0.0);
        assert_eq!(
            progress_fill(0.5, PROGRESS_TRACK_W_PX),
            PROGRESS_TRACK_W_PX / 2.0,
            "线性直读"
        );
        assert_eq!(progress_fill(1.0, PROGRESS_TRACK_W_PX), PROGRESS_TRACK_W_PX);
        assert_eq!(progress_fill(-0.7, PROGRESS_TRACK_W_PX), 0.0, "负值钳 0");
        assert_eq!(
            progress_fill(2.0, PROGRESS_TRACK_W_PX),
            PROGRESS_TRACK_W_PX,
            "越界钳 1"
        );
        assert_eq!(
            progress_fill(f64::NAN, PROGRESS_TRACK_W_PX),
            0.0,
            "NaN 防御"
        );
        // 单调 + 确定(同值同宽:进度动画经本组件落地 = 值即真相)
        let mut prev = -1.0;
        for i in 0..=20 {
            let w = progress_fill(f64::from(i) / 20.0, PROGRESS_TRACK_W_PX);
            assert!(w >= prev, "填充随值单调:{w} < {prev}");
            assert_eq!(w, progress_fill(f64::from(i) / 20.0, PROGRESS_TRACK_W_PX));
            prev = w;
        }
        assert!(is_complete(1.0));
        assert!(!is_complete(0.999));
        assert!(!is_complete(f64::NAN));
    }

    /// TC-CMP-STATE-02(完成状态机 + 对勾 pop 120ms):只认跨越;pop 曲线
    /// 起步 0.5、120ms 长满 1.0。
    #[test]
    fn tc_cmp_state_02_completion_transition_and_check_pop() {
        // 迁移真值表:跨越才起/清 pop,重复 set 1.0 不重放
        assert_eq!(completion_transition(0.9, 1.0), CompletionIntent::Complete);
        assert_eq!(completion_transition(1.0, 0.4), CompletionIntent::Resume);
        assert_eq!(completion_transition(1.0, 1.0), CompletionIntent::Steady);
        assert_eq!(completion_transition(0.5, 0.6), CompletionIntent::Steady);
        // NaN 永不完成:与未完成侧同侧 → 无跨越(Pop 态不动;NaN 写入在
        // Progress::set_value 里先行钳 0,不会泄漏进本裁决)
        assert_eq!(
            completion_transition(0.5, f64::NAN),
            CompletionIntent::Steady,
            "NaN 不构成跨越"
        );
        assert_eq!(
            completion_transition(f64::NAN, 1.0),
            CompletionIntent::Complete,
            "NaN 起点 = 未完成,跨入完成照常起 pop"
        );
        // pop 曲线:0 → 0.5,半程 OutCubic(0.5)=0.875,120ms 长满 1.0
        assert!((check_pop_scale(0.0, false) - CHECK_START_SCALE).abs() < 1e-6);
        let mid = check_pop_scale(60.0, false);
        let expect_mid = CHECK_START_SCALE + (1.0 - CHECK_START_SCALE) * 0.875;
        assert!(
            (mid - expect_mid).abs() < 1e-6,
            "60ms = OutCubic 半程:{mid} vs {expect_mid}"
        );
        assert_eq!(check_pop_scale(POP_STATE_MS, false), 1.0, "120ms 长满");
        assert_eq!(check_pop_scale(10_000.0, false), 1.0, "超时钳满");
        assert_eq!(
            check_pop_scale(-5.0, false),
            CHECK_START_SCALE,
            "负时刻按 0"
        );
        // pop 时长 = STATE 档 120ms(报告 §5.6 #14"完成对勾 pop 120ms")
        assert_eq!(POP_STATE_MS, MotionTokens::DUR_STATE_MS);
        assert_eq!(POP_STATE_MS, 120.0);
    }

    /// 组件装配(builder/语义槽/宽度防御;按钮族同款字段断言风格):
    /// Spinner RenderOnce 默认宽度;Progress 钳制构造、set_value 迁移驱动
    /// pop 态、语义槽缺省回落。
    #[test]
    fn spinner_progress_builders_clamp_and_semantic_fallback() {
        // Spinner:默认宽度 + 下限防御
        let sp = Spinner::new();
        assert_eq!(sp.track_w, SPINNER_TRACK_W_PX);
        assert_eq!(
            Spinner::new().width(8.0).track_w,
            SPINNER_PILL_W_PX + 2.0 * PILL_INSET_PX,
            "下限钳制"
        );
        assert_eq!(sp.resolved_semantic().role(), Some(SemanticRole::Group));
        let named = sp.label("正在导出").role(SemanticRole::Decoration);
        assert_eq!(
            named.resolved_semantic().label().map(|s| s.as_ref()),
            Some("正在导出")
        );
        assert_eq!(
            named.resolved_semantic().role(),
            Some(SemanticRole::Decoration),
            "显式 role 优先"
        );
        // Progress:构造即钳制;1.0 构造不闪 pop(静止完成态直落)
        let p = Progress::new(2.0);
        assert_eq!(p.value(), 1.0);
        assert!(p.is_done());
        assert_eq!(p.pop_scale(0.0, false), 1.0, "静止完成对勾全尺寸");
        // 运行中推进:跨入 1.0 起 pop,回退清 pop
        let mut run = Progress::new(0.0);
        assert_eq!(run.pop_started_ms, None);
        run.value = 0.9; // 直接布防状态(set_value 需要 Context,纯迁移已单测)
        let intent = completion_transition(run.value, 1.0);
        assert_eq!(intent, CompletionIntent::Complete, "跨入完成 → 起对勾 pop");
        assert_eq!(completion_transition(1.0, 0.2), CompletionIntent::Resume);
        // Progress 语义槽缺省回落 Group
        assert_eq!(run.resolved_semantic().role(), Some(SemanticRole::Group));
        let labelled = run.label("导出进度");
        assert_eq!(
            labelled.resolved_semantic().label().map(|s| s.as_ref()),
            Some("导出进度")
        );
    }
}
