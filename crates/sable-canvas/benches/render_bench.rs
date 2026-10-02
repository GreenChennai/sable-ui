//! 场景渲染微基准(分册六 §5.1 基准体系 #4)。
//!
//! 测量什么:
//! - `render/scene_200_shapes_96px`:200 个形状(实心矩形,10% 带描边)经
//!   `render_scene` 调度进 vello_cpu 指令流并光栅化取回 96×96 RGBA 的**整帧**
//!   成本(含视锥剔除判定、LOD 分级、render_list 构建)。
//!
//! 效果管线本基准不涉及(无效果节点 = E12 Off 零开销路径;"caps reduced"
//! 指 EffectLevel 降级矩阵的下限档,对无效果场景无差异——命中/模糊成本归
//! shadow 基准,不在本文件)。
//! 运行:`cargo bench -p sable-canvas`(CI 默认不跑 bench harness)。

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use kurbo::{Affine, BezPath, Vec2};
use sable_canvas::render::{RenderOpts, render_scene};
use sable_foundation::prelude::{NodeContent, Paint, PathNode, Scene, StrokeStyle, Viewport};
use sable_paint::prelude::CpuRenderer;

/// 场景形状数。
const SHAPE_COUNT: u64 = 200;
/// 画布尺寸(小画布聚焦调度/指令流成本,不测大面光栅化)。
const CANVAS: u16 = 96;
/// 形状分布域(部分落在画布外,让视锥剔除有真实命中率)。
const WORLD_SPAN: f64 = 300.0;
/// 带描边节点的比例(1/10;描边是单独的指令路径)。
const STROKE_MODULUS: u64 = 10;

/// 正方形路径(原点,size 由 transform 承担)。
fn square_path(size: f64) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((0.0, 0.0));
    path.line_to((size, 0.0));
    path.line_to((size, size));
    path.line_to((0.0, size));
    path.close_path();
    path
}

/// 200 形状场景:网格化平移 + 固定伪随机变化(xorshift,确定性)。
fn bench_scene() -> Scene {
    let mut state: u64 = 0x853C_49E6_748F_EA9B;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut scene = Scene::new();
    for i in 0..SHAPE_COUNT {
        // 撒满 300×300 分布域:只有一角落在 96×96 视口内,剔除路径真实生效
        let x = (f64::from(i as u16) * 37.0) % WORLD_SPAN;
        let y = (f64::from(i as u16) * 91.0) % WORLD_SPAN;
        let size = 8.0 + (next() % 8) as f64;
        // kurbo 0.13 无 BezPath::apply_transform:用 Affine × path(Mul<BezPath>)
        let path: BezPath = Affine::translate((x, y)) * square_path(size);
        let stroke = (i % STROKE_MODULUS == 0).then_some(StrokeStyle {
            paint: Paint::Solid([30, 30, 30, 255]),
            width: 1.0,
        });
        let content = NodeContent::Path(PathNode {
            path,
            fill: Some(Paint::Solid([(next() % 256) as u8, 140, 160, 255])),
            stroke,
        });
        if scene.add_node(None, format!("s{i}"), content).is_err() {
            break;
        }
    }
    scene
}

fn bench_render_scene(c: &mut Criterion) {
    let scene = bench_scene();
    let viewport = Viewport {
        zoom: 1.0,
        pan: Vec2::ZERO,
    };
    let opts = RenderOpts {
        selection: Vec::new(),
        show_grid: false,
        ..Default::default()
    };
    c.bench_function("render/scene_200_shapes_96px", |b| {
        b.iter(|| {
            let mut renderer = CpuRenderer::new(CANVAS, CANVAS, [250, 250, 250, 255]);
            render_scene(&scene, &viewport, renderer.sink(), &opts);
            black_box(renderer.finish())
        })
    });
}

// criterion 0.5 的 criterion_group!/criterion_main!(docs.rs 惯用形态;
// criterion 未在本机 registry,API 按其 0.3→0.5 稳定签名保守书写——见任务报告)。
criterion_group!(benches, bench_render_scene);
criterion_main!(benches);
