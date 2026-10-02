//! 宿主视图 `EditorApp`:工具栏(V/P/H/撤销/重做)+ DockArea,动作与
//! 快捷键(docs/03 §4.1),以及 M0 的 doc↔canvas 镜像策略(见 document.rs)。
//!
//! # 样板来源
//!
//! 窗口生命周期/Root/DockArea 的组装形态照 gpui-component 仓库 story 示例的
//! 公共形态(`cx.open_window` → 业务视图 → `gpui_component::Root` 包根),
//! API 均以 gpui 0.2.2 / gpui-component 0.7.0 本地源码核实为准。

use sable::canvas::gpui_element::{CanvasTool, SableCanvas};
use sable::canvas::tool::{HandTool, PenTool, SelectTool};
use sable::dock::{SablePanel, WorkspacePresets};
use sable::gpui;
use sable::gpui::InteractiveElement as _;
use sable::gpui::{
    App, AppContext as _, ClickEvent, Context, Div, Entity, FocusHandle, Focusable, IntoElement,
    KeyBinding, ParentElement as _, Render, Styled as _, Window, actions, div, px,
};
use sable::gpui_component::button::Button;
use sable::gpui_component::dock::DockArea;

use crate::document::Document;
use crate::inspector::InspectorHost;
use crate::layers::LayerHost;
use crate::palette::Palette;

// 示例动作(命名空间 vector_editor,不与 gpui-component 的动作冲突)。
actions!(vector_editor, [Undo, Redo, ToolSelect, ToolPen, ToolHand]);

/// 全局键位(类 Illustrator 习惯;context = None 表示窗口全域生效)。
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-z", Undo, None),
        KeyBinding::new("ctrl-shift-z", Redo, None),
        KeyBinding::new("v", ToolSelect, None),
        KeyBinding::new("p", ToolPen, None),
        KeyBinding::new("h", ToolHand, None),
    ]);
}

/// 窗口参数(集中一处,main.rs 调用)。
pub fn window_options() -> gpui::WindowOptions {
    gpui::WindowOptions {
        titlebar: Some(gpui::TitlebarOptions {
            title: Some("Sable · 迷你 Illustrator(M0)".into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// 组装业务根视图:文档 → 画布 → 三段式 DockArea → EditorApp。
/// 在 `cx.open_window` 的 build 闭包里调用(cx 处于 `&mut App` 阶段)。
pub fn build_editor(window: &mut Window, cx: &mut App) -> Entity<EditorApp> {
    let palette = Palette::get(cx);

    // 1. 文档(权威编辑态)+ 初始内容(打开文档语义,不进撤销栈)
    let doc = Document::create(cx);
    let scene_handle = doc.read(cx).scene.clone();
    scene_handle.update(cx, |scene, _| {
        crate::seed::seed_initial_scene(scene, &palette);
    });

    // 2. 画布(场景镜像自 doc;底色走主题)
    let canvas = cx.new(|cx| {
        let mut canvas = SableCanvas::new(cx);
        canvas.scene = doc.read(cx).scene.read(cx).clone();
        canvas.base_color = palette.canvas_bg;
        canvas
    });

    // 3. 三段式 DockArea:左图层 / 中画布 / 右属性
    let left = vec![SablePanel::create(
        "图层",
        LayerHost::new(&doc, cx).into(),
        cx,
    )];
    let center = SablePanel::create("画布", canvas.clone().into(), cx);
    let right = vec![SablePanel::create(
        "属性",
        InspectorHost::new(doc.clone(), cx).into(),
        cx,
    )];
    let dock =
        WorkspacePresets::build_workspace("sable-vector-editor", left, center, right, window, cx);

    // 4. 宿主视图(焦点先行,键位动作才有派发路径)
    let focus = cx.focus_handle();
    window.focus(&focus);
    cx.new(|cx| EditorApp::new(doc, canvas, dock, focus, cx))
}

/// 宿主视图(挂进 `gpui_component::Root` 的业务根)。
pub struct EditorApp {
    doc: Entity<Document>,
    canvas: Entity<SableCanvas>,
    dock: Entity<DockArea>,
    focus: FocusHandle,
    /// 镜像策略的差异基准:上次与画布对齐时的场景快照。
    doc_mirror: sable::core::scene::Scene,
}

impl EditorApp {
    fn new(
        doc: Entity<Document>,
        canvas: Entity<SableCanvas>,
        dock: Entity<DockArea>,
        focus: FocusHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        let doc_mirror = doc.read(cx).scene.read(cx).clone();
        // doc(检查器/图层面板编辑)与 canvas(工具交互)任一变化都触发
        // 宿主重渲,下一帧由 reconcile 完成镜像同步。
        cx.observe(&doc, |_, _, cx| cx.notify()).detach();
        cx.observe(&canvas, |_, _, cx| cx.notify()).detach();
        Self {
            doc,
            canvas,
            dock,
            focus,
            doc_mirror,
        }
    }

    // —— 动作处理(cx.listener 目标)——

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.update(cx, |doc, cx| doc.undo(cx));
        cx.notify();
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.update(cx, |doc, cx| doc.redo(cx));
        cx.notify();
    }

    fn tool_select(&mut self, _: &ToolSelect, _: &mut Window, cx: &mut Context<Self>) {
        self.set_tool(CanvasTool::Select(SelectTool::default()), cx);
    }

    fn tool_pen(&mut self, _: &ToolPen, _: &mut Window, cx: &mut Context<Self>) {
        self.set_tool(CanvasTool::Pen(PenTool::default()), cx);
    }

    fn tool_hand(&mut self, _: &ToolHand, _: &mut Window, cx: &mut Context<Self>) {
        self.set_tool(CanvasTool::Hand(HandTool::default()), cx);
    }

    fn set_tool(&mut self, tool: CanvasTool, cx: &mut Context<Self>) {
        self.canvas.update(cx, |canvas, _| canvas.set_tool(tool));
        cx.notify();
    }

    // —— M0 镜像策略(唯一同步点;语义详见 document.rs 模块文档)——

    fn reconcile(&mut self, cx: &mut Context<Self>) {
        let canvas_scene = self.canvas.read(cx).scene.clone();

        if canvas_scene != self.doc_mirror {
            // 画布侧变化(拖动/钢笔):采纳进 Document(**不进撤销栈**)。
            let adopted = canvas_scene.clone();
            let scene_handle = self.doc.read(cx).scene.clone();
            scene_handle.update(cx, |scene, _| *scene = adopted);
            let selection = self.canvas.read(cx).selection().to_vec();
            self.doc.update(cx, |doc, _| doc.selection = selection);
            self.doc_mirror = canvas_scene;
        } else {
            // doc 侧变化(检查器编辑/撤销重做):回推画布,并清画布自持
            // 历史,保证撤销栈唯一(以 Document::history 为准)。
            let doc_scene = self.doc.read(cx).scene.read(cx).clone();
            if doc_scene != self.doc_mirror {
                self.doc_mirror = doc_scene.clone();
                self.canvas.update(cx, |canvas, cx| {
                    canvas.scene = doc_scene;
                    canvas.history.clear();
                    cx.notify();
                });
            }
            // 选中集镜像(真实来源 = 画布工具)
            let selection = self.canvas.read(cx).selection().to_vec();
            self.doc.update(cx, |doc, _| doc.selection = selection);
        }
    }

    // —— 工具栏 ——

    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> Div {
        let palette = Palette::get(cx);
        div()
            .flex()
            .gap_2()
            .items_center()
            .px_2()
            .h(px(36.))
            .bg(palette.surface_1)
            .border_b_1()
            .border_color(palette.surface_2)
            // 按钮 = 动作派发;真正的处理器在根节点 on_action(键盘/鼠标同路,docs/03 §4.1)
            .child(self.tool_button(
                "tool-select",
                "V 选择",
                cx.listener(|_, _, _, cx| cx.dispatch_action(&ToolSelect)),
            ))
            .child(self.tool_button(
                "tool-pen",
                "P 钢笔",
                cx.listener(|_, _, _, cx| cx.dispatch_action(&ToolPen)),
            ))
            .child(self.tool_button(
                "tool-hand",
                "H 抓手",
                cx.listener(|_, _, _, cx| cx.dispatch_action(&ToolHand)),
            ))
            .child(div().flex_1())
            .child(self.tool_button(
                "undo",
                "撤销 Ctrl-Z",
                cx.listener(|_, _, _, cx| cx.dispatch_action(&Undo)),
            ))
            .child(self.tool_button(
                "redo",
                "重做 Ctrl-Shift-Z",
                cx.listener(|_, _, _, cx| cx.dispatch_action(&Redo)),
            ))
    }

    fn tool_button(
        &self,
        id: &'static str,
        label: &'static str,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Button {
        Button::new(id).label(label).compact().on_click(on_click)
    }
}

impl Focusable for EditorApp {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for EditorApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.reconcile(cx);
        let palette = Palette::get(cx);
        div()
            .id("editor-root")
            .size_full()
            .flex()
            .flex_col()
            .bg(palette.surface_0)
            .text_color(palette.text_primary)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::tool_select))
            .on_action(cx.listener(Self::tool_pen))
            .on_action(cx.listener(Self::tool_hand))
            .child(self.render_toolbar(cx))
            .child(div().flex_1().child(self.dock.clone()))
    }
}
