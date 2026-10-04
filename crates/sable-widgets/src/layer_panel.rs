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
//!
//! # A7 微交互 + A4 FLIP 让位(08 迭代计划)
//!
//! - **行悬停**(分册六 §4.3 #1):面板级单一 [`HoverState`](crate::interact::HoverState)
//!   (悬停互斥)+ `on_hover` 进出事件驱动 120ms ease-out 进度,行底色 =
//!   透明 → surface_3 插值([`lerp_hsla`]);减弱动态(A8)下进度直通 0/1;
//! - **撤销脉冲**(#7):[`PulseState`] 按节点挂表,应用层撤销后调
//!   [`LayerPanel::pulse_for`],行渲染时查进度给 300ms accent 描边着色——
//!   边框常挂、无脉冲时透明(避免动画中途改布局,§4.4 只插值颜色);
//! - **行让位**(#12,A4):[`FlipTracker`] 每帧收全部根图层的行位置
//!   (y = index × 行高),重排后 160ms ease 让位。消费方式 = 行 `.mt()`:
//!   uniform_list 的 item 各自是独立布局根(gpui 0.2.2 源码核实:
//!   `item.layout_as_root` + `prepaint_at`),行内 margin 只移动本行、不牵动
//!   邻行,是最小侵入的逐行偏移通道;减弱动态下偏移恒 0;
//! - 动画运行才请求帧(悬停过渡 / 让位 / 脉冲任一在跑时
//!   `window.request_animation_frame()`,静止零帧提交)。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, Context, ElementId, Entity, Hsla, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, Render, StatefulInteractiveElement, Styled, Window, div, px,
    uniform_list,
};
use sable_foundation::scene::{NodeId, Scene};
use slotmap::Key;

use crate::anim::lerp_hsla;
use crate::flip::FlipTracker;
use crate::interact::{self, HoverState, PulseState};
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
    /// A7:悬停进度(悬停互斥 → 面板级单一状态机)+ 当前悬停行
    hover: HoverState,
    hovered_node: Option<NodeId>,
    /// A4:行让位跟踪(Rc<RefCell> 以共享进 uniform_list 闭包)
    flip: Rc<RefCell<FlipTracker>>,
    /// A7:撤销脉冲表(节点 → 脉冲;查询即清理过期项,无驻留)
    pulses: Rc<RefCell<HashMap<NodeId, PulseState>>>,
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
            hover: HoverState::new(),
            hovered_node: None,
            flip: Rc::new(RefCell::new(FlipTracker::new())),
            pulses: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// A7 撤销脉冲(分册六 §4.3 #7):command 撤销/重做后对受影响节点调用,
    /// 对应行 300ms accent 描边闪一次(重复调用从头再闪)。调用方需保证
    /// 撤销后至少触发一次重绘(撤销改场景 → 面板随场景通知重绘,常规路径
    /// 已满足);窗口内的后续帧由面板渲染时的续帧判断自驱动。
    pub fn pulse_for(&mut self, node: NodeId) {
        self.pulses
            .borrow_mut()
            .entry(node)
            .or_default()
            .begin(interact::now_ms());
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let colors = t.colors;
        let roots: Vec<NodeId> = self.scene.read(cx).iter_roots().collect();
        let selection = self.selection.clone();
        let scene = self.scene.clone();
        let weak = cx.entity().downgrade();
        let flip = self.flip.clone();
        let pulses = self.pulses.clone();

        // A7/A4 帧时钟与悬停快照(悬停互斥,单一进度值服务当前悬停行)
        let now = interact::now_ms();
        let hover_progress = self.hover.progress_at(now);
        let hovered_node = self.hovered_node;

        // A4 First/Last:对**全部**根图层报告行位置(含滚出视口者,虚拟化
        // 列表只 measure 可见行会让重排后滚入的行拿陈旧旧位触发假动画);
        // 行 y = index × 行高,与 uniform_list 的布点公式一致。
        let row_h = f64::from(LayerPanel::row_height());
        {
            let mut tracker = flip.borrow_mut();
            for (ix, &id) in roots.iter().enumerate() {
                tracker.measure(id.data().as_ffi(), ix as f64 * row_h);
            }
        }

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
        // 行内用的共享状态再克隆一份(flip/pulses 本体留给下方续帧判断)
        let (flip_rows, pulses_rows) = (flip.clone(), pulses.clone());
        let list = uniform_list("layers", count, move |range, _window, cx| {
            let scene = scene.read(cx);
            let mut rows = Vec::with_capacity(range.end - range.start);
            for ix in range {
                let Some(&id) = roots.get(ix) else { continue };
                let Some(node) = scene.node(id) else { continue };
                let (visible, locked) = (node.visible, node.locked);
                let name = node.name.clone();
                let selected = selection.contains(&id);
                // FLIP 元素键 = NodeId 的稳定 u64 重排序时不变
                let key = id.data().as_ffi();

                // A4 Invert/Play:本帧该行的让位偏移(重排检出后 160ms 衰减)
                let flip_offset = flip_rows.borrow_mut().invert_play(key, now);
                // A7:悬停高亮(TOK-04:选中行走 state-layer selected,悬停
                // 不与选中叠加)
                let hover_p = if hovered_node == Some(id) {
                    hover_progress
                } else {
                    0.0
                };
                // A7:撤销脉冲进度(查询即清理过期项);描边常挂、无脉冲透明
                let pulse_p = pulses_rows
                    .borrow_mut()
                    .entry(id)
                    .or_default()
                    .progress_at(now);

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
                // A7:悬停进出(on_hover 需要 Stateful 元素,id 用稳定节点键)
                let on_hover = move |hovered: &bool, _win: &mut Window, cx: &mut App| {
                    let _ = panel.update(cx, |this, cx| {
                        let tick = interact::now_ms();
                        if *hovered {
                            this.hovered_node = Some(id);
                            this.hover.on_enter(tick);
                        } else if this.hovered_node == Some(id) {
                            this.hovered_node = None;
                            this.hover.on_leave(tick);
                        }
                        cx.notify();
                    });
                };
                let t = theme(cx);
                let colors = t.colors;
                let row = h_flex()
                    .id(ElementId::NamedInteger("layer-row".into(), key))
                    .w_full()
                    .h(px(LayerPanel::row_height()))
                    .mt(pxv(flip_offset))
                    .px(px(SpacingTokens::XS))
                    .gap(px(SpacingTokens::XS))
                    .rounded(px(RadiusTokens::SM))
                    .border_1()
                    .border_color(if pulse_p > 0.0 {
                        colors.accent.opacity(f32v(pulse_p))
                    } else {
                        Hsla::transparent_black()
                    })
                    // TOK-04:选中 = state-layer(面板底上叠 accent 14%,深浅
                    // 同比例);悬停 = state-layer(Hover)插值(叠白/黑随主题)
                    .when(selected, |el| {
                        el.bg(interact::state_layer(
                            colors.surface_1,
                            interact::InteractState::Selected,
                            colors.accent,
                        ))
                    })
                    .when(!selected && hover_p > 0.0, |el| {
                        el.bg(lerp_hsla(
                            Hsla::transparent_black(),
                            interact::state_layer(
                                colors.surface_1,
                                interact::InteractState::Hover,
                                colors.accent,
                            ),
                            hover_p,
                        ))
                    })
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
                    )
                    .on_hover(on_hover);
                rows.push(row.into_any_element());
            }
            rows
        })
        .flex_1()
        .min_h_0()
        .bg(colors.surface_1);

        // 悬停过渡 / 让位 / 脉冲任一在跑就续帧(动画运行才请求帧,静止零帧
        // 提交;脉冲表的过期项借本次遍历清理,不驻留)
        let needs_frames = self.hover.is_running(now)
            || flip.borrow_mut().is_animating(now)
            || pulses.borrow_mut().values_mut().any(|p| p.is_active(now));
        if needs_frames {
            window.request_animation_frame();
        }

        v_flex()
            .size_full()
            .gap(px(SpacingTokens::XS))
            .p(px(SpacingTokens::XS))
            .bg(colors.surface_1)
            .child(toolbar)
            .child(list)
    }
}

/// f64 几何 → 像素(与 timeline_view 同款惯例:几何 f64,画元素前一刻降 f32;
/// 行偏移/行高均为像素级小量,截断无碍)。
#[allow(clippy::cast_possible_truncation)]
fn pxv(v: f64) -> gpui::Pixels {
    px(v as f32)
}

/// f64 动画进度 → f32(透明度插值入 GPU 域的收口,0..1 进度截断无碍)。
#[allow(clippy::cast_possible_truncation)]
fn f32v(v: f64) -> f32 {
    v as f32
}

/// 工具行小按钮(上移/下移;回调收 &mut App;A7 hover 即时 state-layer 叠加)。
fn simple_tool_button(
    label: &'static str,
    colors: crate::tokens::ColorTokens,
    on_click: Rc<dyn Fn(&mut App)>,
) -> gpui::AnyElement {
    let hover_bg = interact::state_layer(
        colors.surface_3,
        interact::InteractState::Hover,
        colors.accent,
    );
    div()
        .px(px(SpacingTokens::SM))
        .h(px(LayerPanel::row_height()))
        .rounded(px(RadiusTokens::SM))
        .bg(colors.surface_3)
        .text_size(px(FONT_SIZE_CAPTION))
        .text_color(colors.text_secondary)
        .cursor_pointer()
        .hover(move |style| style.bg(hover_bg))
        .child(label)
        .on_mouse_down(MouseButton::Left, move |_ev: &MouseDownEvent, _win, cx| {
            on_click(cx)
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sable_foundation::scene::NodeContent;

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
