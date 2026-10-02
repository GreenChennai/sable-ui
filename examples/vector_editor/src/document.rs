//! `Document`:矢量文档 = 场景 + 撤销栈 + 选中集,包成 Entity。
//!
//! # 架构说明(M0 的镜像策略,务必读)
//!
//! 上游 [`sable::canvas::gpui_element::SableCanvas`] 自持 `scene: Scene`
//! 与 `history: History` 字段(工具状态机直接写它们),无法借出给外部
//! Document。M0 的统一策略(全部集中在宿主 `EditorApp::reconcile`,见
//! `app.rs`):
//!
//! 1. **Document 是权威文档**:检查器等面板的编辑经 [`Document::exec`]
//!    走 [`History`] 事务语义;初始内容在打开文档时直接构建(打开新
//!    文档 = 空撤销栈语义,不进撤销栈)。
//! 2. 画布交互(拖动/钢笔)落在画布自持的 scene 上;宿主每帧做结构化
//!    比较([`Scene`] 的 `PartialEq` 是结构化相等),谁变了就采纳谁:
//!    doc 变 → 回推画布并 `canvas.history.clear()`(保证撤销栈唯一,以
//!    Document 的 History 为准);canvas 变 → 采纳进 doc(**不进撤销栈**
//!    ——画布工具的命令记在画布自持 history 里,统一回收是 M2 的
//!    "canvas 借出编辑态"重构,分册三 §4.2)。
//! 3. 选中集以画布工具持有的为镜像,Document 只存副本供面板读。

use gpui::AppContext as _;
use sable::core::command::{Command, History};
use sable::core::scene::{NodeId, Scene};
use sable::gpui;
use sable::gpui::Entity;

/// 矢量文档(权威编辑态)。
pub struct Document {
    /// 场景。Entity 形态:图层面板契约(`LayerPanel::new`)要 `Entity<Scene>`。
    pub scene: Entity<Scene>,
    /// 撤销栈(唯一权威;画布自持 history 仅服务画布内部拖动事务)。
    pub history: History,
    /// 选中集镜像(真实来源 = 画布工具,见模块文档第 3 条)。
    pub selection: Vec<NodeId>,
}

impl Document {
    /// 新建空文档。
    pub fn create(cx: &mut gpui::App) -> Entity<Self> {
        let scene = cx.new(|_| Scene::new());
        cx.new(|_| Document {
            scene,
            history: History::new(),
            selection: Vec::new(),
        })
    }

    /// 执行一条命令(先 apply 后入栈,清 redo;AGENTS.md §3.1 铁律的唯一
    /// 正门)。`cx` 同时驱动嵌套的 scene Entity 更新(gpui 的
    /// `Context<T>: AppContext` 语义)。
    pub fn exec(&mut self, cmd: Box<dyn Command>, cx: &mut gpui::Context<Self>) {
        let history = &mut self.history;
        self.scene.update(cx, |scene, _| history.exec(cmd, scene));
        self.prune_selection(cx);
    }

    /// 撤销一步(id 重映射由 History 内部广播)。
    pub fn undo(&mut self, cx: &mut gpui::Context<Self>) {
        let history = &mut self.history;
        self.scene.update(cx, |scene, _| history.undo(scene));
        self.prune_selection(cx);
    }

    /// 重做一步。
    pub fn redo(&mut self, cx: &mut gpui::Context<Self>) {
        let history = &mut self.history;
        self.scene.update(cx, |scene, _| history.redo(scene));
        self.prune_selection(cx);
    }

    /// 单选一个节点(图层面板等入口)。
    #[allow(dead_code)] // M2 工具栏接线预留
    pub fn select(&mut self, id: NodeId) {
        self.selection.clear();
        self.selection.push(id);
    }

    /// 场景快照(回推画布/比较用;`Scene: Clone`)。
    #[allow(dead_code)] // M2 工具栏接线预留
    pub fn scene_snapshot(&self, cx: &gpui::App) -> Scene {
        self.scene.read(cx).clone()
    }

    /// 剔除已不存在的节点(撤销删除类操作后选中集可能悬空)。
    fn prune_selection(&mut self, cx: &gpui::Context<Self>) {
        let scene = self.scene.read(cx);
        self.selection.retain(|&id| scene.node(id).is_some());
    }
}
