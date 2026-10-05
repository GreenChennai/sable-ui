//! 渲染回归基线(迭代计划 08 S3 #3.5;V4.0 T2.3 增效果场景):vello_cpu
//! 离屏渲染 6 个确定性场景,与 `tests/golden/{name}.png` 黄金文件逐像素对比。
//!
//! # 生成/更新基线(主 Agent 跑一次并提交 PNG)
//!
//! ```bash
//! SABLE_UPDATE_GOLDENS=1 cargo test -p sable-canvas --test golden
//! ```
//!
//! 生成后把 `crates/sable-canvas/tests/golden/*.png` 随代码一起提交;CI 不设
//! 该变量,只做对比(缺文件时守卫测试 panic 并给出上述命令)。
//!
//! # 确定性纪律
//!
//! 场景全部为**纯几何**(零文本——字体光栅化不可靠;零随机、零时钟、零
//! sleep):96×96 固定尺寸、固定底色、固定 zoom/pan、固定颜色。混合一律
//! Normal——效果管线并行迭代给 Node 加 blend_mode 期间,本文件不依赖任何
//! 非默认混合。场景只经 `Scene::add_node` 构造(不手写 `Node` 字面量),
//! 节点字段演进不破基线脚本。
//!
//! # 对比容差(08 号计划 S3 #3.5:±0.1%)
//!
//! 单通道差 ≤ 2(吸收 vello_cpu 边缘抗锯齿在 u8 量化上的 ±1 舍入)的像素
//! 视为匹配;不匹配像素 ≤ 总数 0.1%(96×96 = 9216 像素 → 最多 9 个)视为
//! 通过。缓冲与 PNG 均为 vello_cpu 的预乘 RGBA8,两侧同语义,直接比字节。
#![cfg(feature = "cpu")]

use std::env;
use std::fs;
use std::path::PathBuf;

use kurbo::{Affine, Rect, Shape};
use sable_canvas::render::{OverlayTheme, RenderOpts, render_scene};
use sable_foundation::effects::{EffectEntry, EffectSpec};
use sable_foundation::scene::{NodeContent, NodeId, Paint, PathNode, Scene, StrokeStyle};
use sable_foundation::viewport::Viewport;
use sable_paint::cpu::CpuRenderer;
use sable_paint::sink::PaintSink;

/// 画布边长(px)。世界坐标 = 屏幕坐标(zoom = 1、pan = 0 时)。
const SIZE: u16 = 96;
/// 底色(不透明白,与 CpuRenderer 既有测试一致)。
const WHITE: [u8; 4] = [255, 255, 255, 255];
/// 单通道容差:|a-b| ≤ 2(抗锯齿 u8 量化舍入)。
const CHANNEL_TOLERANCE: i32 = 2;
/// 不匹配像素比例的分母:上限 = 总数 / 本值(1000 → 0.1%)。
const MISMATCH_DENOMINATOR: usize = 1000;

/// 六个黄金场景名(与下方 #[test] 一一对应;守卫测试按名查文件)。
const SCENARIO_NAMES: [&str; 6] = [
    "grid_rects",
    "selection",
    "rubber_band",
    "lod_zoomout",
    "opacity_stack",
    "effects_shadow",
];

/// 黄金基线路径:`crates/sable-canvas/tests/golden/{name}.png`。
fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(format!("{name}.png"))
}

/// 基线生成模式:`SABLE_UPDATE_GOLDENS=1`。
fn update_goldens_requested() -> bool {
    env::var("SABLE_UPDATE_GOLDENS").as_deref() == Ok("1")
}

fn viewport(zoom: f64) -> Viewport {
    Viewport {
        zoom,
        pan: kurbo::Vec2::ZERO,
    }
}

/// 追加一个实心填充矩形(走 add_node,不手写 Node 字面量)。
fn solid_rect(scene: &mut Scene, name: &str, rect: Rect, color: [u8; 4]) -> NodeId {
    scene
        .add_node(
            None,
            name,
            NodeContent::Path(PathNode {
                path: rect.to_path(0.1),
                fill: Some(Paint::Solid(color)),
                stroke: None,
            }),
        )
        .expect("添加矩形节点")
}

/// 渲染整帧:固定 96×96 白底,返回预乘 RGBA8 缓冲。
fn render_frame(scene: &Scene, viewport: Viewport, opts: &RenderOpts) -> Vec<u8> {
    let mut renderer = CpuRenderer::new(SIZE, SIZE, WHITE);
    render_scene(scene, &viewport, renderer.sink(), opts);
    renderer.finish()
}

fn encode_png(buf: &[u8]) -> Vec<u8> {
    let img = image::RgbaImage::from_raw(u32::from(SIZE), u32::from(SIZE), buf.to_vec())
        .expect("缓冲长度应为 width*height*4");
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("PNG 编码");
    out
}

/// 与黄金基线逐像素对比;`SABLE_UPDATE_GOLDENS=1` 时改为写入基线。
fn assert_matches_golden(name: &str, buf: &[u8]) {
    let path = golden_path(name);
    if update_goldens_requested() {
        let dir = path.parent().expect("golden 路径必有父目录");
        fs::create_dir_all(dir).expect("建 golden 目录");
        fs::write(&path, encode_png(buf)).expect("写黄金基线 PNG");
        return;
    }
    if !path.exists() {
        panic!(
            "缺少黄金基线 {}。请运行 `SABLE_UPDATE_GOLDENS=1 cargo test -p sable-canvas --test golden` 生成后随代码提交。",
            path.display()
        );
    }
    let golden = image::open(&path).expect("读黄金基线 PNG").to_rgba8();
    assert_eq!(
        (golden.width(), golden.height()),
        (u32::from(SIZE), u32::from(SIZE)),
        "{name}:黄金基线尺寸与本渲染不一致"
    );

    let total = usize::from(SIZE) * usize::from(SIZE);
    let mut mismatch_count = 0usize;
    let mut first_mismatch: Option<(u32, u32, [u8; 4], [u8; 4])> = None;
    for (index, (got, want)) in buf
        .as_chunks::<4>()
        .0
        .iter()
        .zip(golden.pixels())
        .enumerate()
    {
        let mismatched = got
            .iter()
            .zip(want.0.iter())
            .any(|(a, b)| (i32::from(*a) - i32::from(*b)).abs() > CHANNEL_TOLERANCE);
        if mismatched {
            mismatch_count += 1;
            if first_mismatch.is_none() {
                let x = u32::try_from(index % usize::from(SIZE)).expect("像素下标非负");
                let y = u32::try_from(index / usize::from(SIZE)).expect("像素下标非负");
                first_mismatch = Some((x, y, [got[0], got[1], got[2], got[3]], want.0));
            }
        }
    }
    assert!(
        mismatch_count * MISMATCH_DENOMINATOR <= total,
        "{name}:不匹配像素 {mismatch_count}/{total}(超过 0.1% 上限);首个差异像素 {:?}",
        first_mismatch
    );
}

/// 场景 1:三个矩形 + 背景网格(网格层在最下,自适应步长由 zoom 决定)。
#[test]
fn grid_rects_matches_golden() {
    let mut scene = Scene::new();
    solid_rect(
        &mut scene,
        "红",
        Rect::new(8.0, 8.0, 32.0, 32.0),
        [200, 40, 40, 255],
    );
    solid_rect(
        &mut scene,
        "绿",
        Rect::new(40.0, 16.0, 64.0, 48.0),
        [40, 180, 70, 255],
    );
    solid_rect(
        &mut scene,
        "蓝",
        Rect::new(56.0, 56.0, 88.0, 88.0),
        [50, 90, 220, 255],
    );
    let opts = RenderOpts {
        show_grid: true,
        overlay: OverlayTheme::default(),
        screen_size: (f64::from(SIZE), f64::from(SIZE)),
        ..RenderOpts::default()
    };
    let buf = render_frame(&scene, viewport(1.0), &opts);
    assert_matches_golden("grid_rects", &buf);
}

/// 场景 2:选中框(1.5px 屏幕等宽)+ 8 向控制柄(6px 方块带描边)。
#[test]
fn selection_matches_golden() {
    let mut scene = Scene::new();
    let id = solid_rect(
        &mut scene,
        "选中矩形",
        Rect::new(24.0, 24.0, 72.0, 72.0),
        [230, 130, 30, 255],
    );
    let opts = RenderOpts {
        selection: vec![id],
        show_grid: false,
        overlay: OverlayTheme::default(),
        screen_size: (f64::from(SIZE), f64::from(SIZE)),
        effect_level: None,
        effects_cache: None, // PERF-05 关:基线确定性(行为与无缓存路径逐位一致)
    };
    let buf = render_frame(&scene, viewport(1.0), &opts);
    assert_matches_golden("selection", &buf);
}

/// 场景 3:橡皮筋预览。
///
/// `render_scene` 是纯场景函数,不画工具预览(预览层在 `ToolBehavior` 里,
/// 08 号计划 3.1/3.2 并行开发中)。此处按 docs/03 §4.2 的视觉约定**直接向
/// sink 画两个半透明矩形**模拟"选区蒙层 + 预览形状"的合成结果;预览层
/// 实现完成后,以其像素输出替换本场景的构造方式(基线文件名不变)。
#[test]
fn rubber_band_matches_golden() {
    // 预览层实现完成后,用真实 SelectTool::preview 输出替换直接绘制(见函数 doc)
    let mut renderer = CpuRenderer::new(SIZE, SIZE, WHITE);
    let sink = renderer.sink();
    let marquee = Rect::new(16.0, 20.0, 64.0, 60.0).to_path(0.1);
    let preview = Rect::new(40.0, 40.0, 84.0, 84.0).to_path(0.1);
    // 选区蒙层:25% 蓝 + 1px 边
    sink.fill_with_opacity(
        &Paint::Solid([70, 130, 240, 255]),
        0.25,
        Affine::IDENTITY,
        &marquee,
    );
    sink.stroke(
        &StrokeStyle {
            paint: Paint::Solid([40, 90, 200, 255]),
            width: 1.0,
        },
        Affine::IDENTITY,
        &marquee,
    );
    // 预览形状:40% 绿(与蒙层交叠处颜色更深,半透明合成路径进基线)
    sink.fill_with_opacity(
        &Paint::Solid([60, 200, 90, 255]),
        0.40,
        Affine::IDENTITY,
        &preview,
    );
    let buf = renderer.finish();
    assert_matches_golden("rubber_band", &buf);
}

/// 场景 4:LOD 降级(zoom = 0.05,docs/02 §7.2)。同一帧覆盖三档:
/// - 200 世界单位 → 屏幕 10px → Full(fill + stroke 原样);
/// - 60 世界单位 → 屏幕 3px → Silhouette(包围盒色块,跳过描边);
/// - 10 世界单位 → 屏幕 0.5px → Point(完全跳过)。
#[test]
fn lod_zoomout_matches_golden() {
    let mut scene = Scene::new();
    scene
        .add_node(
            None,
            "大",
            NodeContent::Path(PathNode {
                path: Rect::new(20.0, 20.0, 220.0, 220.0).to_path(0.1),
                fill: Some(Paint::Solid([200, 60, 60, 255])),
                stroke: Some(StrokeStyle {
                    paint: Paint::Solid([40, 40, 40, 255]),
                    width: 8.0,
                }),
            }),
        )
        .expect("大矩形");
    solid_rect(
        &mut scene,
        "中",
        Rect::new(300.0, 40.0, 360.0, 100.0),
        [60, 160, 220, 255],
    );
    solid_rect(
        &mut scene,
        "小1",
        Rect::new(500.0, 40.0, 510.0, 50.0),
        [20, 20, 20, 255],
    );
    solid_rect(
        &mut scene,
        "小2",
        Rect::new(520.0, 60.0, 530.0, 70.0),
        [20, 20, 20, 255],
    );
    let opts = RenderOpts {
        show_grid: false,
        overlay: OverlayTheme::default(),
        screen_size: (f64::from(SIZE), f64::from(SIZE)),
        ..RenderOpts::default()
    };
    let buf = render_frame(&scene, viewport(0.05), &opts);
    assert_matches_golden("lod_zoomout", &buf);
}

/// 场景 5:三层半透明叠加(节点 opacity 走 render_scene → fill_with_opacity,
/// 叠加次序与合成结果进基线;混合均为 Normal)。
#[test]
fn opacity_stack_matches_golden() {
    let mut scene = Scene::new();
    let bottom = solid_rect(
        &mut scene,
        "底层红",
        Rect::new(8.0, 8.0, 64.0, 64.0),
        [220, 50, 50, 255],
    );
    let middle = solid_rect(
        &mut scene,
        "中层绿",
        Rect::new(32.0, 20.0, 88.0, 76.0),
        [50, 190, 80, 255],
    );
    let top = solid_rect(
        &mut scene,
        "顶层蓝",
        Rect::new(20.0, 48.0, 76.0, 88.0),
        [60, 90, 230, 255],
    );
    scene.node_mut(bottom).expect("底层在").opacity = 0.7;
    scene.node_mut(middle).expect("中层在").opacity = 0.55;
    scene.node_mut(top).expect("顶层在").opacity = 0.4;
    let opts = RenderOpts {
        show_grid: false,
        overlay: OverlayTheme::default(),
        screen_size: (f64::from(SIZE), f64::from(SIZE)),
        ..RenderOpts::default()
    };
    let buf = render_frame(&scene, viewport(1.0), &opts);
    assert_matches_golden("opacity_stack", &buf);
}

/// 场景 6:节点效果栈(V4.0 T2.3,G20)——蓝矩形 + 投影(blur 4、
/// offset [+6,+6]、α=150 深蓝黑),混合保持 Normal。
///
/// 走 `render_scene` 效果路径:节点单独离屏光栅化 → `apply_effects_rgba`
/// → `draw_rgba` 回贴(CpuRenderer 声明 `supports_draw_rgba`,档位 = env
/// 未设 → cpu-only 编译期默认 Reduced,blur 开)。投影落向右下方,支撑域
/// (3×box 半径 = 6px)与偏移的合成扩散进基线。
#[test]
fn effects_shadow_matches_golden() {
    let mut scene = Scene::new();
    let id = solid_rect(
        &mut scene,
        "投影矩形",
        Rect::new(28.0, 28.0, 60.0, 60.0),
        [50, 90, 220, 255],
    );
    scene.node_mut(id).expect("节点在").effects = vec![EffectEntry {
        spec: EffectSpec::DropShadow {
            blur: 4.0,
            offset: [6.0, 6.0],
            color: [10, 10, 30, 150],
        },
        enabled: true,
    }];
    let opts = RenderOpts {
        show_grid: false,
        overlay: OverlayTheme::default(),
        screen_size: (f64::from(SIZE), f64::from(SIZE)),
        ..RenderOpts::default()
    };
    let buf = render_frame(&scene, viewport(1.0), &opts);
    assert_matches_golden("effects_shadow", &buf);
}

/// 基线存在守卫:文件缺失时 panic 并给出补救命令(生成方式见模块 doc)。
#[test]
fn golden_baselines_are_present() {
    if update_goldens_requested() {
        return; // 生成模式:本守卫让位于基线写入
    }
    for name in SCENARIO_NAMES {
        let path = golden_path(name);
        assert!(
            path.exists(),
            "缺少黄金基线 {};运行 `SABLE_UPDATE_GOLDENS=1 cargo test -p sable-canvas --test golden` 生成后随代码提交",
            path.display()
        );
    }
}
