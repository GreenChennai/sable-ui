//! 文本排版管线:parley 0.11 接入(源自 docs/02 §6)。
//!
//! v0.1 落地:measure(测量)+ layout(断行/对齐),供命中测试包围盒、
//! 标尺刻度等使用。V3.0 T1 起,字形绘制由 [`crate::text_glyphs`] 承接
//! (轮廓 → `PaintSink`),并新增 TD-10 布局缓存([`TextPipeline::cached_layout`])。
//!
//! V4.0 T6(文本精度与缓存治理):
//! - **T6.1 bbox 实测化**:[`TextPipeline::measured_text_size`] 给出实测布局
//!   尺寸,[`node_world_bbox_measured`] 在场景级把 Text 分支的包围盒从
//!   foundation 的 0.6em/字符粗估替换为实测宽高(依赖方向:parley 不能下沉
//!   foundation,故实测发生在 canvas 层——方案 A 的变体);渲染/命中/选中/
//!   脏矩形均换用实测口径,CJK(实际 ≈1.0em/字)不再系统性偏窄 ~40%。
//! - **T6.2 缓存有界**:布局缓存条目上限 [`LAYOUT_CACHE_CAP`] + LRU 淘汰,
//!   长会话内存有界(此前 HashMap 只进不出)。
//! - **T6.3 字号边界**:非有限/≤0 字号在布局入口钳制到
//!   [`FALLBACK_FONT_SIZE`]([`normalize_font_size`]),NaN 绝不进缓存键。
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

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use kurbo::Rect;
use parley::style::StyleProperty;
use parley::{Alignment, AlignmentOptions, FontContext, Layout, LayoutContext};
use sable_foundation::scene::{NodeContent, NodeId, Scene};

use crate::text_glyphs::TextLayout;

/// TD-10 布局缓存键:`(文本, 字号的 f64 bit 模式)`。
///
/// 字号用 `to_bits()` 而非哈希后的 f64:常规字号逐位区分,序列化往返无损。
/// **键控前字号先经 [`normalize_font_size`] 规范化**(V4.0 T6.3)——NaN/±inf/
/// ≤0 一律映射到 [`FALLBACK_FONT_SIZE`] 的 bit 模式,病态字号既不产生独立键
/// 也不进入 parley。颜色不入键(布局与颜色无关)。
pub type TextCacheKey = (String, u64);

/// TD-10 缓存条目上限(V4.0 T6.2):达到上限后再插入新键,按 LRU 淘汰最久
/// 未用条目。长会话内存从此有界(此前 HashMap 只进不出,注释自认留 TD)。
pub const LAYOUT_CACHE_CAP: usize = 256;

/// 字号规范化回退值(V4.0 T6.3):非有限或 ≤0 的字号一律钳制到此值(桌面
/// 排版惯例的 16 量级),保证 NaN 绝不进入缓存键、parley 与字形轮廓的
/// `Size::new`。
pub const FALLBACK_FONT_SIZE: f64 = 16.0;

/// 字号边界规范化(T6.3):合法(有限且 > 0)原样返回 `(值, false)`;否则
/// 返回 `([FALLBACK_FONT_SIZE], true)`。不用 `f64::clamp`——它对 NaN 是恒等
/// 传播,拦不住 NaN,必须显式分支。
pub fn normalize_font_size(font_size: f64) -> (f64, bool) {
    if font_size.is_finite() && font_size > 0.0 {
        (font_size, false)
    } else {
        (FALLBACK_FONT_SIZE, true)
    }
}

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
    /// LRU 访问序(V4.0 T6.2):队首 = 最久未用,队尾 = 最近使用;
    /// 与 `cache` 同步增删(命中提序、插入入队、淘汰出队首)。
    lru: VecDeque<TextCacheKey>,
    hits: u64,
    misses: u64,
    /// LRU 淘汰累计(T6.2 观测面,[`Self::cache_evictions`])。
    evictions: u64,
    /// 字号钳制累计(T6.3 观测面,[`Self::font_size_clamps`]):布局入口收到
    /// 非有限或 ≤0 字号的次数。
    font_size_clamps: u64,
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
            lru: VecDeque::new(),
            hits: 0,
            misses: 0,
            evictions: 0,
            font_size_clamps: 0,
        }
    }

    /// 排版一段文本(默认字体栈、可换行),返回 parley [`Layout`]。
    ///
    /// - `font_size`:世界坐标字号(f64 纪律;内部降 f32 传 parley)。
    ///   **非有限/≤0 先经 [`normalize_font_size`] 钳制并计一次钳制数**
    ///   (T6.3:NaN 绝不进 `StyleProperty::FontSize` 与字形轮廓的
    ///   `Size::new`);
    /// - `max_width`:`Some(w)` 时按宽度断行,`None` 单行。
    ///
    /// 注意:本方法**每次都重排**(测量语义,不走 TD-10 缓存);渲染路径
    /// 用 [`Self::cached_layout`]。
    pub fn layout(&mut self, text: &str, font_size: f64, max_width: Option<f64>) -> Layout<()> {
        let (font_size, clamped) = normalize_font_size(font_size);
        if clamped {
            self.font_size_clamps += 1;
        }
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
    ///
    /// V4.0 T6.2/T6.3:字号先经 [`normalize_font_size`] 规范化(病态字号不产
    /// 生独立键,钳制数在本层计一次,传给 [`Self::layout`] 的已是合法值不会
    /// 重复计数);条目达 [`LAYOUT_CACHE_CAP`] 时按 LRU 淘汰最久未用键,
    /// 命中时把该键提到访问序末尾。
    pub fn cached_layout(&mut self, text: &str, font_size: f64) -> TextLayout {
        let (font_size, clamped) = normalize_font_size(font_size);
        if clamped {
            self.font_size_clamps += 1;
        }
        let key: TextCacheKey = (text.to_string(), font_size.to_bits());
        // 命中:取快照后立即释放表的借用,再做 LRU 提序
        if let Some((layout, width, height)) = self
            .cache
            .get(&key)
            .map(|c| (Arc::clone(&c.layout), c.width, c.height))
        {
            self.hits += 1;
            self.bump_lru(&key);
            return TextLayout {
                layout,
                width,
                height,
                color: [0, 0, 0, 0],
                from_cache: true,
            };
        }
        self.misses += 1;
        let layout = Arc::new(self.layout(text, font_size, None));
        let width = f64::from(layout.width());
        let height = f64::from(layout.height());
        self.evict_lru_if_full();
        self.cache.insert(
            key.clone(),
            CachedText {
                layout: Arc::clone(&layout),
                width,
                height,
            },
        );
        self.lru.push_back(key);
        TextLayout {
            layout,
            width,
            height,
            color: [0, 0, 0, 0],
            from_cache: false,
        }
    }

    /// LRU 提序:把命中键移到访问序末尾(表长 ≤ 256,线性查找够用)。
    fn bump_lru(&mut self, key: &TextCacheKey) {
        if let Some(pos) = self.lru.iter().position(|k| k == key) {
            self.lru.remove(pos);
        }
        self.lru.push_back(key.clone());
    }

    /// 满载淘汰:插入前调用,确保插入后条目数 ≤ [`LAYOUT_CACHE_CAP`]。
    fn evict_lru_if_full(&mut self) {
        while self.cache.len() >= LAYOUT_CACHE_CAP {
            match self.lru.pop_front() {
                Some(oldest) => {
                    self.cache.remove(&oldest);
                    self.evictions += 1;
                }
                None => break, // 队列与表不一致的防御(正常不变量下不可达)
            }
        }
    }

    /// 实测一段文本的布局尺寸 `(宽, 高)`(V4.0 T6.1,方案 A 的 canvas 侧入口)。
    ///
    /// 走 [`Self::cached_layout`](TD-10 缓存),同键二次调用零重排;空文本
    /// 短路 `(0.0, 0.0)`(与 [`Self::measure`] 的空文本契约一致)。命中测试/
    /// 渲染/选中框以此替代 foundation 对 Text 的 0.6em/字符粗估。
    pub fn measured_text_size(&mut self, text: &str, font_size: f64) -> (f64, f64) {
        if text.is_empty() {
            return (0.0, 0.0);
        }
        let layout = self.cached_layout(text, font_size);
        (layout.width, layout.height)
    }

    /// 缓存统计:`(命中次数, 未命中次数)`(自管线创建/清零起累计)。
    pub fn cache_stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }

    /// 当前缓存条目数(T6.2 观测面;恒 ≤ [`LAYOUT_CACHE_CAP`])。
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    /// LRU 淘汰累计次数(T6.2 观测面)。
    pub fn cache_evictions(&self) -> u64 {
        self.evictions
    }

    /// 字号钳制累计次数(T6.3 观测面:布局入口收到非有限/≤0 字号的次数)。
    pub fn font_size_clamps(&self) -> u64 {
        self.font_size_clamps
    }

    /// 清空布局缓存并归零计数(测试隔离与内存压力逃生口)。
    ///
    /// V4.0 T6.2 起缓存自带 LRU 上限(长会话内存有界),本方法保留为
    /// 显式逃生口:清表 + 清访问序 + 归零全部计数(含 evictions/钳制)。
    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.lru.clear();
        self.hits = 0;
        self.misses = 0;
        self.evictions = 0;
        self.font_size_clamps = 0;
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

/// 节点世界包围盒,**Text 分支用实测布局尺寸**(V4.0 T6.1)。
///
/// foundation 的 [`Scene::node_world_bbox`] 对 Text 用 0.6em/字符 × 1.2em
/// 粗估(依赖方向:parley 不能下沉 foundation),而 CJK 实际 ≈1.0em/字,
/// 粗估系统性偏窄 ~40%——命中右半边点不中、选择框/LOD Silhouette/脏矩形
/// 全部偏窄(review R6 实锤)。本函数是方案 A 的场景级替代:
///
/// - `Text`:实测 `(宽, 高)`([`crate::text_glyphs::measured_text_size`],走
///   TD-10 缓存,命中即得零重排)× 世界变换;空文本为 (0,0),与"空文本
///   零覆盖"渲染契约一致;
/// - `Group`:各子节点递归取实测包围盒求并(与 foundation 同语义:任一
///   子节点无包围盒则整体 None);
/// - 其余(`Path`/`Image`):原样委托 [`Scene::node_world_bbox`](Path 的
///   bbox 来自路径几何、Image 是精确数据矩形,无需实测)。
pub fn node_world_bbox_measured(scene: &Scene, id: NodeId) -> Option<Rect> {
    let node = scene.node(id)?;
    match &node.content {
        NodeContent::Text(t) => {
            let world = scene.world_transform(id)?;
            let (width, height) = crate::text_glyphs::measured_text_size(&t.text, t.font_size);
            Some(world.transform_rect_bbox(Rect::new(0.0, 0.0, width, height)))
        }
        NodeContent::Group => {
            let mut acc: Option<Rect> = None;
            for &child in &node.children {
                let child_bbox = node_world_bbox_measured(scene, child)?;
                acc = Some(match acc {
                    Some(a) => a.union(child_bbox),
                    None => child_bbox,
                });
            }
            acc
        }
        _ => scene.node_world_bbox(id),
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

    // —— V4.0 T6.2:LRU 淘汰 ——

    /// 塞入超上限的不同键:条目数钳在 256,溢出按 LRU 逐出(evictions 计数
    /// 佐证);最旧的键被逐出后再次获取必重排(未命中)。
    #[test]
    fn layout_cache_lru_evicts_beyond_capacity() {
        let mut pipeline = TextPipeline::new();
        for i in 0..300 {
            pipeline.cached_layout(&format!("key-{i}"), 16.0);
        }
        assert_eq!(pipeline.cache_len(), LAYOUT_CACHE_CAP, "条目数必须钳在上限");
        assert_eq!(pipeline.cache_evictions(), 44, "300 - 256 = 44 次淘汰");

        // 最旧的 key-0 已被逐出:再次获取是未命中(重排),并把另一条挤出去
        let again = pipeline.cached_layout("key-0", 16.0);
        assert!(!again.from_cache, "最旧条目必须已被 LRU 逐出");
        assert_eq!(pipeline.cache_len(), LAYOUT_CACHE_CAP);
        assert_eq!(pipeline.cache_evictions(), 45);

        // 新近插入的 key-299 仍命中
        assert!(pipeline.cached_layout("key-299", 16.0).from_cache);
    }

    /// LRU 是**访问序**而非插入序:满载后摸一下最旧的键(命中提序),再插
    /// 一个新键,被逐出的应是"第二旧",而被摸过的键存活。
    #[test]
    fn layout_cache_lru_is_access_ordered() {
        let mut pipeline = TextPipeline::new();
        for i in 0..(LAYOUT_CACHE_CAP as i32) {
            pipeline.cached_layout(&format!("k{i}"), 12.0);
        }
        assert_eq!(pipeline.cache_evictions(), 0, "恰好装满,尚无淘汰");

        let _ = pipeline.cached_layout("k0", 12.0); // 命中,k0 提到访问序最新
        pipeline.cached_layout("k-new", 12.0); // 挤掉最久未用的 k1
        assert_eq!(pipeline.cache_evictions(), 1);
        assert!(
            pipeline.cached_layout("k0", 12.0).from_cache,
            "被访问过的 k0 必须存活"
        );
        assert!(
            !pipeline.cached_layout("k1", 12.0).from_cache,
            "未被访问的 k1 才是被逐出者"
        );
    }

    /// clear_cache 语义不变:清表 + 清访问序 + 归零全部计数(含新增的
    /// evictions 与字号钳制计数)。
    #[test]
    fn clear_cache_resets_counters_and_lru() {
        let mut pipeline = TextPipeline::new();
        pipeline.cached_layout("x", f64::NAN); // 1 未命中 + 1 次钳制
        assert_eq!(pipeline.cache_len(), 1);
        assert_eq!(pipeline.font_size_clamps(), 1);
        pipeline.clear_cache();
        assert_eq!(pipeline.cache_stats(), (0, 0));
        assert_eq!(pipeline.cache_len(), 0);
        assert_eq!(pipeline.cache_evictions(), 0);
        assert_eq!(pipeline.font_size_clamps(), 0);
        let rebuilt = pipeline.cached_layout("x", 16.0);
        assert!(!rebuilt.from_cache, "清空后重新视为未命中");
    }

    // —— V4.0 T6.3:字号边界 ——

    /// NaN/±inf/0/负:一律钳到 FALLBACK_FONT_SIZE;NaN 不产生独立缓存键
    /// (与显式 fallback 同键命中),布局尺寸有限,键不含 NaN 位型。
    #[test]
    fn font_size_sanitized_before_cache_key() {
        let mut pipeline = TextPipeline::new();
        assert_ne!(
            f64::NAN.to_bits(),
            FALLBACK_FONT_SIZE.to_bits(),
            "前提:NaN 的 bit 模式与 fallback 不同"
        );
        let nan_layout = pipeline.cached_layout("字号钳制", f64::NAN);
        assert!(!nan_layout.from_cache);
        assert_eq!(pipeline.font_size_clamps(), 1);
        assert!(
            nan_layout.width.is_finite() && nan_layout.width > 0.0,
            "钳制后布局宽度必须有限且 > 0,实际 {}",
            nan_layout.width
        );
        // NaN 归一后与显式 fallback 同键:必须命中,尺寸一致
        let fallback_layout = pipeline.cached_layout("字号钳制", FALLBACK_FONT_SIZE);
        assert!(fallback_layout.from_cache, "NaN 不得生成独立缓存键");
        assert_eq!(
            (nan_layout.width, nan_layout.height),
            (fallback_layout.width, fallback_layout.height)
        );

        // 0 / 负 / ±inf 同样钳到 fallback 键
        for bad in [0.0, -4.0, f64::INFINITY, f64::NEG_INFINITY] {
            let l = pipeline.cached_layout("字号钳制", bad);
            assert!(l.from_cache, "{bad} 应与 fallback 同键命中");
        }
        assert_eq!(pipeline.font_size_clamps(), 5, "五个病态字号各计一次钳制");
    }

    /// measure 路径同样受钳制保护:NaN 字号测出有限尺寸并计一次钳制。
    #[test]
    fn measure_with_nan_font_size_clamps_and_counts() {
        let mut pipeline = TextPipeline::new();
        let (w, h) = pipeline.measure("nan size", f64::NAN, None);
        assert!(w.is_finite() && w > 0.0 && h.is_finite() && h > 0.0);
        assert_eq!(pipeline.font_size_clamps(), 1);
    }

    // —— V4.0 T6.1:实测尺寸 ——

    /// 实测尺寸与 cached_layout 的布局尺寸同源一致;实测必须走 TD-10 缓存
    /// (同键零重排);空文本 (0,0)。
    #[test]
    fn measured_text_size_matches_cached_layout_and_empty_is_zero() {
        let mut pipeline = TextPipeline::new();
        let (w, h) = pipeline.measured_text_size("measure me", 20.0);
        assert!(w > 0.0 && h > 0.0);
        let layout = pipeline.cached_layout("measure me", 20.0);
        assert!(layout.from_cache, "实测必须走 TD-10 缓存(同键零重排)");
        assert_eq!((w, h), (layout.width, layout.height));
        assert_eq!(pipeline.measured_text_size("", 20.0), (0.0, 0.0));
    }

    /// CJK 实测宽 ≈ 1em/字:0.6em/字粗估系统性偏窄 ~40%(review R6 实锤)。
    #[test]
    fn measured_cjk_width_is_about_one_em_per_char() {
        let mut pipeline = TextPipeline::new();
        let (w, h) = pipeline.measured_text_size("你好", 20.0);
        assert!(
            w > 36.0,
            "两个 CJK 字的实测宽应 ≈ 2em = 40(旧粗估 24 偏窄),实际 {w}"
        );
        assert!(w < 48.0, "CJK advance 不应显著超过 1.2em/字,实际 {w}");
        assert!(h > 0.0);
    }

    /// 拉丁宽字符(W)实测宽明显大于 0.6em/字符的旧估计(R6 的另一半)。
    #[test]
    fn measured_wide_latin_width_exceeds_coarse_estimate() {
        let mut pipeline = TextPipeline::new();
        let (w, _) = pipeline.measured_text_size("WWW", 24.0);
        let coarse = 0.6 * 24.0 * 3.0; // 旧粗估右缘 = 43.2
        assert!(
            w > coarse * 1.2,
            "W 的 advance ≈ 0.9em+,实测宽 {w} 应明显大于粗估 {coarse}"
        );
    }
}
