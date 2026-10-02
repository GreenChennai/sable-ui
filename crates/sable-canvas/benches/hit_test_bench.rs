//! 命中测试微基准(分册六 §5.1 基准体系 #3)。
//!
//! 测量什么:
//! - `hit_test/point_1000_nodes`:1000 个随机分布的矩形路径节点做**单点
//!   点选**一次的成本(`SceneHitTest::hit_test`,含 render_list 构建 +
//!   顶→底逆序遍历 + 局部坐标换算 + 路径 contains);
//! - `hit_test/select_rect_1000_nodes`:同场景一次**框选**(`select_in_rect`,
//!   包围盒相交过滤)。
//!
//! 随机分布用确定性 xorshift64(不引 rand;种子固定 → 逐位可复现)。
//! 运行:`cargo bench -p sable-canvas`(CI 默认不跑 bench harness)。

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use kurbo::{Affine, BezPath, Point, Rect};
use sable_canvas::hit_test::SceneHitTest;
use sable_foundation::prelude::{NodeContent, Paint, PathNode, Scene};

/// 场景节点数(分册六 §5.1 量级)。
const NODE_COUNT: u64 = 1_000;
/// 点选容差(世界坐标;与 input.rs 的 screen_tolerance 量级一致)。
const TOLERANCE: f64 = 4.0;

/// xorshift64(确定性伪随机;Galaxy 序列,周期 2^64-1)。
struct XorShift(u64);

impl XorShift {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// [0, 1) 均匀分布(取高 53 位保精度)。
    fn next_f64(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64) / (1u64 << 53) as f64
    }
}

/// 正方形路径(原点出发 10×10,节点平移由 transform 承担)。
fn square_path(size: f64) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((0.0, 0.0));
    path.line_to((size, 0.0));
    path.line_to((size, size));
    path.line_to((0.0, size));
    path.close_path();
    path
}

/// 1000 个矩形节点的场景:位置/尺寸由 xorshift 决定,均布在 2000×2000 世界。
fn random_scene(seed: u64) -> Scene {
    let mut rng = XorShift(seed);
    let mut scene = Scene::new();
    for i in 0..NODE_COUNT {
        let x = rng.next_f64() * 2000.0;
        let y = rng.next_f64() * 2000.0;
        let size = 6.0 + rng.next_f64() * 14.0;
        // 平移并进场景(render 时才用 world_xform;命中测试同样走它)
        let content_path: BezPath = Affine::translate((x, y)) * square_path(size);
        let content = NodeContent::Path(PathNode {
            path: content_path,
            fill: Some(Paint::Solid([160, 160, 160, 255])),
            stroke: None,
        });
        if scene.add_node(None, format!("n{i}"), content).is_err() {
            break; // 基准种子插入不应失败;防御性终止避免死循环语义
        }
    }
    scene
}

fn bench_hit_test_point(c: &mut Criterion) {
    // 种子固定:场景与查询点逐位可复现
    let scene = random_scene(0x9E37_79B9_7F4A_7C15);
    let mut rng = XorShift(42);
    // 预生成一批查询点(大部分落在分布域内,miss 路径自然占比)
    let points: Vec<Point> = (0..256)
        .map(|_| Point::new(rng.next_f64() * 2200.0, rng.next_f64() * 2200.0))
        .collect();
    let mut i = 0usize;
    c.bench_function("hit_test/point_1000_nodes", |b| {
        b.iter(|| {
            i = (i + 1) % points.len();
            black_box(scene.hit_test(black_box(points[i]), TOLERANCE))
        })
    });
}

fn bench_hit_test_rect(c: &mut Criterion) {
    let scene = random_scene(0x9E37_79B9_7F4A_7C15);
    let selection = Rect::new(400.0, 400.0, 900.0, 900.0);
    c.bench_function("hit_test/select_rect_1000_nodes", |b| {
        b.iter(|| black_box(scene.select_in_rect(black_box(selection))))
    });
}

// criterion 0.5 的 criterion_group!/criterion_main!(docs.rs 惯用形态;
// criterion 未在本机 registry,API 按其 0.3→0.5 稳定签名保守书写——见任务报告)。
criterion_group!(benches, bench_hit_test_point, bench_hit_test_rect);
criterion_main!(benches);
