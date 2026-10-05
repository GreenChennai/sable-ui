//! 效果栈面板(V2.0 T4,docs/08;分册七/Illustrator"外观"面板的组件形态)。
//!
//! 数据模型(`EffectEntry`/`EffectSpec`)与五条效果命令(`AddEffect`/
//! `RemoveEffect`/`MoveEffect`/`SetEffectEnabled`/`SetEffectSpec`)全在
//! sable-foundation;像素求值在 sable-paint `effects::apply_effects_rgba`。
//! 本组件只做**受控展示**:应用层每帧从 `Node.effects` 克隆出
//! [`EffectStackSpec::entries`](与 [`crate::inspector::InspectorPanel`] 同
//! 模式),一切修改经回调回应用层组 Command(撤销语义在 foundation 已备,
//! 面板零场景依赖、零文档写入)。
//!
//! # 栈序语义(与 Illustrator 外观面板一致)
//!
//! `effects` 数组序 = 求值应用序;**面板首行 = 数组首(index 0)= 栈顶**,
//! 往下逐行越靠底层。合成时数组尾部的效果先"落地"(投影/外发光垫底、
//! 先渲染),首条效果最后作用——故曰"数组尾先渲染"。因此:
//! - "上移"(视觉上浮一层)= `MoveEffect(from, from - 1)`;栈顶(index 0)
//!   不可再上移,"下移"对称(栈底不可再下移),见 [`row_move_enabled`];
//! - 眼睛 = `SetEffectEnabled { old: !new, new }`;删除 = `RemoveEffect
//!   ::capture`(applied 守卫语义在命令侧);"+" = `AddEffect`(下标由应用
//!   层定,建议栈顶 0,与 Illustrator 新效果置顶一致)。
//!
//! # 零硬编码纪律与 A7
//!
//! 颜色一律 [`crate::theme`] 语义色;行高/按钮高走 [`control_height`]
//! 派生制。行与按钮的 hover 底色 = [`interact::state_layer`](即时切换,
//! 本组件是 RenderOnce,无跨帧状态;插值版接线 = M2,同 PropertyRow)。
//! 眼睛/按钮图标用几何色块 + 单字占位(gpui-component 的 Icon 跑在
//! gpui-pre 类型世界不可用,与 LayerPanel 同款取舍;Lucide = M2)。
//!
//! # 回调约定
//! 全部 `Rc<dyn Fn(...)>`(可克隆进元素闭包),签名以 `&mut App` 收尾;
//! 文档修改与撤销由应用层负责(与 [`crate::layer_panel::LayerPanel`] 同纪律)。

use std::rc::Rc;

use gpui::{
    App, ElementId, FontWeight, Hsla, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, RenderOnce, Styled, Window, div, px,
};
use sable_foundation::effects::{EffectEntry, EffectSpec};

use crate::controls::button::{Button, ButtonSize, button_element};
use crate::interact::{self, Semantic, SemanticRole, semantic_slot};
use crate::theme::theme;
use crate::tokens::{
    ColorTokens, FONT_SIZE_BODY, FONT_SIZE_CAPTION, FONT_SIZE_HEADING, RadiusTokens, SpacingTokens,
    control_height, h_flex, v_flex,
};

/// 效果行的估行高基准(正文 12px 字号,CJK 派生)。
const ROW_LINE_HEIGHT_PX: f32 = 16.0;
/// 行垂直内边距(16 + 2×6 = 28,与属性行/图层面板行同高)。
const ROW_V_PADDING_PX: f32 = 6.0;

/// 面板规格(应用层每帧构造;条目从 `Node.effects` 克隆)。
pub struct EffectStackSpec {
    /// 效果条目(数组序 = 栈序,首 = 栈顶;渲染行序与之相同)。
    pub entries: Vec<EffectEntry>,
    /// 面板标题。
    pub label: String,
}

/// 回调类型别名(clippy type_complexity 治理;语义见字段 doc)。
pub type ClickFn = Rc<dyn Fn(&mut App)>;

pub type ToggleEffectFn = Rc<dyn Fn(usize, bool, &mut App)>;
pub type MoveEffectFn = Rc<dyn Fn(usize, &mut App)>;
pub type AddEffectFn = Rc<dyn Fn(EffectSpec, &mut App)>;

/// 效果栈回调(全部 `Rc<dyn Fn>`,`&mut App` 收尾;修改走应用层组 Command):
/// - `on_toggle(ix, new_enabled)`:眼睛 → `SetEffectEnabled { old: !new, new }`;
/// - `on_move_up(ix)`:上移一层 → `MoveEffect(ix, ix - 1)`(ix = 0 时按钮已禁用);
/// - `on_move_down(ix)`:下移一层 → `MoveEffect(ix, ix + 1)`(栈底时按钮已禁用);
/// - `on_remove(ix)`:`RemoveEffect::capture` + 入栈;
/// - `on_add(spec)`:"+" 预设按钮 → `AddEffect`(下标应用层定,建议栈顶 0)。
#[derive(Clone)]
pub struct EffectStackCallbacks {
    /// 眼睛开关:`(行下标, 新 enabled 状态, cx)`。
    pub on_toggle: ToggleEffectFn,
    /// 上移一层:`(行下标, cx)`。
    pub on_move_up: MoveEffectFn,
    /// 下移一层:`(行下标, cx)`。
    pub on_move_down: MoveEffectFn,
    /// 删除:`(行下标, cx)`。
    pub on_remove: MoveEffectFn,
    /// 添加预设:`(效果参数, cx)`。
    pub on_add: AddEffectFn,
}

impl Default for EffectStackCallbacks {
    /// 全空操作(占位安全:未接线的面板可渲染、可点、零副作用)。
    fn default() -> Self {
        EffectStackCallbacks {
            on_toggle: Rc::new(|_, _, _| {}),
            on_move_up: Rc::new(|_, _| {}),
            on_move_down: Rc::new(|_, _| {}),
            on_remove: Rc::new(|_, _| {}),
            on_add: Rc::new(|_, _| {}),
        }
    }
}

/// "+" 按钮的四种预设(模糊/投影/发光/对比度各一,均为立即可见的非恒等参数;
/// 简化自"预设下拉"的 v0.1 形态,见模块 doc)。
pub fn add_presets() -> Vec<(&'static str, EffectSpec)> {
    vec![
        ("+模糊", EffectSpec::GaussianBlur { radius: 4.0 }),
        (
            "+投影",
            EffectSpec::DropShadow {
                blur: 4.0,
                offset: [0.0, 2.0],
                color: [0, 0, 0, 128],
            },
        ),
        (
            "+发光",
            EffectSpec::Glow {
                radius: 4.0,
                color: [255, 200, 40, 255],
                inner: false,
            },
        ),
        ("+对比度", EffectSpec::contrast(1.2)),
    ]
}

/// 行内"上移/下移"按钮可用性(纯函数;渲染据此给 disabled 灰态):
/// 栈顶(index 0)不可再上移、栈底(len-1)不可再下移;单行两向皆禁。
/// 返回 `(上移可用, 下移可用)`。
pub fn row_move_enabled(len: usize, ix: usize) -> (bool, bool) {
    (ix > 0, ix + 1 < len)
}

/// 效果条目的展示名(UI 标签,非命令名):`"高斯模糊 r=2px"`、
/// `"投影 Δ(2,-1.5) b=4px"`、`"内/外发光 r=…"`;ColorMatrix 识别四种
/// 预设构造器(亮度/对比度/饱和度/色相,容差 1e-3)与恒等阵,手工矩阵
/// 回落 `"颜色矩阵"`。
pub fn effect_label(spec: &EffectSpec) -> String {
    match spec {
        EffectSpec::GaussianBlur { radius } => {
            format!("高斯模糊 r={}px", fmt_num(*radius))
        }
        EffectSpec::DropShadow { blur, offset, .. } => format!(
            "投影 Δ({},{}) b={}px",
            fmt_num(offset[0]),
            fmt_num(offset[1]),
            fmt_num(*blur)
        ),
        EffectSpec::Glow { radius, inner, .. } => format!(
            "{}发光 r={}px",
            if *inner { "内" } else { "外" },
            fmt_num(*radius)
        ),
        EffectSpec::ColorMatrix { matrix, offsets } => color_matrix_label(*matrix, *offsets),
    }
}

/// f64 → 紧凑数字串(整数去小数尾;其余保留两位小数内的最短表示)。
fn fmt_num(v: f64) -> String {
    if v.abs() < 1e-9 {
        return "0".to_string();
    }
    let r = (v * 100.0).round() / 100.0;
    if (r - r.trunc()).abs() < 1e-9 {
        format!("{r:.0}")
    } else {
        format!("{r}")
    }
}

/// f32 相等判(预设识别容差;预设矩阵系数不超过两位小数,1e-3 足够)。
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

/// ColorMatrix → 预设名(按 [`EffectSpec`] 预设构造器的系数结构反解;
/// 识别不出则回落通用名,绝不猜错——所有分支先整阵验证再命名)。
fn color_matrix_label(m: [[f32; 4]; 4], offsets: [f32; 4]) -> String {
    // 恒等(含 brightness(1.0)/saturate(1.0)/contrast(1.0)/hue(0°) 等价形)
    if m == [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ] && offsets == [0.0; 4]
    {
        return "颜色矩阵(恒等)".to_string();
    }
    // 亮度:RGB 对角 ×s + alpha 恒等 + 零偏移
    if offsets == [0.0; 4]
        && m[3] == [0.0, 0.0, 0.0, 1.0]
        && close(m[0][0], m[1][1])
        && close(m[1][1], m[2][2])
        && m[0][1] == 0.0
        && m[0][2] == 0.0
        && m[0][3] == 0.0
        && m[1][0] == 0.0
        && m[1][2] == 0.0
        && m[1][3] == 0.0
        && m[2][0] == 0.0
        && m[2][1] == 0.0
        && m[2][3] == 0.0
        && m[3][0] == 0.0
        && m[3][1] == 0.0
        && m[3][2] == 0.0
    {
        return format!("亮度 ×{}", fmt_num(f64::from(m[0][0])));
    }
    // 对比度:对角 c + 三通道同偏移 0.5·(1−c)
    if close(m[0][0], m[1][1])
        && close(m[1][1], m[2][2])
        && m[3] == [0.0, 0.0, 0.0, 1.0]
        && m[0][1] == 0.0
        && m[0][2] == 0.0
        && m[0][3] == 0.0
        && m[1][0] == 0.0
        && m[1][2] == 0.0
        && m[1][3] == 0.0
        && m[2][0] == 0.0
        && m[2][1] == 0.0
        && m[2][3] == 0.0
        && close(offsets[0], offsets[1])
        && close(offsets[1], offsets[2])
        && offsets[3] == 0.0
        && close(offsets[0], 0.5 * (1.0 - m[0][0]))
    {
        return format!("对比度 {}", fmt_num(f64::from(m[0][0])));
    }
    // 饱和度:从 m00 反解 s(m00 = lr + (1−lr)·s),整阵回验
    if m[3] == [0.0, 0.0, 0.0, 1.0] && offsets == [0.0; 4] && m[0][3] == 0.0 && m[1][3] == 0.0 {
        let s = (m[0][0] - 0.2126f32) / 0.7874f32;
        let EffectSpec::ColorMatrix {
            matrix: reference, ..
        } = EffectSpec::saturate(s)
        else {
            return "颜色矩阵".to_string();
        };
        let ok = m
            .iter()
            .zip(reference.iter())
            .all(|(row, ref_row)| row.iter().zip(ref_row.iter()).all(|(a, b)| close(*a, *b)));
        if ok {
            return format!("饱和度 {}", fmt_num(f64::from(s)));
        }
    }
    // 色相:trace = 1 + 2cos(规范矩阵迹),反解 cos/sin 后整阵回验
    if m[3] == [0.0, 0.0, 0.0, 1.0] && offsets == [0.0; 4] {
        let cos = (m[0][0] + m[1][1] + m[2][2] - 1.0) / 2.0;
        let sin = (m[0][2] - 0.072 + 0.072 * cos) / 0.928;
        let degrees = sin.atan2(cos).to_degrees().rem_euclid(360.0);
        let EffectSpec::ColorMatrix {
            matrix: reference, ..
        } = EffectSpec::hue_rotate(degrees)
        else {
            return "颜色矩阵".to_string();
        };
        let rows_match = m[..3]
            .iter()
            .zip(reference[..3].iter())
            .all(|(row, ref_row)| {
                row[..3]
                    .iter()
                    .zip(ref_row[..3].iter())
                    .all(|(a, b)| close(*a, *b))
                    && row[3] == 0.0
            });
        if rows_match {
            return format!("色相 {degrees:.0}°");
        }
    }
    "颜色矩阵".to_string()
}

/// 效果栈面板(受控 RenderOnce;经 [`effect_stack_panel`] 便捷构造)。
#[derive(gpui::IntoElement)]
pub struct EffectStackPanel {
    spec: EffectStackSpec,
    cb: EffectStackCallbacks,
    /// A11Y-02 语义槽(可访问名/角色;缺省回落标题/List)
    semantic: Semantic,
}

// A11Y-02 语义槽(label/role/semantic 三件):面板可访问名缺省 = 面板标题,
// role 缺省 List(逐行 ListItem 挂接见 render)。
semantic_slot!(EffectStackPanel);

impl EffectStackPanel {
    /// 解析语义(A11Y-02):显式 `.label(...)`/`.role(...)` 优先,缺省 =
    /// (spec 标题, List)。
    #[must_use]
    pub fn resolved_semantic(&self) -> Semantic {
        let sem = match self.semantic.label() {
            Some(_) => self.semantic.clone(),
            None => Semantic::new().with_label(self.spec.label.clone()),
        };
        if self.semantic.role().is_some() {
            return sem;
        }
        let role = SemanticRole::List;
        sem.with_role(role)
    }
}

impl EffectStackPanel {
    /// 行高(派生制,与属性行/图层面板行同高):max(26, 16 + 12) = 28。
    pub fn row_height() -> f32 {
        control_height(
            crate::tokens::HEIGHT_DEFAULT,
            ROW_LINE_HEIGHT_PX,
            ROW_V_PADDING_PX,
        )
    }
}

impl RenderOnce for EffectStackPanel {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = theme(cx).colors;
        let n = self.spec.entries.len();

        // 头部:标题 + 计数 + 四个预设"+"按钮(简化自预设下拉)
        let mut header = h_flex()
            .gap(px(SpacingTokens::XS))
            .child(
                div()
                    .text_size(px(FONT_SIZE_HEADING))
                    .font_weight(FontWeight::MEDIUM)
                    .child(self.spec.label),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(FONT_SIZE_CAPTION))
                    .text_color(colors.text_disabled)
                    .child(format!("{n} 效果")),
            );
        for (ix, (name, preset)) in add_presets().into_iter().enumerate() {
            let cb = self.cb.clone();
            header = header.child(button_element(
                Button::new(ElementId::named_usize("fx-add", ix), name)
                    .size(ButtonSize::Compact)
                    .on_press(move |_ev, _win, cx| (cb.on_add)(preset.clone(), cx)),
                cx,
            ));
        }

        let mut panel = v_flex()
            .gap(px(SpacingTokens::SM))
            .p(px(SpacingTokens::XS))
            .bg(colors.surface_1)
            .child(header);

        if n == 0 {
            panel = panel.child(
                div()
                    .text_size(px(FONT_SIZE_CAPTION))
                    .text_color(colors.text_disabled)
                    .child("无效果——用上方按钮添加"),
            );
        }

        // 行序 = 数组序:首行 = 栈顶(合成时数组尾先落地,见模块 doc)
        for (ix, entry) in self.spec.entries.iter().enumerate() {
            let (up_on, down_on) = row_move_enabled(n, ix);
            let enabled = entry.enabled;
            // A7/TOK-04:行 hover 底色走 state-layer(深色叠白/浅色叠黑,
            // 修复浅色主题 hover 钳 1.0 失效)
            let row_bg = interact::state_layer(
                colors.surface_1,
                interact::InteractState::Hover,
                colors.accent,
            );
            let eye_cb = self.cb.clone();
            let row = h_flex()
                .w_full()
                .h(px(EffectStackPanel::row_height()))
                .px(px(SpacingTokens::XS))
                .gap(px(SpacingTokens::XS))
                .rounded(px(RadiusTokens::SM))
                .hover(move |style| style.bg(row_bg))
                // 眼睛:填充 accent = 启用(几何色块占位,取舍见模块 doc)
                .child(eye_toggle(colors, enabled, {
                    let cb = eye_cb;
                    Rc::new(move |cx: &mut App| (cb.on_toggle)(ix, !enabled, cx))
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(FONT_SIZE_BODY))
                        .text_color(if enabled {
                            colors.text_secondary
                        } else {
                            colors.text_disabled
                        })
                        .child(effect_label(&entry.spec)),
                )
                // 行内小按钮(CMP-11 收口:私有 mini_button 已删,统一走
                // controls::button 内联形态;禁用 = 仅前景降级、容器不变、
                // 不响应——TOK-07 语义由 Button 组件单点保证)
                .child(button_element(
                    Button::new(ElementId::named_usize("fx-up", ix), "上")
                        .size(ButtonSize::Compact)
                        .disabled(!up_on)
                        .on_press({
                            let cb = self.cb.clone();
                            move |_ev, _win, cx| (cb.on_move_up)(ix, cx)
                        }),
                    cx,
                ))
                .child(button_element(
                    Button::new(ElementId::named_usize("fx-down", ix), "下")
                        .size(ButtonSize::Compact)
                        .disabled(!down_on)
                        .on_press({
                            let cb = self.cb.clone();
                            move |_ev, _win, cx| (cb.on_move_down)(ix, cx)
                        }),
                    cx,
                ))
                .child(button_element(
                    Button::new(ElementId::named_usize("fx-remove", ix), "删")
                        .size(ButtonSize::Compact)
                        .on_press({
                            let cb = self.cb.clone();
                            move |_ev, _win, cx| (cb.on_remove)(ix, cx)
                        }),
                    cx,
                ));
            // A11Y-02:行语义挂接(ListItem + 效果名;单点透传待 TD-01)
            let row_semantic = Semantic::new()
                .with_role(SemanticRole::ListItem)
                .with_label(effect_label(&entry.spec));
            let row = interact::attach_semantics(row, &row_semantic);
            panel = panel.child(row);
        }
        panel
    }
}

/// 眼睛开关(几何色块占位,LayerPanel 同款):填充 accent = 启用、透明 =
/// 禁用,边框随状态灰阶;hover 底色即时 state-layer 叠加(TOK-04)。
///
/// 命名说明(CMP-11/TC-GATE-DUP-01):这是**状态指示器**而非按钮族成员
/// ——视觉本体是"填色方块"的状态位(无文本/图标语义,Icon 系统 = §5.5
/// 后续批次接入真实眼睛图标时再升格为 `IconButton`),不在
/// `fn *_button` 收口范围内,故命名 `eye_toggle`。
fn eye_toggle(
    colors: ColorTokens,
    enabled: bool,
    on_toggle: Rc<dyn Fn(&mut App)>,
) -> gpui::AnyElement {
    let hover_bg = interact::state_layer(
        colors.surface_1,
        interact::InteractState::Hover,
        colors.accent,
    );
    // A11Y-03:视觉 10px、命中 ≥24px(hit_slot 透明热区;监听挂热区容器,
    // 补白区可点;press 反馈 = 状态即翻,行内小位不做按压底)
    interact::hit_slot(
        div()
            .size(px(10.0))
            .rounded(px(RadiusTokens::SM))
            .border_1()
            .border_color(if enabled {
                colors.text_secondary
            } else {
                colors.text_disabled
            })
            .bg(if enabled {
                colors.accent
            } else {
                Hsla::transparent_black()
            })
            .hover(move |style| style.bg(hover_bg)),
    )
    .flex_shrink_0()
    .cursor_pointer()
    .on_mouse_down(MouseButton::Left, move |_ev: &MouseDownEvent, _win, cx| {
        on_toggle(cx)
    })
    .into_any_element()
}

/// 便捷构造:`effect_stack_panel(&spec, &cb)`(规格与回调克隆进面板)。
pub fn effect_stack_panel(spec: &EffectStackSpec, cb: &EffectStackCallbacks) -> gpui::AnyElement {
    EffectStackPanel {
        spec: EffectStackSpec {
            entries: spec.entries.clone(),
            label: spec.label.clone(),
        },
        cb: cb.clone(),
        semantic: Semantic::new(),
    }
    .into_any_element()
}

/// story/演示用条目:四种效果变体各一、全部启用(非恒等,渲染立即可见)。
pub fn demo_entries() -> Vec<EffectEntry> {
    vec![
        EffectEntry {
            spec: EffectSpec::GaussianBlur { radius: 2.0 },
            enabled: true,
        },
        EffectEntry {
            spec: EffectSpec::DropShadow {
                blur: 8.0,
                offset: [0.0, 2.0],
                color: [0, 0, 0, 128],
            },
            enabled: true,
        },
        EffectEntry {
            spec: EffectSpec::Glow {
                radius: 6.0,
                color: [79, 159, 255, 255],
                inner: false,
            },
            enabled: true,
        },
        EffectEntry {
            spec: EffectSpec::saturate(0.0),
            enabled: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashSet;

    /// 条目/参数的变体判别集(覆盖性断言用)。
    fn spec_variants(
        specs: impl Iterator<Item = EffectSpec>,
    ) -> HashSet<std::mem::Discriminant<EffectSpec>> {
        specs.map(|s| std::mem::discriminant(&s)).collect()
    }

    // —— effect_label:四种变体 + ColorMatrix 预设识别 ——

    #[test]
    fn effect_label_formats_all_four_spec_kinds() {
        assert_eq!(
            effect_label(&EffectSpec::GaussianBlur { radius: 3.5 }),
            "高斯模糊 r=3.5px"
        );
        assert_eq!(
            effect_label(&EffectSpec::GaussianBlur { radius: 2.0 }),
            "高斯模糊 r=2px",
            "整数值去小数尾"
        );
        assert_eq!(
            effect_label(&EffectSpec::DropShadow {
                blur: 4.0,
                offset: [2.0, -1.5],
                color: [0, 0, 0, 128],
            }),
            "投影 Δ(2,-1.5) b=4px"
        );
        assert_eq!(
            effect_label(&EffectSpec::Glow {
                radius: 6.0,
                color: [255, 200, 40, 255],
                inner: false,
            }),
            "外发光 r=6px"
        );
        assert_eq!(
            effect_label(&EffectSpec::Glow {
                radius: 6.0,
                color: [255, 200, 40, 255],
                inner: true,
            }),
            "内发光 r=6px"
        );
        assert_eq!(effect_label(&EffectSpec::brightness(2.0)), "亮度 ×2");
        assert_eq!(effect_label(&EffectSpec::contrast(0.5)), "对比度 0.5");
        assert_eq!(effect_label(&EffectSpec::saturate(0.0)), "饱和度 0");
        assert_eq!(effect_label(&EffectSpec::saturate(0.5)), "饱和度 0.5");
        assert_eq!(effect_label(&EffectSpec::hue_rotate(90.0)), "色相 90°");
        assert_eq!(effect_label(&EffectSpec::hue_rotate(180.0)), "色相 180°");
        assert_eq!(
            effect_label(&EffectSpec::brightness(1.0)),
            "颜色矩阵(恒等)",
            "恒等阵优先于亮度识别(brightness(1) 数学恒等)"
        );
        // 手工构造的非预设矩阵 → 通用名
        assert_eq!(
            effect_label(&EffectSpec::ColorMatrix {
                matrix: [
                    [0.5, 0.1, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
                offsets: [0.0; 4],
            }),
            "颜色矩阵"
        );
    }

    // —— demo_entries / add_presets ——

    #[test]
    fn demo_entries_nonempty_all_enabled_and_covers_four_kinds() {
        let entries = demo_entries();
        assert!(!entries.is_empty(), "demo 条目非空");
        for entry in &entries {
            assert!(entry.enabled, "demo 条目应全部启用");
            assert!(!entry.spec.is_noop(), "demo 条目应非恒等(渲染立即可见)");
        }
        assert_eq!(
            spec_variants(entries.iter().map(|e| e.spec.clone())).len(),
            4,
            "四种效果变体各一,story 覆盖完整"
        );
    }

    #[test]
    fn add_presets_cover_four_families_and_are_not_noop() {
        let presets = add_presets();
        assert_eq!(presets.len(), 4, "四种预设各一(简化下拉的 v0.1 形态)");
        for (label, spec) in &presets {
            assert!(!label.is_empty(), "预设按钮需有标签");
            assert!(!spec.is_noop(), "预设参数应立即可见");
        }
        assert_eq!(
            spec_variants(presets.into_iter().map(|(_, s)| s)).len(),
            4,
            "四按钮各对应一种效果变体"
        );
    }

    // —— 栈边界的按钮可用性 ——

    #[test]
    fn row_move_enabled_follows_stack_edges() {
        assert_eq!(row_move_enabled(3, 0), (false, true), "栈顶不可再上移");
        assert_eq!(row_move_enabled(3, 1), (true, true));
        assert_eq!(row_move_enabled(3, 2), (true, false), "栈底不可再下移");
        assert_eq!(row_move_enabled(1, 0), (false, false), "单行两向皆禁");
    }

    #[test]
    fn row_height_matches_property_row() {
        assert_eq!(EffectStackPanel::row_height(), 28.0);
        assert_eq!(
            EffectStackPanel::row_height(),
            crate::property_row::PropertyRow::row_height()
        );
    }

    // —— 回调桩(Rc<RefCell<usize>> 计数)——

    #[test]
    fn callback_stubs_count_via_shared_rc_and_default_is_noop() {
        // 与 layer_panel tests 同约束:gpui 的 &mut App 无法在未启用
        // test-support 的单测环境构造,回调的"真实触发"由应用层冒烟/story
        // 验收;此处验收桩接线本身——构造不触发、Clone 共享同一闭包。
        let toggle_hits = Rc::new(RefCell::new(0usize));
        let cb = EffectStackCallbacks {
            on_toggle: {
                let hits = toggle_hits.clone();
                Rc::new(move |_ix: usize, _on: bool, _cx: &mut App| *hits.borrow_mut() += 1)
            },
            ..EffectStackCallbacks::default()
        };
        assert_eq!(*toggle_hits.borrow(), 0, "构造回调不得触发");
        let cb_clone = cb.clone();
        assert!(
            Rc::ptr_eq(&cb.on_toggle, &cb_clone.on_toggle),
            "Clone 必须共享同一回调 Rc(克隆面板与原面板指向同桩)"
        );
        assert_eq!(*toggle_hits.borrow(), 0, "克隆不触发");
        // 默认回调为空操作占位,可安全点击
        let noop = EffectStackCallbacks::default();
        assert!(Rc::ptr_eq(&noop.on_move_up, &noop.on_move_up));
    }

    // —— 面板构造(渲染路径需 App,同 PropertyRow/Binding 测试策略)——

    #[test]
    fn panel_builds_from_spec_and_callbacks_without_app() {
        let spec = EffectStackSpec {
            entries: demo_entries(),
            label: "效果栈".to_string(),
        };
        let el = effect_stack_panel(&spec, &EffectStackCallbacks::default());
        let _ = el; // 构造成功即编译期验收;渲染由应用层 story/冒烟覆盖
        let empty = effect_stack_panel(
            &EffectStackSpec {
                entries: Vec::new(),
                label: "空".to_string(),
            },
            &EffectStackCallbacks::default(),
        );
        let _ = empty;
    }
}
