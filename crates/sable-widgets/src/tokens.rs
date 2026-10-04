//! 设计 token(分册六 §3.1 色阶系统 + §3.2 数值规范)。
//!
//! **本文件是全 crate 唯一允许出现颜色字面量的地方**(与上游实践对齐:
//! vb_ui 的 theme.rs 同款纪律,见 docs/upstream/02 §3.6)——组件内一切颜色
//! 必须经 [`crate::theme`] 全局态取自这里的 [`ColorTokens`],禁止散落硬编码。
//!
//! 令牌值来源:分册六 §3.1 + 迭代审查报告 2026-10-04 §5.3(中性色阶 N0~N12、
//! 状态层 alpha、海拔阴影)。语义结构与 docs/upstream/02 §3.1 的令牌表对齐
//! (语义对齐,零源码复制,ACL-1.0 红线)。
//!
//! # JSON 真相源与逐值同步门禁(TOK-08,迭代审查报告 §5.3 原则)
//!
//! 本文件的 color/spacing/radius/elevation/state-layer/motion 六表与
//! `docs/design/sable-tokens.json`(W3C design-tokens 格式)逐值同步;两侧
//! 任一侧改值不同步,`crates/sable-widgets/tests/gate_tokens_sync.rs` 即红。
//! **改动任何令牌值必须同轮改 JSON**(反之亦然)。字号/字体表(TOK-02)属
//! 下一批,JSON 刻意不含,不得提前造假。
//!
//! # 控件高度派生制(AGENTS.md §3.4,docs/upstream/02 §3.4 实证裁决)
//!
//! [`HEIGHT_COMPACT`]/[`HEIGHT_DEFAULT`]/[`HEIGHT_LOOSE`] 只是**下限**:
//! 文字承载控件的实际高度一律 =
//! `max(档位, 文本行高 + 2×垂直 padding)`(见 [`control_height`])——固定档会
//! 随字号/DPI/CJK 字形翻车,必须按内容派生。

use gpui::{Div, Hsla, Styled, div, hsla, rgba};

/// 颜色令牌(分册六 §3.1 + 报告 §5.3.1 中性色阶)。深/浅两套:
/// [`ColorTokens::dark`] / [`ColorTokens::light`]。
///
/// 表面 5 级落在深色中性阶梯 N0~N12 的机身上(dark:surface_0=N0 画布外 →
/// surface_2=N5 凸起/输入 → surface_4=N7 按下);文字 6 档
/// (strong/primary/secondary/tertiary/disabled/placeholder)对比度严格
/// 递减。存量字段名一律保留(旧名重指向新阶梯值,宿主零改动)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorTokens {
    /// 应用底色(最深;深色=N0 #0E0E10 画布外)
    pub surface_0: Hsla,
    /// 面板底(深色=N2 #1C1C1E)
    pub surface_1: Hsla,
    /// 卡片/输入框——凸起材质(深色=N5 #313136,浅色=#F1F1F3 独立调校)
    pub surface_2: Hsla,
    /// hover 态(深色=N6 #3A3A40)
    pub surface_3: Hsla,
    /// active/pressed 态(深色=N7 #45454C)
    pub surface_4: Hsla,
    /// 分隔线/控件描边(半透)
    pub border_subtle: Hsla,
    /// 焦点环/面板边界(半透,更亮)
    pub border_strong: Hsla,
    /// 强调文字(标题/数值,不透明最高档;深色=N12 纯白)
    pub text_strong: Hsla,
    /// 主文字(正文默认档)
    pub text_primary: Hsla,
    /// 次文字/标签(深色=N10 #9A9AA4)
    pub text_secondary: Hsla,
    /// 三级弱文字(时间码/辅助说明;深色=N9 #6E6E78)
    pub text_tertiary: Hsla,
    /// 禁用文字
    pub text_disabled: Hsla,
    /// 输入占位文字(最弱档,弱于 disabled)
    pub text_placeholder: Hsla,
    /// 强调色:选中/聚焦/主按钮
    pub accent: Hsla,
    /// 强调色的低透明版本(选中行底/批量高亮;历史字段,新代码选中底请走
    /// [`crate::interact::state_layer`](TOK-04,深浅同比例 accent alpha))
    pub accent_muted: Hsla,
    /// 危险(删除/错误)
    pub danger: Hsla,
    /// 警告
    pub warning: Hsla,
    /// 成功
    pub success: Hsla,
}

impl ColorTokens {
    /// 深色主题(分册六 §3.1 原值 + 报告 §5.3.1 中性阶梯重指向;Illustrator
    /// 式深色方案)。
    pub fn dark() -> Self {
        ColorTokens {
            surface_0: rgba(0x0E0E10FF).into(),     // N0 #0e0e10 画布外/应用底
            surface_1: rgba(0x1C1C1EFF).into(),     // N2 #1c1c1e 面板底
            surface_2: rgba(0x313136FF).into(),     // N5 #313136 凸起/输入(卡片)
            surface_3: rgba(0x3A3A40FF).into(),     // N6 #3a3a40 hover
            surface_4: rgba(0x45454CFF).into(),     // N7 #45454c pressed
            border_subtle: rgba(0xFFFFFF0F).into(), // white 6%
            border_strong: rgba(0xFFFFFF1F).into(), // white 12%
            text_strong: rgba(0xFFFFFFFF).into(),   // N12 #ffffff 标题/数值
            text_primary: rgba(0xFFFFFFEB).into(),  // white 92% 正文默认
            text_secondary: rgba(0x9A9AA4FF).into(), // N10 #9a9aa4 次文字
            text_tertiary: rgba(0x6E6E78FF).into(), // N9 #6e6e78 弱文字
            text_disabled: rgba(0xFFFFFF52).into(), // white 32%
            text_placeholder: rgba(0xFFFFFF47).into(), // white 28% 占位(最弱)
            accent: rgba(0x4F9FFFFF).into(),        // #4f9fff(选中/聚焦/主按钮)
            accent_muted: rgba(0x4F9FFF33).into(),  // accent 20%(历史选中背景)
            danger: rgba(0xF24822FF).into(),        // #f24822 删除/溢出
            warning: rgba(0xFFC700FF).into(),       // #ffc700 警告
            success: rgba(0x14AE5CFF).into(),       // #14ae5c 成功
        }
    }

    /// 浅色主题(报告 §5.3.1:反向**独立调校**,非深色反相;accent 品牌色
    /// 跨主题共用——与 docs/upstream/02 §3.1 决策一致)。
    pub fn light() -> Self {
        ColorTokens {
            surface_0: rgba(0xF7F7F8FF).into(),     // 应用底(工作区周围)
            surface_1: rgba(0xFFFFFFFF).into(),     // 面板底(白)
            surface_2: rgba(0xF1F1F3FF).into(),     // 卡片/输入(独立调校浅灰)
            surface_3: rgba(0xE8E8EBFF).into(),     // hover
            surface_4: rgba(0xDDDDE1FF).into(),     // active/pressed
            border_subtle: rgba(0x00000014).into(), // black 8%
            border_strong: rgba(0x00000029).into(), // black 16%
            text_strong: rgba(0x000000FF).into(),   // 纯黑 标题/数值
            text_primary: rgba(0x1B1B1FFF).into(),  // #1b1b1f 正文默认
            text_secondary: rgba(0x6B6B6BFF).into(),
            text_tertiary: rgba(0x8A8A90FF).into(),
            text_disabled: rgba(0xA0A0A5FF).into(),
            text_placeholder: rgba(0xACACB2FF).into(), // 比禁用更弱(更浅)
            accent: rgba(0x4F9FFFFF).into(),           // 品牌色共用
            accent_muted: rgba(0x4F9FFF1F).into(),     // accent 12%(历史)
            danger: rgba(0xD93025FF).into(),
            warning: rgba(0x8F6700FF).into(), // 浅底上加深保证对比度
            success: rgba(0x0E7C42FF).into(),
        }
    }
}

/// 状态层 alpha 令牌(报告 §5.3.3,TOK-04):hover/press/selected/focus
/// 环/禁用的统一叠加强度,深/浅各一套(浅色 hover 用黑色叠加,修亮度法
/// 钳 1.0 失效)。消费入口:[`crate::interact::state_layer`]。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StateLayerTokens {
    /// 悬停叠加 alpha(深=白 6%,浅=黑 4%)
    pub hover: f32,
    /// 按下叠加 alpha(深=白 10%,浅=黑 8%)
    pub press: f32,
    /// 选中底:accent 叠加 alpha(深浅同比例 14%,TC-TOK-STATE-02)
    pub selected: f32,
    /// 焦点环:accent 不透明度(1.0 = accent 实色;绘制接线 = A11Y 批次)
    pub focus_ring: f32,
    /// 禁用:容器不变(0 = 底色不动,仅前景降级,§5.3.3/TOK-07)
    pub disabled: f32,
}

impl StateLayerTokens {
    /// 深色表面状态层(白色系叠加)。
    pub const fn dark() -> Self {
        StateLayerTokens {
            hover: 0.06,
            press: 0.10,
            selected: 0.14,
            focus_ring: 1.0,
            disabled: 0.0,
        }
    }

    /// 浅色表面状态层(黑色系叠加,独立调校)。
    pub const fn light() -> Self {
        StateLayerTokens {
            hover: 0.04,
            press: 0.08,
            selected: 0.14,
            focus_ring: 1.0,
            disabled: 0.0,
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
/// 的静态预设;报告 §5.3.2 L0~L4 材质阶梯与之逐级对齐)。
///
/// | 级 | 用途 | blur | offset_y | alpha | 语义 |
/// |---|---|---|---|---|---|
/// | 0 | 平面元素 | 0 | 0 | 0 | 无影:贴地(L0 画布) |
/// | 1 | 卡片 | 2 | 1 | 0.08 | 微浮:列表卡片,边界可辨但不抢焦点(L1 面板) |
/// | 2 | 浮面板 | 8 | 2 | 0.14 | 悬浮:Dock 面板/输入凸起(L2) |
/// | 3 | 下拉 | 16 | 4 | 0.20 | 高浮:下拉菜单/弹层/NeonCard 辉光底(L3 浮层) |
/// | 4 | 对话框 | 32 | 8 | 0.32 | 最高:模态对话框,压暗一切下层(L4) |
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

/// 海拔阴影基色:恒黑(TOK-01;唯一颜色字面量点纪律下的单一出处,各级
/// alpha 由 [`ELEVATIONS`] 与 [`SHADOW_LAYER_WEIGHTS`] 派生)。
pub const ELEVATION_SHADOW_TINT: Hsla = Hsla {
    h: 0.0,
    s: 0.0,
    l: 0.0,
    a: 1.0,
};

/// quad 近似阴影的分层权重(内层→外层;GPUI 0.2.2 无 box-shadow,
/// [`crate::theme::shadow_quads`] 用 3 层扩张矩形逼近高斯衰减)。
/// 和 ≤ 1.0:叠加后总不透明度不超过 [`Elevation::alpha`]。
pub const SHADOW_LAYER_WEIGHTS: [f32; 3] = [0.50, 0.30, 0.20];

/// 海拔 → quad 近似阴影分层参数 `[(外扩 px, alpha); 3]`(纯函数,TC-TOK-
/// ELEV-01 的可断言 spec;内→外 spread 递增、alpha 递减,几何 =
/// `spread_i = blur × 0.25 × i`,`alpha_i = alpha × 权重_i`)。
///
/// `level` 越界钳到最后一级(与 [`crate::theme::shadow`] 同策略,不 panic)。
#[must_use]
pub fn shadow_layer_params(level: usize) -> [(f32, f32); 3] {
    let e = ELEVATIONS[level.min(ELEVATIONS.len() - 1)];
    let step = e.blur * 0.25;
    [
        (step, e.alpha * SHADOW_LAYER_WEIGHTS[0]),
        (step * 2.0, e.alpha * SHADOW_LAYER_WEIGHTS[1]),
        (step * 3.0, e.alpha * SHADOW_LAYER_WEIGHTS[2]),
    ]
}

/// 动效时长四档(报告 §5.8/ANI-07):INSTANT(画布几何直切)/ HOVER(悬停
/// 微过渡)/ STATE(状态切换)/ PANEL(面板折叠、主题过渡)。单一真相,
/// `interact::DUR_INTERACT_MS`/`DUR_PANEL_MS` 等重指向此处。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionTokens;

impl MotionTokens {
    /// 0ms:几何直切(缩放/平移跟手,不做过渡)
    pub const DUR_INSTANT_MS: f64 = 0.0;
    /// 80ms:悬停微过渡(tooltip 淡入等,组件接线随 CMP 批次)
    pub const DUR_HOVER_MS: f64 = 80.0;
    /// 120ms:状态切换(hover/press/selected 三态、勾选)
    pub const DUR_STATE_MS: f64 = 120.0;
    /// 200ms:面板级(折叠/展开、主题过渡)
    pub const DUR_PANEL_MS: f64 = 200.0;
}

/// 弹簧预设参数(ANI-06:stiffness/damping/mass 单点;[`crate::anim::Spring`]
/// 的档位常量重指向此处,消除两处真相)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringPreset {
    /// 刚度 k
    pub stiffness: f64,
    /// 阻尼 c
    pub damping: f64,
    /// 质量 m
    pub mass: f64,
}

/// 弹簧·利落档(§5.8:按下回弹/手柄吸附,近乎临界几乎无过冲)。
pub const SPRING_SNAPPY: SpringPreset = SpringPreset {
    stiffness: 400.0,
    damping: 28.0,
    mass: 1.0,
};

/// 弹簧·柔和档(§5.8:面板拖拽跟手,轻微过冲的柔顺跟随)。
pub const SPRING_SOFT: SpringPreset = SpringPreset {
    stiffness: 180.0,
    damping: 22.0,
    mass: 1.0,
};

/// 弹簧·回弹档(历史档位,弹窗可见过冲;gesture fling 接续在用,值保持
/// 单点源自本文件,深浅主题与宿主不得另立)。
pub const SPRING_BOUNCY: SpringPreset = SpringPreset {
    stiffness: 300.0,
    damping: 15.0,
    mass: 1.0,
};

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

    /// TC-TOK-NEUTRAL-01(报告 §5.3.1/TOK-05):深色表面 5 级严格变亮;
    /// 浅色为独立调校(应用底比面板底深,面板→按下严格变深);文字 6 档
    /// 对比度严格有序(strong > primary > secondary > tertiary > disabled >
    /// placeholder),两主题各自成值。
    #[test]
    fn tc_tok_neutral_01_text_tiers_strictly_ordered_in_both_themes() {
        for tokens in [ColorTokens::dark(), ColorTokens::light()] {
            let dark = tokens.text_primary.l > 0.5;
            let name = if dark { "dark" } else { "light" };
            // 表面 5 级:深色严格递增;浅色 s0(应用底)< s1(白面板)且
            // s1→s4 严格变深(独立调校,非全局单调,更非深色反相)
            let surfaces = [
                tokens.surface_0.l,
                tokens.surface_1.l,
                tokens.surface_2.l,
                tokens.surface_3.l,
                tokens.surface_4.l,
            ];
            if dark {
                for pair in surfaces.windows(2) {
                    assert!(pair[0] < pair[1], "{name} 表面应严格变亮:{surfaces:?}");
                }
            } else {
                assert!(surfaces[0] < surfaces[1], "浅色应用底应深于白面板");
                for pair in surfaces[1..].windows(2) {
                    assert!(pair[0] > pair[1], "{name} 面板→按下应严格变深:{surfaces:?}");
                }
            }
            // 文字 6 档:在面板底(surface_1)上混色后的有效亮度严格有序
            // (深色亮 = 强,浅色暗 = 强;纯灰阶近似 eff = a·l + (1-a)·l_panel)
            let panel = tokens.surface_1.l;
            let eff = |t: Hsla| t.a * t.l + (1.0 - t.a) * panel;
            let ladder = [
                ("strong", eff(tokens.text_strong)),
                ("primary", eff(tokens.text_primary)),
                ("secondary", eff(tokens.text_secondary)),
                ("tertiary", eff(tokens.text_tertiary)),
                ("disabled", eff(tokens.text_disabled)),
                ("placeholder", eff(tokens.text_placeholder)),
            ];
            for pair in ladder.windows(2) {
                let ordered = if dark {
                    pair[0].1 > pair[1].1
                } else {
                    pair[0].1 < pair[1].1
                };
                assert!(
                    ordered,
                    "{name} 文字档对比度应严格有序(strong→placeholder):{ladder:?}"
                );
            }
            // 全档位两主题成值(不透明度有效、互不相同)
            let tier_colors = [
                tokens.text_strong,
                tokens.text_primary,
                tokens.text_secondary,
                tokens.text_tertiary,
                tokens.text_disabled,
                tokens.text_placeholder,
            ];
            for (i, c) in tier_colors.iter().enumerate() {
                assert!(c.a > 0.0, "{name} 第 {i} 档文字不可透明");
            }
            for (i, a) in tier_colors.iter().enumerate() {
                for b in &tier_colors[i + 1..] {
                    assert_ne!(a, b, "{name} 文字档位值必须两两不同");
                }
            }
        }
    }

    /// TC-TOK-ELEV-01 之分层 spec(纯函数):quad 近似分层内→外 spread 严格
    /// 递增、alpha 严格递减,总 alpha 不超过该级海拔的 alpha。
    #[test]
    fn shadow_layer_params_spread_grows_alpha_falls() {
        for (level, e) in ELEVATIONS.iter().enumerate().skip(1) {
            let layers = shadow_layer_params(level);
            assert_eq!(layers.len(), SHADOW_LAYER_WEIGHTS.len());
            let mut alpha_sum = 0.0;
            for pair in layers.windows(2) {
                assert!(pair[0].0 > 0.0, "L{level} 有影层级 spread 必须为正");
                assert!(pair[0].0 < pair[1].0, "spread 应内→外递增:{layers:?}");
                assert!(pair[0].1 > pair[1].1, "alpha 应内→外递减:{layers:?}");
            }
            for (spread, alpha) in layers {
                assert!(spread > 0.0 && alpha > 0.0, "L{level} 层参数应为正");
                alpha_sum += alpha;
            }
            assert!(
                alpha_sum <= e.alpha + 1e-6,
                "分层权重之和不得超过该级 alpha:{alpha_sum} vs {}",
                e.alpha
            );
        }
        // L0 无影:三层全零
        assert!(
            shadow_layer_params(0)
                .iter()
                .all(|(s, a)| *s == 0.0 && *a == 0.0)
        );
        // 越界钳到最后一级(不 panic)
        assert_eq!(shadow_layer_params(99), shadow_layer_params(4));
    }

    /// 状态层令牌(报告 §5.3.3):深浅两套成值,selected 深浅同比例
    /// (TC-TOK-STATE-02 的令牌侧),浅色 hover 弱于深色(白/黑叠加的
    /// 知觉补偿),disabled = 0(容器不变)。
    #[test]
    fn state_layer_tokens_two_themes_with_equal_selected_ratio() {
        let dark = StateLayerTokens::dark();
        let light = StateLayerTokens::light();
        assert!(dark.hover > light.hover, "深色白叠加需更强才可感知");
        assert!(dark.press > light.press);
        assert!(dark.press > dark.hover && light.press > light.hover);
        assert_eq!(dark.selected, light.selected, "selected 深浅同比例");
        assert_eq!(dark.focus_ring, light.focus_ring);
        assert_eq!(dark.disabled, 0.0, "禁用 = 容器不变");
        assert_eq!(light.disabled, 0.0);
        // 全部 alpha 在 [0,1]
        for t in [dark, light] {
            for v in [t.hover, t.press, t.selected, t.focus_ring, t.disabled] {
                assert!((0.0..=1.0).contains(&v), "状态层 alpha 越界:{v}");
            }
        }
    }

    /// 动效时长四档与弹簧三档(§5.8/ANI-06/07;与 JSON motion 表逐值同步,
    /// 逐值门禁在 tests/gate_tokens_sync.rs)。
    #[test]
    fn motion_duration_and_spring_presets_match_spec() {
        assert_eq!(MotionTokens::DUR_INSTANT_MS, 0.0);
        assert_eq!(MotionTokens::DUR_HOVER_MS, 80.0);
        assert_eq!(MotionTokens::DUR_STATE_MS, 120.0);
        assert_eq!(MotionTokens::DUR_PANEL_MS, 200.0);
        // SNAPPY/SOFT/BOUNCY 与 anim::Spring 档位同值(spring.rs 重指向的真相)
        assert_eq!(
            (
                SPRING_SNAPPY.stiffness,
                SPRING_SNAPPY.damping,
                SPRING_SNAPPY.mass
            ),
            (400.0, 28.0, 1.0)
        );
        assert_eq!(
            (SPRING_SOFT.stiffness, SPRING_SOFT.damping, SPRING_SOFT.mass),
            (180.0, 22.0, 1.0)
        );
        assert_eq!(
            (
                SPRING_BOUNCY.stiffness,
                SPRING_BOUNCY.damping,
                SPRING_BOUNCY.mass
            ),
            (300.0, 15.0, 1.0)
        );
        // 弹簧手感单调(SNAPPY 最刚、SOFT 最柔):经运行时序列复核,避免
        // 常量折叠断言(与 spacing_scale 同款纪律)
        let stiffness_desc = [
            SPRING_SNAPPY.stiffness,
            SPRING_BOUNCY.stiffness,
            SPRING_SOFT.stiffness,
        ];
        for pair in stiffness_desc.windows(2) {
            assert!(pair[0] > pair[1], "弹簧刚度应递减:{stiffness_desc:?}");
        }
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
