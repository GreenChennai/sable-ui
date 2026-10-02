//! 命中测试:点选与框选(源自 docs/02 §5;extension trait 模式,`Scene` 是
//! sable-foundation 的外部类型,按孤儿规则在本 crate 定义 trait 再 impl)。
//!
//! # 对 docs/02 §5 的两处实现修正
//!
//! 1. 原文 `distance_to_path` 只算 flatten 后**端点**距离(注释也自认简化),
//!    按任务契约修正为**逐线段的点到线段精确距离**;
//! 2. 描边命中阈值为 `线宽/2 + tolerance`(原文漏了线宽的一半)。
//!
//! 顶→底逆序遍历保证"最上面的对象优先命中";Group 自身不响应命中,但子节点
//! 可以穿透命中(选择工具点到组内对象选中的是对象本身)。
//!
//! # 锚点几何辅助(docs/04 §3 钢笔续接 + 锚点编辑共用)
//!
//! [`first_subpath`] 把单子路径 `BezPath` 分解为锚点序列(直角/带手柄),
//! [`hit_anchor`] 做点到锚点的容差命中,[`nearest_on_subpath`] 用 kurbo
//! `ParamCurveNearest` 求路径最近点(AddAnchor 吸附提示)。三者为纯几何
//! 函数,坐标一律节点局部系,世界换算由调用方(工具层)负责。

use kurbo::{
    BezPath, CubicBez, Line, ParamCurve, ParamCurveNearest, PathEl, PathSeg, Point, QuadBez, Rect,
    Shape,
};
use sable_foundation::scene::{NodeContent, NodeId, Scene};

use crate::text::node_world_bbox_measured;

/// `Scene` 的命中测试能力(foreign trait 模式:`use sable_canvas::hit_test::SceneHitTest;` 后,
/// `scene.hit_test(..)` / `scene.select_in_rect(..)` 直接可用)。
pub trait SceneHitTest {
    /// 点选:从顶层向下找第一个命中的节点。
    ///
    /// `tolerance` 为**世界坐标**下的点击容差(屏幕 4px / zoom 得到,
    /// 见 [`crate::input::screen_tolerance`])。跳过 locked 节点;
    /// invisible 子树已在 render_list 中剪枝。
    fn hit_test(&self, world_pt: Point, tolerance: f64) -> Option<NodeId>;

    /// 框选:返回世界包围盒与 `rect` 相交的全部节点(底→顶序,**含 Group**)。
    /// 与点选一致地跳过 locked 节点。
    fn select_in_rect(&self, rect: Rect) -> Vec<NodeId>;
}

impl SceneHitTest for Scene {
    fn hit_test(&self, world_pt: Point, tolerance: f64) -> Option<NodeId> {
        // render_list 是底→顶,点选要反过来从顶→底(docs/02 §5)
        for (id, xform) in self.render_list().into_iter().rev() {
            let Some(node) = self.node(id) else {
                continue;
            };
            if node.locked {
                continue;
            }
            let inv = xform.inverse();
            let local = inv * world_pt; // 转到节点局部坐标

            let hit = match &node.content {
                NodeContent::Path(p) => {
                    // 1. 填充命中:点在路径内(仅有填充时)
                    let in_fill = p.fill.is_some() && p.path.contains(local);
                    // 2. 描边命中:点到路径距离 <= 线宽/2 + 容差。
                    //    tolerance 是世界口径(screen_tolerance = 4px/zoom),
                    //    distance_to_path 在节点局部坐标比较——节点含缩放 k 时
                    //    局部容差应除以 k,否则放大节点命中域偏大、缩小偏小
                    //    (V4.0 T7,review R8)。非均匀缩放取面积等效 |det|^½
                    //    (与 SVG 径向渐变导入同一近似纪律)。
                    let near_stroke = p.stroke.as_ref().is_some_and(|s| {
                        let c = xform.as_coeffs();
                        let k = (c[0] * c[3] - c[1] * c[2]).abs().sqrt();
                        let local_tol = if k > f64::EPSILON {
                            tolerance / k
                        } else {
                            tolerance
                        };
                        distance_to_path(&p.path, local) <= s.width / 2.0 + local_tol
                    });
                    in_fill || near_stroke
                }
                // 世界包围盒外扩容差(docs/02 §5 的 bbox inflate;世界 bbox 已含
                // 节点变换)。V4.0 T6.1:Text 用实测布局尺寸的包围盒——0.6em/
                // 字符粗估对 CJK(实际 ≈1.0em/字)系统性偏窄 ~40%,右半边点
                // 不中(review R6);Image 本就是精确数据矩形,维持原路。
                NodeContent::Text(_) => node_world_bbox_measured(self, id).is_some_and(|bbox| {
                    let grown = bbox.inflate(tolerance, tolerance);
                    grown.x0 <= world_pt.x
                        && world_pt.x <= grown.x1
                        && grown.y0 <= world_pt.y
                        && world_pt.y <= grown.y1
                }),
                NodeContent::Image(_) => self.node_world_bbox(id).is_some_and(|bbox| {
                    let grown = bbox.inflate(tolerance, tolerance);
                    grown.x0 <= world_pt.x
                        && world_pt.x <= grown.x1
                        && grown.y0 <= world_pt.y
                        && world_pt.y <= grown.y1
                }),
                // 组本身不响应,靠子节点(子节点在逆序中先于组被测试)
                NodeContent::Group => false,
            };
            if hit {
                return Some(id);
            }
        }
        None
    }

    fn select_in_rect(&self, rect: Rect) -> Vec<NodeId> {
        self.render_list()
            .into_iter()
            .filter(|(id, _)| self.node(*id).is_some_and(|node| !node.locked))
            .filter(|(id, _)| {
                // Text 用实测包围盒(T6.1),其余与 foundation 口径一致
                node_world_bbox_measured(self, *id).is_some_and(|bbox| {
                    bbox.x0 < rect.x1 && rect.x0 < bbox.x1 && bbox.y0 < rect.y1 && rect.y0 < bbox.y1
                })
            })
            .map(|(id, _)| id)
            .collect()
    }
}

/// 点到路径的最短距离:flatten 后逐**线段**计算点到线段距离
/// (docs/02 §5 原文只算端点距离;此处为契约要求的精确版本)。
///
/// `tolerance` 为 flatten 精度(世界单位);命中测试调用方传 0.25 量级即可。
pub fn distance_to_path(path: &BezPath, pt: Point) -> f64 {
    let mut min = f64::MAX;
    // prev = 当前子路径里的上一个点;start = 当前子路径起点(ClosePath 闭合用)
    let mut prev: Option<Point> = None;
    let mut start: Option<Point> = None;
    kurbo::flatten(path.elements().iter().copied(), 0.25, |el| {
        match el {
            PathEl::MoveTo(p) => {
                prev = Some(p);
                start = Some(p);
            }
            PathEl::LineTo(p) => {
                if let Some(a) = prev {
                    min = min.min(point_segment_distance(pt, a, p));
                }
                prev = Some(p);
            }
            // flatten 后不应再出现曲线;兜底只算到终点,保持单调不 panic
            PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => {
                if let Some(a) = prev {
                    min = min.min(point_segment_distance(pt, a, p));
                }
                prev = Some(p);
            }
            PathEl::ClosePath => {
                if let (Some(a), Some(s)) = (prev, start) {
                    min = min.min(point_segment_distance(pt, a, s));
                }
                prev = start;
            }
        }
    });
    min
}

/// 点到线段 ab 的最短距离(垂足投影钳制到线段内)。
fn point_segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let ab = b - a;
    let len2 = ab.hypot2();
    if len2 <= f64::EPSILON {
        return (p - a).hypot();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    let closest = a + ab * t;
    (p - closest).hypot()
}

// ===========================================================================
// 锚点几何辅助(钢笔续接 / 锚点编辑;docs/04 §3)
// ===========================================================================

/// 锚点(节点局部坐标;手柄 = 贝塞尔控制点)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorPoint {
    /// 锚点位置。
    pub pos: Point,
    /// 入手柄(进入该锚点方向的控制点)。
    pub in_handle: Option<Point>,
    /// 出手柄(离开该锚点方向的控制点)。
    pub out_handle: Option<Point>,
}

/// 单个子路径的锚点分解。
///
/// 闭合路径的闭合段按**末锚→首锚的直线**建模(与 tool 层 `build_path` 的
/// `close_path()` 同构;曲率闭合段在 v1.0 锚点模型里不可表达,见该模块注记)。
#[derive(Clone, Debug, PartialEq)]
pub struct SubPath {
    /// 是否以 `ClosePath` 收尾。
    pub closed: bool,
    /// 锚点序列(至少 1 个)。
    pub anchors: Vec<AnchorPoint>,
}

/// 分解 `BezPath` 的首个(也是唯一)子路径为锚点序列。
///
/// 空路径、无 `MoveTo` 的病态路径、**多子路径**都返回 `None`(v1.0 限制:
/// 锚点编辑/续接只面向单子路径节点)。`QuadTo` 经 `QuadBez::raise` 精确升阶
/// 为三次贝塞尔后并入(几何等价,升阶无误差)。
pub fn first_subpath(path: &BezPath) -> Option<SubPath> {
    let move_count = path
        .iter()
        .filter(|el| matches!(el, PathEl::MoveTo(_)))
        .count();
    if move_count != 1 {
        return None;
    }
    let mut anchors: Vec<AnchorPoint> = Vec::new();
    let mut closed = false;
    for el in path.iter() {
        match el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => {
                anchors.push(AnchorPoint {
                    pos: p,
                    in_handle: None,
                    out_handle: None,
                });
            }
            PathEl::QuadTo(c, p) => {
                let start = anchors.last()?.pos;
                push_cubic(&mut anchors, QuadBez::new(start, c, p).raise());
            }
            PathEl::CurveTo(c1, c2, p) => {
                let start = anchors.last()?.pos;
                push_cubic(&mut anchors, CubicBez::new(start, c1, c2, p));
            }
            PathEl::ClosePath => closed = true,
        }
    }
    if anchors.is_empty() {
        None
    } else {
        Some(SubPath { closed, anchors })
    }
}

/// 把一段三次贝塞尔并入锚点序列:前锚补出手柄,新锚点带入入手柄。
fn push_cubic(anchors: &mut Vec<AnchorPoint>, c: CubicBez) {
    if let Some(prev) = anchors.last_mut() {
        // 控制点与端点重合 = "缺手柄以锚点补位"的退化写法(build_path 约定),非真手柄
        if prev.out_handle.is_none() && c.p1 != prev.pos {
            prev.out_handle = Some(c.p1);
        }
    }
    let in_handle = (c.p2 != c.p3).then_some(c.p2);
    anchors.push(AnchorPoint {
        pos: c.p3,
        in_handle,
        out_handle: None,
    });
}

/// 点到锚点的容差命中:返回**首个**命中的锚点下标(点到点距离,世界/局部
/// 坐标口径由调用方统一)。
pub fn hit_anchor(anchors: &[AnchorPoint], pt: Point, tolerance: f64) -> Option<usize> {
    anchors
        .iter()
        .position(|a| (a.pos - pt).hypot() <= tolerance)
}

/// 锚点模型相邻两锚点之间的段(与 tool 层 `build_path` 同构:
/// 双侧无手柄 = 直线,否则三次贝塞尔、缺侧手柄以锚点自身补位)。
pub fn anchor_segment(a: &AnchorPoint, b: &AnchorPoint) -> PathSeg {
    match (a.out_handle, b.in_handle) {
        (None, None) => PathSeg::Line(Line::new(a.pos, b.pos)),
        (out, in_) => PathSeg::Cubic(CubicBez::new(
            a.pos,
            out.unwrap_or(a.pos),
            in_.unwrap_or(b.pos),
            b.pos,
        )),
    }
}

/// [`nearest_on_subpath`] 的查询结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathNearest {
    /// 段下标:锚点序列 `windows(2)` 的顺序;闭合路径最后一段 = 末锚→首锚
    /// (下标 `anchors.len() - 1`)。
    pub segment: usize,
    /// 段内参数(0..1)。
    pub t: f64,
    /// 路径上的最近点。
    pub point: Point,
    /// 到最近点的距离。
    pub distance: f64,
}

/// 路径上离 `pt` 最近的点(kurbo `ParamCurveNearest`,内部逐段求解;
/// `accuracy` 为 flatten 精度,与 [`distance_to_path`] 的 0.25 量级同口径)。
pub fn nearest_on_subpath(sub: &SubPath, pt: Point, accuracy: f64) -> Option<PathNearest> {
    let mut best: Option<PathNearest> = None;
    let consider = |segment: usize, seg: PathSeg, best: &mut Option<PathNearest>| {
        let n = seg.nearest(pt, accuracy);
        let distance = n.distance_sq.sqrt();
        let better = match best {
            Some(b) => distance < b.distance,
            None => true,
        };
        if better {
            *best = Some(PathNearest {
                segment,
                t: n.t,
                point: seg.eval(n.t),
                distance,
            });
        }
    };
    for (i, pair) in sub.anchors.windows(2).enumerate() {
        consider(i, anchor_segment(&pair[0], &pair[1]), &mut best);
    }
    if sub.closed {
        let n = sub.anchors.len();
        if n >= 2 {
            consider(
                n - 1,
                anchor_segment(&sub.anchors[n - 1], &sub.anchors[0]),
                &mut best,
            );
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use sable_foundation::scene::Paint;
    use sable_foundation::scene::{NodeContent, PathNode, StrokeStyle};

    fn rect_scene() -> (Scene, NodeId) {
        let mut scene = Scene::new();
        let id = scene
            .add_node(
                None,
                "矩形",
                NodeContent::Path(PathNode {
                    path: kurbo::Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1),
                    fill: Some(Paint::Solid([255, 0, 0, 255])),
                    stroke: None,
                }),
            )
            .expect("矩形");
        (scene, id)
    }

    #[test]
    fn point_inside_fill_hits() {
        let (scene, id) = rect_scene();
        assert_eq!(scene.hit_test(Point::new(5.0, 5.0), 0.5), Some(id));
        assert_eq!(
            scene.hit_test(Point::new(0.0, 5.0), 0.5),
            Some(id),
            "边上(容差内)"
        );
    }

    #[test]
    fn point_outside_misses() {
        let (scene, _) = rect_scene();
        assert_eq!(scene.hit_test(Point::new(20.0, 5.0), 0.5), None);
        // 容差外贴边不算命中
        assert_eq!(scene.hit_test(Point::new(10.0 + 0.6, 5.0), 0.5), None);
    }

    #[test]
    fn stroke_only_path_hits_near_line_with_tolerance() {
        let mut scene = Scene::new();
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((10.0, 0.0));
        let id = scene
            .add_node(
                None,
                "线段",
                NodeContent::Path(PathNode {
                    path,
                    fill: None, // 无填充:只有描边可命中
                    stroke: Some(StrokeStyle {
                        paint: Paint::Solid([0, 0, 0, 255]),
                        width: 1.0,
                    }),
                }),
            )
            .expect("线段");

        // 线中点上方 0.8:距离 0.8 <= 1.0/2 + 0.5 → 命中(端点距离法会漏掉这类)
        assert_eq!(scene.hit_test(Point::new(5.0, 0.8), 0.5), Some(id));
        // 线端点命中也成立
        assert_eq!(scene.hit_test(Point::new(10.0, 0.3), 0.5), Some(id));
        // 距离 3 > 0.5 + 0.5 → 不中
        assert_eq!(scene.hit_test(Point::new(5.0, 3.0), 0.5), None);
    }

    #[test]
    fn stroke_tolerance_scales_with_node_transform() {
        // 局部线段 (0,0)-(100,0)、线宽 2,节点放大 2 倍:世界视觉半宽 = 2,
        // 世界容差 4 → 世界纵向 5 处应在命中域内(3 <= 4),8 处不在(6 > 4)。
        // 旧实现把世界容差直接丢进局部坐标(未除 k),放大节点命中域虚大一倍:
        // 世界 8 处(局部 4 <= 1 + 4)会误命中(V4.0 T7 回归)。
        let mut scene = Scene::new();
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((100.0, 0.0));
        let id = scene
            .add_node(
                None,
                "放大线段",
                NodeContent::Path(PathNode {
                    path,
                    fill: None,
                    stroke: Some(StrokeStyle {
                        paint: Paint::Solid([0, 0, 0, 255]),
                        width: 2.0,
                    }),
                }),
            )
            .expect("线段");
        scene.node_mut(id).expect("节点").transform = kurbo::Affine::scale(2.0);

        // 视觉描边缘(世界 y=2)外 3:3 <= 4 → 命中
        assert_eq!(scene.hit_test(Point::new(50.0, 5.0), 4.0), Some(id));
        // 视觉描边缘外 6:6 > 4 → 不中(旧实现此处误命中)
        assert_eq!(scene.hit_test(Point::new(50.0, 8.0), 4.0), None);
    }

    #[test]
    fn distance_to_path_measures_segment_not_endpoints() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((100.0, 0.0));
        // 线段中点上方 5:端点距离都是 √(50² + 5²) ≈ 50.2,线段距离 = 5
        let d = distance_to_path(&path, Point::new(50.0, 5.0));
        assert!((d - 5.0).abs() < 1e-6, "应取垂线距离,实际 {d}");

        // 垂足在线段外:钳制到端点
        let d = distance_to_path(&path, Point::new(120.0, 5.0));
        assert!((d - (20f64 * 20.0 + 25.0f64).sqrt()).abs() < 1e-6);

        // 曲线也要按 flatten 段算:二次曲线中段的距离远小于到端点
        let mut curve = BezPath::new();
        curve.move_to((0.0, 0.0));
        curve.quad_to((50.0, 100.0), (100.0, 0.0));
        let d = distance_to_path(&curve, Point::new(50.0, 45.0));
        assert!(d < 15.0, "曲线中段附近应命中,实际 {d}");

        // 闭合路径:ClosePath 补出闭合段
        let mut closed = BezPath::new();
        closed.move_to((0.0, 0.0));
        closed.line_to((100.0, 0.0));
        closed.close_path();
        let d = distance_to_path(&closed, Point::new(50.0, 3.0));
        assert!(d < 3.5, "闭合段(这里是 0,0 → 100,0)距离应生效,实际 {d}");
    }

    #[test]
    fn group_children_hit_through_group() {
        let mut scene = Scene::new();
        let group = scene.add_node(None, "组", NodeContent::Group).expect("组");
        let child = scene
            .add_node(
                Some(group),
                "组内矩形",
                NodeContent::Path(PathNode {
                    path: kurbo::Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1),
                    fill: Some(Paint::Solid([0, 255, 0, 255])),
                    stroke: None,
                }),
            )
            .expect("子矩形");

        // 命中的是子矩形本身,不是组(组穿透)
        assert_eq!(scene.hit_test(Point::new(5.0, 5.0), 0.5), Some(child));
        // 空白处无命中
        assert_eq!(scene.hit_test(Point::new(50.0, 50.0), 0.5), None);
    }

    #[test]
    fn locked_nodes_are_skipped() {
        let (mut scene, id) = rect_scene();
        scene.node_mut(id).expect("在").locked = true;
        assert_eq!(
            scene.hit_test(Point::new(5.0, 5.0), 0.5),
            None,
            "locked 节点跳过"
        );
    }

    #[test]
    fn invisible_nodes_are_skipped() {
        let (mut scene, id) = rect_scene();
        scene.node_mut(id).expect("在").visible = false;
        assert_eq!(
            scene.hit_test(Point::new(5.0, 5.0), 0.5),
            None,
            "不可见节点跳过"
        );
    }

    #[test]
    fn topmost_wins() {
        let mut scene = Scene::new();
        let bottom = scene
            .add_node(None, "底", rect_fill(0.0, 0.0, 10.0, 10.0))
            .expect("底");
        let top = scene
            .add_node(None, "顶", rect_fill(0.0, 0.0, 10.0, 10.0))
            .expect("顶");
        assert_eq!(scene.hit_test(Point::new(5.0, 5.0), 0.1), Some(top));
        assert_ne!(bottom, top);
    }

    fn rect_fill(x0: f64, y0: f64, x1: f64, y1: f64) -> NodeContent {
        NodeContent::Path(PathNode {
            path: kurbo::Rect::new(x0, y0, x1, y1).to_path(0.1),
            fill: Some(Paint::Solid([10, 20, 30, 255])),
            stroke: None,
        })
    }

    #[test]
    fn select_in_rect_includes_groups_and_paths() {
        let mut scene = Scene::new();
        let group = scene.add_node(None, "组", NodeContent::Group).expect("组");
        let child = scene
            .add_node(Some(group), "组内", rect_fill(20.0, 20.0, 30.0, 30.0))
            .expect("子");
        let outside = scene
            .add_node(None, "框外", rect_fill(100.0, 100.0, 110.0, 110.0))
            .expect("框外");

        // 框住组内矩形(其世界 bbox 决定组 bbox):组本身也入选("含 Group")
        let hits = scene.select_in_rect(kurbo::Rect::new(15.0, 15.0, 35.0, 35.0));
        assert!(hits.contains(&group), "框选含 Group");
        assert!(hits.contains(&child));
        assert!(!hits.contains(&outside));

        // 空 rect 无命中
        assert!(
            scene
                .select_in_rect(kurbo::Rect::new(200.0, 200.0, 201.0, 201.0))
                .is_empty()
        );
    }

    #[test]
    fn select_in_rect_skips_locked() {
        let (mut scene, id) = rect_scene();
        scene.node_mut(id).expect("在").locked = true;
        assert!(
            scene
                .select_in_rect(kurbo::Rect::new(-1.0, -1.0, 20.0, 20.0))
                .is_empty()
        );
    }

    #[test]
    fn select_in_rect_returns_bottom_to_top_order() {
        let mut scene = Scene::new();
        let bottom = scene
            .add_node(None, "底", rect_fill(0.0, 0.0, 10.0, 10.0))
            .expect("底");
        let top = scene
            .add_node(None, "顶", rect_fill(0.0, 0.0, 10.0, 10.0))
            .expect("顶");
        let hits = scene.select_in_rect(kurbo::Rect::new(-1.0, -1.0, 20.0, 20.0));
        assert_eq!(hits, vec![bottom, top], "保持渲染列表的底→顶序");
    }

    /// Text 按**实测布局尺寸**的包围盒命中(V4.0 T6.1):内部点、右缘容差内
    /// 命中;右缘容差外不命中。断言以实测宽高为基准,不依赖任何粗估常数。
    #[test]
    fn text_hits_by_measured_bbox() {
        let mut scene = Scene::new();
        let id = scene
            .add_node(
                None,
                "文本",
                NodeContent::Text(sable_foundation::scene::TextNode {
                    text: "北京 2026".into(),
                    font_size: 10.0,
                    color: [0, 0, 0, 255],
                }),
            )
            .expect("文本");
        let (w, h) = crate::text_glyphs::measured_text_size("北京 2026", 10.0);
        assert!(w > 0.0 && h > 0.0);
        assert_eq!(
            scene.hit_test(Point::new(w / 2.0, h / 2.0), 0.5),
            Some(id),
            "实测范围内部命中"
        );
        assert_eq!(
            scene.hit_test(Point::new(w + 0.4, h / 2.0), 0.5),
            Some(id),
            "右缘外但容差(0.5)内命中"
        );
        assert_eq!(
            scene.hit_test(Point::new(w + 2.0, h / 2.0), 0.5),
            None,
            "右缘外超过容差不命中"
        );
    }

    /// R6 回归(V4.0 T6.1):0.6em/字符粗估对 CJK(实际 ≈1.0em/字)偏窄
    /// ~40%,右半边点不中。旧粗估右缘**之外**、实测范围**之内**的点必须
    /// 命中——这是旧 bug 的直接回归断言。
    #[test]
    fn cjk_text_hits_beyond_old_coarse_estimate() {
        let mut scene = Scene::new();
        let id = scene
            .add_node(
                None,
                "CJK",
                NodeContent::Text(sable_foundation::scene::TextNode {
                    text: "你好".into(),
                    font_size: 20.0,
                    color: [0, 0, 0, 255],
                }),
            )
            .expect("文本");
        let (w, h) = crate::text_glyphs::measured_text_size("你好", 20.0);
        let coarse_width = 0.6 * 20.0 * 2.0; // 旧粗估右缘 = 24(0.6em × 2 字)
        assert!(
            w > coarse_width + 10.0,
            "CJK 实测宽 {w} 应显著大于旧粗估 {coarse_width}(≈1em/字 vs 0.6em/字)"
        );
        // 旧粗估之外、实测之内的点:旧实现点不中,现在必须命中
        let probe_x = (coarse_width + w) / 2.0;
        assert!(
            probe_x > coarse_width + 0.5,
            "探针点必须在旧粗估 + 容差之外,probe={probe_x}"
        );
        assert_eq!(
            scene.hit_test(Point::new(probe_x, h / 2.0), 0.5),
            Some(id),
            "旧粗估右缘外的实际字形范围必须可点中"
        );
        // 粗估范围内依旧命中(不回归)
        assert_eq!(scene.hit_test(Point::new(10.0, h / 2.0), 0.5), Some(id));
        // 实测范围外(容差外)不命中
        assert_eq!(scene.hit_test(Point::new(w + 2.0, h / 2.0), 0.5), None);
    }

    /// 框选含文本:实测包围盒(而非粗估)决定入选范围——框住 CJK 右半边
    /// (旧粗估之外)也必须选中。
    #[test]
    fn select_in_rect_uses_measured_text_bbox() {
        let mut scene = Scene::new();
        let id = scene
            .add_node(
                None,
                "CJK",
                NodeContent::Text(sable_foundation::scene::TextNode {
                    text: "你好".into(),
                    font_size: 20.0,
                    color: [0, 0, 0, 255],
                }),
            )
            .expect("文本");
        let (w, h) = crate::text_glyphs::measured_text_size("你好", 20.0);
        let coarse_width = 0.6 * 20.0 * 2.0; // 24
        // 只框住右半边(coarse..w):粗估口径会漏选,实测口径必须命中
        let hits = scene.select_in_rect(kurbo::Rect::new(
            (coarse_width + w) / 2.0,
            0.0,
            w + 100.0,
            h,
        ));
        assert!(hits.contains(&id), "文本右半边必须可框选(实测 bbox)");
    }

    // —— 锚点几何辅助(docs/04 §3) ——

    #[test]
    fn first_subpath_decomposes_lines_and_curves() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((10.0, 0.0));
        path.curve_to((12.0, -3.0), (18.0, -3.0), (20.0, 0.0));
        path.close_path();

        let sub = first_subpath(&path).expect("单子路径");
        assert!(sub.closed);
        assert_eq!(sub.anchors.len(), 3);
        // 直线锚点不带手柄;曲线段给前锚配出手柄、后锚配入手柄
        assert_eq!(sub.anchors[0].out_handle, None);
        assert_eq!(sub.anchors[1].in_handle, None);
        assert_eq!(sub.anchors[1].out_handle, Some(Point::new(12.0, -3.0)));
        assert_eq!(sub.anchors[2].in_handle, Some(Point::new(18.0, -3.0)));
        assert_eq!(sub.anchors[2].out_handle, None);
    }

    #[test]
    fn first_subpath_raises_quad_to_cubic_exactly() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.quad_to((5.0, 10.0), (10.0, 0.0));

        let sub = first_subpath(&path).expect("单子路径");
        assert!(!sub.closed);
        assert_eq!(sub.anchors.len(), 2);
        assert_eq!(sub.anchors[1].pos, Point::new(10.0, 0.0));
        // 升阶后 t=0.5 的点与原二次曲线一致(QuadBez::raise 无误差)
        let seg = anchor_segment(&sub.anchors[0], &sub.anchors[1]);
        let mid = seg.eval(0.5);
        let quad_mid = Point::new(5.0, 5.0); // (0,0)-(5,10)-(10,0) 的 t=0.5 点
        assert!((mid - quad_mid).hypot() < 1e-9);
    }

    #[test]
    fn first_subpath_rejects_empty_and_multi_subpath() {
        assert!(first_subpath(&BezPath::new()).is_none(), "空路径");

        let mut multi = BezPath::new();
        multi.move_to((0.0, 0.0));
        multi.line_to((1.0, 1.0));
        multi.move_to((5.0, 5.0));
        multi.line_to((6.0, 6.0));
        assert!(first_subpath(&multi).is_none(), "多子路径 v1.0 不支持");
    }

    #[test]
    fn hit_anchor_returns_first_within_tolerance() {
        let anchors = vec![
            AnchorPoint {
                pos: Point::new(0.0, 0.0),
                in_handle: None,
                out_handle: None,
            },
            AnchorPoint {
                pos: Point::new(10.0, 0.0),
                in_handle: None,
                out_handle: None,
            },
        ];
        assert_eq!(hit_anchor(&anchors, Point::new(10.5, 0.5), 1.0), Some(1));
        assert_eq!(hit_anchor(&anchors, Point::new(5.0, 5.0), 1.0), None);
    }

    #[test]
    fn nearest_on_subpath_finds_line_midpoint() {
        let sub = SubPath {
            closed: false,
            anchors: vec![
                AnchorPoint {
                    pos: Point::new(0.0, 0.0),
                    in_handle: None,
                    out_handle: None,
                },
                AnchorPoint {
                    pos: Point::new(10.0, 0.0),
                    in_handle: None,
                    out_handle: None,
                },
            ],
        };
        let n = nearest_on_subpath(&sub, Point::new(5.0, 1.0), 0.25).expect("有线段");
        assert_eq!(n.segment, 0);
        assert!((n.t - 0.5).abs() < 0.01);
        assert!((n.point - Point::new(5.0, 0.0)).hypot() < 0.01);
        assert!((n.distance - 1.0).abs() < 0.01);
    }

    #[test]
    fn nearest_on_subpath_indexes_closing_segment() {
        let sub = SubPath {
            closed: true,
            anchors: vec![
                AnchorPoint {
                    pos: Point::new(0.0, 0.0),
                    in_handle: None,
                    out_handle: None,
                },
                AnchorPoint {
                    pos: Point::new(10.0, 0.0),
                    in_handle: None,
                    out_handle: None,
                },
                AnchorPoint {
                    pos: Point::new(5.0, 8.0),
                    in_handle: None,
                    out_handle: None,
                },
            ],
        };
        // 闭合段 = 末锚(5,8) → 首锚(0,0),中点 (2.5,4)
        let n = nearest_on_subpath(&sub, Point::new(2.5, 4.0), 0.25).expect("有闭合段");
        assert_eq!(n.segment, 2, "闭合段下标 = anchors.len()-1");
        assert!(n.distance < 0.01);
    }
}
