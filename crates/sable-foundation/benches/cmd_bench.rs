//! 命令系统微基准(分册六 §5.1 基准体系 #1)。
//!
//! 测量什么:
//! - `exec/set_fill_merge`:同节点连续 SetFill **开启 merge** 的执行(栈顶合并,
//!   拖动范式的每步成本);
//! - `exec/set_fill_distinct`:双节点交替 SetFill(merge 不命中,撤销栈线性
//!   增长路径,10k 条);
//! - `undo_redo/single_step`:单步撤销↔重做 10k 次交替(命令 revert/apply 的
//!   纯执行成本);
//! - `history/transaction_1000`:1000 组事务(每组 10 条命令,undo 一步);
//! - `scene/add_node_10k`:Scene::add_node 10k 节点(slotmap 发号 + 父链挂接)。
//!
//! 运行:`cargo bench -p sable-foundation`(CI 默认不跑 bench harness)。

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use kurbo::BezPath;
use sable_foundation::prelude::{
    Command as _, History, NodeContent, NodeId, Paint, PathNode, Scene, SetFill,
};

/// 10k 次执行的目标命令数(分册六 §5.1 的量级约定)。
const EXEC_ITERS: u64 = 10_000;
/// 事务基准:1000 组事务 × 每组 10 条命令。
const TRANSACTION_GROUPS: u64 = 1_000;
const COMMANDS_PER_TRANSACTION: u64 = 10;
/// add_node 基准的节点数。
const ADD_NODE_COUNT: u64 = 10_000;

/// 矩形路径(与 command.rs 测试的 rect_content 同构,固定几何)。
fn rect_path(size: f64) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((0.0, 0.0));
    path.line_to((size, 0.0));
    path.line_to((size, size));
    path.line_to((0.0, size));
    path.close_path();
    path
}

/// 一个带两个矩形路径节点的场景,返回 (scene, 节点A, 节点B)。
fn scene_with_two_paths() -> (Scene, NodeId, NodeId) {
    let mut scene = Scene::new();
    let content_a = || {
        NodeContent::Path(PathNode {
            path: rect_path(10.0),
            fill: Some(Paint::Solid([90, 90, 90, 255])),
            stroke: None,
        })
    };
    let a = scene
        .add_node(None, "A", content_a())
        .expect("基准场景种子节点 A 必须能插入");
    let b = scene
        .add_node(None, "B", content_a())
        .expect("基准场景种子节点 B 必须能插入");
    (scene, a, b)
}

/// 同节点连续 SetFill(merge 开):10k 次执行合并为一步撤销。
fn bench_exec_set_fill_merge(c: &mut Criterion) {
    c.bench_function("exec/set_fill_merge_10k", |b| {
        b.iter(|| {
            let (mut scene, a, _b) = scene_with_two_paths();
            let mut history = History::new();
            for i in 0..EXEC_ITERS {
                let value = (i % 256) as u8;
                history.exec(
                    SetFill {
                        id: a,
                        old: None,
                        new: Some(Paint::Solid([value, value, value, 255])),
                    }
                    .boxed(),
                    &mut scene,
                );
            }
            black_box(history.undo_len())
        })
    });
}

/// 双节点交替 SetFill(merge 不命中):10k 条独立撤销步的线性入栈。
fn bench_exec_set_fill_distinct(c: &mut Criterion) {
    c.bench_function("exec/set_fill_distinct_10k", |b| {
        b.iter(|| {
            let (mut scene, a, b_node) = scene_with_two_paths();
            let mut history = History::new();
            for i in 0..EXEC_ITERS {
                let target = if i % 2 == 0 { a } else { b_node };
                let value = (i % 256) as u8;
                history.exec(
                    SetFill {
                        id: target,
                        old: None,
                        new: Some(Paint::Solid([value, value, value, 255])),
                    }
                    .boxed(),
                    &mut scene,
                );
            }
            black_box(history.undo_len())
        })
    });
}

/// 单步撤销↔重做 10k 次交替(apply/revert 纯执行成本,不含栈增长)。
fn bench_undo_redo_single_step(c: &mut Criterion) {
    c.bench_function("undo_redo/single_step_10k", |b| {
        b.iter(|| {
            let (mut scene, a, _b) = scene_with_two_paths();
            let mut history = History::new();
            history.exec(
                SetFill {
                    id: a,
                    old: None,
                    new: Some(Paint::Solid([1, 2, 3, 4])),
                }
                .boxed(),
                &mut scene,
            );
            for _ in 0..EXEC_ITERS {
                history.undo(&mut scene);
                history.redo(&mut scene);
            }
            black_box(history.undo_len())
        })
    });
}

/// 1000 组事务(每组 10 条命令 = 一步撤销):事务收集 + 批量提交成本。
fn bench_history_transaction(c: &mut Criterion) {
    c.bench_function("history/transaction_1000x10", |b| {
        b.iter(|| {
            let (mut scene, a, b_node) = scene_with_two_paths();
            let mut history = History::new();
            for _ in 0..TRANSACTION_GROUPS {
                history.begin_transaction();
                for j in 0..COMMANDS_PER_TRANSACTION {
                    let target = if j % 2 == 0 { a } else { b_node };
                    history.exec(
                        SetFill {
                            id: target,
                            old: None,
                            new: Some(Paint::Solid([7, 8, 9, 255])),
                        }
                        .boxed(),
                        &mut scene,
                    );
                }
                history.end_transaction();
            }
            black_box(history.undo_len())
        })
    });
}

/// Scene::add_node 10k 节点(平铺 roots;slotmap 发号 + roots 追加)。
fn bench_scene_add_node(c: &mut Criterion) {
    c.bench_function("scene/add_node_10k", |b| {
        b.iter(|| {
            let mut scene = Scene::new();
            let content = || {
                NodeContent::Path(PathNode {
                    path: rect_path(8.0),
                    fill: Some(Paint::Solid([128, 128, 128, 255])),
                    stroke: None,
                })
            };
            for i in 0..ADD_NODE_COUNT {
                black_box(scene.add_node(None, format!("n{i}"), content()).ok());
            }
            black_box(scene.len())
        })
    });
}

// criterion 0.5 的 criterion_group!/criterion_main!(docs.rs 惯用形态;
// criterion 未在本机 registry,API 按其 0.3→0.5 稳定签名保守书写——见任务报告)。
criterion_group!(
    benches,
    bench_exec_set_fill_merge,
    bench_exec_set_fill_distinct,
    bench_undo_redo_single_step,
    bench_history_transaction,
    bench_scene_add_node
);
criterion_main!(benches);
