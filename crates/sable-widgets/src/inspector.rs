//! 数据驱动的通用检查器(分册四 §2 的受控组件版)。
//!
//! 应用层**每帧**从 Document 构造 `Vec<SectionSpec>`(选中集变化 → 区块
//! 自动切换),InspectorPanel 只负责把规格渲染成 PropertyRow + NumberField +
//! ColorWell——组件层对文档模型零依赖,值的双向流动全走 Binding。
//!
//! ```ignore
//! // 应用层示例(每帧构造;undo 由 Binding 的 set 闭包走 History):
//! let sections = vec![
//!     SectionSpec::new("变换", vec![
//!         RowSpec::Number { label: "X".into(), binding: x_binding,
//!                           range: (-1e5, 1e5), step: 1.0, unit: "px" },
//!         RowSpec::Number { label: "Y".into(), binding: y_binding,
//!                           range: (-1e5, 1e5), step: 1.0, unit: "px" },
//!     ]),
//!     SectionSpec::new("外观", vec![
//!         RowSpec::Color { label: "填充".into(), binding: fill_binding },
//!         RowSpec::Info { label: "节点".into(), text: node.name.clone() },
//!     ]),
//! ];
//! panel.update(cx, |p, _| p.sections = sections);
//! ```
//!
//! NumberField 是有状态 Entity,InspectorPanel 渲染时逐行 `cx.new` 创建
//! (v0.1 接受这点开销:规格每帧重建,Entity 随帧丢弃;实体池 = M2 优化)。

use gpui::{
    App, AppContext as _, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window,
    div, px,
};
use sable_foundation::scene::{Paint, Rgba8};

use crate::binding::Binding;
use crate::color::ColorWell;
use crate::number_field::NumberField;
use crate::property_row::{PropertyRow, section};
use crate::theme::theme;
use crate::tokens::{SpacingTokens, v_flex};

/// 行规格:数值 / 颜色 / 只读信息。
pub enum RowSpec {
    /// 数值行(拖拽/滚轮/键步进,见 NumberField)
    Number {
        /// 标签
        label: SharedString,
        /// 双向绑定
        binding: Binding<f64>,
        /// 取值范围 (min, max)
        range: (f64, f64),
        /// 基础步长
        step: f64,
        /// 单位后缀("px"/"°"/"%")
        unit: &'static str,
    },
    /// 颜色行(Solid 域;渐变取首色标预览,完整渐变编辑用 GradientEditor)
    Color {
        /// 标签
        label: SharedString,
        /// Paint 域绑定(内部 map 到 Rgba8 的实心色)
        binding: Binding<Paint>,
    },
    /// 只读信息行
    Info {
        /// 标签
        label: SharedString,
        /// 文本
        text: String,
    },
}

/// 分组规格:标题 + 行列表。
pub struct SectionSpec {
    /// 分组标题
    pub title: SharedString,
    /// 行列表
    pub rows: Vec<RowSpec>,
}

impl SectionSpec {
    /// 构造分组。
    pub fn new(title: impl Into<SharedString>, rows: Vec<RowSpec>) -> Self {
        SectionSpec {
            title: title.into(),
            rows,
        }
    }
}

/// 检查器面板(RenderOnce,受控):`InspectorPanel { sections }`。
#[derive(gpui::IntoElement)]
pub struct InspectorPanel {
    /// 本帧要渲染的全部分组
    pub sections: Vec<SectionSpec>,
}

/// [`Paint`] → 预览实心色:Solid 原样;渐变取首个色标(无色标回退白)。
pub fn paint_solid_preview(paint: &Paint) -> Rgba8 {
    match paint {
        Paint::Solid(c) => *c,
        Paint::LinearGradient { stops, .. }
        | Paint::RadialGradient { stops, .. }
        | Paint::ConicGradient { stops, .. } => stops
            .first()
            .map(|s| s.color)
            .unwrap_or([255, 255, 255, 255]),
    }
}

impl RenderOnce for InspectorPanel {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        // Copy 快照:后面要对 cx 做 &mut(cx.new 建 NumberField Entity)
        let colors = theme(cx).colors;
        let mut panel = v_flex()
            .size_full()
            .p(px(SpacingTokens::MD))
            .gap(px(SpacingTokens::MD))
            .bg(colors.surface_1);

        for spec in self.sections {
            let mut rows = v_flex().gap(px(SpacingTokens::XS));
            for row in spec.rows {
                match row {
                    RowSpec::Number {
                        label,
                        binding,
                        range,
                        step,
                        unit,
                    } => {
                        let field = cx.new(|_| {
                            NumberField::new(binding)
                                .range(range.0, range.1)
                                .step(step)
                                .unit(unit)
                        });
                        rows = rows.child(PropertyRow::new(label).control(field));
                    }
                    RowSpec::Color { label, binding } => {
                        // 受控展示:取当前 Paint 的实心预览色(渐变完整编辑走
                        // GradientEditor;色井点击弹取色浮窗 = 应用层接线/M2)
                        let color = paint_solid_preview(&binding.get(cx));
                        rows = rows.child(PropertyRow::new(label).control(ColorWell::new(color)));
                    }
                    RowSpec::Info { label, text } => {
                        rows = rows.child(
                            PropertyRow::new(label)
                                .control(div().text_color(colors.text_secondary).child(text)),
                        );
                    }
                }
            }
            panel = panel.child(section(spec.title, rows));
        }
        panel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_solid_preview_covers_variants() {
        assert_eq!(
            paint_solid_preview(&Paint::Solid([1, 2, 3, 4])),
            [1, 2, 3, 4]
        );
        let linear = Paint::LinearGradient {
            start: [0.0, 0.0],
            end: [1.0, 0.0],
            stops: vec![
                sable_foundation::scene::GradientStop {
                    offset: 0.0,
                    color: [255, 0, 0, 255],
                },
                sable_foundation::scene::GradientStop {
                    offset: 1.0,
                    color: [0, 0, 255, 255],
                },
            ],
        };
        assert_eq!(
            paint_solid_preview(&linear),
            [255, 0, 0, 255],
            "渐变取首色标"
        );
        let empty = Paint::LinearGradient {
            start: [0.0, 0.0],
            end: [1.0, 0.0],
            stops: vec![],
        };
        assert_eq!(
            paint_solid_preview(&empty),
            [255, 255, 255, 255],
            "无色标回退白"
        );
    }

    #[test]
    fn section_spec_builds_from_rows() {
        let count_binding = Binding::new(|_cx: &App| 0.0_f64, |_v: f64, _cx: &mut App| {});
        let paint_binding = Binding::new(
            |_cx: &App| Paint::Solid([0, 0, 0, 255]),
            |_p: Paint, _cx: &mut App| {},
        );
        let spec = SectionSpec::new(
            "外观",
            vec![
                RowSpec::Number {
                    label: "X".into(),
                    binding: count_binding,
                    range: (0.0, 100.0),
                    step: 1.0,
                    unit: "px",
                },
                RowSpec::Color {
                    label: "填充".into(),
                    binding: paint_binding,
                },
                RowSpec::Info {
                    label: "名称".into(),
                    text: "矩形A".to_string(),
                },
            ],
        );
        assert_eq!(spec.title, "外观");
        assert_eq!(spec.rows.len(), 3);
    }
}
