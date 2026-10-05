//! GATE-07:命中测试几何不变量(proptest;canvas dev-dep `proptest`
//! 既有声明,本文件是其首个消费者)。
//!
//! 不变量(显式化):
//! 1. 单矩形(恒等变换,有填充无描边):点在矩形**内部** ⟺ 命中该节点;
//! 2. 两矩形叠放:交叠区的命中 = **顶层**节点;仅在下层区域 = 下层;
//! 3. 顶层锁定 → 跳过,交叠区命中回落下层。

use proptest::prelude::*;
use sable_canvas::hit_test::SceneHitTest;
use sable_foundation::scene::{NodeContent, Paint, PathNode, Scene};

fn rect_content(x0: f64, y0: f64, x1: f64, y1: f64) -> NodeContent {
    let mut path = kurbo::BezPath::new();
    path.move_to(kurbo::Point::new(x0, y0));
    path.line_to(kurbo::Point::new(x1, y0));
    path.line_to(kurbo::Point::new(x1, y1));
    path.line_to(kurbo::Point::new(x0, y1));
    path.close_path();
    NodeContent::Path(PathNode {
        path,
        fill: Some(Paint::Solid([255, 0, 0, 255])),
        stroke: None,
    })
}

// 不变量 1:内部 ⟺ 命中(随机点 × 固定矩形 10..60)
proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn gate_07_hit_iff_inside_single_rect(px in 0.0f64..100.0, py in 0.0f64..100.0) {
        let mut scene = Scene::new();
        scene.add_node(None, "rect", rect_content(10.0, 10.0, 60.0, 60.0)).expect("节点");
        let hit = scene.hit_test(kurbo::Point::new(px, py), 0.0);
        let inside = (12.0..58.0).contains(&px) && (12.0..58.0).contains(&py);
        // 内部必命中;外部(留 2px 抗锯齿裕度,规避路径边界舍入)必不命中
        if inside {
            assert!(hit.is_some(), "({px},{py}) 在矩形内必命中");
        } else if !(8.0..62.0).contains(&px) || !(8.0..62.0).contains(&py) {
            assert!(hit.is_none(), "({px},{py}) 远离矩形必不命中");
        }
    }
}

// 不变量 2/3:叠放顶优先;锁定跳过
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn gate_07_topmost_wins_and_locked_skipped(px in 32.0f64..55.0, py in 32.0f64..55.0) {
        let mut scene = Scene::new();
        scene.add_node(None, "bottom", rect_content(10.0, 10.0, 60.0, 60.0)).expect("底");
        scene.add_node(None, "top", rect_content(30.0, 30.0, 80.0, 80.0)).expect("顶");
        let pt = kurbo::Point::new(px, py);
        // 交叠区(32..55 ⊂ 30..80 ∩ 10..60)= 顶层
        let hit = scene.hit_test(pt, 0.0);
        let top_name = scene.node(hit.expect("交叠区必命中")).expect("在").name.clone();
        assert_eq!(top_name, "top", "交叠区顶层优先");
        // 顶层锁定 → 回落底层
        let top_id = hit.expect("在");
        scene.node_mut(top_id).expect("在").locked = true;
        let hit2 = scene.hit_test(pt, 0.0);
        let bottom_name = scene.node(hit2.expect("锁定回落底层必命中")).expect("在").name.clone();
        assert_eq!(bottom_name, "bottom", "锁定节点跳过");
    }
}
