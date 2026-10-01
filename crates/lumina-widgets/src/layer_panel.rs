//! 图层面板 LayerPanel(分册四 §1 的 v0.1 子集)。
//!
//! uniform_list(gpui 0.2.2)虚拟化渲染**根图层拍平列表**(万级图层不卡);
//! 每行 = 眼睛 toggle + 锁 icon + 缩略图占位色块 + 名字,行点击 → 选择回调
//! (Shift 加选)。
//!
//! # v0.1 边界(全部 doc 注明,留 M2)
//! - **拖拽排序 = M2**(需要 on_drag/拖放目标系统);替代交互:
//!   工具行的"上移/下移"按钮(`on_move_up`/`on_move_down` 回调);
//! - 缩略图 = 占位色块,vello_cpu 离屏 16×16 缓存 = M2(分册四 §1);
//! - 双击改名 = M2(需文本输入,同 NumberField 的输入态约束);
//! - 仅渲染 roots(拍平平行);子树展开/折叠 = M2;
//! - 眼睛/锁图标 = 几何色块占位(gpui-component 的 Icon 跑在 gpui-pre
//!   类型世界,不可用;Lucide 接入 = M2)。
//!
//! # 回调约定
//! 全部 `Rc<dyn Fn(...)>`(可克隆进 uniform_list 的 'static 闭包),签名
//! 以 `&mut App` 收尾;文档修改与撤销由应用层负责(与 Binding 同纪律)。

use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, Context, Entity, Hsla, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Render, Styled, Window, div, px, uniform_list,
};
use lumina_core::scene::{NodeId, Scene};

use crate::theme::theme;
use crate::tokens::{
    FONT_SIZE_BODY, FONT_SIZE_CAPTION, RadiusTokens, SpacingTokens, control_height, h_flex, v_flex,
};

/// 列表行的估行高基准(正文字号 12 的行高)。
const ROW_LINE_HEIGHT_PX: f32 = 16.0;
/// 行垂直内边距(16 + 2×6 = 28,与属性行同高)。
const ROW_V_PADDING_PX: f32 = 6.0;

/// 眼睛 toggle 回调:`(NodeId, &mut App)`(全部 `Rc<dyn Fn>`,可克隆进
/// uniform_list 的 'static 闭包,见模块 doc「回调约定」)。
pub type ToggleVisibleFn = Rc<dyn Fn(NodeId, &mut App)>;
/// 锁 toggle 回调:`(NodeId, &mut App)`。
pub type ToggleLockFn = Rc<dyn Fn(NodeId, &mut App)>;
/// 行选择回调:`(NodeId, shift 是否加选, &mut App)`。
pub type SelectFn = Rc<dyn Fn(NodeId, bool, &mut App)>;
/// 上移一格回调:`(NodeId, &mut App)`(拖拽排序的替代交互,v0.1)。
pub type MoveUpFn = Rc<dyn Fn(NodeId, &mut App)>;
/// 下移一格回调:`(NodeId, &mut App)`。
pub type MoveDownFn = Rc<dyn Fn(NodeId, &mut App)>;

/// 图层面板(有状态 Entity):
/// `cx.new(|cx| LayerPanel::new(scene_entity).on_select(...).on_toggle_visible(...))`
pub struct LayerPanel {
    /// 场景数据源(只读;修改走应用层回调)
    scene: Entity<Scene>,
    /// 选中集本地副本(真相在应用层;这里仅作高亮显示与按钮目标)
    selection: Vec<NodeId>,
    on_toggle_visible: ToggleVisibleFn,
    on_toggle_lock: ToggleLockFn,
    on_select: SelectFn,
    on_move_up: MoveUpFn,
    on_move_down: MoveDownFn,
}

impl LayerPanel {
    /// 绑定场景的空面板(选中为空、回调为空操作)。
    pub fn new(scene: Entity<Scene>) -> Self {
        LayerPanel {
            scene,
            selection: Vec::new(),
            on_toggle_visible: Rc::new(|_, _| {}),
            on_toggle_lock: Rc::new(|_, _| {}),
            on_select: Rc::new(|_, _, _| {}),
            on_move_up: Rc::new(|_, _| {}),
            on_move_down: Rc::new(|_, _| {}),
        }
    }

    /// 应用层同步选中集(渲染高亮用)。
    pub fn set_selection(&mut self, selection: Vec<NodeId>) {
        self.selection = selection;
    }

    /// 行点击(Shift = 加选)。
    pub fn on_select(mut self, f: impl Fn(NodeId, bool, &mut App) + 'static) -> Self {
        self.on_select = Rc::new(f);
        self
    }

    /// 眼睛 toggle。
    pub fn on_toggle_visible(mut self, f: impl Fn(NodeId, &mut App) + 'static) -> Self {
        self.on_toggle_visible = Rc::new(f);
        self
    }

    /// 锁 toggle。
    pub fn on_toggle_lock(mut self, f: impl Fn(NodeId, &mut App) + 'static) -> Self {
        self.on_toggle_lock = Rc::new(f);
        self
    }

    /// 上移一格(拖拽排序的替代交互,v0.1)。
    pub fn on_move_up(mut self, f: impl Fn(NodeId, &mut App) + 'static) -> Self {
        self.on_move_up = Rc::new(f);
        self
    }

    /// 下移一格。
    pub fn on_move_down(mut self, f: impl Fn(NodeId, &mut App) + 'static) -> Self {
        self.on_move_down = Rc::new(f);
        self
    }

    /// 行高(派生制):max(26, 16 + 12) = 28。
    pub fn row_height() -> f32 {
        control_height(
            crate::tokens::HEIGHT_DEFAULT,
            ROW_LINE_HEIGHT_PX,
            ROW_V_PADDING_PX,
        )
    }
}

/// 选择运算(纯函数):Shift = 并集加选;普通点击 = 替换为单选。
pub fn apply_select(current: &[NodeId], id: NodeId, shift: bool) -> Vec<NodeId> {
    if shift {
        let mut next = current.to_vec();
        if !next.contains(&id) {
            next.push(id);
        }
        next
    } else {
        vec![id]
    }
}

impl Render for LayerPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let colors = t.colors;
        let roots: Vec<NodeId> = self.scene.read(cx).iter_roots().collect();
        let selection = self.selection.clone();
        let scene = self.scene.clone();
        let weak = cx.entity().downgrade();

        // 工具行:上移/下移(拖拽排序 = M2)+ 选中计数
        let first_selected = self.selection.first().copied();
        let (up, down, first) = (
            self.on_move_up.clone(),
            self.on_move_down.clone(),
            first_selected,
        );
        let toolbar = h_flex()
            .gap(px(SpacingTokens::XS))
            .child(simple_tool_button(
                "上移",
                colors,
                Rc::new(move |cx: &mut App| {
                    if let Some(id) = first {
                        up(id, cx);
                    }
                }),
            ))
            .child(simple_tool_button(
                "下移",
                colors,
                Rc::new(move |cx: &mut App| {
                    if let Some(id) = first {
                        down(id, cx);
                    }
                }),
            ))
            .child(
                div()
                    .flex_1()
                    .text_size(px(FONT_SIZE_CAPTION))
                    .text_color(colors.text_disabled)
                    .child(format!("{} 层", roots.len())),
            );

        // 虚拟化列表(gpui 0.2.2:闭包收 (range, window, app),无 view 参数)
        let count = roots.len();
        let list = uniform_list("layers", count, move |range, _window, cx| {
            let scene = scene.read(cx);
            let mut rows = Vec::with_capacity(range.end - range.start);
            for ix in range {
                let Some(&id) = roots.get(ix) else { continue };
                let Some(node) = scene.node(id) else { continue };
                let (visible, locked) = (node.visible, node.locked);
                let name = node.name.clone();
                let selected = selection.contains(&id);

                let panel = weak.clone();
                let on_select = {
                    // 从 weak 中拿回调需要进实体,行点击回调经实体转发
                    let panel = panel.clone();
                    move |ev: &MouseDownEvent, _win: &mut Window, cx: &mut App| {
                        let shift = ev.modifiers.shift;
                        let _ = panel.update(cx, |this, cx| (this.on_select)(id, shift, cx));
                    }
                };
                let eye = {
                    let panel = panel.clone();
                    move |_ev: &MouseDownEvent, _win: &mut Window, cx: &mut App| {
                        let _ = panel.update(cx, |this, cx| (this.on_toggle_visible)(id, cx));
                    }
                };
                let lock = {
                    let panel = panel.clone();
                    move |_ev: &MouseDownEvent, _win: &mut Window, cx: &mut App| {
                        let _ = panel.update(cx, |this, cx| (this.on_toggle_lock)(id, cx));
                    }
                };
                let t = theme(cx);
                let colors = t.colors;
                rows.push(
                    h_flex()
                        .w_full()
                        .h(px(LayerPanel::row_height()))
                        .px(px(SpacingTokens::XS))
                        .gap(px(SpacingTokens::XS))
                        .rounded(px(RadiusTokens::SM))
                        .when(selected, |el| el.bg(colors.accent_muted))
                        // 眼睛:填充方块 = 可见 / 透明 = 隐藏
                        .child(
                            div()
                                .size(px(10.0))
                                .rounded(px(RadiusTokens::SM))
                                .border_1()
                                .border_color(colors.text_secondary)
                                .bg(if visible {
                                    colors.text_secondary
                                } else {
                                    Hsla::transparent_black()
                                })
                                .cursor_pointer()
                                .on_mouse_down(MouseButton::Left, eye),
                        )
                        // 锁:填充 = 锁定
                        .child(
                            div()
                                .size(px(10.0))
                                .rounded(px(RadiusTokens::SM))
                                .border_1()
                                .border_color(colors.text_disabled)
                                .bg(if locked {
                                    colors.warning
                                } else {
                                    Hsla::transparent_black()
                                })
                                .cursor_pointer()
                                .on_mouse_down(MouseButton::Left, lock),
                        )
                        // 缩略图占位色块(vello_cpu 缩略图 = M2)
                        .child(
                            div()
                                .size(px(16.0))
                                .rounded(px(RadiusTokens::SM))
                                .bg(colors.surface_3),
                        )
                        // 名字 = 行选择热区(避免与眼睛/锁的事件冲突)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .text_size(px(FONT_SIZE_BODY))
                                .text_color(if selected {
                                    colors.text_primary
                                } else {
                                    colors.text_secondary
                                })
                                .child(name)
                                .on_mouse_down(MouseButton::Left, on_select),
                        ),
                );
            }
            rows
        })
        .flex_1()
        .min_h_0()
        .bg(colors.surface_1);

        v_flex()
            .size_full()
            .gap(px(SpacingTokens::XS))
            .p(px(SpacingTokens::XS))
            .bg(colors.surface_1)
            .child(toolbar)
            .child(list)
    }
}

/// 工具行小按钮(上移/下移;回调收 &mut App)。
fn simple_tool_button(
    label: &'static str,
    colors: crate::tokens::ColorTokens,
    on_click: Rc<dyn Fn(&mut App)>,
) -> gpui::AnyElement {
    div()
        .px(px(SpacingTokens::SM))
        .h(px(LayerPanel::row_height()))
        .rounded(px(RadiusTokens::SM))
        .bg(colors.surface_3)
        .text_size(px(FONT_SIZE_CAPTION))
        .text_color(colors.text_secondary)
        .cursor_pointer()
        .child(label)
        .on_mouse_down(MouseButton::Left, move |_ev: &MouseDownEvent, _win, cx| {
            on_click(cx)
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::scene::NodeContent;

    #[test]
    fn apply_select_replaces_and_shift_unions() {
        let mut scene = Scene::new();
        let a = scene.add_node(None, "A", NodeContent::Group).expect("a");
        let b = scene.add_node(None, "B", NodeContent::Group).expect("b");
        let c = scene.add_node(None, "C", NodeContent::Group).expect("c");

        // 普通点击 = 单选替换
        assert_eq!(apply_select(&[a], b, false), vec![b]);
        // Shift = 加选去重
        assert_eq!(apply_select(&[a, b], c, true), vec![a, b, c]);
        assert_eq!(
            apply_select(&[a, b], b, true),
            vec![a, b],
            "重复 Shift 不重复加入"
        );
        // 空选 + Shift = 单加
        assert_eq!(apply_select(&[], c, true), vec![c]);
    }

    #[test]
    fn row_height_matches_property_row() {
        assert_eq!(LayerPanel::row_height(), 28.0);
        assert_eq!(
            LayerPanel::row_height(),
            crate::property_row::PropertyRow::row_height()
        );
    }

    // 注:LayerPanel/Entity 的构造与渲染路径需要 gpui 运行时(TestAppContext
    // 依赖未启用的 gpui/test-support feature),纯函数部分以外的验收由应用层
    // 冒烟测试覆盖(与 Binding 同策略)。
}
