//! 输入组件分组:NumberField(编辑态/拖拽/单位/范围)+ ColorWell/ColorWheel
//! + GradientEditor(任务 4.3 分组 1~3)。

use sable::core::prelude::{GradientStop, Paint, Rgba8};
use sable::gpui::{
    App, AppContext as _, Context, ElementId, Entity, IntoElement, ParentElement, Render,
    StatefulInteractiveElement as _, Styled, Window, div, px,
};
use sable::widgets::binding::Binding;
use sable::widgets::color::{ColorWell, ColorWheel};
use sable::widgets::gradient_editor::{GradientEditor, stops_of};
use sable::widgets::number_field::NumberField;
use sable::widgets::prelude::{SpacingTokens, h_flex, v_flex};
use sable::widgets::property_row::{PropertyRow, section};
use sable::widgets::theme::theme;
use sable::widgets::tokens::{FONT_SIZE_BODY, hsla_from_rgba8, rgba8_from_hsla};

use crate::ui::{card, story_button};

// —— NumberField 演示:DemoState(值 + 撤销栈)——

/// 演示文档态:四个数值 + 一条撤销栈(展示"提交一次 = 一步撤销")。
pub struct DemoState {
    values: [f64; 4],
    undo: Vec<(usize, f64)>,
}

impl DemoState {
    /// 写值(变化才入栈;undo 语义由本示例闭包负责,同 Binding 契约)。
    fn set(&mut self, index: usize, value: f64) {
        if self.values[index] != value {
            self.undo.push((index, self.values[index]));
            self.values[index] = value;
        }
    }

    /// 撤销一步。
    fn undo_step(&mut self) {
        if let Some((index, value)) = self.undo.pop() {
            self.values[index] = value;
        }
    }
}

/// 字段 i 的受控绑定(读 = DemoState.values[i];写 = DemoState::set)。
fn demo_binding(state: &Entity<DemoState>, index: usize) -> Binding<f64> {
    let for_get = state.clone();
    let for_set = state.clone();
    Binding::new(
        move |cx: &App| for_get.read(cx).values[index],
        move |value: f64, cx: &mut App| for_set.update(cx, |demo, _| demo.set(index, value)),
    )
}

/// NumberField 分组视图。
pub struct NumberSection {
    demo: Entity<DemoState>,
}

impl NumberSection {
    pub fn new(cx: &mut App) -> Entity<Self> {
        let demo = cx.new(|_| DemoState {
            values: [12.0, 48.0, 90.0, 25.0],
            undo: Vec::new(),
        });
        cx.new(|cx| {
            cx.observe(&demo, |_, _, cx| cx.notify()).detach();
            cx.observe_global::<sable::widgets::theme::SableTheme>(|_, cx| cx.notify())
                .detach();
            NumberSection { demo }
        })
    }
}

/// 四个实例的规格(标签 / 步长 / 单位 / 范围)。
const FIELDS: [(&str, f64, &str, (f64, f64)); 4] = [
    ("编辑态(双击)", 1.0, "", (0.0, 100.0)),
    ("拖拽 Scrub", 0.5, "", (-180.0, 180.0)),
    ("单位后缀", 1.0, "px", (0.0, 800.0)),
    ("范围钳制", 1.0, "%", (0.0, 100.0)),
];

impl Render for NumberSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // ColorTokens 是 Copy:取值即可,避免把 cx 借用拖进后续 listener
        let colors = theme(cx).colors;
        let mut rows = v_flex().gap(px(SpacingTokens::XS));

        for (i, (label, step, unit, range)) in FIELDS.iter().enumerate() {
            let field = cx.new(|_| {
                NumberField::new(demo_binding(&self.demo, i))
                    .range(range.0, range.1)
                    .step(*step)
                    .unit(unit)
                    .element_id(ElementId::named_usize("story-number", i))
            });
            rows = rows.child(PropertyRow::new(*label).control(field));
        }

        let undo_len = self.demo.read(cx).undo.len();
        let undo_label = format!("撤销栈:{undo_len} 步(编辑态提交一次 = 一步;拖拽经 merge 合并)");

        let content = v_flex()
            .gap(px(SpacingTokens::MD))
            .child(section("几何", rows))
            .child(
                h_flex()
                    .gap(px(SpacingTokens::SM))
                    .child(
                        story_button(cx, "nf-undo", "撤销一步").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.demo.update(cx, |demo, _| demo.undo_step());
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        div()
                            .text_size(px(FONT_SIZE_BODY))
                            .text_color(colors.text_secondary)
                            .child(undo_label),
                    ),
            );

        card(
            cx,
            "NumberField — 数值拖拽框",
            "双击 / Enter 进入真文本编辑(0-9 . e +/- 键入、←→/Home/End 移动光标、Enter 提交、Esc 回旧值、Tab 提交并跳下一字段);按住左右拖 scrub(Shift ×10 / Alt ×0.1);滚轮与 ↑↓ 步进。",
            content,
        )
    }
}

// —— 色彩演示:ColorWell + ColorWheel 同色绑定 ——

/// 共享颜色状态(Well 展示、Wheel 编辑,双向同步)。
struct ColorState {
    color: Rgba8,
}

/// 色彩分组视图。
pub struct ColorSection {
    state: Entity<ColorState>,
    wheel: Entity<ColorWheel>,
}

impl ColorSection {
    pub fn new(cx: &mut App) -> Entity<Self> {
        let state = cx.new(|_| ColorState {
            color: [79, 159, 255, 255], // 演示初值(内容数据,非 UI 配色)
        });
        let binding = {
            let for_get = state.clone();
            let for_set = state.clone();
            Binding::new(
                move |cx: &App| for_get.read(cx).color,
                move |value: Rgba8, cx: &mut App| for_set.update(cx, |s, _| s.color = value),
            )
        };
        let wheel = cx.new(|_| ColorWheel::new(binding));
        cx.new(|cx| {
            cx.observe(&state, |_, _, cx| cx.notify()).detach();
            cx.observe(&wheel, |_, _, cx| cx.notify()).detach();
            cx.observe_global::<sable::widgets::theme::SableTheme>(|_, cx| cx.notify())
                .detach();
            ColorSection { state, wheel }
        })
    }
}

impl Render for ColorSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let color = self.state.read(cx).color;
        let state = self.state.clone();
        let well = ColorWell::new(color).on_click(move |cx: &mut App| {
            // 点击色井 = 色相旋转 60°(演示 Well → Wheel 的同色联动)
            state.update(cx, |s, _| {
                let mut hsla = hsla_from_rgba8(s.color);
                hsla.h = (hsla.h + 1.0 / 6.0) % 1.0;
                s.color = rgba8_from_hsla(hsla);
            });
        });

        let content = v_flex()
            .gap(px(SpacingTokens::MD))
            .child(self.wheel.clone())
            .child(
                h_flex().gap(px(SpacingTokens::SM)).child(well).child(
                    div()
                        .text_size(px(FONT_SIZE_BODY))
                        .text_color(theme(cx).colors.text_secondary)
                        .child(format!(
                            "#{:02X}{:02X}{:02X}{:02X}",
                            color[0], color[1], color[2], color[3]
                        )),
                ),
            );

        card(
            cx,
            "ColorWell + ColorWheel — 取色",
            "色轮:外环取色相、内方取饱和度/明度;色井展示当前值,点击色相旋转 60°(二者绑定同一状态)。",
            content,
        )
    }
}

// —— 渐变演示:GradientEditor ——

/// 渐变分组视图。
pub struct GradientSection {
    paint: Entity<Paint>,
    editor: Entity<GradientEditor>,
}

impl GradientSection {
    pub fn new(cx: &mut App) -> Entity<Self> {
        let paint = cx.new(|_| Paint::LinearGradient {
            start: [0.0, 0.0],
            end: [1.0, 0.0],
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: [79, 159, 255, 255],
                },
                GradientStop {
                    offset: 1.0,
                    color: [255, 140, 90, 255],
                },
            ],
        });
        let binding = {
            let for_get = paint.clone();
            let for_set = paint.clone();
            Binding::new(
                move |cx: &App| for_get.read(cx).clone(),
                move |value: Paint, cx: &mut App| for_set.update(cx, |p, _| *p = value),
            )
        };
        let editor = cx.new(|_| GradientEditor::new(binding));
        cx.new(|cx| {
            cx.observe(&paint, |_, _, cx| cx.notify()).detach();
            cx.observe(&editor, |_, _, cx| cx.notify()).detach();
            cx.observe_global::<sable::widgets::theme::SableTheme>(|_, cx| cx.notify())
                .detach();
            GradientSection { paint, editor }
        })
    }
}

impl Render for GradientSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let stop_count = stops_of(&self.paint.read(cx).clone()).len();
        let content = v_flex()
            .gap(px(SpacingTokens::MD))
            .child(self.editor.clone())
            .child(
                h_flex()
                    .gap(px(SpacingTokens::SM))
                    .child(
                        story_button(cx, "gradient-add", "+ 色标").on_click(cx.listener(
                            |this, _, window, cx| {
                                this.editor.update(cx, |editor, cx| {
                                    editor.add_stop(window, cx);
                                });
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        story_button(cx, "gradient-remove", "− 色标").on_click(cx.listener(
                            |this, _, window, cx| {
                                this.editor.update(cx, |editor, cx| {
                                    editor.remove_selected(window, cx);
                                });
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        div()
                            .text_size(px(FONT_SIZE_BODY))
                            .text_color(theme(cx).colors.text_secondary)
                            .child(format!("{stop_count} 个色标(拖动色标调位置)")),
                    ),
            );

        card(
            cx,
            "GradientEditor — 渐变 stops 编辑",
            "拖动色标改 offset、点选后经按钮增删;色标颜色经应用层接 ColorWheel(v0.1 预置双色)。",
            content,
        )
    }
}
