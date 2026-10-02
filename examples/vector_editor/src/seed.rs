//! 初始场景:程序化放 2 个矩形 + 1 个手写圆角路径(BezPath),颜色取自
//! sable-widgets 主题。
//!
//! 初始内容是"打开文档"语义(见 document.rs 模块文档第 1 条):直接构建,
//! 不产生 Command、不进撤销栈;此后的一切修改必须走 `Document::exec`。

use sable::core::scene::{NodeContent, Paint, PathNode, Rgba8, Scene};
use sable::kurbo::BezPath;

use crate::palette::Palette;

/// 铺初始场景(调用方持有 `Entity<Document>`,这里直接改 scene 数据;
/// `add_node` 失败只可能是父不存在,父为 None 恒成功,expect 安全)。
pub fn seed_initial_scene(scene: &mut Scene, palette: &Palette) {
    scene
        .add_node(
            None,
            "矩形 A",
            rect(40.0, 40.0, 240.0, 160.0, palette.accent_solid),
        )
        .expect("根级添加矩形 A:无父节点,不可能失败");
    scene
        .add_node(
            None,
            "矩形 B",
            rect(300.0, 120.0, 520.0, 320.0, palette.success_solid),
        )
        .expect("根级添加矩形 B:无父节点,不可能失败");
    scene
        .add_node(
            None,
            "圆角路径",
            rounded_rect(120.0, 260.0, 420.0, 460.0, 48.0, palette.warning_solid),
        )
        .expect("根级添加圆角路径:无父节点,不可能失败");
}

/// 矩形节点内容。
fn rect(x0: f64, y0: f64, x1: f64, y1: f64, color: Rgba8) -> NodeContent {
    let mut path = BezPath::new();
    path.move_to((x0, y0));
    path.line_to((x1, y0));
    path.line_to((x1, y1));
    path.line_to((x0, y1));
    path.close_path();
    NodeContent::Path(PathNode {
        path,
        fill: Some(Paint::Solid(color)),
        stroke: None,
    })
}

/// 手写圆角矩形:四条直边 + 四个三次贝塞尔角(控制点落在角点上,
/// 四分之一圆的经典近似)。
fn rounded_rect(x0: f64, y0: f64, x1: f64, y1: f64, radius: f64, color: Rgba8) -> NodeContent {
    let r = radius.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
    let mut path = BezPath::new();
    // 顶边 → 右上角
    path.move_to((x0 + r, y0));
    path.line_to((x1 - r, y0));
    path.curve_to((x1, y0), (x1, y0 + r), (x1, y0 + r));
    // 右边 → 右下角
    path.line_to((x1, y1 - r));
    path.curve_to((x1, y1), (x1 - r, y1), (x1 - r, y1));
    // 底边 → 左下角
    path.line_to((x0 + r, y1));
    path.curve_to((x0, y1), (x0, y1 - r), (x0, y1 - r));
    // 左边 → 左上角
    path.line_to((x0, y0 + r));
    path.curve_to((x0, y0), (x0 + r, y0), (x0 + r, y0));
    path.close_path();
    NodeContent::Path(PathNode {
        path,
        fill: Some(Paint::Solid(color)),
        stroke: None,
    })
}
