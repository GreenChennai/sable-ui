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
use sable_foundation::effects::EffectEntry;
use sable_foundation::scene::{
    BlendMode, NodeContent, NodeId, Paint, Rgba8, Scene, StrokeStyle, TextNode,
};
use sable_foundation::viewport::Viewport;
#[cfg(feature = "cpu")]
use sable_paint::effects::EffectCaps;
use sable_paint::effects::{self, EffectLevel};
use sable_paint::sink::PaintSink;

use crate::grid;
use crate::lod::{self, DetailLevel};
use crate::text::node_world_bbox_measured;
use crate::text_glyphs;

/// 选中包围盒的屏幕线宽(px,docs/02 §4.2:任意缩放下 1.5px)。
pub const SELECTION_STROKE_PX: f64 = 1.5;
/// 控制柄方块的屏幕边长(px,docs/02 §9:8 向控制柄 6px)。
pub const HANDLE_SIZE_PX: f64 = 6.0;
/// 控制柄描边的屏幕线宽(px)。
pub const HANDLE_STROKE_PX: f64 = 1.0;

/// 覆盖层主题色(选中框/网格/参考线/锚点)。
///
/// **默认值仅供无 token 场景(测试/无头渲染);UI 侧应从 sable-widgets
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
#[derive(Clone, Default)]
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
    /// 效果降级档位直控(G20/V4.0 T2):`None`(Default)= 按
    /// `SABLE_EFFECTS_LEVEL` env 检测(未设回落编译期默认,gpu→Full /
    /// cpu→Reduced)。测试与嵌入方可显式指定——如 `Some(EffectLevel::Off)`
    /// 锁定与 v3.0 逐位一致的回归基线。
    pub effect_level: Option<EffectLevel>,
    /// PERF-05:效果离屏缓存(静止场景零重算)。`None`(Default)= 不缓存
    /// (行为与 v4.0 逐位一致);`Some` = 效果路径光栅结果按
    /// [`effects_raster_key`] 指纹入缓存,静止帧命中零光栅化。宿主持有一份
    /// `Rc<RefCell<ShadowCache>>` 跨帧传入(golden/测试用 Default 关闭,
    /// 保证基线确定性)。
    pub effects_cache: Option<std::rc::Rc<std::cell::RefCell<sable_paint::effects::ShadowCache>>>,
}

impl std::fmt::Debug for RenderOpts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // effects_cache 不实现 Debug(ShadowCache 无 Debug 派生):只报在位与否
        f.debug_struct("RenderOpts")
            .field("selection", &self.selection)
            .field("show_grid", &self.show_grid)
            .field("overlay", &self.overlay)
            .field("screen_size", &self.screen_size)
            .field("effect_level", &self.effect_level)
            .field("effects_cache", &self.effects_cache.is_some())
            .finish()
    }
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
///
/// # 混合模式(迭代计划 08 E5)
///
/// 节点 `blend_mode != Normal` 时,该节点的全部内容(fill+stroke/文本字形)
/// 被包进一次 `push_blend`/`pop_blend`(**逐节点**一个混合层,与 Illustrator
/// 图层面板语义一致):节点内容作为整体与其下的已绘背景混合。网格与选中框/
/// 控制柄等**覆盖层在混合循环之外,不受任何节点混合模式影响**。push/pop
/// 严格配对由本函数结构保证(push 之后的所有分支都汇合到同一个 pop)。
///
/// # 文本节点(V3.0 T1;bbox 实测化 V4.0 T6.1)
///
/// `NodeContent::Text` 参与渲染:布局走 [`crate::text_glyphs`] 的 TD-10
/// 缓存(键 = 文本 + 字号,颜色不入键),字形以轮廓路径填充;LOD 语义与
/// Path 一致(Point 跳过 / Silhouette 包围盒色块 / Full 真字形);节点
/// 不透明度折入字形颜色 alpha。剔除/LOD 特征尺寸/Silhouette 色块/选中框
/// 对 Text 一律用 [`crate::text::node_world_bbox_measured`] 的**实测布局
/// 尺寸**包围盒(命中测试同口径)——foundation 的 0.6em/字符粗估对 CJK
/// 偏窄 ~40% 的口径不再进入渲染路径。
///
/// # 节点效果栈(G20,V4.0 T2)
///
/// 节点 `effects` 存在**活动条目**(启用且非恒等,`EffectEntry::is_active`)、
/// 当前档位允许(E12:`RenderOpts::effect_level` 直控或 `SABLE_EFFECTS_LEVEL`
/// env 检测,Off 档关闭全部)、且后端支持像素回贴
/// ([`PaintSink::supports_draw_rgba`])时,该节点走**效果路径**:单独光栅化
/// 到透明离屏([`effects::EffectSurface`],LOD 档位与直绘同一判定)→
/// [`effects::apply_effects_rgba`] 按栈序求值 → [`PaintSink::draw_rgba`]
/// 合成回画布。嵌套顺序定义为**效果先于混合**(T2.2):回贴发生在该节点的
/// push_blend 层之内,即效果先作用于节点内容、其结果作为整体参与混合。
/// 阴影/光晕可投到节点包围盒之外,故剔除用"bbox 外扩效果支撑域"复检。
/// **任一门槛不满足(Off 档/空栈/全禁用/后端不支持)→ 与 v3.0 完全相同的
/// 直绘路径,输出逐位一致**(硬验收)。
pub fn render_scene(
    scene: &Scene,
    viewport: &Viewport,
    sink: &mut dyn PaintSink,
    opts: &RenderOpts,
) {
    // RB-11:关键路径结构化日志(节点数/屏幕尺寸;宿主装配 subscriber 才可见)
    let _span = tracing::debug_span!(
        "render_scene",
        screen = ?opts.screen_size,
        selection = opts.selection.len(),
    )
    .entered();
    let vp = viewport.world_to_viewport();
    let size_known = opts.screen_size.0 > 0.0 && opts.screen_size.1 > 0.0;
    let visible = size_known.then(|| visible_world_rect(viewport, opts.screen_size));

    // E12 降级档位(opts 直控优先,env 检测兜底):整帧求值一次,逐节点
    // 只查能力位。Off 档 blur=false → effects_active 恒 false → 与 v3.0
    // 完全相同的直绘路径。
    let effect_caps = effects::caps(opts.effect_level.unwrap_or_else(effects::detect));

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
        // 组自身无内容(子节点是 render_list 的独立条目);内容包围盒缺失同理。
        // Text 用实测布局尺寸的包围盒(V4.0 T6.1),其余与 foundation 口径一致
        let Some(world_bbox) = node_world_bbox_measured(scene, id) else {
            continue;
        };

        // 效果路径门槛(G20/T2.1):活动效果(启用且非恒等)× 档位允许 ×
        // 后端支持像素回贴。任一不满足 → 与 v3.0 完全相同的直绘路径
        // (Off 档 / 空栈 / 全禁用条目零开销,输出逐位一致——硬验收)。
        let effects_active = effect_caps.blur
            && node.effects.iter().any(EffectEntry::is_active)
            && sink.supports_draw_rgba();

        // 视锥剔除:包围盒经视口变换后与可见世界矩形不相交 → 跳过。
        // 例外(G20):阴影/光晕可把内容投到节点 bbox 之外——活动效果时用
        // "bbox 外扩效果支撑域(像素 margin / zoom 换算回世界)"复检一次,
        // 不把影子还在视口内的节点误剔。
        if let Some(visible) = visible {
            if !rects_intersect(world_bbox, visible) {
                let culled = if effects_active {
                    let m = effects::effect_margins_px(&node.effects, viewport.zoom);
                    let pad = m[0].max(m[1]).max(m[2]).max(m[3]) / viewport.zoom;
                    !rects_intersect(world_bbox.inflate(pad, pad), visible)
                } else {
                    true
                };
                if culled {
                    continue;
                }
            }
        }

        // 传给 sink 的变换 = 视口 × 节点世界变换(f64,docs/02 §4.2 的 `total`)
        let total = vp * world_xform;

        // LOD(docs/02 §7.2):以包围盒短边为特征尺寸(Text/Path 同一判定)
        let feature_size = world_bbox.width().min(world_bbox.height());
        let detail = lod::detail_level(feature_size, viewport.zoom);

        // 混合层先开:E5 要求节点内容(fill/stroke/文本字形)作为一个整体
        // 参与混合;之后的分支全部汇合到循环尾的 pop_blend,配对由结构保证。
        let blended = node.blend_mode != BlendMode::Normal;

        // —— 效果路径(G20/T2.1):节点单独离屏光栅化 → 效果求值 → 回贴。——
        // Point 档连内容都不画,效果无从作用,与直绘同语义跳过(混合节点仍
        // 弹掉空混合层,保持 push/pop 配对)。回贴被包裹在混合层**之内** =
        // "效果先于混合"(T2.2:效果作用在节点内容上,其结果作为整体参与
        // 混合)。
        if effects_active {
            if detail == DetailLevel::Point {
                if blended {
                    sink.push_blend(node.blend_mode);
                    sink.pop_blend();
                }
                continue;
            }
            #[cfg(feature = "cpu")]
            if let Some(frame) = {
                let params = EffectRasterParams {
                    content: &node.content,
                    effects: &node.effects,
                    detail,
                    zoom: viewport.zoom,
                    world_bbox,
                    total,
                    vp,
                    opacity: node.opacity,
                };
                // PERF-05:缓存命中零光栅化(静止场景零重算);存储格式 =
                // 12 字节头(w/h:u16 LE + dx/dy:i32 LE)+ 像素(打包自包含,
                // 避免为 w/h/dx/dy 再开第二张表)。
                let key = opts
                    .effects_cache
                    .as_ref()
                    .map(|_| effects_raster_key(&params, opts.screen_size));
                let cached = match (&opts.effects_cache, key) {
                    (Some(cache), Some(key)) => cache.borrow_mut().get(key),
                    _ => None,
                };
                /// 帧来源二态:命中(自包含头+像素的缓存条)或新渲(裸像素)。
                /// `pixels()` 统一剥头,回贴路径两态同口径。
                enum EffectFrame {
                    Cached(std::sync::Arc<Vec<u8>>, u16, u16, i32, i32),
                    Fresh(Vec<u8>, u16, u16, i32, i32),
                }
                impl EffectFrame {
                    fn dims(&self) -> (u16, u16, i32, i32) {
                        match self {
                            EffectFrame::Cached(_, w, h, dx, dy)
                            | EffectFrame::Fresh(_, w, h, dx, dy) => (*w, *h, *dx, *dy),
                        }
                    }
                    fn pixels(&self) -> &[u8] {
                        match self {
                            // 命中:剥 12 字节几何头(存储自包含头+像素)
                            EffectFrame::Cached(buf, ..) => {
                                if buf.len() >= 12 {
                                    &buf[12..]
                                } else {
                                    &buf[..]
                                }
                            }
                            EffectFrame::Fresh(buf, ..) => buf,
                        }
                    }
                }
                match cached {
                    Some(arc) if arc.len() >= 12 => {
                        let w = u16::from_le_bytes([arc[0], arc[1]]);
                        let h = u16::from_le_bytes([arc[2], arc[3]]);
                        let dx = i32::from_le_bytes([arc[4], arc[5], arc[6], arc[7]]);
                        let dy = i32::from_le_bytes([arc[8], arc[9], arc[10], arc[11]]);
                        Some(EffectFrame::Cached(arc, w, h, dx, dy))
                    }
                    _ => rasterize_node_effects(&params, opts.screen_size, effect_caps).map(
                        |(rgba, w, h, dx, dy)| {
                            if let (Some(cache), Some(key)) = (&opts.effects_cache, key) {
                                let mut stored = Vec::with_capacity(12 + rgba.len());
                                stored.extend_from_slice(&w.to_le_bytes());
                                stored.extend_from_slice(&h.to_le_bytes());
                                stored.extend_from_slice(&dx.to_le_bytes());
                                stored.extend_from_slice(&dy.to_le_bytes());
                                stored.extend_from_slice(&rgba);
                                cache.borrow_mut().insert(key, stored);
                            }
                            EffectFrame::Fresh(rgba, w, h, dx, dy)
                        },
                    ),
                }
            } {
                let (w, h, dx, dy) = frame.dims();
                let rgba = frame.pixels();
                if blended {
                    sink.push_blend(node.blend_mode);
                }
                sink.draw_rgba(rgba, w, h, dx, dy);
                if blended {
                    sink.pop_blend();
                }
                continue;
            }
            // 非 cpu 构建(离屏光栅化未实现)或离屏预算失败:回落直绘,
            // 内容绝不丢(E12 纪律),只是无效果。
        }

        if blended {
            sink.push_blend(node.blend_mode);
        }

        draw_node_content_at_lod(sink, &node.content, detail, world_bbox, total, node.opacity);

        if blended {
            sink.pop_blend();
        }
    }

    // 覆盖层:选中包围盒(1.5px 屏幕等宽)+ 8 向控制柄(6px 方块)
    if opts.selection.is_empty() {
        return;
    }
    let mut selection_union: Option<Rect> = None;
    for &id in &opts.selection {
        // 选中框 hug 真实内容:Text 用实测布局尺寸(V4.0 T6.1)
        let Some(bbox) = node_world_bbox_measured(scene, id) else {
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

/// 单节点内容的 LOD 分派(直绘路径与效果离屏路径**共用**,保证两路对同一
/// 节点产生逐位相同的绘制指令;混合层配对由调用方负责,本函数不碰)。
///
/// - Path:Full = fill + stroke(世界线宽原样传入,模块 doc "对原文的修正");
///   Silhouette = 包围盒色块(保留体量感,跳过描边);Point = 不画。
/// - Text(V3.0 T1):布局(TD-10 缓存)→ 字形轮廓填充,Silhouette 同语义
///   降级为包围盒色块;节点不透明度折入字形颜色 alpha。
/// - Image(资产管线 = M2)与 Group(无直接内容)不产生绘制。
fn draw_node_content_at_lod(
    sink: &mut dyn PaintSink,
    content: &NodeContent,
    detail: DetailLevel,
    world_bbox: Rect,
    total: Affine,
    opacity: f64,
) {
    match content {
        NodeContent::Path(path_node) => match detail {
            DetailLevel::Point => {} // 屏幕上不足 1px:不画
            DetailLevel::Silhouette => {
                if let Some(fill) = &path_node.fill {
                    let silhouette = world_bbox.to_path(0.1);
                    sink.fill_with_opacity(fill, opacity, total, &silhouette);
                }
            }
            DetailLevel::Full => {
                if let Some(fill) = &path_node.fill {
                    sink.fill_with_opacity(fill, opacity, total, &path_node.path);
                }
                if let Some(stroke) = &path_node.stroke {
                    sink.stroke(stroke, total, &path_node.path);
                }
            }
        },
        NodeContent::Text(text_node) => match detail {
            DetailLevel::Point => {}
            DetailLevel::Silhouette => {
                let silhouette = world_bbox.to_path(0.1);
                let fill = Paint::Solid(text_node.color);
                sink.fill_with_opacity(&fill, opacity, total, &silhouette);
            }
            DetailLevel::Full => {
                draw_text_node(sink, text_node, total, opacity);
            }
        },
        NodeContent::Image(_) | NodeContent::Group => {}
    }
}

/// 效果离屏光栅化的单节点参数(压参数计数;`cpu` feature 内部使用)。
#[cfg(feature = "cpu")]
struct EffectRasterParams<'a> {
    /// 节点内容。
    content: &'a NodeContent,
    /// 节点效果栈(按栈序求值)。
    effects: &'a [EffectEntry],
    /// LOD 档位(调用方按包围盒短边 × zoom 判定,与直绘同一结论)。
    detail: DetailLevel,
    /// 世界→像素换算(zoom),支撑域 margin 定尺寸用。
    zoom: f64,
    /// 节点内容世界包围盒(Silhouette 色块与窗口预算用)。
    world_bbox: Rect,
    /// 内容变换 = 视口 × 节点世界变换。
    total: Affine,
    /// 视口变换(屏幕包围盒换算用)。
    vp: Affine,
    /// 节点不透明度。
    opacity: f64,
}

/// 单节点效果离屏管线(G20/T2.1,`cpu` feature):
///
/// 1. 预算离屏窗口:节点屏幕包围盒(整数外扩,保留 AA 分数位)∩(画布 +
///    效果支撑域)。窗口外内容回贴时本来就被画布裁掉,而模糊/投影对可见区
///    的影响被 margin 完整覆盖——已知屏幕尺寸时离屏面积因此有界(视口 +
///    margin),不随极端缩放爆炸;
/// 2. [`effects::EffectSurface`] 透明离屏光栅化:窗口原点平移进表面,内容
///    变换 = 平移 × 视口 × 节点世界变换;LOD 档位与直绘同一判定(见
///    [`draw_node_content_at_lod`]);
/// 3. [`effects::apply_effects_rgba`] 按栈序求值——它**内部自行外扩**容纳
///    支撑域,本函数不预垫 margin,避免双重外扩;
/// 4. 返回 `(rgba, w, h, dx, dy)`,`(dx, dy)` 是结果左上角的**设备像素**
///    坐标(窗口原点 + 效果外扩偏移),调用方经 `draw_rgba` 回贴。
///
/// `None` = 窗口预算失败(空包围盒/画布外退化/超出 u16 上限):调用方回落
/// 直绘路径,内容绝不丢(E12 纪律),只是无效果。
///
/// TD(v4.1 缓存化):每节点每帧一次离屏光栅化 + 全量效果求值,静止场景
/// 重复付费;应按(内容指纹, 变换, 效果参数, 档位, 窗口)为键缓存离屏结果
/// (v4.0 先正确性),参照 `sable_paint::effects::ShadowCache` 的 FIFO
/// 驱逐范式。
/// PERF-05:节点内容指纹。键侧绝不存"版本号"(场景无 per-node revision),
/// 直接对**值**哈希:路径元素逐段坐标位、填充/描边 Paint 全字段、文本串与
/// 字号/颜色、图像 rect+名。哈希成本 ~O(路径段数),远低于离屏光栅化。
#[cfg(feature = "cpu")]
fn hash_paint(h: &mut std::collections::hash_map::DefaultHasher, paint: &Paint) {
    use std::hash::Hash;
    match paint {
        Paint::Solid(c) => {
            0u8.hash(h);
            c.hash(h);
        }
        Paint::LinearGradient { start, end, stops } => {
            1u8.hash(h);
            start.iter().for_each(|v| v.to_bits().hash(h));
            end.iter().for_each(|v| v.to_bits().hash(h));
            for st in stops {
                st.offset.to_bits().hash(h);
                st.color.hash(h);
            }
        }
        Paint::RadialGradient {
            center,
            radius,
            stops,
        } => {
            2u8.hash(h);
            center.iter().for_each(|v| v.to_bits().hash(h));
            radius.to_bits().hash(h);
            for st in stops {
                st.offset.to_bits().hash(h);
                st.color.hash(h);
            }
        }
        Paint::ConicGradient {
            center,
            start_angle,
            end_angle,
            stops,
        } => {
            3u8.hash(h);
            center.iter().for_each(|v| v.to_bits().hash(h));
            start_angle.to_bits().hash(h);
            end_angle.to_bits().hash(h);
            for st in stops {
                st.offset.to_bits().hash(h);
                st.color.hash(h);
            }
        }
    }
}

/// PERF-05:内容指纹(见 [`hash_paint`])。
#[cfg(feature = "cpu")]
fn node_content_fingerprint(content: &NodeContent) -> u64 {
    use kurbo::PathEl;
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    match content {
        NodeContent::Group => 0u8.hash(&mut h),
        NodeContent::Path(p) => {
            1u8.hash(&mut h);
            for el in p.path.elements() {
                match el {
                    PathEl::MoveTo(pt) | PathEl::LineTo(pt) => {
                        0u8.hash(&mut h);
                        pt.x.to_bits().hash(&mut h);
                        pt.y.to_bits().hash(&mut h);
                    }
                    PathEl::QuadTo(c, pt) => {
                        1u8.hash(&mut h);
                        c.x.to_bits().hash(&mut h);
                        c.y.to_bits().hash(&mut h);
                        pt.x.to_bits().hash(&mut h);
                        pt.y.to_bits().hash(&mut h);
                    }
                    PathEl::CurveTo(c0, c1, pt) => {
                        2u8.hash(&mut h);
                        c0.x.to_bits().hash(&mut h);
                        c0.y.to_bits().hash(&mut h);
                        c1.x.to_bits().hash(&mut h);
                        c1.y.to_bits().hash(&mut h);
                        pt.x.to_bits().hash(&mut h);
                        pt.y.to_bits().hash(&mut h);
                    }
                    PathEl::ClosePath => 3u8.hash(&mut h),
                }
            }
            match &p.fill {
                Some(fill) => {
                    1u8.hash(&mut h);
                    hash_paint(&mut h, fill);
                }
                None => 0u8.hash(&mut h),
            }
            match &p.stroke {
                Some(stroke) => {
                    1u8.hash(&mut h);
                    hash_paint(&mut h, &stroke.paint);
                    stroke.width.to_bits().hash(&mut h);
                }
                None => 0u8.hash(&mut h),
            }
        }
        NodeContent::Text(t) => {
            2u8.hash(&mut h);
            t.text.hash(&mut h);
            t.font_size.to_bits().hash(&mut h);
            t.color.hash(&mut h);
        }
        NodeContent::Image(i) => {
            3u8.hash(&mut h);
            i.name.hash(&mut h);
            for v in [i.rect.x0, i.rect.y0, i.rect.x1, i.rect.y1] {
                v.to_bits().hash(&mut h);
            }
        }
    }
    h.finish()
}

/// PERF-05:效果条目指纹(逐字段位哈希)。
#[cfg(feature = "cpu")]
fn hash_effect_entries(
    h: &mut std::collections::hash_map::DefaultHasher,
    entries: &[sable_foundation::effects::EffectEntry],
) {
    use std::hash::Hash;
    for e in entries {
        e.enabled.hash(h);
        match &e.spec {
            sable_foundation::effects::EffectSpec::GaussianBlur { radius } => {
                0u8.hash(h);
                radius.to_bits().hash(h);
            }
            sable_foundation::effects::EffectSpec::DropShadow {
                blur,
                offset,
                color,
            } => {
                1u8.hash(h);
                blur.to_bits().hash(h);
                offset.iter().for_each(|v| v.to_bits().hash(h));
                color.hash(h);
            }
            sable_foundation::effects::EffectSpec::Glow {
                radius,
                color,
                inner,
            } => {
                2u8.hash(h);
                radius.to_bits().hash(h);
                color.hash(h);
                inner.hash(h);
            }
            sable_foundation::effects::EffectSpec::ColorMatrix { matrix, offsets } => {
                3u8.hash(h);
                matrix.iter().for_each(|row| {
                    row.iter().for_each(|v| v.to_bits().hash(h));
                });
                offsets.iter().for_each(|v| v.to_bits().hash(h));
            }
        }
    }
}

/// PERF-05:离屏光栅结果缓存键。覆盖全部决定输出的输入:内容指纹、效果栈、
/// LOD、变换(total 含视口与节点变换)、透明度、屏幕尺寸与窗口几何。
/// 悬停/动画类输入不在键内(它们不经效果路径)。
#[cfg(feature = "cpu")]
fn effects_raster_key(params: &EffectRasterParams, screen_size: (f64, f64)) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    node_content_fingerprint(params.content).hash(&mut h);
    hash_effect_entries(&mut h, params.effects);
    std::mem::discriminant(&params.detail).hash(&mut h);
    for c in params.total.as_coeffs() {
        c.to_bits().hash(&mut h);
    }
    for c in params.vp.as_coeffs() {
        c.to_bits().hash(&mut h);
    }
    params.opacity.to_bits().hash(&mut h);
    screen_size.0.to_bits().hash(&mut h);
    screen_size.1.to_bits().hash(&mut h);
    for v in [
        params.world_bbox.x0,
        params.world_bbox.y0,
        params.world_bbox.x1,
        params.world_bbox.y1,
    ] {
        v.to_bits().hash(&mut h);
    }
    h.finish()
}

#[cfg(feature = "cpu")]
fn rasterize_node_effects(
    params: &EffectRasterParams,
    screen_size: (f64, f64),
    caps: EffectCaps,
) -> Option<(Vec<u8>, u16, u16, i32, i32)> {
    let EffectRasterParams {
        content,
        effects: effects_entries,
        detail,
        zoom,
        world_bbox,
        total,
        vp,
        opacity,
    } = *params;
    // 节点屏幕包围盒(vp = 缩放 + 平移,变换两对角点归一即可)
    let p0 = vp * Point::new(world_bbox.x0, world_bbox.y0);
    let p1 = vp * Point::new(world_bbox.x1, world_bbox.y1);
    let (bx0, bx1) = (p0.x.min(p1.x), p0.x.max(p1.x));
    let (by0, by1) = (p0.y.min(p1.y), p0.y.max(p1.y));
    let mut wx0 = bx0.floor();
    let mut wy0 = by0.floor();
    let mut wx1 = bx1.ceil();
    let mut wy1 = by1.ceil();

    // 与"画布 ± 支撑域"相交(margin 上取整,宁可多留)。注意方向:**对侧**
    // margin——内容像素 p 能影响画布,当且仅当 p ∈ [-m_r, w+m_l](p 处的
    // 结果向左最多伸 m_l、向右最多伸 m_r;故左边界由右侧 margin 决定,
    // 反之亦然)。已知屏幕尺寸时离屏面积因此有界(视口 + margin),不随
    // 极端缩放爆炸。
    let [m_l, m_t, m_r, m_b] = effects::effect_margins_px(effects_entries, zoom);
    if screen_size.0 > 0.0 && screen_size.1 > 0.0 {
        wx0 = wx0.max(-m_r.ceil());
        wy0 = wy0.max(-m_b.ceil());
        wx1 = wx1.min(screen_size.0 + m_l.ceil());
        wy1 = wy1.min(screen_size.1 + m_t.ceil());
    }
    let (ww, wh) = ((wx1 - wx0).round() as i64, (wy1 - wy0).round() as i64);
    if ww <= 0 || wh <= 0 {
        return None; // 窗口退化(空包围盒/完全在画布+margin 之外):回落直绘
    }
    let Ok(w16) = u16::try_from(ww) else {
        return None; // 极端参数:宁可无效果,不溢出(u16::MAX 以上不可表示)
    };
    let Ok(h16) = u16::try_from(wh) else {
        return None;
    };

    // 离屏光栅化:窗口原点平移进表面(内容落位 = 屏幕坐标 − 窗口原点)
    let mut surface = effects::EffectSurface::new(w16, h16);
    let total_off = Affine::translate((-wx0, -wy0)) * total;
    surface.draw(|off| {
        draw_node_content_at_lod(off, content, detail, world_bbox, total_off, opacity);
    });
    let (rgba, w, h) = surface.into_rgba();

    // 效果求值(栈序;内部按需外扩,返回相对窗口原点的外扩偏移 ≤ 0)
    let (rgba, dx, dy, w, h) = effects::apply_effects_rgba((rgba, w, h), effects_entries, caps);

    // 设备像素偏移 = 窗口原点 + 效果外扩偏移(i64 域夹取后转 i32,回贴由
    // 光栅器按画布裁剪)
    let ox = (wx0 as i64)
        .saturating_add(i64::from(dx))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX));
    let oy = (wy0 as i64)
        .saturating_add(i64::from(dy))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX));
    Some((rgba, w, h, ox as i32, oy as i32))
}

/// 渲染一个文本节点(V3.0 T1):布局(TD-10 缓存)→ 字形轮廓填充。
///
/// 空文本短路为零绘制;节点不透明度折入字形颜色 alpha(字形走
/// `sink.fill` 直通,不经 `fill_with_opacity`)。
fn draw_text_node(sink: &mut dyn PaintSink, text_node: &TextNode, total: Affine, opacity: f64) {
    if text_node.text.is_empty() {
        return; // 契约:空文本零覆盖(parley 空串会排出带行高的空行)
    }
    let layout = text_glyphs::layout_text(&text_node.text, text_node.font_size, text_node.color);
    let color = with_opacity(text_node.color, opacity);
    let mut cache_hits: u64 = 0;
    let mut cache_misses: u64 = 0;
    text_glyphs::draw_text(
        sink,
        &layout,
        total,
        color,
        &mut cache_hits,
        &mut cache_misses,
    );
}

/// 节点不透明度折入纯色 alpha(`opacity < 1.0` 时乘 alpha 通道)。
fn with_opacity(color: Rgba8, opacity: f64) -> Rgba8 {
    let mut color = color;
    if opacity < 1.0 {
        let alpha = f64::from(color[3]) * opacity.clamp(0.0, 1.0);
        color[3] = alpha.round() as u8;
    }
    color
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
    use sable_foundation::effects::EffectSpec;
    use sable_foundation::scene::PathNode;

    /// 记录型 sink:数 fill/stroke 调用,存下变换与路径包围盒供断言;
    /// 另记统一事件流(含混合层 push/pop 与 draw_rgba 回贴)供顺序断言。
    struct RecordingSink {
        fills: Vec<(Rgba8, Affine, Rect)>,
        strokes: Vec<(Rgba8, f64, Affine, Rect)>,
        /// 统一事件流:fill/stroke/push_blend/pop_blend/draw_rgba 按发生顺序记录。
        events: Vec<Event>,
        /// 效果管线门槛:是否声明 draw_rgba 回贴能力(`new()` 默认 false,
        /// 效果路径不触发;`rgba_capable()` 打开)。
        rgba_capable: bool,
    }

    /// [`RecordingSink::events`] 的事件种类。
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Event {
        Fill,
        Stroke,
        PushBlend(BlendMode),
        PopBlend,
        /// draw_rgba 回贴(尺寸 + 设备像素偏移)。
        DrawRgba {
            w: u16,
            h: u16,
            dx: i32,
            dy: i32,
        },
    }

    impl RecordingSink {
        fn new() -> Self {
            RecordingSink {
                fills: Vec::new(),
                strokes: Vec::new(),
                events: Vec::new(),
                rgba_capable: false,
            }
        }

        /// 声明支持 draw_rgba 的记录 sink(效果路径门槛打开)。
        fn rgba_capable() -> Self {
            let mut sink = Self::new();
            sink.rgba_capable = true;
            sink
        }

        fn paint_solid(paint: &Paint) -> Rgba8 {
            match paint {
                Paint::Solid(c) => *c,
                _ => [0, 0, 0, 0],
            }
        }

        /// 最后一个 PopBlend 事件的下标(无则 None)。
        fn last_pop_index(&self) -> Option<usize> {
            self.events.iter().rposition(|e| *e == Event::PopBlend)
        }
    }

    impl PaintSink for RecordingSink {
        fn fill(&mut self, paint: &Paint, transform: Affine, path: &BezPath) {
            self.fills
                .push((Self::paint_solid(paint), transform, path.bounding_box()));
            self.events.push(Event::Fill);
        }

        fn stroke(&mut self, style: &StrokeStyle, transform: Affine, path: &BezPath) {
            self.strokes.push((
                Self::paint_solid(&style.paint),
                style.width,
                transform,
                path.bounding_box(),
            ));
            self.events.push(Event::Stroke);
        }

        fn push_blend(&mut self, mode: BlendMode) {
            self.events.push(Event::PushBlend(mode));
        }

        fn pop_blend(&mut self) {
            self.events.push(Event::PopBlend);
        }

        fn draw_rgba(&mut self, _rgba: &[u8], w: u16, h: u16, dx: i32, dy: i32) {
            self.events.push(Event::DrawRgba { w, h, dx, dy });
        }

        fn supports_draw_rgba(&self) -> bool {
            self.rgba_capable
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
        use sable_paint::cpu::CpuRenderer;

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

        /// 文本场景 → CpuRenderer 96×96:真字形轮廓产生非零暗色像素覆盖
        /// (V3.0 T1 验收:非占位框,黑字白底按红通道计数)。
        #[test]
        fn text_scene_rasterizes_nonzero_coverage() {
            let mut scene = Scene::new();
            scene
                .add_node(
                    None,
                    "标题",
                    NodeContent::Text(TextNode {
                        text: "你好 Aa 123".to_string(),
                        font_size: 24.0,
                        color: [0, 0, 0, 255],
                    }),
                )
                .expect("文本节点");

            crate::text_glyphs::reset_text_cache();
            let mut renderer = CpuRenderer::new(96, 96, WHITE);
            let opts = RenderOpts {
                screen_size: (96.0, 96.0),
                ..RenderOpts::default()
            };
            render_scene(&scene, &viewport_at_origin(1.0), renderer.sink(), &opts);
            let buf = renderer.finish();

            let ink = buf.chunks_exact(4).filter(|p| p[0] < 128).count();
            assert!(ink > 0, "文本必须产生非零像素覆盖(真字形轮廓),实际 0");
        }

        /// 空文本:零覆盖(整幅保持底色)。
        #[test]
        fn empty_text_scene_rasterizes_zero_coverage() {
            let mut scene = Scene::new();
            scene
                .add_node(
                    None,
                    "空文本",
                    NodeContent::Text(TextNode {
                        text: String::new(),
                        font_size: 24.0,
                        color: [0, 0, 0, 255],
                    }),
                )
                .expect("空文本节点");

            let mut renderer = CpuRenderer::new(96, 96, WHITE);
            let opts = RenderOpts {
                screen_size: (96.0, 96.0),
                ..RenderOpts::default()
            };
            render_scene(&scene, &viewport_at_origin(1.0), renderer.sink(), &opts);
            let buf = renderer.finish();
            let ink = buf.chunks_exact(4).filter(|p| p[0] < 128).count();
            assert_eq!(ink, 0, "空文本必须零覆盖");
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

    // —— 文本节点(V3.0 T1:渲染接入 / TD-10 缓存 / LOD / 不透明度)——

    fn text_node_content(text: &str, font_size: f64, color: Rgba8) -> NodeContent {
        NodeContent::Text(TextNode {
            text: text.to_string(),
            font_size,
            color,
        })
    }

    /// 同键两次 render_scene:第二次必须命中 TD-10 布局缓存(misses == 1),
    /// 且两次绘制的字形 fill 数一致。
    #[test]
    fn same_text_key_second_render_hits_layout_cache() {
        crate::text_glyphs::reset_text_cache();
        let mut scene = Scene::new();
        scene
            .add_node(None, "文本", text_node_content("cache me", 20.0, red()))
            .expect("文本节点");

        let mut first = RecordingSink::new();
        render_scene(
            &scene,
            &viewport_at_origin(1.0),
            &mut first,
            &RenderOpts::default(),
        );
        assert!(!first.fills.is_empty(), "首次渲染必须画出字形");

        let mut second = RecordingSink::new();
        render_scene(
            &scene,
            &viewport_at_origin(1.0),
            &mut second,
            &RenderOpts::default(),
        );

        assert_eq!(
            second.fills.len(),
            first.fills.len(),
            "命中缓存的渲染必须画出同样多的字形 fill"
        );
        let (hits, misses) = text_glyphs::cache_stats();
        assert_eq!(misses, 1, "同键两次 render_scene 只允许一次重排");
        assert!(hits >= 1, "第二次渲染必须命中布局缓存");
    }

    /// 文本 LOD 与 Path 同语义:Silhouette = 单个包围盒色块(节点色),
    /// Point = 不画,Full = 真字形(≥1 次 fill)。特征尺寸取**实测**包围盒
    /// 短边("lod" @10px ≈ 13×12,短边 = 行高),LOD 阈值结论与粗估时代一致。
    #[test]
    fn text_lod_degrades_and_skips_like_paths() {
        let mut scene = Scene::new();
        scene
            .add_node(None, "文本", text_node_content("lod", 10.0, red()))
            .expect("文本节点");
        let opts = RenderOpts {
            screen_size: (64.0, 64.0),
            ..RenderOpts::default()
        };

        // zoom=0.3:屏幕 3.6px → Silhouette
        let mut sink = RecordingSink::new();
        render_scene(&scene, &viewport_at_origin(0.3), &mut sink, &opts);
        assert_eq!(sink.fills.len(), 1, "Silhouette 降级为单个包围盒色块");
        let (color, _, _) = sink.fills[0];
        assert_eq!(color, red(), "Silhouette 色块取文本节点色");

        // zoom=0.05:屏幕 0.6px → Point
        let mut sink = RecordingSink::new();
        render_scene(&scene, &viewport_at_origin(0.05), &mut sink, &opts);
        assert_eq!(sink.fills.len(), 0, "屏幕不足 1px 不画");

        // zoom=1:屏幕 12px → Full
        let mut sink = RecordingSink::new();
        render_scene(&scene, &viewport_at_origin(1.0), &mut sink, &opts);
        assert!(!sink.fills.is_empty(), "Full 级必须画真字形");
    }

    /// 节点不透明度折入字形颜色 alpha(opacity 0.5 → alpha ≈ 128)。
    #[test]
    fn text_node_opacity_scales_color_alpha() {
        let mut scene = Scene::new();
        let id = scene
            .add_node(
                None,
                "半透明文本",
                text_node_content("op", 20.0, [255, 0, 0, 255]),
            )
            .expect("文本节点");
        scene.node_mut(id).expect("在").opacity = 0.5;

        let mut sink = RecordingSink::new();
        render_scene(
            &scene,
            &viewport_at_origin(1.0),
            &mut sink,
            &RenderOpts::default(),
        );
        assert!(!sink.fills.is_empty(), "半透明文本仍须绘制");
        for (color, _, _) in &sink.fills {
            assert_eq!(color[0], 255, "红通道保持");
            assert_eq!(color[3], 128, "alpha 应为 255×0.5=128,实际 {color:?}");
        }
    }

    /// 空文本节点:零绘制调用(渲染层短路,不进布局缓存)。
    #[test]
    fn empty_text_node_emits_no_draw_calls() {
        crate::text_glyphs::reset_text_cache();
        let (before_hits, before_misses) = text_glyphs::cache_stats();
        let mut scene = Scene::new();
        scene
            .add_node(None, "空文本", text_node_content("", 20.0, red()))
            .expect("空文本节点");

        let mut sink = RecordingSink::new();
        render_scene(
            &scene,
            &viewport_at_origin(1.0),
            &mut sink,
            &RenderOpts::default(),
        );
        assert_eq!(sink.fills.len(), 0, "空文本零绘制");
        assert_eq!(
            text_glyphs::cache_stats(),
            (before_hits, before_misses),
            "空文本不进布局缓存"
        );
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

    // —— 混合模式(E5:逐节点 push_blend/pop_blend)——

    #[test]
    fn blend_node_wraps_draw_in_push_pop_pair() {
        let mut scene = Scene::new();
        let id = scene
            .add_node(None, "混合矩形", rect_content(0.0, 0.0, 10.0, 10.0, red()))
            .expect("节点");
        scene.node_mut(id).expect("在").blend_mode = BlendMode::Multiply;

        let mut sink = RecordingSink::new();
        render_scene(
            &scene,
            &viewport_at_origin(1.0),
            &mut sink,
            &RenderOpts::default(),
        );
        assert_eq!(
            sink.events,
            vec![
                Event::PushBlend(BlendMode::Multiply),
                Event::Fill,
                Event::PopBlend
            ],
            "非 Normal 节点必须被一对 push/pop 混合层完整包裹"
        );
    }

    #[test]
    fn normal_node_emits_no_blend_ops() {
        let mut scene = Scene::new();
        scene
            .add_node(None, "普通矩形", rect_content(0.0, 0.0, 10.0, 10.0, red()))
            .expect("节点");

        let mut sink = RecordingSink::new();
        render_scene(
            &scene,
            &viewport_at_origin(1.0),
            &mut sink,
            &RenderOpts::default(),
        );
        assert!(
            !sink
                .events
                .iter()
                .any(|e| matches!(e, Event::PushBlend(_) | Event::PopBlend)),
            "Normal 节点不得产生混合层调用"
        );
    }

    #[test]
    fn culled_and_point_level_nodes_do_not_leak_blend_layer() {
        let mut scene = Scene::new();
        // 视口外的混合节点(剔除分支在 push 之前,不得开层)。
        // zoom=0.05 时可见世界区 = (0,0)-(1280,1280),所以"远处"放在 5000。
        let far = scene
            .add_node(
                None,
                "远处",
                rect_content(5000.0, 5000.0, 5010.0, 5010.0, red()),
            )
            .expect("节点1");
        scene.node_mut(far).expect("在").blend_mode = BlendMode::Screen;
        // 屏幕上不足 1px 的混合节点(Point LOD 分支)
        let tiny = scene
            .add_node(None, "极小", rect_content(0.0, 0.0, 10.0, 10.0, red()))
            .expect("节点2");
        scene.node_mut(tiny).expect("在").blend_mode = BlendMode::Screen;

        // 极小节点 0.05 zoom → 10×0.05 = 0.5px 屏幕尺寸 → Point
        let mut sink = RecordingSink::new();
        let opts = RenderOpts {
            screen_size: (64.0, 64.0),
            ..RenderOpts::default()
        };
        render_scene(&scene, &viewport_at_origin(0.05), &mut sink, &opts);
        let pushes = sink
            .events
            .iter()
            .filter(|e| matches!(e, Event::PushBlend(_)))
            .count();
        let pops = sink
            .events
            .iter()
            .filter(|e| **e == Event::PopBlend)
            .count();
        assert_eq!(pushes, pops, "push/pop 必须配对,任何分支不得泄漏混合层");
        assert_eq!(pushes, 1, "只有 Point 分支的节点开层(远处节点被剔除)");
    }

    #[test]
    fn overlay_is_drawn_outside_blend_layers() {
        let mut scene = Scene::new();
        let id = scene
            .add_node(None, "混合矩形", rect_content(0.0, 0.0, 10.0, 10.0, red()))
            .expect("节点");
        scene.node_mut(id).expect("在").blend_mode = BlendMode::Difference;

        let mut sink = RecordingSink::new();
        let opts = RenderOpts {
            selection: vec![id],
            screen_size: (64.0, 64.0),
            ..RenderOpts::default()
        };
        render_scene(&scene, &viewport_at_origin(1.0), &mut sink, &opts);

        // 恰一对混合层;选中框/控制柄的全部描边发生在混合层之外
        assert_eq!(
            sink.events
                .iter()
                .filter(|e| matches!(e, Event::PushBlend(BlendMode::Difference)))
                .count(),
            1
        );
        let last_pop = sink.last_pop_index().expect("有 PopBlend");
        let first_stroke = sink
            .events
            .iter()
            .position(|e| *e == Event::Stroke)
            .expect("选中框必有描边");
        assert!(
            first_stroke > last_pop,
            "覆盖层(选中框/控制柄)必须画在混合层之外,不受节点混合影响"
        );
        // 选中框 + 8 控制柄描边全部存在
        assert_eq!(sink.strokes.len(), 9);
    }

    // —— 效果栈(G20/V4.0 T2:render_scene 消费 node.effects)——

    /// 投影效果条目(硬影 [+6,0],纯黑;blur=0 便于精确推算窗口尺寸)。
    fn shadow_entry() -> EffectEntry {
        EffectEntry {
            spec: EffectSpec::DropShadow {
                blur: 0.0,
                offset: [6.0, 0.0],
                color: [0, 0, 0, 255],
            },
            enabled: true,
        }
    }

    /// T2.2 顺序锁定:**效果先于混合** —— 效果回贴(draw_rgba)发生在该
    /// 节点的 push/pop 混合层**之内**:效果先作用于节点内容,其结果再作为
    /// 整体参与混合;且效果节点不得再直绘内容(离屏结果替代原绘制)。
    #[test]
    fn effect_composite_is_wrapped_by_blend_layer() {
        let mut scene = Scene::new();
        let id = scene
            .add_node(None, "效果+混合", rect_content(0.0, 0.0, 10.0, 10.0, red()))
            .expect("节点");
        scene.node_mut(id).expect("在").blend_mode = BlendMode::Multiply;
        scene.node_mut(id).expect("在").effects = vec![shadow_entry()];

        let mut sink = RecordingSink::rgba_capable();
        render_scene(
            &scene,
            &viewport_at_origin(1.0),
            &mut sink,
            &RenderOpts::default(),
        );
        assert_eq!(sink.events.len(), 3, "必须恰为 push/draw_rgba/pop 三事件");
        assert_eq!(sink.events[0], Event::PushBlend(BlendMode::Multiply));
        assert!(
            matches!(sink.events[1], Event::DrawRgba { .. }),
            "回贴必须落在混合层之内(效果先于混合),实际 {:?}",
            sink.events[1]
        );
        assert_eq!(sink.events[2], Event::PopBlend);
        assert!(
            !sink.events.contains(&Event::Fill),
            "效果节点的内容经离屏求值回贴,不得再直绘"
        );
    }

    /// 空效果栈零开销路径:即使后端声明 draw_rgba 能力,空栈节点也必须走
    /// 直绘(恰一次 fill),不得进离屏分支(v3.0 输出逐位保持的门槛前提)。
    #[test]
    fn empty_effect_stack_takes_direct_draw_path() {
        let mut scene = Scene::new();
        scene
            .add_node(None, "普通矩形", rect_content(0.0, 0.0, 10.0, 10.0, red()))
            .expect("节点");

        let mut sink = RecordingSink::rgba_capable();
        render_scene(
            &scene,
            &viewport_at_origin(1.0),
            &mut sink,
            &RenderOpts::default(),
        );
        assert_eq!(sink.events, vec![Event::Fill], "空栈必须直绘、零效果调用");
    }

    /// 全禁用(或恒等)效果条目与空栈同语义:is_active 门槛拦下,不进
    /// 离屏分支(禁用条目保留参数但渲染时整条跳过——数据模型契约)。
    #[test]
    fn disabled_effect_entries_take_direct_draw_path() {
        let mut scene = Scene::new();
        let id = scene
            .add_node(None, "禁用效果", rect_content(0.0, 0.0, 10.0, 10.0, red()))
            .expect("节点");
        scene.node_mut(id).expect("在").effects = vec![EffectEntry {
            enabled: false,
            ..shadow_entry()
        }];

        let mut sink = RecordingSink::rgba_capable();
        render_scene(
            &scene,
            &viewport_at_origin(1.0),
            &mut sink,
            &RenderOpts::default(),
        );
        assert_eq!(sink.events, vec![Event::Fill], "全禁用栈必须与空栈同路径");
    }

    /// 效果节点经 draw_rgba 回贴恰一次,窗口与偏移可精确推算:10×10 矩形
    /// 在原点,硬影 offset [+6,0] → 支撑域右侧 6px → 窗口 16×10、回贴
    /// 偏移 (0,0)(左侧未外扩)。
    #[test]
    fn effect_node_composites_via_single_draw_rgba() {
        let mut scene = Scene::new();
        let id = scene
            .add_node(None, "投影矩形", rect_content(0.0, 0.0, 10.0, 10.0, red()))
            .expect("节点");
        scene.node_mut(id).expect("在").effects = vec![shadow_entry()];

        let mut sink = RecordingSink::rgba_capable();
        render_scene(
            &scene,
            &viewport_at_origin(1.0),
            &mut sink,
            &RenderOpts::default(),
        );
        assert_eq!(
            sink.events,
            vec![Event::DrawRgba {
                w: 16,
                h: 10,
                dx: 0,
                dy: 0
            }],
            "效果节点必须恰一次回贴,窗口 = bbox + 偏移方向支撑域"
        );
    }

    /// Point 档效果节点:内容都不画,效果无从作用 —— 零 draw_rgba;混合
    /// 节点仍弹掉空混合层(与直绘 Point 分支的 push/pop 配对一致)。
    #[test]
    fn point_level_effect_node_skips_rasterization_but_keeps_blend_pairing() {
        let mut scene = Scene::new();
        let id = scene
            .add_node(None, "极小效果", rect_content(0.0, 0.0, 10.0, 10.0, red()))
            .expect("节点");
        scene.node_mut(id).expect("在").blend_mode = BlendMode::Screen;
        scene.node_mut(id).expect("在").effects = vec![shadow_entry()];

        // 10×0.05 = 0.5px 屏幕 → Point
        let mut sink = RecordingSink::rgba_capable();
        let opts = RenderOpts {
            screen_size: (64.0, 64.0),
            ..RenderOpts::default()
        };
        render_scene(&scene, &viewport_at_origin(0.05), &mut sink, &opts);
        assert_eq!(
            sink.events,
            vec![Event::PushBlend(BlendMode::Screen), Event::PopBlend],
            "Point 档不得离屏/回贴,混合层必须保持空配对"
        );
    }

    /// 阴影可投进视口:节点 bbox 在视口外、但"bbox + 效果支撑域"与视口
    /// 相交时不得被剔除(向视口方向的投影必须存活);远离视口的同款节点
    /// 仍照常剔除。
    #[test]
    fn effect_node_reaching_into_viewport_is_not_culled() {
        let opts = RenderOpts {
            screen_size: (64.0, 64.0),
            ..RenderOpts::default()
        };
        let build = |x0: f64, offset_x: f64| {
            let mut scene = Scene::new();
            let id = scene
                .add_node(
                    None,
                    "远处投影",
                    rect_content(x0, 0.0, x0 + 10.0, 10.0, red()),
                )
                .expect("节点");
            scene.node_mut(id).expect("在").effects = vec![EffectEntry {
                spec: EffectSpec::DropShadow {
                    blur: 0.0,
                    offset: [offset_x, 0.0],
                    color: [0, 0, 0, 255],
                },
                enabled: true,
            }];
            scene
        };

        // 影子向左 30px:bbox (70..80) 在视口 (0..64) 外,外扩后 x0=40 < 64 → 保留
        let mut sink = RecordingSink::rgba_capable();
        render_scene(
            &build(70.0, -30.0),
            &viewport_at_origin(1.0),
            &mut sink,
            &opts,
        );
        assert!(
            matches!(sink.events[0], Event::DrawRgba { .. }),
            "投影伸入视口的节点不得被剔除,实际 {:?}",
            sink.events
        );

        // 节点与影子都在视口外((200..210),影子向右到 240)→ 照常剔除
        let mut sink = RecordingSink::rgba_capable();
        render_scene(
            &build(200.0, 30.0),
            &viewport_at_origin(1.0),
            &mut sink,
            &opts,
        );
        assert!(sink.events.is_empty(), "视口外且影子也在视口外的节点应剔除");
    }

    #[cfg(feature = "cpu")]
    mod blend_pixel {
        use super::*;
        use sable_paint::cpu::CpuRenderer;

        /// 混合模式真正到达像素:同色灰 multiply 把背景压暗(vello_cpu 真实现);
        /// 矩形外不受影响。
        #[test]
        fn multiply_node_darkens_pixels() {
            let gray: Rgba8 = [180, 180, 180, 255];
            let mut scene = Scene::new();
            let id = scene
                .add_node(None, "灰", rect_content(16.0, 16.0, 48.0, 48.0, gray))
                .expect("节点");
            scene.node_mut(id).expect("在").blend_mode = BlendMode::Multiply;

            let mut renderer = CpuRenderer::new(64, 64, gray);
            let opts = RenderOpts {
                screen_size: (64.0, 64.0),
                ..RenderOpts::default()
            };
            render_scene(&scene, &viewport_at_origin(1.0), renderer.sink(), &opts);
            let buf = renderer.finish();

            let i = 4 * (32 * 64 + 32);
            let inside = [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]];
            assert!(
                i32::from(inside[1]) < 160,
                "multiply(s=d=180) 应暗于 180,实际 {inside:?}"
            );
            let j = 4 * (2 * 64 + 2);
            let outside = [buf[j], buf[j + 1], buf[j + 2], buf[j + 3]];
            assert_eq!(outside, gray, "混合矩形外保持底色");
        }
    }

    // —— 效果栈像素回归(G20/T2.1,CpuRenderer 64×64 真实回贴)——

    #[cfg(feature = "cpu")]
    mod effect_pixel {
        use super::*;
        use sable_paint::cpu::CpuRenderer;

        const WHITE: Rgba8 = [255, 255, 255, 255];

        fn pixel(buf: &[u8], x: u16, y: u16) -> [u8; 4] {
            let i = 4 * (usize::from(y) * 64 + usize::from(x));
            [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
        }

        fn render(scene: &Scene, level: Option<EffectLevel>) -> Vec<u8> {
            let opts = RenderOpts {
                screen_size: (64.0, 64.0),
                effect_level: level,
                ..RenderOpts::default()
            };
            let mut renderer = CpuRenderer::new(64, 64, WHITE);
            render_scene(scene, &viewport_at_origin(1.0), renderer.sink(), &opts);
            renderer.finish()
        }

        /// 高斯模糊场景:32×32 红矩形(16..48),`with_effect` 决定是否挂
        /// radius=6 的模糊(支撑域 = 3×box 半径 = 9px)。
        fn blur_scene(with_effect: bool) -> Scene {
            let mut scene = Scene::new();
            let id = scene
                .add_node(None, "红", rect_content(16.0, 16.0, 48.0, 48.0, red()))
                .expect("节点");
            if with_effect {
                scene.node_mut(id).expect("在").effects = vec![EffectEntry {
                    spec: EffectSpec::GaussianBlur { radius: 6.0 },
                    enabled: true,
                }];
            }
            scene
        }

        /// TC-PERF-SHADOW-01(PERF-05):静止场景二次渲染全命中零光栅化,
        /// 且命中帧像素与首帧/无缓存路径逐位一致。
        #[test]
        fn tc_perf_shadow_01_idle_scene_second_render_is_full_hit_and_bitwise_identical() {
            let scene = blur_scene(true);
            let cache = std::rc::Rc::new(std::cell::RefCell::new(
                sable_paint::effects::ShadowCache::new(16),
            ));
            let render_with =
                |cache: &std::rc::Rc<std::cell::RefCell<sable_paint::effects::ShadowCache>>| {
                    let opts = RenderOpts {
                        screen_size: (64.0, 64.0),
                        effects_cache: Some(cache.clone()),
                        ..RenderOpts::default()
                    };
                    let mut renderer = CpuRenderer::new(64, 64, WHITE);
                    render_scene(&scene, &viewport_at_origin(1.0), renderer.sink(), &opts);
                    renderer.finish()
                };

            let first = render_with(&cache);
            assert_eq!(cache.borrow().misses(), 1, "首帧:恰一次光栅化未命中");
            let second = render_with(&cache);
            assert_eq!(cache.borrow().misses(), 1, "静止二次渲染:零新增未命中");
            assert!(cache.borrow().hits() >= 1, "静止二次渲染:至少一次命中");
            assert_eq!(first, second, "命中帧与首帧逐位一致");

            // 无缓存对照:缓存路径不改变像素(golden 同款确定性)
            let plain = render(&blur_scene(true), None);
            assert_eq!(first, plain, "缓存开/关输出逐位一致");
        }

        /// T2.1a 高斯模糊:同一场景开/关效果像素必有差异,且模糊后边缘向
        /// 外扩散(原矩形外 6px、支撑域 9px 之内出现红色覆盖);支撑域外
        /// 的矩形深处保持原样。
        #[test]
        fn gaussian_blur_changes_pixels_and_spreads_edges() {
            let without = render(&blur_scene(false), None);
            let with = render(&blur_scene(true), None);

            assert_ne!(without, with, "开/关效果必须有像素差异");
            // 原矩形左缘外 6px:无效果 = 纯白底;模糊后有覆盖(边缘扩散)
            assert_eq!(pixel(&without, 10, 32), WHITE, "无效果时矩形外是纯底色");
            let spread = pixel(&with, 10, 32);
            assert!(
                spread[3] > 0 && spread != WHITE,
                "模糊后矩形外 6px 应有扩散覆盖,实际 {spread:?}"
            );
            // 距边缘 16px > 支撑域 9px:矩形深处逐位不变
            assert_eq!(
                pixel(&with, 32, 32),
                [255, 0, 0, 255],
                "支撑域外的矩形内部保持不透明红"
            );
        }

        /// T2.1b 投影:偏移方向上出现阴影像素 —— 硬影 offset [+6,0]、
        /// α=140 黑:矩形右侧 6px 外的影子专属区变中性灰(255·(1−140/255)
        /// ≈ 115),反方向与矩形本体不变。
        #[test]
        fn drop_shadow_appears_in_offset_direction() {
            let shadow_scene = |with_effect: bool| {
                let mut scene = Scene::new();
                let id = scene
                    .add_node(None, "红", rect_content(8.0, 24.0, 24.0, 40.0, red()))
                    .expect("节点");
                if with_effect {
                    scene.node_mut(id).expect("在").effects = vec![EffectEntry {
                        spec: EffectSpec::DropShadow {
                            blur: 0.0,
                            offset: [6.0, 0.0],
                            color: [0, 0, 0, 140],
                        },
                        enabled: true,
                    }];
                }
                scene
            };
            let without = render(&shadow_scene(false), None);
            let buf = render(&shadow_scene(true), None);

            // 影子专属区(矩形右侧,base 未覆盖):无效果 = 白,有效果 = 中性灰
            assert_eq!(pixel(&without, 28, 32), WHITE, "无效果时该处是纯底色");
            let shadow = pixel(&buf, 28, 32);
            assert!(
                (i32::from(shadow[0]) - 115).abs() <= 4,
                "偏移方向上应出现阴影像素(≈115 灰),实际 {shadow:?}"
            );
            assert!(
                (i32::from(shadow[0]) - i32::from(shadow[1])).abs() <= 2
                    && (i32::from(shadow[1]) - i32::from(shadow[2])).abs() <= 2,
                "黑影叠白底应为中性灰,实际 {shadow:?}"
            );
            // 反方向(矩形左侧)保持底色
            assert_eq!(pixel(&buf, 4, 32), WHITE, "偏移反方向不得出现阴影");
            // 矩形本体不透明,base over shadow 后保持红
            assert_eq!(pixel(&buf, 16, 32), [255, 0, 0, 255], "矩形本体保持红");
        }

        /// T2.1c Off 档:输出与无效果基线**逐位一致**(E12 军规)。
        ///
        /// 档位来源链:`SABLE_EFFECTS_LEVEL=off` → `detect_with` →
        /// `caps(Off).blur == false`(sable-paint 既有单测)→ 本测试以
        /// `RenderOpts::effect_level` 直控同一能力位,等价锁定 env 路径。
        /// 对照组用 `Some(Reduced)` 显式开档,防止断言空转。
        #[test]
        fn off_level_output_is_bitwise_identical_to_no_effects() {
            let effect_scene = |with_effects: bool| {
                let mut scene = Scene::new();
                let a = scene
                    .add_node(None, "模糊", rect_content(16.0, 16.0, 48.0, 48.0, red()))
                    .expect("模糊节点");
                let b = scene
                    .add_node(None, "投影", rect_content(0.0, 0.0, 12.0, 12.0, blue()))
                    .expect("投影节点");
                if with_effects {
                    scene.node_mut(a).expect("在").effects = vec![EffectEntry {
                        spec: EffectSpec::GaussianBlur { radius: 6.0 },
                        enabled: true,
                    }];
                    scene.node_mut(b).expect("在").effects = vec![EffectEntry {
                        spec: EffectSpec::DropShadow {
                            blur: 4.0,
                            offset: [6.0, 6.0],
                            color: [0, 0, 0, 150],
                        },
                        enabled: true,
                    }];
                }
                scene
            };
            let with = effect_scene(true);
            let without = effect_scene(false);

            let off = render(&with, Some(EffectLevel::Off));
            let baseline = render(&without, None);
            assert_eq!(
                off, baseline,
                "Off 档输出必须与无效果基线逐位一致(E12 军规)"
            );

            let on = render(&with, Some(EffectLevel::Reduced));
            assert_ne!(on, off, "Reduced 档必须真实进效果路径(否则断言空转)");
        }
    }
}
