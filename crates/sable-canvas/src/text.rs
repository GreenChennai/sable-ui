//! 文本排版管线:parley 0.11 接入(源自 docs/02 §6)。
//!
//! v0.1 落地:measure(测量)+ layout(断行/对齐),供命中测试包围盒、
//! 标尺刻度等使用。**字形实际绘制留 M2**:`vello::Scene::draw_glyphs` 的
//! 接入点在 `draw()`(GPU 字形缓存,比转路径快一个数量级,docs/02 §6 原注);
//! CPU 后端的文字绘制同样等 M2。
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

use parley::style::StyleProperty;
use parley::{Alignment, AlignmentOptions, FontContext, Layout, LayoutContext};

/// parley 排版管线:持字体上下文与布局上下文,复用内部缓存。
pub struct TextPipeline {
    pub font_cx: FontContext,
    pub layout_cx: LayoutContext<()>,
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
        }
    }

    /// 排版一段文本(默认字体栈、可换行),返回 parley [`Layout`]。
    ///
    /// - `font_size`:世界坐标字号(f64 纪律;内部降 f32 传 parley);
    /// - `max_width`:`Some(w)` 时按宽度断行,`None` 单行。
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
}
