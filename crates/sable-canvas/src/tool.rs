//! 工具状态机(源自 docs/03 §4.2 工具模型 + docs/04 §3 钢笔工具)。
//!
//! **framework-free**:本模块不引 gpui。上层(gpui_element / 示例)负责
//! 把框架事件换算成"世界坐标点 + 修饰键"后分发到 [`ToolBehavior`],工具
//! 内部对文档的一切修改都经 [`ToolCtx`] 里的 [`History`] 走 Command
//! (AGENTS.md 铁律:一切文档修改必须走 Command)。
//!
//! # 拖动 = 一步撤销(docs/03 §2 事务)
//!
//! SelectTool 拖动移动/缩放、AnchorEditTool 拖锚点/手柄都在 `mouse_down` 时
//! `begin_transaction`,拖动过程每次 exec 命令,`mouse_up` 时 `end_transaction`
//! → 整次拖动撤销为一步。
//!
//! # 钢笔状态机(docs/04 §3 全集)
//!
//! ```text
//! Idle ──点击──▶ 锚点(直角);持续拖动 ──▶ 拖出双向手柄(平滑点)
//!   Alt拖手柄 ──▶ 单边手柄(尖点)
//!   点击起点 ──▶ 闭合路径 → History::exec(AddNode)
//!   点击已有路径首/尾锚点 ──▶ 从该端续接 → commit 时 exec(AppendToPath)
//!   hover 端点/起点 ──▶ preview 高亮圈(续接提示/闭合提示)
//!   Esc/右键 ──▶ 取消(上层调用 PenTool::cancel_draft)
//!   Enter ──▶ 开放提交(上层调用 PenTool::commit_open)
//! ```
//!
//! # 本地命令例外(序列化缺口,待 foundation 镜像)
//!
//! [`AppendToPath`] / [`SetAnchorPoints`] 是本模块私有的 Command 实现(apply/
//! revert 语义与 foundation 内置命令完全一致),因为 foundation 本迭代由并行
//! 波排他修改、不能在此追加。**[`History::to_serialized`] 会跳过它们(撤销
//! 栈保存时这两步不持久化,不崩)**;foundation 补上序列化镜像后,应把这两个
//! 结构迁入 `sable_foundation::command` 并加进 `SerializedCommand` 枚举。
//!
//! # 锚点编辑(docs/04 §3 SubPathEdit 模式)
//!
//! SelectTool 双击路径节点产生子编辑请求([`SelectTool::take_sub_edit_request`]),
//! 上层据此切换 [`AnchorEditTool`]。编辑全部锚点/手柄操作统一走
//! [`SetAnchorPoints`](整条 path 换新,天然可撤销;拖动用事务合并为一步)。
//! Esc/点击空白置位退出请求([`AnchorEditTool::exit_requested`]),由上层切回
//! Select——本模块不持有工具间的切换权(framework-free 组合原则)。

use std::any::Any;
use std::borrow::Cow;

use kurbo::{Affine, BezPath, Circle, CubicBez, ParamCurve, Point, Rect, Shape, Vec2};
use sable_foundation::command::{AddNode, Command, History, SetTransform};
use sable_foundation::scene::IdRemap;
use sable_foundation::scene::{NodeContent, NodeId, Paint, PathNode, Rgba8, Scene, StrokeStyle};
use sable_foundation::viewport::Viewport;
use sable_paint::sink::PaintSink;

use crate::damage::DamageTracker;
use crate::hit_test::{
    AnchorPoint, SceneHitTest, SubPath, first_subpath, hit_anchor, nearest_on_subpath,
};
use crate::input::screen_tolerance;

/// 工具的一次事件所能触达的全部可变状态(framework-free 的接缝)。
pub struct ToolCtx<'a> {
    pub scene: &'a mut Scene,
    pub history: &'a mut History,
    pub viewport: &'a mut Viewport,
    pub damage: &'a mut DamageTracker,
}

/// 修饰键快照(framework-free;gpui 侧从 `Modifiers` 摘出三个 bool)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// 工具光标(framework-free 自有枚举;gpui 侧映射到 `gpui::CursorStyle`:
/// Default→Arrow,Hand→PointingHand,Crosshair→Crosshair,Text→IBeam,
/// Move→ClosedHand)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorStyle {
    #[default]
    Default,
    Hand,
    Crosshair,
    Text,
    Move,
}

/// 工具行为(docs/03 §4.2 `ToolBehavior` 的 framework-free 版;
/// 输入点一律**世界坐标**,工具参考 Graphite 的 Tool trait 经手册转述)。
pub trait ToolBehavior {
    /// 按下(世界坐标)。
    fn mouse_down(&mut self, pt: Point, mods: Mods, ctx: &mut ToolCtx);
    /// 拖动(按住移动)。
    fn mouse_drag(&mut self, pt: Point, mods: Mods, ctx: &mut ToolCtx);
    /// 抬起。
    fn mouse_up(&mut self, pt: Point, mods: Mods, ctx: &mut ToolCtx);
    /// 双击(默认无操作;框架侧把系统双击事件换算成世界坐标后转发)。
    ///
    /// SelectTool 用它产生子路径编辑请求([`SelectTool::take_sub_edit_request`]),
    /// AnchorEditTool 用它切换锚点的直角/平滑。
    fn double_click(&mut self, pt: Point, mods: Mods, ctx: &mut ToolCtx) {
        let _ = (pt, mods, ctx);
    }
    /// 预览层绘制:橡皮筋、钢笔草稿等画布临时视觉(docs/02 §7.3 双缓冲的
    /// "叠加层";不产生 Command,不进撤销栈)。
    fn preview(&self, sink: &mut dyn PaintSink, viewport: &Viewport);
    /// 当前光标样式。
    fn cursor(&self) -> CursorStyle;
}

// —— 工具预览用的固定色(与 render::OverlayTheme::default 同源;
//    预览签名不带主题,theme 由上层渲染层统一,这里用同值常量)——

/// 预览主色(与 [`crate::render::OverlayTheme`] 默认选中色一致的蓝)。
const PREVIEW_BLUE: Rgba8 = [0x4f, 0x9f, 0xff, 0xff];
/// 钢笔预览青(docs/03 §5 参考线/预览的经典青)。
const PEN_PREVIEW: Rgba8 = [0x00, 0xc8, 0xff, 0xff];
/// 钢笔路径默认描边黑(工具生成的路径先用中性描边,属性交给检查器改)。
const PEN_STROKE: Rgba8 = [0x20, 0x20, 0x20, 0xff];

// ===========================================================================
// 本地命令(foundation 序列化镜像缺口,见模块头注记)
// ===========================================================================

/// 【本地命令例外】把新绘的段并入已有 Path 节点(钢笔续接的提交步)。
///
/// 语义与 foundation 内置命令一致:整条 path 以 old/new 对换,天然可撤销。
/// **序列化缺口**:`History::to_serialized` 的 downcast 不认识它 → 该步在
/// 工程保存时被跳过(不崩,重开后该步不可撤销)。主 Agent 补 foundation
/// 镜像时应迁入 `SerializedCommand::AppendToPath`。
#[derive(Debug, Clone)]
pub struct AppendToPath {
    /// 被续接的 Path 节点。
    pub id: NodeId,
    /// 续接前的完整路径。
    pub old_path: BezPath,
    /// 续接后的完整路径(= 旧段 + 新段,闭合时含 `ClosePath`)。
    pub new_path: BezPath,
}

impl Command for AppendToPath {
    fn apply(&mut self, scene: &mut Scene) {
        if let Some(p) = scene.path_mut(self.id) {
            p.path = self.new_path.clone();
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        if let Some(p) = scene.path_mut(self.id) {
            p.path = self.old_path.clone();
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
        Cow::Borrowed("续接路径")
    }
}

/// 【本地命令例外】锚点编辑的统一提交步:拖锚点/拖手柄/转直角平滑/删除/
/// 插入锚点全部落为"整条 path 以 old/new 对换"。
///
/// 序列化缺口同 [`AppendToPath`](迁移名建议 `SerializedCommand::SetAnchorPoints`)。
#[derive(Debug, Clone)]
pub struct SetAnchorPoints {
    /// 被编辑的 Path 节点。
    pub id: NodeId,
    /// 编辑前的完整路径。
    pub old_path: BezPath,
    /// 编辑后的完整路径。
    pub new_path: BezPath,
}

impl Command for SetAnchorPoints {
    fn apply(&mut self, scene: &mut Scene) {
        if let Some(p) = scene.path_mut(self.id) {
            p.path = self.new_path.clone();
        }
    }

    fn revert(&mut self, scene: &mut Scene) -> Option<IdRemap> {
        if let Some(p) = scene.path_mut(self.id) {
            p.path = self.old_path.clone();
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
        Cow::Borrowed("编辑锚点")
    }
}

// ===========================================================================
// SelectTool
// ===========================================================================

/// 选择工具:点选/Shift 加选/橡皮筋框选/拖动移动/8 向控制柄缩放。
///
/// 选中集归工具持有(框架侧经 [`SelectTool::selection`] 只读访问,
/// 供 `RenderOpts::selection` 渲染选中框)。
#[derive(Debug, Default)]
pub struct SelectTool {
    selection: Vec<NodeId>,
    drag: Option<SelectDrag>,
    /// 双击路径节点产生的子路径编辑请求(docs/04 §3 SubPathEdit;
    /// 上层经 [`SelectTool::take_sub_edit_request`] 取走后切换 AnchorEditTool)。
    sub_edit_request: Option<NodeId>,
}

/// 选择工具的拖动种类。
#[derive(Debug, Clone)]
enum SelectDrag {
    /// 橡皮筋框选(start → current 的世界矩形)
    RubberBand { start: Point, current: Point },
    /// 平移拖动(事务内:每次 drag exec SetTransform,up 时 end_transaction;
    /// 没有实际位移的"空事务"由 History::end_transaction 自然丢弃)
    Move {
        start: Point,
        originals: Vec<(NodeId, Affine)>,
    },
    /// 控制柄缩放(以 `anchor` 为不动点缩放)
    Scale {
        anchor: Point,
        /// 各选中节点:初始局部变换 + 父世界逆变换
        originals: Vec<(NodeId, Affine, Affine)>,
        /// 手柄占位的轴向(hx/hy ∈ {0, 1},0 = 该轴不缩放)
        hx: f64,
        hy: f64,
        /// 手柄所在边(该轴缩放的分母基准)
        edge_x: f64,
        edge_y: f64,
    },
}

/// 8 向控制柄方位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handle {
    NorthWest,
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
}

impl Handle {
    /// 全部控制柄(与 `crate::render::handle_positions` 同序)。
    const ALL: [Handle; 8] = [
        Handle::NorthWest,
        Handle::North,
        Handle::NorthEast,
        Handle::East,
        Handle::SouthEast,
        Handle::South,
        Handle::SouthWest,
        Handle::West,
    ];

    /// 控制柄中心(世界坐标)。
    fn position(self, r: Rect) -> Point {
        let cx = (r.x0 + r.x1) / 2.0;
        let cy = (r.y0 + r.y1) / 2.0;
        match self {
            Handle::NorthWest => Point::new(r.x0, r.y0),
            Handle::North => Point::new(cx, r.y0),
            Handle::NorthEast => Point::new(r.x1, r.y0),
            Handle::East => Point::new(r.x1, cy),
            Handle::SouthEast => Point::new(r.x1, r.y1),
            Handle::South => Point::new(cx, r.y1),
            Handle::SouthWest => Point::new(r.x0, r.y1),
            Handle::West => Point::new(r.x0, cy),
        }
    }

    /// 缩放的不动锚点:手柄的对角/对边中点。
    fn anchor(self, r: Rect) -> Point {
        let cx = (r.x0 + r.x1) / 2.0;
        let cy = (r.y0 + r.y1) / 2.0;
        match self {
            Handle::NorthWest => Point::new(r.x1, r.y1),
            Handle::North => Point::new(cx, r.y1),
            Handle::NorthEast => Point::new(r.x0, r.y1),
            Handle::East => Point::new(r.x0, cy),
            Handle::SouthEast => Point::new(r.x0, r.y0),
            Handle::South => Point::new(cx, r.y0),
            Handle::SouthWest => Point::new(r.x1, r.y0),
            Handle::West => Point::new(r.x1, cy),
        }
    }

    /// 各轴是否参与缩放与对应的手柄边坐标。
    fn axes(self, r: Rect) -> (bool, f64, bool, f64) {
        match self {
            Handle::NorthWest => (true, r.x0, true, r.y0),
            Handle::North => (false, r.x0, true, r.y0),
            Handle::NorthEast => (true, r.x1, true, r.y0),
            Handle::East => (true, r.x1, false, r.y0),
            Handle::SouthEast => (true, r.x1, true, r.y1),
            Handle::South => (false, r.x0, true, r.y1),
            Handle::SouthWest => (true, r.x0, true, r.y1),
            Handle::West => (true, r.x0, false, r.y0),
        }
    }
}

impl SelectTool {
    /// 当前选中集(渲染层画选中框用)。
    pub fn selection(&self) -> &[NodeId] {
        &self.selection
    }

    /// 以指定节点初始化选中集(图层面板点击等入口)。
    pub fn select(&mut self, id: NodeId) {
        self.selection.clear();
        self.selection.push(id);
        self.drag = None;
    }

    /// 清空选中集(切换工具/Esc)。
    pub fn clear_selection(&mut self) {
        self.selection.clear();
        self.drag = None;
    }

    /// 取走子路径编辑请求(双击路径节点时产生;一次一取)。
    pub fn take_sub_edit_request(&mut self) -> Option<NodeId> {
        self.sub_edit_request.take()
    }

    /// 命中测试 8 向控制柄:返回落在容差内的手柄(仅当选中集非空)。
    fn hit_handle(&self, pt: Point, ctx: &ToolCtx, tolerance: f64) -> Option<Handle> {
        if self.selection.is_empty() {
            return None;
        }
        let bbox = selection_union(ctx.scene, &self.selection)?;
        Handle::ALL.into_iter().find(|&h| {
            let pos = h.position(bbox);
            (pt - pos).hypot() <= tolerance
        })
    }
}

/// 选中集的世界包围盒并集;全为空(空组)时返回 None。
fn selection_union(scene: &Scene, selection: &[NodeId]) -> Option<Rect> {
    let mut acc: Option<Rect> = None;
    for &id in selection {
        let bbox = scene.node_world_bbox(id)?;
        acc = Some(match acc {
            Some(a) => a.union(bbox),
            None => bbox,
        });
    }
    acc
}

impl ToolBehavior for SelectTool {
    fn mouse_down(&mut self, pt: Point, mods: Mods, ctx: &mut ToolCtx) {
        let tolerance = screen_tolerance(ctx.viewport.zoom);

        // 1. 控制柄优先:命中即进入缩放拖动
        if let Some(handle) = self.hit_handle(pt, ctx, tolerance) {
            self.start_scale(handle, ctx);
            return;
        }

        // 2. 命中对象:点选 / Shift 加减选;随后进入移动拖动(事务内)
        match ctx.scene.hit_test(pt, tolerance) {
            Some(id) => {
                if mods.shift {
                    if let Some(pos) = self.selection.iter().position(|&s| s == id) {
                        self.selection.remove(pos);
                    } else {
                        self.selection.push(id);
                    }
                } else if !self.selection.contains(&id) {
                    self.selection.clear();
                    self.selection.push(id);
                }
                ctx.history.begin_transaction();
                let originals: Vec<(NodeId, Affine)> = self
                    .selection
                    .iter()
                    .filter_map(|&sel| {
                        let xform = ctx.scene.node(sel)?.transform;
                        Some((sel, xform))
                    })
                    .collect();
                self.drag = Some(SelectDrag::Move {
                    start: pt,
                    originals,
                });
            }
            None => {
                // 空白:Shift 保留既有选择续框,否则清空后开始橡皮筋
                if !mods.shift {
                    self.selection.clear();
                }
                self.drag = Some(SelectDrag::RubberBand {
                    start: pt,
                    current: pt,
                });
            }
        }
    }

    fn mouse_drag(&mut self, pt: Point, _mods: Mods, ctx: &mut ToolCtx) {
        match &mut self.drag {
            Some(SelectDrag::RubberBand { current, .. }) => {
                *current = pt;
            }
            Some(SelectDrag::Move { start, originals }) => {
                let delta = pt - *start;
                apply_transforms(ctx, originals, |original| {
                    *original * Affine::translate(delta)
                });
            }
            Some(SelectDrag::Scale {
                anchor,
                originals,
                hx,
                hy,
                edge_x,
                edge_y,
            }) => {
                let sx = axis_scale(pt.x, anchor.x, *edge_x, *hx);
                let sy = axis_scale(pt.y, anchor.y, *edge_y, *hy);
                let anchor_v = anchor.to_vec2();
                // 以锚点为不动点的世界系缩放,再变换回节点局部系:
                // new_local = parent⁻¹ · S(anchor) · parent · local
                let world_scale = Affine::translate(anchor_v)
                    * Affine::scale_non_uniform(sx, sy)
                    * Affine::translate(-anchor_v);
                for (id, local, parent_inv) in originals.iter() {
                    let parent = parent_inv.inverse();
                    let new = *parent_inv * world_scale * parent * *local;
                    exec_transform(ctx, *id, *local, new);
                }
            }
            None => {}
        }
    }

    fn mouse_up(&mut self, _pt: Point, _mods: Mods, ctx: &mut ToolCtx) {
        match self.drag.take() {
            Some(SelectDrag::RubberBand { start, current }) => {
                let rect = Rect::from_points(start, current);
                let hits = ctx.scene.select_in_rect(rect);
                for id in hits {
                    if !self.selection.contains(&id) {
                        self.selection.push(id);
                    }
                }
            }
            // 空事务(点了但没拖)不会产生撤销步
            Some(_) => {
                ctx.history.end_transaction();
            }
            None => {}
        }
    }

    fn double_click(&mut self, pt: Point, _mods: Mods, ctx: &mut ToolCtx) {
        // 双击 Path 节点 → 请求进入锚点编辑(docs/04 §3 SubPathEdit)。
        // 只置请求不切工具:工具间的切换权在上层(framework-free 组合)。
        let tolerance = screen_tolerance(ctx.viewport.zoom);
        let Some(id) = ctx.scene.hit_test(pt, tolerance) else {
            return;
        };
        let is_path = ctx
            .scene
            .node(id)
            .is_some_and(|n| matches!(n.content, NodeContent::Path(_)));
        if is_path {
            self.selection.clear();
            self.selection.push(id);
            self.drag = None;
            self.sub_edit_request = Some(id);
        }
    }

    fn preview(&self, sink: &mut dyn PaintSink, viewport: &Viewport) {
        let Some(SelectDrag::RubberBand { start, current }) = &self.drag else {
            return;
        };
        let rect = Rect::from_points(*start, *current);
        let vp = viewport.world_to_viewport();
        let z = viewport.zoom;
        // 经典橡皮筋:半透明填充 + 1px 屏幕等宽描边
        sink.fill(
            &Paint::Solid([PREVIEW_BLUE[0], PREVIEW_BLUE[1], PREVIEW_BLUE[2], 28]),
            vp,
            &rect.to_path(0.1),
        );
        sink.stroke(
            &StrokeStyle {
                paint: Paint::Solid(PREVIEW_BLUE),
                width: 1.0 / z,
            },
            vp,
            &rect.to_path(0.1),
        );
    }

    fn cursor(&self) -> CursorStyle {
        match &self.drag {
            Some(SelectDrag::Move { .. }) | Some(SelectDrag::Scale { .. }) => CursorStyle::Move,
            Some(SelectDrag::RubberBand { .. }) | None => CursorStyle::Default,
        }
    }
}

impl SelectTool {
    /// 进入缩放拖动(事务内;选中集整体按 union 包围盒缩放)。
    fn start_scale(&mut self, handle: Handle, ctx: &mut ToolCtx) {
        let Some(bbox) = selection_union(ctx.scene, &self.selection) else {
            return;
        };
        let originals: Vec<(NodeId, Affine, Affine)> = self
            .selection
            .iter()
            .filter_map(|&id| {
                let xform = ctx.scene.node(id)?.transform;
                let parent_world = ctx
                    .scene
                    .node(id)
                    .and_then(|n| n.parent)
                    .and_then(|p| ctx.scene.world_transform(p))
                    .unwrap_or(Affine::IDENTITY);
                Some((id, xform, parent_world.inverse()))
            })
            .collect();
        if originals.is_empty() {
            return;
        }
        ctx.history.begin_transaction();
        let (has_x, edge_x, has_y, edge_y) = handle.axes(bbox);
        self.drag = Some(SelectDrag::Scale {
            anchor: handle.anchor(bbox),
            originals,
            hx: if has_x { 1.0 } else { 0.0 },
            hy: if has_y { 1.0 } else { 0.0 },
            edge_x,
            edge_y,
        });
    }
}

/// 对一组 (节点, 初始变换) 统一应用变换函数:exec SetTransform 并记账脏区。
/// 执行单个 SetTransform 并标记新旧包围盒脏区(Move/Scale 共用的最小步)。
fn exec_transform(ctx: &mut ToolCtx, id: NodeId, old: Affine, new: Affine) {
    let old_bbox = ctx.scene.node_world_bbox(id);
    ctx.history
        .exec(SetTransform { id, old, new }.boxed(), ctx.scene);
    if let Some(b) = old_bbox {
        ctx.damage.mark(b);
    }
    if let Some(b) = ctx.scene.node_world_bbox(id) {
        ctx.damage.mark(b);
    }
}

fn apply_transforms(
    ctx: &mut ToolCtx,
    originals: &[(NodeId, Affine)],
    f: impl Fn(&Affine) -> Affine,
) {
    for (id, original) in originals {
        exec_transform(ctx, *id, *original, f(original));
    }
}

/// 单轴缩放因子:光标相对锚点的距离 / 手柄初始边相对锚点的距离。
/// 不参与缩放的轴(占位 0)恒返回 1;基准退化(边贴锚点)时保持 1,避免除零。
fn axis_scale(cursor: f64, anchor: f64, edge: f64, occupied: f64) -> f64 {
    if occupied == 0.0 {
        return 1.0;
    }
    let denominator = edge - anchor;
    if denominator.abs() < 1e-9 {
        return 1.0;
    }
    (cursor - anchor) / denominator
}

// ===========================================================================
// PenTool
// ===========================================================================

/// 已落下的钢笔锚点(贝塞尔编辑核心,docs/04 §3)。
#[derive(Clone, Copy, Debug, PartialEq)]
struct PenAnchor {
    pos: Point,
    /// 出手柄(离开该锚点方向的控制点)
    out: Option<Point>,
    /// 入手柄(进入该锚点方向的控制点);`None` = 尖角(Alt 拖出的单边手柄)
    in_: Option<Point>,
}

impl PenAnchor {
    /// hit_test 锚点模型 → 钢笔锚点。
    fn from_anchor_point(a: &AnchorPoint) -> Self {
        PenAnchor {
            pos: a.pos,
            out: a.out_handle,
            in_: a.in_handle,
        }
    }

    /// 钢笔锚点 → hit_test 锚点模型(锚点编辑/最近点查询用)。
    fn to_anchor_point(self) -> AnchorPoint {
        AnchorPoint {
            pos: self.pos,
            in_handle: self.in_,
            out_handle: self.out,
        }
    }
}

/// 正在放置中的锚点(等 mouse_up 决定直角/平滑/尖角)。
#[derive(Clone, Copy, Debug)]
struct PendingAnchor {
    pos: Point,
    handle: Option<Point>,
    sharp: bool,
}

/// 钢笔悬停命中(docs/04 §3 `PenHover`;preview 只画高亮,不改状态)。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum PenHover {
    #[default]
    None,
    /// 悬停已有路径的端点(续接提示;pos = 端点世界坐标)
    Endpoint {
        id: NodeId,
        at_head: bool,
        pos: Point,
    },
    /// 悬停草稿起点(闭合提示)
    Close { pos: Point },
}

/// 进行中的续接:目标节点 + 续接前的原路径(命令回退锚点)。
#[derive(Clone, Debug)]
struct Continuation {
    id: NodeId,
    old_path: BezPath,
}

/// 钢笔工具(docs/04 §3 状态机):点击落锚点,拖动出手柄,点起点闭合提交;
/// 空闲时点击已有路径端点则从该端续接。
#[derive(Debug, Default)]
pub struct PenTool {
    anchors: Vec<PenAnchor>,
    pending: Option<PendingAnchor>,
    /// 最后已知鼠标位置(橡皮筋预览用;拖动间隙也会保留上一次的值)
    cursor_pt: Option<Point>,
    /// 非空 = 正在续接已有路径(草稿锚点 = 原路径锚点分解 + 新锚点)
    continuing: Option<Continuation>,
    /// 悬停命中(端点续接提示/起点闭合提示;`hover_at`/`mouse_down` 时刷新)
    hover: PenHover,
}

impl PenTool {
    /// 路径草稿的锚点数(状态面板显示用)。
    pub fn anchor_count(&self) -> usize {
        self.anchors.len()
    }

    /// 是否有进行中的草稿。
    pub fn has_draft(&self) -> bool {
        !self.anchors.is_empty() || self.pending.is_some()
    }

    /// 正在续接的目标节点(上层/状态面板显示用)。
    pub fn continuing_node(&self) -> Option<NodeId> {
        self.continuing.as_ref().map(|c| c.id)
    }

    /// 取消草稿(Esc/右键;上层调用)。续接中的状态一并丢弃,不产生任何命令。
    pub fn cancel_draft(&mut self) {
        self.anchors.clear();
        self.pending = None;
        self.cursor_pt = None;
        self.continuing = None;
        self.hover = PenHover::None;
    }

    /// 悬停刷新(上层在鼠标移动时调用;`mouse_down` 内部也会先调一次)。
    ///
    /// 判定顺序:草稿起点(闭合提示)→ 已有路径端点(仅空闲时,续接提示)。
    pub fn hover_at(&mut self, pt: Point, ctx: &mut ToolCtx) {
        self.cursor_pt = Some(pt);
        let tolerance = screen_tolerance(ctx.viewport.zoom);
        self.hover = if self.clicked_start(pt, tolerance) {
            PenHover::Close {
                pos: self.anchors[0].pos,
            }
        } else if !self.has_draft() {
            self.find_endpoint(pt, ctx, tolerance)
                .map(|(id, at_head, pos)| PenHover::Endpoint { id, at_head, pos })
                .unwrap_or(PenHover::None)
        } else {
            PenHover::None
        };
    }

    /// 悬停命中已有路径的端点(顶→底首个;闭合路径/单锚路径不可续接)。
    fn find_endpoint(
        &self,
        pt: Point,
        ctx: &ToolCtx,
        tolerance: f64,
    ) -> Option<(NodeId, bool, Point)> {
        for (id, xform) in ctx.scene.render_list().into_iter().rev() {
            let Some(node) = ctx.scene.node(id) else {
                continue;
            };
            if node.locked {
                continue;
            }
            let NodeContent::Path(p) = &node.content else {
                continue;
            };
            let Some(sub) = first_subpath(&p.path) else {
                continue;
            };
            if sub.closed || sub.anchors.len() < 2 {
                continue;
            }
            let local = xform.inverse() * pt;
            for (at_head, end) in [(true, sub.anchors[0].pos), (false, sub.anchors.last()?.pos)] {
                if (local - end).hypot() <= tolerance {
                    return Some((id, at_head, xform * end));
                }
            }
        }
        None
    }

    /// 从已有路径的端点开始续接(docs/04 §3:点击路径端点 ──▶ 续接路径)。
    ///
    /// 草稿锚点 = 原路径的锚点分解;从头端续接时反转序列(统一为"尾部追加",
    /// 代价是新路径以原尾锚为起点——几何等价,仅参数化方向反转,v1.0 接受)。
    fn begin_continuation(&mut self, id: NodeId, at_head: bool, ctx: &mut ToolCtx) {
        let Some(node) = ctx.scene.path(id) else {
            return;
        };
        let Some(sub) = first_subpath(&node.path) else {
            return;
        };
        if sub.closed || sub.anchors.len() < 2 {
            return;
        }
        let mut anchors: Vec<PenAnchor> = sub
            .anchors
            .iter()
            .map(PenAnchor::from_anchor_point)
            .collect();
        if at_head {
            anchors.reverse();
            for a in &mut anchors {
                std::mem::swap(&mut a.in_, &mut a.out);
            }
        }
        self.anchors = anchors;
        self.pending = None;
        self.cursor_pt = None;
        self.continuing = Some(Continuation {
            id,
            old_path: node.path.clone(),
        });
    }

    /// 点击是否落在路径起点(容差内)→ 请求闭合。
    fn clicked_start(&self, pt: Point, tolerance: f64) -> bool {
        self.anchors.len() >= 2
            && self
                .anchors
                .first()
                .is_some_and(|first| (pt - first.pos).hypot() <= tolerance)
    }

    /// 闭合提交(docs/04 §3 `commit`):续接中 → AppendToPath 并入原节点;
    /// 新草稿 → AddNode 生成节点。
    fn commit_closed(&mut self, ctx: &mut ToolCtx) {
        if let Some(cont) = self.continuing.take() {
            let new_path = build_path(&self.anchors, true);
            self.commit_append(ctx, cont.id, cont.old_path, new_path);
            self.cancel_draft();
            return;
        }
        let path = build_path(&self.anchors, true);
        let bbox = path.bounding_box();
        ctx.history.exec(
            AddNode::new(None, None, make_pen_node(path)).boxed(),
            ctx.scene,
        );
        ctx.damage.mark(bbox);
        self.cancel_draft();
    }

    /// 开放提交(Enter/切工具;上层调用):续接中 → AppendToPath(开放段并入);
    /// 新草稿(≥2 锚)→ AddNode 开放路径。两者都是一条命令、一步撤销。
    pub fn commit_open(&mut self, ctx: &mut ToolCtx) {
        if let Some(cont) = self.continuing.take() {
            let new_path = build_path(&self.anchors, false);
            self.commit_append(ctx, cont.id, cont.old_path, new_path);
        } else if self.anchors.len() >= 2 {
            let path = build_path(&self.anchors, false);
            let bbox = path.bounding_box();
            ctx.history.exec(
                AddNode::new(None, None, make_pen_node(path)).boxed(),
                ctx.scene,
            );
            ctx.damage.mark(bbox);
        }
        self.cancel_draft();
    }

    /// 续接的提交点:exec 一条本地 [`AppendToPath`] 命令并记账新旧脏区。
    ///
    /// foundation 的序列化镜像补齐前,该步在 `History::to_serialized` 中被
    /// 跳过(撤销栈保存时此步不持久化,不崩;见模块头注记)。
    pub fn commit_append(
        &mut self,
        ctx: &mut ToolCtx,
        id: NodeId,
        old_path: BezPath,
        new_path: BezPath,
    ) {
        let old_bbox = ctx.scene.node_world_bbox(id);
        ctx.history.exec(
            AppendToPath {
                id,
                old_path,
                new_path,
            }
            .boxed(),
            ctx.scene,
        );
        if let Some(b) = old_bbox {
            ctx.damage.mark(b);
        }
        if let Some(b) = ctx.scene.node_world_bbox(id) {
            ctx.damage.mark(b);
        }
    }
}

/// 钢笔产出的节点:中性描边、无填充(属性交给检查器改)。
fn make_pen_node(path: BezPath) -> sable_foundation::scene::Node {
    sable_foundation::scene::Node::new(
        "钢笔路径",
        NodeContent::Path(PathNode {
            path,
            fill: None,
            stroke: Some(StrokeStyle {
                paint: Paint::Solid(PEN_STROKE),
                width: 1.5,
            }),
        }),
    )
}

/// 锚点序列 → BezPath:相邻锚点间以 (出, 入) 手柄连三次贝塞尔,
/// 缺手柄的锚点退化为直线控制点(直角点)。
fn build_path(anchors: &[PenAnchor], close: bool) -> BezPath {
    let mut path = BezPath::new();
    let Some(first) = anchors.first() else {
        return path;
    };
    path.move_to(first.pos);
    for pair in anchors.windows(2) {
        let a = &pair[0];
        let b = &pair[1];
        let c1 = a.out.unwrap_or(a.pos);
        let c2 = b.in_.unwrap_or(b.pos);
        path.curve_to(c1, c2, b.pos);
    }
    if close {
        path.close_path();
    }
    path
}

impl ToolBehavior for PenTool {
    fn mouse_down(&mut self, pt: Point, mods: Mods, ctx: &mut ToolCtx) {
        // 先刷悬停判定(起点闭合 / 端点续接都依赖它;同时更新橡皮筋光标)
        self.hover_at(pt, ctx);
        match self.hover {
            // 点击起点 → 闭合 → commit(docs/04 §3)
            PenHover::Close { .. } => {
                self.commit_closed(ctx);
                return;
            }
            // 空闲时点击已有路径端点 → 从该端续接(docs/04 §3)
            PenHover::Endpoint { id, at_head, .. } => {
                if !self.has_draft() {
                    self.begin_continuation(id, at_head, ctx);
                    return;
                }
            }
            PenHover::None => {}
        }
        self.pending = Some(PendingAnchor {
            pos: pt,
            handle: None,
            sharp: mods.alt,
        });
        self.cursor_pt = Some(pt);
    }

    fn mouse_drag(&mut self, pt: Point, mods: Mods, _ctx: &mut ToolCtx) {
        if let Some(pending) = &mut self.pending {
            pending.handle = Some(pt);
            // Alt 拖 = 单边尖点(docs/04 §3);松开 Alt 恢复平滑
            pending.sharp = mods.alt;
        }
        self.cursor_pt = Some(pt);
    }

    fn mouse_up(&mut self, _pt: Point, mods: Mods, _ctx: &mut ToolCtx) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let sharp = pending.sharp || mods.alt;
        // 平滑点:入手柄 = 出手柄关于锚点的镜像;尖点(Alt):单边无入手柄
        let in_ = if sharp {
            None
        } else {
            pending.handle.map(|h| pending.pos + (pending.pos - h))
        };
        self.anchors.push(PenAnchor {
            pos: pending.pos,
            out: pending.handle,
            in_,
        });
    }

    fn preview(&self, sink: &mut dyn PaintSink, viewport: &Viewport) {
        let vp = viewport.world_to_viewport();
        let z = viewport.zoom;

        // hover 视觉反馈(docs/04 §3 PenHover):只画,不改状态
        match self.hover {
            // 端点续接提示:空心高亮圈
            PenHover::Endpoint { pos, .. } => {
                stroke_circle(sink, vp, z, pos, 3.5 / z, PEN_PREVIEW, 1.5);
            }
            // 起点闭合提示:加粗变色高亮圈 + 锚点标记
            PenHover::Close { pos } => {
                stroke_circle(sink, vp, z, pos, 4.5 / z, PREVIEW_BLUE, 2.0);
                draw_anchor_marker(sink, vp, z, pos);
            }
            PenHover::None => {}
        }

        if !self.has_draft() {
            return;
        }
        let style = StrokeStyle {
            paint: Paint::Solid(PEN_PREVIEW),
            width: 1.0 / z, // 屏幕等宽 1px(覆盖层)
        };

        // 已落锚点连成的草稿曲线
        if self.anchors.len() >= 2 {
            sink.stroke(&style, vp, &build_path(&self.anchors, false));
        }

        // 橡皮筋:最后锚点 → 正在放置的锚点/鼠标(docs/04 §3 preview)
        if let Some(last) = self.anchors.last() {
            if let Some(pending) = self.pending {
                let mut seg = BezPath::new();
                seg.move_to(last.pos);
                seg.line_to(pending.pos);
                sink.stroke(&style, vp, &seg);
                draw_anchor_marker(sink, vp, z, pending.pos);
                // 拖出的手柄:锚点 → 手柄 的控制杆
                if let Some(h) = pending.handle {
                    let mut lever = BezPath::new();
                    lever.move_to(pending.pos);
                    lever.line_to(h);
                    sink.stroke(&style, vp, &lever);
                }
            } else if let Some(cursor) = self.cursor_pt {
                let mut seg = BezPath::new();
                seg.move_to(last.pos);
                seg.line_to(cursor);
                sink.stroke(&style, vp, &seg);
            }
        }

        // 已落锚点标记 + 起点高亮(可闭合提示)
        for anchor in &self.anchors {
            draw_anchor_marker(sink, vp, z, anchor.pos);
        }
    }

    fn cursor(&self) -> CursorStyle {
        CursorStyle::Crosshair
    }
}

/// 锚点标记:3px/zoom 小方块。
fn draw_anchor_marker(sink: &mut dyn PaintSink, vp: Affine, zoom: f64, pos: Point) {
    let half = 1.5 / zoom;
    let rect = Rect::new(pos.x - half, pos.y - half, pos.x + half, pos.y + half);
    sink.fill(&Paint::Solid(PEN_PREVIEW), vp, &rect.to_path(0.1));
}

/// 空心圆环(hover 高亮圈:续接端点/闭合提示/悬停锚点)。
fn stroke_circle(
    sink: &mut dyn PaintSink,
    vp: Affine,
    zoom: f64,
    center: Point,
    radius: f64,
    color: Rgba8,
    width_px: f64,
) {
    let style = StrokeStyle {
        paint: Paint::Solid(color),
        width: width_px / zoom,
    };
    sink.stroke(&style, vp, &Circle::new(center, radius).to_path(0.1));
}

// ===========================================================================
// AnchorEditTool(锚点编辑,docs/04 §3 SubPathEdit 模式)
// ===========================================================================

/// 手柄方位(选中锚点的入/出控制点)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HandleSide {
    In,
    Out,
}

/// 锚点编辑的悬停命中(判定优先级:选中锚点的手柄 → 锚点 → 段上插入点)。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum AnchorHover {
    #[default]
    None,
    /// 悬停锚点(高亮圈)
    Anchor(usize),
    /// 悬停选中锚点的手柄
    Handle { index: usize, side: HandleSide },
    /// 悬停段上插入点(AddAnchor 吸附提示,docs/04 §3)
    Insert { segment: usize, t: f64, pos: Point },
}

/// 锚点编辑拖动的种类(事务内:每次 drag exec SetAnchorPoints,up 时合并)。
#[derive(Clone, Debug)]
enum AnchorDrag {
    /// 拖锚点本体:曲线锚点的相邻控制点随锚点整体平移(保持相对位置)
    Anchor {
        grab: Point,
        originals: Vec<PenAnchor>,
        index: usize,
    },
    /// 拖手柄:只改被拖控制点;Shift = 对称联动(对侧取镜像)
    Handle {
        originals: Vec<PenAnchor>,
        index: usize,
        side: HandleSide,
    },
}

/// 锚点编辑工具(docs/04 §3 SubPathEdit):显示全部锚点,拖锚点/手柄、
/// 双击转直角平滑、删除、段上插入锚点。
///
/// 进入方式:SelectTool 双击路径节点 → 上层取 [`SelectTool::take_sub_edit_request`]
/// 切换到本工具(`AnchorEditTool::new(id)`)。所有修改统一 exec 本地
/// [`SetAnchorPoints`] 命令(见模块头"本地命令例外");拖动用事务合并为
/// 一步撤销。Esc(上层调 [`AnchorEditTool::escape`])或点击空白置位
/// [`AnchorEditTool::exit_requested`],由上层切回 Select。
///
/// 坐标口径:事件点是世界坐标,内部一律换算到目标节点局部系;`world` 是
/// 事件时刷新的世界变换快照(preview 无 ctx,只能用快照做局部→世界)。
#[derive(Debug, Default)]
pub struct AnchorEditTool {
    /// 被编辑的 Path 节点。
    target: Option<NodeId>,
    /// 目标节点世界变换快照(每次事件刷新)。
    world: Affine,
    /// 目标路径是否闭合(决定闭合段的存在与删除下限)。
    closed: bool,
    /// 目标路径的锚点工作副本(每次事件从场景同步;拖动中由拖动逻辑改写)。
    cache: Vec<PenAnchor>,
    /// 当前选中锚点(手柄只对选中锚点显示/可拖)。
    selected: Option<usize>,
    drag: Option<AnchorDrag>,
    hover: AnchorHover,
    /// 点击空白/Esc 置位;上层轮询到即切回 Select。
    exit_request: bool,
}

impl AnchorEditTool {
    /// 进入锚点编辑态(双击路径节点后由上层构造)。
    pub fn new(id: NodeId) -> Self {
        AnchorEditTool {
            target: Some(id),
            ..AnchorEditTool::default()
        }
    }

    /// 被编辑的节点。
    pub fn target(&self) -> Option<NodeId> {
        self.target
    }

    /// 当前选中锚点下标。
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// 是否请求退出编辑态(点击空白/Esc 后为真;上层切回 Select)。
    pub fn exit_requested(&self) -> bool {
        self.exit_request
    }

    /// Esc(上层键盘事件转发):清除选中并请求退出。
    pub fn escape(&mut self) {
        self.exit_request = true;
        self.selected = None;
    }

    /// 删除当前选中锚点(Delete/Backspace 键;上层调用)。
    pub fn delete_selected(&mut self, ctx: &mut ToolCtx) {
        let Some(index) = self.selected else {
            return;
        };
        self.delete_anchor(index, ctx);
    }

    /// 悬停刷新(上层鼠标移动时调用):锚点/手柄高亮与插入点吸附提示。
    pub fn hover_at(&mut self, pt: Point, ctx: &mut ToolCtx) {
        if !self.refresh(ctx) {
            return;
        }
        let tolerance = screen_tolerance(ctx.viewport.zoom);
        let local = self.world.inverse() * pt;
        self.hover = self.compute_hover(local, tolerance);
    }

    /// 事件入口的同步:目标存在 → 刷新世界变换快照与锚点缓存(拖动中
    /// 缓存由拖动逻辑独占,不从场景覆盖)。目标失效 → 返回 false 事件作废。
    fn refresh(&mut self, ctx: &ToolCtx) -> bool {
        let Some(id) = self.target else {
            return false;
        };
        let Some(world) = ctx.scene.world_transform(id) else {
            return false;
        };
        let Some(node) = ctx.scene.path(id) else {
            return false;
        };
        self.sync_from(&node.path, world)
    }

    /// 从场景路径同步工作副本(无 ToolCtx 的纯读路径,供 preview 用)。
    pub fn sync_from(&mut self, path: &BezPath, world: Affine) -> bool {
        let Some(sub) = first_subpath(path) else {
            return false;
        };
        self.world = world;
        self.closed = sub.closed;
        if self.drag.is_none() {
            self.cache = sub
                .anchors
                .iter()
                .map(PenAnchor::from_anchor_point)
                .collect();
        }
        true
    }

    /// 测试/上层入口:同步场景路径后绘制覆盖层(无 ToolCtx;不改任何状态)。
    pub fn preview_scene(
        &mut self,
        path: &BezPath,
        world: Affine,
        sink: &mut dyn PaintSink,
        viewport: &Viewport,
    ) {
        if self.sync_from(path, world) {
            let vp = viewport.world_to_viewport();
            let z = viewport.zoom;
            self.draw_overlay_vp(vp, z, sink);
        }
    }

    /// 覆盖层绘制(缓存快照)。
    fn draw_overlay_vp(&self, vp: Affine, z: f64, sink: &mut dyn PaintSink) {
        // 路径轮廓(拖动中为实时形态;屏幕等宽 1px)
        sink.stroke(
            &StrokeStyle {
                paint: Paint::Solid(PREVIEW_BLUE),
                width: 1.0 / z,
            },
            vp,
            &build_path(&self.cache, self.closed),
        );

        // 全部锚点:实心方块
        for a in &self.cache {
            draw_anchor_marker(sink, vp, z, self.world * a.pos);
        }

        // 选中锚点:高亮方块 + 手柄杆与空心手柄圆
        if let Some(sel) = self.selected {
            let a = &self.cache[sel];
            let pos_w = self.world * a.pos;
            let half = 2.5 / z;
            let rect = Rect::new(
                pos_w.x - half,
                pos_w.y - half,
                pos_w.x + half,
                pos_w.y + half,
            );
            sink.fill(&Paint::Solid(PREVIEW_BLUE), vp, &rect.to_path(0.1));
            for h in [a.in_, a.out].into_iter().flatten() {
                let hw = self.world * h;
                let mut lever = BezPath::new();
                lever.move_to(pos_w);
                lever.line_to(hw);
                sink.stroke(
                    &StrokeStyle {
                        paint: Paint::Solid(PREVIEW_BLUE),
                        width: 1.0 / z,
                    },
                    vp,
                    &lever,
                );
                stroke_circle(sink, vp, z, hw, 2.5 / z, PEN_PREVIEW, 1.5);
            }
        }

        // hover 高亮(docs/04 §3 PenHover):只画,不改状态;
        // 已选中的锚点不画悬停圈(选中方块已是高亮,双重指示冗余)
        match self.hover {
            AnchorHover::Anchor(index) if self.selected == Some(index) => {}
            AnchorHover::Anchor(index) => {
                stroke_circle(
                    sink,
                    vp,
                    z,
                    self.world * self.cache[index].pos,
                    4.0 / z,
                    PREVIEW_BLUE,
                    1.5,
                );
            }
            AnchorHover::Handle { index, side } => {
                let a = &self.cache[index];
                if let Some(h) = match side {
                    HandleSide::In => a.in_,
                    HandleSide::Out => a.out,
                } {
                    stroke_circle(sink, vp, z, self.world * h, 4.0 / z, PEN_PREVIEW, 1.5);
                }
            }
            AnchorHover::Insert { pos, .. } => {
                // AddAnchor 吸附提示:空心圈 + 实心小点
                stroke_circle(sink, vp, z, self.world * pos, 3.5 / z, PEN_PREVIEW, 1.5);
                draw_anchor_marker(sink, vp, z, self.world * pos);
            }
            AnchorHover::None => {}
        }
    }

    /// 悬停判定(局部坐标):选中锚点的手柄 → 锚点 → 段上插入点。
    /// 手柄优先于锚点:手柄通常贴近自己的锚点,先查才抓得到;零长度手柄
    /// (与锚点重合)不参与命中,避免遮蔽锚点选择。
    fn compute_hover(&self, local: Point, tolerance: f64) -> AnchorHover {
        if let Some(sel) = self.selected {
            let a = &self.cache[sel];
            let near = |h: Point| (h - local).hypot() <= tolerance && (h - a.pos).hypot() > 1e-9;
            if a.in_.is_some_and(near) {
                return AnchorHover::Handle {
                    index: sel,
                    side: HandleSide::In,
                };
            }
            if a.out.is_some_and(near) {
                return AnchorHover::Handle {
                    index: sel,
                    side: HandleSide::Out,
                };
            }
        }
        let sub = self.subpath_snapshot();
        if let Some(index) = hit_anchor(&sub.anchors, local, tolerance) {
            return AnchorHover::Anchor(index);
        }
        // AddAnchor 吸附提示:最近点落在容差内且不贴段端(t 贴端 = 锚点本身)
        if let Some(n) = nearest_on_subpath(&sub, local, INSERT_NEAREST_ACCURACY) {
            if n.distance <= tolerance && n.t > INSERT_T_MARGIN && n.t < 1.0 - INSERT_T_MARGIN {
                return AnchorHover::Insert {
                    segment: n.segment,
                    t: n.t,
                    pos: n.point,
                };
            }
        }
        AnchorHover::None
    }

    /// 锚点缓存 → hit_test 子路径模型(最近点/命中查询用)。
    fn subpath_snapshot(&self) -> SubPath {
        SubPath {
            closed: self.closed,
            anchors: self.cache.iter().map(|a| a.to_anchor_point()).collect(),
        }
    }

    /// 插入锚点:分割 segment 号段(t 处 de Casteljau),一次 exec = 一步撤销。
    fn insert_anchor(&mut self, segment: usize, t: f64, ctx: &mut ToolCtx) {
        let originals = self.cache.clone();
        let n = originals.len();
        let closed = self.closed;
        let windows = n.saturating_sub(1); // windows(2) 段数(闭合段不计入)
        let mut next = originals.clone();
        let new_index = if segment < windows {
            let (a, new, b) = split_segment(originals[segment], originals[segment + 1], t);
            next[segment] = a;
            next.insert(segment + 1, new);
            next[segment + 2] = b;
            segment + 1
        } else if closed && n >= 2 && segment == windows {
            // 闭合段(末锚→首锚):新锚点追加到序列尾,首尾锚各补一侧手柄
            let (a, new, b) = split_segment(originals[n - 1], originals[0], t);
            next[n - 1] = a;
            next.push(new);
            next[0] = b;
            n
        } else {
            return;
        };
        self.selected = Some(new_index);
        self.cache = next.clone();
        if let Some(id) = self.target {
            exec_anchor_points(ctx, id, build_path(&originals, closed), &next, closed);
        }
    }

    /// 删除锚点(线段自动合并;首锚删除后由次锚接管头部 = 首尾接续)。
    /// 保留下限:开路径 ≥2 锚、闭路径 ≥3 锚,低于下限不响应。
    fn delete_anchor(&mut self, index: usize, ctx: &mut ToolCtx) {
        let floor = if self.closed { 3 } else { 2 };
        if self.cache.len() <= floor || index >= self.cache.len() {
            return;
        }
        let originals = self.cache.clone();
        let mut next = originals.clone();
        next.remove(index);
        self.selected = None;
        self.cache = next.clone();
        if let Some(id) = self.target {
            exec_anchor_points(
                ctx,
                id,
                build_path(&originals, self.closed),
                &next,
                self.closed,
            );
        }
    }

    /// 双击切换直角↔平滑:带手柄 → 全部移除(直角);直角 → 由邻段方向
    /// 自动配一对共线手柄(平滑,长度 = 到邻锚距离的 1/3)。
    fn toggle_corner_smooth(&mut self, index: usize, ctx: &mut ToolCtx) {
        let originals = self.cache.clone();
        let mut next = originals.clone();
        let has_handles = next[index].in_.is_some() || next[index].out.is_some();
        if has_handles {
            next[index].in_ = None;
            next[index].out = None;
        } else {
            let Some(dir) = auto_smooth_dir(&originals, index) else {
                return;
            };
            let a = &mut next[index];
            if let Some(nxt) = originals.get(index + 1) {
                let len = a.pos.distance(nxt.pos) / 3.0;
                a.out = Some(a.pos + dir * len);
            }
            if index > 0 {
                let len = a.pos.distance(originals[index - 1].pos) / 3.0;
                a.in_ = Some(a.pos - dir * len);
            }
        }
        self.cache = next.clone();
        if let Some(id) = self.target {
            exec_anchor_points(
                ctx,
                id,
                build_path(&originals, self.closed),
                &next,
                self.closed,
            );
        }
    }
}

/// AddAnchor 插入点的最近点精度:粗精度下 t 偏差会放大成 0.6px 级的插入点
/// 偏移(实测),交互单段查询代价可忽略,收紧到近解析精度。
const INSERT_NEAREST_ACCURACY: f64 = 0.02;
/// 插入点的参数边距:t 贴段端视为锚点本身,不提供插入提示。
const INSERT_T_MARGIN: f64 = 0.02;

/// 把 a→b 段在参数 t 处一分为二(de Casteljau;kurbo `ParamCurve::subsegment`,
/// 已核实 0.13.1:`CubicBez::subdivide` 只切 t=0.5,任意 t 走 subsegment):
/// 返回 (更新后的 a, 新锚点, 更新后的 b)。双侧无手柄的直线段走线性内插,
/// 新锚点不带手柄(两段仍为直线)。
fn split_segment(a: PenAnchor, b: PenAnchor, t: f64) -> (PenAnchor, PenAnchor, PenAnchor) {
    if a.out.is_none() && b.in_.is_none() {
        let mid = a.pos + (b.pos - a.pos) * t;
        return (
            a,
            PenAnchor {
                pos: mid,
                out: None,
                in_: None,
            },
            b,
        );
    }
    let cubic = CubicBez::new(a.pos, a.out.unwrap_or(a.pos), b.in_.unwrap_or(b.pos), b.pos);
    let left = cubic.subsegment(0.0..t);
    let right = cubic.subsegment(t..1.0);
    let mut new_a = a;
    new_a.out = Some(left.p1);
    let new = PenAnchor {
        pos: left.p3,
        out: Some(right.p1),
        in_: Some(left.p2),
    };
    let mut new_b = b;
    new_b.in_ = Some(right.p2);
    (new_a, new, new_b)
}

/// 平滑点自动配手柄的方向:相邻两锚点连线方向(端点锚取单侧邻方向)。
fn auto_smooth_dir(anchors: &[PenAnchor], i: usize) -> Option<Vec2> {
    let pos = anchors[i].pos;
    let d = match (
        i.checked_sub(1).map(|j| anchors[j].pos),
        anchors.get(i + 1).map(|a| a.pos),
    ) {
        (Some(prev), Some(next)) => next - prev,
        (None, Some(next)) => next - pos,
        (Some(prev), None) => pos - prev,
        (None, None) => return None,
    };
    (d.hypot() > 1e-9).then_some(d.normalize())
}

/// 执行一条 SetAnchorPoints 并记账新旧包围盒脏区(锚点编辑的统一提交步;
/// 拖动中每帧调用,事务在 mouse_down/up 开合 → 整次拖动一步撤销)。
fn exec_anchor_points(
    ctx: &mut ToolCtx,
    id: NodeId,
    old_path: BezPath,
    new_anchors: &[PenAnchor],
    closed: bool,
) {
    let new_path = build_path(new_anchors, closed);
    let old_bbox = ctx.scene.node_world_bbox(id);
    ctx.history.exec(
        SetAnchorPoints {
            id,
            old_path,
            new_path,
        }
        .boxed(),
        ctx.scene,
    );
    if let Some(b) = old_bbox {
        ctx.damage.mark(b);
    }
    if let Some(b) = ctx.scene.node_world_bbox(id) {
        ctx.damage.mark(b);
    }
}

impl ToolBehavior for AnchorEditTool {
    fn mouse_down(&mut self, pt: Point, mods: Mods, ctx: &mut ToolCtx) {
        if !self.refresh(ctx) {
            return;
        }
        let tolerance = screen_tolerance(ctx.viewport.zoom);
        let local = self.world.inverse() * pt;
        self.hover = self.compute_hover(local, tolerance);
        match self.hover {
            AnchorHover::Handle { index, side } => {
                ctx.history.begin_transaction();
                self.drag = Some(AnchorDrag::Handle {
                    originals: self.cache.clone(),
                    index,
                    side,
                });
            }
            AnchorHover::Anchor(index) => {
                // Alt+点击 = 删除该锚点(docs/04 §3)
                if mods.alt {
                    self.delete_anchor(index, ctx);
                    return;
                }
                self.selected = Some(index);
                ctx.history.begin_transaction();
                self.drag = Some(AnchorDrag::Anchor {
                    grab: local,
                    originals: self.cache.clone(),
                    index,
                });
            }
            AnchorHover::Insert { segment, t, .. } => self.insert_anchor(segment, t, ctx),
            AnchorHover::None => {
                // 点击空白:清除选中并请求上层退回 Select
                self.selected = None;
                self.exit_request = true;
            }
        }
    }

    fn mouse_drag(&mut self, pt: Point, mods: Mods, ctx: &mut ToolCtx) {
        if !self.refresh(ctx) {
            return;
        }
        let Some(id) = self.target else {
            return;
        };
        let local = self.world.inverse() * pt;
        match &self.drag {
            Some(AnchorDrag::Anchor {
                grab,
                originals,
                index,
            }) => {
                let delta = local - *grab;
                let src = originals[*index];
                let mut next = originals.clone();
                let a = &mut next[*index];
                a.pos = src.pos + delta;
                if src.in_.is_some() || src.out.is_some() {
                    // 曲线锚点:相邻控制点随锚点整体平移(保持相对位置)
                    a.in_ = src.in_.map(|h| h + delta);
                    a.out = src.out.map(|h| h + delta);
                }
                let old_path = build_path(originals, self.closed);
                self.cache = next.clone();
                exec_anchor_points(ctx, id, old_path, &next, self.closed);
            }
            Some(AnchorDrag::Handle {
                originals,
                index,
                side,
            }) => {
                let src = originals[*index];
                let mut next = originals.clone();
                match side {
                    HandleSide::Out => next[*index].out = Some(local),
                    HandleSide::In => next[*index].in_ = Some(local),
                }
                if mods.shift {
                    // Shift = 对称联动:对侧手柄取镜像(方向反转、等长 → 平滑点)
                    let mirror = src.pos + (src.pos - local);
                    match side {
                        HandleSide::Out => next[*index].in_ = Some(mirror),
                        HandleSide::In => next[*index].out = Some(mirror),
                    }
                }
                // 无修饰/Alt = 单边(只改被拖控制点;Alt 明确保留尖点单边)
                let old_path = build_path(originals, self.closed);
                self.cache = next.clone();
                exec_anchor_points(ctx, id, old_path, &next, self.closed);
            }
            None => {}
        }
    }

    fn mouse_up(&mut self, _pt: Point, _mods: Mods, ctx: &mut ToolCtx) {
        if self.drag.take().is_some() {
            // 空事务(按下未动)由 History 自然丢弃
            ctx.history.end_transaction();
        }
    }

    fn double_click(&mut self, pt: Point, _mods: Mods, ctx: &mut ToolCtx) {
        if !self.refresh(ctx) {
            return;
        }
        let tolerance = screen_tolerance(ctx.viewport.zoom);
        let local = self.world.inverse() * pt;
        if let AnchorHover::Anchor(index) = self.compute_hover(local, tolerance) {
            self.selected = Some(index);
            self.toggle_corner_smooth(index, ctx);
        }
    }

    fn preview(&self, sink: &mut dyn PaintSink, viewport: &Viewport) {
        if self.target.is_none() || self.cache.is_empty() {
            return;
        }
        let vp = viewport.world_to_viewport();
        let z = viewport.zoom;
        self.draw_overlay_vp(vp, z, sink);
    }

    fn cursor(&self) -> CursorStyle {
        CursorStyle::Default
    }
}

// ===========================================================================
// HandTool
// ===========================================================================

/// 抓手工具:拖动 = 平移视口(docs/03 §4.2 的 Hand)。
///
/// 空格临时切换(按住空格任何工具变抓手,Illustrator 手势)由**上层**组合:
/// 上层在空格按下期间把事件路由给 HandTool 即可,本模块不引键盘状态。
#[derive(Debug, Default)]
pub struct HandTool {
    last: Option<Point>,
}

impl ToolBehavior for HandTool {
    fn mouse_down(&mut self, pt: Point, _mods: Mods, _ctx: &mut ToolCtx) {
        self.last = Some(pt);
    }

    fn mouse_drag(&mut self, pt: Point, _mods: Mods, ctx: &mut ToolCtx) {
        if let Some(last) = self.last.replace(pt) {
            // 世界位移 × zoom = 屏幕位移;内容跟手 → pan 加同样的屏幕增量
            let delta_screen: Vec2 = (pt - last) * ctx.viewport.zoom;
            ctx.viewport.pan_by(delta_screen);
        }
    }

    fn mouse_up(&mut self, _pt: Point, _mods: Mods, _ctx: &mut ToolCtx) {
        self.last = None;
    }

    fn preview(&self, _sink: &mut dyn PaintSink, _viewport: &Viewport) {}

    fn cursor(&self) -> CursorStyle {
        CursorStyle::Hand
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::BezPath;

    /// 组装 ToolCtx 的测试台:返回 (ctx 各件)。
    struct Rig {
        scene: Scene,
        history: History,
        viewport: Viewport,
        damage: DamageTracker,
    }

    impl Rig {
        fn new(zoom: f64) -> Self {
            Rig {
                scene: Scene::new(),
                history: History::new(),
                viewport: Viewport {
                    zoom,
                    pan: Vec2::ZERO,
                },
                damage: DamageTracker::default(),
            }
        }

        fn ctx(&mut self) -> ToolCtx<'_> {
            ToolCtx {
                scene: &mut self.scene,
                history: &mut self.history,
                viewport: &mut self.viewport,
                damage: &mut self.damage,
            }
        }
    }

    fn rect_node(x0: f64, y0: f64, x1: f64, y1: f64) -> NodeContent {
        NodeContent::Path(PathNode {
            path: Rect::new(x0, y0, x1, y1).to_path(0.1),
            fill: Some(Paint::Solid([200, 40, 40, 255])),
            stroke: None,
        })
    }

    fn node_transform(scene: &Scene, id: NodeId) -> Affine {
        scene.node(id).expect("节点在").transform
    }

    #[test]
    fn select_tool_drag_is_one_undo_step() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(None, "矩形", rect_node(0.0, 0.0, 10.0, 10.0))
            .expect("矩形");
        let mut tool = SelectTool::default();
        let mods = Mods::default();

        // down 命中 → 事务开始;drag 两次(每帧一个 SetTransform);up → 事务结束
        tool.mouse_down(Point::new(5.0, 5.0), mods, &mut rig.ctx());
        assert_eq!(tool.selection(), &[id]);
        tool.mouse_drag(Point::new(8.0, 7.0), mods, &mut rig.ctx());
        tool.mouse_drag(Point::new(13.0, 12.0), mods, &mut rig.ctx());
        tool.mouse_up(Point::new(13.0, 12.0), mods, &mut rig.ctx());

        assert_eq!(rig.history.undo_len(), 1, "整次拖动 = 一步撤销(事务)");
        // down 在 (5,5),拖到 (13,12):节点位移 = 光标位移 = (8,7)
        assert_eq!(
            node_transform(&rig.scene, id),
            Affine::translate((8.0, 7.0)),
            "拖动以起点为基准的绝对位移"
        );
        // 脏区:每次 drag 前后包围盒都被记账
        assert!(rig.damage.is_dirty());
        assert!(rig.damage.take().is_some());

        rig.history.undo(&mut rig.scene);
        assert_eq!(
            node_transform(&rig.scene, id),
            Affine::IDENTITY,
            "撤销一步回到原位"
        );
        rig.history.redo(&mut rig.scene);
        assert_eq!(
            node_transform(&rig.scene, id),
            Affine::translate((8.0, 7.0))
        );
    }

    #[test]
    fn select_tool_click_without_drag_creates_no_undo_step() {
        let mut rig = Rig::new(1.0);
        rig.scene
            .add_node(None, "矩形", rect_node(0.0, 0.0, 10.0, 10.0))
            .expect("矩形");
        let mut tool = SelectTool::default();
        let mods = Mods::default();

        tool.mouse_down(Point::new(5.0, 5.0), mods, &mut rig.ctx());
        tool.mouse_up(Point::new(5.0, 5.0), mods, &mut rig.ctx());
        assert_eq!(rig.history.undo_len(), 0, "空事务不产生撤销步");
        assert!(!rig.history.can_undo());
    }

    #[test]
    fn select_tool_shift_click_toggles_selection() {
        let mut rig = Rig::new(1.0);
        let a = rig
            .scene
            .add_node(None, "A", rect_node(0.0, 0.0, 10.0, 10.0))
            .expect("A");
        let b = rig
            .scene
            .add_node(None, "B", rect_node(20.0, 20.0, 30.0, 30.0))
            .expect("B");
        let mut tool = SelectTool::default();
        let plain = Mods::default();
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };

        tool.mouse_down(Point::new(5.0, 5.0), plain, &mut rig.ctx());
        tool.mouse_up(Point::new(5.0, 5.0), plain, &mut rig.ctx());
        assert_eq!(tool.selection(), &[a]);

        // Shift 点 B → 加选
        tool.mouse_down(Point::new(25.0, 25.0), shift, &mut rig.ctx());
        tool.mouse_up(Point::new(25.0, 25.0), shift, &mut rig.ctx());
        assert_eq!(tool.selection(), &[a, b]);

        // Shift 再点 B → 减选
        tool.mouse_down(Point::new(25.0, 25.0), shift, &mut rig.ctx());
        tool.mouse_up(Point::new(25.0, 25.0), shift, &mut rig.ctx());
        assert_eq!(tool.selection(), &[a]);
    }

    #[test]
    fn select_tool_blank_down_starts_rubber_band_select() {
        let mut rig = Rig::new(1.0);
        let a = rig
            .scene
            .add_node(None, "A", rect_node(0.0, 0.0, 10.0, 10.0))
            .expect("A");
        let mut tool = SelectTool::default();
        let mods = Mods::default();

        tool.mouse_down(Point::new(-5.0, -5.0), mods, &mut rig.ctx());
        tool.mouse_drag(Point::new(50.0, 50.0), mods, &mut rig.ctx());
        // 拖动中预览橡皮筋(不产生命令、不 panic)
        tool.preview(&mut NullSink, &rig.viewport);
        assert_eq!(rig.history.undo_len(), 0, "预览不进撤销栈");
        tool.mouse_up(Point::new(50.0, 50.0), mods, &mut rig.ctx());
        assert_eq!(tool.selection(), &[a], "橡皮筋套住 A → 入选");
    }

    struct NullSink;
    impl PaintSink for NullSink {
        fn fill(&mut self, _p: &Paint, _t: Affine, _path: &BezPath) {}
        fn stroke(&mut self, _s: &StrokeStyle, _t: Affine, _path: &BezPath) {}
    }

    #[test]
    fn select_tool_handle_scales_about_opposite_anchor() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(None, "矩形", rect_node(0.0, 0.0, 10.0, 10.0))
            .expect("矩形");
        let mut tool = SelectTool::default();
        let mods = Mods::default();

        // 先选中
        tool.mouse_down(Point::new(5.0, 5.0), mods, &mut rig.ctx());
        tool.mouse_up(Point::new(5.0, 5.0), mods, &mut rig.ctx());

        // SE 角手柄在世界 (10,10);拖到 (20,20) → 以 (0,0) 为锚放大 2 倍
        tool.mouse_down(Point::new(10.0, 10.0), mods, &mut rig.ctx());
        tool.mouse_drag(Point::new(20.0, 20.0), mods, &mut rig.ctx());
        tool.mouse_up(Point::new(20.0, 20.0), mods, &mut rig.ctx());

        assert_eq!(rig.history.undo_len(), 1, "缩放拖动也是一步撤销");
        let xform = node_transform(&rig.scene, id);
        // 世界变换(单位局部变换时 = 节点变换)= translate(0,0)*scale(2,2)*translate(0,0)
        assert_eq!(
            xform,
            Affine::scale(2.0),
            "SE 手柄拖拽 → 以 NW 为锚放大 2 倍"
        );

        rig.history.undo(&mut rig.scene);
        assert_eq!(node_transform(&rig.scene, id), Affine::IDENTITY);
    }

    #[test]
    fn pen_tool_corner_point_then_smooth_point() {
        let mut rig = Rig::new(1.0);
        let mut tool = PenTool::default();
        let plain = Mods::default();

        // 直角点:点击后不拖
        tool.mouse_down(Point::new(0.0, 0.0), plain, &mut rig.ctx());
        assert_eq!(tool.anchor_count(), 0, "mouse_up 前还在 pending");
        tool.mouse_up(Point::new(0.0, 0.0), plain, &mut rig.ctx());
        assert_eq!(tool.anchor_count(), 1);

        // 平滑点:点击后拖出手柄
        tool.mouse_down(Point::new(10.0, 0.0), plain, &mut rig.ctx());
        tool.mouse_drag(Point::new(12.0, 2.0), plain, &mut rig.ctx());
        tool.mouse_up(Point::new(12.0, 2.0), plain, &mut rig.ctx());
        assert_eq!(tool.anchor_count(), 2);
        assert!(tool.has_draft());
    }

    #[test]
    fn pen_tool_close_commits_add_node_command() {
        let mut rig = Rig::new(1.0);
        let len_before = rig.scene.len();
        let mut tool = PenTool::default();
        let plain = Mods::default();

        // 三个直角锚点:(0,0) (10,0) (5,8)
        for pt in [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(5.0, 8.0),
        ] {
            tool.mouse_down(pt, plain, &mut rig.ctx());
            tool.mouse_up(pt, plain, &mut rig.ctx());
        }
        // 点击起点(容差 4/zoom=4)→ 闭合提交
        tool.mouse_down(Point::new(1.0, 0.0), plain, &mut rig.ctx());

        assert_eq!(rig.scene.len(), len_before + 1, "commit 生成一个 PathNode");
        assert_eq!(rig.history.undo_len(), 1, "提交 = 一条 AddNode 命令");
        assert!(!tool.has_draft(), "提交后草稿清空");
        let node = rig
            .scene
            .node(rig.scene.iter_roots().last().expect("新节点"))
            .expect("在");
        let NodeContent::Path(path_node) = &node.content else {
            panic!("应是 Path 节点");
        };
        assert!(path_node.fill.is_none());
        assert!(path_node.stroke.is_some());

        // 撤销 → 节点消失
        rig.history.undo(&mut rig.scene);
        assert_eq!(rig.scene.len(), len_before);
    }

    #[test]
    fn pen_tool_alt_drag_makes_sharp_point() {
        let mut rig = Rig::new(1.0);
        let mut tool = PenTool::default();
        let alt = Mods {
            alt: true,
            ..Mods::default()
        };

        tool.mouse_down(Point::new(0.0, 0.0), alt, &mut rig.ctx());
        tool.mouse_drag(Point::new(5.0, 5.0), alt, &mut rig.ctx());
        tool.mouse_up(Point::new(5.0, 5.0), alt, &mut rig.ctx());
        assert_eq!(tool.anchor_count(), 1, "Alt 拖出的尖点也落锚");
        assert!(tool.has_draft());

        // 取消清草稿(Esc/右键的上层入口)
        tool.cancel_draft();
        assert!(!tool.has_draft());
        assert_eq!(tool.anchor_count(), 0);
        assert_eq!(rig.scene.len(), 0, "取消不产生任何节点/命令");
        assert_eq!(rig.history.undo_len(), 0);
    }

    #[test]
    fn hand_tool_drags_pan_viewport() {
        let mut rig = Rig::new(2.0);
        let mut tool = HandTool::default();
        let mods = Mods::default();

        tool.mouse_down(Point::new(0.0, 0.0), mods, &mut rig.ctx());
        tool.mouse_drag(Point::new(3.0, 1.0), mods, &mut rig.ctx());
        tool.mouse_drag(Point::new(5.0, -2.0), mods, &mut rig.ctx());
        tool.mouse_up(Point::new(5.0, -2.0), mods, &mut rig.ctx());

        // 世界位移累计 (5,-2) × zoom 2 = 屏幕位移 (10,-4)
        assert_eq!(rig.viewport.pan, Vec2::new(10.0, -4.0));
        assert_eq!(rig.history.undo_len(), 0, "平移视口不进撤销栈");
    }

    // =========================================================================
    // 任务 3.1:PenTool 五行为(直角/平滑/尖点/闭合/续接)+ hover 视觉反馈
    // =========================================================================

    /// 记录 fill/stroke 次数的测试 Sink(preview 视觉断言用)。
    struct RecordingSink {
        fills: usize,
        strokes: usize,
    }

    impl RecordingSink {
        fn new() -> Self {
            RecordingSink {
                fills: 0,
                strokes: 0,
            }
        }
    }

    impl PaintSink for RecordingSink {
        fn fill(&mut self, _p: &Paint, _t: Affine, _path: &BezPath) {
            self.fills += 1;
        }
        fn stroke(&mut self, _s: &StrokeStyle, _t: Affine, _path: &BezPath) {
            self.strokes += 1;
        }
    }

    /// 开放双锚线段节点(续接测试的靶节点)。
    fn two_anchor_node() -> NodeContent {
        NodeContent::Path(PathNode {
            path: build_path(
                &[
                    PenAnchor {
                        pos: Point::new(0.0, 0.0),
                        out: None,
                        in_: None,
                    },
                    PenAnchor {
                        pos: Point::new(10.0, 0.0),
                        out: None,
                        in_: None,
                    },
                ],
                false,
            ),
            fill: None,
            stroke: Some(StrokeStyle {
                paint: Paint::Solid(PEN_STROKE),
                width: 1.5,
            }),
        })
    }

    fn path_of(rig: &Rig, id: NodeId) -> BezPath {
        rig.scene.path(id).expect("路径节点在").path.clone()
    }

    #[test]
    fn pen_tool_hover_start_draws_close_hint_without_side_effects() {
        let mut rig = Rig::new(1.0);
        let mut tool = PenTool::default();
        let plain = Mods::default();
        for pt in [Point::new(0.0, 0.0), Point::new(10.0, 0.0)] {
            tool.mouse_down(pt, plain, &mut rig.ctx());
            tool.mouse_up(pt, plain, &mut rig.ctx());
        }

        tool.hover_at(Point::new(1.0, 0.0), &mut rig.ctx());
        let mut sink = RecordingSink::new();
        tool.preview(&mut sink, &rig.viewport);
        // 草稿曲线 + 橡皮筋 + 闭合高亮圈 = 3 stroke;2 锚点标记 + 闭合标记 = 3 fill
        assert_eq!(sink.strokes, 3, "闭合提示应画出高亮圈");
        assert_eq!(sink.fills, 3);
        assert_eq!(rig.history.undo_len(), 0, "preview 不产生命令");
        assert_eq!(tool.anchor_count(), 2, "preview 不改状态");
    }

    #[test]
    fn pen_tool_hover_endpoint_draws_continuation_hint_only() {
        let mut rig = Rig::new(1.0);
        rig.scene
            .add_node(None, "路径", two_anchor_node())
            .expect("路径");
        let mut tool = PenTool::default();

        // 空闲时悬停已有路径尾锚 → 续接提示圈
        tool.hover_at(Point::new(10.0, 0.0), &mut rig.ctx());
        let mut sink = RecordingSink::new();
        tool.preview(&mut sink, &rig.viewport);
        assert_eq!(sink.strokes, 1, "端点高亮圈");
        assert_eq!(sink.fills, 0);

        assert!(!tool.has_draft(), "hover 不落锚");
        assert_eq!(tool.continuing_node(), None, "hover 不进入续接");
        assert_eq!(rig.history.undo_len(), 0, "hover 不产生命令");
        assert_eq!(rig.scene.len(), 1);
    }

    #[test]
    fn pen_tool_continue_from_tail_close_appends_to_same_node() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(None, "路径", two_anchor_node())
            .expect("路径");
        let original = path_of(&rig, id);
        let mut tool = PenTool::default();
        let plain = Mods::default();

        // 点击尾锚 (10,0) → 进入续接
        tool.mouse_down(Point::new(10.0, 0.0), plain, &mut rig.ctx());
        assert_eq!(tool.continuing_node(), Some(id));
        assert_eq!(tool.anchor_count(), 2, "草稿 = 原路径锚点分解");

        // 新直角锚点 (30,5)
        tool.mouse_down(Point::new(30.0, 5.0), plain, &mut rig.ctx());
        tool.mouse_up(Point::new(30.0, 5.0), plain, &mut rig.ctx());
        assert_eq!(tool.anchor_count(), 3);

        // 点击草稿起点(原路径头 (0,0))→ 闭合并入原节点
        tool.mouse_down(Point::new(0.0, 0.0), plain, &mut rig.ctx());

        assert!(rig.scene.node(id).is_some(), "节点身份不变(非新建)");
        assert_eq!(rig.scene.len(), 1, "不新建节点");
        assert_eq!(rig.history.undo_len(), 1, "续接闭合 = 一条命令");
        assert!(!tool.has_draft());
        let sub = first_subpath(&path_of(&rig, id)).expect("单子路径");
        assert!(sub.closed, "点起点闭合");
        assert_eq!(sub.anchors.len(), 3);

        rig.history.undo(&mut rig.scene);
        assert_eq!(path_of(&rig, id), original, "撤销一步回到原路径");
        rig.history.redo(&mut rig.scene);
        assert!(first_subpath(&path_of(&rig, id)).expect("在").closed);
    }

    #[test]
    fn pen_tool_continue_from_head_open_commit_reverses_parameterization() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(None, "路径", two_anchor_node())
            .expect("路径");
        let original = path_of(&rig, id);
        let mut tool = PenTool::default();
        let plain = Mods::default();

        // 点击头锚 (0,0) → 续接(内部反转为尾追加)
        tool.mouse_down(Point::new(0.0, 0.0), plain, &mut rig.ctx());
        assert_eq!(tool.continuing_node(), Some(id));

        // 新锚点 (-10,5),Enter 式开放提交
        tool.mouse_down(Point::new(-10.0, 5.0), plain, &mut rig.ctx());
        tool.mouse_up(Point::new(-10.0, 5.0), plain, &mut rig.ctx());
        tool.commit_open(&mut rig.ctx());

        assert_eq!(rig.history.undo_len(), 1);
        assert_eq!(rig.scene.len(), 1, "并入原节点");
        let sub = first_subpath(&path_of(&rig, id)).expect("单子路径");
        assert!(!sub.closed);
        assert_eq!(sub.anchors.len(), 3);
        // 头端续接 = 反转参数化:新路径以原尾锚 (10,0) 为起点,几何等价
        assert_eq!(sub.anchors[0].pos, Point::new(10.0, 0.0));
        assert_eq!(sub.anchors[2].pos, Point::new(-10.0, 5.0));

        rig.history.undo(&mut rig.scene);
        assert_eq!(path_of(&rig, id), original);
    }

    #[test]
    fn pen_tool_cancel_during_continuation_discards_without_command() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(None, "路径", two_anchor_node())
            .expect("路径");
        let original = path_of(&rig, id);
        let mut tool = PenTool::default();
        let plain = Mods::default();

        tool.mouse_down(Point::new(10.0, 0.0), plain, &mut rig.ctx());
        assert_eq!(tool.continuing_node(), Some(id));
        tool.cancel_draft();

        assert!(!tool.has_draft());
        assert_eq!(tool.continuing_node(), None);
        assert_eq!(rig.history.undo_len(), 0, "取消不产生命令");
        assert_eq!(path_of(&rig, id), original, "目标节点原样保留");
    }

    #[test]
    fn pen_tool_commit_open_on_fresh_draft_adds_open_node() {
        let mut rig = Rig::new(1.0);
        let mut tool = PenTool::default();
        let plain = Mods::default();

        for pt in [Point::new(0.0, 0.0), Point::new(10.0, 0.0)] {
            tool.mouse_down(pt, plain, &mut rig.ctx());
            tool.mouse_up(pt, plain, &mut rig.ctx());
        }
        tool.commit_open(&mut rig.ctx());

        assert_eq!(rig.scene.len(), 1);
        assert_eq!(rig.history.undo_len(), 1);
        let sub = first_subpath(&path_of(
            &rig,
            rig.scene.iter_roots().last().expect("新节点"),
        ))
        .expect("单子路径");
        assert!(!sub.closed);
        assert_eq!(sub.anchors.len(), 2);
        rig.history.undo(&mut rig.scene);
        assert_eq!(rig.scene.len(), 0);
    }

    // =========================================================================
    // 任务 3.2:锚点编辑(SubPathEdit)
    // =========================================================================

    /// 三锚曲线节点:中锚带双向手柄(平滑点)。
    fn curve_node_content() -> NodeContent {
        NodeContent::Path(PathNode {
            path: build_path(
                &[
                    PenAnchor {
                        pos: Point::new(0.0, 0.0),
                        out: None,
                        in_: None,
                    },
                    PenAnchor {
                        pos: Point::new(10.0, 0.0),
                        out: Some(Point::new(12.0, -3.0)),
                        in_: Some(Point::new(8.0, 3.0)),
                    },
                    PenAnchor {
                        pos: Point::new(20.0, 10.0),
                        out: None,
                        in_: None,
                    },
                ],
                false,
            ),
            fill: None,
            stroke: Some(StrokeStyle {
                paint: Paint::Solid(PEN_STROKE),
                width: 1.5,
            }),
        })
    }

    #[test]
    fn select_tool_double_click_path_requests_sub_edit() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(None, "矩形", rect_node(0.0, 0.0, 10.0, 10.0))
            .expect("矩形");
        let mut tool = SelectTool::default();
        let plain = Mods::default();

        tool.double_click(Point::new(5.0, 5.0), plain, &mut rig.ctx());
        assert_eq!(tool.selection(), &[id], "双击同时选中");
        assert_eq!(tool.take_sub_edit_request(), Some(id));
        assert_eq!(tool.take_sub_edit_request(), None, "一次一取");

        // 空白双击不产生请求
        tool.double_click(Point::new(50.0, 50.0), plain, &mut rig.ctx());
        assert_eq!(tool.take_sub_edit_request(), None);
    }

    #[test]
    fn anchor_edit_preview_shows_anchors_selection_and_hover() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(None, "曲线", curve_node_content())
            .expect("曲线");
        let mut tool = AnchorEditTool::new(id);
        let plain = Mods::default();

        // 未选中:轮廓 1 stroke + 3 锚点方块
        let mut sink = RecordingSink::new();
        let node_path = rig.scene.path(id).expect("路径在").path.clone();
        let world = rig.scene.world_transform(id).expect("world");
        tool.preview_scene(&node_path, world, &mut sink, &rig.viewport);
        assert_eq!((sink.strokes, sink.fills), (1, 3));
        assert_eq!(rig.history.undo_len(), 0, "preview 不产生命令");

        // 选中中锚:选中高亮 + 双手柄杆与手柄圆
        tool.mouse_down(Point::new(10.0, 0.0), plain, &mut rig.ctx());
        tool.mouse_up(Point::new(10.0, 0.0), plain, &mut rig.ctx());
        assert_eq!(tool.selected(), Some(1));
        let mut sink = RecordingSink::new();
        let node_path = rig.scene.path(id).expect("路径在").path.clone();
        let world = rig.scene.world_transform(id).expect("world");
        tool.preview_scene(&node_path, world, &mut sink, &rig.viewport);
        assert_eq!(
            (sink.strokes, sink.fills),
            (5, 4),
            "轮廓+2杆+2圈 / 3锚+选中块"
        );

        // 悬停锚点 0:多一个高亮圈
        tool.hover_at(Point::new(0.0, 0.0), &mut rig.ctx());
        let mut sink = RecordingSink::new();
        let node_path = rig.scene.path(id).expect("路径在").path.clone();
        let world = rig.scene.world_transform(id).expect("world");
        tool.preview_scene(&node_path, world, &mut sink, &rig.viewport);
        assert_eq!(sink.strokes, 6);
    }

    #[test]
    fn anchor_edit_drag_anchor_is_one_undo_step_and_moves_handles() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(None, "曲线", curve_node_content())
            .expect("曲线");
        let original = path_of(&rig, id);
        let mut tool = AnchorEditTool::new(id);
        let plain = Mods::default();

        tool.mouse_down(Point::new(10.0, 0.0), plain, &mut rig.ctx());
        tool.mouse_drag(Point::new(13.0, 2.0), plain, &mut rig.ctx());
        tool.mouse_up(Point::new(13.0, 2.0), plain, &mut rig.ctx());

        assert_eq!(rig.history.undo_len(), 1, "拖动 = 一步撤销(事务)");
        let sub = first_subpath(&path_of(&rig, id)).expect("单子路径");
        assert_eq!(sub.anchors[1].pos, Point::new(13.0, 2.0));
        assert_eq!(
            sub.anchors[1].in_handle,
            Some(Point::new(11.0, 5.0)),
            "曲线锚点的控制点随锚点整体平移(保持相对位置)"
        );
        assert_eq!(sub.anchors[1].out_handle, Some(Point::new(15.0, -1.0)));
        assert_eq!(sub.anchors[0].pos, Point::new(0.0, 0.0), "其余锚点不动");

        rig.history.undo(&mut rig.scene);
        assert_eq!(path_of(&rig, id), original, "撤销回原路径");
        rig.history.redo(&mut rig.scene);
        assert_eq!(
            first_subpath(&path_of(&rig, id)).expect("在").anchors[1].pos,
            Point::new(13.0, 2.0)
        );
    }

    #[test]
    fn anchor_edit_drag_handle_default_moves_single_side() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(None, "曲线", curve_node_content())
            .expect("曲线");
        let original = path_of(&rig, id);
        let mut tool = AnchorEditTool::new(id);
        let plain = Mods::default();

        // 先选中锚点 1(空点击事务被自然丢弃)
        tool.mouse_down(Point::new(10.0, 0.0), plain, &mut rig.ctx());
        tool.mouse_up(Point::new(10.0, 0.0), plain, &mut rig.ctx());
        // 抓出手手柄(点 (10,0) 本身也在锚点容差内,手柄优先命中)
        tool.mouse_down(Point::new(12.0, -3.0), plain, &mut rig.ctx());
        tool.mouse_drag(Point::new(16.0, -6.0), plain, &mut rig.ctx());
        tool.mouse_up(Point::new(16.0, -6.0), plain, &mut rig.ctx());

        assert_eq!(rig.history.undo_len(), 1);
        let sub = first_subpath(&path_of(&rig, id)).expect("单子路径");
        assert_eq!(sub.anchors[1].out_handle, Some(Point::new(16.0, -6.0)));
        assert_eq!(
            sub.anchors[1].in_handle,
            Some(Point::new(8.0, 3.0)),
            "默认/Alt = 单边:对侧手柄不动(保留尖点)"
        );
        assert_eq!(sub.anchors[1].pos, Point::new(10.0, 0.0), "锚点本体不动");

        rig.history.undo(&mut rig.scene);
        assert_eq!(path_of(&rig, id), original);
    }

    #[test]
    fn anchor_edit_drag_handle_shift_mirrors_opposite() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(None, "曲线", curve_node_content())
            .expect("曲线");
        let mut tool = AnchorEditTool::new(id);
        let plain = Mods::default();
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };

        tool.mouse_down(Point::new(10.0, 0.0), plain, &mut rig.ctx());
        tool.mouse_up(Point::new(10.0, 0.0), plain, &mut rig.ctx());
        tool.mouse_down(Point::new(12.0, -3.0), shift, &mut rig.ctx());
        tool.mouse_drag(Point::new(16.0, -6.0), shift, &mut rig.ctx());
        tool.mouse_up(Point::new(16.0, -6.0), shift, &mut rig.ctx());

        let sub = first_subpath(&path_of(&rig, id)).expect("单子路径");
        assert_eq!(sub.anchors[1].out_handle, Some(Point::new(16.0, -6.0)));
        assert_eq!(
            sub.anchors[1].in_handle,
            Some(Point::new(4.0, 6.0)),
            "Shift = 对称联动:对侧手柄取锚点镜像(方向反转、等长)"
        );
    }

    #[test]
    fn anchor_edit_double_click_toggles_corner_then_smooth() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(None, "曲线", curve_node_content())
            .expect("曲线");
        let original = path_of(&rig, id);
        let mut tool = AnchorEditTool::new(id);
        let plain = Mods::default();

        // 平滑点 → 双击 → 直角(手柄全移除)
        tool.double_click(Point::new(10.0, 0.0), plain, &mut rig.ctx());
        let sub = first_subpath(&path_of(&rig, id)).expect("单子路径");
        assert_eq!(sub.anchors[1].in_handle, None);
        assert_eq!(sub.anchors[1].out_handle, None);
        assert_eq!(rig.history.undo_len(), 1);

        // 直角 → 双击 → 平滑(邻段方向自动配一对共线手柄)
        tool.double_click(Point::new(10.0, 0.0), plain, &mut rig.ctx());
        let sub = first_subpath(&path_of(&rig, id)).expect("单子路径");
        let pos = sub.anchors[1].pos;
        let out = sub.anchors[1].out_handle.expect("出手柄");
        let in_ = sub.anchors[1].in_handle.expect("入手柄");
        let cross = (out - pos).cross(pos - in_);
        assert!(cross.abs() < 1e-6, "自动手柄应共线,叉积 {cross}");
        assert!(
            (out - pos).dot(pos - in_) > 0.0,
            "出手柄与入手柄分居锚点两侧"
        );
        assert_eq!(rig.history.undo_len(), 2);

        rig.history.undo(&mut rig.scene);
        rig.history.undo(&mut rig.scene);
        assert_eq!(path_of(&rig, id), original, "两步撤销回原路径");
    }

    #[test]
    fn anchor_edit_alt_click_deletes_anchor_merging_segments() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(None, "曲线", curve_node_content())
            .expect("曲线");
        let original = path_of(&rig, id);
        let mut tool = AnchorEditTool::new(id);
        let alt = Mods {
            alt: true,
            ..Mods::default()
        };

        tool.mouse_down(Point::new(10.0, 0.0), alt, &mut rig.ctx());
        let sub = first_subpath(&path_of(&rig, id)).expect("单子路径");
        assert_eq!(sub.anchors.len(), 2, "中锚被删除,相邻两段合并为一段");
        assert_eq!(sub.anchors[0].pos, Point::new(0.0, 0.0));
        assert_eq!(sub.anchors[1].pos, Point::new(20.0, 10.0));
        assert_eq!(tool.selected(), None);
        assert_eq!(rig.history.undo_len(), 1);

        rig.history.undo(&mut rig.scene);
        assert_eq!(path_of(&rig, id), original);
    }

    #[test]
    fn anchor_edit_delete_first_anchor_hands_head_to_next() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(None, "曲线", curve_node_content())
            .expect("曲线");
        let original = path_of(&rig, id);
        let mut tool = AnchorEditTool::new(id);
        let plain = Mods::default();

        tool.mouse_down(Point::new(0.0, 0.0), plain, &mut rig.ctx());
        tool.mouse_up(Point::new(0.0, 0.0), plain, &mut rig.ctx());
        tool.delete_selected(&mut rig.ctx()); // Delete 键入口(上层转发)

        let sub = first_subpath(&path_of(&rig, id)).expect("单子路径");
        assert_eq!(sub.anchors.len(), 2);
        assert_eq!(
            sub.anchors[0].pos,
            Point::new(10.0, 0.0),
            "首锚删除 = 次锚接管头部,路径保持连续"
        );
        assert_eq!(rig.history.undo_len(), 1);

        rig.history.undo(&mut rig.scene);
        assert_eq!(path_of(&rig, id), original);
    }

    #[test]
    fn anchor_edit_insert_anchor_on_line_segment_with_hover_hint() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(None, "线段", two_anchor_node())
            .expect("线段");
        let original = path_of(&rig, id);
        let mut tool = AnchorEditTool::new(id);
        let plain = Mods::default();

        // 悬停线段中点上方 1px → AddAnchor 吸附提示(只画不执行)
        tool.hover_at(Point::new(5.0, 1.0), &mut rig.ctx());
        let mut sink = RecordingSink::new();
        let node_path = rig.scene.path(id).expect("路径在").path.clone();
        let world = rig.scene.world_transform(id).expect("world");
        tool.preview_scene(&node_path, world, &mut sink, &rig.viewport);
        assert_eq!(
            (sink.strokes, sink.fills),
            (2, 3),
            "轮廓+提示圈 / 2锚+提示点"
        );
        assert_eq!(rig.history.undo_len(), 0, "hover 不产生命令");

        // 点击插入:线段一分为二
        tool.mouse_down(Point::new(5.0, 1.0), plain, &mut rig.ctx());
        let sub = first_subpath(&path_of(&rig, id)).expect("单子路径");
        assert_eq!(sub.anchors.len(), 3);
        assert!((sub.anchors[1].pos - Point::new(5.0, 0.0)).hypot() < 0.05);
        assert_eq!(sub.anchors[1].in_handle, None, "直线段分割不带手柄");
        assert_eq!(tool.selected(), Some(1), "插入后新锚点即为选中");
        assert_eq!(rig.history.undo_len(), 1);

        rig.history.undo(&mut rig.scene);
        assert_eq!(path_of(&rig, id), original);
    }

    #[test]
    fn anchor_edit_insert_anchor_on_cubic_segment_keeps_geometry() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(None, "曲线", two_anchor_node())
            .expect("曲线");
        // 换成带手柄的 S 曲线:(0,0) → out(3,6) → in(7,-6) → (10,0)
        let cubic = build_path(
            &[
                PenAnchor {
                    pos: Point::new(0.0, 0.0),
                    out: Some(Point::new(3.0, 6.0)),
                    in_: None,
                },
                PenAnchor {
                    pos: Point::new(10.0, 0.0),
                    out: None,
                    in_: Some(Point::new(7.0, -6.0)),
                },
            ],
            false,
        );
        rig.scene.path_mut(id).expect("在").path = cubic;
        let mut tool = AnchorEditTool::new(id);
        let plain = Mods::default();

        // t=0.5 处曲线点恰为 (5,0)(对称 S)
        tool.mouse_down(Point::new(5.0, 0.5), plain, &mut rig.ctx());
        let sub = first_subpath(&path_of(&rig, id)).expect("单子路径");
        assert_eq!(sub.anchors.len(), 3);
        assert!(
            (sub.anchors[1].pos - Point::new(5.0, 0.0)).hypot() < 0.5,
            "插入点 = de Casteljau 分割点(容差内;nearest 的 t 有 flatten 精度量级的误差);实际 {:?}",
            sub.anchors[1].pos
        );
        assert!(
            sub.anchors[1].in_handle.is_some() && sub.anchors[1].out_handle.is_some(),
            "曲线分割后新锚点两侧手柄齐备"
        );
        assert_eq!(rig.history.undo_len(), 1);
        rig.history.undo(&mut rig.scene);
        assert_eq!(rig.history.undo_len(), 0);
    }

    #[test]
    fn anchor_edit_insert_on_closing_segment_of_closed_path() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(
                None,
                "三角形",
                NodeContent::Path(PathNode {
                    path: build_path(
                        &[
                            PenAnchor {
                                pos: Point::new(0.0, 0.0),
                                out: None,
                                in_: None,
                            },
                            PenAnchor {
                                pos: Point::new(10.0, 0.0),
                                out: None,
                                in_: None,
                            },
                            PenAnchor {
                                pos: Point::new(5.0, 8.0),
                                out: None,
                                in_: None,
                            },
                        ],
                        true,
                    ),
                    fill: None,
                    stroke: Some(StrokeStyle {
                        paint: Paint::Solid(PEN_STROKE),
                        width: 1.5,
                    }),
                }),
            )
            .expect("三角形");
        let mut tool = AnchorEditTool::new(id);
        let plain = Mods::default();

        // 悬停闭合段(末锚 (5,8) → 首锚 (0,0))中点 → 插入
        tool.mouse_down(Point::new(2.5, 4.0), plain, &mut rig.ctx());
        let sub = first_subpath(&path_of(&rig, id)).expect("单子路径");
        assert!(sub.closed, "闭合属性保留");
        assert_eq!(sub.anchors.len(), 4);
        assert_eq!(
            sub.anchors[3].pos,
            Point::new(2.5, 4.0),
            "新锚点追加到序列尾"
        );
        assert_eq!(tool.selected(), Some(3));
    }

    #[test]
    fn anchor_edit_delete_guards_keep_minimum_anchor_count() {
        let mut rig = Rig::new(1.0);
        let tri = rig
            .scene
            .add_node(
                None,
                "三角形",
                NodeContent::Path(PathNode {
                    path: build_path(
                        &[
                            PenAnchor {
                                pos: Point::new(0.0, 0.0),
                                out: None,
                                in_: None,
                            },
                            PenAnchor {
                                pos: Point::new(10.0, 0.0),
                                out: None,
                                in_: None,
                            },
                            PenAnchor {
                                pos: Point::new(5.0, 8.0),
                                out: None,
                                in_: None,
                            },
                        ],
                        true,
                    ),
                    fill: None,
                    stroke: Some(StrokeStyle {
                        paint: Paint::Solid(PEN_STROKE),
                        width: 1.5,
                    }),
                }),
            )
            .expect("三角形");
        let line = rig
            .scene
            .add_node(None, "线段", two_anchor_node())
            .expect("线段");
        let alt = Mods {
            alt: true,
            ..Mods::default()
        };

        // 闭路径下限 3:三角形删任一锚 → 不响应
        let mut tool = AnchorEditTool::new(tri);
        tool.mouse_down(Point::new(0.0, 0.0), alt, &mut rig.ctx());
        assert_eq!(
            first_subpath(&path_of(&rig, tri))
                .expect("在")
                .anchors
                .len(),
            3
        );
        assert_eq!(rig.history.undo_len(), 0);

        // 开路径下限 2:两锚线段删除 → 不响应
        let mut tool = AnchorEditTool::new(line);
        tool.mouse_down(Point::new(0.0, 0.0), alt, &mut rig.ctx());
        assert_eq!(
            first_subpath(&path_of(&rig, line))
                .expect("在")
                .anchors
                .len(),
            2
        );
        assert_eq!(rig.history.undo_len(), 0);
    }

    #[test]
    fn anchor_edit_blank_click_and_escape_request_exit() {
        let mut rig = Rig::new(1.0);
        let id = rig
            .scene
            .add_node(None, "曲线", curve_node_content())
            .expect("曲线");
        let mut tool = AnchorEditTool::new(id);
        let plain = Mods::default();

        assert!(!tool.exit_requested());
        // 点击空白 → 请求退回 Select
        tool.mouse_down(Point::new(100.0, 100.0), plain, &mut rig.ctx());
        assert!(tool.exit_requested());
        assert_eq!(tool.selected(), None);
        assert_eq!(rig.history.undo_len(), 0);

        // Esc(上层转发)→ 同样请求退出
        let mut tool = AnchorEditTool::new(id);
        tool.escape();
        assert!(tool.exit_requested());
        assert_eq!(tool.target(), Some(id));
    }
}
