//! 面板组件分组:LayerPanel(20 行假场景 + 上移/下移触发 FLIP 让位动画)
//! + TimelineView(假时间轴)+ CurvePreview(任务 4.3 分组 4~5)。

use std::cell::RefCell;
use std::rc::Rc;

use sable::core::prelude::{
    Command as _, History, NodeContent, NodeId, Paint, PathNode, Reparent, Scene,
};
use sable::gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render,
    StatefulInteractiveElement as _, Styled, Window, div, hsla, px,
};
use sable::video::model::Easing;
use sable::video::model::{AssetRef, Timeline, TrackKind};
use sable::widgets::curve_editor::CurvePreview;
use sable::widgets::layer_panel::LayerPanel;
use sable::widgets::prelude::{SpacingTokens, h_flex, v_flex};
use sable::widgets::theme::theme;
use sable::widgets::timeline_view::TimelineView;
use sable::widgets::tokens::FONT_SIZE_BODY;
use sable::widgets::tokens::rgba8_from_hsla;

use crate::ui::{card, story_button};

// —— 图层分组:LayerPanel + FlipTracker 让位 ——

/// 假场景:20 行矩形图层(渲染顺序 = roots 序)。
fn seed_layers(scene: &mut Scene) {
    for i in 0..20u32 {
        let content = NodeContent::Path(PathNode {
            path: rect_path(24.0 + f64::from(i)),
            fill: Some(Paint::Solid(layer_color(i))),
            stroke: None,
        });
        let _ = scene.add_node(None, format!("图层 {i:02}"), content);
    }
}

fn rect_path(size: f64) -> sable::kurbo::BezPath {
    let mut path = sable::kurbo::BezPath::new();
    path.move_to((0.0, 0.0));
    path.line_to((size, 0.0));
    path.line_to((size, size));
    path.line_to((0.0, size));
    path.close_path();
    path
}

/// 图层示意色(内容数据:沿色相环取样;非 UI 配色,不走 token 语义色)。
fn layer_color(i: u32) -> [u8; 4] {
    rgba8_from_hsla(hsla(((i as f32) * 0.05 + 0.55) % 1.0, 0.55, 0.55, 1.0))
}

/// 图层分组视图:上移/下移走 Reparent 命令(经本地 History,可撤销),
/// 行顺序变化时 LayerPanel 内置的 FlipTracker 自动播放 160ms 让位动画(A4)。
pub struct LayersSection {
    scene: Entity<Scene>,
    panel: Entity<LayerPanel>,
    history: Rc<RefCell<History>>,
}

impl LayersSection {
    pub fn new(cx: &mut App) -> Entity<Self> {
        let scene = cx.new(|_| {
            let mut scene = Scene::new();
            seed_layers(&mut scene);
            scene
        });
        let history: Rc<RefCell<History>> = Rc::new(RefCell::new(History::new()));

        // 上移/下移:同一父层内换位(roots 内交换下标),命令化 + 可撤销
        let scene_for_up = scene.clone();
        let history_for_up = history.clone();
        let scene_for_down = scene.clone();
        let history_for_down = history.clone();
        let scene_for_panel = scene.clone();

        let panel = cx.new(|_| {
            LayerPanel::new(scene_for_panel)
                .on_select(|_id: NodeId, _shift: bool, _cx: &mut App| {
                    // 选中态由面板自持;示例不需要跨面板同步
                })
                .on_move_up(move |id: NodeId, cx: &mut App| {
                    move_within_parent(cx, &scene_for_up, &history_for_up, id, true);
                })
                .on_move_down(move |id: NodeId, cx: &mut App| {
                    move_within_parent(cx, &scene_for_down, &history_for_down, id, false);
                })
        });

        cx.new(|cx| {
            cx.observe(&scene, |_, _, cx| cx.notify()).detach();
            cx.observe_global::<sable::widgets::theme::SableTheme>(|_, cx| cx.notify())
                .detach();
            LayersSection {
                scene,
                panel,
                history,
            }
        })
    }
}

/// 在父层内平移一个节点(true = 上移,即渲染序提前)。
fn move_within_parent(
    cx: &mut App,
    scene: &Entity<Scene>,
    history: &Rc<RefCell<History>>,
    id: NodeId,
    up: bool,
) {
    scene.update(cx, |scene, _| {
        let Ok((parent, index)) = scene.position(id) else {
            return;
        };
        let new_index = if up {
            index.saturating_sub(1)
        } else {
            (index + 1).min(sibling_count(scene, parent).saturating_sub(1))
        };
        if new_index == index {
            return;
        }
        if let Ok(cmd) = Reparent::capture(scene, id, parent, Some(new_index)) {
            history.borrow_mut().exec(cmd.boxed(), scene);
        }
    });
    // LayerPanel 不观察场景实体(读模式):全窗刷新驱动重渲 + FLIP 测量
    cx.refresh_windows();
}

/// 同层兄弟数(parent = None 时为 roots 数)。
fn sibling_count(scene: &Scene, parent: Option<NodeId>) -> usize {
    match parent {
        Some(p) => scene.node(p).map(|n| n.children.len()).unwrap_or(0),
        None => scene.roots.len(),
    }
}

impl Render for LayersSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let undo_steps = self.history.borrow().undo_len();
        let content =
            v_flex()
                .gap(px(SpacingTokens::MD))
                // uniform_list 需要有界视口:外层定高
                .child(div().h(px(320.)).child(self.panel.clone()))
                .child(
                    h_flex()
                        .gap(px(SpacingTokens::SM))
                        .child(story_button(cx, "layers-undo", "撤销一次移动").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.scene.update(cx, |scene, _| {
                                    this.history.borrow_mut().undo(scene);
                                });
                                cx.refresh_windows();
                            }),
                        ))
                        .child(
                            div()
                                .text_size(px(FONT_SIZE_BODY))
                                .text_color(theme(cx).colors.text_secondary)
                                .child(format!("{undo_steps} 步可撤销")),
                        ),
                );

        card(
            cx,
            "LayerPanel — 图层面板(FLIP 让位动画)",
            "20 行假场景;行内上移/下移触发真实 reorder,其余行的 160ms 让位由 FlipTracker(A4)驱动;减弱动态开时直接落位。",
            content,
        )
    }
}

// —— 时间轴分组:TimelineView + CurvePreview ——

/// 假时间轴:2 视频轨 + 1 音频轨,3 个彩条 clip(总长 9s)。
fn seed_timeline() -> Timeline {
    let mut timeline = Timeline::new();
    timeline.add_track(TrackKind::Video);
    timeline.add_track(TrackKind::Video);
    timeline.add_track(TrackKind::Audio);
    let clips = [
        (0usize, "story://clip-a", 0u64, 4_000u64),
        (0, "story://clip-b", 4_000, 5_000),
        (1, "story://clip-c", 2_000, 6_000),
        (2, "story://tone", 1_000, 7_000),
    ];
    for (track, name, start, duration) in clips {
        let _ = timeline.place_clip(track, AssetRef::new(name, 1), start, duration);
    }
    timeline
}

/// 时间轴分组视图(静态展示:播放头固定,seek/拖 clip 回调为默认空操作)。
pub struct TimelineSection {
    view: Entity<TimelineView>,
}

impl TimelineSection {
    pub fn new(cx: &mut App) -> Entity<Self> {
        let timeline = cx.new(|_| seed_timeline());
        let view = cx.new(|_| TimelineView::new(timeline.clone()));
        cx.new(|cx| {
            cx.observe_global::<sable::widgets::theme::SableTheme>(|_, cx| cx.notify())
                .detach();
            TimelineSection { view }
        })
    }
}

impl Render for TimelineSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = v_flex()
            .gap(px(SpacingTokens::MD))
            .child(self.view.clone())
            .child(
                h_flex()
                    .gap(px(SpacingTokens::LG))
                    .child(
                        v_flex()
                            .gap(px(SpacingTokens::XS))
                            .child(CurvePreview::new(Easing::OutCubic, 160.0, 90.0))
                            .child(
                                div()
                                    .text_size(px(FONT_SIZE_BODY))
                                    .text_color(theme(cx).colors.text_secondary)
                                    .child("OutCubic"),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap(px(SpacingTokens::XS))
                            .child(CurvePreview::new(Easing::InOutCubic, 160.0, 90.0))
                            .child(
                                div()
                                    .text_size(px(FONT_SIZE_BODY))
                                    .text_color(theme(cx).colors.text_secondary)
                                    .child("InOutCubic"),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap(px(SpacingTokens::XS))
                            .child(CurvePreview::new(Easing::Linear, 160.0, 90.0))
                            .child(
                                div()
                                    .text_size(px(FONT_SIZE_BODY))
                                    .text_color(theme(cx).colors.text_secondary)
                                    .child("Linear"),
                            ),
                    ),
            );

        card(
            cx,
            "TimelineView + CurvePreview — 时间轴与缓动曲线",
            "假时间轴(2 视频轨 + 1 音频轨,4 个 clip):播放头红线、clip 拖拽吸附在组件内实现;曲线预览为缓动函数形状。",
            content,
        )
    }
}
