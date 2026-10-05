//! 骨架屏(迭代审查报告 2026-10-04 §5.6 组件矩阵 #15 / §5.9 状态设计表,
//! CMP-05):面板轮廓块 + shimmer 1.2s 呼吸。
//!
//! ```ignore
//! // 打开工程的首屏占位(§5.9 加载态:骨架屏 shimmer 1.2s):
//! Skeleton::new(SkeletonKind::Card)
//! Skeleton::new(SkeletonKind::Row).width(200.0)
//! ```
//!
//! # 三种预设(§5.6 #15:行/块/卡)
//!
//! [`SkeletonKind::Row`](文本行占位)/[`Block`](缩略块)/[`Card`](面板轮廓
//! = 卡片骨架),几何单点在 [`skeleton_geometry`](纯函数,TC 断言面):
//! 宽/高落在 4px 间距网格、圆角取 [`crate::tokens::RadiusTokens`] 档位,
//! 不另立魔法数。
//!
//! # Shimmer(灰阶令牌呼吸)
//!
//! 呼吸 = 表面令牌 **surface_2 ↔ surface_3** 之间的余弦插值
//! ([`shimmer_progress`],1.2s 周期 [`SHIMMER_PERIOD_MS`],周期取舍与
//! `controls::spinner::SPIN_PERIOD_MS` 同理由,见该模块 doc)——不引白色
//! 叠加、不调 alpha,**只呼吸灰阶令牌**,深浅两主题自动成立且零硬编码色。
//!
//! # 减弱动态(A8)
//!
//! `reduced_motion` 为真:恒 [`shimmer_surface`] 的静态灰(surface_2,
//! 与相位无关),零帧提交;渲染层据此停请求动画帧。

use gpui::{App, IntoElement, RenderOnce, Styled, Window, div, px};

use crate::anim::{lerp_hsla, reduced_motion};
use crate::interact::{self, Semantic, SemanticRole, semantic_slot};
use crate::theme::theme;
use crate::tokens::{ColorTokens, RadiusTokens, SpacingTokens};

// ---------------------------------------------------------------------------
// 周期与预设几何(具名单点;颜色一律令牌)
// ---------------------------------------------------------------------------

/// Shimmer 呼吸周期(毫秒)。报告 §5.6 #15 / §5.9 明写 1.2s;循环周期与
/// [`crate::tokens::MotionTokens`] 过渡时长不同类,取舍说明见
/// `controls::spinner` 模块 doc("周期常量的取舍"),tokens 扩表后重指向。
pub const SHIMMER_PERIOD_MS: f64 = 1200.0;

/// 骨架块几何(纯数据;[`skeleton_geometry`] 的输出)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkeletonGeom {
    /// 宽 px
    pub width: f32,
    /// 高 px
    pub height: f32,
    /// 圆角 px(RadiusTokens 档)
    pub radius: f32,
}

/// 骨架块预设(§5.6 #15:行/块/卡)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SkeletonKind {
    /// 文本行占位(120×16,圆角 SM——行高取 LABEL 行高附近,灰条可辨)
    Row,
    /// 缩略块占位(64×64,圆角 MD——图层缩略图/媒体网格)
    Block,
    /// 面板轮廓占位(200×120,圆角 LG——打开工程的首屏卡片骨架)
    Card,
}

/// 预设几何(纯函数,TC 断言面):行 120×16 r4 / 块 64×64 r6 / 卡
/// 200×120 r8;全部落在 4px 网格,圆角 = [`RadiusTokens`] 档位。
#[must_use]
pub fn skeleton_geometry(kind: SkeletonKind) -> SkeletonGeom {
    match kind {
        SkeletonKind::Row => SkeletonGeom {
            width: 120.0,
            height: 16.0,
            radius: RadiusTokens::SM,
        },
        SkeletonKind::Block => SkeletonGeom {
            width: 64.0,
            height: 64.0,
            radius: RadiusTokens::MD,
        },
        SkeletonKind::Card => SkeletonGeom {
            width: 200.0,
            height: 120.0,
            radius: RadiusTokens::LG,
        },
    }
}

// ---------------------------------------------------------------------------
// Shimmer 纯函数(相位 → 呼吸 → 表面色;TC 的被测单点)
// ---------------------------------------------------------------------------

/// 呼吸进度(纯函数):相位 0..1 → 0..1..0 的余弦呼吸(`0.5 - 0.5·cos(2πφ)`
///;无三角函数表依赖,f64 求值确定)。相位越界有 `cos` 的天然周期性
/// (周期 = 1),任意有限相位都有定义。
#[must_use]
pub fn shimmer_progress(phase: f64) -> f64 {
    if !phase.is_finite() {
        return 0.0;
    }
    0.5 - 0.5 * (std::f64::consts::TAU * phase).cos()
}

/// 骨架表面色(纯函数):`reduced` = 恒静态灰 surface_2(与相位无关);
/// 否则 surface_2 ↔ surface_3 灰阶令牌呼吸(零白色叠加、零 alpha 调制)。
#[must_use]
pub fn shimmer_surface(phase: f64, colors: &ColorTokens, reduced: bool) -> gpui::Hsla {
    if reduced {
        return colors.surface_2;
    }
    lerp_hsla(colors.surface_2, colors.surface_3, shimmer_progress(phase))
}

/// 相位求值(渲染期入口;时间注入边界):`now_ms` 折进呼吸周期
///(`rem_euclid`,负时刻自然回绕;非有限输入落 0)。与
/// `controls::spinner::cycle_phase` 同语义——两处各自单点,因为本轮只许写
/// 状态件四文件,公共化留待批次收口(宿主无感知)。
#[must_use]
fn cycle(now_ms: f64) -> f64 {
    if !now_ms.is_finite() {
        return 0.0;
    }
    now_ms.rem_euclid(SHIMMER_PERIOD_MS) / SHIMMER_PERIOD_MS
}

// ---------------------------------------------------------------------------
// Skeleton(RenderOnce:呼吸是 now 的纯函数,无跨帧状态)
// ---------------------------------------------------------------------------

/// 骨架占位块(§5.6 #15;逐帧重建场景安全——RenderOnce 无跨帧状态)。
///
/// 呼吸进行中每帧续帧;`reduced_motion` 下静态灰、零帧提交(A8)。
#[derive(gpui::IntoElement)]
pub struct Skeleton {
    kind: SkeletonKind,
    width_override: Option<f32>,
    /// A11Y-02 语义槽(缺省 role = Decoration:轮廓占位,读屏应跳过)
    semantic: Semantic,
}

impl Skeleton {
    /// 按预设创建骨架块。
    pub fn new(kind: SkeletonKind) -> Self {
        Skeleton {
            kind,
            width_override: None,
            semantic: Semantic::new(),
        }
    }

    /// 宽度覆盖(px;行宽随容器时用)。高度/圆角仍随预设。
    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.width_override = Some(width.max(SpacingTokens::XS));
        self
    }

    /// 解析语义(A11Y-02):显式 `.label(...)` 优先;role 默认
    /// [`SemanticRole::Decoration`](骨架是加载占位,读屏应跳过——
    /// 加载完成由内容本身承载语义)。
    #[must_use]
    pub fn resolved_semantic(&self) -> Semantic {
        let mut sem = Semantic::new();
        if let Some(label) = self.semantic.label() {
            sem = sem.with_label(label.clone());
        }
        sem.with_role(self.semantic.role().unwrap_or(SemanticRole::Decoration))
    }
}

// A11Y-02 语义槽(label/role/semantic 三件)
semantic_slot!(Skeleton);

impl RenderOnce for Skeleton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = theme(cx).colors;
        let reduced = reduced_motion();
        let phase = cycle(interact::now_ms());
        let surface = shimmer_surface(phase, &colors, reduced);
        if !reduced {
            // 呼吸进行中续帧;reduced 静态零帧提交(A8)
            window.request_animation_frame();
        }
        let geom = skeleton_geometry(self.kind);
        let width = self.width_override.unwrap_or(geom.width);
        let block = div()
            .w(px(width))
            .h(px(geom.height))
            .rounded(px(geom.radius))
            .bg(surface);
        interact::attach_semantics(block, &self.resolved_semantic())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ColorTokens;

    /// TC(shimmer 相位):余弦呼吸两端精确、单调、周期回卷;非法相位防御。
    #[test]
    fn tc_cmp_state_skeleton_shimmer_phase_breathes_and_wraps() {
        assert_eq!(SHIMMER_PERIOD_MS, 1200.0, "报告 §5.6 #15:shimmer 1.2s");
        // 两端:0 → 暗(0)、半程 → 亮(1)、整周期 → 暗(回卷)
        assert!(shimmer_progress(0.0).abs() < 1e-9);
        assert!((shimmer_progress(0.5) - 1.0).abs() < 1e-9, "半程最亮");
        assert!(shimmer_progress(1.0).abs() < 1e-9, "整周期回卷");
        // 前半程单调上行、后半程对称回落
        let mut prev = -1.0;
        for i in 0..=10 {
            let p = shimmer_progress(f64::from(i) / 20.0);
            assert!(p > prev, "前半程单调变亮:{p} ≤ {prev}");
            prev = p;
        }
        for i in 0..=10 {
            let rise = shimmer_progress(f64::from(i) / 20.0);
            let fall = shimmer_progress(1.0 - f64::from(i) / 20.0);
            assert!((rise - fall).abs() < 1e-9, "呼吸对称:{rise} vs {fall}");
        }
        // 周期确定性:2.5 周期后与半程一致(长任务不漂移)
        assert!((shimmer_progress(2.5) - shimmer_progress(0.5)).abs() < 1e-9);
        // 防御:NaN → 0
        assert_eq!(shimmer_progress(f64::NAN), 0.0);
        // cycle 求值:负时刻经 rem_euclid 自然回绕(循环对纪元不敏感);
        // 回绕点与正向同相位一致
        assert!(
            (cycle(-1.0) - cycle(1199.0)).abs() < 1e-9,
            "负时刻回卷到周期尾部"
        );
        assert!((cycle(600.0) - 0.5).abs() < 1e-9);
        assert!((cycle(0.0) - 0.0).abs() < 1e-9);
    }

    /// TC(reduced 静态断言):任意相位恒静态灰 surface_2;正常路径两相位
    /// 可辨且都落在两枚灰阶令牌之间(零硬编码色)。
    #[test]
    fn tc_cmp_state_skeleton_reduced_is_static_gray() {
        for colors in [ColorTokens::dark(), ColorTokens::light()] {
            // reduced:与相位无关,恒 surface_2
            for phase in [0.0, 0.25, 0.5, 0.75, 1.0, 7.25] {
                assert_eq!(
                    shimmer_surface(phase, &colors, true),
                    colors.surface_2,
                    "reduced:静态灰,相位 {phase}"
                );
            }
            // 正常:暗相位 = surface_2、亮相位 = surface_3(端点即令牌)
            assert_eq!(shimmer_surface(0.0, &colors, false), colors.surface_2);
            assert_eq!(shimmer_surface(0.5, &colors, false), colors.surface_3);
            // 中途相位落在两枚灰阶令牌之间(呼吸不越过令牌域)
            let mid = shimmer_surface(0.25, &colors, false);
            let (lo, hi) = if colors.surface_2.l <= colors.surface_3.l {
                (colors.surface_2.l, colors.surface_3.l)
            } else {
                (colors.surface_3.l, colors.surface_2.l)
            };
            assert!(
                mid.l >= lo - 1e-6 && mid.l <= hi + 1e-6,
                "呼吸值越出灰阶令牌域:{mid:?}"
            );
            // 亮相位可辨于暗相位(呼吸确实可见)
            assert_ne!(
                shimmer_surface(0.0, &colors, false),
                shimmer_surface(0.5, &colors, false)
            );
        }
    }

    /// TC(三种预设几何):行/块/卡尺寸落 4px 网格、圆角取 RadiusTokens 档、
    /// builder 覆盖宽度仍守下限;语义槽缺省 Decoration、显式覆盖优先。
    #[test]
    fn tc_cmp_state_skeleton_geometry_presets_and_semantic() {
        // 逐预设值(§5.6 #15 的行/块/卡)
        let row = skeleton_geometry(SkeletonKind::Row);
        assert_eq!(
            (row.width, row.height, row.radius),
            (120.0, 16.0, RadiusTokens::SM)
        );
        let block = skeleton_geometry(SkeletonKind::Block);
        assert_eq!(
            (block.width, block.height, block.radius),
            (64.0, 64.0, RadiusTokens::MD)
        );
        let card = skeleton_geometry(SkeletonKind::Card);
        assert_eq!(
            (card.width, card.height, card.radius),
            (200.0, 120.0, RadiusTokens::LG)
        );
        // 4px 网格 + 圆角档位合法性
        for kind in [SkeletonKind::Row, SkeletonKind::Block, SkeletonKind::Card] {
            let g = skeleton_geometry(kind);
            assert_eq!(g.width % 4.0, 0.0, "{kind:?} 宽不在 4 网格");
            assert_eq!(g.height % 4.0, 0.0, "{kind:?} 高不在 4 网格");
            assert!(
                [
                    RadiusTokens::SM,
                    RadiusTokens::MD,
                    RadiusTokens::LG,
                    RadiusTokens::XL
                ]
                .contains(&g.radius),
                "{kind:?} 圆角必须取档位令牌"
            );
        }
        // builder:宽度覆盖守下限;语义槽
        let sk = Skeleton::new(SkeletonKind::Row);
        assert_eq!(sk.width_override, None);
        assert_eq!(
            Skeleton::new(SkeletonKind::Row).width(2.0).width_override,
            Some(SpacingTokens::XS),
            "下限 4px"
        );
        assert_eq!(
            Skeleton::new(SkeletonKind::Row).width(200.0).width_override,
            Some(200.0)
        );
        assert_eq!(
            sk.resolved_semantic().role(),
            Some(SemanticRole::Decoration),
            "骨架缺省 role = Decoration(读屏跳过)"
        );
        let named = Skeleton::new(SkeletonKind::Card)
            .label("正在打开工程")
            .role(SemanticRole::Group);
        assert_eq!(
            named.resolved_semantic().label().map(|s| s.as_ref()),
            Some("正在打开工程")
        );
        assert_eq!(named.resolved_semantic().role(), Some(SemanticRole::Group));
    }
}
