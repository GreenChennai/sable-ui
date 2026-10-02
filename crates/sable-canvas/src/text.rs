//! 文本排版管线:parley 0.11 接入(源自 docs/02 §6)。
//!
//! v0.1 落地:measure(测量)+ layout(断行/对齐),供命中测试包围盒、
//! 标尺刻度等使用。V3.0 T1 起,字形绘制由 [`crate::text_glyphs`] 承接
//! (轮廓 → `PaintSink`),并新增 TD-10 布局缓存([`TextPipeline::cached_layout`])。
//!
//! # parley 0.11 API 核实结论(2026-10,docs.rs/parley/0.11.1)
//!
//! 与 docs/02 §6 示例的签名差异(原文基于旧版 parley):
//! 1. `Layout::align` **没有 `max_advance` 参数**:原文
//!    `layout.align(max_width, Alignment::Start, ..)` → 实际为
//!    `align(&mut self, alignment: Alignment, options: AlignmentOptions)`;
//! 2. `ranged_builder(&mut self, fcx, text, scale: f32, quantize: bool)` ——
//!    第 4 参 `quantize` 为 0.11 新增,传 `true`(量化字号,缓存友好);
//! 3. `break_all_lines(max_advance: Option<f32>)` 与原文一致。
//!
//! 刷子类型用 `()`(无色布局;parley crate 级示例即 `Layout<()>`);
//! 需要按 run 着色时再换 `Layout<Color>`。

use std::collections::HashMap;
use std::sync::Arc;

use parley::style::StyleProperty;
use parley::{Alignment, AlignmentOptions, FontContext, Layout, LayoutContext};

use crate::text_glyphs::TextLayout;

/// TD-10 布局缓存键:`(文本, 字号的 f64 bit 模式)`。
///
/// 字号用 `to_bits()` 而非哈希后的 f64:NaN/±0.0 之外的常规字号逐位区分,
/// 序列化往返无损。颜色不入键(布局与颜色无关)。
pub type TextCacheKey = (String, u64);

/// TD-10 缓存记录:`Arc<Layout>` 共享 + 总尺寸(命中零拷贝、零重排)。
struct CachedText {
    layout: Arc<Layout<()>>,
    width: f64,
    height: f64,
}

/// parley 排版管线:持字体上下文与布局上下文,复用内部缓存。
pub struct TextPipeline {
    pub font_cx: FontContext,
    pub layout_cx: LayoutContext<()>,
    /// TD-10 布局缓存(渲染侧经 [`Self::cached_layout`] 使用)。
    cache: HashMap<TextCacheKey, CachedText>,
    hits: u64,
    misses: u64,
}

impl Default for TextPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl TextPipeline {
    /// 新建管线(`FontContext::new` 会发现系统字体)。
    pub fn new() -> Self {
        TextPipeline {
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
            cache: HashMap::new(),
            hits: 0,
            misses: 0,
        }
    }

    /// 排版一段文本(默认字体栈、可换行),返回 parley [`Layout`]。
    ///
    /// - `font_size`:世界坐标字号(f64 纪律;内部降 f32 传 parley);
    /// - `max_width`:`Some(w)` 时按宽度断行,`None` 单行。
    ///
    /// 注意:本方法**每次都重排**(测量语义,不走 TD-10 缓存);渲染路径
    /// 用 [`Self::cached_layout`]。
    pub fn layout(&mut self, text: &str, font_size: f64, max_width: Option<f64>) -> Layout<()> {
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, text, 1.0, true);
        builder.push_default(StyleProperty::FontSize(font_size as f32));
        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(max_width.map(|w| w as f32));
        layout.align(Alignment::Start, AlignmentOptions::default());
        layout
    }

    /// TD-10 带缓存布局:同 `(text, font_size)` 二次调用直接命中,不重排。
    ///
    /// 返回的 [`TextLayout`] 是廉价句柄(`Arc<Layout>` 共享);`from_cache`
    /// 报告本次命中情况。`color` 字段占位为全零——颜色不入键,由
    /// [`crate::text_glyphs::layout_text`] 覆写。
    pub fn cached_layout(&mut self, text: &str, font_size: f64) -> TextLayout {
        let key: TextCacheKey = (text.to_string(), font_size.to_bits());
        if let Some(cached) = self.cache.get(&key) {
            let hit = TextLayout {
                layout: Arc::clone(&cached.layout),
                width: cached.width,
                height: cached.height,
                color: [0, 0, 0, 0],
                from_cache: true,
            };
            self.hits += 1;
            return hit;
        }
        self.misses += 1;
        let layout = self.layout(text, font_size, None);
        let width = f64::from(layout.width());
        let height = f64::from(layout.height());
        let cached = CachedText {
            layout: Arc::new(layout),
            width,
            height,
        };
        let miss = TextLayout {
            layout: Arc::clone(&cached.layout),
            width,
            height,
            color: [0, 0, 0, 0],
            from_cache: false,
        };
        self.cache.insert(key, cached);
        miss
    }

    /// 缓存统计:`(命中次数, 未命中次数)`(自管线创建/清零起累计)。
    pub fn cache_stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }

    /// 清空布局缓存并归零计数(测试隔离与内存压力逃生口)。
    ///
    /// v3.0 无自动淘汰;长会话内存治理留 TD 跟进。
    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.hits = 0;
        self.misses = 0;
    }

    /// 测量排版结果,返回 `(宽, 高)`,世界坐标 f64。
    ///
    /// 宽度为 parley 的 `Layout::width`(不含行尾空白);空文本/无字体回退为
    /// `(0.0, 0.0)`,不 panic。
    pub fn measure(&mut self, text: &str, font_size: f64, max_width: Option<f64>) -> (f64, f64) {
        // 空文本没有可度量的内容;parley 会给空串排出一行空行(返回字体行高),
        // 与 doc 契约"空文本为 (0,0)"不符,这里短路
        if text.is_empty() {
            return (0.0, 0.0);
        }
        let layout = self.layout(text, font_size, max_width);
        (f64::from(layout.width()), f64::from(layout.height()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_returns_positive_size_for_nonempty_text() {
        let mut pipeline = TextPipeline::new();
        let (w, h) = pipeline.measure("Hello", 16.0, None);
        assert!(w > 0.0, "非空文本宽度应 > 0,实际 {w}");
        assert!(h > 0.0, "行高应 > 0,实际 {h}");
    }

    #[test]
    fn measure_scales_with_font_size() {
        let mut pipeline = TextPipeline::new();
        let (w16, h16) = pipeline.measure("Hello world", 16.0, None);
        let (w32, h32) = pipeline.measure("Hello world", 32.0, None);
        assert!(w32 > w16, "字号翻倍宽度应增长:{w32} vs {w16}");
        assert!(h32 > h16, "字号翻倍行高应增长:{h32} vs {h16}");
    }

    #[test]
    fn max_width_wraps_into_multiple_lines() {
        let mut pipeline = TextPipeline::new();
        let (w_single, h_single) = pipeline.measure("hello world hello world", 16.0, None);
        let (w_wrapped, h_wrapped) =
            pipeline.measure("hello world hello world", 16.0, Some(w_single / 3.0));
        assert!(
            h_wrapped > h_single,
            "窄容器应断行增高:{h_wrapped} vs {h_single}"
        );
        assert!(
            w_wrapped < w_single,
            "断行后宽度应收窄:{w_wrapped} vs {w_single}"
        );
    }

    #[test]
    fn empty_text_measures_zero() {
        let mut pipeline = TextPipeline::new();
        let (w, h) = pipeline.measure("", 16.0, None);
        assert_eq!((w, h), (0.0, 0.0), "空文本测量为 (0, 0)");
    }

    #[test]
    fn cjk_text_measures_positive() {
        // 中文 shaping(docs/02 §9 验收项);Windows CI 有系统中文字体
        let mut pipeline = TextPipeline::new();
        let (w, h) = pipeline.measure("北京 2026", 12.0, None);
        assert!(w > 0.0 && h > 0.0, "CJK 文本应可测量,实际 ({w}, {h})");
    }

    #[test]
    fn layout_is_reusable_across_calls() {
        // 布局上下文缓存复用:连续多次排版不 panic、结果稳定
        let mut pipeline = TextPipeline::new();
        let first = pipeline.measure("stable", 14.0, None);
        for _ in 0..3 {
            assert_eq!(pipeline.measure("stable", 14.0, None), first);
        }
        let other = pipeline.measure("different text", 20.0, Some(50.0));
        assert!(other.0 > 0.0);
    }

    /// TD-10:同键二次 cached_layout 命中缓存,计数器正确累加;
    /// 命中与未命中的布局尺寸一致;字号不同视为不同键。
    #[test]
    fn cached_layout_counts_hits_and_misses() {
        let mut pipeline = TextPipeline::new();
        let first = pipeline.cached_layout("cached text", 16.0);
        assert!(!first.from_cache, "首次布局必为未命中");
        assert!(first.width > 0.0 && first.height > 0.0);
        assert_eq!(pipeline.cache_stats(), (0, 1));

        let second = pipeline.cached_layout("cached text", 16.0);
        assert!(second.from_cache, "同键第二次必须命中");
        assert_eq!(pipeline.cache_stats(), (1, 1));
        assert_eq!(
            (first.width, first.height),
            (second.width, second.height),
            "命中与未命中的布局尺寸必须一致"
        );

        // 字号不同 → 不同键,再计一次未命中
        let _ = pipeline.cached_layout("cached text", 20.0);
        assert_eq!(pipeline.cache_stats(), (1, 2));

        pipeline.clear_cache();
        assert_eq!(pipeline.cache_stats(), (0, 0), "清空后计数归零");
        let rebuilt = pipeline.cached_layout("cached text", 16.0);
        assert!(!rebuilt.from_cache, "清空后重新视为未命中");
    }
}
