//! `.sable` 工程存取微基准(分册六 §5.1 基准体系 #2)。
//!
//! 测量什么:
//! - `project/save_1k_nodes_500_cmds`:1000 节点场景 + 500 条命令的撤销栈,
//!   序列化(含 MessagePack 编码 + 原子写临时文件 rename)一次的耗时;
//! - `project/load_1k_nodes_500_cmds`:同一文件的反序列化(魔数/版本校验 +
//!   MessagePack 解码 + 历史栈重建)一次的耗时。
//!
//! 临时文件落在 `std::env::temp_dir()`(每个样本固定覆写同一文件,内容恒定)。
//! 运行:`cargo bench -p sable-foundation`(CI 默认不跑 bench harness)。

use std::path::PathBuf;

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use kurbo::BezPath;
use sable_foundation::prelude::{
    Command as _, History, NodeContent, Paint, PathNode, Scene, SetFill,
};
use sable_foundation::project::{load_project, save_project};

/// 场景节点数。
const NODE_COUNT: u64 = 1_000;
/// 撤销栈命令数。
const COMMAND_COUNT: u64 = 500;

/// 矩形路径(固定几何,节点间仅平移不同)。
fn rect_path(x: f64, size: f64) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((x, 0.0));
    path.line_to((x + size, 0.0));
    path.line_to((x + size, size));
    path.line_to((x, size));
    path.close_path();
    path
}

/// 基准数据:1000 节点场景 + 500 条 SetFill(双节点交替防 merge 合并)。
fn make_project() -> (Scene, History) {
    let mut scene = Scene::new();
    let mut ids = Vec::with_capacity(NODE_COUNT as usize);
    for i in 0..NODE_COUNT {
        let content = NodeContent::Path(PathNode {
            path: rect_path(f64::from(i as u16) * 8.0, 6.0),
            fill: Some(Paint::Solid([100, 100, 100, 255])),
            stroke: None,
        });
        if let Ok(id) = scene.add_node(None, format!("节点{i}"), content) {
            ids.push(id);
        }
    }
    let mut history = History::new();
    for i in 0..COMMAND_COUNT {
        // 双节点交替 → merge 不命中 → 撤销栈 500 步(与真实编辑量级一致)
        let id = ids[(i % ids.len() as u64) as usize];
        let value = (i % 256) as u8;
        history.exec(
            SetFill {
                id,
                old: Some(Paint::Solid([100, 100, 100, 255])),
                new: Some(Paint::Solid([value, 100, 100, 255])),
            }
            .boxed(),
            &mut scene,
        );
    }
    (scene, history)
}

/// 固定临时文件路径(同一进程内反复覆写,内容恒定)。
fn bench_file() -> PathBuf {
    std::env::temp_dir().join("sable-bench-project.sable")
}

fn bench_save_project(c: &mut Criterion) {
    let (scene, history) = make_project();
    let path = bench_file();
    c.bench_function("project/save_1k_nodes_500_cmds", |b| {
        b.iter(|| {
            black_box(
                save_project(&path, black_box(&scene), black_box(&history))
                    .expect("基准工程写入不得失败(临时目录可写)"),
            )
        })
    });
}

fn bench_load_project(c: &mut Criterion) {
    let (scene, history) = make_project();
    let path = bench_file();
    save_project(&path, &scene, &history).expect("基准工程预写入不得失败");
    c.bench_function("project/load_1k_nodes_500_cmds", |b| {
        b.iter(|| {
            let data = load_project(&path).expect("基准工程读取不得失败");
            black_box((data.scene.len(), data.history.undo.len()))
        })
    });
}

// criterion 0.5 的 criterion_group!/criterion_main!(docs.rs 惯用形态;
// criterion 未在本机 registry,API 按其 0.3→0.5 稳定签名保守书写——见任务报告)。
criterion_group!(benches, bench_save_project, bench_load_project);
criterion_main!(benches);
