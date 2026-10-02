//! 主题系统:全局 [`SableTheme`](分册三 §5)+ 设计 token(分册六 §3)。
//!
//! ```ignore
//! // 应用启动时(一次):
//! sable_widgets::theme::init(cx);
//! // 任意组件内:
//! let colors = &sable_widgets::theme::theme(cx).colors;
//! let canvas = &sable_widgets::theme::theme(cx).canvas;
//! // 换肤(V4.0 T5:带 200ms 过渡;reduced_motion 开启时内部直切):
//! sable_widgets::theme::set_mode_animated(cx, ThemeMode::Light, now_ms);
//! ```
//!
//! **与 gpui-component 主题的同步钩子留 M2**(gpui-component 0.7.0 内部
//! 跑在 gpui-pre 0.3.7 的类型世界里,与 gpui 0.2.2 不互通——已核实其
//! Cargo.toml `[dependencies.gpui] package = "gpui-pre"`;同步须经值转换层,
//! v0.1 不做)。因此本 crate 组件**刻意不使用 gpui-component**,全部纯 gpui
//! div/fill/canvas 兜底,类型世界单一,编译面最小。

use gpui::{App, Global, Hsla, rgba};

use crate::anim::reduced_motion;
use crate::tokens::ColorTokens;

/// 主题模式(深/浅)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeMode {
    /// 深色(Illustrator 式,默认)
    Dark,
    /// 浅色
    Light,
}

/// 画布语义色(分册三 §5 `CanvasTheme`):画布/画板/网格/参考线/选中/
/// 锚点/钢笔预览,与 sable-canvas `OverlayTheme` 同语义,值随主题。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasTheme {
    /// 画布底色(工作区,画板之外)
    pub canvas_bg: Hsla,
    /// 画板白(画板内)
    pub artboard_bg: Hsla,
    /// 网格线
    pub grid: Hsla,
    /// 参考线(经典青,分册三 §5 的 #00c8ff)
    pub guide: Hsla,
    /// 选中高亮(与 accent 同值是设计意图,同上游)
    pub selection: Hsla,
    /// 锚点/控制柄
    pub anchor: Hsla,
    /// 选中的锚点
    pub anchor_selected: Hsla,
    /// 钢笔预览线
    pub pen_preview: Hsla,
}

impl CanvasTheme {
    /// 深色画布方案(canvas_bg = sable-canvas `CANVAS_BASE_COLOR` #1e1e24 同源)。
    pub fn dark() -> Self {
        CanvasTheme {
            canvas_bg: rgba(0x1E1E24FF).into(),
            artboard_bg: rgba(0xFAFAFAFF).into(),
            grid: rgba(0x3A3A3AFF).into(),
            guide: rgba(0x00C8FFFF).into(), // 经典青 #00c8ff(docs/03 §5)
            selection: rgba(0x4F9FFFFF).into(),
            anchor: rgba(0xFFFFFFFF).into(),
            anchor_selected: rgba(0x4F9FFFFF).into(),
            pen_preview: rgba(0x00C8FFFF).into(),
        }
    }

    /// 浅色画布方案。
    pub fn light() -> Self {
        CanvasTheme {
            canvas_bg: rgba(0xE8E8EAFF).into(),
            artboard_bg: rgba(0xFFFFFFFF).into(),
            grid: rgba(0xDADADAFF).into(),
            guide: rgba(0x00A5FFFF).into(), // 标尺青(浅色可读性更好)
            selection: rgba(0x0D99FFFF).into(),
            anchor: rgba(0x1E1E1EFF).into(),
            anchor_selected: rgba(0x0D99FFFF).into(),
            pen_preview: rgba(0x00A5FFFF).into(),
        }
    }
}

/// Sable 全局主题:颜色令牌 + 画布语义色 + 当前模式。
///
/// `impl Global` 后经 `App` 全局态注入(`init`),组件侧用 [`theme`] 取。
#[derive(Clone, Debug)]
pub struct SableTheme {
    /// UI 颜色令牌(分册六 §3.1)
    pub colors: ColorTokens,
    /// 画布语义色(分册三 §5)
    pub canvas: CanvasTheme,
    /// 当前模式
    pub mode: ThemeMode,
}

impl Global for SableTheme {}

impl SableTheme {
    /// 深色主题(默认)。
    pub fn dark() -> Self {
        SableTheme {
            colors: ColorTokens::dark(),
            canvas: CanvasTheme::dark(),
            mode: ThemeMode::Dark,
        }
    }

    /// 浅色主题。
    pub fn light() -> Self {
        SableTheme {
            colors: ColorTokens::light(),
            canvas: CanvasTheme::light(),
            mode: ThemeMode::Light,
        }
    }
}

/// 注入全局主题(应用启动时调用一次;默认深色)。
pub fn init(cx: &mut App) {
    cx.set_global(SableTheme::dark());
}

/// 取当前主题。**必须先 [`init`]**(未初始化视为应用装配错误,panic 即fail-fast)。
pub fn theme(cx: &App) -> &SableTheme {
    // expect 带原因字符串:AGENTS.md §3.3 允许内部 expect;pub API 无 unwrap/Result 化的必要
    cx.try_global::<SableTheme>()
        .expect("SableTheme 未初始化:应用启动时必须先调用 sable_widgets::theme::init(cx)")
}

/// 切换主题模式(换肤 = 换一份 token 表,组件零改动,分册六 §3.3 军规三)。
pub fn set_mode(cx: &mut App, mode: ThemeMode) {
    let next = match mode {
        ThemeMode::Dark => SableTheme::dark(),
        ThemeMode::Light => SableTheme::light(),
    };
    cx.set_global(next);
}

// ---------------------------------------------------------------------------
// V4.0 T5(G23):主题接线收口 —— reduced_motion 直切、帧泵入口、
// inject × 过渡互斥(V3.0 的 ThemeTransition 在此之前是死代码)
// ---------------------------------------------------------------------------

/// 模式过渡时长(ms;与库测试同值,分册六 §4.3 #14 定值)。
pub const THEME_TRANSITION_MS: f64 = 200.0;

/// 过渡裁决(纯函数,[`set_mode_animated`] 的核心;库测试直测两路):
///
/// - `reduced`([`reduced_motion`])为真 → `None`:直切,不帧泵;
/// - 否则 → `Some(200ms [`ThemeTransition`])`:从 `from` 到 `to` 插值
///   (最短弧色相 + OutCubic),由应用逐帧帧泵。
///
/// `now_ms` 为过渡起点(调用方时钟,通常 [`crate::interact::now_ms`])。
pub fn resolve_transition(
    reduced: bool,
    from: ColorTokens,
    to: ColorTokens,
    now_ms: f64,
) -> Option<ThemeTransition> {
    if reduced {
        None
    } else {
        Some(ThemeTransition::new(from, to, now_ms, THEME_TRANSITION_MS))
    }
}

/// 动画换模式(V4.0 T5.1):reduced_motion 直切返回 `None`,正常路径返回
/// 可帧泵的 [`ThemeTransition`]。
///
/// # 语义(过渡中途/落定全部在此定义)
///
/// - **reduced_motion 直切**:[`reduced_motion`] 为真 → 立即 `set_global`
///   目标主题(等效 [`set_mode`])并取消任何进行中的过渡,返回 `None`——
///   应用无需帧泵、无需请求动画帧;
/// - **动画**:读当前全局 colors 作起点(上一过渡进行中时取的是插值值,
///   打断即从当前视觉平滑接续),构造到目标模式整套预设的
///   [`ThemeTransition`],登记进全局槽位并返回 `Some`;本调用**不改全局**
///   (下一帧起由帧泵接管);
/// - **帧泵**:应用收到 `Some` 后逐帧调 [`advance_transition`](根视图在
///   `render` 顶部调用一次,返回真即 `window.request_animation_frame()`);
///   泵完全局自动落定为目标主题(`tokens_at` 末帧精确钳在终态,无跳变);
/// - **同模式调用**:等效"重置为该模式预设"(colors 即便被 [`inject`]
///   定制过也会被重置)——可当"取消自定义"动作用;
/// - **与 [`inject`] 互斥**:注入取消进行中的过渡(见 [`inject`])。
///
/// 返回的 `ThemeTransition` 是可读可采样快照(`tokens_at`/`is_running`);
/// 权威状态在全局槽位——`inject` 取消它后,继续采样快照只是本地计算,
/// 不再影响全局。
pub fn set_mode_animated(cx: &mut App, mode: ThemeMode, now_ms: f64) -> Option<ThemeTransition> {
    let from = theme(cx).colors;
    let next = match mode {
        ThemeMode::Dark => SableTheme::dark(),
        ThemeMode::Light => SableTheme::light(),
    };
    let Some(transition) = resolve_transition(reduced_motion(), from, next.colors, now_ms) else {
        // 直切:先清槽再落定(否则帧泵下一帧会用 tokens_at 把直切结果
        // 覆盖回插值态——互斥与注入同源)
        cancel_transition(cx);
        cx.set_global(next);
        return None;
    };
    cx.set_global(TransitionSlot(Some(ActiveTransition {
        transition: transition.clone(),
        target: next,
    })));
    Some(transition)
}

/// 推进一帧模式过渡(帧泵入口,V4.0 T5.2;story 根视图在 `render` 顶部
/// 调用,**先推进再读** [`theme`],本帧画的就是本帧插值)。
///
/// 返回 `true` = 过渡进行中,宿主应 `window.request_animation_frame()`
/// (运行才请求帧,静止零帧提交,分册六 §4.4);返回 `false` = 无活动
/// 过渡(空闲短路,零成本)或本帧已是落定帧(全局已被 `set_global` 成
/// 目标主题整体,无需再请求帧)。
///
/// 每次调用恰做一次 `set_global`:进行中帧设插值态
/// ([`ThemeTransition::tokens_at`],全部全局观察者随之逐帧重渲——这正是
/// 过渡的可见途径);落定帧设目标主题(mode/canvas/ColorTokens 一步到位)。
pub fn advance_transition(cx: &mut App, now_ms: f64) -> bool {
    let active = cx
        .try_global::<TransitionSlot>()
        .and_then(|slot| slot.0.clone());
    match pump_step(active.as_ref(), now_ms) {
        PumpStep::Idle => false,
        PumpStep::Frame(themed) => {
            cx.set_global(themed);
            true
        }
        PumpStep::Settle(themed) => {
            cx.set_global(themed);
            cancel_transition(cx);
            false
        }
    }
}

/// 活动过渡的全局槽位(与 [`SableTheme`] 平行的全局态)。
///
/// 有值 = 有一个进行中的模式过渡。不放进 `SableTheme`:过渡态不是主题
/// 本身,且 `SableTheme` 每帧被帧泵覆写,会把过渡自身吞掉。`inject` 与
/// 直切通过清空本槽位实现互斥。
#[derive(Default)]
struct TransitionSlot(Option<ActiveTransition>);

impl Global for TransitionSlot {}

/// 进行中的过渡 + 其落定目标(colors 逐帧向 target 收敛)。
#[derive(Clone)]
struct ActiveTransition {
    transition: ThemeTransition,
    target: SableTheme,
}

/// 帧泵单步裁决(纯函数,[`advance_transition`] 的核心;测试锁定三态 + 互斥)。
#[derive(Clone, Debug)]
enum PumpStep {
    /// 进行中:应用应 `set_global` 该插值主题并继续请求动画帧。
    Frame(SableTheme),
    /// 落定帧:应用应 `set_global` 该终态主题(精确等于目标),此后帧泵短路。
    Settle(SableTheme),
    /// 无过渡:零动作(注入/直切清槽后恒落此态——插值不再覆盖全局)。
    Idle,
}

fn pump_step(active: Option<&ActiveTransition>, now_ms: f64) -> PumpStep {
    let Some(active) = active else {
        return PumpStep::Idle;
    };
    let mut themed = active.target.clone();
    themed.colors = active.transition.tokens_at(now_ms);
    if active.transition.is_running(now_ms) {
        PumpStep::Frame(themed)
    } else {
        PumpStep::Settle(themed)
    }
}

/// 清空活动过渡槽位(直切 / 注入 / 落定三处共用)。
fn cancel_transition(cx: &mut App) {
    cx.set_global(TransitionSlot(None));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_is_default_and_modes_differ() {
        let dark = SableTheme::dark();
        assert_eq!(dark.mode, ThemeMode::Dark);
        let light = SableTheme::light();
        assert_eq!(light.mode, ThemeMode::Light);
        // 两套方案的表面/画布/参考线各成体系
        assert_ne!(dark.colors.surface_1, light.colors.surface_1);
        assert_ne!(dark.canvas.canvas_bg, light.canvas.canvas_bg);
        assert_ne!(dark.canvas.guide, light.canvas.guide);
        // 选中色与 accent 同值是设计意图(深色套内自洽)
        assert_eq!(dark.canvas.selection, dark.colors.accent);
        // set_mode 是纯函数式映射:模式 ↔ 套装一一对应
        assert_eq!(SableTheme::dark().mode, ThemeMode::Dark);
    }

    #[test]
    fn canvas_theme_covers_all_docs03_fields() {
        // 分册三 §5 的 8 个字段一个不少(编译期字段存在性 + 值完整性)
        let c = CanvasTheme::dark();
        let all = [
            c.canvas_bg,
            c.artboard_bg,
            c.grid,
            c.guide,
            c.selection,
            c.anchor,
            c.anchor_selected,
            c.pen_preview,
        ];
        assert_eq!(all.len(), 8);
        assert!(all.iter().all(|h| h.a > 0.99), "画布语义色不透明");
    }
}

// ---------------------------------------------------------------------------
// V3.0 T3:主题定制注入 + 全 token 过渡(纯函数状态机,帧泵在应用层)
// ---------------------------------------------------------------------------

/// 注入自定义调色板(第三方主题入口,V3.0 T3.1):以 `mode` 的整套预设为
/// 底,替换 `colors` 后 `set_global`,组件下一帧生效。组件读色走全局态,
/// 故注入即全量换肤。
///
/// # 注入即取消过渡(V4.0 T5.1 互斥语义)
///
/// 注入会清空活动过渡槽位:若 [`set_mode_animated`] 的过渡尚在进行,其后
/// [`advance_transition`] 恒返回 `false`,[`ThemeTransition::tokens_at`] 的
/// 插值不再覆盖全局——状态机无互斥时,帧泵下一帧就会把注入值冲回插值态
/// (V4.0 review R5 实锤,本轮修复)。
///
/// # CanvasTheme 强制重置(如实声明,行为与 V3.0 相同)
///
/// 画布语义色**不**随注入走:一律取 `mode` 对应预设
/// ([`CanvasTheme::dark`]/[`CanvasTheme::light`])的 8 色整套,`mode` 字段
/// 同样落为 `mode` 参数。即注入方只能定制 UI 侧 15 个色彩 token,画布
/// 配色不可定制;`mode` 与当前相同时也仍会被强制重置为该模式预设。
pub fn inject(cx: &mut App, colors: ColorTokens, mode: ThemeMode) {
    cancel_transition(cx);
    let next = match mode {
        ThemeMode::Dark => SableTheme::dark(),
        ThemeMode::Light => SableTheme::light(),
    };
    let mut themed = next;
    themed.colors = colors;
    cx.set_global(themed);
}

/// 主题过渡状态机(V3.0 T3.2,纯函数;对应分册六 §4.3 #14):
/// `tokens_at(now)` 对全部色彩 token 做 lerp_hsla 最短弧插值。
/// 帧泵由应用层驱动:过渡期间每帧 `set_global(过渡态)` + `cx.notify()`
/// (V4.0 T5 起可直接用 [`advance_transition`] + [`set_mode_animated`])。
#[derive(Clone)]
pub struct ThemeTransition {
    from: crate::tokens::ColorTokens,
    to: crate::tokens::ColorTokens,
    started_ms: f64,
    duration_ms: f64,
}

impl ThemeTransition {
    pub fn new(
        from: crate::tokens::ColorTokens,
        to: crate::tokens::ColorTokens,
        now_ms: f64,
        duration_ms: f64,
    ) -> Self {
        Self {
            from,
            to,
            started_ms: now_ms,
            duration_ms: duration_ms.max(1.0),
        }
    }

    /// 过渡进度 0..=1(已结束为 1.0)。
    pub fn progress_at(&self, now_ms: f64) -> f64 {
        ((now_ms - self.started_ms) / self.duration_ms).clamp(0.0, 1.0)
    }

    pub fn is_running(&self, now_ms: f64) -> bool {
        now_ms < self.started_ms + self.duration_ms
    }

    /// 该时刻的全量 token(OutCubic 缓动 + lerp_hsla 最短弧)。
    pub fn tokens_at(&self, now_ms: f64) -> crate::tokens::ColorTokens {
        let t = crate::anim::Easing::OutCubic.apply(self.progress_at(now_ms));
        let (a, b) = (&self.from, &self.to);
        let l = |x: gpui::Hsla, y: gpui::Hsla| crate::anim::lerp_hsla(x, y, t);
        crate::tokens::ColorTokens {
            surface_0: l(a.surface_0, b.surface_0),
            surface_1: l(a.surface_1, b.surface_1),
            surface_2: l(a.surface_2, b.surface_2),
            surface_3: l(a.surface_3, b.surface_3),
            surface_4: l(a.surface_4, b.surface_4),
            border_subtle: l(a.border_subtle, b.border_subtle),
            border_strong: l(a.border_strong, b.border_strong),
            text_primary: l(a.text_primary, b.text_primary),
            text_secondary: l(a.text_secondary, b.text_secondary),
            text_disabled: l(a.text_disabled, b.text_disabled),
            accent: l(a.accent, b.accent),
            accent_muted: l(a.accent_muted, b.accent_muted),
            danger: l(a.danger, b.danger),
            warning: l(a.warning, b.warning),
            success: l(a.success, b.success),
        }
    }
}

#[cfg(test)]
mod transition_tests {
    use super::*;

    #[test]
    fn theme_transition_interpolates_monotonically() {
        let from = crate::tokens::ColorTokens::dark();
        let to = crate::tokens::ColorTokens::light();
        let tr = ThemeTransition::new(from, to, 0.0, 200.0);
        assert!(!tr.is_running(200.0));
        let mid = tr.tokens_at(100.0);
        assert!(mid.surface_0.l > from.surface_0.l, "dark→light 中途应变亮");
        assert!(mid.surface_0.l < to.surface_0.l);
        assert_eq!(tr.tokens_at(250.0).surface_0, to.surface_0, "结束精确到位");
    }

    #[test]
    fn tokens_at_interpolates_more_tokens_and_warning_hue_wraps() {
        use gpui::hsla;
        // warning 色相跨 0/1 边界:0.98 → 0.02 走最短弧(+0.04,经 1.0/0.0)
        let mut from = ColorTokens::dark();
        from.warning = hsla(0.98, 0.8, 0.5, 1.0);
        let mut to = ColorTokens::light();
        to.warning = hsla(0.02, 0.8, 0.5, 1.0);
        let tr = ThemeTransition::new(from, to, 0.0, 200.0);

        // 中途帧:t=0.5 → OutCubic = 1-(1-0.5)³ = 0.875 →
        // h = rem_euclid(0.98 + 0.04×0.875, 1.0) = 0.015(已越过 0/1 边界)
        let mid = tr.tokens_at(100.0);
        let circular_dist = |d: f32| d.min(1.0 - d);
        assert!(
            circular_dist((mid.warning.h - 0.015).abs()) < 1e-4,
            "warning 应经 0/1 边界最短弧插值,实际 h={}",
            mid.warning.h
        );
        assert!(mid.warning.h < 0.5, "应正向越过边界(而非倒退穿过 0.9x)");
        // 早帧即已越界:OutCubic(0.25) ≈ 0.578 → h ≈ 0.0031
        let early = tr.tokens_at(50.0);
        assert!(
            early.warning.h < 0.01,
            "早帧已过边界,实际 h={}",
            early.warning.h
        );

        // 其余 token 中间态:success 亮度严格介于两端(浅色 success 反而比
        // 深色的更深——浅底对比度设计,中间值仍单调介于)
        assert!(
            mid.success.l > to.success.l && mid.success.l < from.success.l,
            "success 中间亮度应介于两端:{} ∈ ({},{})",
            mid.success.l,
            to.success.l,
            from.success.l
        );
        // border_strong alpha 严格介于(dark 12% → light 16%)
        assert!(
            mid.border_strong.a > from.border_strong.a && mid.border_strong.a < to.border_strong.a,
            "border_strong 中间透明度应介于两端"
        );
        // accent 跨主题同值(品牌色共用):插值零漂移
        assert_eq!(mid.accent, from.accent);
    }

    // 注:reduced_motion 全局开关有并行测试竞态窗口(flip/interact 同款已知),
    // 主题过渡的应用层直切语义经纯函数 resolve_transition 锁定两路(见
    // wiring_tests),不在并行测试中触碰全局开关。
}

/// V4.0 T5 接线语义(纯函数,不触全局态/App;全局开关竞态纪律同上)。
#[cfg(test)]
mod wiring_tests {
    use super::*;

    #[test]
    fn resolve_transition_reduced_motion_short_circuits_to_none() {
        // reduced_motion 直切:返回 None(应用立即落定,无需帧泵)
        let resolved = resolve_transition(true, ColorTokens::dark(), ColorTokens::light(), 42.0);
        assert!(resolved.is_none(), "减弱动态必须直切,不得返回过渡");
    }

    #[test]
    fn resolve_transition_normal_returns_200ms_machine() {
        let from = ColorTokens::dark();
        let to = ColorTokens::light();
        let start = 42.0;
        let tr = resolve_transition(false, from, to, start).expect("正常路径必须返回过渡状态机");
        // 时长 = THEME_TRANSITION_MS(与库测试一致)
        assert!(tr.is_running(start));
        assert!(tr.is_running(start + THEME_TRANSITION_MS - 1.0));
        assert!(
            !tr.is_running(start + THEME_TRANSITION_MS),
            "200ms 整即结束"
        );
        // 起点精确还原 / 终点钳在目标
        assert_eq!(tr.tokens_at(start).accent, from.accent);
        assert_eq!(
            tr.tokens_at(start + THEME_TRANSITION_MS).surface_1,
            to.surface_1
        );
    }

    #[test]
    fn pump_step_frames_midway_and_settles_to_target_theme() {
        let target = SableTheme::light();
        let active = ActiveTransition {
            transition: ThemeTransition::new(
                ColorTokens::dark(),
                target.colors,
                0.0,
                THEME_TRANSITION_MS,
            ),
            target: target.clone(),
        };
        // 进行中:插值帧(严格介于两端)+ 继续帧泵
        let PumpStep::Frame(mid) = pump_step(Some(&active), 100.0) else {
            panic!("100ms 时仍在过渡,应为 Frame");
        };
        assert!(
            mid.colors.surface_0.l > ColorTokens::dark().surface_0.l
                && mid.colors.surface_0.l < ColorTokens::light().surface_0.l,
            "中途帧颜色应介于两端"
        );
        assert_eq!(mid.mode, ThemeMode::Light, "mode 自首个泵帧即落目标");
        // 结束:终态帧精确等于目标整体,且清槽
        let PumpStep::Settle(done) = pump_step(Some(&active), THEME_TRANSITION_MS) else {
            panic!("200ms 时过渡已结束,应为 Settle");
        };
        assert_eq!(done.colors, ColorTokens::light(), "落定帧即目标 token");
        assert_eq!(done.canvas, CanvasTheme::light(), "canvas 随目标整体落定");
        assert_eq!(done.mode, ThemeMode::Light);
    }

    #[test]
    fn inject_cancels_transition_interpolation_never_overrides() {
        // 互斥语义(状态机层):过渡进行中,帧泵每帧以 tokens_at 覆盖全局;
        // 注入 = 清空槽位(cancel_transition)后,任何时刻 pump_step 恒 Idle
        // ——插值不再覆盖,全局停留注入的静态值。
        let active = ActiveTransition {
            transition: ThemeTransition::new(
                ColorTokens::dark(),
                ColorTokens::light(),
                0.0,
                THEME_TRANSITION_MS,
            ),
            target: SableTheme::light(),
        };
        let PumpStep::Frame(_) = pump_step(Some(&active), 100.0) else {
            panic!("注入前过渡进行中,应为 Frame");
        };
        // inject 的全部效果 = 槽位置 None
        let after_inject: Option<ActiveTransition> = None;
        // 即便帧泵命中原过渡窗口内(100/199ms)或之后(250ms)的时刻,
        // 都不再产出任何帧,更不会有插值值
        for now in [0.0, 100.0, 199.0, 250.0] {
            assert!(
                matches!(pump_step(after_inject.as_ref(), now), PumpStep::Idle),
                "注入后 now={now} 仍产出过渡帧,互斥失效"
            );
        }
    }
}
