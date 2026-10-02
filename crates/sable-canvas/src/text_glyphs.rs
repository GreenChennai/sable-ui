//! 画布真文本渲染(V3.0 T1 / G13):parley 布局 → 字形轮廓 → [`PaintSink`] 填充。
//!
//! # 实现档位(依赖源码核实结论,2026-10,registry 实证)
//!
//! - **首选档(vello_cpu 字形入口)——已核实存在,但本层不可达**:
//!   `vello_cpu 0.2.0` 的 `text` 模块提供 `GlyphRunBuilder`/`CpuGlyphRunBackend`
//!   (glifo 0.3 后端),`RenderContext` 实现 `glifo::GlyphRenderer`
//!   (`fill_glyphs`/`stroke_glyphs`),`Resources` 内建字形图集与图集页同步。
//!   但该入口要求**直接持有 `&mut RenderContext`**,而 sable-canvas 的渲染
//!   调度面向 `&mut dyn PaintSink`(GPU/CPU 两后端共用),`PaintSink` 尚无
//!   字形指令位——扩展 sink 属 sable-paint 所有权,本任务不越界(留 v3.1)。
//! - **本实现 = 回退档(真轮廓)**:parley 0.11 布局迭代(`lines()` →
//!   `items()` → `GlyphRun::positioned_glyphs()`)取得每字形 `(glyph_id, x, y)`,
//!   用 **skrifa 0.44**(parley 自身的依赖,Cargo.lock 实证同版本单拷贝)按
//!   字号无 hint 光栅化前取轮廓,经 `KurboPen` 适配为 `kurbo::BezPath`,
//!   走既有 `sink.fill`。CPU/GPU 两后端同一份代码,且天然兼容路径级效果
//!   (模糊等)与 SVG 导出(T1.3 的 text→轮廓化路径直接复用本模块产物)。
//! - 计划书的最后回退(色块占位)**未启用**:真轮廓档已可行。
//!
//! # 坐标系
//!
//! skrifa 轮廓按 `Size::new(font_size)` 缩放到"字号=px"单位、**y 轴向上**;
//! parley 定位字形在 **y 轴向下** 的布局空间。每个字形的放置变换 =
//! `transform × translate((glyph.x, glyph.y)) × y 翻转`(见 `FLIP_Y`)。
//!
//! # TD-10 布局缓存
//!
//! 缓存本体在 [`crate::text::TextPipeline`](键 `(text, font_size bits)`,颜色
//! 不入键;V4.0 T6.2 起条目上限 256 + LRU 淘汰,T6.3 起字号键控前钳制);
//! 本模块用 thread_local 管线提供任务书约定的无状态入口 [`layout_text`]、
//! 实测尺寸入口 [`measured_text_size`](T6.1,场景级消费者见
//! [`crate::text::node_world_bbox_measured`]),统计面 [`cache_stats`] /
//! [`reset_text_cache`]。每个测试线程各自一份管线,互不串扰。
//!
//! # 来源与许可
//!
//! 适配层代码为本仓库原创,针对 skrifa 0.44 `OutlinePen` 公开 trait 编写;
//! 迭代模式参照 linebender parley/vello 0.10 `Scene::draw_glyphs` 的
//! `lines → items → positioned_glyphs` 用法(Apache-2.0 OR MIT)。

use std::cell::RefCell;
use std::sync::Arc;

use kurbo::{Affine, BezPath};
use parley::{Layout, PositionedLayoutItem};
use sable_foundation::scene::{Paint, Rgba8};
use sable_paint::sink::PaintSink;
use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::text::TextPipeline;

/// 字体轮廓坐标系(y 轴向上)→ 画布坐标系(y 轴向下)的翻转。
const FLIP_Y: Affine = Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, 0.0]);

/// 一段文本的排版结果:parley [`Layout`] + 总尺寸 + 缓存来源标志。
///
/// `layout` 走 `Arc` 共享:TD-10 缓存命中时零拷贝、零重排。`color` 仅记录
/// 创建时颜色(缓存键不含颜色——同一文本不同颜色共享同一布局,绘制颜色
/// 以 [`draw_text`] 参数为准)。
#[derive(Clone)]
pub struct TextLayout {
    /// parley 排版产物(行 → 定位字形 run)。
    pub layout: Arc<Layout<()>>,
    /// 总宽度(世界坐标,f64;空文本为 0)。
    pub width: f64,
    /// 总高度(世界坐标,f64;空文本为 0)。
    pub height: f64,
    /// 创建时传入的颜色(信息性;绘制颜色见 [`draw_text`])。
    pub color: Rgba8,
    /// 本次获取是否命中 TD-10 布局缓存(观测面,供计数断言)。
    pub from_cache: bool,
}

thread_local! {
    /// 本线程共享的排版管线:`FontContext` 的系统字体发现只做一次,
    /// TD-10 布局缓存随管线存活(跨帧)。
    static PIPELINE: RefCell<TextPipeline> = RefCell::new(TextPipeline::new());
}

/// 排版一段文本(TD-10 缓存入口,任务书约定的无状态形态)。
///
/// - 缓存键 = `(text, font_size)`,命中时不重排;颜色不入键;字号键控前经
///   [`crate::text::normalize_font_size`] 钳制(T6.3:NaN/≤0 不产生独立键);
/// - 空文本短路为零尺寸布局(parley 对空串会排出带行高的空行,与
///   "空文本零覆盖"契约不符),不进缓存;
/// - `color` 记录进 [`TextLayout::color`](见上,不入键)。
pub fn layout_text(text: &str, font_size: f64, color: Rgba8) -> TextLayout {
    if text.is_empty() {
        return TextLayout {
            layout: Arc::new(Layout::new()),
            width: 0.0,
            height: 0.0,
            color,
            from_cache: false,
        };
    }
    let mut layout = PIPELINE.with(|cell| cell.borrow_mut().cached_layout(text, font_size));
    layout.color = color;
    layout
}

/// 实测文本布局尺寸 `(宽, 高)`(V4.0 T6.1):本线程管线的
/// [`crate::text::TextPipeline::measured_text_size`]。
///
/// 与 [`layout_text`] 同源同缓存(命中零重排);空文本 `(0.0, 0.0)`,不进
/// 缓存。命中测试/渲染的 Text 包围盒以本函数的实测口径替代 foundation 的
/// 0.6em/字符粗估。
pub fn measured_text_size(text: &str, font_size: f64) -> (f64, f64) {
    if text.is_empty() {
        return (0.0, 0.0);
    }
    PIPELINE.with(|cell| cell.borrow_mut().measured_text_size(text, font_size))
}

/// TD-10 缓存统计:`(命中次数, 未命中次数)`(本线程排版管线的累计值)。
pub fn cache_stats() -> (u64, u64) {
    PIPELINE.with(|cell| cell.borrow().cache_stats())
}

/// 清空布局缓存并归零计数器(测试隔离与内存压力逃生口)。
pub fn reset_text_cache() {
    PIPELINE.with(|cell| cell.borrow_mut().clear_cache());
}

/// 把一段排版好的文本绘入 sink:逐字形取轮廓 → `BezPath` → `sink.fill`。
///
/// - `transform`:世界 → 目标的仿射(调用方传 `视口 × 节点世界变换`);
/// - `color`:字形颜色(与布局缓存解耦,见 [`TextLayout::color`]);
/// - `cache_hits`/`cache_misses`:按本 `layout` 的缓存来源累加(恰好一次),
///   供调用方观测;全局累计见 [`cache_stats`]。
///
/// 字体数据不可读、字形无轮廓(空格)、轮廓绘制失败均**静默跳过**,
/// 不 panic、不报错(渲染路径不允许因脏数据中断)。
pub fn draw_text(
    sink: &mut dyn PaintSink,
    layout: &TextLayout,
    transform: Affine,
    color: Rgba8,
    cache_hits: &mut u64,
    cache_misses: &mut u64,
) {
    if layout.from_cache {
        *cache_hits += 1;
    } else {
        *cache_misses += 1;
    }
    if layout.width <= 0.0 || layout.height <= 0.0 {
        return;
    }
    let paint = Paint::Solid(color);
    for line in layout.layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue; // InlineBox(v0.1 无内联盒子场景)
            };
            fill_glyph_run(sink, &paint, transform, &glyph_run);
        }
    }
}

/// 填充一个定位字形 run:run 内同字体,`FontRef` 只解析一次。
fn fill_glyph_run(
    sink: &mut dyn PaintSink,
    paint: &Paint,
    transform: Affine,
    glyph_run: &parley::GlyphRun<'_, ()>,
) {
    let run = glyph_run.run();
    let font = run.font();
    // fontique 的 FontData = 字体文件字节 + 集合内索引,直接喂 skrifa
    let Ok(face) = FontRef::from_index(font.data.data(), font.index) else {
        return;
    };
    let outlines = face.outline_glyphs();
    // 无 hint 缩放到字号 = px(parley 布局空间同单位);变体坐标取默认位置
    for glyph in glyph_run.positioned_glyphs() {
        let settings = unhinted_settings(run.font_size());
        let Some(outline) = outlines.get(GlyphId::new(glyph.id)) else {
            continue;
        };
        let mut pen = KurboPen::default();
        if outline.draw(settings, &mut pen).is_err() {
            continue;
        }
        if pen.path.elements().is_empty() {
            continue; // 空格等无轮廓字形:零绘制
        }
        let placement =
            transform * Affine::translate((f64::from(glyph.x), f64::from(glyph.y))) * FLIP_Y;
        sink.fill(paint, placement, &pen.path);
    }
}

/// 无 hint 绘制参数(空变体坐标;const 切片避免逐 run 分配)。
///
/// 不变量(T6.3):`font_size` 来自 parley 布局 run,而布局入口已钳制过
/// 非有限/≤0 字号(text.rs `normalize_font_size`),故 `Size::new` 不会吃进
/// NaN——不要绕过布局管线直接以外部字号调用本函数。
fn unhinted_settings(font_size: f32) -> DrawSettings<'static> {
    const NO_COORDS: [NormalizedCoord; 0] = [];
    DrawSettings::unhinted(Size::new(font_size), LocationRef::new(&NO_COORDS))
}

/// skrifa [`OutlinePen`] → `kurbo::BezPath` 适配器(f32 → f64 原值转换)。
#[derive(Default)]
struct KurboPen {
    path: BezPath,
}

impl OutlinePen for KurboPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to((f64::from(x), f64::from(y)));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to((f64::from(x), f64::from(y)));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.path.quad_to(
            (f64::from(cx0), f64::from(cy0)),
            (f64::from(x), f64::from(y)),
        );
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.path.curve_to(
            (f64::from(cx0), f64::from(cy0)),
            (f64::from(cx1), f64::from(cy1)),
            (f64::from(x), f64::from(y)),
        );
    }

    fn close(&mut self) {
        self.path.close_path();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape;

    /// 计数型 sink:记 fill 的(颜色, 变换, 路径包围盒)。
    #[derive(Default)]
    struct CountingSink {
        fills: Vec<(Rgba8, Affine, kurbo::Rect)>,
    }

    impl PaintSink for CountingSink {
        fn fill(&mut self, paint: &Paint, transform: Affine, path: &BezPath) {
            let color = match paint {
                Paint::Solid(c) => *c,
                _ => [0, 0, 0, 0],
            };
            self.fills.push((color, transform, path.bounding_box()));
        }

        fn stroke(
            &mut self,
            _style: &sable_foundation::scene::StrokeStyle,
            _t: Affine,
            _p: &BezPath,
        ) {
        }
    }

    fn black() -> Rgba8 {
        [0, 0, 0, 255]
    }

    /// 同键两次布局:尺寸稳定、第二次命中缓存。
    #[test]
    fn layout_text_positive_and_stable_for_cjk_and_latin() {
        reset_text_cache();
        let first = layout_text("你好", 20.0, black());
        assert!(
            first.width > 0.0 && first.height > 0.0,
            "CJK 布局尺寸应 > 0,实际 ({}, {})",
            first.width,
            first.height
        );
        let second = layout_text("你好", 20.0, black());
        assert!(
            !first.from_cache && second.from_cache,
            "同键第二次获取必须命中缓存"
        );
        assert_eq!(
            (first.width, first.height),
            (second.width, second.height),
            "同键两次布局尺寸必须稳定"
        );
        let latin = layout_text("Aa123", 20.0, black());
        assert!(
            latin.width > 0.0 && latin.height > 0.0,
            "拉丁文本布局尺寸应 > 0,实际 ({}, {})",
            latin.width,
            latin.height
        );
    }

    /// 空文本:零尺寸布局、不进缓存(缓存计数不动)。
    #[test]
    fn layout_text_empty_is_zero_sized_and_uncached() {
        reset_text_cache();
        let empty = layout_text("", 16.0, black());
        assert_eq!((empty.width, empty.height), (0.0, 0.0), "空文本零尺寸");
        assert_eq!(cache_stats(), (0, 0), "空文本不进缓存");
    }

    /// 真字形轮廓:draw_text 产生非空 fill,变换带 y 翻转,颜色按参数。
    #[test]
    fn draw_text_emits_outline_fills_with_y_flip() {
        reset_text_cache();
        let layout = layout_text("axy", 24.0, black());
        let mut sink = CountingSink::default();
        let mut hits = 0u64;
        let mut misses = 0u64;
        draw_text(
            &mut sink,
            &layout,
            Affine::IDENTITY,
            black(),
            &mut hits,
            &mut misses,
        );
        assert_eq!((hits, misses), (0, 1), "首次布局计一次未命中");
        assert!(!sink.fills.is_empty(), "真字形轮廓必须产生 fill 调用");
        for (_, transform, bbox) in &sink.fills {
            assert!(bbox.area() > 0.0, "每个字形轮廓包围盒面积必须 > 0");
            let c = transform.as_coeffs();
            assert!(
                c[0] * c[3] - c[1] * c[2] < 0.0,
                "字形变换必须含 y 翻转(行列式 < 0)"
            );
            assert!(c[4] >= 0.0 && c[5].is_finite(), "字形原点应在布局空间内");
        }
        // 同一 TextLayout 重复绘制:来源标志不变,不再计 miss
        draw_text(
            &mut sink,
            &layout,
            Affine::IDENTITY,
            black(),
            &mut hits,
            &mut misses,
        );
        assert_eq!((hits, misses), (0, 2), "复用同一布局仍按未命中来源计");
    }

    /// 同键二次 layout_text → draw_text 计一次命中;全局 cache_stats 同步。
    #[test]
    fn cache_counters_report_hit_on_second_layout() {
        reset_text_cache();
        let color: Rgba8 = [10, 20, 30, 255];
        let first = layout_text("hitme", 18.0, color);
        let second = layout_text("hitme", 18.0, color);
        assert!(!first.from_cache);
        assert!(second.from_cache);
        assert_eq!(second.color, color, "layout_text 应记录传入颜色");
        let mut sink = CountingSink::default();
        let mut hits = 0u64;
        let mut misses = 0u64;
        draw_text(
            &mut sink,
            &second,
            Affine::IDENTITY,
            [255, 0, 0, 255], // 绘制颜色可与布局色不同(缓存键不含颜色)
            &mut hits,
            &mut misses,
        );
        assert_eq!((hits, misses), (1, 0), "命中布局的绘制计一次命中");
        let (global_hits, global_misses) = cache_stats();
        assert!(global_hits >= 1 && global_misses >= 1, "全局统计应同步累计");
    }

    /// V4.0 T6.3:NaN 字号在布局入口被钳制——测出有限正尺寸(渲染层因此
    /// 不会把 NaN 送进字形轮廓的 `Size::new`)。
    #[test]
    fn layout_text_non_finite_font_size_clamps_to_finite_layout() {
        reset_text_cache();
        let clamped = layout_text("边界", f64::NAN, black());
        assert!(
            clamped.width.is_finite() && clamped.width > 0.0,
            "NaN 字号钳制后宽度必须有限且 > 0,实际 {}",
            clamped.width
        );
        assert!(clamped.height.is_finite() && clamped.height > 0.0);
        // 与显式 fallback 字号同键:再次获取必命中(NaN 不产生独立缓存键)
        let fallback = layout_text("边界", crate::text::FALLBACK_FONT_SIZE, black());
        assert!(fallback.from_cache, "NaN 不得生成独立缓存键");
        assert_eq!(
            (clamped.width, clamped.height),
            (fallback.width, fallback.height)
        );
    }

    /// V4.0 T6.1:实测尺寸入口与 layout_text 同源一致;空文本 (0,0) 不进缓存。
    #[test]
    fn measured_text_size_matches_layout_text() {
        reset_text_cache();
        let (w, h) = measured_text_size("实测你好", 20.0);
        assert!(w > 0.0 && h > 0.0);
        let layout = layout_text("实测你好", 20.0, black());
        assert!(layout.from_cache, "实测必须走 TD-10 缓存");
        assert_eq!((w, h), (layout.width, layout.height));
        assert_eq!(measured_text_size("", 20.0), (0.0, 0.0), "空文本 (0,0)");
    }
}
