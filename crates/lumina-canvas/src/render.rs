//! 场景渲染调度:场景图 → [`PaintSink`] 指令流(源自 docs/02 §4.2/§4.3/§7.2)。
//!
//! 上层只面向 `&mut dyn PaintSink` 编程,不感知后端:GPU(vello)与
//! CPU(vello_cpu)各给一份 Sink 实现,降级 = 换 Sink,本模块零改动。
//!
//! # 描边宽度的对原文修正(重要)
//!
//! docs/02 §4.2 代码示例把形状描边写成 `width / zoom`(屏幕等宽)。**这是
//! 原文的笔误**:设计软件的形状描边是世界属性,放大视图时描边应随之变粗
//! (Illustrator/Figma 行为)。本实现把 [`StrokeStyle::width`] 按世界线宽
//! **原样**传给 sink,由 `viewport × 节点` 变换自然带出缩放;`/zoom` 屏幕等宽
//! 只适用于**覆盖层**(选中框/网格/手柄/参考线)。
//!
//! # LOD(细节层次,docs/02 §7.2)
//!
//! 屏幕尺寸 < 4px 的对象降级为包围盒色块(跳过描边),< 1px 完全跳过,
//! 判定见 [`crate::lod`]。
//!
//! # 坐标纪律
//!
//! 一切输入输出均为 f64 世界坐标;传给 sink 的 transform 是
//! `viewport × 节点世界变换`(f64 仿射),f32 降级只发生在后端内部。

use kurbo::{Affine, Point, Rect, Shape};
use lumina_core::scene::{NodeContent, NodeId, Paint, Rgba8, Scene, StrokeStyle};
use lumina_core::viewport::Viewport;
use lumina_paint::sink::PaintSink;

use crate::grid;
use crate::lod::{self, DetailLevel};

/// 选中包围盒的屏幕线宽(px,docs/02 §4.2:任意缩放下 1.5px)。
pub const SELECTION_STROKE_PX: f64 = 1.5;
/// 控制柄方块的屏幕边长(px,docs/02 §9:8 向控制柄 6px)。
pub const HANDLE_SIZE_PX: f64 = 6.0;
/// 控制柄描边的屏幕线宽(px)。
pub const HANDLE_STROKE_PX: f64 = 1.0;

/// 覆盖层主题色(选中框/网格/参考线/锚点)。
///
/// **默认值仅供无 token 场景(测试/无头渲染);UI 侧应从 lumina-widgets
/// tokens 取主题色**(docs/03 §5 `CanvasTheme`),并在 `RenderOpts` 里传入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayTheme {
    /// 选中高亮(docs/02 §4.2 的 `#4f9fff`)
    pub selection: Rgba8,
    /// 网格线
    pub grid: Rgba8,
    /// 参考线(经典青,docs/03 §5 的 `#00c8ff`)
    pub guide: Rgba8,
    /// 锚点/控制柄
    pub anchor: Rgba8,
}

impl Default for OverlayTheme {
    fn default() -> Self {
        OverlayTheme {
            selection: [0x4f, 0x9f, 0xff, 0xff],
            grid: grid::GRID_COLOR,
            guide: [0x00, 0xc8, 0xff, 0xff],
            anchor: [0xff, 0xff, 0xff, 0xff],
        }
    }
}

/// 渲染选项:选中集、网格开关与覆盖层主题。
///
/// 与场景数据无关,逐帧可变;`screen_size` 是**相对手册补充的字段**(见下)。
#[derive(Clone, Debug, Default)]
pub struct RenderOpts {
    /// 选中节点(画包围盒 + 控制柄)
    pub selection: Vec<NodeId>,
    /// 是否画背景网格
    pub show_grid: bool,
    /// 覆盖层主题色
    pub overlay: OverlayTheme,
    /// 渲染目标的屏幕尺寸(px),用于视锥剔除与网格范围。
    ///
    /// **契约补充说明**:sink 抽象没有"画布多大"的查询能力,而剔除与网格
    /// 都需要目标尺寸,故挂进 opts。`(0.0, 0.0)`(Default)表示"尺寸未知",
    /// 此时**不做剔除**也不画网格(保守行为,绝不把可见对象剔掉)。
    pub screen_size: (f64, f64),
}

/// 视口可见区域对应的世界矩形(视锥剔除与网格的判定范围)。
pub fn visible_world_rect(viewport: &Viewport, screen_size: (f64, f64)) -> Rect {
    let to_world = viewport.viewport_to_world();
    let top_left = to_world * Point::new(0.0, 0.0);
    let bottom_right = to_world * Point::new(screen_size.0, screen_size.1);
    Rect::from_points(top_left, bottom_right)
}

/// 渲染一帧:场景对象(底→顶)→ 覆盖层(网格 → 选中框 → 控制柄)。
///
/// # 与手册的两处差异(均在模块 doc 说明)
/// 1. 形状描边用世界线宽原样传入(docs/02 §4.2 修正);
/// 2. 橡皮筋等**交互预览**不在本函数绘制——预览状态在工具里,走
///    `ToolBehavior::preview`(docs/03 §4.2 的预览层;本函数是纯场景函数,
///    不持有工具状态)。
pub fn render_scene(
    scene: &Scene,
    viewport: &Viewport,
    sink: &mut dyn PaintSink,
    opts: &RenderOpts,
) {
    let vp = viewport.world_to_viewport();
    let size_known = opts.screen_size.0 > 0.0 && opts.screen_size.1 > 0.0;
    let visible = size_known.then(|| visible_world_rect(viewport, opts.screen_size));

    // 网格是背景层:先画(在一切对象之下)
    if opts.show_grid {
        if let Some(visible) = visible {
            grid::draw_grid(sink, viewport, visible);
        }
    }

    // 场景对象:render_list 已按底→顶排序、剪枝不可见子树(docs/02 §3)
    for (id, world_xform) in scene.render_list() {
        let Some(node) = scene.node(id) else {
            continue;
        };
        // 组自身无内容(子节点是 render_list 的独立条目);内容包围盒缺失同理
        let Some(world_bbox) = scene.node_world_bbox(id) else {
            continue;
        };

        // 视锥剔除:包围盒经视口变换后与可见世界矩形不相交 → 跳过
        if let Some(visible) = visible {
            if !rects_intersect(world_bbox, visible) {
                continue;
            }
        }

        let NodeContent::Path(path_node) = &node.content else {
            // Text/Image 的真实绘制 = M2(vello draw_glyphs / 资产管线);
            // Group 无直接内容。
            continue;
        };

        // 传给 sink 的变换 = 视口 × 节点世界变换(f64,docs/02 §4.2 的 `total`)
        let total = vp * world_xform;

        // LOD(docs/02 §7.2):以包围盒短边为特征尺寸
        let feature_size = world_bbox.width().min(world_bbox.height());
        match lod::detail_level(feature_size, viewport.zoom) {
            DetailLevel::Point => continue, // 屏幕上不足 1px,跳过
            DetailLevel::Silhouette => {
                // 降级为包围盒色块:保留体量感,跳过描边等细节
                if let Some(fill) = &path_node.fill {
                    let silhouette = world_bbox.to_path(0.1);
                    sink.fill_with_opacity(fill, node.opacity, total, &silhouette);
                }
            }
            DetailLevel::Full => {
                if let Some(fill) = &path_node.fill {
                    sink.fill_with_opacity(fill, node.opacity, total, &path_node.path);
                }
                // 世界线宽原样传入:随视图缩放(模块 doc "对原文的修正")
                if let Some(stroke) = &path_node.stroke {
                    sink.stroke(stroke, total, &path_node.path);
                }
            }
        }
    }

    // 覆盖层:选中包围盒(1.5px 屏幕等宽)+ 8 向控制柄(6px 方块)
    if opts.selection.is_empty() {
        return;
    }
    let mut selection_union: Option<Rect> = None;
    for &id in &opts.selection {
        let Some(bbox) = scene.node_world_bbox(id) else {
            continue;
        };
        selection_union = Some(match selection_union {
            Some(acc) => acc.union(bbox),
            None => bbox,
        });
        let style = StrokeStyle {
            paint: Paint::Solid(opts.overlay.selection),
            width: SELECTION_STROKE_PX / viewport.zoom,
        };
        sink.stroke(&style, vp, &bbox.to_path(0.1));
    }
    if let Some(bbox) = selection_union {
        draw_handles(sink, vp, bbox, viewport.zoom, &opts.overlay);
    }
}

/// 在包围盒四角 + 四边中点画 8 向控制柄(6px/zoom 方块,docs/02 §9)。
fn draw_handles(sink: &mut dyn PaintSink, vp: Affine, bbox: Rect, zoom: f64, theme: &OverlayTheme) {
    let half = HANDLE_SIZE_PX / (2.0 * zoom); // 世界坐标下的半边长(屏幕 3px)
    let fill = Paint::Solid(theme.anchor);
    let outline = StrokeStyle {
        paint: Paint::Solid(theme.selection),
        width: HANDLE_STROKE_PX / zoom,
    };
    for point in handle_positions(bbox) {
        let rect = Rect::new(
            point.x - half,
            point.y - half,
            point.x + half,
            point.y + half,
        );
        let path = rect.to_path(0.1);
        sink.fill(&fill, vp, &path);
        sink.stroke(&outline, vp, &path);
    }
}

/// 8 向控制柄的中心点:四角 + 四边中点。
fn handle_positions(bbox: Rect) -> [Point; 8] {
    let cx = (bbox.x0 + bbox.x1) / 2.0;
    let cy = (bbox.y0 + bbox.y1) / 2.0;
    [
        Point::new(bbox.x0, bbox.y0),
        Point::new(cx, bbox.y0),
        Point::new(bbox.x1, bbox.y0),
        Point::new(bbox.x1, cy),
        Point::new(bbox.x1, bbox.y1),
        Point::new(cx, bbox.y1),
        Point::new(bbox.x0, bbox.y1),
        Point::new(bbox.x0, cy),
    ]
}

/// 开区间相交测试(公共边相切不算可见重叠,剔除更激进)。
fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::BezPath;
    use lumina_core::scene::PathNode;

    /// 记录型 sink:数 fill/stroke 调用,存下变换与路径包围盒供断言。
    struct RecordingSink {
        fills: Vec<(Rgba8, Affine, Rect)>,
        strokes: Vec<(Rgba8, f64, Affine, Rect)>,
    }

    impl RecordingSink {
        fn new() -> Self {
            RecordingSink {
                fills: Vec::new(),
                strokes: Vec::new(),
            }
        }

        fn paint_solid(paint: &Paint) -> Rgba8 {
            match paint {
                Paint::Solid(c) => *c,
                _ => [0, 0, 0, 0],
            }
        }
    }

    impl PaintSink for RecordingSink {
        fn fill(&mut self, paint: &Paint, transform: Affine, path: &BezPath) {
            self.fills
                .push((Self::paint_solid(paint), transform, path.bounding_box()));
        }

        fn stroke(&mut self, style: &StrokeStyle, transform: Affine, path: &BezPath) {
            self.strokes.push((
                Self::paint_solid(&style.paint),
                style.width,
                transform,
                path.bounding_box(),
            ));
        }
    }

    fn rect_content(x0: f64, y0: f64, x1: f64, y1: f64, color: Rgba8) -> NodeContent {
        NodeContent::Path(PathNode {
            path: kurbo::Rect::new(x0, y0, x1, y1).to_path(0.1),
            fill: Some(Paint::Solid(color)),
            stroke: None,
        })
    }

    fn viewport_at_origin(zoom: f64) -> Viewport {
        Viewport {
            zoom,
            pan: kurbo::Vec2::ZERO,
        }
    }

    fn red() -> Rgba8 {
        [255, 0, 0, 255]
    }

    fn blue() -> Rgba8 {
        [0, 0, 255, 255]
    }

    // —— 逐像素回归(默认 feature "cpu" 下运行)——

    #[cfg(feature = "cpu")]
    mod pixel {
        use super::*;
        use lumina_paint::cpu::CpuRenderer;

        const WHITE: Rgba8 = [255, 255, 255, 255];

        fn pixel(buf: &[u8], x: u16, y: u16) -> [u8; 4] {
            let i = 4 * (usize::from(y) * 64 + usize::from(x));
            [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
        }

        /// 两矩形场景 → CpuRenderer 64×64:两区域像素颜色正确(docs/02 §9 回归)。
        #[test]
        fn two_rects_render_expected_pixels() {
            let mut scene = Scene::new();
            scene
                .add_node(None, "红", rect_content(0.0, 0.0, 16.0, 16.0, red()))
                .expect("红矩形");
            scene
                .add_node(None, "蓝", rect_content(48.0, 0.0, 64.0, 16.0, blue()))
                .expect("蓝矩形");

            let mut renderer = CpuRenderer::new(64, 64, WHITE);
            let opts = RenderOpts {
                show_grid: false,
                screen_size: (64.0, 64.0),
                ..RenderOpts::default()
            };
            render_scene(&scene, &viewport_at_origin(1.0), renderer.sink(), &opts);
            let buf = renderer.finish();

            assert_eq!(pixel(&buf, 8, 8), red(), "红矩形内部(世界=屏幕,zoom=1)");
            assert_eq!(pixel(&buf, 56, 8), blue(), "蓝矩形内部");
            assert_eq!(pixel(&buf, 32, 32), WHITE, "两矩形之间应保持底色");
        }

        /// 节点 opacity 应生效(半透明红叠白底 → 约 50% 红)。
        #[test]
        fn node_opacity_reaches_pixels() {
            let mut scene = Scene::new();
            let id = scene
                .add_node(
                    None,
                    "半透明红",
                    rect_content(16.0, 16.0, 48.0, 48.0, red()),
                )
                .expect("节点");
            scene.node_mut(id).expect("在").opacity = 0.5;

            let mut renderer = CpuRenderer::new(64, 64, WHITE);
            let opts = RenderOpts {
                screen_size: (64.0, 64.0),
                ..RenderOpts::default()
            };
            render_scene(&scene, &viewport_at_origin(1.0), renderer.sink(), &opts);
            let buf = renderer.finish();

            let p = pixel(&buf, 32, 32);
            assert_eq!(p[0], 255, "红通道满(与白底混合)");
            assert!(
                (128i32 - i32::from(p[1])).abs() <= 2,
                "g ≈ 128(白底混入一半),实际 {p:?}"
            );
        }

        /// 视锥剔除的像素面验证:视口外的对象不出现在像素里。
        #[test]
        fn offscreen_object_not_rasterized() {
            let mut scene = Scene::new();
            scene
                .add_node(
                    None,
                    "远处的红",
                    rect_content(100.0, 100.0, 140.0, 140.0, red()),
                )
                .expect("节点");
            let mut renderer = CpuRenderer::new(64, 64, WHITE);
            let opts = RenderOpts {
                screen_size: (64.0, 64.0),
                ..RenderOpts::default()
            };
            render_scene(&scene, &viewport_at_origin(1.0), renderer.sink(), &opts);
            let buf = renderer.finish();
            assert_eq!(pixel(&buf, 32, 32), WHITE, "画布全白:对象在视口外被剔除");
        }
    }

    // —— 剔除/覆盖层/LOD(RecordingSink,不依赖后端)——

    #[test]
    fn object_outside_viewport_produces_zero_fill_calls() {
        let mut scene = Scene::new();
        scene
            .add_node(
                None,
                "视口外",
                rect_content(1000.0, 1000.0, 1100.0, 1100.0, red()),
            )
            .expect("节点");

        let mut sink = RecordingSink::new();
        let opts = RenderOpts {
            screen_size: (64.0, 64.0), // 可见世界区 ≈ (0,0)-(64,64)
            ..RenderOpts::default()
        };
        render_scene(&scene, &viewport_at_origin(1.0), &mut sink, &opts);
        assert_eq!(sink.fills.len(), 0, "视口外对象不得产生 fill 调用");
        assert_eq!(sink.strokes.len(), 0);
    }

    #[test]
    fn partially_visible_object_is_kept() {
        let mut scene = Scene::new();
        // 只露出 (60,0)-(64,10) 一角,也应绘制
        scene
            .add_node(None, "半露", rect_content(60.0, 0.0, 90.0, 10.0, red()))
            .expect("节点");
        let mut sink = RecordingSink::new();
        let opts = RenderOpts {
            screen_size: (64.0, 64.0),
            ..RenderOpts::default()
        };
        render_scene(&scene, &viewport_at_origin(1.0), &mut sink, &opts);
        assert_eq!(sink.fills.len(), 1, "与可见区相交的对象必须绘制");
    }

    #[test]
    fn unknown_screen_size_disables_culling() {
        let mut scene = Scene::new();
        scene
            .add_node(
                None,
                "视口外",
                rect_content(1000.0, 1000.0, 1100.0, 1100.0, red()),
            )
            .expect("节点");
        let mut sink = RecordingSink::new();
        // screen_size = (0,0) = 未知 → 保守不剔除
        render_scene(
            &scene,
            &viewport_at_origin(1.0),
            &mut sink,
            &RenderOpts::default(),
        );
        assert_eq!(sink.fills.len(), 1, "尺寸未知时不得误剔");
    }

    #[test]
    fn selection_draws_at_least_one_stroke_with_screen_constant_width() {
        let mut scene = Scene::new();
        let id = scene
            .add_node(None, "矩形", rect_content(0.0, 0.0, 10.0, 10.0, red()))
            .expect("节点");

        let mut sink = RecordingSink::new();
        let opts = RenderOpts {
            selection: vec![id],
            screen_size: (64.0, 64.0),
            ..RenderOpts::default()
        };
        render_scene(&scene, &viewport_at_origin(2.0), &mut sink, &opts);

        // stroke 调用 ≥ 1:选中框 + 8 控制柄描边
        assert!(!sink.strokes.is_empty(), "selection 非空时必须画选中框");
        // 第一条 = 选中框:线宽 1.5/zoom(屏幕等宽),路径 = 节点世界包围盒
        let (_, width, transform, bbox) = sink.strokes[0];
        assert!(
            (width - 1.5 / 2.0).abs() < 1e-9,
            "选中框世界线宽应为 1.5/zoom"
        );
        assert_eq!(transform, viewport_at_origin(2.0).world_to_viewport());
        assert!((bbox.x0 - 0.0).abs() < 1e-6 && (bbox.x1 - 10.0).abs() < 1e-6);
        // 8 个控制柄 = 8 次 fill(方块)+ 8 次描边;另有场景对象自身的 1 次 fill
        assert_eq!(sink.fills.len(), 9, "场景矩形 1 + 8 向控制柄方块");
        assert_eq!(sink.strokes.len(), 9, "选中框 1 + 控制柄描边 8");
        // 控制柄方块屏幕尺寸:世界边长 6/zoom = 3px(fills[0] 是场景矩形)
        let (_, _, handle_bbox) = sink.fills[1];
        assert!(
            ((handle_bbox.width()) * 2.0 - 6.0).abs() < 1e-9,
            "手柄应为 6px/zoom 方块"
        );
    }

    #[test]
    fn empty_selection_draws_no_overlay() {
        let mut scene = Scene::new();
        scene
            .add_node(None, "矩形", rect_content(0.0, 0.0, 10.0, 10.0, red()))
            .expect("节点");
        let mut sink = RecordingSink::new();
        render_scene(
            &scene,
            &viewport_at_origin(1.0),
            &mut sink,
            &RenderOpts::default(),
        );
        assert_eq!(sink.fills.len(), 1, "只有场景矩形本身");
        assert_eq!(sink.strokes.len(), 0, "无选中不画覆盖层");
    }

    #[test]
    fn tiny_object_degrades_to_silhouette_then_skipped() {
        let mut scene = Scene::new();
        scene
            .add_node(None, "小方块", rect_content(0.0, 0.0, 10.0, 10.0, red()))
            .expect("节点");

        // zoom=0.3:屏幕 3px → Silhouette:fill 画的是包围盒(而非原路径细节),无 stroke
        let mut sink = RecordingSink::new();
        let opts = RenderOpts {
            screen_size: (64.0, 64.0),
            ..RenderOpts::default()
        };
        render_scene(&scene, &viewport_at_origin(0.3), &mut sink, &opts);
        assert_eq!(sink.fills.len(), 1, "降级为包围盒色块");
        let (_, _, bbox) = sink.fills[0];
        assert!(
            (bbox.width() - 10.0).abs() < 1e-6 && (bbox.height() - 10.0).abs() < 1e-6,
            "轮廓降级 = 世界包围盒色块"
        );

        // zoom=0.05:屏幕 0.5px → Point:完全跳过
        let mut sink = RecordingSink::new();
        render_scene(&scene, &viewport_at_origin(0.05), &mut sink, &opts);
        assert_eq!(sink.fills.len(), 0, "屏幕不足 1px 不画");

        // zoom=1:屏幕 10px → Full:fill + (本例无 stroke)
        let mut sink = RecordingSink::new();
        render_scene(&scene, &viewport_at_origin(1.0), &mut sink, &opts);
        assert_eq!(sink.fills.len(), 1);
    }

    #[test]
    fn stroke_width_passes_through_in_world_units() {
        // 对原文的修正回归:形状描边不除 zoom(世界线宽原样),随视图缩放
        let mut scene = Scene::new();
        let mut path = kurbo::Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1);
        path.close_path();
        scene
            .add_node(
                None,
                "描边矩形",
                NodeContent::Path(PathNode {
                    path,
                    fill: None,
                    stroke: Some(StrokeStyle {
                        paint: Paint::Solid(blue()),
                        width: 2.0,
                    }),
                }),
            )
            .expect("节点");

        for zoom in [0.5, 1.0, 4.0] {
            let mut sink = RecordingSink::new();
            let opts = RenderOpts {
                screen_size: (64.0, 64.0),
                ..RenderOpts::default()
            };
            render_scene(&scene, &viewport_at_origin(zoom), &mut sink, &opts);
            let (_, width, _, _) = sink.strokes[0];
            assert!(
                (width - 2.0).abs() < 1e-9,
                "zoom={zoom}:形状描边应保持世界线宽 2.0,实际 {width}"
            );
        }
    }

    #[test]
    fn visible_world_rect_matches_viewport_math() {
        let vp = viewport_at_origin(2.0);
        let visible = visible_world_rect(&vp, (100.0, 50.0));
        assert_eq!(
            visible,
            Rect::new(0.0, 0.0, 50.0, 25.0),
            "zoom=2 → 世界范围减半"
        );

        let panned = Viewport {
            zoom: 1.0,
            pan: kurbo::Vec2::new(30.0, -10.0),
        };
        let visible = visible_world_rect(&panned, (100.0, 100.0));
        // screen_to_world = (screen - pan)/zoom:y1 = (100-(-10))/1 = 110
        assert_eq!(visible, Rect::new(-30.0, 10.0, 70.0, 110.0));
    }
}
