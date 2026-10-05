//! RBT-03/04/12 深度与畸形输入回归(迭代审查报告 §4.6/§6)。
//!
//! - TC-RBT-SCENE-01:十万级深链遍历/摘除不栈溢出(显式栈,RBT-04);
//! - TC-RBT-SVG-01:1024 层嵌套导出 = 深度帽计数可观测,不崩(RBT-03);
//! - TC-RBT-FUZZ-01:畸形 `.sable` 字节流喂 `load_project` 零 panic(proptest,
//!   RBT-12 的工程文件面;SVG 面已有 `prop_sv`)。

use proptest::prelude::*;
use sable_foundation::project::load_project;
use sable_foundation::scene::{NodeContent, Scene};
#[cfg(feature = "svg")]
use sable_foundation::svg::export_svg_with_report;

/// 构造 n 层深链(每层一个 Group 子节点,挂最前),返回 (scene, 最深节点)。
fn deep_chain(n: usize) -> (Scene, sable_foundation::scene::NodeId) {
    let mut scene = Scene::new();
    let mut cur = scene
        .add_node(None, "root", NodeContent::Group)
        .expect("root");
    for i in 0..n {
        cur = scene
            .insert_at(
                Some(cur),
                Some(0),
                sable_foundation::scene::Node::new(format!("layer{i}"), NodeContent::Group),
            )
            .expect("insert");
    }
    (scene, cur)
}

/// TC-RBT-SCENE-01:十万层深链 render_list / 摘除 全走显式栈。
#[test]
fn tc_rbt_scene_01_100k_deep_chain_walk_and_remove_no_overflow() {
    const DEPTH: usize = 100_000;
    let (mut scene, deepest) = deep_chain(DEPTH);
    let list = scene.render_list();
    assert_eq!(list.len(), DEPTH + 1, "根 + 每层一节点");
    let _ = deepest;
    // 摘除整棵树(collect_subtree 显式栈,十万节点全入栈不溢出)
    let roots = scene.roots.clone();
    for root in roots {
        scene.remove_subtree(root).expect("摘除根子树");
    }
    assert_eq!(scene.render_list().len(), 0);
}

/// TC-RBT-SVG-01:1024 层嵌套导出 → 深度帽可观测(>0),不崩;
/// 浅层场景帽计数为 0(既有输出不变)。feature `svg` 门控。
#[cfg(feature = "svg")]
#[test]
fn tc_rbt_svg_01_deep_export_reports_depth_cap_without_crash() {
    let (scene, _) = deep_chain(1024);
    let (svg, report) = export_svg_with_report(&scene);
    assert!(!svg.is_empty());
    assert!(
        report.depth_exceeded_nodes > 0,
        "1024 层必触发深度帽(帽 512)"
    );
    // 浅层不受影响
    let (shallow, _) = deep_chain(16);
    let (svg2, report2) = export_svg_with_report(&shallow);
    assert!(!svg2.is_empty());
    assert_eq!(report2.depth_exceeded_nodes, 0, "浅层零帽计数");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    /// TC-RBT-FUZZ-01:任意字节流(含合法头前缀的损坏体)喂加载路径
    /// 零 panic——结构化错误或成功,绝不崩(RB-01)。
    #[test]
    fn tc_rbt_fuzz_01_corrupt_sable_never_panics(
        prefix in proptest::prelude::any::<bool>(),
        body in proptest::prelude::any::<Vec<u8>>(),
    ) {
        // .sable 头 = 4 字节魔数 "SABL" + 1 字节版本(见 project.rs;
        // prefix 决定是否给合法头,覆盖"头坏/体坏"两族)。
        let mut bytes: Vec<u8> = if prefix {
            b"SABL".to_vec()
        } else {
            b"JUNK".to_vec()
        };
        bytes.push(1);
        bytes.extend_from_slice(&body);
        // 写临时文件喂 load_project(公开 API;不触内部解析)
        let dir = std::env::temp_dir().join("sable-rbt-fuzz");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("fuzz-{}-{}.sable", std::process::id(), body.len()));
        std::fs::write(&path, &bytes).expect("临时文件");
        let _ = load_project(&path); // Err 或 Ok,断言不 panic 即达成
        let _ = std::fs::remove_file(&path);
    }
}
