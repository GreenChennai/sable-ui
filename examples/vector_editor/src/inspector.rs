//! 属性检查器宿主:单选 → 变换区块(X/Y 走 `SetTransform` 命令)+ 填充
//! 区块(走 `SetFill`);多选/未选 → Info 行。
//!
//! 每帧按 Document 当前选中集**重建** `Vec<SectionSpec>` 喂给 widgets 的
//! `InspectorPanel`——随选中集动态切换区块,分册四 §2。
//!
//! widgets 真实签名(2026-10 以 crates/sable-widgets/src/inspector.rs 为准):
//! `SectionSpec::new(title, rows)`;`RowSpec::Number { label, binding:
//! Binding<f64>, range, step, unit }` / `RowSpec::Color { label, binding:
//! Binding<Paint> }` / `RowSpec::Info { label, text: String }`;值的双向
//! 流动全走 `Binding`(get 闭包读文档,set 闭包内走 `Document::exec`
//! 出撤销步,`SetFill` 自带同节点合并);`InspectorPanel` 是 Entity
//! (PERF-02 实体池):构造一次,每帧 `set_sections(sections, cx)` 喂入。

use gpui::{
    App, AppContext as _, Entity, IntoElement, ParentElement as _, Render, Styled as _, div,
};
use sable::core::command::{SetFill, SetTransform};
use sable::core::scene::{NodeId, Paint};
use sable::gpui;
use sable::kurbo::Affine;
use sable::widgets::binding::Binding;
use sable::widgets::inspector::{InspectorPanel, RowSpec, SectionSpec};

use crate::document::Document;
use crate::palette::{Palette, hsla_to_rgba8};

/// 检查器宿主面板(挂在 dock 右侧)。
pub struct InspectorHost {
    doc: Entity<Document>,
    panel: Entity<InspectorPanel>,
}

impl InspectorHost {
    /// 构造宿主(PERF-02:面板是 Entity,NumberField 实体跨帧池化复用)。
    pub fn new(doc: Entity<Document>, cx: &mut App) -> Entity<Self> {
        let panel = cx.new(|_| InspectorPanel::new());
        cx.new(|_| Self { doc, panel })
    }
}

impl Render for InspectorHost {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let sections = build_sections(&self.doc, cx);
        self.panel
            .update(cx, |panel, cx| panel.set_sections(sections, cx));
        div().size_full().child(self.panel.clone())
    }
}

/// 选中集 → 区块列表。
fn build_sections(doc: &Entity<Document>, cx: &App) -> Vec<SectionSpec> {
    let document = doc.read(cx);
    match document.selection.as_slice() {
        [] => vec![SectionSpec::new(
            "信息",
            vec![info_row("选中", "未选中对象:在画布上点选,或拖拽框选")],
        )],
        [id] => {
            let id = *id;
            let scene = document.scene.read(cx);
            let transform = scene
                .node(id)
                .map(|node| node.transform)
                .unwrap_or(Affine::IDENTITY);
            let coeffs = transform.as_coeffs();
            let fill = scene.path(id).and_then(|path| path.fill.clone());
            let name = scene
                .node(id)
                .map(|node| node.name.clone())
                .unwrap_or_default();
            // 非 Solid 填充的垫显色走主题次级色(禁硬编码)
            let neutral = Paint::Solid(hsla_to_rgba8(Palette::get(cx).text_secondary));
            vec![
                SectionSpec::new(
                    "变换",
                    vec![
                        number_row("X", doc.clone(), id, 4, coeffs[4]),
                        number_row("Y", doc.clone(), id, 5, coeffs[5]),
                    ],
                ),
                SectionSpec::new(
                    "外观",
                    vec![
                        RowSpec::Color {
                            label: "填充".into(),
                            binding: fill_binding(doc.clone(), id, fill, neutral),
                        },
                        info_row("节点", &name),
                    ],
                ),
            ]
        }
        sel => vec![SectionSpec::new(
            "信息",
            vec![info_row(
                "选中",
                &format!("已选中 {} 个对象(多选批量属性留 M2)", sel.len()),
            )],
        )],
    }
}

/// X/Y 数值行:get 读节点 Affine 的平移分量(coeffs[4]/[5]),set 走
/// `History::exec(SetTransform)`(只改平移,其余系数保持)。
fn number_row(
    label: &'static str,
    doc: Entity<Document>,
    id: NodeId,
    axis: usize,
    _value: f64,
) -> RowSpec {
    RowSpec::Number {
        label: label.into(),
        binding: Binding::new(
            {
                let doc = doc.clone();
                move |cx: &App| {
                    doc.read(cx)
                        .scene
                        .read(cx)
                        .node(id)
                        .map(|node| node.transform)
                        .unwrap_or(Affine::IDENTITY)
                        .as_coeffs()[axis]
                }
            },
            set_translation(doc, id, axis),
        ),
        range: (-1.0e5, 1.0e5),
        step: 1.0,
        unit: "px".into(),
    }
}

/// 填充行:`Binding<Paint>`(Solid 域;set 经 `SetFill`,同节点连续改色
/// 合并为一步撤销,分册三 §2)。渐变填充 v0.1 由 ColorWell 显示实心预览
/// (首色标),完整渐变编辑走 widgets 的 GradientEditor = M2 接线。
fn fill_binding(
    doc: Entity<Document>,
    id: NodeId,
    _fill: Option<Paint>,
    fallback: Paint,
) -> Binding<Paint> {
    Binding::new(
        {
            let doc = doc.clone();
            move |cx: &App| {
                doc.read(cx)
                    .scene
                    .read(cx)
                    .path(id)
                    .and_then(|path| path.fill.clone())
                    .unwrap_or_else(|| fallback.clone())
            }
        },
        move |paint: Paint, cx: &mut App| {
            doc.update(cx, |d, cx| {
                let old = d.scene.read(cx).path(id).and_then(|p| p.fill.clone());
                d.exec(
                    Box::new(SetFill {
                        id,
                        old,
                        new: Some(paint),
                    }),
                    cx,
                );
            });
        },
    )
}

/// 平移分量写入闭包:`as_coeffs()[axis] = v` 后整体 exec。
fn set_translation(
    doc: Entity<Document>,
    id: NodeId,
    axis: usize,
) -> impl Fn(f64, &mut App) + 'static {
    move |value, cx| {
        doc.update(cx, |d, cx| {
            let old = d
                .scene
                .read(cx)
                .node(id)
                .map(|node| node.transform)
                .unwrap_or(Affine::IDENTITY);
            let mut coeffs = old.as_coeffs();
            coeffs[axis] = value;
            d.exec(
                Box::new(SetTransform {
                    id,
                    old,
                    new: Affine::new(coeffs),
                }),
                cx,
            );
        });
    }
}

fn info_row(label: &str, text: &str) -> RowSpec {
    RowSpec::Info {
        // SharedString 只有 From<&'static str>/From<String>,运行期 &str 走 String
        label: label.to_string().into(),
        text: text.to_string(),
    }
}
