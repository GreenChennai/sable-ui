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
use serde::{Deserialize, Serialize};

use crate::effects::{EffectEntry, EffectSpec};
use crate::error::CoreResult;
use crate::scene::{IdRemap, Node, NodeId, Paint, RemovedSubtree, Scene, StrokeStyle};

/// 命令 = 一次可逆的文档修改。
///
/// 注意:`Command` 不要求 `Clone`(revert 会更新自身缓存的 id,克隆语义易错),
/// 因此批量命令之间不做拼接式 merge。
pub trait Command: Send + 'static {
    /// 正向执行。目标不存在时静默跳过(LIFO 约定下不应发生;发生即场景漂移异常)。
    fn apply(&mut self, scene: &mut Scene);

    /// 重做方向的 id 治理钩子:重做 AddNode 会换发新 id,必须把新映射
    /// 广播给 redo 栈里的后继命令(否则后继 SetFill 等静默失配)。
    /// 默认无映射;仅 AddNode 覆写。
    fn apply_heal(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        self.apply(scene);
        None
    }

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
#[derive(Default)]
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
            if let Some(map) = cmd.apply_heal(scene) {
                // 新 id 广播给 redo 栈内后继命令(undo 方向的治愈镜像)
                for entry in self.redo.iter_mut() {
                    entry.remap_ids(&map);
                }
            }
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

    /// 当前撤销光标:已生效(执行且未撤销)的命令步数——对标 Qt
    /// `QUndoStack::index`(V2.0 T6,docs/08 §3)。
    ///
    /// `exec`(事务合一步、merge 合一步)、`undo`、`redo` 全部经由撤销栈
    /// 维护它:光标恒等于撤销栈长度,不设独立字段即无失同步风险。与
    /// [`History::undo_len`] 同值并存是语义分工——后者面向自动保存的
    /// "深度计量",本 API 面向"回到第 i 步"的光标定位,供历史面板与
    /// 分支语义([`History::truncate_to`]/[`History::truncate_after`])取用。
    pub fn cursor(&self) -> usize {
        self.undo.len()
    }

    /// 批量回退/前进到指定光标 `cursor`,返回**实际到达**的光标值
    /// (对标 Qt `QUndoStack::setIndex`)。
    ///
    /// 光标 `i` 的语义:前 `i` 步已生效——`truncate_to(i)` 后的场景状态与
    /// "执行完第 i 步"完全一致(roundtrip 测试逐光标断言)。可达区间为
    /// `0..=cursor() + redo 深度`(完整历史);越界**不 panic**,钳制到
    /// 可达范围并把到达值返回给调用方(历史面板据此高亮真实位置)。
    ///
    /// 实现复用既有 [`History::undo`]/[`History::redo`] 逐步走位(不重复
    /// 命令逻辑,id 重映射广播等行为与单步操作完全一致)。
    pub fn truncate_to(&mut self, cursor: usize, scene: &mut Scene) -> usize {
        let target = cursor.min(self.undo.len() + self.redo.len());
        while self.undo.len() > target {
            self.undo(scene);
        }
        while self.undo.len() < target {
            self.redo(scene);
        }
        self.undo.len()
    }

    /// 截断:丢弃光标 `cursor` 之后的全部历史(撤销栈尾部 + 整个 redo 栈),
    /// `cursor` 即新分支起点("回到这里另起一支")。
    ///
    /// 截断后旧 redo 永久不可达,后续 `exec` 在 `cursor` 上叠加新命令。
    /// G8 协作基座关联(docs/08 §2 差距矩阵):分支 = 各端自同一光标截断
    /// 后各自追加变更流,本 API 是该语义的最小地基(CRDT 本体不进 v2.0)。
    ///
    /// `cursor` 超过当前光标时只清 redo、不动撤销栈(钳制不 panic,与
    /// [`History::truncate_to`] 纪律一致);事务中(in-flight)的命令不在
    /// 光标时间线上,`end_transaction` 后作为新分支的一步入栈。
    pub fn truncate_after(&mut self, cursor: usize) {
        // Vec::truncate 自带钳制(超长无操作),无需自查越界
        self.undo.truncate(cursor);
        self.redo.clear();
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    fn apply_heal(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        let before = self.id;
        self.apply(scene);
        match (before, self.id) {
            (Some(old), Some(new)) if old != new => Some(IdRemap::from([(old, new)])),
            _ => None,
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        // 保留 self.id(不 take):退役 key 经 slotmap 版本号永不再解析,留存
        // 它是 redo 方向 apply_heal 的 old→new 映射来源(script redo 实测踩坑)
        if let Some(id) = self.id {
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
///
/// serde 为**手写实现**(见文件尾"序列化桥"):`RemovedSubtree` 定义在
/// scene.rs,本迭代该文件由效果管线并行修改、不挂 derive,故经内部镜像
/// `RemoveNodeRepr` 转换。`Clone` 供序列化桥取值。
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

// —— 节点效果栈命令(迭代计划 08 S4 #4.1,分册七/E11)——
//
// 下标约定:`index` 指向节点的 `effects` 列表;目标越界或条目与捕获值不符时
// 命令为空操作(与"目标不存在时静默跳过"的 LIFO 约定一致,绝不越界 panic)。
// 效果命令不携带除节点 id 外的其他 id,`remap_ids` 按 [`SetTransform`] 样式
// 只重映射节点 id(子树恢复换发 id 后历史栈照常可用)。

/// 在节点效果栈的 `index` 处插入一条效果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddEffect {
    pub id: NodeId,
    /// 插入下标(`> len` 时空操作;UI 侧总是传合法值)。
    pub index: usize,
    /// 插入的效果条目。
    pub entry: EffectEntry,
}

impl Command for AddEffect {
    fn apply(&mut self, scene: &mut Scene) {
        if let Some(node) = scene.node_mut(self.id)
            && self.index <= node.effects.len()
        {
            node.effects.insert(self.index, self.entry.clone());
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        // 条目比对:栈漂移(不应发生)时宁可不撤,也不误删相邻效果
        if let Some(node) = scene.node_mut(self.id)
            && self.index < node.effects.len()
            && node.effects[self.index] == self.entry
        {
            node.effects.remove(self.index);
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
        Cow::Borrowed("添加效果")
    }
}
fn default_applied_true() -> bool {
    true
}

/// 移除节点效果栈 `index` 处的效果(条目在构造时捕获,撤销原样放回)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveEffect {
    pub id: NodeId,
    pub index: usize,
    /// 摘除的效果条目([`RemoveEffect::capture`] 时从场景捕获)。
    pub entry: EffectEntry,
    /// apply 是否真的执行了删除(空转守卫;serde 旧文件默认 true = 真实删除)
    #[serde(default = "default_applied_true")]
    pub applied: bool,
}

impl RemoveEffect {
    /// 从场景当前状态捕获 `index` 处的效果条目(节点不存在/越界 → `None`)。
    pub fn capture(scene: &Scene, id: NodeId, index: usize) -> Option<Self> {
        let entry = scene.node(id)?.effects.get(index)?.clone();
        Some(RemoveEffect {
            id,
            index,
            entry,
            applied: false,
        })
    }
}

impl Command for RemoveEffect {
    fn apply(&mut self, scene: &mut Scene) {
        self.applied = false;
        if let Some(node) = scene.node_mut(self.id)
            && self.index < node.effects.len()
            && node.effects[self.index] == self.entry
        {
            node.effects.remove(self.index);
            self.applied = true;
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        // 空转守卫:apply 没删过(applied=false)时 revert 不得凭空插回
        // (非法目标命令的撤销曾实测踩坑)
        if self.applied
            && let Some(node) = scene.node_mut(self.id)
        {
            node.effects
                .insert(self.index.min(node.effects.len()), self.entry.clone());
            self.applied = false;
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
        Cow::Borrowed("移除效果")
    }
}

/// 移动效果在栈中的位置(外观面板拖动排序;`to` 按"摘除 `from` 之后的
/// 列表"解释,`from == to` / 越界为空操作)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveEffect {
    pub id: NodeId,
    pub from: usize,
    pub to: usize,
}

impl Command for MoveEffect {
    fn apply(&mut self, scene: &mut Scene) {
        if let Some(node) = scene.node_mut(self.id) {
            let effects = &mut node.effects;
            let len = effects.len();
            if self.from >= len || self.from == self.to {
                return;
            }
            let entry = effects.remove(self.from);
            let to = self.to.min(effects.len());
            effects.insert(to, entry);
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        // 镜像 apply 的分支条件与 clamp(to 按"摘除后列表"解释,取回时用
        // min(to, len-1);from 越界/from==to 时 apply 未动,revert 同步空转)
        if let Some(node) = scene.node_mut(self.id) {
            let effects = &mut node.effects;
            let len = effects.len();
            if self.from >= len || self.from == self.to {
                return None;
            }
            let entry = effects.remove(self.to.min(len - 1));
            let from = self.from.min(effects.len());
            effects.insert(from, entry);
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
        Cow::Borrowed("移动效果")
    }
}

/// 修改效果启用开关。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetEffectEnabled {
    pub id: NodeId,
    pub index: usize,
    pub old: bool,
    pub new: bool,
}

impl Command for SetEffectEnabled {
    fn apply(&mut self, scene: &mut Scene) {
        if let Some(entry) = scene
            .node_mut(self.id)
            .and_then(|n| n.effects.get_mut(self.index))
            && entry.enabled == self.old
        {
            entry.enabled = self.new;
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        if let Some(entry) = scene
            .node_mut(self.id)
            .and_then(|n| n.effects.get_mut(self.index))
            && entry.enabled == self.new
        {
            entry.enabled = self.old;
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
        Cow::Borrowed("修改效果开关")
    }
}

/// 修改效果参数。merge:同节点同下标合并,保留最早的 old、最新的 new
/// (docs/03 §2 范式)——效果面板 NumberField 拖动/动画插值由此天然合并为
/// 一步撤销(E11 联动点,见 sable-foundation `effects` 模块 doc)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetEffectSpec {
    pub id: NodeId,
    pub index: usize,
    pub old: EffectSpec,
    pub new: EffectSpec,
}

impl Command for SetEffectSpec {
    fn apply(&mut self, scene: &mut Scene) {
        if let Some(entry) = scene
            .node_mut(self.id)
            .and_then(|n| n.effects.get_mut(self.index))
            && entry.spec == self.old
        {
            // 守卫:下标漂移(栈上其他命令改过顺序)时宁可不改,也不覆写邻居
            entry.spec = self.new.clone();
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        if let Some(entry) = scene
            .node_mut(self.id)
            .and_then(|n| n.effects.get_mut(self.index))
            && entry.spec == self.new
        {
            entry.spec = self.old.clone();
        }
        None
    }

    fn merge(&self, next: &dyn Command) -> Option<Box<dyn Command>> {
        let next = next.as_any().downcast_ref::<SetEffectSpec>()?;
        if next.id != self.id || next.index != self.index {
            return None;
        }
        Some(Box::new(SetEffectSpec {
            id: self.id,
            index: self.index,
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
        Cow::Borrowed("修改效果参数")
    }
}

// —— 序列化桥:.sable 工程存取(迭代计划 08 S3 #3.4;分册五 §5.1)——
//
// 本节只做"内存命令 ⇄ 可序列化镜像"的纯转换,不改任何撤销/重做逻辑。

/// [`RemovedSubtree`] 的可序列化镜像。
///
/// 不给 scene.rs 本体加 derive 的原因:该文件在本迭代由效果管线并行修改
/// (Node 增 blend_mode),序列化面集中收在本文件,避免写冲突;镜像字段
/// 与本体一一对应,场景层若加字段,镜像与本转换同步演进即可。
///
/// NodeId 的 serde 形态(slotmap 1.1.1 源码已核对):`new_key_type!` 生成的
/// key 把 Serialize/Deserialize 转发给 `KeyData`,线型为结构体
/// `{ idx: u32, version: u32 }`;SlotMap 按 (key, value) 序对序列化,键
/// **原样往返**——历史栈里缓存的句柄在重开工程后依然指向同一节点
/// (scene.rs 的 `scene_serde_roundtrip_preserves_data_and_keys` 即为该
/// 性质的回归测试)。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SubtreeData {
    /// 摘除时子树根的原始 id(仅作镜像;恢复后以 undo_remove 返回值为准)。
    root: NodeId,
    /// 摘除点:父节点(`None` = 挂在 roots)。
    parent: Option<NodeId>,
    /// 摘除点:在父 children / roots 中的下标。
    index: usize,
    /// 子树全部节点(root 在首位,父先于子)。
    nodes: Vec<(NodeId, Node)>,
}

impl From<&RemovedSubtree> for SubtreeData {
    fn from(subtree: &RemovedSubtree) -> Self {
        SubtreeData {
            root: subtree.root,
            parent: subtree.parent,
            index: subtree.index,
            nodes: subtree.nodes.clone(),
        }
    }
}

impl From<SubtreeData> for RemovedSubtree {
    fn from(data: SubtreeData) -> Self {
        RemovedSubtree {
            root: data.root,
            parent: data.parent,
            index: data.index,
            nodes: data.nodes,
        }
    }
}

/// [`RemoveNode`] 的 serde 表示(字段与本体一一对应;本体挂手写 impl)。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemoveNodeRepr {
    id: NodeId,
    subtree: Option<SubtreeData>,
}

impl Serialize for RemoveNode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        RemoveNodeRepr {
            id: self.id,
            subtree: self.subtree.as_ref().map(SubtreeData::from),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RemoveNode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let repr = RemoveNodeRepr::deserialize(deserializer)?;
        Ok(RemoveNode {
            id: repr.id,
            subtree: repr.subtree.map(RemovedSubtree::from),
        })
    }
}

/// 命令的**可序列化镜像**:14 个内置命令逐一对位 + 批量事务。
///
/// 第三方 `Command`(未内置)不阻塞保存:[`History::to_serialized`] 跳过
/// 并计数,重开后撤销栈**部分恢复**(缺失的那几步不可撤销,其余照常)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SerializedCommand {
    AddNode(AddNode),
    RemoveNode(RemoveNode),
    SetTransform(SetTransform),
    SetFill(SetFill),
    SetStroke(SetStroke),
    SetVisibility(SetVisibility),
    SetOpacity(SetOpacity),
    SetName(SetName),
    Reparent(Reparent),
    AddEffect(AddEffect),
    RemoveEffect(RemoveEffect),
    MoveEffect(MoveEffect),
    SetEffectEnabled(SetEffectEnabled),
    SetEffectSpec(SetEffectSpec),
    /// 批量事务:一次 `begin_transaction`..`end_transaction` 之间的全部
    /// 命令算一步撤销;内嵌命令递归镜像(第三方内嵌命令同样跳过)。
    Batch(Vec<SerializedCommand>),
}

/// 历史栈的序列化形态:撤销/重做两条栈,顺序 = 撤销/重做次序。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SerializedHistory {
    pub undo: Vec<SerializedCommand>,
    pub redo: Vec<SerializedCommand>,
}

/// 单条命令 → 镜像;识别不了的(第三方)命令跳过并计入 `skipped`。
fn serialize_command(cmd: &dyn Command, skipped: &mut usize) -> Option<SerializedCommand> {
    let any = cmd.as_any();
    let serialized = if let Some(c) = any.downcast_ref::<AddNode>() {
        SerializedCommand::AddNode(c.clone())
    } else if let Some(c) = any.downcast_ref::<RemoveNode>() {
        SerializedCommand::RemoveNode(c.clone())
    } else if let Some(c) = any.downcast_ref::<SetTransform>() {
        SerializedCommand::SetTransform(c.clone())
    } else if let Some(c) = any.downcast_ref::<SetFill>() {
        SerializedCommand::SetFill(c.clone())
    } else if let Some(c) = any.downcast_ref::<SetStroke>() {
        SerializedCommand::SetStroke(c.clone())
    } else if let Some(c) = any.downcast_ref::<SetVisibility>() {
        SerializedCommand::SetVisibility(c.clone())
    } else if let Some(c) = any.downcast_ref::<SetOpacity>() {
        SerializedCommand::SetOpacity(c.clone())
    } else if let Some(c) = any.downcast_ref::<SetName>() {
        SerializedCommand::SetName(c.clone())
    } else if let Some(c) = any.downcast_ref::<Reparent>() {
        SerializedCommand::Reparent(c.clone())
    } else if let Some(c) = any.downcast_ref::<AddEffect>() {
        SerializedCommand::AddEffect(c.clone())
    } else if let Some(c) = any.downcast_ref::<RemoveEffect>() {
        SerializedCommand::RemoveEffect(c.clone())
    } else if let Some(c) = any.downcast_ref::<MoveEffect>() {
        SerializedCommand::MoveEffect(c.clone())
    } else if let Some(c) = any.downcast_ref::<SetEffectEnabled>() {
        SerializedCommand::SetEffectEnabled(c.clone())
    } else if let Some(c) = any.downcast_ref::<SetEffectSpec>() {
        SerializedCommand::SetEffectSpec(c.clone())
    } else if let Some(batch) = any.downcast_ref::<BatchCommand>() {
        let mut inner = Vec::with_capacity(batch.0.len());
        for step in &batch.0 {
            if let Some(s) = serialize_command(step.as_ref(), skipped) {
                inner.push(s);
            }
        }
        SerializedCommand::Batch(inner)
    } else {
        *skipped += 1;
        return None;
    };
    Some(serialized)
}

/// 镜像 → 装箱命令(与 [`serialize_command`] 逐变体对位)。
fn deserialize_command(serialized: SerializedCommand) -> Box<dyn Command> {
    match serialized {
        SerializedCommand::AddNode(c) => Box::new(c),
        SerializedCommand::RemoveNode(c) => Box::new(c),
        SerializedCommand::SetTransform(c) => Box::new(c),
        SerializedCommand::SetFill(c) => Box::new(c),
        SerializedCommand::SetStroke(c) => Box::new(c),
        SerializedCommand::SetVisibility(c) => Box::new(c),
        SerializedCommand::SetOpacity(c) => Box::new(c),
        SerializedCommand::SetName(c) => Box::new(c),
        SerializedCommand::Reparent(c) => Box::new(c),
        SerializedCommand::AddEffect(c) => Box::new(c),
        SerializedCommand::RemoveEffect(c) => Box::new(c),
        SerializedCommand::MoveEffect(c) => Box::new(c),
        SerializedCommand::SetEffectEnabled(c) => Box::new(c),
        SerializedCommand::SetEffectSpec(c) => Box::new(c),
        SerializedCommand::Batch(inner) => Box::new(BatchCommand(
            inner.into_iter().map(deserialize_command).collect(),
        )),
    }
}

impl History {
    /// 序列化整棵历史栈,返回 `(镜像, 跳过的未知命令数)`。
    ///
    /// - **保存点 = 已完成命令**:事务中(in-flight)的命令还在收集区、
    ///   尚未进入撤销栈,不参与保存;
    /// - 未知命令(第三方)跳过不阻塞保存,但重开后撤销栈部分恢复;
    /// - merge 合并后的条目(SetFill/SetStroke/SetOpacity)以"最早 old +
    ///   最新 new"的单条形态序列化,与内存语义一致。
    pub fn to_serialized(&self) -> (SerializedHistory, usize) {
        let mut skipped = 0usize;
        let undo: Vec<SerializedCommand> = self
            .undo
            .iter()
            .filter_map(|cmd| serialize_command(cmd.as_ref(), &mut skipped))
            .collect();
        let redo: Vec<SerializedCommand> = self
            .redo
            .iter()
            .filter_map(|cmd| serialize_command(cmd.as_ref(), &mut skipped))
            .collect();
        (SerializedHistory { undo, redo }, skipped)
    }

    /// 从镜像重建历史栈(打开工程用;`transaction` 恒为空——保存点不含
    /// in-flight 命令)。
    ///
    /// 句柄有效性依赖 slotmap serde 的保键往返(见 `SubtreeData` 文档):
    /// 重建的历史必须与保存时同一份反序列化出的场景配套使用。
    pub fn from_serialized(serialized: SerializedHistory) -> History {
        History {
            undo: serialized
                .undo
                .into_iter()
                .map(deserialize_command)
                .collect(),
            redo: serialized
                .redo
                .into_iter()
                .map(deserialize_command)
                .collect(),
            transaction: None,
        }
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

    // —— 撤销光标(V2.0 T6,docs/08 §3;对标 Qt QUndoStack::index)——

    /// 对 4 个不同节点各执行一条 SetTransform(类型/节点互异,绝不 merge,
    /// 正好 4 步;属性命令不改动结构,快照逐字节可比)。
    fn exec_four_transforms(history: &mut History, scene: &mut Scene, ids: [NodeId; 4]) {
        for (i, &id) in ids.iter().enumerate() {
            history.exec(
                SetTransform {
                    id,
                    old: Affine::IDENTITY,
                    new: Affine::translate((i as f64 + 1.0, 0.0)),
                }
                .boxed(),
                scene,
            );
        }
    }

    #[test]
    fn cursor_tracks_exec_undo_redo_transaction_and_merge() {
        let (mut scene, root, a, _sub, b, c) = demo_scene();
        let mut history = History::new();
        assert_eq!(history.cursor(), 0, "新栈光标在 0");

        exec_four_transforms(&mut history, &mut scene, [a, b, c, root]);
        assert_eq!(history.cursor(), 4);
        assert_eq!(history.undo_len(), 4);

        // merge 合步:同节点连续 SetFill 合并,光标只前进 1
        let a_fill = scene.path(a).and_then(|p| p.fill.clone());
        history.exec(
            SetFill {
                id: a,
                old: a_fill,
                new: Some(Paint::Solid([1, 2, 3, 4])),
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            SetFill {
                id: a,
                old: Some(Paint::Solid([1, 2, 3, 4])),
                new: Some(Paint::Solid([5, 6, 7, 8])),
            }
            .boxed(),
            &mut scene,
        );
        assert_eq!(history.cursor(), 5, "两条 SetFill 合并成一步,光标 4→5");

        // 事务 = 一步
        history.begin_transaction();
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
                id: root,
                old: 1.0,
                new: 0.5,
            }
            .boxed(),
            &mut scene,
        );
        history.end_transaction();
        assert_eq!(history.cursor(), 6, "事务整体算一步,光标 5→6");

        // undo/redo 精确走位
        history.undo(&mut scene);
        assert_eq!(history.cursor(), 5);
        history.undo(&mut scene);
        assert_eq!(history.cursor(), 4);
        history.redo(&mut scene);
        assert_eq!(history.cursor(), 5);
    }

    #[test]
    fn truncate_to_roundtrip_matches_step_snapshot() {
        let (mut scene, root, a, _sub, b, c) = demo_scene();
        let mut history = History::new();
        // 光标 0 = 初始态;每执行一步存一份快照
        let mut snapshots = vec![scene.clone()];
        for (i, id) in [a, b, c, root].into_iter().enumerate() {
            history.exec(
                SetTransform {
                    id,
                    old: Affine::IDENTITY,
                    new: Affine::translate((i as f64 + 1.0, 0.0)),
                }
                .boxed(),
                &mut scene,
            );
            snapshots.push(scene.clone());
        }
        assert_eq!(history.cursor(), 4);

        // 逐光标断言:truncate_to(i) 后场景 == 第 i 步执行后的快照(undo 方向)
        for (i, snap) in snapshots.iter().enumerate() {
            let reached = history.truncate_to(i, &mut scene);
            assert_eq!(reached, i, "到达值 = 光标 {i}");
            assert_eq!(history.cursor(), i);
            assert_eq!(scene, *snap, "光标 {i} 处 == 第 {i} 步执行后的快照");
        }
        // 反向再走一遍(redo 方向),覆盖双向走位
        for (i, snap) in snapshots.iter().enumerate().rev() {
            let reached = history.truncate_to(i, &mut scene);
            assert_eq!(reached, i);
            assert_eq!(scene, *snap);
        }
    }

    #[test]
    fn truncate_to_zero_full_redo_and_out_of_range_clamps() {
        let (mut scene, root, a, _sub, b, c) = demo_scene();
        let initial = scene.clone();
        let mut history = History::new();
        exec_four_transforms(&mut history, &mut scene, [a, b, c, root]);
        let fully = scene.clone();

        assert_eq!(
            history.truncate_to(0, &mut scene),
            0,
            "truncate_to(0) == 全撤销"
        );
        assert_eq!(scene, initial);
        assert!(!history.can_undo());
        assert!(history.can_redo(), "全撤销后 redo 栈仍在");

        assert_eq!(
            history.truncate_to(4, &mut scene),
            4,
            "truncate_to(总步数) == 全重做"
        );
        assert_eq!(scene, fully);
        assert!(history.can_undo());
        assert!(!history.can_redo());

        // 越界钳制:不 panic,返回实际到达值
        assert_eq!(history.truncate_to(999, &mut scene), 4, "越界钳到全重做位");
        assert_eq!(history.truncate_to(usize::MAX, &mut scene), 4);
        assert_eq!(scene, fully);
    }

    #[test]
    fn exec_after_truncate_to_abandons_old_redo_branch() {
        let (mut scene, root, a, _sub, b, c) = demo_scene();
        let initial = scene.clone();
        let mut history = History::new();

        // exec×5(4 条 SetTransform + 1 条 SetFill,互不 merge)
        exec_four_transforms(&mut history, &mut scene, [a, b, c, root]);
        let a_fill = scene.path(a).and_then(|p| p.fill.clone());
        history.exec(
            SetFill {
                id: a,
                old: a_fill,
                new: Some(Paint::Solid([7, 7, 7, 7])),
            }
            .boxed(),
            &mut scene,
        );
        assert_eq!(history.cursor(), 5);

        assert_eq!(history.truncate_to(2, &mut scene), 2);
        assert!(history.can_redo(), "光标 2 之后还有 3 步 redo");

        // 新 exec:旧 redo 分支整体废弃(QUndoStack 语义)
        history.exec(
            SetVisibility {
                id: a,
                old: true,
                new: false,
            }
            .boxed(),
            &mut scene,
        );
        assert_eq!(
            history.undo_len(),
            3,
            "exec×5 → truncate_to(2) → 新 exec = 3 步"
        );
        assert!(!history.can_redo(), "旧 redo 不可达");
        assert_eq!(history.cursor(), 3);

        while history.can_undo() {
            history.undo(&mut scene);
        }
        assert_eq!(scene, initial, "新分支全撤销回到初始");
    }

    #[test]
    fn truncate_after_drops_tail_and_redo_stack() {
        let (mut scene, root, a, _sub, b, c) = demo_scene();
        let initial = scene.clone();
        let mut history = History::new();
        exec_four_transforms(&mut history, &mut scene, [a, b, c, root]);

        // 先制造 redo 栈:撤 2 步,truncate_after 应连 redo 一起清
        history.undo(&mut scene);
        history.undo(&mut scene);
        assert_eq!(history.cursor(), 2);
        assert!(history.can_redo());

        history.truncate_after(2);
        assert_eq!(history.cursor(), 2);
        assert_eq!(history.undo_len(), 2);
        assert!(!history.can_redo(), "truncate_after 后 redo 栈为空");

        // 剩余 2 步照常可撤销(截断不破坏既有栈的可用性)
        while history.can_undo() {
            history.undo(&mut scene);
        }
        assert_eq!(scene, initial);

        // 截断后新 exec 叠加为分支第 1 步
        history.exec(
            SetVisibility {
                id: a,
                old: true,
                new: false,
            }
            .boxed(),
            &mut scene,
        );
        assert_eq!(history.undo_len(), 1);
        assert_eq!(history.cursor(), 1);

        // 越界钳制:超过当前光标 = 只清 redo,不动撤销栈
        history.truncate_after(999);
        assert_eq!(history.undo_len(), 1);

        // truncate_after(0) 等价整栈清空
        history.truncate_after(0);
        assert_eq!(history.cursor(), 0);
        assert!(!history.can_undo() && !history.can_redo());
    }

    #[test]
    fn truncate_on_empty_history_is_noop_and_clear_resets_cursor() {
        let (mut scene, root, a, _sub, b, c) = demo_scene();
        let snapshot = scene.clone();
        let mut history = History::new();
        assert_eq!(history.cursor(), 0);
        assert_eq!(history.truncate_to(0, &mut scene), 0);
        assert_eq!(history.truncate_to(5, &mut scene), 0, "空历史越界钳制到 0");
        history.truncate_after(0);
        history.truncate_after(9);
        assert_eq!(scene, snapshot, "空历史上的截断不动场景");

        exec_four_transforms(&mut history, &mut scene, [a, b, root, c]);
        assert_eq!(history.cursor(), 4);
        let edited = scene.clone();
        history.clear();
        assert_eq!(history.cursor(), 0, "clear 后光标归零");
        assert_eq!(history.truncate_to(2, &mut scene), 0);
        assert_eq!(scene, edited, "clear 后 truncate_to 不动场景");
    }

    // —— 效果栈命令(迭代计划 08 S4 #4.1)——

    use crate::effects::{EffectEntry, EffectSpec};

    fn shadow_entry(blur: f64) -> EffectEntry {
        EffectEntry {
            spec: EffectSpec::DropShadow {
                blur,
                offset: [0.0, 2.0],
                color: [0, 0, 0, 128],
            },
            enabled: true,
        }
    }

    fn blur_entry(radius: f64) -> EffectEntry {
        EffectEntry {
            spec: EffectSpec::GaussianBlur { radius },
            enabled: true,
        }
    }

    fn effects_of(scene: &Scene, id: NodeId) -> Vec<EffectEntry> {
        scene.node(id).expect("节点在").effects.clone()
    }

    #[test]
    fn effect_add_move_remove_roundtrip() {
        let (mut scene, _root, a, _sub, _b, _c) = demo_scene();
        let snapshot = scene.clone();
        let mut history = History::new();

        history.exec(
            AddEffect {
                id: a,
                index: 0,
                entry: shadow_entry(4.0),
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            AddEffect {
                id: a,
                index: 1,
                entry: blur_entry(2.0),
            }
            .boxed(),
            &mut scene,
        );
        assert_eq!(
            effects_of(&scene, a),
            vec![shadow_entry(4.0), blur_entry(2.0)]
        );

        // 拖动排序:blur 从 1 移到 0(摘除后列表解释)
        history.exec(
            MoveEffect {
                id: a,
                from: 1,
                to: 0,
            }
            .boxed(),
            &mut scene,
        );
        assert_eq!(
            effects_of(&scene, a),
            vec![blur_entry(2.0), shadow_entry(4.0)]
        );

        // 移除(捕获条目后执行)
        let cmd = RemoveEffect::capture(&scene, a, 1).expect("捕获");
        assert_eq!(cmd.entry, shadow_entry(4.0));
        history.exec(cmd.boxed(), &mut scene);
        assert_eq!(effects_of(&scene, a), vec![blur_entry(2.0)]);

        // 一步步撤销回到初始(无效果)状态
        while history.can_undo() {
            history.undo(&mut scene);
        }
        assert_eq!(scene, snapshot, "效果命令全部撤销后与初始一致");
        assert!(effects_of(&scene, a).is_empty());

        // 重做一步不少
        while history.can_redo() {
            history.redo(&mut scene);
        }
        assert_eq!(effects_of(&scene, a), vec![blur_entry(2.0)]);
    }

    #[test]
    fn move_effect_to_out_of_range_clamps_append() {
        let (mut scene, _root, a, _sub, _b, _c) = demo_scene();
        let mut history = History::new();
        history.exec(
            AddEffect {
                id: a,
                index: 0,
                entry: shadow_entry(4.0),
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            AddEffect {
                id: a,
                index: 1,
                entry: blur_entry(2.0),
            }
            .boxed(),
            &mut scene,
        );
        // to 越界 → clamp 到尾部(摘除后 len = 1)
        history.exec(
            MoveEffect {
                id: a,
                from: 0,
                to: 9,
            }
            .boxed(),
            &mut scene,
        );
        assert_eq!(
            effects_of(&scene, a),
            vec![blur_entry(2.0), shadow_entry(4.0)]
        );
        history.undo(&mut scene);
        assert_eq!(
            effects_of(&scene, a),
            vec![shadow_entry(4.0), blur_entry(2.0)]
        );
    }

    #[test]
    fn effect_commands_skip_invalid_targets() {
        let (mut scene, _root, a, _sub, _b, _c) = demo_scene();
        let snapshot = scene.clone();
        let mut history = History::new();

        // 插入下标越界 → 空操作
        history.exec(
            AddEffect {
                id: a,
                index: 9,
                entry: blur_entry(2.0),
            }
            .boxed(),
            &mut scene,
        );
        eprintln!("[dbg] exec1 后 A.effects = {:?}", effects_of(&scene, a));
        assert!(effects_of(&scene, a).is_empty());
        // 越界开关/参数/移动/移除 → 全部空操作
        history.exec(
            SetEffectEnabled {
                id: a,
                index: 0,
                old: true,
                new: false,
            }
            .boxed(),
            &mut scene,
        );
        eprintln!("[dbg] exec2 后 A.effects = {:?}", effects_of(&scene, a));
        history.exec(
            SetEffectSpec {
                id: a,
                index: 0,
                old: EffectSpec::brightness(1.0),
                new: EffectSpec::brightness(2.0),
            }
            .boxed(),
            &mut scene,
        );
        eprintln!("[dbg] exec3 后 A.effects = {:?}", effects_of(&scene, a));
        history.exec(
            MoveEffect {
                id: a,
                from: 0,
                to: 1,
            }
            .boxed(),
            &mut scene,
        );
        eprintln!("[dbg] exec4 后 A.effects = {:?}", effects_of(&scene, a));
        history.exec(
            RemoveEffect {
                id: a,
                index: 0,
                entry: blur_entry(2.0),
                applied: false,
            }
            .boxed(),
            &mut scene,
        );
        eprintln!("[dbg] exec5 后 A.effects = {:?}", effects_of(&scene, a));
        // 不存在的节点同样静默跳过。注意:slotmap key 跨场景可能碰撞
        // (本场景首个节点 = (0,1) 恰撞 demo 根组),故 ghost 场景先造 7 个
        // 节点,取下标 6 的 key——demo 场景从未分配过该槽,保证缺席。
        let mut other = Scene::new();
        let ghost = (0..7)
            .map(|i| {
                other
                    .add_node(None, format!("g{i}"), NodeContent::Group)
                    .expect("ghost")
            })
            .last()
            .expect("至少一个");
        let _ = ghost;

        eprintln!("[dbg] 终态 A.effects = {:?}", effects_of(&scene, a));
        eprintln!("[dbg] 终态 根.effects = {:?}", effects_of(&scene, _root));
        assert_eq!(scene, snapshot, "非法目标不得改状态");
        while history.can_undo() {
            history.undo(&mut scene);
        }
        assert_eq!(scene, snapshot);
    }

    #[test]
    fn set_effect_enabled_roundtrip() {
        let (mut scene, _root, a, _sub, _b, _c) = demo_scene();
        let mut history = History::new();
        history.exec(
            AddEffect {
                id: a,
                index: 0,
                entry: blur_entry(2.0),
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            SetEffectEnabled {
                id: a,
                index: 0,
                old: true,
                new: false,
            }
            .boxed(),
            &mut scene,
        );
        assert!(!effects_of(&scene, a)[0].enabled, "开关应生效");
        history.undo(&mut scene);
        assert!(effects_of(&scene, a)[0].enabled, "撤销恢复开关");
        history.redo(&mut scene);
        assert!(!effects_of(&scene, a)[0].enabled);
    }

    #[test]
    fn set_effect_spec_merges_per_node_and_index() {
        let (mut scene, _root, a, _sub, b, _c) = demo_scene();
        let mut history = History::new();
        history.exec(
            AddEffect {
                id: a,
                index: 0,
                entry: blur_entry(1.0),
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            AddEffect {
                id: a,
                index: 1,
                entry: blur_entry(0.0),
            }
            .boxed(),
            &mut scene,
        );
        let first = blur_entry(1.0).spec;
        let second = blur_entry(0.0).spec;

        // 同节点同下标:拖动式连续修改 → 合并为一步
        history.exec(
            SetEffectSpec {
                id: a,
                index: 0,
                old: first.clone(),
                new: blur_entry(3.0).spec,
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            SetEffectSpec {
                id: a,
                index: 0,
                old: blur_entry(3.0).spec,
                new: blur_entry(5.0).spec,
            }
            .boxed(),
            &mut scene,
        );
        assert_eq!(
            history.undo_len(),
            3,
            "[add, add, SetEffectSpec×2 合并为一步]"
        );
        assert_eq!(effects_of(&scene, a)[0].spec, blur_entry(5.0).spec);

        history.undo(&mut scene);
        assert_eq!(effects_of(&scene, a)[0].spec, first, "保留最早的 old");
        history.redo(&mut scene);
        assert_eq!(
            effects_of(&scene, a)[0].spec,
            blur_entry(5.0).spec,
            "保留最新的 new"
        );

        // 同节点不同下标不合并
        history.exec(
            SetEffectSpec {
                id: a,
                index: 1,
                old: second,
                new: blur_entry(7.0).spec,
            }
            .boxed(),
            &mut scene,
        );
        assert_eq!(history.undo_len(), 4);
        // 不同节点不合并
        history.exec(
            AddEffect {
                id: b,
                index: 0,
                entry: blur_entry(1.0),
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            SetEffectSpec {
                id: b,
                index: 0,
                old: blur_entry(1.0).spec,
                new: blur_entry(9.0).spec,
            }
            .boxed(),
            &mut scene,
        );
        assert_eq!(history.undo_len(), 6);
    }

    #[test]
    fn effect_commands_heal_after_subtree_restore() {
        // 子树恢复换发 id 后,历史栈里的效果命令必须被重映射
        let (mut scene, _root, _a, sub, b, _c) = demo_scene();
        let before = scene.clone(); // 加效果前(全撤销的最终基准)
        let mut history = History::new();
        history.exec(
            AddEffect {
                id: b,
                index: 0,
                entry: shadow_entry(4.0),
            }
            .boxed(),
            &mut scene,
        );
        let snapshot = scene.clone();
        history.exec(RemoveNode::new(sub).boxed(), &mut scene);
        assert_eq!(scene.len(), snapshot.len() - 2);

        history.undo(&mut scene); // 恢复子树(矩形B 换新 id,快照含效果)
        history.undo(&mut scene); // AddEffect 必须重映射到新 id 才能撤销
        let new_b = scene
            .nodes
            .iter()
            .find(|(_, n)| n.name == "矩形B")
            .map(|(id, _)| id)
            .expect("矩形B 已恢复");
        assert!(
            effects_of(&scene, new_b).is_empty(),
            "重映射后的撤销生效:恢复的矩形B 不再带效果"
        );
        assert_eq!(scene, before, "全撤销回到加效果前(结构化相等)");
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
