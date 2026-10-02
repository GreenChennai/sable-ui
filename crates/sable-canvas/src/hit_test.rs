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

use kurbo::{BezPath, PathEl, Point, Rect, Shape};
use sable_foundation::scene::{NodeContent, NodeId, Scene};

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
                    // 2. 描边命中:点到路径距离 <= 线宽/2 + 容差
                    let near_stroke = p.stroke.as_ref().is_some_and(|s| {
                        distance_to_path(&p.path, local) <= s.width / 2.0 + tolerance
                    });
                    in_fill || near_stroke
                }
                // Text/Image 用世界包围盒外扩容差(docs/02 §5 的 bbox inflate;
                // 世界 bbox 已含节点变换,与 scene 的粗估口径一致)
                NodeContent::Text(_) | NodeContent::Image(_) => {
                    self.node_world_bbox(id).is_some_and(|bbox| {
                        let grown = bbox.inflate(tolerance, tolerance);
                        grown.x0 <= world_pt.x
                            && world_pt.x <= grown.x1
                            && grown.y0 <= world_pt.y
                            && world_pt.y <= grown.y1
                    })
                }
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
                self.node_world_bbox(*id).is_some_and(|bbox| {
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

    #[test]
    fn text_hits_by_inflated_bbox() {
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
        // 粗估 bbox:0.6em 字宽 × 1.2em 行高;"北京 2026" 7 字符 → 0.6*10*7=42 宽 × 12 高
        assert_eq!(scene.hit_test(Point::new(10.0, 6.0), 0.5), Some(id));
        // bbox 外但容差内
        assert_eq!(scene.hit_test(Point::new(42.0 + 0.4, 6.0), 0.5), Some(id));
        assert_eq!(scene.hit_test(Point::new(60.0, 6.0), 0.5), None);
    }
}
