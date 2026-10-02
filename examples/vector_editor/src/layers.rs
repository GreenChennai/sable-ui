//! 图层面板宿主:widgets `LayerPanel` 挂进 dock 左侧。
//!
//! widgets 真实签名(2026-10 以 crates/sable-widgets/src/layer_panel.rs
//! 为准):`LayerPanel::new(Entity<Scene>)`(无 cx)+ 消费式 builder
//! `.on_select(Fn(NodeId, bool, &mut App))` / `.on_toggle_visible(Fn(NodeId,
//! &mut App))` / `.on_move_up/.on_move_down(Fn(NodeId, &mut App))`;
//! 选中高亮经 `set_selection(Vec<NodeId>)` 由应用层回推。
//!
//! 本宿主的职责:回调 → `Document::exec` 出撤销步(AGENTS.md §3.1);
//! 每帧把 Document 的选中集镜像进面板。锁 toggle(core 命令集无
//! SetLock,v0.1)不挂接,面板默认空操作。

use gpui::{
    App, AppContext as _, Entity, IntoElement, ParentElement as _, Render, Styled as _, div,
};
use sable::core::command::{Command as _, Reparent, SetVisibility};
use sable::core::scene::NodeId;
use sable::gpui;
use sable::widgets::layer_panel::LayerPanel;

use crate::document::Document;

/// 图层面板宿主。
pub struct LayerHost {
    panel: Entity<LayerPanel>,
    doc: Entity<Document>,
}

impl LayerHost {
    /// 构造宿主并挂接全部回调(场景实体与 Document 共享同一份)。
    pub fn new(doc: &Entity<Document>, cx: &mut App) -> Entity<Self> {
        let scene = doc.read(cx).scene.clone();

        let doc_select = doc.clone();
        let doc_visible = doc.clone();
        let doc_up = doc.clone();
        let doc_down = doc.clone();
        let panel = cx.new(|_| {
            LayerPanel::new(scene)
                .on_select(move |id, shift, cx| {
                    doc_select.update(cx, |d, _| {
                        d.selection =
                            sable::widgets::layer_panel::apply_select(&d.selection, id, shift);
                    });
                })
                .on_toggle_visible(move |id, cx| {
                    doc_visible.update(cx, |d, cx| {
                        let old = d
                            .scene
                            .read(cx)
                            .node(id)
                            .map(|node| node.visible)
                            .unwrap_or(true);
                        d.exec(Box::new(SetVisibility { id, old, new: !old }), cx);
                    });
                })
                .on_move_up(move |id, cx| move_within_roots(&doc_up, id, -1, cx))
                .on_move_down(move |id, cx| move_within_roots(&doc_down, id, 1, cx))
        });

        cx.new(|cx| {
            // Document 任一变化(含选中集镜像)触发本宿主重渲
            cx.observe(doc, |_, _, cx| cx.notify()).detach();
            Self {
                panel,
                doc: doc.clone(),
            }
        })
    }
}

/// roots 内上/下移一格(同父 Reparent 命令语义,可撤销;LayerPanel v0.1
/// 只渲染 roots,故仅处理根级节点)。
fn move_within_roots(doc: &Entity<Document>, id: NodeId, delta: isize, cx: &mut App) {
    doc.update(cx, |d, cx| {
        let target = {
            let scene = d.scene.read(cx);
            let Ok((parent, index)) = scene.position(id) else {
                return;
            };
            if parent.is_some() {
                return;
            }
            let next = index as isize + delta;
            if next < 0 || next >= scene.iter_roots().count() as isize {
                return;
            }
            next as usize
        };
        if let Ok(cmd) = Reparent::capture(d.scene.read(cx), id, None, Some(target)) {
            d.exec(cmd.boxed(), cx);
        }
    });
}

impl Render for LayerHost {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        // 选中集真相在 Document(画布工具镜像),回推面板画高亮
        let selection = self.doc.read(cx).selection.clone();
        self.panel
            .update(cx, |panel, _| panel.set_selection(selection));
        div().size_full().child(self.panel.clone())
    }
}
