//! 场景图:文档的内存表示(纯数据)。
//!
//! 场景图源自项目设计文档 docs/02 §3,**架构决策:从原方案所在的 sable-canvas
//! 下沉进 sable-foundation**——Command/History 必须能命名被编辑状态(AGENTS.md §3.1),
//! 而 Scene 是纯数据(kurbo/slotmap/serde 全在 core 白名单内);canvas crate 保留
//! 行为层(命中测试/网格/渲染调度)。
//!
//! 刻意不用 `peniko::Brush`:序列化友好 + core 不背 peniko 的 serde 面,
//! 用自有 [`Paint`] 枚举表达 Solid / LinearGradient / RadialGradient。
//!
//! # id 身份与撤销的相互作用
//!
//! slotmap 节点被移除后,其 slot 版本号单调递增,安全 API 无法恢复原 key。
//! 因此 [`Scene::undo_remove`] 恢复子树时**换发新 id** 并重映射子树内部链接;
//! 命令层(command.rs)负责追踪"当前生效 id"。

use std::collections::HashMap;

use kurbo::{Affine, BezPath, Rect, Shape};
use serde::{Deserialize, Serialize};
use slotmap::{SlotMap, new_key_type};

use crate::effects::EffectEntry;
use crate::error::{CoreError, CoreResult};

new_key_type! {
    /// 场景节点句柄(slotmap key,值语义;序列化后可跨进程往返)。
    pub struct NodeId;
}

/// 0-255 RGBA 颜色。v0.1 用字节组而非 peniko::Color:零依赖、serde 直出。
pub type Rgba8 = [u8; 4];

/// 节点 id 重映射:摘除子树被恢复时,旧 id → 恢复后新 id。
/// 命令系统用它修正历史栈里引用了已恢复子树的命令(见 command.rs)。
pub type IdRemap = HashMap<NodeId, NodeId>;

/// 渐变色标。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub offset: f32,
    pub color: Rgba8,
}

/// 节点混合模式(迭代计划 08 E5,Illustrator 图层面板同款)。
///
/// 一一对应 `peniko::BlendMode` 的 `Mix` 轴(16 种;`Compose` 轴 v0.1 恒为
/// `SrcOver`,不进数据模型)。枚举值只做数据,映射到具体后端的
/// `peniko::BlendMode` 在 sable-paint 完成(降级矩阵 E12:CPU 端
/// vello_cpu 0.2 原生支持混合层,无 CPU 降级损失)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BlendMode {
    /// 正常(默认;peniko `Mix::Normal`)
    #[default]
    Normal,
    /// 正片叠底(peniko `Mix::Multiply`)
    Multiply,
    /// 滤色(peniko `Mix::Screen`)
    Screen,
    /// 叠加(peniko `Mix::Overlay`)
    Overlay,
    /// 变暗(peniko `Mix::Darken`)
    Darken,
    /// 变亮(peniko `Mix::Lighten`)
    Lighten,
    /// 颜色减淡(peniko `Mix::ColorDodge`)
    ColorDodge,
    /// 颜色加深(peniko `Mix::ColorBurn`)
    ColorBurn,
    /// 强光(peniko `Mix::HardLight`)
    HardLight,
    /// 柔光(peniko `Mix::SoftLight`)
    SoftLight,
    /// 差值(peniko `Mix::Difference`)
    Difference,
    /// 排除(peniko `Mix::Exclusion`)
    Exclusion,
    /// 色相(peniko `Mix::Hue`)
    Hue,
    /// 饱和度(peniko `Mix::Saturation`)
    Saturation,
    /// 颜色(peniko `Mix::Color`)
    Color,
    /// 明度(peniko `Mix::Luminosity`)
    Luminosity,
}

/// 填充/描边用的绘制源。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Paint {
    Solid(Rgba8),
    LinearGradient {
        start: [f64; 2],
        end: [f64; 2],
        stops: Vec<GradientStop>,
    },
    RadialGradient {
        center: [f64; 2],
        radius: f64,
        stops: Vec<GradientStop>,
    },
    /// 锥形渐变(迭代计划 08 E9;Illustrator/CSS conic-gradient 同款)。
    ///
    /// 角度一律**弧度**,自正 X 轴起、Y 轴向下坐标系中顺时针测量——与
    /// peniko 0.6 `SweepGradientPosition`(f32 弧度)的约定一致,渲染侧
    /// 只做 f64→f32 一次性降位。`start_angle == end_angle` 无扫描区间,
    /// 渲染侧表现为首色标纯色(不报错)。
    ConicGradient {
        center: [f64; 2],
        start_angle: f64,
        end_angle: f64,
        stops: Vec<GradientStop>,
    },
}

/// 网格渐变(迭代计划 08 E9 数据模型;渲染接入 = S4)。
///
/// 四角 Coons patch:四角位置 + 四角颜色,`eval` 做双线性颜色插值
/// (Coons patch 的退化形式——位置场未参与插值,渲染侧 S4 再补完整
/// Coons/WGSL 求值)。纯数据 + 纯函数,core 零依赖约束不破。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshGradient {
    /// 四角位置,顺序固定:`[左下, 右下, 右上, 左上]`(参数域 (u,v) =
    /// (0,0) → (1,0) → (1,1) → (0,1),逆时针)。
    pub corners: [[f64; 2]; 4],
    /// 四角颜色,与 [`MeshGradient::corners`] 一一对应。
    pub corner_colors: [Rgba8; 4],
    /// 渲染细分密度(条数;S4 渲染接入时使用,数据模型先行定义)。
    pub subdivisions: u32,
}

impl MeshGradient {
    /// 双线性(Coons patch 退化形)颜色插值:`(u, v) ∈ [0, 0]~[1, 1]`,
    /// 越界值被钳制。四角精确: `(0,0)=corners[0]` 色、`(1,1)=corners[2]` 色。
    ///
    /// 每个通道独立线性插值后四舍五入回 u8(中途不预乘——角颜色是
    /// 直观语义,预乘插值留给渲染侧 S4 按需选择)。
    pub fn eval(&self, u: f64, v: f64) -> Rgba8 {
        let u = u.clamp(0.0, 1.0);
        let v = v.clamp(0.0, 1.0);
        let c = &self.corner_colors;
        let (w00, w10, w11, w01) = ((1.0 - u) * (1.0 - v), u * (1.0 - v), u * v, (1.0 - u) * v);
        let mut out = [0u8; 4];
        for ch in 0..4 {
            // 系数和恒为 1,插值结果必落在 [0, 255],round 后 cast 无损语义
            let value = f64::from(c[0][ch]) * w00
                + f64::from(c[1][ch]) * w10
                + f64::from(c[2][ch]) * w11
                + f64::from(c[3][ch]) * w01;
            out[ch] = value.round().clamp(0.0, 255.0) as u8;
        }
        out
    }
}

/// 描边样式(颜色 + 线宽;线型/端帽 v0.1 不做)。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StrokeStyle {
    pub paint: Paint,
    pub width: f64,
}

/// 矢量路径节点数据。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathNode {
    pub path: BezPath,
    pub fill: Option<Paint>,
    pub stroke: Option<StrokeStyle>,
}

/// 文本节点数据(布局/塑形缓存归 canvas/text 管线,这里只存数据)。
///
/// 字号纪律(V4.0 T6.3):纯数据层不做拒收(无合适错误变体且 error 枚举
/// 归属 error.rs);非有限/≤0 的 `font_size` 由 canvas 布局入口钳制到合法值
/// (`sable_canvas::text::normalize_font_size`)并记计数,NaN 绝不进入缓存键
/// 与字形轮廓。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextNode {
    pub text: String,
    pub font_size: f64,
    pub color: Rgba8,
}

/// 位图节点数据(v0.1 占位:rect + 资源名,像素数据走资产管线)。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageNode {
    pub rect: Rect,
    pub name: String,
}

/// 节点内容:设计软件里"一个对象"的种类。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NodeContent {
    /// 图层组(纯容器)
    Group,
    Path(PathNode),
    Text(TextNode),
    Image(ImageNode),
}

/// 场景节点。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    /// 父节点;`None` = 顶层(挂在 [`Scene::roots`])。
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    /// 相对父节点的局部变换
    pub transform: Affine,
    pub visible: bool,
    pub locked: bool,
    /// 不透明度 0.0~1.0(渲染时与父链累乘)
    pub opacity: f64,
    /// 混合模式(迭代计划 08 E5):`Normal` = 普通合成;其余模式在渲染时
    /// 把本节点内容作为一个混合层与其下背景混合。serde default 兼容旧工程。
    #[serde(default)]
    pub blend_mode: BlendMode,
    /// 节点效果栈(迭代计划 08 S4 #4.1,分册七):按序应用到"本节点单独
    /// 渲染的结果"上再合成回画布(Illustrator 外观面板语义)。空 vec =
    /// 无效果(渲染零开销路径)。serde default 兼容旧工程(与 blend_mode
    /// 同款)。
    #[serde(default)]
    pub effects: Vec<EffectEntry>,
    pub content: NodeContent,
}

impl Node {
    /// 用库约定默认值构造节点(可见、不锁定、opacity 1.0、单位变换、
    /// Normal 混合、无效果)。
    pub fn new(name: impl Into<String>, content: NodeContent) -> Self {
        Node {
            name: name.into(),
            parent: None,
            children: Vec::new(),
            transform: Affine::IDENTITY,
            visible: true,
            locked: false,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            effects: Vec::new(),
            content,
        }
    }
}

/// 场景:slotmap 句柄表 + 根节点渲染序(渲染顺序 = 数组顺序,底→顶)。
///
/// `PartialEq` 是**结构化相等**:从 roots 出发递归比较节点数据,不比较
/// slotmap 的 key/版本号(remove→undo_remove 循环会换发 key,数据不变)。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Scene {
    pub nodes: SlotMap<NodeId, Node>,
    /// 顶层图层(渲染顺序 = 数组顺序)
    pub roots: Vec<NodeId>,
}

impl PartialEq for Scene {
    fn eq(&self, other: &Self) -> bool {
        self.roots.len() == other.roots.len()
            && self
                .roots
                .iter()
                .zip(other.roots.iter())
                .all(|(&a, &b)| self.subtree_eq(a, other, b))
    }
}

impl Eq for Scene {}

/// 被摘除的子树:结构 + 数据全保存,供撤销恢复。
///
/// `nodes` 里各节点的 `parent`/`children` 字段引用**摘除时的原始 id**;
/// [`Scene::undo_remove`] 会把它们重映射到恢复时的新 id。
#[derive(Clone, Debug)]
pub struct RemovedSubtree {
    /// 摘除时子树根的原始 id(恢复后失效,以 undo_remove 返回值为准)。
    pub root: NodeId,
    /// 摘除点:父节点(`None` = 挂在 roots)。
    pub parent: Option<NodeId>,
    /// 摘除点:在父 children / roots 中的下标。
    pub index: usize,
    /// 子树全部节点(root 在首位,父先于子)。
    pub nodes: Vec<(NodeId, Node)>,
}

impl Scene {
    /// 空场景。
    pub fn new() -> Self {
        Scene {
            nodes: SlotMap::with_key(),
            roots: Vec::new(),
        }
    }

    /// 追加一个节点:`parent = None` 挂 roots 尾;否则挂到父节点 children 尾。
    ///
    /// 新节点没有 children,**不可能成环**,只校验父节点存在性
    /// ([`Scene::reparent`] 才需要成环检测)。
    pub fn add_node(
        &mut self,
        parent: Option<NodeId>,
        name: impl Into<String>,
        content: NodeContent,
    ) -> CoreResult<NodeId> {
        if let Some(p) = parent
            && !self.nodes.contains_key(p)
        {
            return Err(CoreError::ParentNotFound(p));
        }
        let mut node = Node::new(name, content);
        node.parent = parent;
        let id = self.nodes.insert(node);
        match parent {
            Some(p) => {
                // 存在性已在上文校验(slotmap 插入不会使既有 key 失效);
                // 仍按 RB-01 走结构化出口,不设"必然命中"假设。
                let parent_node = self.nodes.get_mut(p).ok_or(CoreError::ParentNotFound(p))?;
                parent_node.children.push(id);
            }
            None => self.roots.push(id),
        }
        Ok(id)
    }

    /// 定点插入(撤销恢复用):把 `node` 挂到 `parent` 的 children(或 roots)
    /// 的 `index` 处;`index = None` 追加到尾部。
    ///
    /// slotmap 在插入瞬间才发号,安全 API 无法恢复已删除的原 key
    /// (`KeyData` 字段私有、slot 版本单调递增),故不存在 `insert_with_id`;
    /// 撤销恢复走 [`Scene::undo_remove`] 的 id 重映射。
    pub fn insert_at(
        &mut self,
        parent: Option<NodeId>,
        index: Option<usize>,
        mut node: Node,
    ) -> CoreResult<NodeId> {
        let len = match parent {
            Some(p) => self
                .nodes
                .get(p)
                .ok_or(CoreError::ParentNotFound(p))?
                .children
                .len(),
            None => self.roots.len(),
        };
        let index = index.unwrap_or(len);
        if index > len {
            return Err(CoreError::IndexOutOfBounds { index, len });
        }
        node.parent = parent;
        let id = self.nodes.insert(node);
        match parent {
            Some(p) => {
                // 父节点存在性已在上文 len 计算时校验(slotmap 插入不会使
                // 既有 key 失效);按 RB-01 走结构化出口,不设"必然命中"假设。
                let parent_node = self.nodes.get_mut(p).ok_or(CoreError::ParentNotFound(p))?;
                parent_node.children.insert(index, id);
            }
            None => self.roots.insert(index, id),
        }
        Ok(id)
    }

    /// 摘除整棵子树:结构 + 数据全保存进 [`RemovedSubtree`]。
    pub fn remove_subtree(&mut self, id: NodeId) -> CoreResult<RemovedSubtree> {
        let (parent, index) = self.position(id)?;
        match parent {
            Some(p) => {
                // position() 已确认父节点存在且挂着 id;按 RB-01 走结构化出口。
                let parent_node = self.nodes.get_mut(p).ok_or(CoreError::ParentNotFound(p))?;
                parent_node.children.remove(index);
            }
            None => {
                self.roots.remove(index);
            }
        }
        let mut nodes = Vec::new();
        self.collect_subtree(id, &mut nodes);
        Ok(RemovedSubtree {
            root: id,
            parent,
            index,
            nodes,
        })
    }

    /// 恢复 [`Scene::remove_subtree`] 摘除的子树,挂回原摘除点
    /// (parent/index),返回 **old id → 新 id 的重映射表**(含子树根)。
    ///
    /// 子树内部链接按 old→new 映射重写;根的 parent 指向子树外,原样保留。
    /// 调用方(History)必须把映射表广播给历史栈中其他命令,修正它们缓存的 id。
    pub fn undo_remove(&mut self, subtree: &RemovedSubtree) -> CoreResult<IdRemap> {
        // 先校验再写入,失败不留孤儿节点
        if let Some(p) = subtree.parent
            && !self.nodes.contains_key(p)
        {
            return Err(CoreError::ParentNotFound(p));
        }
        let attach_len = match subtree.parent {
            Some(p) => self
                .nodes
                .get(p)
                .ok_or(CoreError::ParentNotFound(p))?
                .children
                .len(),
            None => self.roots.len(),
        };
        if subtree.index > attach_len {
            return Err(CoreError::IndexOutOfBounds {
                index: subtree.index,
                len: attach_len,
            });
        }

        // 重新插入全部节点(slotmap 发新号),建立 old→new 映射
        let mut remap: HashMap<NodeId, NodeId> = HashMap::with_capacity(subtree.nodes.len());
        for (old_id, node) in &subtree.nodes {
            let new_id = self.nodes.insert(node.clone());
            remap.insert(*old_id, new_id);
        }
        // 重写链接:子树内部的 parent/children 换新号;指向子树外的 id 原样保留。
        // 根的 parent 以锚点字段 subtree.parent 为准:摘除点可能已被外部重映射
        // (History 治愈的是本结构的 anchor 字段,治不到 nodes 数据里的旧 id),
        // 两者在摘除时刻本就相等,锚点是更新的一方。
        for (old_id, node) in &subtree.nodes {
            let Some(&new_id) = remap.get(old_id) else {
                continue;
            };
            let Some(new_node) = self.nodes.get_mut(new_id) else {
                continue;
            };
            new_node.parent = if *old_id == subtree.root {
                subtree.parent
            } else {
                node.parent.map(|p| remap.get(&p).copied().unwrap_or(p))
            };
            new_node.children = node
                .children
                .iter()
                .map(|c| remap.get(c).copied().unwrap_or(*c))
                .collect();
        }
        let new_root = remap
            .get(&subtree.root)
            .copied()
            .ok_or(CoreError::OrphanNode(subtree.root))?;
        match subtree.parent {
            Some(p) => {
                // 父节点存在性已在本函数开头校验(slotmap 插入不会使既有
                // key 失效);按 RB-01 走结构化出口,不设"必然命中"假设。
                let parent_node = self.nodes.get_mut(p).ok_or(CoreError::ParentNotFound(p))?;
                parent_node.children.insert(subtree.index, new_root);
            }
            None => self.roots.insert(subtree.index, new_root),
        }
        Ok(remap)
    }

    /// 移动节点到新父节点(成环检测:`new_parent` 不能是 `id` 自己或其后代)。
    ///
    /// `index` 指移动后的位置(同父移动时先摘再插,下标按摘除后的列表解释);
    /// `None` = 追加到尾部。
    pub fn reparent(
        &mut self,
        id: NodeId,
        new_parent: Option<NodeId>,
        index: Option<usize>,
    ) -> CoreResult<()> {
        if !self.nodes.contains_key(id) {
            return Err(CoreError::NodeNotFound(id));
        }
        if let Some(p) = new_parent {
            if !self.nodes.contains_key(p) {
                return Err(CoreError::ParentNotFound(p));
            }
            // 成环检测:p 不能是 id 自己,也不能是 id 的后代
            if p == id || self.is_descendant(p, id) {
                return Err(CoreError::CycleDetected(id));
            }
        }
        // 计算目标列表在摘除 id 之后的长度,先做越界校验
        let target_len = match new_parent {
            Some(p) => self
                .nodes
                .get(p)
                .ok_or(CoreError::ParentNotFound(p))?
                .children
                .len(),
            None => self.roots.len(),
        };
        let same_parent = self.nodes.get(id).map(|n| n.parent) == Some(new_parent);
        let len_after = target_len - if same_parent { 1 } else { 0 };
        if let Some(i) = index {
            if i > len_after {
                return Err(CoreError::IndexOutOfBounds {
                    index: i,
                    len: len_after,
                });
            }
        }
        // 摘除当前链接
        let (old_parent, _) = self.position(id)?;
        match old_parent {
            Some(p) => {
                if let Some(pn) = self.nodes.get_mut(p) {
                    pn.children.retain(|&c| c != id);
                }
            }
            None => self.roots.retain(|&r| r != id),
        }
        // 挂到新位置
        let attach = index.unwrap_or(len_after);
        match new_parent {
            Some(p) => {
                // new_parent 存在性已在上文校验(摘除用的是 old_parent,
                // 不影响 new_parent 的 key);按 RB-01 走结构化出口。
                let pn = self.nodes.get_mut(p).ok_or(CoreError::ParentNotFound(p))?;
                pn.children.insert(attach, id);
            }
            None => self.roots.insert(attach, id),
        }
        if let Some(node) = self.nodes.get_mut(id) {
            node.parent = new_parent;
        }
        Ok(())
    }

    /// 节点的世界变换:沿 parent 链从根到本节点累乘。
    pub fn world_transform(&self, id: NodeId) -> Option<Affine> {
        let mut chain = Vec::new();
        let mut cur = Some(id);
        // 步数上界 = 节点总数,防御脏数据导致的 parent 链成环
        let mut steps = 0;
        while let Some(c) = cur {
            let node = self.nodes.get(c)?;
            chain.push(node.transform);
            cur = node.parent;
            steps += 1;
            if steps > self.nodes.len() {
                return None;
            }
        }
        Some(chain.iter().rev().fold(Affine::IDENTITY, |acc, &t| acc * t))
    }

    /// 深度优先遍历,产出"世界变换已应用"的渲染列表(底→顶序)。
    /// 不可见节点整棵剪枝(源自 docs/02 §3 的 walk)。
    pub fn render_list(&self) -> Vec<(NodeId, Affine)> {
        let mut out = Vec::with_capacity(self.nodes.len());
        for &root in &self.roots {
            self.walk(root, Affine::IDENTITY, &mut out);
        }
        out
    }

    /// 节点内容包围盒 × 世界变换;Group = 各子节点世界包围盒的并集
    /// (空组返回 None)。Text 用 0.6em 字宽 × 1.2em 行高粗估,不引 parley。
    ///
    /// V4.0 T6.1(canvas 层实测化):本粗估只是纯数据层的零依赖回退口径——
    /// CJK 实际 ≈1.0em/字,粗估系统性偏窄 ~40%。依赖方向不允许 parley 下沉
    /// foundation,故 canvas 的渲染/命中/选中/脏矩形一律改用
    /// `sable_canvas::text::node_world_bbox_measured`(实测布局尺寸覆盖
    /// Text 分支,其余口径与本函数一致)。
    pub fn node_world_bbox(&self, id: NodeId) -> Option<Rect> {
        let node = self.nodes.get(id)?;
        let world = self.world_transform(id)?;
        // kurbo 0.13 无 Mul<Rect> for Affine,用 transform_rect_bbox(变换四角取包围盒)
        let own = self
            .content_local_bbox(node)
            .map(|local| world.transform_rect_bbox(local));
        match &node.content {
            NodeContent::Group => {
                let mut acc = own; // Group 自身无内容包围盒
                for &child in &node.children {
                    let cb = self.node_world_bbox(child)?;
                    acc = Some(match acc {
                        Some(a) => a.union(cb),
                        None => cb,
                    });
                }
                acc
            }
            _ => own,
        }
    }

    /// 只读访问节点。
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// 可变访问节点。
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id)
    }

    /// 只读访问路径数据。
    pub fn path(&self, id: NodeId) -> Option<&PathNode> {
        match &self.nodes.get(id)?.content {
            NodeContent::Path(p) => Some(p),
            _ => None,
        }
    }

    /// 可变访问路径数据(命令系统修改 fill/stroke 的入口,docs/03 §2)。
    pub fn path_mut(&mut self, id: NodeId) -> Option<&mut PathNode> {
        match &mut self.nodes.get_mut(id)?.content {
            NodeContent::Path(p) => Some(p),
            _ => None,
        }
    }

    /// 存活节点总数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 场景是否为空。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 按渲染序(底→顶)迭代根节点。
    pub fn iter_roots(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.roots.iter().copied()
    }

    /// 节点当前位置:(父节点, 在父 children/roots 中的下标)。
    pub fn position(&self, id: NodeId) -> CoreResult<(Option<NodeId>, usize)> {
        let node = self.nodes.get(id).ok_or(CoreError::NodeNotFound(id))?;
        match node.parent {
            Some(p) => {
                let pn = self.nodes.get(p).ok_or(CoreError::ParentNotFound(p))?;
                let idx = pn
                    .children
                    .iter()
                    .position(|&c| c == id)
                    .ok_or(CoreError::OrphanNode(id))?;
                Ok((Some(p), idx))
            }
            None => {
                let idx = self
                    .roots
                    .iter()
                    .position(|&r| r == id)
                    .ok_or(CoreError::OrphanNode(id))?;
                Ok((None, idx))
            }
        }
    }

    // —— 内部实现 ——

    fn walk(&self, id: NodeId, parent_xform: Affine, out: &mut Vec<(NodeId, Affine)>) {
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        if !node.visible {
            return;
        }
        let world = parent_xform * node.transform;
        out.push((id, world));
        // RBT-04:显式栈迭代(树经校验 API 构建无环,深度可达十万级;
        // 递归会栈溢出,显式栈永不)。
        let mut stack: Vec<(NodeId, Affine)> =
            node.children.iter().rev().map(|&c| (c, world)).collect();
        while let Some((cid, cworld)) = stack.pop() {
            if let Some(cn) = self.nodes.get(cid) {
                if !cn.visible {
                    continue;
                }
                let cw = cworld * cn.transform;
                out.push((cid, cw));
                // 逆序入栈 → LIFO 弹出即原 children 顺序(底→顶)
                for &cc in cn.children.iter().rev() {
                    stack.push((cc, cw));
                }
            }
        }
    }

    /// 递归摘除:先子后父 push(无妨,恢复时按映射重链)。
    fn collect_subtree(&mut self, id: NodeId, out: &mut Vec<(NodeId, Node)>) {
        // RBT-04:显式栈前序摘除(深度十万级不栈溢出)。恢复按 id 重链,
        // 顺序无关——见方法上注"先子后父 push(无妨)"。
        let mut stack: Vec<NodeId> = vec![id];
        while let Some(cur) = stack.pop() {
            if let Some(node) = self.nodes.remove(cur) {
                for &child in &node.children {
                    stack.push(child);
                }
                out.push((cur, node));
            }
        }
    }

    /// `maybe_desc` 是否为 `ancestor` 的后代(沿 parent 链上溯,步数有界)。
    fn is_descendant(&self, maybe_desc: NodeId, ancestor: NodeId) -> bool {
        let mut cur = self.nodes.get(maybe_desc).and_then(|n| n.parent);
        let mut steps = 0;
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            cur = self.nodes.get(c).and_then(|n| n.parent);
            steps += 1;
            if steps > self.nodes.len() {
                return false;
            }
        }
        false
    }

    fn content_local_bbox(&self, node: &Node) -> Option<Rect> {
        match &node.content {
            NodeContent::Path(p) => Some(p.path.bounding_box()),
            NodeContent::Text(t) => {
                // 粗估:0.6em 字宽 × 1.2em 行高(V4.0 T6.1:canvas 侧以实测
                // 布局尺寸替代,这里保持零依赖回退;字号病态时本口径不拒收,
                // 行为层钳制见 canvas/text 的 normalize_font_size)
                let chars = t.text.chars().count();
                let width = 0.6 * t.font_size * chars as f64;
                let height = 1.2 * t.font_size;
                Some(Rect::new(0.0, 0.0, width, height))
            }
            NodeContent::Image(img) => Some(img.rect),
            NodeContent::Group => None,
        }
    }

    /// 结构化比较:同位置子树逐字段比较(children 按下标配对递归;
    /// 不比较 parent/children 里的具体 id——remove/undo 循环会重映射)。
    fn subtree_eq(&self, a: NodeId, other: &Scene, b: NodeId) -> bool {
        // RBT-04:显式栈成对比较(同位子树逐字段;深树不递归)。
        let mut stack: Vec<(NodeId, NodeId)> = vec![(a, b)];
        while let Some((ca, cb)) = stack.pop() {
            let (Some(na), Some(nb)) = (self.nodes.get(ca), other.nodes.get(cb)) else {
                return false;
            };
            if !(na.name == nb.name
                && na.transform == nb.transform
                && na.visible == nb.visible
                && na.locked == nb.locked
                && na.opacity == nb.opacity
                && na.blend_mode == nb.blend_mode
                && na.effects == nb.effects
                && na.content == nb.content
                && na.children.len() == nb.children.len())
            {
                return false;
            }
            for (&x, &y) in na.children.iter().zip(nb.children.iter()) {
                stack.push((x, y));
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;

    fn rect_content(x0: f64, y0: f64, x1: f64, y1: f64) -> NodeContent {
        let mut path = BezPath::new();
        path.move_to((x0, y0));
        path.line_to((x1, y0));
        path.line_to((x1, y1));
        path.line_to((x0, y1));
        path.close_path();
        NodeContent::Path(PathNode {
            path,
            fill: Some(Paint::Solid([200, 40, 40, 255])),
            stroke: None,
        })
    }

    fn find_by_name(scene: &Scene, name: &str) -> NodeId {
        scene
            .nodes
            .iter()
            .find(|(_, n)| n.name == name)
            .map(|(id, _)| id)
            .expect("按名字找回节点")
    }

    /// 根组 → (矩形A, 子组 → 矩形B),外加根级矩形C(共 5 节点)
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
    fn add_node_validates_parent() {
        let mut scene = Scene::new();
        let root = scene
            .add_node(None, "x", NodeContent::Group)
            .expect("根级添加应成功");
        scene
            .add_node(Some(root), "子", NodeContent::Group)
            .expect("父存在,应成功");
        scene.remove_subtree(root).expect("摘除");
        // 父节点被摘除后再挂 → ParentNotFound(add_node 不可能成环:新节点无 children)
        match scene.add_node(Some(root), "子", NodeContent::Group) {
            Err(CoreError::ParentNotFound(p)) => assert_eq!(p, root),
            other => panic!("应返回 ParentNotFound,实际 {other:?}"),
        }
    }

    #[test]
    fn reparent_rejects_cycles() {
        let (mut scene, root, a, sub, _b, _c) = demo_scene();
        // 把根组挪进自己的后代 → 拒绝
        match scene.reparent(root, Some(sub), None) {
            Err(CoreError::CycleDetected(id)) => assert_eq!(id, root),
            other => panic!("应返回 CycleDetected,实际 {other:?}"),
        }
        // 挪给自己 → 拒绝
        match scene.reparent(root, Some(root), None) {
            Err(CoreError::CycleDetected(_)) => {}
            other => panic!("应返回 CycleDetected,实际 {other:?}"),
        }
        // 合法移动:矩形A 挪到根级
        scene.reparent(a, None, None).expect("挪到根");
        assert_eq!(scene.node(a).expect("a 在").parent, None);
        assert!(!scene.node(root).expect("root 在").children.contains(&a));
        // 结构完好:全部节点仍从 roots 可达且可见
        assert_eq!(scene.render_list().len(), scene.len());
    }

    #[test]
    fn reparent_rejects_out_of_bounds_index() {
        let (mut scene, root, a, _sub, _b, _c) = demo_scene();
        match scene.reparent(a, Some(root), Some(9)) {
            Err(CoreError::IndexOutOfBounds { index, .. }) => assert_eq!(index, 9),
            other => panic!("应返回 IndexOutOfBounds,实际 {other:?}"),
        }
    }

    #[test]
    fn render_list_orders_bottom_to_top_and_prunes_invisible() {
        let (mut scene, _root, a, sub, b, c) = demo_scene();
        let ids: Vec<NodeId> = scene.render_list().into_iter().map(|(id, _)| id).collect();
        // 底→顶 = roots 顺序 DFS:根组、A、子组、B、C
        assert_eq!(ids.len(), 5);
        let pos = |x: NodeId| ids.iter().position(|&i| i == x).expect("在列表中");
        assert!(pos(a) < pos(b) && pos(b) < pos(c), "顺序应为 A < B < C");

        // 隐藏父组 → 整棵子树剪枝
        scene.node_mut(sub).expect("sub 在").visible = false;
        let ids: Vec<NodeId> = scene.render_list().into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids.len(), 3);
        assert!(!ids.contains(&sub));
        assert!(!ids.contains(&b));

        // 恢复子组,隐藏单叶 A
        scene.node_mut(sub).expect("sub 在").visible = true;
        scene.node_mut(a).expect("a 在").visible = false;
        let ids: Vec<NodeId> = scene.render_list().into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids.len(), 4);
        assert!(!ids.contains(&a));
    }

    #[test]
    fn remove_then_undo_remove_restores_subtree_exactly() {
        let (mut scene, root, _a, sub, b, _c) = demo_scene();
        let snapshot = scene.clone();
        let removed = scene.remove_subtree(sub).expect("摘除子组");
        assert_eq!(removed.parent, Some(root));
        assert_eq!(removed.root, sub);
        assert_eq!(removed.nodes.len(), 2, "子组 + 孙矩形B");
        assert_eq!(scene.len(), snapshot.len() - 2);

        // 摘除期间插入别的节点(占坑,模拟真实编辑序列)
        scene
            .add_node(None, "占位", NodeContent::Group)
            .expect("占位节点");
        assert_eq!(scene.len(), snapshot.len() - 1);

        let remap = scene.undo_remove(&removed).expect("恢复子树");
        let new_root = remap[&removed.root];
        assert_ne!(new_root, sub, "slotmap 无法复原旧 key,恢复后换发新 id");
        assert_eq!(remap.len(), 2, "子组 + 孙矩形B 都有映射");
        assert_eq!(scene.len(), snapshot.len() + 1);

        // 结构化相等:与"快照 + 占位节点"逐字段一致(数据一字不差)
        let mut expected = snapshot.clone();
        expected
            .add_node(None, "占位", NodeContent::Group)
            .expect("快照上加占位节点");
        assert_eq!(scene, expected);

        // 孙矩形B 挂回原位,世界包围盒与摘除前一致(内部链接重映射正确)
        let bbox_before = snapshot.node_world_bbox(b).expect("摘除前 bbox");
        let new_b = find_by_name(&scene, "矩形B");
        let bbox_after = scene.node_world_bbox(new_b).expect("恢复后 bbox");
        assert_eq!(bbox_before, bbox_after);
        assert_eq!(
            scene.node(new_b).expect("B 在").parent,
            Some(new_root),
            "孙节点的 parent 已重映射到新子组 id"
        );
    }

    #[test]
    fn remove_root_and_restore() {
        let (mut scene, _root, _a, _sub, _b, c) = demo_scene();
        let snapshot = scene.clone();
        let removed = scene.remove_subtree(c).expect("摘除根级节点");
        assert_eq!(removed.parent, None, "根级节点挂在 roots");
        let remap = scene.undo_remove(&removed).expect("恢复");
        let new_c = remap[&removed.root];
        assert_eq!(scene.len(), snapshot.len());
        assert_eq!(scene, snapshot);
        // 恢复回 roots 原下标(摘除时在尾部,恢复后仍在尾部)
        let roots: Vec<NodeId> = scene.iter_roots().collect();
        assert_eq!(*roots.last().expect("roots 非空"), new_c);
    }

    #[test]
    fn world_transform_accumulates_along_parent_chain() {
        let (mut scene, root, a, sub, b, c) = demo_scene();
        scene.node_mut(root).expect("root").transform = Affine::translate((100.0, 50.0));
        scene.node_mut(sub).expect("sub").transform = Affine::rotate(90f64.to_radians());
        scene.node_mut(b).expect("b").transform = Affine::scale(2.0);
        let world_b = scene.world_transform(b).expect("b 可达");
        let expected = Affine::translate((100.0, 50.0))
            * Affine::rotate(90f64.to_radians())
            * Affine::scale(2.0);
        assert_eq!(world_b, expected);
        // 嵌套节点 = 父链累乘:矩形A 挂在根组下,世界变换 = 根变换 × 自身变换
        // (此前的断言误把 a 当顶层节点、漏乘根变换,与实现无关)
        let root_t = scene.node(root).expect("root").transform;
        let a_t = scene.node(a).expect("a").transform;
        assert_eq!(scene.world_transform(a), Some(root_t * a_t));
        // 真正的顶层节点(矩形C 挂在 roots)= 自身变换
        assert_eq!(
            scene.world_transform(c),
            Some(scene.node(c).expect("c").transform)
        );
    }

    #[test]
    fn node_world_bbox_content_times_world_transform() {
        let (mut scene, root, _a, _sub, b, _c) = demo_scene();
        scene.node_mut(root).expect("root").transform = Affine::translate((10.0, 20.0));
        scene.node_mut(b).expect("b").transform = Affine::translate((1.0, 2.0));
        // 矩形B 本地 (20,20)-(30,30) → ×B 变换 → ×根变换 → 世界 (31,42)-(41,52)
        let bbox = scene.node_world_bbox(b).expect("bbox");
        assert!((bbox.x0 - 31.0).abs() < 1e-9 && (bbox.y0 - 42.0).abs() < 1e-9);
        assert!((bbox.x1 - 41.0).abs() < 1e-9 && (bbox.y1 - 52.0).abs() < 1e-9);

        // 组 = 各子节点世界包围盒的并集:A(10,20)-(20,30) ∪ B(31,42)-(41,52)
        let root_bbox = scene.node_world_bbox(root).expect("root bbox");
        assert!((root_bbox.x0 - 10.0).abs() < 1e-9);
        assert!((root_bbox.y0 - 20.0).abs() < 1e-9);
        assert!((root_bbox.x1 - 41.0).abs() < 1e-9);
        assert!((root_bbox.y1 - 52.0).abs() < 1e-9);

        // 文本粗估:0.6em 字宽 × 1.2em 行高
        let text = "北京 2026".to_string();
        let t = scene
            .add_node(
                None,
                "文本",
                NodeContent::Text(TextNode {
                    text: text.clone(),
                    font_size: 10.0,
                    color: [0, 0, 0, 255],
                }),
            )
            .expect("文本节点");
        let tb = scene.node_world_bbox(t).expect("text bbox");
        let chars = text.chars().count() as f64;
        assert!((tb.width() - 0.6 * 10.0 * chars).abs() < 1e-9);
        assert!((tb.height() - 12.0).abs() < 1e-9);

        // 空组没有内容包围盒
        let empty = scene
            .add_node(None, "空组", NodeContent::Group)
            .expect("空组");
        assert_eq!(scene.node_world_bbox(empty), None);
    }

    #[test]
    fn position_reports_parent_and_index() {
        let (mut scene, root, a, _sub, _b, c) = demo_scene();
        assert_eq!(scene.position(root).expect("pos root"), (None, 0));
        assert_eq!(scene.position(c).expect("pos c"), (None, 1));
        assert_eq!(scene.position(a).expect("pos a"), (Some(root), 0));
        scene.remove_subtree(a).expect("摘除 a");
        match scene.position(a) {
            Err(CoreError::NodeNotFound(id)) => assert_eq!(id, a),
            other => panic!("应 NodeNotFound,实际 {other:?}"),
        }
    }

    #[test]
    fn insert_at_validates_parent_and_bounds() {
        let (mut scene, root, _a, _sub, _b, _c) = demo_scene();
        // 根组当前 children = [矩形A, 子组],len = 2;99 越界报当前长度
        match scene.insert_at(Some(root), Some(99), Node::new("x", NodeContent::Group)) {
            Err(CoreError::IndexOutOfBounds { index, len }) => {
                assert_eq!((index, len), (99, 2));
            }
            other => panic!("应 IndexOutOfBounds,实际 {other:?}"),
        }
        let ghost = scene
            .add_node(None, "ghost", NodeContent::Group)
            .expect("ok");
        scene.remove_subtree(ghost).expect("摘除");
        match scene.insert_at(Some(ghost), None, Node::new("y", NodeContent::Group)) {
            Err(CoreError::ParentNotFound(_)) => {}
            other => panic!("应 ParentNotFound,实际 {other:?}"),
        }
    }

    #[test]
    fn scene_serde_roundtrip_preserves_data_and_keys() {
        let (scene, _root, _a, _sub, _b, _c) = demo_scene();
        let mut scene = scene;
        // 补齐全部 Paint 变体与字段类型,一次覆盖所有数据的序列化
        let path_id = find_by_name(&scene, "矩形A");
        if let Some(p) = scene.path_mut(path_id) {
            p.stroke = Some(StrokeStyle {
                paint: Paint::LinearGradient {
                    start: [0.0, 0.0],
                    end: [10.0, 0.0],
                    stops: vec![
                        GradientStop {
                            offset: 0.0,
                            color: [255, 0, 0, 255],
                        },
                        GradientStop {
                            offset: 1.0,
                            color: [0, 0, 255, 128],
                        },
                    ],
                },
                width: 1.5,
            });
            p.fill = Some(Paint::RadialGradient {
                center: [5.0, 5.0],
                radius: 7.0,
                stops: vec![GradientStop {
                    offset: 0.5,
                    color: [10, 20, 30, 40],
                }],
            });
        }
        let json = serde_json::to_string(&scene).expect("序列化");
        let back: Scene = serde_json::from_str(&json).expect("反序列化");
        assert_eq!(back, scene);
        // slotmap serde 重建同一套 key:既有句柄反序列化后继续有效
        assert_eq!(back.node(path_id), scene.node(path_id));
        assert_eq!(back.roots, scene.roots);
    }

    #[test]
    fn node_blend_mode_serializes_and_defaults_to_normal() {
        let (mut scene, _root, a, _sub, _b, _c) = demo_scene();
        scene.node_mut(a).expect("a 在").blend_mode = BlendMode::Multiply;

        let json = serde_json::to_string(&scene).expect("序列化");
        assert!(
            json.contains("Multiply"),
            "非默认混合模式必须出现在序列化产物里"
        );
        let back: Scene = serde_json::from_str(&json).expect("反序列化");
        assert_eq!(back.node(a).expect("a 在").blend_mode, BlendMode::Multiply);

        // 旧工程文件(无 blend_mode 字段)→ serde(default) 回落 Normal,不报错
        let legacy = serde_json::to_string(&scene).expect("序列化带字段场景");
        let stripped = strip_json_field(&legacy, "blend_mode");
        let old: Scene = serde_json::from_str(&stripped).expect("旧版文件应可打开");
        assert_eq!(
            old.node(a).expect("a 在").blend_mode,
            BlendMode::Normal,
            "缺字段 → serde(default) 回落 Normal"
        );
    }

    /// 效果栈(迭代计划 08 S4 #4.1)序列化 + 旧工程兼容:带效果的节点
    /// 无损往返;旧文件(无 effects 字段)→ serde(default) 回落空栈。
    #[test]
    fn node_effects_serialize_and_default_to_empty() {
        use crate::effects::{EffectEntry, EffectSpec};
        let (mut scene, _root, a, _sub, _b, _c) = demo_scene();
        scene.node_mut(a).expect("a 在").effects = vec![EffectEntry {
            spec: EffectSpec::DropShadow {
                blur: 4.0,
                offset: [0.0, 2.0],
                color: [0, 0, 0, 128],
            },
            enabled: true,
        }];

        let json = serde_json::to_string(&scene).expect("序列化");
        assert!(json.contains("DropShadow"), "效果必须出现在序列化产物里");
        let back: Scene = serde_json::from_str(&json).expect("反序列化");
        assert_eq!(back.node(a).expect("a 在").effects.len(), 1);
        assert_eq!(back, scene, "效果栈参与结构化相等");

        // 旧工程文件(无 effects 字段)→ serde(default) 回落空栈,不报错
        let stripped = strip_json_array_field(&json, "effects");
        let old: Scene = serde_json::from_str(&stripped).expect("旧版文件应可打开");
        assert!(
            old.node(a).expect("a 在").effects.is_empty(),
            "缺字段 → serde(default) 回落空效果栈"
        );
    }

    /// 从 JSON 文本里剥掉 `"字段名":[...],`(仅测试用;effects 的序列化
    /// 形态是数组)。按括号配平找数组结尾,悬挂逗号一并移除。
    fn strip_json_array_field(json: &str, field: &str) -> String {
        let needle = format!("\"{field}\":");
        let mut out = String::new();
        let mut rest = json;
        // 循环剥掉**每一处**出现:每个节点都有自己的该字段,只剥首个达不到目的
        while let Some(start) = rest.find(&needle) {
            out.push_str(&rest[..start]);
            let arr_start = start + needle.len();
            debug_assert!(rest[arr_start..].starts_with('['), "字段应为数组形态");
            let mut depth = 0usize;
            let mut end = arr_start;
            for (i, ch) in rest[arr_start..].char_indices() {
                match ch {
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            end = arr_start + i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let after = &rest[end..];
            if let Some(stripped) = after.strip_prefix(',') {
                rest = stripped;
            } else if out.ends_with(',') {
                out.pop();
                rest = after;
            } else {
                rest = after;
            }
        }
        out.push_str(rest);
        out
    }

    /// 从 JSON 文本里剥掉 `"字段名":<字符串值>,`(仅测试用;BlendMode 的
    /// 序列化形态是字符串)。剥掉后若该字段是对象的最后一项,遗留的悬挂
    /// 逗号一并移除。
    fn strip_json_field(json: &str, field: &str) -> String {
        let needle = format!("\"{field}\":");
        let mut out = String::with_capacity(json.len());
        let mut rest = json;
        while let Some(pos) = rest.find(&needle) {
            out.push_str(&rest[..pos]);
            let after = &rest[pos + needle.len()..];
            rest = &after[skip_json_value(after)..];
            // 剥掉本字段后,前后必剩一个多余逗号:rest 以 ',' 开头就吃 rest 侧;
            // 否则本字段是末项,吃 out 侧悬挂逗号
            if rest.starts_with(',') {
                rest = &rest[1..];
            } else if out.ends_with(',') {
                out.pop();
            }
        }
        out.push_str(rest);
        out
    }

    /// 值的字节长度(测试辅助):字符串越过收引号(转义不处理,serde 输出
    /// 无需转义的测试值即可);数组/对象走括号配对;字面量读到逗号/括号/尾。
    fn skip_json_value(v: &str) -> usize {
        match v.as_bytes().first() {
            Some(b'"') => match v[1..].find('"') {
                Some(end) => end + 2,
                None => v.len(),
            },
            Some(b'[') | Some(b'{') => {
                let (open, close) = if v.starts_with('[') {
                    (b'[', b']')
                } else {
                    (b'{', b'}')
                };
                let mut depth = 0usize;
                for (i, b) in v.bytes().enumerate() {
                    if b == open {
                        depth += 1;
                    } else if b == close {
                        depth -= 1;
                        if depth == 0 {
                            return i + 1;
                        }
                    }
                }
                v.len()
            }
            _ => v.find([',', ']', '}']).unwrap_or(v.len()),
        }
    }

    #[test]
    fn conic_gradient_roundtrips_and_preserves_geometry() {
        let (mut scene, _root, a, _sub, _b, _c) = demo_scene();
        let paint = Paint::ConicGradient {
            center: [32.0, 32.0],
            start_angle: 0.0,
            end_angle: std::f64::consts::TAU,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: [255, 0, 0, 255],
                },
                GradientStop {
                    offset: 1.0,
                    color: [0, 0, 255, 255],
                },
            ],
        };
        scene.path_mut(a).expect("路径在").fill = Some(paint.clone());
        let json = serde_json::to_string(&scene).expect("序列化");
        let back: Scene = serde_json::from_str(&json).expect("反序列化");
        assert_eq!(
            back.path(a).expect("路径在").fill,
            Some(paint),
            "ConicGradient 必须无损往返"
        );
    }

    #[test]
    fn mesh_gradient_eval_is_exact_at_corners_and_symmetric_at_center() {
        let mesh = MeshGradient {
            corners: [[0.0, 0.0]; 4],
            // 颜色按 u↔v 对调对称(c1==c3):中心平均与对调对称性断言才成立
            corner_colors: [
                [0, 0, 0, 255],
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [255, 0, 0, 255],
            ],
            subdivisions: 4,
        };
        // 角纯色边界精确
        assert_eq!(mesh.eval(0.0, 0.0), mesh.corner_colors[0]);
        assert_eq!(mesh.eval(1.0, 0.0), mesh.corner_colors[1]);
        assert_eq!(mesh.eval(1.0, 1.0), mesh.corner_colors[2]);
        assert_eq!(mesh.eval(0.0, 1.0), mesh.corner_colors[3]);
        // 中心 = 四角等权平均(双线性在 (0.5,0.5) 权重各 1/4);
        // eval 的取整语义是 round(见其 doc),期望值同样 round 保持一致
        let mut expected = [0u8; 4];
        for ch in 0..4 {
            let sum: u32 = mesh.corner_colors.iter().map(|c| u32::from(c[ch])).sum();
            expected[ch] = (sum as f64 / 4.0).round() as u8;
        }
        assert_eq!(mesh.eval(0.5, 0.5), expected, "中心应为四角平均色");
        // 对称性:u 与 v 对调保持一致(四个角在参数域对称布置)
        assert_eq!(mesh.eval(0.25, 0.75), mesh.eval(0.75, 0.25));
        // 越界钳制:(-1,2) 钳到 (0,1) → 左上 c3;(2,-1) 钳到 (1,0) → 右下 c1
        assert_eq!(mesh.eval(-1.0, 2.0), mesh.corner_colors[3]);
        assert_eq!(mesh.eval(2.0, -1.0), mesh.corner_colors[1]);
    }
}
