//! A7 微交互接线(08 迭代计划 A7;分册六 §4.3 动画清单 14 项的组件层落点)。
//!
//! # 可复用件(本模块,组件按需取用)
//!
//! - 状态层(TOK-04,报告 §5.3.3):[`InteractState`] + [`state_layer`]——
//!   hover/press 用白/黑 alpha 叠加(极性随底色亮度自动取向),selected =
//!   accent 同比例 alpha(深浅一致),disabled 容器不变;修复旧亮度法在
//!   浅色底钳到 1.0 失效的问题;
//! - 三态交互时长/时长档重指向:[`DUR_INTERACT_MS`] 系列常量(120/200ms
//!   已重指向 [`crate::tokens::MotionTokens`] 动效四档,消除两处真相);
//!   [`hover_tint`] / [`pressed_tint`] 保留为**兼容别名**,内部改走
//!   [`state_layer`];
//! - [`HoverState`]:悬停进度状态机(120ms ease-out,0..1,支持中途打断从
//!   当前值接续),供**有状态组件**(Entity)每帧读进度做 bg 插值;
//! - [`PulseState`]:撤销/重做视觉脉冲(300ms accent 描边闪一次,#7),
//!   LayerPanel 行已接([`crate::layer_panel::LayerPanel::pulse_for`]);
//! - [`now_ms`]:事件回调边界的毫秒时钟(事件不携带时间戳,引擎模块"零
//!   `Instant::now()`"纪律不变,时钟只存在于组件接线边界)。
//!
//! # 减弱动态(A8)
//!
//! [`HoverState::progress_at`] / [`HoverState::is_running`] /
//! [`PulseState::progress_at`] 一律 [`reduced_motion`] 短路:进度直通 0/1、
//! 脉冲整体省略、运行态恒假(UI 停止请求帧)。
//!
//! # 分册六 §4.3 清单 14 项逐项落点(08-A7 验收表)
//!
//! 组件层(本文件集)已承接的项在"接线"列给出具体 API;不属于组件层的
//! 项标注**归属层 + 接线点**(代码不动,后续迭代在此接线):
//!
//! | # | 清单项 | 参数 | 接线 / 归属层 |
//! |---|---|---|---|
//! | 1 | 控件 hover/press | 120ms ease-out,state-layer alpha 叠加 | ✅ 组件层:本模块
//!   [`state_layer`](TOK-04,深色叠白/浅色叠黑,selected = accent 14%)+
//!   [`HoverState`](NumberField 动画插值)、PropertyRow/LayerPanel/LayerTree/
//!   EffectStack 行(gpui hover 样式即时三态) |
//! | 2 | Dock 面板拖拽重排 | Spring::SNAPPY 归位 | 归属 **sable-dock**:gpui-component
//!   DockArea 拖放结束回调 → [`crate::anim::Spring::SNAPPY`](anim 引擎已备),
//!   面板重排接线 = TODO(上游 Dock 无动画 seam,需 M2 评估) |
//! | 3 | 面板折叠/展开 | 200ms ease-in-out 高度插值 | 组件层 token 已备
//!   ([`DUR_PANEL_MS`]);LayerPanel 分组折叠本身 = M2(分册四 §1 v0.1 边界),
//!   折叠头实现时直接取该 token + `Animated<Length>` |
//! | 4 | 对话框进出场 | 160ms scale .96→1 + fade | 归属 **应用层/dock 浮层系统**:
//!   浮层 host 出场动画接线 = TODO(当前无对话框组件) |
//! | 5 | Toast 滑入滑出 | 240ms ease-out + 自动退场 | 归属 **应用层通知系统**:
//!   [`DUR_OVERLAY_MS`] 已备;无 Toast 组件 = TODO |
//! | 6 | 工具切换指示 | 160ms 选中 pill 滑动 | 归属 **应用层工具栏**(不在本波
//!   文件集):pill 位移用 `Animated<Length>` + 160ms 接线 = TODO |
//! | 7 | 撤销/重做视觉脉冲 | 300ms accent 描边闪一次 | ✅ 组件层:[`PulseState`] →
//!   `LayerPanel::pulse_for`;画布侧复用点 = **sable-canvas** 绘制描边时查询
//!   同款状态机(接线 = TODO) |
//! | 8 | 画布缩放跟手 | 直接操作无动画 | 归属 **sable-canvas**(跟手优先,设计
//!   如此,无动画可接) |
//! | 9 | "缩放到适应"视图跳转 | 280ms ease-in-out Viewport 插值 | 归属 **应用层
//!   zoom-to-fit 命令 → sable-foundation Viewport**:[`DUR_VIEW_JUMP_MS`] +
//!   `Animated<Viewport>` 接线 = TODO |
//! | 10 | 时间轴播放头吸附 | 80ms 微弹 | 归属 **timeline_view**(拖动释放后微弹):
//!   v0.2 未接(`set_playhead` 直通显示),Spring 微弹接线 = TODO-M2 |
//! | 11 | clip 拖拽投影 | 实时跟随 + 落点 Spring | 归属 **timeline_view**:拖拽
//!   1:1 跟随已实现;落点投影/回弹 = TODO-M2(分册四 §8 性能红线:拖动不重解码) |
//! | 12 | 图层拖拽排序让位 | 160ms | ✅ 组件层:[`crate::flip::FlipTracker`] →
//!   LayerPanel 行 `.mt()` 偏移(A4) |
//! | 13 | 进度条/导出 | 线性 + 不确定态脉冲 | 归属 **应用层/未来 Progress 组件**:
//!   线性直读 + [`DUR_PULSE_MS`] 不确定态 = TODO |
//! | 14 | 主题切换 | 200ms 全 token 插值 | 归属 **theme/应用层**:`lerp_hsla`
//!   (A6)已备,`theme::set_mode` 切换时逐 token 200ms 插值接线 = TODO |
//!
//! # 时间单位契约
//!
//! 本模块全部 `*_at(now_ms)` 收 `f64` 毫秒(调用方时钟,通常取 [`now_ms`]),
//! 与 anim 引擎纯逻辑模块同契约(Animated 的 `Instant` 是历史例外)。

use std::sync::OnceLock;
use std::time::Instant;

use gpui::Hsla;

use crate::anim::{Easing, reduced_motion};

/// 三态交互时长(hover/press,分册六 §4.3 #1):= 动效四档的 STATE 档
/// ([`crate::tokens::MotionTokens::DUR_STATE_MS`],ANI-07 单点;`theme`
/// feature 关闭的降级编译才落到字面量)。
#[cfg(feature = "theme")]
pub const DUR_INTERACT_MS: f64 = crate::tokens::MotionTokens::DUR_STATE_MS;
/// 降级编译(theme feature 关闭,tokens 不在编译面)的字面量回退。
#[cfg(not(feature = "theme"))]
pub const DUR_INTERACT_MS: f64 = 120.0;
/// 面板折叠/展开时长(#3):= 动效四档的 PANEL 档(单点同上)。
#[cfg(feature = "theme")]
pub const DUR_PANEL_MS: f64 = crate::tokens::MotionTokens::DUR_PANEL_MS;
/// 降级编译(theme feature 关闭)的字面量回退。
#[cfg(not(feature = "theme"))]
pub const DUR_PANEL_MS: f64 = 200.0;
/// 浮层/Toast 时长(#4/#5):240ms。
pub const DUR_OVERLAY_MS: f64 = 240.0;
/// 脉冲时长(撤销/重做 #7、不确定进度):300ms。
pub const DUR_PULSE_MS: f64 = 300.0;
/// 视图跳转时长("缩放到适应" #9):280ms。
pub const DUR_VIEW_JUMP_MS: f64 = 280.0;

/// hover 亮度偏移(历史值 +4% L,分册六 §4.3 #1)。**仅存兼容**:三态表达
/// 已迁 state-layer alpha 叠加(TOK-04),本常量只供旧宿主对照,不再参与
/// 任何本仓逻辑。
pub const HOVER_LIGHTNESS_DELTA: f32 = 0.04;
/// pressed 亮度偏移(历史值 +8% L)。仅存兼容,同上。
pub const PRESSED_LIGHTNESS_DELTA: f32 = 0.08;

/// hover/leave 过渡缓动(分册六 §4.3 #1:ease-out)。
const HOVER_EASE: Easing = Easing::OutCubic;

// ---------------------------------------------------------------------------
// TOK-04:状态层 state-layer(报告 §5.3.3;alpha 叠加替代亮度偏移)
//
// 亮度法(旧 hover_tint 的 L+4%/+8%)在浅色高亮度底会钳到 1.0 而完全失效
// (旧代码注释自认)。state-layer 用 alpha 叠加:深色表面叠白、浅色表面叠
// 黑(极性按底色亮度自动取向),selected 统一 accent@14%(深浅同比例),
// disabled 容器不变(仅前景降级,TOK-07)。alpha 值单一源自
// [`crate::tokens::StateLayerTokens`](深/浅各一套)。
//
// 新 API 定义在 `theme` feature 下(依赖 tokens;anim 单开的降级编译面不
// 含状态层,`hover_tint`/`pressed_tint` 在该配置回退旧亮度法)。
// ---------------------------------------------------------------------------

/// 交互状态(state-layer 的裁决输入)。
#[cfg(feature = "theme")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InteractState {
    /// 静止:原样返回 base
    Idle,
    /// 悬停:中性叠加(深色表面叠白、浅色表面叠黑)
    Hover,
    /// 按下:更强的中性叠加
    Pressed,
    /// 选中:accent 同比例 alpha(深浅一致,TC-TOK-STATE-02)
    Selected,
    /// 焦点环:accent 实色(1.5px 外描边绘制接线 = A11Y 批次)
    FocusRing,
    /// 禁用:容器不变(底色原样,仅前景降级)
    Disabled,
}

/// 状态层裁决(纯函数,TOK-04):把 `state` 对应的叠加层混合到 `base` 上。
///
/// - 极性自动取向:`base.l >= 0.5` 视为浅色表面(叠黑),否则叠白——
///   浅色白底 hover 因此**可见变化**(TC-TOK-STATE-01,不再钳 1.0 死值);
/// - `accent` 仅 [`InteractState::Selected`]/[`InteractState::FocusRing`]
///   消费(其余状态可传任意占位色,推荐传 `colors.accent` 保持调用形一致);
/// - 混合在 HSL 域按 alpha 贡献比加权(h 走最短弧),对不透明底 =
///   `lerp(base, overlay, alpha)`;对透明底(图层行悬停垫片)= 纯叠加层
///   (色 = overlay,alpha = 状态 alpha),两种调用形一个公式。
#[cfg(feature = "theme")]
#[must_use]
pub fn state_layer(base: Hsla, state: InteractState, accent: Hsla) -> Hsla {
    let layers = if base.l >= 0.5 {
        crate::tokens::StateLayerTokens::light()
    } else {
        crate::tokens::StateLayerTokens::dark()
    };
    let light_surface = base.l >= 0.5;
    // 中性叠加:白(深色表面)/黑(浅色表面);h/s 取 0(消色差)
    let neutral = Hsla {
        h: 0.0,
        s: 0.0,
        l: if light_surface { 0.0 } else { 1.0 },
        a: 1.0,
    };
    let (overlay, alpha) = match state {
        InteractState::Idle | InteractState::Disabled => return base,
        InteractState::Hover => (neutral, layers.hover),
        InteractState::Pressed => (neutral, layers.press),
        InteractState::Selected => (accent, layers.selected),
        InteractState::FocusRing => (accent, layers.focus_ring),
    };
    overlay_over_base(base, overlay, alpha)
}

/// alpha 叠加混合(HSL 域):`overlay` 以 `alpha` 不透明度压在 `base` 上。
/// 结果 alpha = 标准 over;h/s/l 按叠加贡献占比 `t` 加权(h 最短弧)。
#[cfg(feature = "theme")]
fn overlay_over_base(base: Hsla, overlay: Hsla, alpha: f32) -> Hsla {
    let alpha = alpha.clamp(0.0, 1.0);
    let out_a = overlay.a * alpha + base.a * (1.0 - alpha);
    if out_a <= f32::EPSILON {
        return Hsla { a: 0.0, ..overlay };
    }
    let t = (overlay.a * alpha) / out_a;
    // 色相最短弧(与 anim::lerp_hsla 同语义,避免 0.9→0.1 绕远)
    let dh = overlay.h - base.h;
    let dh = dh - dh.round();
    Hsla {
        h: base.h + dh * t,
        s: base.s + (overlay.s - base.s) * t,
        l: base.l + (overlay.l - base.l) * t,
        a: out_a,
    }
}

/// hover 态底色(**兼容别名**):内部 = [`state_layer`]`(base, Hover)`——
/// 深色表面 +白 6%、浅色表面 -黑 4%(修复浅色底亮度法钳 1.0 失效)。
/// 新代码请直接用 [`state_layer`]。
#[cfg(feature = "theme")]
pub fn hover_tint(base: Hsla) -> Hsla {
    state_layer(base, InteractState::Hover, base)
}

/// pressed 态底色(**兼容别名**):内部 = [`state_layer`]`(base, Pressed)`。
#[cfg(feature = "theme")]
pub fn pressed_tint(base: Hsla) -> Hsla {
    state_layer(base, InteractState::Pressed, base)
}

/// 亮度偏移(仅 theme-less 降级编译的别名回退路径使用):对 Hsla 只调
/// L 分量并钳制 [0,1],h/s/a 原样保留。
#[cfg(not(feature = "theme"))]
fn tint(base: Hsla, delta: f32) -> Hsla {
    Hsla {
        l: (base.l + delta).clamp(0.0, 1.0),
        ..base
    }
}

/// hover 态底色(theme-less 降级编译回退:旧亮度法 +4%)。
#[cfg(not(feature = "theme"))]
pub fn hover_tint(base: Hsla) -> Hsla {
    tint(base, HOVER_LIGHTNESS_DELTA)
}

/// pressed 态底色(theme-less 降级编译回退:旧亮度法 +8%)。
#[cfg(not(feature = "theme"))]
pub fn pressed_tint(base: Hsla) -> Hsla {
    tint(base, PRESSED_LIGHTNESS_DELTA)
}

/// 组件事件边界的毫秒时钟(进程纪元 = 首次调用时刻)。事件回调(gpui 的
/// on_hover 等)不携带时间戳,组件把它喂给 [`HoverState`] / [`PulseState`] /
/// [`crate::flip::FlipTracker`] 的 `now_ms` 参数;同一组件内时钟同源即可,
/// 只要求单调。
pub fn now_ms() -> f64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = EPOCH.get_or_init(Instant::now);
    epoch.elapsed().as_secs_f64() * 1000.0
}

/// 悬停状态机(A7):记录 hover 进度 0..1,供组件每帧读。
///
/// - `on_enter`/`on_leave` 打断进行中的过渡时**从当前进度接续**(视觉无跳变);
/// - [`Self::progress_at`] 到达终点后自动"沉降"(duration 归零,后续读取
///   直通目标值,`is_running` 恒假——UI 停止请求帧);
/// - A8:[`reduced_motion`] 为真时进度直通目标值(0/1)、运行态恒假。
///
/// 时间由调用方注入(`now_ms`,通常取 [`now_ms`])。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HoverState {
    from: f64,
    target: f64,
    started_ms: f64,
    duration_ms: f64,
}

impl HoverState {
    /// 静止且未悬停的新状态(进度恒 0)。
    pub fn new() -> Self {
        HoverState {
            from: 0.0,
            target: 0.0,
            started_ms: 0.0,
            duration_ms: 0.0,
        }
    }

    /// 指针进入:120ms ease-out 过渡到 1。
    pub fn on_enter(&mut self, now_ms: f64) {
        self.animate_to(1.0, now_ms);
    }

    /// 指针离开:120ms ease-out 过渡回 0。
    pub fn on_leave(&mut self, now_ms: f64) {
        self.animate_to(0.0, now_ms);
    }

    /// 悬停进度(0..1):过渡中按 ease-out 插值;到点/减弱动态直通目标值并
    /// 沉降。`now_ms` 早于起始时刻(时钟回拨)按 0 处理。
    pub fn progress_at(&mut self, now_ms: f64) -> f64 {
        if self.duration_ms <= 0.0 {
            return self.target;
        }
        let elapsed = (now_ms - self.started_ms).max(0.0); // NaN → 0(f64::max 语义)
        if reduced_motion() || elapsed >= self.duration_ms {
            self.settle();
            return self.target;
        }
        let u = elapsed / self.duration_ms;
        self.from + (self.target - self.from) * HOVER_EASE.apply(u)
    }

    /// 过渡是否仍在进行:为真时宿主应继续请求动画帧。
    pub fn is_running(&mut self, now_ms: f64) -> bool {
        if reduced_motion() {
            self.settle();
            return false;
        }
        self.duration_ms > 0.0 && (now_ms - self.started_ms).max(0.0) < self.duration_ms
    }

    /// 从当前进度发起向 `target` 的过渡(打断平滑接续,同 Animated 语义)。
    fn animate_to(&mut self, target: f64, now_ms: f64) {
        let current = self.progress_at(now_ms); // 顺带沉降已完成的过渡
        self.from = current;
        self.target = target;
        self.started_ms = now_ms;
        self.duration_ms = DUR_INTERACT_MS;
    }

    /// 沉降:终值即目标,停止计动画运行。
    fn settle(&mut self) {
        self.from = self.target;
        self.started_ms = 0.0;
        self.duration_ms = 0.0;
    }
}

/// 撤销/重做视觉脉冲(A7;分册六 §4.3 #7):300ms accent 描边闪一次。
///
/// 进度曲线 = 三角波加缓动:前半程 0→1(ease-out,描边亮起),后半程
/// 1→0(ease-in,回落);窗口外 0 并**自动复位**(查询即推进状态机,便于
/// 组件渲染时顺带清理)。A8:减弱动态下脉冲整体省略(恒 0——装饰性闪烁
/// 对无障碍是负担而非反馈)。
///
/// 典型挂法:面板持有 `Map<节点, PulseState>`,command 撤销后对受影响节点
/// `begin`,行渲染时 `progress_at` 查进度给描边着色(见 LayerPanel)。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PulseState {
    started_ms: Option<f64>,
}

impl PulseState {
    /// 未开始的脉冲(进度恒 0)。
    pub fn new() -> Self {
        PulseState { started_ms: None }
    }

    /// (重)启动脉冲:每次调用都从 0 重新闪一遍。
    pub fn begin(&mut self, now_ms: f64) {
        self.started_ms = Some(now_ms);
    }

    /// 脉冲进度 0→1→0:窗口外返回 0 并清除;减弱动态直接省略。
    pub fn progress_at(&mut self, now_ms: f64) -> f64 {
        let Some(started) = self.started_ms else {
            return 0.0;
        };
        if reduced_motion() {
            self.started_ms = None;
            return 0.0;
        }
        let elapsed = (now_ms - started).max(0.0);
        if elapsed >= DUR_PULSE_MS {
            self.started_ms = None; // 窗口外自动复位(查询即清理)
            return 0.0;
        }
        let half = DUR_PULSE_MS / 2.0;
        if elapsed <= half {
            Easing::OutCubic.apply(elapsed / half)
        } else {
            1.0 - Easing::InCubic.apply((elapsed - half) / half)
        }
    }

    /// 脉冲是否仍在窗口内:为真时宿主应继续请求动画帧。
    pub fn is_active(&mut self, now_ms: f64) -> bool {
        self.progress_at(now_ms) > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: f64 = 1000.0;

    #[test]
    fn duration_tokens_match_spec() {
        // 分册六 §4.3 参数表逐项对齐;120/200 两档已重指向动效四档(tokens
        // 单点,ANI-07;测试配置恒开 theme,故直接断言与 MotionTokens 一致)
        assert_eq!(DUR_INTERACT_MS, 120.0);
        assert_eq!(DUR_INTERACT_MS, crate::tokens::MotionTokens::DUR_STATE_MS);
        assert_eq!(DUR_PANEL_MS, 200.0);
        assert_eq!(DUR_PANEL_MS, crate::tokens::MotionTokens::DUR_PANEL_MS);
        assert_eq!(DUR_OVERLAY_MS, 240.0);
        assert_eq!(DUR_PULSE_MS, 300.0);
        assert_eq!(DUR_VIEW_JUMP_MS, 280.0);
        // 历史亮度偏移仅存兼容(值冻结,不参与本仓逻辑)
        assert_eq!(HOVER_LIGHTNESS_DELTA, 0.04);
        assert_eq!(PRESSED_LIGHTNESS_DELTA, 0.08);
    }

    /// TC-TOK-STATE-01(报告 §5.3.3/TOK-04):浅色白底 hover 可见变化,
    /// 不再是亮度法钳 1.0 的死值;深色底 hover 相应变亮。
    #[test]
    fn tc_tok_state_01_light_hover_changes_and_dark_brightens() {
        let white = Hsla {
            h: 0.0,
            s: 0.0,
            l: 1.0,
            a: 1.0,
        };
        let hover_white = state_layer(white, InteractState::Hover, white);
        assert!(hover_white.l < 1.0, "浅色白底 hover 必须可见变化(压暗)");
        assert!(
            (hover_white.l - (1.0 - 0.04)).abs() < 1e-6,
            "浅色 hover = 黑 4% 叠加"
        );
        assert_eq!(hover_white.a, 1.0, "不透明底叠后仍不透明");
        // 旧亮度法在此底会钳 1.0(state_layer 的极性取向按 l>=0.5)
        let pressed_white = state_layer(white, InteractState::Pressed, white);
        assert!(
            pressed_white.l < hover_white.l,
            "press 弱于 hover(压得更深)"
        );
        // 深色底:hover 变亮(白 6% 叠加)
        let dark_surface = crate::tokens::ColorTokens::dark().surface_2;
        let hover_dark = state_layer(dark_surface, InteractState::Hover, dark_surface);
        assert!(hover_dark.l > dark_surface.l, "深色表面 hover 应变亮");
        assert!((hover_dark.l - (dark_surface.l + (1.0 - dark_surface.l) * 0.06)).abs() < 1e-6);
    }

    /// TC-TOK-STATE-02:深/浅 selected 底均为 accent 同比例 alpha(14%),
    /// 混合结果 = lerp(base, accent, 0.14),两主题一致。
    #[test]
    fn tc_tok_state_02_selected_is_accent_at_same_ratio_both_themes() {
        let dark = crate::tokens::ColorTokens::dark();
        let light = crate::tokens::ColorTokens::light();
        assert_eq!(
            crate::tokens::StateLayerTokens::dark().selected,
            crate::tokens::StateLayerTokens::light().selected,
            "selected alpha 深浅同比例(令牌侧)"
        );
        for tokens in [dark, light] {
            let base = tokens.surface_1;
            let sel = state_layer(base, InteractState::Selected, tokens.accent);
            let t = crate::tokens::StateLayerTokens::dark().selected;
            assert!(
                (sel.l - (base.l + (tokens.accent.l - base.l) * t)).abs() < 1e-6,
                "selected = lerp(base, accent, {t})"
            );
            // 色相按最短弧混合(同实现语义;accent 色相 0.58 对灰底 h=0 会
            // 取 -0.417 弧,两主题一致)
            let dh = tokens.accent.h - base.h;
            let dh_shortest = dh - dh.round();
            assert!(
                (sel.h - (base.h + dh_shortest * t)).abs() < 1e-6,
                "色相最短弧同比例混合"
            );
            assert_eq!(sel.a, 1.0);
        }
        // 透明底(行垫片形态):结果 = accent@14% 的纯叠加层
        let transparent = Hsla::transparent_black();
        let sel_over_transparent = state_layer(transparent, InteractState::Selected, dark.accent);
        assert!((sel_over_transparent.a - 0.14).abs() < 1e-6);
        assert_eq!(sel_over_transparent.l, dark.accent.l);
    }

    /// 状态层其余语义(纯函数):disabled/idle 原样直通;极性按亮度自动
    /// 取向;色相走最短弧。
    #[test]
    fn state_layer_idle_disabled_passthrough_and_polarity() {
        let base = Hsla {
            h: 0.3,
            s: 0.5,
            l: 0.4,
            a: 1.0,
        };
        assert_eq!(state_layer(base, InteractState::Idle, base), base);
        assert_eq!(state_layer(base, InteractState::Disabled, base), base);
        // 深色底(l<0.5)叠白、浅色底(l>=0.5)叠黑
        let hover = state_layer(base, InteractState::Hover, base);
        assert!(hover.l > base.l && hover.l < 1.0);
        let light_base = Hsla { l: 0.9, ..base };
        let hover_light = state_layer(light_base, InteractState::Hover, light_base);
        assert!(hover_light.l < light_base.l, "浅色底 hover 应压暗(叠黑)");
        // 兼容别名与直调一致
        assert_eq!(hover_tint(base), hover);
        assert_eq!(
            pressed_tint(base),
            state_layer(base, InteractState::Pressed, base)
        );
        // 高亮度底不再钳死(旧亮度法在 l=0.98 时 hover 无变化)
        let near_white = Hsla {
            h: 0.0,
            s: 0.0,
            l: 0.98,
            a: 1.0,
        };
        assert!(
            hover_tint(near_white).l < 0.98,
            "浅底 hover 必须可见(修复点)"
        );
        // 色相最短弧:0.9 → 0.1 的叠加应走 +0.2 弧(经 1.0/0.0),不倒退 -0.8
        let magenta_base = Hsla {
            h: 0.9,
            s: 0.8,
            l: 0.3,
            a: 1.0,
        };
        let selected_magenta = state_layer(
            magenta_base,
            InteractState::Selected,
            Hsla {
                h: 0.1,
                ..magenta_base
            },
        );
        let t = crate::tokens::StateLayerTokens::dark().selected;
        let expected = magenta_base.h + 0.2 * t;
        assert!((selected_magenta.h - expected).abs() < 1e-6);
    }

    #[test]
    fn hover_progress_follows_120ms_outcubic() {
        let mut h = HoverState::new();
        assert_eq!(h.progress_at(T0), 0.0, "初始未悬停");
        h.on_enter(T0);
        assert_eq!(h.progress_at(T0), 0.0, "进入瞬间进度 0");
        assert!(
            (h.progress_at(T0 + 60.0) - 0.875).abs() < 1e-9,
            "半程 = OutCubic(0.5)"
        );
        assert_eq!(h.progress_at(T0 + 120.0), 1.0, "120ms 到位");
        assert!(!h.is_running(T0 + 120.0), "到点即停(沉降)");
        assert_eq!(h.progress_at(T0 + 10_000.0), 1.0, "超时钳在 1");
        // leave:从 1 回落,同样 ease-out
        h.on_leave(T0 + 120.0);
        assert_eq!(h.progress_at(T0 + 120.0), 1.0);
        assert!(
            (h.progress_at(T0 + 180.0) - 0.125).abs() < 1e-9,
            "回落半程 = 1 - OutCubic(0.5)"
        );
        assert_eq!(h.progress_at(T0 + 240.0), 0.0);
        assert!(!h.is_running(T0 + 240.0));
    }

    #[test]
    fn hover_interrupt_continues_from_current_progress() {
        let mut h = HoverState::new();
        h.on_enter(T0);
        // 半程(进度 0.875)打断离开:应从 0.875 回落而不是从 1
        h.on_leave(T0 + 60.0);
        assert_eq!(h.progress_at(T0 + 60.0), 0.875, "打断瞬间不跳变");
        let mid = h.progress_at(T0 + 90.0);
        assert!(
            mid > 0.875 * 0.125 && mid < 0.875,
            "从 0.875 向 0 推进 1/4:0.875→{mid}"
        );
    }

    #[test]
    fn hover_reduced_motion_short_circuits() {
        crate::anim::set_reduced_motion(true);
        let mut h = HoverState::new();
        h.on_enter(T0);
        assert_eq!(h.progress_at(T0 + 1.0), 1.0, "减弱动态:直通目标值");
        assert!(!h.is_running(T0 + 1.0), "减弱动态:不再运行");
        h.on_leave(T0 + 2.0);
        assert_eq!(h.progress_at(T0 + 3.0), 0.0, "直通 0");
        crate::anim::set_reduced_motion(false);
    }

    #[test]
    fn pulse_progress_is_a_300ms_triangle() {
        let mut p = PulseState::new();
        assert_eq!(p.progress_at(T0), 0.0, "未开始恒 0");
        p.begin(T0);
        assert_eq!(p.progress_at(T0), 0.0, "起亮瞬间 0");
        assert!(
            (p.progress_at(T0 + 75.0) - 0.875).abs() < 1e-9,
            "前半程 ease-out 亮起"
        );
        assert!(
            (p.progress_at(T0 + 150.0) - 1.0).abs() < 1e-9,
            "峰值在 150ms"
        );
        assert!(
            (p.progress_at(T0 + 225.0) - 0.875).abs() < 1e-9,
            "后半程 ease-in 回落"
        );
        assert_eq!(p.progress_at(T0 + 300.0), 0.0, "300ms 收尾");
        assert!(!p.is_active(T0 + 301.0));
        // 窗口外自动复位:再次查询仍 0(状态已被清除,不会"永动续帧")
        assert_eq!(p.progress_at(T0 + 302.0), 0.0);
        // 重启:从头再闪一遍
        p.begin(T0 + 400.0);
        assert!((p.progress_at(T0 + 475.0) - 0.875).abs() < 1e-9);
    }

    #[test]
    fn pulse_reduced_motion_is_skipped_entirely() {
        crate::anim::set_reduced_motion(true);
        let mut p = PulseState::new();
        p.begin(T0);
        assert_eq!(p.progress_at(T0 + 10.0), 0.0, "减弱动态:脉冲整体省略");
        assert!(!p.is_active(T0 + 10.0));
        crate::anim::set_reduced_motion(false);
    }

    #[test]
    fn clock_is_monotonic_milliseconds() {
        let a = now_ms();
        let b = now_ms();
        assert!(b >= a, "now_ms 单调不减");
        assert!(a.is_finite());
    }
}
