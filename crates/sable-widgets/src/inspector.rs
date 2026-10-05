//! 数据驱动的通用检查器(分册四 §2 的受控组件版)。
//!
//! 应用层从 Document 构造 `Vec<SectionSpec>` 经 [`InspectorPanel::set_sections`]
//! 喂入(选中集变化 → 区块自动切换),InspectorPanel 只负责把规格渲染成
//! PropertyRow + NumberField + ColorWell——组件层对文档模型零依赖,值的双向
//! 流动全走 Binding。
//!
//! ```ignore
//! // 应用层示例(spec 每帧重建;undo 由 Binding 的 set 闭包走 History):
//! let panel = cx.new(|_| InspectorPanel::new());
//! // 每帧(或选中集变化时):
//! panel.update(cx, |p, cx| p.set_sections(sections, cx));
//! div().child(panel.clone())
//! ```
//!
//! PERF-02/CMP-06:面板是 Entity,NumberField 实体**按位复用**——标签与数值
//! 形状(range/step/unit)一致的行只做 rebind(重定向绑定,焦点/编辑态保留,
//! 修复旧"实体随帧丢弃、编辑态易丢"病),规格变化才新建实体;创建计数见
//! [`InspectorPanel::created_count`]。Binding 不可克隆也不可比较(每帧新构造),
//! 故身份判定 = 位置 + (标签, 形状),绑定每帧重定向,语义与旧实现一致
//! (读新闭包 = 读当前文档)。

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled,
    Window, div, px,
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
        unit: SharedString,
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

/// 数值行槽(PERF-02):复用判定键 = 标签 + 数值形状;实体跨帧持有。
struct NumberSlot {
    label: SharedString,
    range: (f64, f64),
    step: f64,
    unit: SharedString,
    field: Entity<NumberField>,
}

/// 复用判定(PERF-02 纯函数核心):标签与数值形状一致 → 复用实体(rebind
/// 重定向绑定);任一不同 → 新建。`prev` = 旧模型同位行的数值槽视图。
fn number_slot_matches(
    prev: Option<(&str, (f64, f64), f64, &str)>,
    label: &str,
    range: (f64, f64),
    step: f64,
    unit: &str,
) -> bool {
    match prev {
        Some((pl, pr, ps, pu)) => pl == label && pr == range && ps == step && pu == unit,
        None => false,
    }
}

/// 渲染模型(`set_sections` 的产物;Number 行的绑定已被实体消费,渲染期
/// 只持实体句柄)。
enum RowModel {
    Number(NumberSlot),
    Color {
        label: SharedString,
        binding: Binding<Paint>,
    },
    Info {
        label: SharedString,
        text: String,
    },
}

struct SectionModel {
    title: SharedString,
    rows: Vec<RowModel>,
}

/// 检查器面板(Entity,受控):spec 经 [`Self::set_sections`] 喂入。
pub struct InspectorPanel {
    sections: Vec<SectionModel>,
    /// PERF-02:实体创建累计数(复用不计数;宿主可断言"同规格重复喂入
    /// created_count 不增长")
    created_count: usize,
}

impl Default for InspectorPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl InspectorPanel {
    /// 空面板;规格经 [`Self::set_sections`] 喂入。
    pub fn new() -> Self {
        InspectorPanel {
            sections: Vec::new(),
            created_count: 0,
        }
    }

    /// 喂入本帧规格(PERF-02):与上一帧按位 (分组 ix, 行 ix) 对齐——
    /// 数值行标签+形状一致则复用实体并重定向绑定,否则新建。Info/Color 行
    /// 无实体,直接换模型。
    pub fn set_sections(&mut self, sections: Vec<SectionSpec>, cx: &mut Context<Self>) {
        let old = std::mem::take(&mut self.sections);
        let mut next = Vec::with_capacity(sections.len());
        let mut created = 0usize;
        for (si, spec) in sections.into_iter().enumerate() {
            let old_rows = old.get(si).map(|s| &s.rows);
            let mut rows = Vec::with_capacity(spec.rows.len());
            for (ri, row) in spec.rows.into_iter().enumerate() {
                match row {
                    RowSpec::Number {
                        label,
                        binding,
                        range,
                        step,
                        unit,
                    } => {
                        let prev = old_rows.and_then(|r| r.get(ri)).and_then(|m| match m {
                            RowModel::Number(s) => {
                                Some((s.label.as_ref(), s.range, s.step, s.unit.as_ref()))
                            }
                            _ => None,
                        });
                        if number_slot_matches(prev, &label, range, step, &unit) {
                            // 复用:旧实体重定向绑定(焦点保留,拖拽/编辑缓冲取消)
                            if let Some(RowModel::Number(slot)) = old_rows.and_then(|r| r.get(ri)) {
                                let field = slot.field.clone();
                                field.update(cx, |f, _| f.rebind(binding));
                                rows.push(RowModel::Number(NumberSlot {
                                    label,
                                    range,
                                    step,
                                    unit,
                                    field,
                                }));
                                continue;
                            }
                        }
                        // 新建:元素 id 用创建序号(同屏稳定)
                        let id = gpui::ElementId::named_usize(
                            "inspector-number",
                            self.created_count + created,
                        );
                        let field = cx.new(|_| {
                            NumberField::new(binding)
                                .range(range.0, range.1)
                                .step(step)
                                .unit(unit.clone())
                                .element_id(id)
                        });
                        created += 1;
                        rows.push(RowModel::Number(NumberSlot {
                            label,
                            range,
                            step,
                            unit,
                            field,
                        }));
                    }
                    RowSpec::Color { label, binding } => {
                        rows.push(RowModel::Color { label, binding });
                    }
                    RowSpec::Info { label, text } => {
                        rows.push(RowModel::Info { label, text });
                    }
                }
            }
            next.push(SectionModel {
                title: spec.title,
                rows,
            });
        }
        self.created_count += created;
        self.sections = next;
    }

    /// PERF-02:实体创建累计数。同规格重复 [`Self::set_sections`] 不增长
    /// (全复用);宿主可据此断言"逐帧喂入不逐帧建实体"。
    pub fn created_count(&self) -> usize {
        self.created_count
    }

    /// 可访问名(A11Y-02 语义槽,**接口占位,恒等返回**):检查器是分组
    /// 容器,语义 = Group、名称真相 = 各分组标题/各行标签,本槽为
    /// TC-A11Y-LABEL-01 门禁面与 TD-01 预留——升级时改为存态落树,签名不变。
    #[must_use]
    pub fn label(self, _label: impl Into<SharedString>) -> Self {
        self
    }

    /// 语义(A11Y-02,只读):role = Group;label = 首个分组标题(容器
    /// 语义由子件承担,gpui 0.2.2 无语义树、存态消费待 TD-01)。
    #[must_use]
    pub fn semantic(&self) -> crate::interact::Semantic {
        let sem = crate::interact::Semantic::new().with_role(crate::interact::SemanticRole::Group);
        match self.sections.first() {
            Some(first) => sem.with_label(first.title.clone()),
            None => sem,
        }
    }
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

impl Render for InspectorPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Copy 快照:后面要对 cx 做只读取值
        let colors = theme(cx).colors;
        let mut panel = v_flex()
            .size_full()
            .p(px(SpacingTokens::MD))
            .gap(px(SpacingTokens::MD))
            .bg(colors.surface_1);

        for sec in &self.sections {
            let mut rows = v_flex().gap(px(SpacingTokens::XS));
            for row in &sec.rows {
                match row {
                    RowModel::Number(slot) => {
                        // PERF-02:实体跨帧复用,仅句柄 clone(Arc 计数)
                        rows = rows.child(
                            PropertyRow::new(slot.label.clone()).control(slot.field.clone()),
                        );
                    }
                    RowModel::Color { label, binding } => {
                        // 受控展示:取当前 Paint 的实心预览色(渐变完整编辑走
                        // GradientEditor;色井点击弹取色浮窗 = 应用层接线/M2)
                        let color = paint_solid_preview(&binding.get(cx));
                        rows = rows
                            .child(PropertyRow::new(label.clone()).control(ColorWell::new(color)));
                    }
                    RowModel::Info { label, text } => {
                        rows =
                            rows.child(PropertyRow::new(label.clone()).control(
                                div().text_color(colors.text_secondary).child(text.clone()),
                            ));
                    }
                }
            }
            panel = panel.child(section(sec.title.clone(), rows));
        }
        panel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::App;

    #[test]
    fn tc_perf_insp_01_slot_reuse_decision_by_label_and_shape() {
        // PERF-02 复用判定核心(实体创建需 App,裁决层先纯函数断言):
        // 同位同标签同形状 → 复用(不新建);形状任一变化 → 新建。
        assert!(number_slot_matches(
            Some(("X", (0.0, 100.0), 1.0, "px")),
            "X",
            (0.0, 100.0),
            1.0,
            "px"
        ));
        assert!(
            !number_slot_matches(
                Some(("X", (0.0, 100.0), 1.0, "px")),
                "Y",
                (0.0, 100.0),
                1.0,
                "px"
            ),
            "标签变 = 新建"
        );
        assert!(
            !number_slot_matches(
                Some(("X", (0.0, 100.0), 1.0, "px")),
                "X",
                (-1.0, 1.0),
                1.0,
                "px"
            ),
            "范围变 = 新建"
        );
        assert!(
            !number_slot_matches(
                Some(("X", (0.0, 100.0), 1.0, "px")),
                "X",
                (0.0, 100.0),
                0.1,
                "px"
            ),
            "步长变 = 新建"
        );
        assert!(
            !number_slot_matches(
                Some(("X", (0.0, 100.0), 1.0, "px")),
                "X",
                (0.0, 100.0),
                1.0,
                "°"
            ),
            "单位变 = 新建"
        );
        assert!(
            !number_slot_matches(None, "X", (0.0, 100.0), 1.0, "px"),
            "无旧槽 = 新建"
        );
    }

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
                    unit: "px".into(),
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
