//! 设计 token(分册六 §3.1 色阶系统 + §3.2 数值规范)。
//!
//! **本文件是全 crate 唯一允许出现颜色字面量的地方**(与上游实践对齐:
//! vb_ui 的 theme.rs 同款纪律,见 docs/upstream/02 §3.6)——组件内一切颜色
//! 必须经 [`crate::theme`] 全局态取自这里的 [`ColorTokens`],禁止散落硬编码。
//!
//! 令牌值来源:分册六 §3.1(surface 5 级/边框/文本/accent/功能色,深色值
//! 逐条给出;浅色值为本仓按同一语义补齐的配套方案)。语义结构与
//! docs/upstream/02 §3.1 的 17 令牌表对齐(语义对齐,零源码复制,ACL-1.0 红线)。
//!
//! # 控件高度派生制(AGENTS.md §3.4,docs/upstream/02 §3.4 实证裁决)
//!
//! [`HEIGHT_COMPACT`]/[`HEIGHT_DEFAULT`]/[`HEIGHT_LOOSE`] 只是**下限**:
//! 文字承载控件的实际高度一律 =
//! `max(档位, 文本行高 + 2×垂直 padding)`(见 [`control_height`])——固定档会
//! 随字号/DPI/CJK 字形翻车,必须按内容派生。

use gpui::{Div, Hsla, Styled, div, hsla, rgba};

/// 颜色令牌(分册六 §3.1)。深/浅两套:[`ColorTokens::dark`] / [`ColorTokens::light`]。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorTokens {
    /// 应用底色(最深)
    pub surface_0: Hsla,
    /// 面板底
    pub surface_1: Hsla,
    /// 卡片/输入框
    pub surface_2: Hsla,
    /// hover 态
    pub surface_3: Hsla,
    /// active/pressed 态
    pub surface_4: Hsla,
    /// 分隔线/控件描边(半透)
    pub border_subtle: Hsla,
    /// 焦点环/面板边界(半透,更亮)
    pub border_strong: Hsla,
    /// 主文字
    pub text_primary: Hsla,
    /// 次文字/标签
    pub text_secondary: Hsla,
    /// 禁用/占位
    pub text_disabled: Hsla,
    /// 强调色:选中/聚焦/主按钮
    pub accent: Hsla,
    /// 强调色的低透明版本(选中行底/批量高亮)
    pub accent_muted: Hsla,
    /// 危险(删除/错误)
    pub danger: Hsla,
    /// 警告
    pub warning: Hsla,
    /// 成功
    pub success: Hsla,
}

impl ColorTokens {
    /// 深色主题(分册六 §3.1 的原始值;Illustrator 式深色方案)。
    pub fn dark() -> Self {
        ColorTokens {
            surface_0: rgba(0x0F0F12FF).into(),      // #0f0f12 应用底色
            surface_1: rgba(0x16161AFF).into(),      // #16161a 面板底
            surface_2: rgba(0x1E1E24FF).into(),      // #1e1e24 卡片/输入框
            surface_3: rgba(0x26262EFF).into(),      // #26262e hover 态
            surface_4: rgba(0x2F2F38FF).into(),      // #2f2f38 active/pressed
            border_subtle: rgba(0xFFFFFF0F).into(),  // white 6%
            border_strong: rgba(0xFFFFFF1F).into(),  // white 12%
            text_primary: rgba(0xFFFFFFEB).into(),   // white 92%
            text_secondary: rgba(0xFFFFFF99).into(), // white 60%
            text_disabled: rgba(0xFFFFFF52).into(),  // white 32%
            accent: rgba(0x4F9FFFFF).into(),         // #4f9fff(选中/聚焦/主按钮)
            accent_muted: rgba(0x4F9FFF33).into(),   // accent 20%(选中背景)
            danger: rgba(0xF24822FF).into(),         // #f24822 删除/溢出
            warning: rgba(0xFFC700FF).into(),        // #ffc700 警告
            success: rgba(0x14AE5CFF).into(),        // #14ae5c 成功
        }
    }

    /// 浅色主题(同一语义的配套浅色方案;accent 品牌色跨主题共用——与
    /// docs/upstream/02 §3.1 "主题间共用品牌色" 的决策一致)。
    pub fn light() -> Self {
        ColorTokens {
            surface_0: rgba(0xE9E9EBFF).into(),     // 应用底(工作区周围)
            surface_1: rgba(0xF5F5F5FF).into(),     // 面板底
            surface_2: rgba(0xFFFFFFFF).into(),     // 卡片/输入框(白)
            surface_3: rgba(0xEDEDEDFF).into(),     // hover
            surface_4: rgba(0xDFDFE2FF).into(),     // active/pressed
            border_subtle: rgba(0x00000014).into(), // black 8%
            border_strong: rgba(0x00000029).into(), // black 16%
            text_primary: rgba(0x1E1E1EFF).into(),
            text_secondary: rgba(0x6B6B6BFF).into(),
            text_disabled: rgba(0xA0A0A5FF).into(),
            accent: rgba(0x4F9FFFFF).into(),       // 品牌色共用
            accent_muted: rgba(0x4F9FFF1F).into(), // accent 12%
            danger: rgba(0xD93025FF).into(),
            warning: rgba(0x8F6700FF).into(), // 浅底上加深保证对比度
            success: rgba(0x0E7C42FF).into(),
        }
    }
}

/// 间距令牌:4px 网格(分册六 §3.2;禁止 5/7/10/15/20 等离格值)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpacingTokens;

impl SpacingTokens {
    /// 4px——图标与文字的紧凑间隙
    pub const XS: f32 = 4.0;
    /// 8px——行内元素间距
    pub const SM: f32 = 8.0;
    /// 12px——控件内边距/分组内间距
    pub const MD: f32 = 12.0;
    /// 16px——分组间距
    pub const LG: f32 = 16.0;
    /// 24px——面板内边距
    pub const XL: f32 = 24.0;
    /// 32px——大区块分隔
    pub const XXL: f32 = 32.0;
}

/// 圆角令牌(分册六 §3.2:控件 6 / 卡片 8 / 对话框 12;4 = 小输入框/小按钮)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RadiusTokens;

impl RadiusTokens {
    /// 4px:输入框/小按钮
    pub const SM: f32 = 4.0;
    /// 6px:按钮/Tab/下拉(全局默认控件圆角)
    pub const MD: f32 = 6.0;
    /// 8px:卡片/分组/菜单
    pub const LG: f32 = 8.0;
    /// 12px:对话框/浮层
    pub const XL: f32 = 12.0;
}

/// 单级海拔的阴影参数(迭代计划 08 E4,分册六 §3.2 两档扩为五档)。
///
/// 只描述"黑色环境影":`blur` = 高斯近似的模糊半径(px),`offset_y` =
/// 垂直偏移(光源在正上方,水平偏移恒 0),`alpha` = 阴影不透明度。
/// 颜色恒黑(rgba(0,0,0,alpha)),着色/内阴影等派生效果由消费方组合。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Elevation {
    /// 模糊半径 px(0 = 无模糊,即无影/硬影)
    pub blur: f32,
    /// 垂直偏移 px(向下为正)
    pub offset_y: f32,
    /// 阴影 alpha(0~1)
    pub alpha: f32,
}

/// 海拔阴影 5 级令牌(迭代计划 08 E4;对应 sable-paint `effects::ShadowParams`
/// 的静态预设)。下标即海拔级:e0(无影)→ e4(对话框)。
///
/// | 级 | 用途 | blur | offset_y | alpha | 语义 |
/// |---|---|---|---|---|---|
/// | 0 | 平面元素 | 0 | 0 | 0 | 无影:贴地(工具栏内嵌、画布内图形) |
/// | 1 | 卡片 | 2 | 1 | 0.08 | 微浮:列表卡片,边界可辨但不抢焦点 |
/// | 2 | 浮面板 | 8 | 2 | 0.14 | 悬浮:Dock 面板/悬浮工具条 |
/// | 3 | 下拉 | 16 | 4 | 0.20 | 高浮:下拉菜单/弹层,明显脱离背景 |
/// | 4 | 对话框 | 32 | 8 | 0.32 | 最高:模态对话框,压暗一切下层 |
///
/// blur/alpha 严格单调递增(视觉重量随海拔单调),offset_y 取 blur 的
/// 1/4 档(0/1/2/4/8)——光源接近正上方的设计软件惯例。
pub const ELEVATIONS: [Elevation; 5] = [
    Elevation {
        blur: 0.0,
        offset_y: 0.0,
        alpha: 0.0,
    },
    Elevation {
        blur: 2.0,
        offset_y: 1.0,
        alpha: 0.08,
    },
    Elevation {
        blur: 8.0,
        offset_y: 2.0,
        alpha: 0.14,
    },
    Elevation {
        blur: 16.0,
        offset_y: 4.0,
        alpha: 0.20,
    },
    Elevation {
        blur: 32.0,
        offset_y: 8.0,
        alpha: 0.32,
    },
];

/// 控件高度紧凑档下限(22px,分册六 §3.2)。
pub const HEIGHT_COMPACT: f32 = 22.0;
/// 控件高度默认档下限(26px,分册六 §3.2)。
pub const HEIGHT_DEFAULT: f32 = 26.0;
/// 控件高度宽松档下限(32px,分册六 §3.2)。
pub const HEIGHT_LOOSE: f32 = 32.0;

/// 控件实际高度(派生制,AGENTS.md §3.4):
/// `max(档位下限, 文本行高 + 2×垂直 padding)`。
///
/// 22/26/32 三档是**下限**;文字承载控件必须用本函数按实际行高派生,
/// 固定写死会在 CJK/大字号/DPI 缩放下压字(上游实证)。
pub fn control_height(tier: f32, line_height: f32, v_padding: f32) -> f32 {
    tier.max(line_height + 2.0 * v_padding)
}

/// 面板正文字号(12px,分册六 §3.2 字号表)。
pub const FONT_SIZE_BODY: f32 = 12.0;
/// 面板标题字号(13px)。
pub const FONT_SIZE_HEADING: f32 = 13.0;
/// 注释/时间码字号(11px)。
pub const FONT_SIZE_CAPTION: f32 = 11.0;

/// 水平弹性容器预设(行):flex row + 垂直居中。
///
/// gpui 0.2.2 / gpui-component 0.7.0 均未内置 `h_flex`/`v_flex`(已核实
/// 两份源码),本 crate 自备极薄预设;放在 tokens 模块因为全部组件都依赖
/// theme feature(见 lib.rs 的 mod 门控)。
pub fn h_flex() -> Div {
    div().flex().flex_row().items_center()
}

/// 垂直弹性容器预设(列)。
pub fn v_flex() -> Div {
    div().flex().flex_col()
}

/// 以 0~255 RGBA 直接构造 [`Hsla`](色轮/色井的数学底座;组件渲染侧
/// 仍只许走 theme 语义色,本助手供颜色编辑组件在**用户色**域换算使用)。
pub fn hsla_from_rgba8(c: [u8; 4]) -> Hsla {
    let (h, s, l) = rgb_to_hsl(c[0], c[1], c[2]);
    hsla(h, s, l, f32::from(c[3]) / 255.0)
}

/// [`Hsla`] → 0~255 RGBA(alpha 舍入到字节)。
pub fn rgba8_from_hsla(color: Hsla) -> [u8; 4] {
    let (r, g, b) = hsl_to_rgb(color.h, color.s, color.l);
    let a = (color.a * 255.0).round().clamp(0.0, 255.0) as u8;
    [r, g, b, a]
}

/// sRGB(0~255)→ HSL(h/s/l ∈ [0,1])。
pub(crate) fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let rf = f32::from(r) / 255.0;
    let gf = f32::from(g) / 255.0;
    let bf = f32::from(b) / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f32::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == rf {
        ((gf - bf) / d + if gf < bf { 6.0 } else { 0.0 }) / 6.0
    } else if max == gf {
        ((bf - rf) / d + 2.0) / 6.0
    } else {
        ((rf - gf) / d + 4.0) / 6.0
    };
    (h, s, l)
}

/// HSL(h/s/l ∈ [0,1])→ sRGB(0~255)三元组(不含 alpha)。
pub(crate) fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let (r, g, b) = hsl_to_rgb_f(h, s, l);
    (
        (r * 255.0).round().clamp(0.0, 255.0) as u8,
        (g * 255.0).round().clamp(0.0, 255.0) as u8,
        (b * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

/// HSL → [0,1] RGB(色轮/渐变插值的数学核心,与 [`hsl_to_rgb`] 共享)。
pub(crate) fn hsl_to_rgb_f(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s.abs() < f32::EPSILON {
        return (l, l, l);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    (
        hue_to_rgb_f(p, q, h + 1.0 / 3.0),
        hue_to_rgb_f(p, q, h),
        hue_to_rgb_f(p, q, h - 1.0 / 3.0),
    )
}

fn hue_to_rgb_f(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 0.5 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

/// 两个 0~255 RGBA 颜色的线性插值(渐变预览条手动插值的数学核心)。
pub(crate) fn lerp_rgba8(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| -> u8 {
        (f32::from(x) * (1.0 - t) + f32::from(y) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    [
        mix(a[0], b[0]),
        mix(a[1], b[1]),
        mix(a[2], b[2]),
        mix(a[3], b[3]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_height_is_derived_from_content_not_fixed_tier() {
        // 下限生效:行高小的时候取档位值
        assert_eq!(control_height(HEIGHT_DEFAULT, 12.0, 5.0), HEIGHT_DEFAULT);
        // 内容派生生效:CJK 行高 16 + 2×6 padding = 28 > 26
        assert_eq!(control_height(HEIGHT_DEFAULT, 16.0, 6.0), 28.0);
        // 紧凑档也会被内容顶起来
        assert_eq!(control_height(HEIGHT_COMPACT, 16.0, 6.0), 28.0);
        // 宽松档遇上大内容
        assert_eq!(control_height(HEIGHT_LOOSE, 20.0, 8.0), 36.0);
        // 边界:行高 + padding 恰等于档位
        assert_eq!(control_height(HEIGHT_COMPACT, 12.0, 5.0), HEIGHT_COMPACT);
    }

    #[test]
    fn dark_and_light_token_sets_differ_on_surfaces_and_text() {
        let dark = ColorTokens::dark();
        let light = ColorTokens::light();
        // 表面亮度分层:深色由深到浅,浅色相反
        assert!(dark.surface_0.l < dark.surface_4.l, "深色主题表面由深到浅");
        assert!(
            light.surface_0.l > light.surface_4.l,
            "浅色主题表面由浅到深"
        );
        assert!(dark.surface_0.l < light.surface_0.l);
        // 文本:深色主题用白、浅色主题用黑
        assert!(dark.text_primary.l > 0.5);
        assert!(light.text_primary.l < 0.5);
        // 边框半透
        assert!(dark.border_subtle.a < 0.2 && light.border_subtle.a < 0.2);
        // 品牌色跨主题共用
        assert_eq!(dark.accent, light.accent);
        // 功能色两套各自成值
        assert_ne!(dark.danger, light.danger);
        assert_ne!(dark.success, light.success);
    }

    #[test]
    fn spacing_is_base_4_grid() {
        // 4px 网格门禁(与上游 spacing_scale_is_base_4 测试同语义)
        for v in [
            SpacingTokens::XS,
            SpacingTokens::SM,
            SpacingTokens::MD,
            SpacingTokens::LG,
            SpacingTokens::XL,
            SpacingTokens::XXL,
        ] {
            assert_eq!(v % 4.0, 0.0, "间距 {v} 不在 4px 网格上");
        }
        // 档位严格递增:经运行时函数取值再比较(两侧都是编译期常量的
        // 直接断言会被判为常量断言;运行时取值保持同一测试意图)
        let scale = spacing_scale();
        for pair in scale.windows(2) {
            assert!(
                pair[0] < pair[1],
                "间距档位应严格递增:{} < {}",
                pair[0],
                pair[1]
            );
        }
    }

    /// 间距档位表运行时取值(测试用):从 token 常量装配为运行时序列,
    /// 供递增断言在运行期复核(而非编译期折叠)。
    fn spacing_scale() -> Vec<f32> {
        vec![
            SpacingTokens::XS,
            SpacingTokens::SM,
            SpacingTokens::MD,
            SpacingTokens::LG,
            SpacingTokens::XL,
            SpacingTokens::XXL,
        ]
    }

    #[test]
    fn radius_tiers_match_spec() {
        assert_eq!(
            (
                RadiusTokens::SM,
                RadiusTokens::MD,
                RadiusTokens::LG,
                RadiusTokens::XL
            ),
            (4.0, 6.0, 8.0, 12.0),
            "圆角档 = 分册六 §3.2 的 4/6/8/12"
        );
    }

    #[test]
    fn elevations_are_monotonic_and_e0_has_no_shadow() {
        // e0 = 无影(三参数全零)
        assert_eq!(
            (
                ELEVATIONS[0].blur,
                ELEVATIONS[0].offset_y,
                ELEVATIONS[0].alpha
            ),
            (0.0, 0.0, 0.0),
            "e0 必须无影"
        );
        // 视觉重量随海拔单调:blur / offset_y / alpha 逐级不降且至少一项严格增
        for pair in ELEVATIONS.windows(2) {
            assert!(pair[0].blur <= pair[1].blur, "blur 应单调不降");
            assert!(pair[0].offset_y <= pair[1].offset_y, "offset_y 应单调不降");
            assert!(pair[0].alpha <= pair[1].alpha, "alpha 应单调不降");
            assert!(
                pair[0].blur < pair[1].blur
                    || pair[0].offset_y < pair[1].offset_y
                    || pair[0].alpha < pair[1].alpha,
                "相邻海拔必须有一项严格递增,否则两级无区分度"
            );
        }
        // 验收表数值:blur 0/2/8/16/32,alpha 0/0.08/0.14/0.20/0.32
        let blurs: Vec<f32> = ELEVATIONS.iter().map(|e| e.blur).collect();
        assert_eq!(blurs, vec![0.0, 2.0, 8.0, 16.0, 32.0]);
        let alphas: Vec<f32> = ELEVATIONS.iter().map(|e| e.alpha).collect();
        assert_eq!(alphas, vec![0.0, 0.08, 0.14, 0.20, 0.32]);
    }

    #[test]
    fn hsl_conversions_round_trip() {
        // 灰色:色相/饱和度为 0,亮度 = 分量/255
        let (h, s, l) = rgb_to_hsl(128, 128, 128);
        assert_eq!((h, s), (0.0, 0.0));
        assert!((l - 128.0 / 255.0).abs() < 1e-6);
        // 主色往返(容差 1/255:舍入边界)
        for (r, g, b) in [
            (255u8, 0u8, 0u8),
            (0, 255, 0),
            (0, 0, 255),
            (79, 159, 255),
            (30, 30, 36),
        ] {
            let (h, s, l) = rgb_to_hsl(r, g, b);
            let (r2, g2, b2) = hsl_to_rgb(h, s, l);
            assert!((i16::from(r) - i16::from(r2)).abs() <= 1, "r {r} vs {r2}");
            assert!((i16::from(g) - i16::from(g2)).abs() <= 1, "g {g} vs {g2}");
            assert!((i16::from(b) - i16::from(b2)).abs() <= 1, "b {b} vs {b2}");
        }
    }

    #[test]
    fn rgba8_hsla_round_trip_keeps_channels() {
        let color = [79u8, 159, 255, 200];
        let back = rgba8_from_hsla(hsla_from_rgba8(color));
        for ch in 0..4 {
            assert!(
                (i16::from(color[ch]) - i16::from(back[ch])).abs() <= 1,
                "通道 {ch}:{} vs {}",
                color[ch],
                back[ch]
            );
        }
    }

    #[test]
    fn lerp_rgba8_interpolates_linearly() {
        assert_eq!(
            lerp_rgba8([0, 0, 0, 0], [255, 255, 255, 255], 0.0),
            [0, 0, 0, 0]
        );
        assert_eq!(
            lerp_rgba8([0, 0, 0, 0], [255, 255, 255, 255], 1.0),
            [255, 255, 255, 255]
        );
        assert_eq!(
            lerp_rgba8([0, 0, 0, 0], [100, 200, 40, 255], 0.5),
            [50, 100, 20, 128]
        );
        // t 越界被钳制
        assert_eq!(
            lerp_rgba8([0, 0, 0, 0], [255, 255, 255, 255], 2.0),
            [255, 255, 255, 255]
        );
    }
}
