//! 工具状态机(源自 docs/03 §4.2 工具模型 + docs/04 §3 钢笔工具)。
//!
//! **framework-free**:本模块不引 gpui。上层(gpui_element / 示例)负责
//! 把框架事件换算成"世界坐标点 + 修饰键"后分发到 [`ToolBehavior`],工具
//! 内部对文档的一切修改都经 [`ToolCtx`] 里的 [`History`] 走 Command
//! (AGENTS.md 铁律:一切文档修改必须走 Command)。
//!
//! # 拖动 = 一步撤销(docs/03 §2 事务)
//!
//! SelectTool 拖动移动/缩放在 `mouse_down` 时 `begin_transaction`,拖动过程
//! 每次 exec [`SetTransform`],`mouse_up` 时 `end_transaction` → 整次拖动
//! 撤销为一步。
//!
//! # 钢笔状态机(docs/04 §3)
//!
//! ```text
//! Idle ──点击──▶ 锚点(直角);持续拖动 ──▶ 拖出双向手柄(平滑点)
//!   Alt拖手柄 ──▶ 单边手柄(尖点)
//!   点击起点 ──▶ 闭合路径 → History::exec(AddNode)
//!   Esc/右键 ──▶ 取消(上层调用 PenTool::cancel_draft)
//! ```

use kurbo::{Affine, BezPath, Point, Rect, Shape, Vec2};
use lumina_core::command::{AddNode, Command, History, SetTransform};
use lumina_core::scene::{NodeContent, NodeId, Paint, PathNode, Rgba8, Scene, StrokeStyle};
use lumina_core::viewport::Viewport;
use lumina_paint::sink::PaintSink;

use crate::damage::DamageTracker;
use crate::hit_test::SceneHitTest;
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

/// 正在放置中的锚点(等 mouse_up 决定直角/平滑/尖角)。
#[derive(Clone, Copy, Debug)]
struct PendingAnchor {
    pos: Point,
    handle: Option<Point>,
    sharp: bool,
}

/// 钢笔工具(docs/04 §3 状态机):点击落锚点,拖动出手柄,点起点闭合提交。
#[derive(Debug, Default)]
pub struct PenTool {
    anchors: Vec<PenAnchor>,
    pending: Option<PendingAnchor>,
    /// 最后已知鼠标位置(橡皮筋预览用;拖动间隙也会保留上一次的值)
    cursor_pt: Option<Point>,
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

    /// 取消草稿(Esc/右键;上层调用)。
    pub fn cancel_draft(&mut self) {
        self.anchors.clear();
        self.pending = None;
        self.cursor_pt = None;
    }

    /// 点击是否落在路径起点(容差内)→ 请求闭合。
    fn clicked_start(&self, pt: Point, tolerance: f64) -> bool {
        self.anchors.len() >= 2
            && self
                .anchors
                .first()
                .is_some_and(|first| (pt - first.pos).hypot() <= tolerance)
    }

    /// 闭合提交:草稿 → AddNode 命令 → 撤销栈(docs/04 §3 `commit`)。
    fn commit_closed(&mut self, ctx: &mut ToolCtx) {
        let path = build_path(&self.anchors, true);
        let bbox = path.bounding_box();
        let node = lumina_core::scene::Node::new(
            "钢笔路径",
            NodeContent::Path(PathNode {
                path,
                fill: None,
                stroke: Some(StrokeStyle {
                    paint: Paint::Solid(PEN_STROKE),
                    width: 1.5,
                }),
            }),
        );
        ctx.history
            .exec(AddNode::new(None, None, node).boxed(), ctx.scene);
        ctx.damage.mark(bbox);
        self.cancel_draft();
    }
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
        let tolerance = screen_tolerance(ctx.viewport.zoom);
        if self.clicked_start(pt, tolerance) {
            // 点击起点 → 闭合 → commit 生成 AddNode 命令(docs/04 §3)
            self.commit_closed(ctx);
            return;
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
        if !self.has_draft() {
            return;
        }
        let vp = viewport.world_to_viewport();
        let z = viewport.zoom;
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
}
