//! A7 微交互接线(08 迭代计划 A7;分册六 §4.3 动画清单 14 项的组件层落点)。
//!
//! # 可复用件(本模块,组件按需取用)
//!
//! - 状态层(TOK-04,报告 §5.3.3):[`InteractState`] + [`state_layer`]——
//!   hover/press 用白/黑 alpha 叠加(极性随底色亮度自动取向),selected =
//!   accent 同比例 alpha(深浅一致),disabled 容器不变;修复旧亮度法在
//!   浅色底钳到 1.0 失效的问题;
//! - 禁用态统一规则(TOK-07,报告 §5.9):**禁用 = 仅前景降级、容器不变**
//!   ——任何启用态前景档一律降为 `text_disabled`([`disabled_foreground`]),
//!   容器背景/描边**不**整体降饱和或变色([`state_layer`] 的
//!   [`InteractState::Disabled`] 分支恒等直通,`StateLayerTokens` 的
//!   `disabled` alpha 恒 0)。组件的禁用路径(如 NumberField 禁用态、
//!   EffectStack 行内小按钮)一律经这两件落色,禁止再各写各的灰态
//!   (规则门禁:TC-TOK-DISABLED-01);
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
//! # 可访问性(第 4 组 A11Y 批次,报告 §4.3/§5.10)
//!
//! 本模块同时是无障碍落地的**单点老家**:
//! - A11Y-01 焦点环:[`focus_ring`](两层环唯一实现,choice/tabs/select/
//!   text_field 共用)+ [`focus_ring_spec`](深浅主题纯函数裁决)+
//!   [`focus_region`](F6 面板区环游选择,键位绑定属宿主壳);
//! - A11Y-01 列表键盘导航:[`list_nav`](图层行/树行/效果行共用状态机)、
//!   [`timeline_seek_step`](时间轴 seek)、[`gradient_stop_nav`](色标);
//! - A11Y-02 语义接口:[`SemanticRole`] / [`Semantic`](存态) +
//!   [`attach_semantics`](渲染层唯一挂接点)。**gpui 0.2.2 无语义树 API**
//!   (crate 无 `_accessibility` 模块、无 accesskit 依赖,源码核实)——
//!   label/role 目前为库侧存态,读屏消费待 TD-01 升级窗口,如实声明、
//!   不虚标;走查表契约见 `docs/a11y-notes.md`;
//! - A11Y-03 命中区:[`MIN_HIT_PX`] + [`hit_size`] / [`hit_slot`]。
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

use gpui::{Div, FocusHandle, Hsla, IntoElement, ParentElement, SharedString, Styled, div, px};

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

/// 禁用态前景色(TOK-07 统一规则,纯函数):**禁用 = 仅前景降级**——任何
/// 启用态前景档(`text_strong`..`text_placeholder`)一律降为
/// `text_disabled` 令牌,不做亮度/饱和度派生(文字档位是 token,禁用色
/// 必须同源,禁止组件自调灰)。容器侧规则见 [`state_layer`] 的
/// [`InteractState::Disabled`] 分支:恒等直通,背景/描边逐位不变。
///
/// `enabled_fg` 参与签名是为了让调用点可读:`disabled_foreground(启用档,
/// text_disabled)` 自证"从哪一档降级、降到哪一档";实现恒返回第二参,
/// 组件不得用它做颜色运算。
#[must_use]
pub fn disabled_foreground(_enabled_fg: Hsla, text_disabled: Hsla) -> Hsla {
    text_disabled
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

// ---------------------------------------------------------------------------
// A11Y-03:命中区下限(报告 §4.3/§5.10;视觉可小、命中必须 ≥24px)
// ---------------------------------------------------------------------------

/// 命中区下限(px,A11Y-03:桌面可用性 24px 红线)。视觉尺寸可小于它
/// (眼睛/锁 10px、色标芯片 12px、IconButton 20px),但**可点热区**必须
/// ≥ 本值——经 [`hit_size`](外扩)或 [`hit_slot`](透明热区容器)落地。
pub const MIN_HIT_PX: f32 = 24.0;

/// 命中区尺寸(纯函数,TC-A11Y-HIT-01 的被测单点):`max(视觉值, 24)`。
/// 视觉已 ≥ 24(Choice 命中行 26、ColorWell 24)原样返回;不足则抬到
/// [`MIN_HIT_PX`]——调用方把返回值用在**热区容器**上,视觉本体尺寸不变
/// (命中区扩容允许视觉不变)。
#[must_use]
pub fn hit_size(visual_px: f32) -> f32 {
    if visual_px.is_finite() {
        visual_px.max(MIN_HIT_PX)
    } else {
        MIN_HIT_PX
    }
}

/// 透明命中容器(A11Y-03,`hit_size` 的装配落点):把小于 24px 的视觉件
/// 包进 ≥24px 的方形热区(视觉居中、热区透明)。事件监听(`on_mouse_down`
/// 等)由调用方挂在**返回的容器**上——补白区点击同样生效;视觉本体
/// (参数 div)只负责画。
#[must_use]
pub fn hit_slot(visual: Div) -> Div {
    div()
        .size(px(MIN_HIT_PX))
        .flex()
        .items_center()
        .justify_center()
        .child(visual)
}

// ---------------------------------------------------------------------------
// A11Y-01:焦点环单点(报告 §5.3.3/§5.10.1;原 choice.rs 私有件的提升)
// ---------------------------------------------------------------------------

/// 焦点环 accent 描边宽(px,报告 §5.3.3:focus 1.5)。
pub const RING_BORDER_PX: f32 = 1.5;
/// 焦点环外扩(px,报告 §5.3.3:accent 环在控件外缘之外)。
pub const RING_OUTSET_PX: f32 = 2.5;
/// 焦点环内侧隔离环外扩(px):贴控件外缘 1px。
pub const RING_ISOLATION_OUTSET_PX: f32 = 1.0;
/// 焦点环内侧隔离环不透明度(报告 §5.3.3 内侧 40% 隔离;色 =
/// [`crate::tokens::ELEVATION_SHADOW_TINT`],alpha 经元素 opacity 落地,
/// 无颜色字面量)。
pub const RING_ISOLATION_OPACITY: f32 = 0.4;

/// 焦点环样式规格(纯数据;[`focus_ring_spec`] 的输出,TC-A11Y-RING-01
/// 的断言面——元素树不可内省,几何/颜色裁决独立为纯函数可测)。
#[cfg(feature = "theme")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusRingSpec {
    /// 外环颜色(= 主题 accent 令牌)
    pub ring_color: Hsla,
    /// 外环描边宽([`RING_BORDER_PX`])
    pub ring_border_px: f32,
    /// 外环外扩([`RING_OUTSET_PX`])
    pub ring_outset_px: f32,
    /// 内侧隔离环颜色(= [`crate::tokens::ELEVATION_SHADOW_TINT`])
    pub isolation_color: Hsla,
    /// 内侧隔离环不透明度([`RING_ISOLATION_OPACITY`])
    pub isolation_opacity: f32,
    /// 内侧隔离环外扩([`RING_ISOLATION_OUTSET_PX`])
    pub isolation_outset_px: f32,
}

/// 焦点环样式裁决(纯函数,深浅两主题各出一套,TC-A11Y-RING-01):外环 =
/// 主题 accent;隔离环 = 全局阴影色调令牌(深浅同值);几何常量跨主题
/// 一致(可辨识性,WCAG 2.4.7 焦点可见的组件侧落地)。
#[cfg(feature = "theme")]
#[must_use]
pub fn focus_ring_spec(colors: &crate::tokens::ColorTokens) -> FocusRingSpec {
    FocusRingSpec {
        ring_color: colors.accent,
        ring_border_px: RING_BORDER_PX,
        ring_outset_px: RING_OUTSET_PX,
        isolation_color: crate::tokens::ELEVATION_SHADOW_TINT,
        isolation_opacity: RING_ISOLATION_OPACITY,
        isolation_outset_px: RING_ISOLATION_OUTSET_PX,
    }
}

/// 焦点环两层(报告 §5.3.3,A11Y-01 的**单点实现**;choice/tabs/select/
/// text_field 及新组件共用——禁止组件各画各的环):内层 = 贴控件外缘
/// 1px 黑 40% 隔离环,外层 = accent 1.5px 环。两层均为绝对定位描边 div,
/// 由调用方挂在 `relative` 容器内(画在内容下层)。
#[cfg(feature = "theme")]
pub fn focus_ring(accent: Hsla, radius: f32) -> [gpui::AnyElement; 2] {
    [
        div()
            .absolute()
            .inset(px(-RING_ISOLATION_OUTSET_PX))
            .border_1()
            .border_color(crate::tokens::ELEVATION_SHADOW_TINT)
            .opacity(RING_ISOLATION_OPACITY)
            .rounded(px(radius))
            .into_any_element(),
        div()
            .absolute()
            .inset(px(-RING_OUTSET_PX))
            .border(px(RING_BORDER_PX))
            .border_color(accent)
            .rounded(px(radius))
            .into_any_element(),
    ]
}

/// F6 区环游的纯下标步进([`focus_region`] 的被测单点):`len` 区、当前
/// 下标(`None` = 无当前),向后 +1、向前 -1,端点回绕成环;无当前时向后
/// 取 0、向前取尾。`len = 0` → `None`。
#[must_use]
fn region_step(len: usize, current: Option<usize>, forward: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match current {
        Some(ix) => {
            if forward {
                (ix + 1) % len
            } else {
                (ix + len - 1) % len
            }
        }
        None if forward => 0,
        None => len - 1,
    })
}

/// F6 面板区循环(纯选择函数,A11Y-01 的宿主契约助手):`regions` 为面板
/// 区焦点句柄表(宿主壳注册),返回下一个应聚焦的句柄——`forward` 向后
/// (F6)、`!forward` 向前(Ctrl+F6),端点回绕成环;`current` 不在表内
/// (或 `None`)时取首/尾。**绑定键位属宿主壳**(键位注册表单源纪律),
/// 本函数只做环游选择,宿主拿到返回值后 `handle.focus(window)`。
#[must_use]
pub fn focus_region(
    regions: &[FocusHandle],
    current: Option<&FocusHandle>,
    forward: bool,
) -> Option<FocusHandle> {
    let current_ix = current.and_then(|cur| regions.iter().position(|h| h == cur));
    region_step(regions.len(), current_ix, forward).and_then(|ix| regions.get(ix).cloned())
}

// ---------------------------------------------------------------------------
// A11Y-01:列表键盘导航(报告 §5.10.2;状态机纯函数 + 组件接线)
// ---------------------------------------------------------------------------

/// 列表键盘导航意图([`list_nav`] 的输出)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListNavIntent {
    /// 移动高亮/选择到该下标(已钳制;同值 = 已在端点)
    Move(usize),
    /// 激活当前项(Enter:翻转可见/锁/启用等,语义由列表件定义)
    Activate,
    /// 退出列表(Esc:宿主决定回落焦点;无浮层的列表件可忽略)
    Escape,
}

/// 列表键盘导航状态机(纯函数,唯一导航语义单点):`up`/`down` 相邻移动
/// (端点钳制不回绕)、`home`/`end` 跳端、`enter` 激活、`escape` 退出;
/// 其余键无意图(`None`)。空表恒 `None`。图层行/树行/效果栈行共用
/// (TC-A11Y-NAV-01 的被测单点)。
#[must_use]
pub fn list_nav(highlighted: usize, count: usize, key: &str) -> Option<ListNavIntent> {
    if count == 0 {
        return None;
    }
    let last = count - 1;
    match key {
        "up" => Some(ListNavIntent::Move(highlighted.saturating_sub(1))),
        "down" => Some(ListNavIntent::Move(highlighted.saturating_add(1).min(last))),
        "home" => Some(ListNavIntent::Move(0)),
        "end" => Some(ListNavIntent::Move(last)),
        "enter" => Some(ListNavIntent::Activate),
        "escape" => Some(ListNavIntent::Escape),
        _ => None,
    }
}

/// 时间轴键盘 seek 步进(纯函数,A11Y-01 时间轴入 Tab 序的导航语义):
/// `left`/`right` = ∓/± `step_ms`(步长由调用方取 [`crate::timeline_view::
/// nice_tick_step_ms`] 等既有档,不另立手感系数)、`home` = 0、`end` =
/// 时长末尾;结果一律钳入 `[0, duration_ms]`。其余键 `None`。
#[must_use]
pub fn timeline_seek_step(
    playhead_ms: u64,
    step_ms: u64,
    duration_ms: u64,
    key: &str,
) -> Option<u64> {
    let clamped = |v: u64| v.min(duration_ms);
    match key {
        "left" => Some(clamped(playhead_ms.saturating_sub(step_ms))),
        "right" => Some(clamped(playhead_ms.saturating_add(step_ms))),
        "home" => Some(0),
        "end" => Some(duration_ms),
        _ => None,
    }
}

/// 渐变编辑器色标键盘导航(纯函数,A11Y-01 色标入 Tab 序的导航语义):
/// `left`/`right` = 相邻色标(端点钳制)、`home`/`end` = 首/末色标;
/// 未选中时 `left` 落首标、`right` 落次标(从 0 起步)。空表/其余键 `None`。
#[must_use]
pub fn gradient_stop_nav(selected: Option<usize>, count: usize, key: &str) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let last = count - 1;
    let current = selected.map_or(0, |s| s.min(last));
    match key {
        "left" => Some(current.saturating_sub(1)),
        "right" => Some(current.saturating_add(1).min(last)),
        "home" => Some(0),
        "end" => Some(last),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// A11Y-02:语义接口层(gpui 0.2.2 无语义树,见下——库侧存态 + 挂接点)
// ---------------------------------------------------------------------------

/// 组件语义角色(A11Y-02 库侧枚举;**gpui 0.2.2 无语义树 API**——crate 内
/// 无 `_accessibility` 模块、Cargo 无 accesskit 依赖、`Interactivity` 无
/// role/label 字段,2026-10 源码核实,与旧版 docs/a11y-notes.md 记载的
/// "自带 AccessKit 骨架"不符)。本枚举按 AccessKit/WCAG 常用角色建面,
/// 供宿主与测试引用;TD-01(gpui 升级)出现语义树后,在
/// [`attach_semantics`] 单点映射为原生 role。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SemanticRole {
    /// 按钮(Button/触发钮)
    Button,
    /// 图标按钮(可访问名 = label 槽/tooltip)
    IconButton,
    /// 文本输入(NumberField/TextField 编辑态)
    TextField,
    /// 勾选框
    Checkbox,
    /// 开关
    Switch,
    /// 单选钮
    Radio,
    /// 下拉选择
    Select,
    /// 页签(单个页签页)
    Tab,
    /// 滑杆(时间轴/连续量)
    Slider,
    /// 列表项(图层行/效果行/树行)
    ListItem,
    /// 列表容器(图层面板/效果栈)
    List,
    /// 分组容器(检查器/属性行)
    Group,
    /// 色井(点击开取色器)
    ColorWell,
    /// 取色器(色轮/渐变编辑器)
    ColorPicker,
    /// 滚动区
    ScrollRegion,
    /// 装饰件(读屏应跳过;NeonCard 等)
    Decoration,
}

impl SemanticRole {
    /// 角色的稳定标识串(诊断/未来映射用)。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SemanticRole::Button => "button",
            SemanticRole::IconButton => "icon-button",
            SemanticRole::TextField => "text-field",
            SemanticRole::Checkbox => "checkbox",
            SemanticRole::Switch => "switch",
            SemanticRole::Radio => "radio",
            SemanticRole::Select => "select",
            SemanticRole::Tab => "tab",
            SemanticRole::Slider => "slider",
            SemanticRole::ListItem => "list-item",
            SemanticRole::List => "list",
            SemanticRole::Group => "group",
            SemanticRole::ColorWell => "color-well",
            SemanticRole::ColorPicker => "color-picker",
            SemanticRole::ScrollRegion => "scroll-region",
            SemanticRole::Decoration => "decoration",
        }
    }
}

/// 组件语义槽(A11Y-02 库侧存态):可访问名(label)+ 角色(role)。
///
/// **如实边界**:gpui 0.2.2 无语义树,label/role 目前只**存态**(宿主可读、
/// 测试可断言、读屏器暂时不可消费)。TD-01 升级出语义树后,渲染层在
/// [`attach_semantics`] 单点把本结构落成原生语义节点——届时各组件已带的
/// `.label()`/`.role()` 槽零改动接通,读屏走查表见 `docs/a11y-notes.md`。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Semantic {
    label: Option<SharedString>,
    role: Option<SemanticRole>,
}

impl Semantic {
    /// 空语义槽。
    #[must_use]
    pub fn new() -> Self {
        Semantic::default()
    }

    /// 设置可访问名。
    #[must_use]
    pub fn with_label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// 设置角色。
    #[must_use]
    pub fn with_role(mut self, role: SemanticRole) -> Self {
        self.role = Some(role);
        self
    }

    /// 可访问名(组件显式 `.label(...)` 的存态)。
    #[must_use]
    pub fn label(&self) -> Option<&SharedString> {
        self.label.as_ref()
    }

    /// 角色(组件显式 `.role(...)` 的存态)。
    #[must_use]
    pub fn role(&self) -> Option<SemanticRole> {
        self.role
    }

    /// 解析角色:显式 `.role(...)` 优先,否则回落组件类型默认值。
    #[must_use]
    pub fn resolved_role(&self, fallback: SemanticRole) -> SemanticRole {
        self.role.unwrap_or(fallback)
    }
}

/// 语义挂接点(A11Y-02 渲染层**单点**):全部组件渲染根经此透传——当前
///(gpui 0.2.2)为恒等透传;TD-01 升级出语义树后,唯一需要改的本函数:
/// 在此把 `semantic` 落成原生节点再返回。**禁止**组件绕开本点自行挂语义
/// (单点收口,防升级时漏改)。
pub fn attach_semantics<E: IntoElement>(element: E, _semantic: &Semantic) -> gpui::AnyElement {
    element.into_any_element()
}

/// 组件语义槽 builder 三件套(`label`/`role`/`semantic` 访问器)的收口
/// 宏:字段名统一 `semantic: Semantic`(由使用方在结构体声明)。按钮族
/// 等可见文本即可访问名的组件,另覆写 `resolved` 语义见各自模块。
macro_rules! semantic_slot {
    ($ty:ty) => {
        #[allow(missing_docs)]
        impl $ty {
            /// 可访问名(A11Y-02 语义槽;gpui 0.2.2 无语义树,存态待 TD-01
            /// 接通,见 [`Semantic`](crate::interact::Semantic))。
            #[must_use]
            pub fn label(mut self, label: impl Into<gpui::SharedString>) -> Self {
                self.semantic = std::mem::take(&mut self.semantic).with_label(label);
                self
            }

            /// 语义角色覆写(默认值 = 组件类型映射,见各模块 doc)。
            #[must_use]
            pub fn role(mut self, role: crate::interact::SemanticRole) -> Self {
                self.semantic = std::mem::take(&mut self.semantic).with_role(role);
                self
            }

            /// 语义槽只读访问(宿主/测试断言面)。
            #[must_use]
            pub fn semantic(&self) -> &crate::interact::Semantic {
                &self.semantic
            }
        }
    };
}
pub(crate) use semantic_slot;

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

    // -----------------------------------------------------------------------
    // 第 4 组 A11Y 批次(报告 §4.3/§5.10)
    // -----------------------------------------------------------------------

    /// TC-A11Y-RING-01:focus ring 深浅两主题样式断言(纯函数)。外环 =
    /// 各主题 accent;隔离环 = 阴影色调令牌(两主题同值);几何常量跨主题
    /// 一致、与报告 §5.3.3 规格(1.5px 外描边 + 内侧隔离环)逐项相等。
    #[cfg(feature = "theme")]
    #[test]
    fn tc_a11y_ring_01_focus_ring_spec_both_themes() {
        for colors in [
            crate::tokens::ColorTokens::dark(),
            crate::tokens::ColorTokens::light(),
        ] {
            let spec = focus_ring_spec(&colors);
            assert_eq!(spec.ring_color, colors.accent, "外环 = 主题 accent");
            assert_eq!(spec.isolation_color, crate::tokens::ELEVATION_SHADOW_TINT);
            assert_eq!(spec.ring_border_px, 1.5, "报告 §5.3.3:accent 描边 1.5px");
            assert_eq!(spec.ring_outset_px, 2.5, "外环在控件外缘之外");
            assert_eq!(spec.isolation_outset_px, 1.0, "隔离环贴控件外缘");
            assert!((spec.isolation_opacity - 0.4).abs() < 1e-6, "内侧隔离 40%");
        }
        // 深浅两主题的外环必须互异(可辨识断言不能靠同色凑数)
        let dark = focus_ring_spec(&crate::tokens::ColorTokens::dark());
        let light = focus_ring_spec(&crate::tokens::ColorTokens::light());
        assert_ne!(dark.ring_color, light.ring_color, "两主题 accent 不同");
        assert_eq!(dark.ring_border_px, light.ring_border_px, "几何跨主题一致");
    }

    /// TC-A11Y-HIT-01:命中区尺寸纯函数断言(A11Y-03 逐组件列值)。视觉
    /// 尺寸可小,`hit_size` 一律抬到 ≥24px;已达标者原样返回。
    #[test]
    fn tc_a11y_hit_01_hit_size_per_component() {
        // 逐组件(视觉 → 命中):
        assert_eq!(
            hit_size(10.0),
            24.0,
            "layer_panel/layer_tree 眼睛/锁 10 → 24"
        );
        assert_eq!(hit_size(12.0), 24.0, "layer_tree 折叠箭头 12 → 24");
        assert_eq!(hit_size(12.0), 24.0, "渐变编辑器色标芯片 12 → 24");
        assert_eq!(hit_size(20.0), 24.0, "IconButton Icon20 热区 20 → 24");
        assert_eq!(hit_size(22.0), 24.0, "Button Compact 高 22 → 24");
        assert_eq!(hit_size(24.0), 24.0, "IconButton Icon24 已达标原样");
        assert_eq!(hit_size(26.0), 26.0, "Choice 命中行 26 已达标原样");
        assert_eq!(hit_size(28.0), 28.0, "面板行 28 已达标原样");
        // 下限常量与防御
        assert_eq!(MIN_HIT_PX, 24.0);
        assert_eq!(hit_size(f32::NAN), 24.0, "非有限入参回落下限");
        assert_eq!(hit_size(0.0), 24.0);
        // 透明热区容器:24px 方形(几何装配由渲染层消费,此处锁常量口径)
        assert_eq!(hit_size(MIN_HIT_PX), MIN_HIT_PX);
    }

    /// TC-A11Y-NAV-01(键盘闭环的状态层):仅键盘完成"选中图层 → 改属性 →
    /// 确认"闭环——列表导航选中目标行、激活翻转可见(Enter)、经数值步进
    /// 改属性(委托 NumberField 同款语义:↑ ↓ 步进)、Enter/Tab 确认提交。
    /// 全程纯函数状态层,不经 GUI。
    #[test]
    fn tc_a11y_nav_01_keyboard_select_edit_confirm_closed_loop() {
        use ListNavIntent as Nav;
        // 场景:3 行图层,初始选中第 0 行、可见性 [真, 真, 真]
        let count = 3;
        let mut selected: usize = 0;
        let mut visible = [true; 3];
        let mut value = 10.0_f64;

        // Tab 进入列表(容器 track_focus 宿主侧),↓ 两次选中第 2 行
        assert_eq!(list_nav(selected, count, "down"), Some(Nav::Move(1)));
        assert_eq!(list_nav(1, count, "down"), Some(Nav::Move(2)));
        selected = 2;
        // Enter 激活 = 翻转可见(列表件的激活语义)
        assert_eq!(list_nav(selected, count, "enter"), Some(Nav::Activate));
        visible[selected] = !visible[selected];
        assert!(!visible[2], "键盘激活翻转可见");
        // ↑ 回到第 1 行改属性(数值 +1 步进,NumberField 同款;Shift = ×10)
        assert_eq!(list_nav(selected, count, "up"), Some(Nav::Move(1)));
        selected = 1;
        value += 1.0;
        // 端点钳制:第 1 行继续 ↑ 到顶不回绕
        assert_eq!(list_nav(selected, count, "up"), Some(Nav::Move(0)));
        selected = 0;
        // Home/End 跳端
        assert_eq!(list_nav(selected, count, "end"), Some(Nav::Move(2)));
        assert_eq!(list_nav(2, count, "home"), Some(Nav::Move(0)));
        // 确认提交(Enter/Tab 语义由编辑器承担;此处锁终值)
        let committed = value;
        assert_eq!(committed, 11.0, "闭环终值 = 选中行改属性后提交");
        // Esc 退出列表(宿主回落焦点的契约信号)
        assert_eq!(list_nav(selected, count, "escape"), Some(Nav::Escape));
        // 边界:空表无导航;其余键无意图
        assert_eq!(list_nav(0, 0, "down"), None);
        assert_eq!(list_nav(0, count, "left"), None);
        assert_eq!(list_nav(0, count, "a"), None);
        // 端点:末行 down 钳末行,首行 up 钳首行
        assert_eq!(list_nav(2, count, "down"), Some(Nav::Move(2)));
        assert_eq!(list_nav(0, count, "up"), Some(Nav::Move(0)));
    }

    /// F6 面板区环游(纯下标步进;句柄匹配壳 [`focus_region`] 的
    /// `position` 查找):向后/向前、端点回绕、无当前取首/尾、空表 None。
    /// 键位绑定属宿主壳(库不绑键)。
    #[test]
    fn focus_region_cycles_and_wraps() {
        assert_eq!(region_step(3, Some(0), true), Some(1));
        assert_eq!(region_step(3, Some(2), true), Some(0), "向后端点回绕");
        assert_eq!(region_step(3, Some(0), false), Some(2), "向前端点回绕");
        assert_eq!(region_step(3, Some(1), false), Some(0));
        assert_eq!(region_step(3, None, true), Some(0), "无当前向后取首");
        assert_eq!(region_step(3, None, false), Some(2), "无当前向前取尾");
        assert_eq!(region_step(0, None, true), None, "空表");
        assert_eq!(region_step(1, Some(0), true), Some(0), "单区自环");
        assert_eq!(region_step(1, None, false), Some(0));
    }

    /// TC-A11Y-LABEL-01 的运行时半边(静态扫描半边在
    /// tests/gate_a11y_label.rs):Semantic 槽的存态与解析语义。
    #[test]
    fn semantic_slot_stores_label_and_role() {
        let sem = Semantic::new()
            .with_label("导出")
            .with_role(SemanticRole::Button);
        assert_eq!(sem.label().map(SharedString::as_ref), Some("导出"));
        assert_eq!(sem.role(), Some(SemanticRole::Button));
        assert_eq!(
            sem.resolved_role(SemanticRole::IconButton),
            SemanticRole::Button
        );
        // 未显式给角色:回落组件类型默认
        let bare = Semantic::new();
        assert_eq!(bare.label(), None);
        assert_eq!(
            bare.resolved_role(SemanticRole::TextField),
            SemanticRole::TextField
        );
        // 角色标识串稳定(未来映射原生 role 的对照面)
        assert_eq!(SemanticRole::Button.as_str(), "button");
        assert_eq!(SemanticRole::ListItem.as_str(), "list-item");
        assert_eq!(SemanticRole::Decoration.as_str(), "decoration");
    }
}
