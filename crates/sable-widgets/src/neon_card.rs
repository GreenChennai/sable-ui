//! Neon Card —— luminaui.in "Neon Card" 的 Sable 对标移植(V4.0 对标组件)。
//!
//! 自绘路线:纯函数 [`render_neon_card`] 用 `sable-paint` 的
//! [`CpuRenderer`](sable_paint::cpu::CpuRenderer) 离屏渲一张**预乘 RGBA8**
//! 位图(gpui 元素层负责 `paint_image` 上屏与 hover/鼠标事件),全部动画
//! 都是 `(state, now_ms)` 的纯函数——可离线单测、逐像素断言(渲染回归
//! 基石,与 sable-paint cpu 兜底同纪律)。
//!
//! # 对标视觉解剖(数值照抄 luminaui.in 的 Neon Card)
//!
//! | # | 对标效果 | 本实现 | 偏差 |
//! |---|---|---|---|
//! | 1 | 2px 流动渐变描边 `linear-gradient(90deg,#FF0080→#7928CA→#FF0080)`,200% 底 2s 线性无限循环 | 描边 paint 为 [`Paint::LinearGradient`],span = 2×卡宽,端点随 `now_ms` 沿描边方向平移(周期 = 卡宽,渐变首尾同色 → 无缝循环) | 对标是 CSS 无限循环;本实现**仅 hover/过渡期间逐帧流动**,空闲冻结缓存(性能纪律,见下) |
//! | 2 | 环境辉光 `box-shadow 0 0 20px 2px` 粉紫 15% | [`render_shadow_rgba`](sable_paint::effects::render_shadow_rgba):blur **消费 L3 浮层海拔令牌**(`tokens::ELEVATIONS[3].blur` = 16,TOK-01 接线);α 0.15 与 [`NeonCardStyle::glow_color`] 为品牌辉光载荷保持移植保真,预乘域着色 | 辉光几何随海拔令牌(消费门禁在 tests/gate_elevation_consumed.rs);强度/着色保持对标值 |
//! | 3 | 光标跟随外辉光:1000px 径向,主色 15%→透明 40%,层透明度 0.75→1 | 卡下整幅径向渐变(中心 = 光标),透明度随 hover 进度在 0.75→1.0 插值 | 对标有 CSS blur;径向渐变本身即软边,无需再模糊 |
//! | 4 | 卡体:圆角 16px、1px white/10 内描边、slate-900 85%(hover 70%) | 卡体 = `tokens.surface_2` × [`NeonCardStyle::glass_alpha`](hover 线性降 [`NEON_GLASS_HOVER_ALPHA_DELTA`]);内描边 = `tokens.border_strong`(深色即 white 12%,浅色为黑系,深浅皆可辨) | **真 backdrop-blur 是 GPU 真机项**,本组件用"半透明深底 + 高光内边"等效——doc 如实声明 |
//! | 5 | 光标跟随内高光:800px 径向 white 8%→透明 40%,淡入 | 以卡体路径为裁切填充径向渐变(路径填充 = 天然裁切),alpha × hover 进度 | 高光物理上是白色,取对标白色常量(非主题语义色) |
//! | 6 | hover 缩放 1.02(500ms 缓动);入场 opacity 0→1 + y 上移 20px→0 | 缩放/位移/整帧透明度都是状态机进度的纯函数,OutCubic | 入场时长对标未给,取 400ms([`NEON_ENTER_MS`]) |
//! | 7 | hover 漂移粒子:10 个 6-10px 小圆点,随机起终点 + scale/opacity 0→1→0 循环(~3s),错峰 0.2s,取渐变色板,各带小辉光 | [`NeonCardState::new`] 种子 → splitmix64 确定性参数;相位错峰纯函数实现(不开 timeline);点 = 径向渐变圆(中心亮→透明,自带辉光) | reduced_motion 时不渲染粒子(装饰运动整体直切) |
//! | 8 | 标题渐变文字(bg-clip-text) | **降级路线 A**:标题由调用方用 gpui 文本 + `tokens.accent` 纯色上屏 | gpui 0.2.2 文本无渐变填充;字形轮廓+渐变 paint 等待 GPU 文本管线,落地后可原样升级 |
//!
//! # 颜色纪律(AGENTS.md §3.4 的边界说明)
//!
//! UI 语义色(卡体 surface_2、内描边 border_strong、兜底 accent)一律来自
//! [`ColorTokens`] 参数(不触全局);`NeonCardStyle` 的渐变色板/辉光色默认值
//! 是**对标效果的样式载荷**(与色轮的用户色同性质,且契约明文指定默认值),
//! 不属于组件硬编码 UI 色。深浅两主题均只影响语义色,霓虹色板为品牌载荷。
//!
//! # 性能纪律(帧泵契约)
//!
//! - [`NeonCardState::tick`] 返回"仍在动画中"——宿主据此决定
//!   `window.request_animation_frame()`(运行才请求帧,分册六 §4.4);
//! - hover 期间 shimmer 与粒子持续吃帧;**空闲(无 hover、无过渡)时
//!   [`is_animating`] 为假,宿主停止帧泵**,[`frame_cache_key`] 键稳定,
//!   缓存位图零重渲(对比标组件的无限 CSS 循环是刻意的性能取舍);
//! - 状态是 `(字段, now_ms)` 的纯函数,宿主每帧读进度即可,无需逐帧
//!   mutate( tick 仅负责启动入场并回报活动态)。
//!
//! # reduced_motion(A8)
//!
//! [`NeonCardState::reduced_motion`] 字段由宿主从 `anim::reduced_motion()`
//! 全局同步(状态机刻意不触全局,保持纯函数可测,与 [`ColorTokens`]
//! 参数同纪律)。为真时:入场/缩放**直切**(进度恒终值)、shimmer 相位
//! 冻结、粒子不渲染。

use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, Bounds, Corners, Div, Entity, InteractiveElement, IntoElement, MouseMoveEvent,
    ParentElement, Pixels, Point, RenderImage, Stateful, StatefulInteractiveElement, Styled as _,
    Window, canvas, div, point, px,
};
use kurbo::{Affine, Rect, Shape as _};
use sable_foundation::scene::{GradientStop, Paint, Rgba8, StrokeStyle};
use sable_paint::prelude::{CpuRenderer, PaintSink as _, ShadowParams, render_shadow_rgba};

use crate::anim::Easing;
use crate::interact::now_ms;
use crate::tokens::{ColorTokens, rgba8_from_hsla};

// ---------------------------------------------------------------------------
// 对标数值常量(来源 luminaui.in;样式载荷,非主题语义色)
// ---------------------------------------------------------------------------

/// 缓冲四周外边距(px,4px 网格):容纳环境辉光(blur 16 的 3×box 支撑域
/// 24px)+ hover 缩放 2% 余量 + 粒子小辉光。缓冲 = 卡体 + 2×本值。
pub const NEON_PAD_PX: f32 = 48.0;
/// 环境辉光模糊(px):**消费 L3 浮层海拔令牌**(TOK-01,报告 §5.3.2,
/// `tokens::ELEVATIONS[3].blur`;消费门禁 tests/gate_elevation_consumed.rs)。
/// 原对标值 20 收编进海拔阶梯(16 = 同阶梯最近档),辉光随令牌收紧。
pub const NEON_GLOW_BLUR_PX: f32 = crate::tokens::ELEVATIONS[3].blur;
/// 环境辉光强度(对标载荷 15%,与 [`NeonCardStyle::glow_color`] 同属品牌
/// 辉光样式,不是中性投影——故**不**取海拔 alpha;海拔几何(blur)消费
/// 令牌,强度保持移植保真)。
pub const NEON_GLOW_ALPHA: f32 = 0.15;
/// 光标跟随外辉光半径(对标的 1000px)。
pub const NEON_CURSOR_GLOW_RADIUS: f64 = 1000.0;
/// 光标外辉光层透明度区间(对标的 0.75→1,随 hover 进度插值)。
pub const NEON_CURSOR_GLOW_ALPHA_RANGE: (f32, f32) = (0.75, 1.0);
/// 光标外辉光/内高光的峰值透明度基准(对标的"主色 15%")。
pub const NEON_CURSOR_GLOW_ALPHA: f32 = 0.15;
/// 光标内高光半径(对标的 800px)。
pub const NEON_SHEEN_RADIUS: f64 = 800.0;
/// 光标内高光峰值(对标的 white 8%)。
pub const NEON_SHEEN_ALPHA: f32 = 0.08;
/// 径向渐变淡出位置(对标两处"→透明 40%")。
pub const NEON_GLOW_FADE_OFFSET: f32 = 0.4;
/// 内描边宽(对标的 1px white/10;颜色走 `border_strong` token)。
pub const NEON_INNER_RING_PX: f32 = 1.0;
/// hover 缩放过渡时长(对标的 500ms 缓动)。
pub const NEON_HOVER_MS: f64 = 500.0;
/// 入场动画时长(对标未给时长;取 400ms 惯例档)。
pub const NEON_ENTER_MS: f64 = 400.0;
/// 入场垂直位移(对标的"y 上移 20px→0")。
pub const NEON_ENTER_OFFSET_Y: f32 = 20.0;
/// hover 时玻璃体降透明幅度(对标的 0.85→0.70)。
pub const NEON_GLASS_HOVER_ALPHA_DELTA: f32 = 0.15;
/// 粒子循环周期(对标的每个 ~3s)。
pub const NEON_PARTICLE_CYCLE_MS: f64 = 3000.0;
/// 粒子错峰启动间隔(对标的 0.2s)。
pub const NEON_PARTICLE_STAGGER_MS: f64 = 200.0;
/// 粒子最小直径(对标的 6-10px)。
pub const NEON_PARTICLE_MIN_SIZE: f32 = 6.0;
/// 粒子最大直径(对标的 6-10px)。
pub const NEON_PARTICLE_MAX_SIZE: f32 = 10.0;
/// 粒子活动区内缩(直径 + 小辉光不出卡体,免裁切层)。
const PARTICLE_INSET_PX: f32 = 24.0;
/// 内高光的物理白(对标 white 8%;高光是光效载荷,不随主题语义翻转)。
const SHEEN_WHITE: Rgba8 = [255, 255, 255, 255];

/// 对标默认渐变色板:`linear-gradient(90deg, #FF0080 → #7928CA → #FF0080)`。
/// 首尾同色是 shimmer 无缝循环的条件;非对称色板会在循环点跳变(如实行为)。
fn default_gradient_stops() -> Vec<(f32, [u8; 4])> {
    vec![
        (0.0, [0xFF, 0x00, 0x80, 0xFF]),
        (0.5, [0x79, 0x28, 0xCA, 0xFF]),
        (1.0, [0xFF, 0x00, 0x80, 0xFF]),
    ]
}

// ---------------------------------------------------------------------------
// 样式与状态
// ---------------------------------------------------------------------------

/// Neon Card 样式(对标 luminaui.in 的可调参数;默认值全部标注对标来源)。
#[derive(Clone, Debug)]
pub struct NeonCardStyle {
    /// 描边渐变 stop(offset, rgba8)。默认 = 对标粉紫三停(首尾同色)。
    pub gradient_stops: Vec<(f32, [u8; 4])>,
    /// 环境辉光色,默认粉 15%(对标 box-shadow 颜色)。
    pub glow_color: [u8; 4],
    /// 卡体圆角(px),默认 16.0(对标的 rounded-2xl)。
    pub corner_radius: f32,
    /// 描边宽(px),默认 2.0(对标的流动渐变描边宽)。
    pub border_width: f32,
    /// shimmer 循环周期(ms),默认 2000.0(对标的 2s 线性)。
    pub shimmer_period_ms: f64,
    /// hover 缩放,默认 1.02(对标值)。
    pub hover_scale: f32,
    /// 玻璃卡体不透明度,默认 0.85(hover 降 0.15 → 0.70,对标值)。
    pub glass_alpha: f32,
    /// 粒子数,默认 10;0 = 关(对标的 10 个漂移光点)。
    pub particles: usize,
}

impl Default for NeonCardStyle {
    fn default() -> Self {
        NeonCardStyle {
            gradient_stops: default_gradient_stops(),
            // 对标:粉紫 15% 环境辉光(0.15 × 255 ≈ 0x26)
            glow_color: [0xFF, 0x00, 0x80, 0x26],
            corner_radius: 16.0,
            border_width: 2.0,
            shimmer_period_ms: 2000.0,
            hover_scale: 1.02,
            glass_alpha: 0.85,
            particles: 10,
        }
    }
}

/// hover 缩放/透明过渡(线性插值段,可被反向打断并从当前进度接续)。
#[derive(Clone, Copy, Debug)]
struct HoverLerp {
    from: f64,
    to: f64,
    started_ms: f64,
}

/// Neon Card 状态机(纯数据 + 时间注入;可离线单测,不触 gpui 全局)。
///
/// 全部动画进度是 `(self, now_ms)` 的纯函数:[`tick`](Self::tick) 只负责
/// 启动入场并回报活动态,宿主逐帧**读**进度即可。
#[derive(Clone, Debug)]
pub struct NeonCardState {
    /// 减弱动态直切开关(宿主从 `anim::reduced_motion()` 全局同步;
    /// 本状态机刻意不触全局,测试直接置位,不碰进程级开关)。
    pub reduced_motion: bool,
    hover: bool,
    /// 鼠标**卡体局部**坐标(原点 = 卡左上,不含缓冲外边距)。
    mouse: Point<Pixels>,
    hover_anim: Option<HoverLerp>,
    /// 入场起点;None = 未起跑(进度恒 1,无入场直显——[`tick`] 首调起跑)。
    enter_started_ms: Option<f64>,
    seed: u64,
}

impl NeonCardState {
    /// 以确定性种子构造(粒子起终点/尺寸由种子派生,同种子逐位同帧)。
    pub fn new(seed: u64) -> Self {
        NeonCardState {
            reduced_motion: false,
            hover: false,
            mouse: point(px(0.0), px(0.0)),
            hover_anim: None,
            enter_started_ms: None,
            seed,
        }
    }

    /// 悬停进出(打断进行中的过渡时从当前进度接续,视觉无跳变;
    /// reduced_motion 直切)。
    pub fn set_hover(&mut self, hover: bool, now_ms: f64) {
        if self.hover == hover {
            return;
        }
        if self.reduced_motion {
            self.hover = hover;
            self.hover_anim = None;
            return;
        }
        let from = self.hover_progress_at(now_ms);
        self.hover = hover;
        self.hover_anim = Some(HoverLerp {
            from,
            to: if hover { 1.0 } else { 0.0 },
            started_ms: now_ms,
        });
    }

    /// 鼠标卡体局部坐标(原点 = 卡左上;元素层负责扣除外边距)。
    pub fn set_mouse(&mut self, pos: Point<Pixels>) {
        self.mouse = pos;
    }

    /// 推进一帧并回报"仍在动画中"(宿主帧泵判决:真 = 继续请求动画帧)。
    /// 首调启动入场动画;之后为幂等读。
    pub fn tick(&mut self, now_ms: f64) -> bool {
        if self.enter_started_ms.is_none() {
            self.enter_started_ms = Some(now_ms);
        }
        self.is_animating(now_ms)
    }

    /// 仍在动画中(无副作用读):入场/缩放过渡未完,或 hover 期间
    /// shimmer/粒子循环持续吃帧。reduced_motion 恒假(空闲零帧提交)。
    pub fn is_animating(&self, now_ms: f64) -> bool {
        if self.reduced_motion {
            return false;
        }
        if self
            .enter_started_ms
            .is_some_and(|start| now_ms < start + NEON_ENTER_MS)
        {
            return true;
        }
        if let Some(anim) = self.hover_anim {
            if now_ms < anim.started_ms + NEON_HOVER_MS {
                return true;
            }
        }
        self.hover
    }

    /// hover 过渡进度 0..=1(缩放/玻璃降透/辉光淡入共用;终值精确钳位)。
    pub fn hover_progress_at(&self, now_ms: f64) -> f64 {
        if self.reduced_motion {
            return if self.hover { 1.0 } else { 0.0 };
        }
        let Some(anim) = self.hover_anim else {
            return if self.hover { 1.0 } else { 0.0 };
        };
        let t =
            Easing::OutCubic.apply(((now_ms - anim.started_ms) / NEON_HOVER_MS).clamp(0.0, 1.0));
        anim.from + (anim.to - anim.from) * t
    }

    /// 入场进度 0..=1(opacity 与 y 位移共用;未起跑 = 1 直显)。
    pub fn enter_progress_at(&self, now_ms: f64) -> f64 {
        if self.reduced_motion {
            return 1.0;
        }
        match self.enter_started_ms {
            None => 1.0,
            Some(start) => {
                Easing::OutCubic.apply(((now_ms - start) / NEON_ENTER_MS).clamp(0.0, 1.0))
            }
        }
    }

    /// 是否悬停中。
    pub fn is_hovered(&self) -> bool {
        self.hover
    }

    /// 鼠标卡体局部坐标。
    pub fn mouse(&self) -> Point<Pixels> {
        self.mouse
    }

    /// 确定性种子(粒子参数源)。
    pub fn seed(&self) -> u64 {
        self.seed
    }
}

// ---------------------------------------------------------------------------
// 纯函数渲染
// ---------------------------------------------------------------------------

/// 渲染一帧 Neon Card 位图(**预乘 RGBA8**,缓冲 = 卡体 + 2×[`NEON_PAD_PX`])。
///
/// 纯函数:同 `(width, height, style, state, tokens, now_ms)` 逐位同帧,
/// 可直接对输出做像素断言。卡体矩形 = 缓冲内缩 [`NEON_PAD_PX`](居中)。
/// 标题/正文文字不在位图内——由元素层的 gpui 文本承担(见模块 doc 第 8 条)。
pub fn render_neon_card(
    width: u32,
    height: u32,
    style: &NeonCardStyle,
    state: &NeonCardState,
    tokens: &ColorTokens,
    now_ms: f64,
) -> Vec<u8> {
    let w16 = width.clamp(1, u32::from(u16::MAX)) as u16;
    let h16 = height.clamp(1, u32::from(u16::MAX)) as u16;
    let (card, card_w, card_h) = card_geometry(width, height);

    let hover_p = state.hover_progress_at(now_ms);
    let enter_p = state.enter_progress_at(now_ms);

    // 卡体矩形(card_geometry 返回值)已在缓冲坐标(x0=PAD 对称外边距)——
    // 变换只做绕**缓冲系卡心**的 hover 缩放 × 入场 y 位移;若再平移一次
    // PAD 会把整幅内容推进外边距(首轮实测踩坑:描边/粒子采样全偏)。
    let center = (
        card.x0 + f64::from(card_w) / 2.0,
        card.y0 + f64::from(card_h) / 2.0,
    );
    let to_buffer = Affine::translate(center)
        * Affine::scale(f64::from(1.0 + (style.hover_scale - 1.0) * hover_p as f32))
        * Affine::translate((-center.0, -center.1))
        * Affine::translate((
            0.0,
            f64::from(-NEON_ENTER_OFFSET_Y * (1.0 - enter_p as f32)),
        ));

    let bw = style.border_width.max(0.0);
    let body_path = Rect::new(
        card.x0 + f64::from(bw) / 2.0,
        card.y0 + f64::from(bw) / 2.0,
        card.x1 - f64::from(bw) / 2.0,
        card.y1 - f64::from(bw) / 2.0,
    )
    .to_rounded_rect(f64::from((style.corner_radius - bw / 2.0).max(0.0)))
    .to_path(0.1);
    let glow_path = card
        .to_rounded_rect(f64::from(style.corner_radius.max(0.0)))
        .to_path(0.1);
    let ring_path = Rect::new(
        card.x0 + f64::from(bw) + f64::from(NEON_INNER_RING_PX) / 2.0,
        card.y0 + f64::from(bw) + f64::from(NEON_INNER_RING_PX) / 2.0,
        card.x1 - f64::from(bw) - f64::from(NEON_INNER_RING_PX) / 2.0,
        card.y1 - f64::from(bw) - f64::from(NEON_INNER_RING_PX) / 2.0,
    )
    .to_rounded_rect(f64::from(
        (style.corner_radius - bw - NEON_INNER_RING_PX / 2.0).max(0.0),
    ))
    .to_path(0.1);

    // 1) 静态环境辉光(对标 box-shadow 0 0 20px 2px @15%;TOK-01:blur 消费
    //    L3 浮层海拔 tokens::ELEVATIONS[3]):阴影管线出预乘黑 coverage,
    //    再按 glow_color 在预乘域着色(垫底;辉光居中无投影偏移)。
    let mut out = render_shadow_rgba(
        &glow_path,
        to_buffer,
        ShadowParams {
            blur_px: NEON_GLOW_BLUR_PX,
            offset: (0.0, 0.0),
            alpha: NEON_GLOW_ALPHA,
        },
        w16,
        h16,
    );
    tint_premul(&mut out, style.glow_color);

    // 2..6) 主内容离屏:光标外辉光 / 粒子 / 玻璃卡体 / 内高光 / 内环+描边
    let mut renderer = CpuRenderer::new(w16, h16, [0, 0, 0, 0]);
    {
        let sink = renderer.sink();
        let mouse = state.mouse();
        let mouse_local = (
            f64::from(mouse.x) + f64::from(NEON_PAD_PX),
            f64::from(mouse.y) + f64::from(NEON_PAD_PX),
        );

        // 2) 光标跟随外辉光(hover 淡入;对标 1000px 径向,主色 15%→透明 40%)
        if hover_p > 0.0 {
            let layer_a = NEON_CURSOR_GLOW_ALPHA_RANGE.0
                + (NEON_CURSOR_GLOW_ALPHA_RANGE.1 - NEON_CURSOR_GLOW_ALPHA_RANGE.0)
                    * hover_p as f32;
            let primary = primary_color(style, tokens);
            let glow_paint = Paint::RadialGradient {
                center: [mouse_local.0, mouse_local.1],
                radius: NEON_CURSOR_GLOW_RADIUS,
                stops: vec![
                    GradientStop {
                        offset: 0.0,
                        color: with_alpha(primary, alpha_byte(NEON_CURSOR_GLOW_ALPHA * layer_a)),
                    },
                    GradientStop {
                        offset: NEON_GLOW_FADE_OFFSET,
                        color: [0, 0, 0, 0],
                    },
                ],
            };
            sink.fill(&glow_paint, Affine::IDENTITY, &buffer_rect(w16, h16));
        }

        // 3) hover 漂移粒子(reduced_motion 直切不渲染;0 = 关)
        if hover_p > 0.0 && !state.reduced_motion && style.particles > 0 {
            let palette = if style.gradient_stops.is_empty() {
                vec![(0.0, rgba8_from_hsla(tokens.accent))]
            } else {
                style.gradient_stops.clone()
            };
            for index in 0..style.particles {
                if let Some((pos, radius, color)) =
                    particle_frame(&palette, state.seed(), index, card_w, card_h, now_ms)
                {
                    let dot = Paint::RadialGradient {
                        center: [pos.0, pos.1],
                        radius: f64::from(radius),
                        stops: vec![
                            GradientStop { offset: 0.0, color },
                            GradientStop {
                                offset: 0.55,
                                color: with_alpha(
                                    color,
                                    alpha_byte(f32::from(color[3]) / 255.0 * 0.55),
                                ),
                            },
                            GradientStop {
                                offset: 1.0,
                                color: [0, 0, 0, 0],
                            },
                        ],
                    };
                    let circle =
                        kurbo::Circle::new(kurbo::Point::new(pos.0, pos.1), f64::from(radius))
                            .to_path(0.1);
                    sink.fill(&dot, to_buffer, &circle);
                }
            }
        }

        // 4) 玻璃卡体:半透明深/浅底(surface_2 × glass_alpha,hover 降透)
        let surface = rgba8_from_hsla(tokens.surface_2);
        let body_alpha =
            (style.glass_alpha - NEON_GLASS_HOVER_ALPHA_DELTA * hover_p as f32).clamp(0.05, 1.0);
        let body_color = with_alpha(
            surface,
            alpha_byte(f32::from(surface[3]) / 255.0 * body_alpha),
        );
        sink.fill(&Paint::Solid(body_color), to_buffer, &body_path);

        // 5) 光标跟随内高光(对标 800px white 8%→透明 40%;卡体路径 = 裁切)
        if hover_p > 0.0 {
            let sheen_paint = Paint::RadialGradient {
                center: [f64::from(mouse.x), f64::from(mouse.y)],
                radius: NEON_SHEEN_RADIUS,
                stops: vec![
                    GradientStop {
                        offset: 0.0,
                        color: with_alpha(
                            SHEEN_WHITE,
                            alpha_byte(NEON_SHEEN_ALPHA * hover_p as f32),
                        ),
                    },
                    GradientStop {
                        offset: NEON_GLOW_FADE_OFFSET,
                        color: [0, 0, 0, 0],
                    },
                ],
            };
            sink.fill(&sheen_paint, to_buffer, &body_path);
        }

        // 6a) 内描边 1px(对标 white/10;走 border_strong token 深浅自适配)
        sink.stroke(
            &StrokeStyle {
                paint: Paint::Solid(rgba8_from_hsla(tokens.border_strong)),
                width: f64::from(NEON_INNER_RING_PX),
            },
            to_buffer,
            &ring_path,
        );

        // 6b) 流动渐变描边(shimmer:渐变端点随时间沿描边方向平移循环;
        //     reduced_motion 相位冻结)
        sink.stroke(
            &StrokeStyle {
                paint: shimmer_paint(
                    style,
                    tokens,
                    card_w,
                    card.x0 + f64::from(card_w) / 2.0,
                    now_ms,
                    state.reduced_motion,
                ),
                width: f64::from(bw),
            },
            to_buffer,
            &body_path,
        );
    }
    let main = renderer.finish();
    over(&mut out, &main);

    // 入场透明度:整帧预乘缩放(opacity 0→1,含辉光)
    scale_premul(&mut out, enter_p as f32);
    out
}

/// 卡体几何:返回(缓冲坐标卡体矩形, 卡宽, 卡高)。缓冲过小时钳到 4px
/// 防退化(形状空路径,渲染安全)。
pub fn card_geometry(width: u32, height: u32) -> (Rect, f32, f32) {
    let fw = width.min(u32::from(u16::MAX)) as f32;
    let fh = height.min(u32::from(u16::MAX)) as f32;
    let card_w = (fw - 2.0 * NEON_PAD_PX).max(4.0);
    let card_h = (fh - 2.0 * NEON_PAD_PX).max(4.0);
    let x0 = ((fw - card_w) / 2.0).max(0.0);
    let y0 = ((fh - card_h) / 2.0).max(0.0);
    (
        Rect::new(
            f64::from(x0),
            f64::from(y0),
            f64::from(x0 + card_w),
            f64::from(y0 + card_h),
        ),
        card_w,
        card_h,
    )
}

/// shimmer 描边 paint:span = 2×卡宽、色板加倍铺满(空间周期 = 卡宽),
/// 端点锚定卡心——相位 p 时卡心落在 span 的 `0.5+(p-0.5)/2` 处(p=0.5
/// 恰在加倍停 = 首停色),窗口每周期平移一个卡宽,对 w 周期图案视觉无缝;
/// reduced_motion 相位恒 0(静止渐变)。
fn shimmer_paint(
    style: &NeonCardStyle,
    tokens: &ColorTokens,
    card_w: f32,
    card_center_x: f64,
    now_ms: f64,
    reduced_motion: bool,
) -> Paint {
    if style.gradient_stops.is_empty() {
        return Paint::Solid(rgba8_from_hsla(tokens.accent));
    }
    let phase = if reduced_motion {
        0.0
    } else {
        (now_ms / style.shimmer_period_ms.max(1.0)).rem_euclid(1.0)
    };
    let span = f64::from(card_w) * 2.0;
    let start_x = card_center_x - span * (0.5 + (phase - 0.5) * 0.5);
    let (mut first, mut second) = (Vec::with_capacity(4), Vec::with_capacity(4));
    for (offset, color) in &style.gradient_stops {
        first.push(GradientStop {
            offset: offset * 0.5,
            color: *color,
        });
        second.push(GradientStop {
            offset: 0.5 + offset * 0.5,
            color: *color,
        });
    }
    first.append(&mut second);
    Paint::LinearGradient {
        start: [start_x, 0.0],
        end: [start_x + f64::from(card_w) * 2.0, 0.0],
        stops: first,
    }
}

/// 单粒子一帧:None = 该帧不可见(未到错峰起点 / 波形谷底)。
/// 返回(卡体局部圆心, 半径, 预乘前颜色·已乘波形透明度)。
/// 起终点/尺寸由 splitmix64(seed ⊕ 序号)确定性派生;周期/错峰为对标常量。
fn particle_frame(
    palette: &[(f32, [u8; 4])],
    seed: u64,
    index: usize,
    card_w: f32,
    card_h: f32,
    now_ms: f64,
) -> Option<((f64, f64), f32, Rgba8)> {
    let mut rng = seed ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut unit = move || {
        rng = rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) % 10_000) as f32 / 10_000.0
    };
    let inset = PARTICLE_INSET_PX.min(card_w / 4.0).min(card_h / 4.0);
    let span_w = (card_w - 2.0 * inset).max(1.0);
    let span_h = (card_h - 2.0 * inset).max(1.0);
    let (x1, y1) = (inset + unit() * span_w, inset + unit() * span_h);
    let (x2, y2) = (inset + unit() * span_w, inset + unit() * span_h);
    let size = NEON_PARTICLE_MIN_SIZE + unit() * (NEON_PARTICLE_MAX_SIZE - NEON_PARTICLE_MIN_SIZE);

    let delay = index as f64 * NEON_PARTICLE_STAGGER_MS;
    if now_ms < delay {
        return None;
    }
    let q = ((now_ms - delay) / NEON_PARTICLE_CYCLE_MS).rem_euclid(1.0);
    let wave = (std::f64::consts::PI * q).sin();
    if wave < 0.05 {
        return None;
    }
    let q = q as f32;
    let color = palette[index % palette.len()].1;
    Some((
        (f64::from(x1 + (x2 - x1) * q), f64::from(y1 + (y2 - y1) * q)),
        ((size * (0.5 + 0.5 * wave as f32)) / 2.0).max(1.0),
        with_alpha(color, alpha_byte(wave as f32)),
    ))
}

/// 缓存指纹:位图全部输入(样式/状态/token 语义色/动画活动时的帧桶)。
/// 空闲时键稳定(缓存命中零重渲),动画中每帧变键(逐帧重画)。
pub fn frame_cache_key(
    style: &NeonCardStyle,
    state: &NeonCardState,
    tokens: &ColorTokens,
    now_ms: f64,
    animating: bool,
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (offset, color) in &style.gradient_stops {
        offset.to_bits().hash(&mut hasher);
        color.hash(&mut hasher);
    }
    style.glow_color.hash(&mut hasher);
    style.corner_radius.to_bits().hash(&mut hasher);
    style.border_width.to_bits().hash(&mut hasher);
    style.shimmer_period_ms.to_bits().hash(&mut hasher);
    style.hover_scale.to_bits().hash(&mut hasher);
    style.glass_alpha.to_bits().hash(&mut hasher);
    style.particles.hash(&mut hasher);
    state.hover.hash(&mut hasher);
    f32::from(state.mouse.x).to_bits().hash(&mut hasher);
    f32::from(state.mouse.y).to_bits().hash(&mut hasher);
    state.reduced_motion.hash(&mut hasher);
    state
        .enter_started_ms
        .map(f64::to_bits)
        .unwrap_or(0)
        .hash(&mut hasher);
    if let Some(anim) = state.hover_anim {
        anim.from.to_bits().hash(&mut hasher);
        anim.to.to_bits().hash(&mut hasher);
        anim.started_ms.to_bits().hash(&mut hasher);
    } else {
        0u64.hash(&mut hasher);
    }
    state.seed.hash(&mut hasher);
    // 仅两个语义色进位图(卡体/内描边);色板与辉光已在 style 里
    rgba8_from_hsla(tokens.surface_2).hash(&mut hasher);
    rgba8_from_hsla(tokens.border_strong).hash(&mut hasher);
    if animating {
        // ~60fps 帧桶:动画中每帧不同键 → 逐帧重画
        ((now_ms / 16.0) as u64).hash(&mut hasher);
    }
    hasher.finish()
}

// ---------------------------------------------------------------------------
// gpui 元素封装
// ---------------------------------------------------------------------------

/// Neon Card 元素组装:事件(hover / 鼠标局部坐标)+ 离屏位图上屏
/// (`canvas` paint 阶段 `Window::paint_image`,pixels 演示页同款)+ 内容
/// 覆盖层(标题/说明等 gpui 文本,由调用方用 token 上色)。
///
/// - `frame`:调用方按 [`frame_cache_key`] 缓存的位图(预乘缓冲经 un-premul
///   转的 `RenderImage`;`image` crate 不进本 crate,转换留在示例/宿主侧);
/// - `frame_width/height`:位图像素尺寸 = 元素尺寸(含 [`NEON_PAD_PX`] 外边距,
///   辉光在此区域内绘制);
/// - 鼠标坐标:监听器拿窗口坐标,减元素原点(canvas prep 阶段捕获)再扣
///   [`NEON_PAD_PX`],得到 [`NeonCardState::set_mouse`] 的卡体局部坐标;
/// - 命中区含外边距(比可视卡体大一圈)——装饰卡可接受,如实注明。
pub fn neon_card(
    id: &'static str,
    style: &NeonCardStyle,
    state: &Entity<NeonCardState>,
    frame: Option<Arc<RenderImage>>,
    frame_width: f32,
    frame_height: f32,
    content: impl IntoElement,
) -> Stateful<Div> {
    let origin: Rc<RefCell<Point<Pixels>>> = Rc::new(RefCell::new(point(px(0.0), px(0.0))));
    let origin_for_prep = origin.clone();
    let origin_for_move = origin.clone();
    let state_for_hover = state.clone();
    let state_for_move = state.clone();

    div()
        .id(id)
        .relative()
        .w(px(frame_width))
        .h(px(frame_height))
        .child(
            canvas(
                move |bounds: Bounds<Pixels>, _: &mut Window, _: &mut App| {
                    *origin_for_prep.borrow_mut() = bounds.origin;
                },
                move |bounds: Bounds<Pixels>, _: (), window: &mut Window, _: &mut App| {
                    if let Some(image) = &frame {
                        let _ = window.paint_image(
                            bounds,
                            Corners::all(px(0.0)),
                            image.clone(),
                            0,
                            false,
                        );
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
        .child(
            div()
                .absolute()
                .inset_0()
                .p(px((NEON_PAD_PX - crate::tokens::SpacingTokens::XL).max(0.0)))
                .rounded(px(style.corner_radius))
                .overflow_hidden()
                .child(content),
        )
        .on_hover(move |hovered: &bool, _: &mut Window, cx: &mut App| {
            state_for_hover.update(cx, |card, cx| {
                card.set_hover(*hovered, now_ms());
                cx.notify();
            });
        })
        .on_mouse_move(
            move |event: &MouseMoveEvent, _: &mut Window, cx: &mut App| {
                let origin = *origin_for_move.borrow();
                let local = point(
                    event.position.x - origin.x - px(NEON_PAD_PX),
                    event.position.y - origin.y - px(NEON_PAD_PX),
                );
                state_for_move.update(cx, |card, cx| {
                    card.set_mouse(local);
                    cx.notify();
                });
            },
        )
}

// ---------------------------------------------------------------------------
// 预乘像素小件(缓冲级合成;与 pixels 演示页的本地实现同语义)
// ---------------------------------------------------------------------------

/// src-over 合成(双方均为预乘 RGBA8):src 盖在 dst 上。
fn over(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let sa = f32::from(s[3]) / 255.0;
        let da = f32::from(d[3]) / 255.0;
        for channel in 0..3 {
            let value = f32::from(s[channel]) + f32::from(d[channel]) * (1.0 - sa);
            d[channel] = value.round().clamp(0.0, 255.0) as u8;
        }
        let out_a = sa + da * (1.0 - sa);
        d[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
    }
}

/// 预乘域着色:把 coverage(alpha)辉光染成目标色(rgb = tint × coverage)。
fn tint_premul(data: &mut [u8], color: Rgba8) {
    for px in data.chunks_exact_mut(4) {
        let coverage = f32::from(px[3]) / 255.0;
        for (channel, tint) in px[..3].iter_mut().zip(&color[..3]) {
            *channel = (f32::from(*tint) * coverage).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// 整帧透明度:预乘域全通道缩放(alpha 同步缩,合法的整体 opacity)。
fn scale_premul(data: &mut [u8], k: f32) {
    let k = k.clamp(0.0, 1.0);
    if k >= 1.0 {
        return;
    }
    for px in data.chunks_exact_mut(4) {
        for channel in px.iter_mut() {
            *channel = (f32::from(*channel) * k).round().clamp(0.0, 255.0) as u8;
        }
    }
}

fn with_alpha(color: Rgba8, alpha: u8) -> Rgba8 {
    [color[0], color[1], color[2], alpha]
}

fn alpha_byte(a: f32) -> u8 {
    (a.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// 外辉光主色:色板首停;色板为空时退到 accent token(纪律兜底)。
fn primary_color(style: &NeonCardStyle, tokens: &ColorTokens) -> Rgba8 {
    style
        .gradient_stops
        .first()
        .map(|(_, color)| *color)
        .unwrap_or_else(|| rgba8_from_hsla(tokens.accent))
}

fn buffer_rect(width: u16, height: u16) -> kurbo::BezPath {
    Rect::new(0.0, 0.0, f64::from(width), f64::from(height)).to_path(0.1)
}

// ---------------------------------------------------------------------------
// 测试:纯函数像素断言(不触 gpui App / 全局开关)+ 状态机
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use gpui::{point, px};

    use super::*;

    /// 测试缓冲:卡体 240×150 + 2×48 外边距。
    const W: u32 = 336;
    const H: u32 = 246;
    const PAD: f32 = NEON_PAD_PX;
    const CARD_W: f32 = 240.0;
    const CARD_H: f32 = 150.0;

    fn sample(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
        let i = ((y * W + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    /// 已起跑、入场已落定、未悬停的静止状态。
    fn settled_state() -> NeonCardState {
        let mut state = NeonCardState::new(42);
        state.tick(0.0);
        state
    }

    fn close(a: u8, b: i16, tol: i16) -> bool {
        (i16::from(a) - b).abs() <= tol
    }

    #[test]
    fn static_frame_border_body_and_glow_pixels() {
        // 静止帧:t=5000(入场 400ms 早已落定,shimmer 相位 0.5)
        let state = settled_state();
        let buf = render_neon_card(
            W,
            H,
            &NeonCardStyle::default(),
            &state,
            &ColorTokens::dark(),
            5000.0,
        );

        // a1. 描边像素:顶边中点。相位 0.5 → 渐变窗口 [−卡宽, +卡宽],
        //     中点落在 span 0.5 的加倍停(= 原板首停粉 #FF0080)。
        let border = sample(&buf, (PAD + CARD_W / 2.0) as u32, (PAD + 1.0) as u32);
        assert!(close(border[3], 255, 6), "描边应不透明:α={}", border[3]);
        assert!(close(border[0], 0xFF, 40), "描边 r 应≈0xFF:{}", border[0]);
        assert!(close(border[1], 0x00, 40), "描边 g 应≈0x00:{}", border[1]);
        assert!(close(border[2], 0x80, 40), "描边 b 应≈0x80:{}", border[2]);

        // a2. 卡体:半透明深底(surface_2 × 0.85,预乘域)
        let dark_surface = rgba8_from_hsla(ColorTokens::dark().surface_2);
        let body = sample(
            &buf,
            (PAD + CARD_W / 2.0) as u32,
            (PAD + CARD_H / 2.0) as u32,
        );
        let expect_a = (0.85_f64 * 255.0).round() as i16;
        assert!(
            close(body[3], expect_a, 6),
            "卡体 α 应≈0.85×255:{}",
            body[3]
        );
        for ch in 0..3 {
            let expect = f32::from(dark_surface[ch]) * 0.85;
            assert!(
                (f32::from(body[ch]) - expect).abs() <= 8.0,
                "卡体通道 {ch} 应≈surface_2×0.85:{} vs {expect}",
                body[ch]
            );
        }

        // a3. 辉光区(卡外 10px):非零且为 glow_color 系(粉:r 主导)
        let glow = sample(&buf, (PAD - 10.0) as u32, (PAD + CARD_H / 2.0) as u32);
        assert!(glow[3] > 0, "卡外 10px 应有辉光覆盖:α={}", glow[3]);
        assert!(
            glow[0] > glow[1] && glow[0] > glow[2],
            "辉光应为 glow_color(粉)系:r={} g={} b={}",
            glow[0],
            glow[1],
            glow[2]
        );
    }

    #[test]
    fn shimmer_flows_and_reduced_motion_freezes() {
        let style = NeonCardStyle::default();
        let state = settled_state();
        let a = render_neon_card(W, H, &style, &state, &ColorTokens::dark(), 0.0);
        let b = render_neon_card(W, H, &style, &state, &ColorTokens::dark(), 1000.0);
        // b1. t=0 与 t=period/2:同一描边采样点颜色不同(渐变确实流动)
        let pa = sample(&a, (PAD + CARD_W / 2.0) as u32, (PAD + 1.0) as u32);
        let pb = sample(&b, (PAD + CARD_W / 2.0) as u32, (PAD + 1.0) as u32);
        assert_ne!(
            (pa[0], pa[1], pa[2]),
            (pb[0], pb[1], pb[2]),
            "半周期后描边颜色必须不同(shimmer 未流动?)"
        );

        // b2. reduced_motion:两帧逐位相同(直切,相位冻结)
        let mut frozen = settled_state();
        frozen.reduced_motion = true;
        let fa = render_neon_card(W, H, &style, &frozen, &ColorTokens::dark(), 0.0);
        let fb = render_neon_card(W, H, &style, &frozen, &ColorTokens::dark(), 1000.0);
        assert_eq!(fa, fb, "reduced_motion 下 shimmer 两帧必须逐位相同");
    }

    #[test]
    fn cursor_highlight_follows_mouse() {
        let style = NeonCardStyle {
            particles: 0, // 隔离变量:粒子不参与本断言
            ..NeonCardStyle::default()
        };
        let tokens = ColorTokens::dark();

        let mut idle = settled_state();
        let mut hover_tl = settled_state();
        let mut hover_br = settled_state();
        hover_tl.set_hover(true, 0.0);
        hover_tl.set_mouse(point(px(30.0), px(30.0)));
        hover_br.set_hover(true, 0.0);
        hover_br.set_mouse(point(px(210.0), px(120.0)));
        let _ = &mut idle;

        let buf_idle = render_neon_card(W, H, &style, &idle, &tokens, 5000.0);
        let buf_tl = render_neon_card(W, H, &style, &hover_tl, &tokens, 5000.0);
        let buf_br = render_neon_card(W, H, &style, &hover_br, &tokens, 5000.0);

        // c1. 鼠标在左上:左上象限采样点比右下亮(高光跟随光标)
        let tl_point = (PAD + 45.0) as u32;
        let tl = sample(&buf_tl, tl_point, tl_point);
        let br_same = sample(&buf_br, tl_point, tl_point);
        assert!(
            tl[0] + tl[1] + tl[2] > br_same[0] + br_same[1] + br_same[2],
            "鼠标在左上时左上采样应更亮"
        );

        // c2. 鼠标在右下:右下象限采样点比左上亮
        let bx = (PAD + CARD_W - 45.0) as u32;
        let by = (PAD + CARD_H - 45.0) as u32;
        let br = sample(&buf_br, bx, by);
        let tl_same = sample(&buf_tl, bx, by);
        assert!(
            br[0] + br[1] + br[2] > tl_same[0] + tl_same[1] + tl_same[2],
            "鼠标在右下时右下采样应更亮"
        );

        // c3. 非 hover 无高光:hover 后同一像素必须变亮(淡入生效)
        let idle_point = sample(&buf_idle, tl_point, tl_point);
        assert!(
            tl[0] + tl[1] + tl[2] > idle_point[0] + idle_point[1] + idle_point[2],
            "非 hover 帧不应有内高光"
        );
    }

    #[test]
    fn hover_scales_card_and_entrance_fades_in() {
        let style = NeonCardStyle {
            particles: 0,
            ..NeonCardStyle::default()
        };
        let tokens = ColorTokens::dark();

        // d1. hover 缩放:满 hover(进度 1,scale 1.02)时卡体外接尺寸更大
        let idle = settled_state();
        let mut hovered = settled_state();
        hovered.set_hover(true, 0.0);
        let buf_idle = render_neon_card(W, H, &style, &idle, &tokens, 5000.0);
        let buf_hover = render_neon_card(W, H, &style, &hovered, &tokens, 5000.0);
        let right_extent = |buf: &[u8]| -> u32 {
            let y = (PAD + CARD_H / 2.0) as u32;
            (0..W)
                .rev()
                .find(|&x| sample(buf, x, y)[3] > 128)
                .unwrap_or(0)
        };
        let extent_idle = right_extent(&buf_idle);
        let extent_hover = right_extent(&buf_hover);
        assert!(
            extent_hover >= extent_idle + 2,
            "hover 帧卡体外接尺寸应更大(scale 1.02):{extent_hover} vs {extent_idle}"
        );

        // d2. 入场透明度渐进:0ms 全透明 → 50 < 200 < 400ms(落定)
        let entering = settled_state();
        let a0 = render_neon_card(W, H, &style, &entering, &tokens, 0.0);
        let a50 = render_neon_card(W, H, &style, &entering, &tokens, 50.0);
        let a200 = render_neon_card(W, H, &style, &entering, &tokens, 200.0);
        let a400 = render_neon_card(W, H, &style, &entering, &tokens, 400.0);
        let body_alpha = |buf: &[u8]| {
            sample(
                buf,
                (PAD + CARD_W / 2.0) as u32,
                (PAD + CARD_H / 2.0) as u32,
            )[3]
        };
        assert_eq!(body_alpha(&a0), 0, "入场起点应全透明(opacity 0)");
        assert!(
            body_alpha(&a0) < body_alpha(&a50) && body_alpha(&a50) < body_alpha(&a200),
            "入场透明度应随时间上升"
        );
        assert!(
            body_alpha(&a200) < body_alpha(&a400),
            "入场透明度应单调上升到落定值"
        );
        assert!(
            body_alpha(&a400) >= 210,
            "入场落定帧应达玻璃体目标 α(0.85×255≈217):{}",
            body_alpha(&a400)
        );
    }

    #[test]
    fn particles_appear_only_on_hover() {
        let style = NeonCardStyle::default(); // 默认 10 粒
        let tokens = ColorTokens::dark();
        let idle = settled_state();
        let mut hovered = settled_state();
        hovered.set_hover(true, 0.0);
        // t=1500:粒子 0(错峰 0ms)恰在波形峰顶(opacity 1)
        let buf_idle = render_neon_card(W, H, &style, &idle, &tokens, 1500.0);
        let buf_hover = render_neon_card(W, H, &style, &hovered, &tokens, 1500.0);

        // 卡体内部(避开描边)α>230 的像素 = 粒子亮点(玻璃体 α≈217)
        let bright = |buf: &[u8]| -> usize {
            let mut count = 0;
            for y in (PAD + 6.0) as u32..(PAD + CARD_H - 6.0) as u32 {
                for x in (PAD + 6.0) as u32..(PAD + CARD_W - 6.0) as u32 {
                    if sample(buf, x, y)[3] > 230 {
                        count += 1;
                    }
                }
            }
            count
        };
        assert_eq!(bright(&buf_idle), 0, "非 hover 不应出现粒子像素");
        assert!(bright(&buf_hover) > 0, "hover 帧应存在渐变色系粒子亮点");

        // reduced_motion:装饰运动整体直切,hover 也不渲染粒子
        let mut reduced_hover = hovered.clone();
        reduced_hover.reduced_motion = true;
        let buf_reduced = render_neon_card(W, H, &style, &reduced_hover, &tokens, 1500.0);
        assert_eq!(
            bright(&buf_reduced),
            0,
            "reduced_motion 下 hover 也不应有粒子"
        );
    }

    #[test]
    fn light_theme_renders_theme_aware_body_and_ring() {
        // 深浅两主题:语义色随 token,结构(描边/辉光/玻璃 α)不变
        let style = NeonCardStyle {
            particles: 0,
            ..NeonCardStyle::default()
        };
        let state = settled_state();
        let buf_light = render_neon_card(W, H, &style, &state, &ColorTokens::light(), 5000.0);
        let buf_dark = render_neon_card(W, H, &style, &state, &ColorTokens::dark(), 5000.0);
        let center = ((PAD + CARD_W / 2.0) as u32, (PAD + CARD_H / 2.0) as u32);
        let light_body = sample(&buf_light, center.0, center.1);
        let dark_body = sample(&buf_dark, center.0, center.1);
        // 浅色卡体 = 白底(surface_2 white)× 0.85:预乘 RGB 高;深色为暗底:RGB 低
        assert!(
            light_body[0] > 200 && dark_body[0] < 60,
            "卡体应随主题换底色"
        );
        assert!(
            (i16::from(light_body[3]) - i16::from(dark_body[3])).abs() <= 2,
            "玻璃体 α 两主题一致(0.85)"
        );
        // 描边色板为样式载荷:两主题同帧同色
        let border_at = ((PAD + 1.0) as u32, (PAD + CARD_W / 2.0) as u32);
        let lb = sample(&buf_light, border_at.1, border_at.0);
        let db = sample(&buf_dark, border_at.1, border_at.0);
        assert_eq!(lb, db, "霓虹描边是样式载荷,不随主题变");
    }

    #[test]
    fn cache_key_stable_when_idle_and_shifting_when_animating() {
        let style = NeonCardStyle::default();
        let tokens = ColorTokens::dark();
        let idle = settled_state();
        let k1 = frame_cache_key(&style, &idle, &tokens, 1000.0, false);
        let k2 = frame_cache_key(&style, &idle, &tokens, 1600.0, false);
        assert_eq!(k1, k2, "空闲时缓存键必须稳定(零重渲)");

        let mut hovered = idle.clone();
        hovered.set_hover(true, 0.0);
        let h1 = frame_cache_key(&style, &hovered, &tokens, 1000.0, true);
        let h2 = frame_cache_key(&style, &hovered, &tokens, 1032.0, true);
        assert_ne!(h1, h2, "动画中键应随帧桶变化(逐帧重画)");
        let k3 = frame_cache_key(&style, &hovered, &tokens, 1000.0, false);
        assert_ne!(k1, k3, "hover 态与空闲态键必须不同");
    }

    // —— 状态机 ——

    #[test]
    fn hover_progress_monotonic_and_interrupt_continues() {
        let mut state = NeonCardState::new(7);
        state.set_hover(true, 100.0);
        let p_start = state.hover_progress_at(100.0);
        let p_mid = state.hover_progress_at(350.0);
        let p_end = state.hover_progress_at(600.0);
        assert_eq!(p_start, 0.0);
        assert!((0.0..1.0).contains(&p_mid), "中途进度应在开区间:{p_mid}");
        assert_eq!(p_end, 1.0, "500ms 过渡在 600ms 应精确落定");

        // 反向打断:从当前进度接续(不回跳、单调)
        state.set_hover(false, 350.0);
        let q0 = state.hover_progress_at(350.0);
        let q1 = state.hover_progress_at(475.0);
        assert!((q0 - p_mid).abs() < 1e-9, "打断瞬间进度无跳变");
        assert!(q1 < q0, "离场应单调下降:{q1} vs {q0}");
        state.set_hover(true, 475.0);
        let r0 = state.hover_progress_at(475.0);
        let r1 = state.hover_progress_at(600.0);
        assert!((r0 - q1).abs() < 1e-9, "再次打断无跳变");
        assert!(r1 > r0, "再次进场应单调上升:{r1} vs {r0}");
        assert!(r1 < 1.0, "打断重启后 125ms 不应已落定");
    }

    #[test]
    fn reduced_motion_direct_switch_and_tick_settles() {
        let mut state = NeonCardState::new(7);
        state.reduced_motion = true;
        state.set_hover(true, 100.0);
        assert_eq!(state.hover_progress_at(100.0), 1.0, "reduced 直切到终值");
        assert!(!state.is_animating(100.0), "reduced 恒空闲(零帧提交)");
        assert!(!state.tick(100.0));

        // 正常路径:tick 起跑入场 → 400ms 内为真、落定后假;hover 使其持续为真
        let mut state = NeonCardState::new(7);
        assert!(state.tick(0.0), "入场期间 tick 应为真");
        assert!(state.tick(399.0));
        assert!(!state.tick(400.0), "入场 400ms 落定后 tick 应为假");
        state.set_hover(true, 400.0);
        assert!(state.tick(400.0), "hover 期间 shimmer/粒子循环持续吃帧");
        assert!(state.tick(10_000.0), "长时 hover 仍为真(循环动画)");
        state.set_hover(false, 10_000.0);
        assert!(state.tick(10_000.0), "缩放回程 500ms 内仍应汇报动画中");
        assert!(!state.tick(10_600.0), "全部过渡落定后 tick 恒假");
    }

    #[test]
    fn deterministic_seed_renders_identical_particles() {
        let style = NeonCardStyle::default();
        let tokens = ColorTokens::dark();
        let mut a = settled_state();
        let mut b = settled_state();
        a.set_hover(true, 0.0);
        b.set_hover(true, 0.0);
        let buf_a = render_neon_card(W, H, &style, &a, &tokens, 1500.0);
        let buf_b = render_neon_card(W, H, &style, &b, &tokens, 1500.0);
        assert_eq!(buf_a, buf_b, "同种子同时刻必须逐位同帧(确定性)");
        // 不同种子:粒子参数不同(非逐位同帧;整帧相等概率可忽略)
        let other = NeonCardState {
            seed: 99,
            ..a.clone()
        };
        let buf_c = render_neon_card(W, H, &style, &other, &tokens, 1500.0);
        assert_ne!(buf_a, buf_c, "不同种子的粒子帧应不同");
    }
}
