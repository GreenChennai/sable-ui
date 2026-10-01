//! 命令系统:撤销重做的唯一正解(docs/03 §2)。
//!
//! 铁律(AGENTS.md §3.1):**一切文档修改必须走 Command,直接改 Scene 的路径不允许存在。**
//!
//! # id 身份与撤销的相互作用(本实现相对 docs/03 §2 原文的关键适配)
//!
//! slotmap 节点被移除后 slot 版本号单调递增,安全 API 无法复原原 key,因此
//! [`crate::scene::Scene::undo_remove`] 恢复子树时换发新 id 并返回重映射表。
//! 由此带来三处适配,均已在测试中验证:
//!
//! 1. `apply`/`revert` 接收 `&mut self`:`AddNode`/`RemoveNode` 需要追踪
//!    "当前生效 id"(undo/redo 任意次循环后仍能精确回退);
//! 2. `revert` 返回 `Option<IdRemap>`:恢复子树产生的 old→new 映射;
//! 3. `History` 在每次 undo 后把映射广播给全部历史条目
//!    ([`Command::remap_ids`]),否则"改子节点属性 → 删父组 → 撤销×2"这类
//!    基础流程会静默失效。
//!
//! `merge` 合并范式(连续输入/拖动 → 一步撤销)保留 docs/03 §2 原文语义:
//! 同节点合并,保留最早的 old、最新的 new。

use std::any::Any;
use std::borrow::Cow;
use std::fmt;

use kurbo::Affine;

use crate::error::CoreResult;
use crate::scene::{IdRemap, Node, NodeId, Paint, RemovedSubtree, Scene, StrokeStyle};

/// 命令 = 一次可逆的文档修改。
///
/// 注意:`Command` 不要求 `Clone`(revert 会更新自身缓存的 id,克隆语义易错),
/// 因此批量命令之间不做拼接式 merge。
pub trait Command: Send + 'static {
    /// 正向执行。目标不存在时静默跳过(LIFO 约定下不应发生;发生即场景漂移异常)。
    fn apply(&mut self, scene: &mut Scene);

    /// 逆向执行(撤销)。恢复子树的命令返回 old→new 重映射表,其余返回 `None`。
    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        let _ = scene;
        None
    }

    /// 命令合并(连续打字/拖动 → 合并为一步撤销;保留最早的 old、最新的 new)。
    fn merge(&self, next: &dyn Command) -> Option<Box<dyn Command>> {
        let _ = next;
        None
    }

    /// `merge` 里 downcast 用,必填。
    fn as_any(&self) -> &dyn Any;

    /// 结构型命令实现:按映射重写自身缓存的节点 id(History 在 undo 后统一调用)。
    fn remap_ids(&mut self, map: &IdRemap) {
        let _ = map;
    }

    /// 命令名(用于历史面板/日志)。
    fn name(&self) -> Cow<'static, str>;

    /// 装箱为 trait object。
    fn boxed(self) -> Box<dyn Command>
    where
        Self: Sized,
    {
        Box::new(self)
    }
}

/// 批量事务命令:一次 [`History::begin_transaction`]..[`History::end_transaction`]
/// 之间执行的全部命令,撤销/重做各算一步。
pub struct BatchCommand(pub Vec<Box<dyn Command>>);

impl fmt::Debug for BatchCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BatchCommand")
            .field("steps", &self.0.len())
            .finish()
    }
}

impl Command for BatchCommand {
    fn apply(&mut self, scene: &mut Scene) {
        for cmd in &mut self.0 {
            cmd.apply(scene);
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        // 逆序回退;某步产生 id 重映射时,先喂给尚未回退的前序命令
        let mut total = IdRemap::new();
        let mut any = false;
        for i in (0..self.0.len()).rev() {
            if let Some(m) = self.0[i].revert(scene) {
                any = true;
                for cmd in self.0[..i].iter_mut() {
                    cmd.remap_ids(&m);
                }
                // 累积总映射:已累积的值穿过本次映射,再并入本次的新键
                let mut merged: IdRemap = total
                    .into_iter()
                    .map(|(k, v)| (k, m.get(&v).copied().unwrap_or(v)))
                    .collect();
                for (k, v) in m {
                    merged.entry(k).or_insert(v);
                }
                total = merged;
            }
        }
        if any { Some(total) } else { None }
    }

    fn remap_ids(&mut self, map: &IdRemap) {
        for cmd in &mut self.0 {
            cmd.remap_ids(map);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("批量操作")
    }
}

/// 历史栈:撤销/重做的唯一入口(docs/03 §2)。
pub struct History {
    undo: Vec<Box<dyn Command>>,
    redo: Vec<Box<dyn Command>>,
    /// 批量事务:如"对齐 5 个对象" = 一步撤销
    transaction: Option<Vec<Box<dyn Command>>>,
}

impl fmt::Debug for History {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("History")
            .field("undo_len", &self.undo.len())
            .field("redo_len", &self.redo.len())
            .field("in_transaction", &self.transaction.is_some())
            .finish()
    }
}

impl Default for History {
    fn default() -> Self {
        History {
            undo: Vec::new(),
            redo: Vec::new(),
            transaction: None,
        }
    }
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// 执行一条命令:先 apply;事务内只收集,否则尝试与栈顶合并,并清空 redo。
    pub fn exec(&mut self, mut cmd: Box<dyn Command>, scene: &mut Scene) {
        cmd.apply(scene);
        match &mut self.transaction {
            Some(batch) => batch.push(cmd),
            None => {
                // 尝试与栈顶合并(连续打字/拖动场景)
                let merged = self.undo.last().and_then(|top| top.merge(&*cmd));
                if let Some(merged) = merged {
                    self.undo.pop();
                    self.undo.push(merged);
                } else {
                    self.undo.push(cmd);
                }
                self.redo.clear();
            }
        }
    }

    /// 撤销一步;子树恢复产生的 id 重映射会广播给全部历史条目。
    pub fn undo(&mut self, scene: &mut Scene) {
        if let Some(mut cmd) = self.undo.pop() {
            let remap = cmd.revert(scene);
            self.redo.push(cmd);
            if let Some(map) = remap {
                self.remap_all(&map);
            }
        }
    }

    /// 重做一步(按撤销的逆序逐条重放)。
    pub fn redo(&mut self, scene: &mut Scene) {
        if let Some(mut cmd) = self.redo.pop() {
            cmd.apply(scene);
            self.undo.push(cmd);
        }
    }

    /// 开始批量事务(嵌套调用被忽略,不丢已收集的命令)。
    pub fn begin_transaction(&mut self) {
        if self.transaction.is_none() {
            self.transaction = Some(Vec::new());
        }
    }

    /// 结束批量事务;空事务不产生撤销步。
    pub fn end_transaction(&mut self) {
        if let Some(batch) = self.transaction.take() {
            if !batch.is_empty() {
                self.undo.push(Box::new(BatchCommand(batch)));
                self.redo.clear();
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// 撤销栈深度(自动保存用:每 N 个未存命令触发一次,docs/06 §1.2)。
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// 清空全部历史(如"打开新文档")。
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.transaction = None;
    }

    fn remap_all(&mut self, map: &IdRemap) {
        for cmd in &mut self.undo {
            cmd.remap_ids(map);
        }
        for cmd in &mut self.redo {
            cmd.remap_ids(map);
        }
        if let Some(batch) = &mut self.transaction {
            for cmd in batch {
                cmd.remap_ids(map);
            }
        }
    }
}

// —— 内置命令集 ——

/// 添加节点(把 `node` 模板挂到 parent 的 index 处;`index = None` 追加)。
#[derive(Debug)]
pub struct AddNode {
    pub parent: Option<NodeId>,
    pub index: Option<usize>,
    /// 首次 apply 后为实际生效的节点 id(slotmap 插入时才发号;undo/redo 循环后会变)。
    pub id: Option<NodeId>,
    /// 节点模板(apply 时克隆插入;parent 字段以命令的 parent 字段为准)。
    pub node: Node,
}

impl AddNode {
    pub fn new(parent: Option<NodeId>, index: Option<usize>, node: Node) -> Self {
        AddNode {
            parent,
            index,
            id: None,
            node,
        }
    }
}

impl Command for AddNode {
    fn apply(&mut self, scene: &mut Scene) {
        if let Ok(id) = scene.insert_at(self.parent, self.index, self.node.clone()) {
            self.id = Some(id);
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        if let Some(id) = self.id.take() {
            let _ = scene.remove_subtree(id);
        }
        None
    }

    fn remap_ids(&mut self, map: &IdRemap) {
        self.parent = self.parent.map(|p| map.get(&p).copied().unwrap_or(p));
        if let Some(id) = self.id {
            self.id = Some(map.get(&id).copied().unwrap_or(id));
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("添加节点")
    }
}

/// 删除节点及其子树(结构+数据保存在 [`RemovedSubtree`],撤销时整树恢复)。
#[derive(Debug)]
pub struct RemoveNode {
    /// 子树根。撤销恢复后会换新 id,本字段由 revert 更新为当前生效 id。
    pub id: NodeId,
    /// 摘除的子树(apply 时捕获;每次正向执行都重新捕获,保证 undo/redo 任意次循环可用)。
    pub subtree: Option<RemovedSubtree>,
}

impl RemoveNode {
    pub fn new(id: NodeId) -> Self {
        RemoveNode { id, subtree: None }
    }
}

impl Command for RemoveNode {
    fn apply(&mut self, scene: &mut Scene) {
        if let Ok(subtree) = scene.remove_subtree(self.id) {
            self.subtree = Some(subtree);
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        let subtree = self.subtree.as_ref()?;
        let map = scene.undo_remove(subtree).ok()?;
        self.id = map.get(&self.id).copied().unwrap_or(self.id);
        Some(map)
    }

    fn remap_ids(&mut self, map: &IdRemap) {
        self.id = map.get(&self.id).copied().unwrap_or(self.id);
        // 摘除点锚点同样会被外部子树的恢复改变:先删叶子、再删其祖先,撤销时
        // 祖先先恢复(换新 id),本命令的 subtree.parent 若不跟着重映射,
        // 自己的 undo 会因 ParentNotFound 静默失败(proptest 最小反例)。
        if let Some(subtree) = &mut self.subtree {
            subtree.parent = subtree.parent.map(|p| map.get(&p).copied().unwrap_or(p));
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("删除节点")
    }
}

/// 修改节点局部变换。
#[derive(Debug)]
pub struct SetTransform {
    pub id: NodeId,
    pub old: Affine,
    pub new: Affine,
}

impl Command for SetTransform {
    fn apply(&mut self, scene: &mut Scene) {
        if let Some(node) = scene.node_mut(self.id) {
            node.transform = self.new;
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        if let Some(node) = scene.node_mut(self.id) {
            node.transform = self.old;
        }
        None
    }

    fn remap_ids(&mut self, map: &IdRemap) {
        self.id = map.get(&self.id).copied().unwrap_or(self.id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("修改变换")
    }
}

/// 修改路径填充。merge:同节点合并,保留最早的 old、最新的 new(docs/03 §2 范式)。
#[derive(Debug)]
pub struct SetFill {
    pub id: NodeId,
    pub old: Option<Paint>,
    pub new: Option<Paint>,
}

impl Command for SetFill {
    fn apply(&mut self, scene: &mut Scene) {
        if let Some(p) = scene.path_mut(self.id) {
            p.fill = self.new.clone();
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        if let Some(p) = scene.path_mut(self.id) {
            p.fill = self.old.clone();
        }
        None
    }

    fn merge(&self, next: &dyn Command) -> Option<Box<dyn Command>> {
        let next = next.as_any().downcast_ref::<SetFill>()?;
        if next.id != self.id {
            return None;
        }
        Some(Box::new(SetFill {
            id: self.id,
            old: self.old.clone(),
            new: next.new.clone(),
        }))
    }

    fn remap_ids(&mut self, map: &IdRemap) {
        self.id = map.get(&self.id).copied().unwrap_or(self.id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("修改填充")
    }
}

/// 修改路径描边。merge 同 [`SetFill`]。
#[derive(Debug)]
pub struct SetStroke {
    pub id: NodeId,
    pub old: Option<StrokeStyle>,
    pub new: Option<StrokeStyle>,
}

impl Command for SetStroke {
    fn apply(&mut self, scene: &mut Scene) {
        if let Some(p) = scene.path_mut(self.id) {
            p.stroke = self.new.clone();
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        if let Some(p) = scene.path_mut(self.id) {
            p.stroke = self.old.clone();
        }
        None
    }

    fn merge(&self, next: &dyn Command) -> Option<Box<dyn Command>> {
        let next = next.as_any().downcast_ref::<SetStroke>()?;
        if next.id != self.id {
            return None;
        }
        Some(Box::new(SetStroke {
            id: self.id,
            old: self.old.clone(),
            new: next.new.clone(),
        }))
    }

    fn remap_ids(&mut self, map: &IdRemap) {
        self.id = map.get(&self.id).copied().unwrap_or(self.id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("修改描边")
    }
}

/// 修改可见性。
#[derive(Debug)]
pub struct SetVisibility {
    pub id: NodeId,
    pub old: bool,
    pub new: bool,
}

impl Command for SetVisibility {
    fn apply(&mut self, scene: &mut Scene) {
        if let Some(node) = scene.node_mut(self.id) {
            node.visible = self.new;
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        if let Some(node) = scene.node_mut(self.id) {
            node.visible = self.old;
        }
        None
    }

    fn remap_ids(&mut self, map: &IdRemap) {
        self.id = map.get(&self.id).copied().unwrap_or(self.id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("修改可见性")
    }
}

/// 修改不透明度(apply 时夹到 [0,1];merge 同 [`SetFill`])。
#[derive(Debug)]
pub struct SetOpacity {
    pub id: NodeId,
    pub old: f64,
    pub new: f64,
}

impl Command for SetOpacity {
    fn apply(&mut self, scene: &mut Scene) {
        if let Some(node) = scene.node_mut(self.id) {
            node.opacity = self.new.clamp(0.0, 1.0);
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        if let Some(node) = scene.node_mut(self.id) {
            node.opacity = self.old.clamp(0.0, 1.0);
        }
        None
    }

    fn merge(&self, next: &dyn Command) -> Option<Box<dyn Command>> {
        let next = next.as_any().downcast_ref::<SetOpacity>()?;
        if next.id != self.id {
            return None;
        }
        Some(Box::new(SetOpacity {
            id: self.id,
            old: self.old,
            new: next.new,
        }))
    }

    fn remap_ids(&mut self, map: &IdRemap) {
        self.id = map.get(&self.id).copied().unwrap_or(self.id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("修改不透明度")
    }
}

/// 重命名节点。
#[derive(Debug)]
pub struct SetName {
    pub id: NodeId,
    pub old: String,
    pub new: String,
}

impl Command for SetName {
    fn apply(&mut self, scene: &mut Scene) {
        if let Some(node) = scene.node_mut(self.id) {
            node.name = self.new.clone();
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        if let Some(node) = scene.node_mut(self.id) {
            node.name = self.old.clone();
        }
        None
    }

    fn remap_ids(&mut self, map: &IdRemap) {
        self.id = map.get(&self.id).copied().unwrap_or(self.id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("重命名")
    }
}

/// 移动节点到新父节点(场景层完成成环检测;失败时本命令为空操作)。
#[derive(Debug)]
pub struct Reparent {
    pub id: NodeId,
    pub old_parent: Option<NodeId>,
    pub old_index: usize,
    pub new_parent: Option<NodeId>,
    pub new_index: Option<usize>,
}

impl Reparent {
    /// 从场景读取当前(父, 下标)作为回退锚点后构造命令。
    pub fn capture(
        scene: &Scene,
        id: NodeId,
        new_parent: Option<NodeId>,
        new_index: Option<usize>,
    ) -> CoreResult<Self> {
        let (old_parent, old_index) = scene.position(id)?;
        Ok(Reparent {
            id,
            old_parent,
            old_index,
            new_parent,
            new_index,
        })
    }
}

impl Command for Reparent {
    fn apply(&mut self, scene: &mut Scene) {
        let _ = scene.reparent(self.id, self.new_parent, self.new_index);
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        let _ = scene.reparent(self.id, self.old_parent, Some(self.old_index));
        None
    }

    fn remap_ids(&mut self, map: &IdRemap) {
        self.id = map.get(&self.id).copied().unwrap_or(self.id);
        self.old_parent = self.old_parent.map(|p| map.get(&p).copied().unwrap_or(p));
        self.new_parent = self.new_parent.map(|p| map.get(&p).copied().unwrap_or(p));
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("移动节点")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{NodeContent, PathNode};
    use kurbo::BezPath;
    use proptest::prelude::*;

    fn rect_content(x0: f64, y0: f64, x1: f64, y1: f64) -> NodeContent {
        let mut path = BezPath::new();
        path.move_to((x0, y0));
        path.line_to((x1, y0));
        path.line_to((x1, y1));
        path.line_to((x0, y1));
        path.close_path();
        NodeContent::Path(PathNode {
            path,
            fill: Some(Paint::Solid([90, 90, 90, 255])),
            stroke: None,
        })
    }

    /// 根组 → (矩形A, 子组 → 矩形B),外加根级矩形C
    fn demo_scene() -> (Scene, NodeId, NodeId, NodeId, NodeId, NodeId) {
        let mut scene = Scene::new();
        let root = scene
            .add_node(None, "根组", NodeContent::Group)
            .expect("add root");
        let a = scene
            .add_node(Some(root), "矩形A", rect_content(0.0, 0.0, 10.0, 10.0))
            .expect("add a");
        let sub = scene
            .add_node(Some(root), "子组", NodeContent::Group)
            .expect("add sub");
        let b = scene
            .add_node(Some(sub), "矩形B", rect_content(20.0, 20.0, 30.0, 30.0))
            .expect("add b");
        let c = scene
            .add_node(None, "矩形C", rect_content(-5.0, -5.0, 5.0, 5.0))
            .expect("add c");
        (scene, root, a, sub, b, c)
    }

    #[test]
    fn add_node_command_roundtrip() {
        let (mut scene, _root, _a, _sub, _b, _c) = demo_scene();
        let len_before = scene.len();
        let snapshot = scene.clone();
        let mut history = History::new();
        history.exec(
            AddNode::new(None, None, Node::new("新节点", NodeContent::Group)).boxed(),
            &mut scene,
        );
        assert_eq!(scene.len(), len_before + 1);
        history.undo(&mut scene);
        assert_eq!(scene, snapshot);
        history.redo(&mut scene);
        assert_eq!(scene.len(), len_before + 1);
        history.undo(&mut scene);
        assert_eq!(scene, snapshot);
    }

    #[test]
    fn remove_node_command_roundtrip() {
        let (mut scene, _root, _a, sub, _b, _c) = demo_scene();
        let snapshot = scene.clone();
        let mut history = History::new();
        history.exec(RemoveNode::new(sub).boxed(), &mut scene);
        assert_eq!(scene.len(), snapshot.len() - 2, "子组 + 孙矩形B 被摘除");
        history.undo(&mut scene);
        assert_eq!(scene, snapshot, "子树完整恢复(数据一字不差)");
        history.redo(&mut scene);
        assert_eq!(scene.len(), snapshot.len() - 2);
        history.undo(&mut scene);
        assert_eq!(scene, snapshot);
    }

    #[test]
    fn set_property_commands_roundtrip() {
        let (mut scene, _root, a, _sub, b, c) = demo_scene();
        let snapshot = scene.clone();
        let a_name = scene.node(a).expect("a 在").name.clone();
        let a_fill = scene.path(a).and_then(|p| p.fill.clone());
        let mut history = History::new();

        history.exec(
            SetTransform {
                id: b,
                old: Affine::IDENTITY,
                new: Affine::translate((5.0, 7.0)) * Affine::rotate(0.3),
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            SetFill {
                id: a,
                old: a_fill,
                new: Some(Paint::LinearGradient {
                    start: [0.0, 0.0],
                    end: [1.0, 1.0],
                    stops: vec![],
                }),
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            SetStroke {
                id: a,
                old: None,
                new: Some(StrokeStyle {
                    paint: Paint::Solid([255, 255, 0, 255]),
                    width: 2.0,
                }),
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            SetVisibility {
                id: c,
                old: true,
                new: false,
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            SetOpacity {
                id: a,
                old: 1.0,
                new: 0.25,
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            SetName {
                id: a,
                old: a_name,
                new: "改名A".into(),
            }
            .boxed(),
            &mut scene,
        );

        // 各字段确已生效
        assert_ne!(scene.node(b).expect("b").transform, Affine::IDENTITY);
        assert!(scene.path(a).expect("a path").stroke.is_some());
        assert_eq!(scene.node(a).expect("a").opacity, 0.25);
        assert_eq!(scene.node(a).expect("a").name, "改名A");
        assert!(!scene.render_list().iter().any(|(id, _)| *id == c));
        assert_eq!(history.undo_len(), 6);

        // 一步一步撤销,全部回到原状
        for _ in 0..6 {
            history.undo(&mut scene);
        }
        assert_eq!(scene, snapshot);
        assert!(!history.can_undo());
        assert!(history.can_redo());
        // 一步不少地重做
        for _ in 0..6 {
            history.redo(&mut scene);
        }
        assert_eq!(scene.node(a).expect("a").name, "改名A");
    }

    #[test]
    fn set_fill_merge_collapses_to_one_undo_step() {
        let (mut scene, _root, a, _sub, b, _c) = demo_scene();
        let a_fill = scene.path(a).and_then(|p| p.fill.clone());
        let mut history = History::new();
        let mid = Some(Paint::Solid([1, 2, 3, 4]));
        let last = Some(Paint::Solid([9, 9, 9, 9]));

        history.exec(
            SetFill {
                id: a,
                old: a_fill.clone(),
                new: mid.clone(),
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            SetFill {
                id: a,
                old: mid.clone(),
                new: last.clone(),
            }
            .boxed(),
            &mut scene,
        );
        // 同节点连续修改 → 合并成一步
        assert_eq!(history.undo_len(), 1);
        assert_eq!(scene.path(a).expect("path").fill, last);

        history.undo(&mut scene);
        // 保留最早的 old:撤销一步回到最初的填充(rect_content 的实心色)
        assert_eq!(scene.path(a).expect("path").fill, a_fill.clone());
        history.redo(&mut scene);
        // 保留最新的 new:重做一步到最新
        assert_eq!(scene.path(a).expect("path").fill, last);

        // 不同节点的 SetFill 不合并
        let b_fill = scene.path(b).and_then(|p| p.fill.clone());
        history.exec(
            SetFill {
                id: b,
                old: b_fill,
                new: None,
            }
            .boxed(),
            &mut scene,
        );
        assert_eq!(history.undo_len(), 2);
    }

    #[test]
    fn set_stroke_and_opacity_merge_per_node() {
        let (mut scene, _root, a, _sub, b, _c) = demo_scene();
        let mut history = History::new();
        history.exec(
            SetOpacity {
                id: a,
                old: 1.0,
                new: 0.8,
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            SetOpacity {
                id: a,
                old: 0.8,
                new: 0.6,
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            SetStroke {
                id: a,
                old: None,
                new: None,
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            SetOpacity {
                id: b,
                old: 1.0,
                new: 0.5,
            }
            .boxed(),
            &mut scene,
        );
        // [SetOpacity(a) 合并为一步, SetStroke(a), SetOpacity(b)] = 3 步
        assert_eq!(history.undo_len(), 3);
        assert_eq!(scene.node(a).expect("a").opacity, 0.6);
    }

    #[test]
    fn transaction_is_one_undo_step() {
        let (mut scene, root, _a, _sub, _b, c) = demo_scene();
        let snapshot = scene.clone();
        let mut history = History::new();

        history.begin_transaction();
        history.exec(
            SetOpacity {
                id: root,
                old: 1.0,
                new: 0.9,
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            AddNode::new(None, None, Node::new("事务内新增", NodeContent::Group)).boxed(),
            &mut scene,
        );
        history.exec(RemoveNode::new(c).boxed(), &mut scene);
        history.end_transaction();

        assert_eq!(history.undo_len(), 1, "三个命令合并为一步撤销");
        assert_eq!(scene.len(), snapshot.len(), "-1 +1 相抵");
        history.undo(&mut scene);
        assert_eq!(scene, snapshot, "批量一步撤销,场景完整还原");
        history.redo(&mut scene);
        assert_eq!(scene.len(), snapshot.len());
        history.undo(&mut scene);
        assert_eq!(scene, snapshot);
    }

    #[test]
    fn empty_transaction_creates_no_step() {
        let (mut scene, _root, _a, _sub, _b, _c) = demo_scene();
        let mut history = History::new();
        history.begin_transaction();
        history.end_transaction();
        assert_eq!(history.undo_len(), 0);
        assert!(!history.can_undo());

        // 嵌套 begin 被忽略,不丢已收集命令
        history.begin_transaction();
        history.exec(
            AddNode::new(None, None, Node::new("x", NodeContent::Group)).boxed(),
            &mut scene,
        );
        history.begin_transaction();
        history.end_transaction();
        assert_eq!(history.undo_len(), 1);
    }

    #[test]
    fn reparent_command_roundtrip() {
        let (mut scene, _root, a, sub, _b, _c) = demo_scene();
        let snapshot = scene.clone();
        let mut history = History::new();

        let cmd = Reparent::capture(&scene, a, Some(sub), None).expect("capture");
        history.exec(cmd.boxed(), &mut scene);
        assert_eq!(scene.node(a).expect("a 在").parent, Some(sub));
        history.undo(&mut scene);
        assert_eq!(scene, snapshot);
        history.redo(&mut scene);
        assert_eq!(scene.node(a).expect("a 在").parent, Some(sub));
        history.undo(&mut scene);
        assert_eq!(scene, snapshot);
    }

    #[test]
    fn undo_after_remove_heals_ids_of_older_entries() {
        // 核心场景:先改子节点属性,再删父组,连撤两步 → 全部还原。
        // (slotmap 恢复换发 id,History 必须把重映射广播给历史条目)
        let (mut scene, _root, _a, sub, b, _c) = demo_scene();
        let snapshot = scene.clone();
        let old_fill = scene.path(b).and_then(|p| p.fill.clone());
        let mut history = History::new();

        history.exec(
            SetFill {
                id: b,
                old: old_fill,
                new: Some(Paint::Solid([10, 200, 10, 255])),
            }
            .boxed(),
            &mut scene,
        );
        history.exec(RemoveNode::new(sub).boxed(), &mut scene);
        assert_eq!(scene.len(), snapshot.len() - 2);

        history.undo(&mut scene); // 恢复子组(矩形B 换新 id)
        history.undo(&mut scene); // SetFill 必须被重映射到新 id 才能生效
        assert_eq!(scene, snapshot, "两步撤销后与初始快照完全一致");

        history.redo(&mut scene);
        history.redo(&mut scene);
        assert_eq!(scene.len(), snapshot.len() - 2);
        history.undo(&mut scene);
        history.undo(&mut scene);
        assert_eq!(scene, snapshot);
    }

    #[test]
    fn undo_nested_removals_heals_anchor_of_deeper_removal() {
        // proptest 最小反例的确定性回归:先删叶子(矩形A),再删其祖先(根组,
        // 连带子组+矩形B)。撤销时祖先先恢复并换发新 id,叶子删除命令的摘除点
        // 锚点(subtree.parent)必须随之重映射,否则它的 undo 因 ParentNotFound
        // 静默失败,矩形A 永久丢失。
        let (mut scene, root, a, _sub, _b, _c) = demo_scene();
        let snapshot = scene.clone();
        let mut history = History::new();
        history.exec(RemoveNode::new(a).boxed(), &mut scene);
        history.exec(RemoveNode::new(root).boxed(), &mut scene);
        assert_eq!(scene.len(), 1, "只剩矩形C");

        while history.can_undo() {
            history.undo(&mut scene);
        }
        assert_eq!(scene, snapshot, "嵌套删除全部撤销后完整还原");

        // redo/undo 交替下锚点依然正确
        while history.can_redo() {
            history.redo(&mut scene);
        }
        assert_eq!(scene.len(), 1);
        while history.can_undo() {
            history.undo(&mut scene);
        }
        assert_eq!(scene, snapshot);
    }

    #[test]
    fn remove_then_add_then_undo_all_restores() {
        // 删除 → 新增(slotmap 可能复用 slot)→ 撤销全部
        let (mut scene, _root, _a, sub, _b, _c) = demo_scene();
        let snapshot = scene.clone();
        let mut history = History::new();
        history.exec(RemoveNode::new(sub).boxed(), &mut scene);
        history.exec(
            AddNode::new(None, None, Node::new("新层", NodeContent::Group)).boxed(),
            &mut scene,
        );
        while history.can_undo() {
            history.undo(&mut scene);
        }
        assert_eq!(scene, snapshot);
    }

    #[test]
    fn clear_drops_history() {
        let (mut scene, _root, _a, _sub, _b, _c) = demo_scene();
        let mut history = History::new();
        history.exec(
            AddNode::new(None, None, Node::new("x", NodeContent::Group)).boxed(),
            &mut scene,
        );
        assert!(history.can_undo());
        history.clear();
        assert!(!history.can_undo() && !history.can_redo());
        assert_eq!(history.undo_len(), 0);
    }

    // —— proptest:随机命令序列全部撤销后回到初始快照 ——

    #[derive(Debug, Clone)]
    enum Op {
        AddNode,
        Remove(usize),
        SetTransform(usize, [f64; 6]),
        SetFill(usize, [u8; 4]),
        SetStroke(usize, f64),
        SetVisibility(usize, bool),
        SetOpacity(usize, f64),
        SetName(usize, u16),
        Reparent(usize, usize),
    }

    fn any_op() -> impl Strategy<Value = Op> {
        prop_oneof![
            2 => Just(Op::AddNode),
            2 => (0usize..8).prop_map(Op::Remove),
            2 => (0usize..8, -100f64..100.0, -100f64..100.0, -10f64..10.0, -10f64..10.0, -10f64..10.0, -10f64..10.0)
                .prop_map(|(i, a, b, c, d, e, f)| Op::SetTransform(i, [a, b, c, d, e, f])),
            2 => (0usize..8, any::<u8>(), any::<u8>(), any::<u8>(), any::<u8>())
                .prop_map(|(i, r, g, b, a)| Op::SetFill(i, [r, g, b, a])),
            1 => (0usize..8, 0.1f64..10.0).prop_map(|(i, w)| Op::SetStroke(i, w)),
            1 => (0usize..8, any::<bool>()).prop_map(|(i, v)| Op::SetVisibility(i, v)),
            1 => (0usize..8, 0f64..1.0).prop_map(|(i, o)| Op::SetOpacity(i, o)),
            1 => (0usize..8, 0u16..1000).prop_map(|(i, n)| Op::SetName(i, n)),
            1 => (0usize..8, 0usize..8).prop_map(|(x, y)| Op::Reparent(x, y)),
        ]
    }

    /// 从登记表取一个"当前存活"的节点 id(取模避免越界;死句柄自动跳过)。
    fn pick(scene: &Scene, registry: &[NodeId], idx: usize) -> Option<NodeId> {
        if registry.is_empty() {
            return None;
        }
        let id = registry[idx % registry.len()];
        scene.node(id).is_some().then_some(id)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn random_command_sequence_full_undo_restores_snapshot(
            ops in proptest::collection::vec(any_op(), 1..50),
        ) {
            let (mut scene, _root, a0, _sub, b0, _c) = demo_scene();
            let snapshot = scene.clone();
            let mut history = History::new();
            let mut registry: Vec<NodeId> = vec![a0, b0];
            registry.extend(scene.iter_roots());

            for (step, op) in ops.iter().enumerate() {
                match *op {
                    Op::AddNode => {
                        let node = Node::new(format!("n{step}"), NodeContent::Group);
                        history.exec(AddNode::new(None, None, node).boxed(), &mut scene);
                        if let Some(&id) = scene.roots.last() {
                            registry.push(id);
                        }
                    }
                    Op::Remove(i) => {
                        if let Some(id) = pick(&scene, &registry, i) {
                            history.exec(RemoveNode::new(id).boxed(), &mut scene);
                        }
                    }
                    Op::SetTransform(i, m) => {
                        if let Some(id) = pick(&scene, &registry, i) {
                            let old = scene.node(id).expect("存活").transform;
                            history.exec(
                                SetTransform { id, old, new: Affine::new(m) }.boxed(),
                                &mut scene,
                            );
                        }
                    }
                    Op::SetFill(i, rgba) => {
                        if let Some(id) = pick(&scene, &registry, i) {
                            let old = scene.path(id).and_then(|p| p.fill.clone());
                            history.exec(
                                SetFill { id, old, new: Some(Paint::Solid(rgba)) }.boxed(),
                                &mut scene,
                            );
                        }
                    }
                    Op::SetStroke(i, w) => {
                        if let Some(id) = pick(&scene, &registry, i) {
                            let old = scene.path(id).and_then(|p| p.stroke.clone());
                            let new = Some(StrokeStyle {
                                paint: Paint::Solid([1, 2, 3, 4]),
                                width: w,
                            });
                            history.exec(SetStroke { id, old, new }.boxed(), &mut scene);
                        }
                    }
                    Op::SetVisibility(i, v) => {
                        if let Some(id) = pick(&scene, &registry, i) {
                            let old = scene.node(id).expect("存活").visible;
                            history.exec(SetVisibility { id, old, new: v }.boxed(), &mut scene);
                        }
                    }
                    Op::SetOpacity(i, o) => {
                        if let Some(id) = pick(&scene, &registry, i) {
                            let old = scene.node(id).expect("存活").opacity;
                            history.exec(SetOpacity { id, old, new: o }.boxed(), &mut scene);
                        }
                    }
                    Op::SetName(i, n) => {
                        if let Some(id) = pick(&scene, &registry, i) {
                            let old = scene.node(id).expect("存活").name.clone();
                            history.exec(
                                SetName { id, old, new: format!("节点{n}") }.boxed(),
                                &mut scene,
                            );
                        }
                    }
                    Op::Reparent(x, y) => {
                        if let (Some(id), Some(parent)) =
                            (pick(&scene, &registry, x), pick(&scene, &registry, y))
                        {
                            if id != parent {
                                if let Ok(cmd) = Reparent::capture(&scene, id, Some(parent), None) {
                                    history.exec(cmd.boxed(), &mut scene);
                                }
                            }
                        }
                    }
                }
            }

            while history.can_undo() {
                history.undo(&mut scene);
            }
            prop_assert_eq!(scene, snapshot, "任意命令序列全部撤销后必须回到初始快照");
        }
    }
}
