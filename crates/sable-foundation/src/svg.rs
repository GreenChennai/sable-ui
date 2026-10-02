//! SVG 互通(V2.0 T1 起,docs/08;V3.0 T2 渐变完整映射收尾):
//! 场景图 → SVG 字符串(导出,零额外依赖手写序列化)与
//! SVG → 场景图(导入,usvg 0.48 解析)。
//!
//! 范围:路径/组/变换/纯色填充/描边宽度颜色/混合模式;线性与径向渐变
//! 双向完整映射(几何/全部色标/stop 透明度;导入侧 gradientTransform
//! 折入几何坐标);Conic 渐变导出降级为首停纯色(SVG 1.1 无锥形
//! paint server,doc 保留说明);Text 双向互通(V4.0 T3:导出 `<text>`,
//! 导入消费 usvg 解析期轮廓化好的 `Text::flattened()` 路径组,与场景
//! "文本渲染走字形轮廓"同模型;仅轮廓组缺失/为空——典型无字体环境——
//! 才跳过计数);Image 导入跳过并计数;`<pattern>` 导入降级中性灰计数;
//! reflect/repeat `spreadMethod` 按 pad 近似并计数(G26);效果
//! (EffectSpec)不进 SVG(filter 映射 = v2.1,doc 记录)。

use crate::error::CoreResult;
use crate::scene::{
    BlendMode, GradientStop, NodeContent, Paint, PathNode, Rgba8, Scene, StrokeStyle,
};
use kurbo::{Affine, BezPath};
use usvg::tiny_skia_path;

/// 导入上限:输入字节数(分册六 §1.3 恶意输入防御)。
pub const MAX_SVG_BYTES: usize = 10 * 1024 * 1024;
/// 导入上限:嵌套组深度。
const MAX_DEPTH: usize = 64;

/// 导入统计:跳过/降级项一目了然(不静默)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// 成功导入的节点数(含组;V4.0 T3 起文本轮廓化产出的组/路径也计)。
    pub nodes: usize,
    /// 无法轮廓化而跳过的 `<text>` 节点数。
    ///
    /// V4.0 T3 起语义:仅当文本轮廓组缺失/为空(典型是无字体环境,
    /// usvg 解析期字形布局失败)才计数;正常轮廓化产出组+路径,
    /// 计入 [`ImportReport::nodes`]。
    pub skipped_text: usize,
    /// 跳过的 `<image>` 节点数。
    pub skipped_image: usize,
    /// 降级为中性灰纯色的 `<pattern>` 填充数。
    ///
    /// v3.0 T2 起语义收窄:Linear/Radial 渐变已完整映射,不再计入;
    /// 仅 Pattern 降级仍计数。
    pub simplified_gradients: usize,
    /// 以 pad 语义近似的 reflect/repeat `spreadMethod` 渐变数(G26,
    /// V4.0 T3 起计数不再静默;Paint 模型无 spread 概念,渐变结构
    /// 完整保留,仅首尾色标外区域按 pad 钳制)。
    pub simplified_spreads: usize,
}

// ---------------------------------------------------------------------------
// 导出
// ---------------------------------------------------------------------------

/// 导出统计(V4.0 T3):导出了什么一目了然,文本不再静默丢弃。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExportReport {
    /// 导出的组/路径节点数(与导入侧 [`ImportReport::nodes`] 同口径,组也计;
    /// 不可见节点不导出故不计)。
    pub nodes: usize,
    /// 导出的 `<text>` 文本节点数。
    pub texts: usize,
    /// 下一个渐变 defs id 的序号(内部计数器:单调派生 `g0/g1/…`,
    /// 不外露;G26 起替代可碰撞的"defs 长度+名字节和"派生)。
    grad_next: usize,
}

/// 场景 → SVG 字符串 + 导出报告(需要计数口径时用;纯字符串用
/// [`export_svg`])。
pub fn export_svg_with_report(scene: &Scene) -> (String, ExportReport) {
    let mut defs = String::new();
    let mut body = String::new();
    let mut report = ExportReport::default();
    for &root in &scene.roots {
        export_node(scene, root, &mut body, &mut defs, &mut report);
    }
    let defs = if defs.is_empty() {
        String::new()
    } else {
        format!("<defs>{defs}</defs>")
    };
    (
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{defs}{body}</svg>"),
        report,
    )
}

/// 场景 → SVG 字符串(`xmlns` 齐备,可直接写 `.svg` 文件)。
pub fn export_svg(scene: &Scene) -> String {
    export_svg_with_report(scene).0
}

fn export_node(
    scene: &Scene,
    id: crate::scene::NodeId,
    body: &mut String,
    defs: &mut String,
    report: &mut ExportReport,
) {
    let Some(node) = scene.node(id) else {
        return;
    };
    if !node.visible {
        return;
    }
    let t = transform_attr(&node.transform);
    let open = if t.is_empty() {
        String::new()
    } else {
        format!(" transform=\"{t}\"")
    };
    let blend = blend_attr(node.blend_mode);
    match &node.content {
        NodeContent::Group => {
            body.push_str(&format!(
                "<g data-name=\"{}\"{open}{blend}>",
                esc(&node.name)
            ));
            for &child in &node.children {
                export_node(scene, child, body, defs, report);
            }
            body.push_str("</g>");
            report.nodes += 1;
        }
        NodeContent::Path(p) => {
            let mut attrs = format!(" data-name=\"{}\"{}", esc(&node.name), open);
            let fill_attr_s: String = match &p.fill {
                Some(fill) => format!(" fill=\"{}\"", fill_attr(fill, defs, report)),
                None => " fill=\"none\"".to_string(),
            };
            attrs.push_str(&fill_attr_s);
            if let Some(stroke) = &p.stroke {
                attrs.push_str(&format!(
                    " stroke=\"{}\" stroke-width=\"{}\"",
                    fill_attr(&stroke.paint, defs, report),
                    f(stroke.width)
                ));
            }
            body.push_str(&format!(
                "<path d=\"{}\"{attrs}{blend}/>",
                path_data(&p.path)
            ));
            report.nodes += 1;
        }
        NodeContent::Text(t) => {
            // 文本导出为 <text>(V4.0 T3 收口 v2.0 遗留):字号/颜色直写,
            // 内容 XML 转义。定位:场景 Text 节点位置存在节点 transform 里
            // (文本局部锚点是原点)。SVG 的 x/y 在 transform 之前应用——
            // 若把平移拆到 x/y、线性部分留在 transform,旋转/斜切会把锚点
            // 再变换一遍而漂移;故仅纯平移(线性部分为单位阵)时把平移写成
            // x/y(锚点即节点位置字段),其余情况锚点保持原点、位置整体走
            // transform,两种写法渲染结果逐点一致。
            let c = node.transform.as_coeffs();
            let linear_identity = c[0] == 1.0 && c[1] == 0.0 && c[2] == 0.0 && c[3] == 1.0;
            let (xy, open_text) = if linear_identity {
                (
                    format!(" x=\"{}\" y=\"{}\"", f(c[4]), f(c[5])),
                    String::new(),
                )
            } else {
                (String::from(" x=\"0\" y=\"0\""), open)
            };
            body.push_str(&format!(
                "<text{xy} font-size=\"{}\" fill=\"{}\"{open_text}{blend}>{}</text>",
                f(t.font_size),
                rgba_attr(t.color),
                esc(&t.text)
            ));
            report.texts += 1;
        }
        // Image 不进 SVG v2.0(位图走资产管线;与导入侧跳过计数对称)
        NodeContent::Image(_) => {}
    }
}

/// kurbo PathEl → SVG `d` 属性。
fn path_data(path: &BezPath) -> String {
    let mut d = String::new();
    for el in path.elements() {
        if !d.is_empty() {
            d.push(' ');
        }
        match el {
            kurbo::PathEl::MoveTo(p) => d.push_str(&format!("M {} {}", f(p.x), f(p.y))),
            kurbo::PathEl::LineTo(p) => d.push_str(&format!("L {} {}", f(p.x), f(p.y))),
            kurbo::PathEl::QuadTo(c, p) => {
                d.push_str(&format!("Q {} {} {} {}", f(c.x), f(c.y), f(p.x), f(p.y)))
            }
            kurbo::PathEl::CurveTo(c1, c2, p) => d.push_str(&format!(
                "C {} {} {} {} {} {}",
                f(c1.x),
                f(c1.y),
                f(c2.x),
                f(c2.y),
                f(p.x),
                f(p.y)
            )),
            kurbo::PathEl::ClosePath => d.push('Z'),
        }
    }
    d
}

fn fill_attr(paint: &Paint, defs: &mut String, report: &mut ExportReport) -> String {
    match paint {
        Paint::Solid(c) => rgba_attr(*c),
        Paint::LinearGradient { start, end, stops } => {
            let id = next_grad_id(report);
            defs.push_str(&format!(
                "<linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">",
                f(start[0]),
                f(start[1]),
                f(end[0]),
                f(end[1])
            ));
            write_gradient_stops(defs, stops);
            defs.push_str("</linearGradient>");
            format!("url(#{id})")
        }
        // 径向完整映射(v3.0 T2;与 linear 同款 userSpaceOnUse 模式)
        Paint::RadialGradient {
            center,
            radius,
            stops,
        } => {
            let id = next_grad_id(report);
            defs.push_str(&format!(
                "<radialGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" cx=\"{}\" cy=\"{}\" r=\"{}\">",
                f(center[0]),
                f(center[1]),
                f(*radius)
            ));
            write_gradient_stops(defs, stops);
            defs.push_str("</radialGradient>");
            format!("url(#{id})")
        }
        // 锥形导出降级为首停纯色:SVG 1.1 无锥形渐变元素(CSS
        // conic-gradient 不是 SVG 1.1 的 paint server),完整表达需
        // SVG 2/专有序列化,降级说明保留。
        Paint::ConicGradient { stops, .. } => stops
            .first()
            .map(|s| rgba_attr(s.color))
            .unwrap_or_else(|| "none".into()),
    }
}

/// 渐变 defs id(G26,V4.0 T3 起):按导出顺序单调计数派生
/// `g0/g1/…`,永不碰撞——旧式"defs 已有长度 + 节点名字节和"在不同
/// 命名组合下可产生相同值。
fn next_grad_id(report: &mut ExportReport) -> String {
    let id = format!("g{}", report.grad_next);
    report.grad_next += 1;
    id
}

/// 渐变色标序列化:不透明 stop 只写 stop-color;半透明补 `stop-opacity`
/// (v3.0 T2 补齐——v2.0 把 alpha 内嵌进 rgba(),导入端拿不到独立
/// stop-opacity,且非 SVG 标准的 stop 颜色写法)。
fn write_gradient_stops(defs: &mut String, stops: &[GradientStop]) {
    for s in stops {
        let c = s.color;
        defs.push_str(&format!(
            "<stop offset=\"{}\" stop-color=\"rgb({},{},{})\"",
            f(f64::from(s.offset)),
            c[0],
            c[1],
            c[2]
        ));
        if c[3] != 255 {
            defs.push_str(&format!(" stop-opacity=\"{}\"", f(f64::from(c[3]) / 255.0)));
        }
        defs.push_str("/>");
    }
}

fn rgba_attr(c: Rgba8) -> String {
    if c[3] == 255 {
        format!("rgb({},{},{})", c[0], c[1], c[2])
    } else {
        format!(
            "rgba({},{},{},{})",
            c[0],
            c[1],
            c[2],
            f(f64::from(c[3]) / 255.0)
        )
    }
}

fn blend_attr(mode: BlendMode) -> String {
    let name = match mode {
        BlendMode::Normal => return String::new(),
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
        BlendMode::Darken => "darken",
        BlendMode::Lighten => "lighten",
        BlendMode::ColorDodge => "color-dodge",
        BlendMode::ColorBurn => "color-burn",
        BlendMode::HardLight => "hard-light",
        BlendMode::SoftLight => "soft-light",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
        BlendMode::Hue => "hue",
        BlendMode::Saturation => "saturation",
        BlendMode::Color => "color",
        BlendMode::Luminosity => "luminosity",
    };
    format!(" style=\"mix-blend-mode:{name}\"")
}

fn transform_attr(t: &Affine) -> String {
    let c = t.as_coeffs();
    if c == [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] {
        return String::new();
    }
    format!(
        "matrix({} {} {} {} {} {})",
        f(c[0]),
        f(c[1]),
        f(c[2]),
        f(c[3]),
        f(c[4]),
        f(c[5])
    )
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 3 位小数、去尾随零(9.500 → "9.5";-0.0004 → "0")。
fn f(v: f64) -> String {
    let v = if v.abs() < 5e-4 { 0.0 } else { v };
    format!("{v:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

/// SVG 字符串 → 场景(新 Scene;Text 轮廓化为路径组,仅轮廓缺失才计数;
/// Image 跳过计数;`<pattern>` 降级中性灰计数;Linear/Radial 渐变完整映射)。
pub fn import_svg(svg: &str) -> CoreResult<(Scene, ImportReport)> {
    import_svg_with_limit(svg, MAX_SVG_BYTES)
}

/// 带显式大小上限的导入(测试注入小上限;生产用 [`import_svg`])。
pub fn import_svg_with_limit(svg: &str, max_bytes: usize) -> CoreResult<(Scene, ImportReport)> {
    if svg.len() > max_bytes {
        return Err(crate::error::CoreError::SvgTooLarge {
            size: svg.len(),
            limit: max_bytes,
        });
    }
    let mut opt = usvg::Options::default();
    // 系统字体(V4.0 T3):默认 Options 的 fontdb 是**空库**——不加载
    // 字体时文本布局在解析期直接失败,Text 节点根本不进树(usvg
    // parser/text.rs 的 convert 提前 return,旧版"文本被静默丢"的真正
    // 机制),文本互通无从谈起。加载系统字体后(Windows 字体目录 /
    // Linux fontconfig),Text 节点携带解析期轮廓化好的 flattened 组
    // 进树,导入侧才有现成轮廓可消费。Arc 刚构造必唯一,get_mut 必
    // 成功;万一失败则维持空库,文本按 skipped_text 如实计数。
    if let Some(db) = std::sync::Arc::get_mut(&mut opt.fontdb) {
        db.load_system_fonts();
        resolve_generic_family_aliases(db);
    }
    let tree = usvg::Tree::from_str(svg, &opt)
        .map_err(|e| crate::error::CoreError::SvgParse(e.to_string()))?;
    let mut scene = Scene::new();
    let mut report = ImportReport::default();
    let root = tree.root();
    import_group(&mut scene, None, root, &mut report, 0)?;
    Ok((scene, report))
}

/// 通用族别名按本机字体集解析。usvg 对缺省 font-family 的 `<text>` 以
/// 通用族名("sans-serif" 等)查询,而 fontdb 的通用族别名出厂指向
/// Arial/Times New Roman 等 Windows 字族——Linux 服务器普遍没有,查空
/// 时 usvg 在解析期把 Text 节点整个丢弃(树里无痕,导入侧 skipped_text
/// 也不计;V4.0 CI ubuntu 实测踩坑)。按 preference 列表落到实际存在的
/// 字族,文本互通才能跨平台成立。都缺省时不覆盖出厂别名。
fn resolve_generic_family_aliases(db: &mut usvg::fontdb::Database) {
    const SANS: &[&str] = &[
        "Arial",
        "Helvetica",
        "DejaVu Sans",
        "Liberation Sans",
        "Noto Sans",
    ];
    const SERIF: &[&str] = &[
        "Times New Roman",
        "Georgia",
        "DejaVu Serif",
        "Liberation Serif",
        "Noto Serif",
    ];
    const MONO: &[&str] = &[
        "Consolas",
        "Courier New",
        "DejaVu Sans Mono",
        "Liberation Mono",
        "Noto Sans Mono",
    ];
    let sans = pick_family(db, SANS);
    let serif = pick_family(db, SERIF);
    let mono = pick_family(db, MONO);
    if let Some(name) = sans {
        db.set_sans_serif_family(name);
    }
    if let Some(name) = serif {
        db.set_serif_family(name);
    }
    if let Some(name) = mono {
        db.set_monospace_family(name);
    }
}

fn family_present(db: &usvg::fontdb::Database, name: &str) -> bool {
    db.faces()
        .any(|f| f.families.iter().any(|(family, _)| family == name))
}

fn pick_family<'a>(db: &usvg::fontdb::Database, candidates: &[&'a str]) -> Option<&'a str> {
    candidates
        .iter()
        .copied()
        .find(|name| family_present(db, name))
}

fn import_group(
    scene: &mut Scene,
    parent: Option<crate::scene::NodeId>,
    group: &usvg::Group,
    report: &mut ImportReport,
    depth: usize,
) -> CoreResult<()> {
    if depth > MAX_DEPTH {
        return Err(crate::error::CoreError::SvgParse(String::from(
            "嵌套深度超限(疑似恶意构造)",
        )));
    }
    let group_id = scene.add_node(parent, node_name(group.id()), NodeContent::Group)?;
    apply_transform(scene, group_id, group.transform());
    report.nodes += 1;
    for child in group.children() {
        match child {
            usvg::Node::Group(g) => {
                import_group(scene, Some(group_id), g, report, depth + 1)?;
            }
            usvg::Node::Path(p) => {
                let mut fill = None;
                if let Some(f) = p.fill() {
                    fill = Some(import_paint(f.paint(), f.opacity().get(), report));
                }
                let mut stroke = None;
                if let Some(st) = p.stroke() {
                    stroke = Some(StrokeStyle {
                        paint: import_paint(st.paint(), st.opacity().get(), report),
                        width: f64::from(st.width().get()),
                    });
                }
                scene.add_node(
                    Some(group_id),
                    node_name(p.id()),
                    NodeContent::Path(PathNode {
                        path: tiny_to_kurbo(p.data()),
                        fill,
                        stroke,
                    }),
                )?;
                // usvg 0.48 的 Path 无本地 transform(已折入祖先组):组链变换
                // 由我们场景的组节点承担,路径节点保持恒等即可,渲染结果一致
                let _ = p;
                report.nodes += 1;
            }
            usvg::Node::Text(t) => {
                // 文本轮廓化导入(V4.0 T3,docs/10 T3.1):usvg 0.48 在解析
                // 期已完成文本布局与字形轮廓化,但 Text 节点仍留在树里,
                // `Text::flattened()` 返回现成的轮廓组(路径/嵌套字形组/
                // 位图字形;轮廓坐标在文本父级用户空间,与"我们按组链应用
                // transform、路径保持局部"的模型一致——usvg 官方 writer
                // 也是把该组就地序列化,不补变换)。走既有 Path/Group 导入
                // 逻辑(fill/stroke/transform 同款),正常轮廓化计入节点数。
                let flattened = t.flattened();
                if flattened.children().is_empty() {
                    // 无字体环境等导致的空轮廓组:无可导入内容,如实计数
                    // 不 panic(skipped_text 的新语义)。
                    report.skipped_text += 1;
                } else {
                    import_group(scene, Some(group_id), flattened, report, depth + 1)?;
                }
            }
            usvg::Node::Image(_) => report.skipped_image += 1,
        }
    }
    Ok(())
}

fn node_name(id: &str) -> String {
    if id.is_empty() {
        String::from("svg-node")
    } else {
        format!("svg-{id}")
    }
}

/// spreadMethod 降级计数:Paint 模型无 spread 概念,reflect/repeat 按
/// pad 近似(渐变结构完整保留);G26 起计数,不再静默。
fn note_spread(m: usvg::SpreadMethod, report: &mut ImportReport) {
    if m != usvg::SpreadMethod::Pad {
        report.simplified_spreads += 1;
    }
}

fn import_paint(paint: &usvg::Paint, opacity: f32, report: &mut ImportReport) -> Paint {
    match paint {
        usvg::Paint::Color(c) => Paint::Solid(color_to_rgba8(*c, opacity)),
        // Linear/Radial 完整映射(v3.0 T2)。usvg 0.48 在解析收尾的
        // `update_paint_servers` 已把全部 paint server 折算为
        // userSpaceOnUse(objectBoundingBox 按目标形状 bbox 展开;
        // units 不进 pub API,"for the caller they are always
        // userSpaceOnUse"——usvg 源码 paint_server.rs),故坐标直读即
        // 用户空间。gradientTransform 折入几何;spreadMethod 不进
        // Paint 模型,reflect/repeat 按 pad 语义近似并计数(G26,
        // 不再静默),pad 直读首尾色标。
        usvg::Paint::LinearGradient(g) => {
            note_spread(g.spread_method(), report);
            let t = tiny_transform_to_affine(g.transform());
            Paint::LinearGradient {
                start: affine_point(t, f64::from(g.x1()), f64::from(g.y1())),
                end: affine_point(t, f64::from(g.x2()), f64::from(g.y2())),
                stops: import_stops(g.stops(), opacity),
            }
        }
        usvg::Paint::RadialGradient(g) => {
            note_spread(g.spread_method(), report);
            let t = tiny_transform_to_affine(g.transform());
            // 焦点 fx/fy/fr 不进模型(标准径向以圆心为准);非均匀
            // gradientTransform 下半径按面积等效缩放(sqrt|det|)近似。
            let c = t.as_coeffs();
            let det = (c[0] * c[3] - c[1] * c[2]).abs();
            Paint::RadialGradient {
                center: affine_point(t, f64::from(g.cx()), f64::from(g.cy())),
                radius: f64::from(g.r().get()) * det.sqrt(),
                stops: import_stops(g.stops(), opacity),
            }
        }
        // Pattern 仍降级:平铺内容需要 Image/嵌套子树表达(v2.0 起
        // 不进 SVG 互通模型),降级为中性灰并计数;`simplified_gradients`
        // 自 v3.0 起仅指 Pattern 降级。
        usvg::Paint::Pattern(_) => {
            report.simplified_gradients += 1;
            Paint::Solid([128, 128, 128, 255])
        }
    }
}

/// tiny-skia Transform(usvg 节点/渐变变换)→ kurbo Affine
/// (系数序与场景侧 `Affine::new` 一致)。
fn tiny_transform_to_affine(t: tiny_skia_path::Transform) -> Affine {
    Affine::new([
        f64::from(t.sx),
        f64::from(t.kx),
        f64::from(t.ky),
        f64::from(t.sy),
        f64::from(t.tx),
        f64::from(t.ty),
    ])
}

fn affine_point(t: Affine, x: f64, y: f64) -> [f64; 2] {
    let p = t * kurbo::Point::new(x, y);
    [p.x, p.y]
}

/// usvg 色标 → 场景色标:Color(RGB)+ `stop-opacity`,再乘填充/描边级
/// opacity(SVG 规范:paint opacity 作用于整个 paint,折入各 stop 的
/// alpha,使透明度往返无损)。全部在非预乘直通域,无预乘往返损失。
fn import_stops(stops: &[usvg::Stop], paint_opacity: f32) -> Vec<GradientStop> {
    stops
        .iter()
        .map(|s| {
            let c = s.color();
            let a = (s.opacity().get() * paint_opacity).clamp(0.0, 1.0);
            GradientStop {
                offset: s.offset().get(),
                color: [c.red, c.green, c.blue, (a * 255.0).round() as u8],
            }
        })
        .collect()
}

/// usvg Color(RGB)+ 独立透明度 → 直通域 Rgba8。
fn color_to_rgba8(c: usvg::Color, opacity: f32) -> Rgba8 {
    let a = opacity.clamp(0.0, 1.0);
    [
        f32::from(c.red).round() as u8,
        f32::from(c.green).round() as u8,
        f32::from(c.blue).round() as u8,
        (a * 255.0).round() as u8,
    ]
}

fn tiny_to_kurbo(data: &tiny_skia_path::Path) -> BezPath {
    let mut path = BezPath::new();
    let mut last_start = tiny_skia_path::Point::from_xy(0.0, 0.0);
    let mut last = last_start;
    for seg in data.segments() {
        match seg {
            tiny_skia_path::PathSegment::MoveTo(p) => {
                path.move_to((f64::from(p.x), f64::from(p.y)));
                last_start = p;
                last = p;
            }
            tiny_skia_path::PathSegment::LineTo(p) => {
                path.line_to((f64::from(p.x), f64::from(p.y)));
                last = p;
            }
            tiny_skia_path::PathSegment::QuadTo(c, p) => {
                // 二次 → 三次升阶(标准 2/3 权重)
                let c1 = tiny_skia_path::Point::from_xy(
                    last.x + (c.x - last.x) * 2.0 / 3.0,
                    last.y + (c.y - last.y) * 2.0 / 3.0,
                );
                let c2 = tiny_skia_path::Point::from_xy(
                    p.x + (c.x - p.x) * 2.0 / 3.0,
                    p.y + (c.y - p.y) * 2.0 / 3.0,
                );
                path.curve_to(
                    (f64::from(c1.x), f64::from(c1.y)),
                    (f64::from(c2.x), f64::from(c2.y)),
                    (f64::from(p.x), f64::from(p.y)),
                );
                last = p;
            }
            tiny_skia_path::PathSegment::CubicTo(c1, c2, p) => {
                path.curve_to(
                    (f64::from(c1.x), f64::from(c1.y)),
                    (f64::from(c2.x), f64::from(c2.y)),
                    (f64::from(p.x), f64::from(p.y)),
                );
                last = p;
            }
            tiny_skia_path::PathSegment::Close => {
                path.close_path();
                last = last_start;
            }
        }
    }
    path
}

fn apply_transform(scene: &mut Scene, id: crate::scene::NodeId, t: tiny_skia_path::Transform) {
    if t != tiny_skia_path::Transform::identity() {
        let affine = tiny_transform_to_affine(t);
        if let Some(node) = scene.node_mut(id) {
            node.transform = affine;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::scene::TextNode;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
        let mut p = BezPath::new();
        p.move_to((x0, y0));
        p.line_to((x1, y0));
        p.line_to((x1, y1));
        p.line_to((x0, y1));
        p.close_path();
        p
    }

    fn demo_scene() -> Scene {
        let mut scene = Scene::new();
        let group = scene.add_node(None, "组", NodeContent::Group).expect("组");
        scene
            .add_node(
                Some(group),
                "矩形",
                NodeContent::Path(PathNode {
                    path: rect(0.0, 0.0, 10.0, 10.0),
                    fill: Some(Paint::Solid([200, 40, 40, 255])),
                    stroke: Some(StrokeStyle {
                        paint: Paint::Solid([0, 0, 0, 255]),
                        width: 1.5,
                    }),
                }),
            )
            .expect("矩形");
        scene
            .add_node(
                Some(group),
                "渐变路径",
                NodeContent::Path(PathNode {
                    path: rect(20.0, 0.0, 30.0, 10.0),
                    fill: Some(Paint::LinearGradient {
                        start: [0.0, 0.0],
                        end: [1.0, 0.0],
                        stops: vec![
                            GradientStop {
                                offset: 0.0,
                                color: [255, 0, 0, 255],
                            },
                            GradientStop {
                                offset: 1.0,
                                color: [0, 0, 255, 255],
                            },
                        ],
                    }),
                    stroke: None,
                }),
            )
            .expect("渐变");
        scene
    }

    #[test]
    fn export_contains_geometry_fill_stroke_gradient() {
        let svg = export_svg(&demo_scene());
        assert!(svg.starts_with("<svg xmlns="));
        assert!(svg.contains("data-name=\"矩形\""));
        assert!(svg.contains("d=\"M 0 0 L 10 0 L 10 10 L 0 10 Z\""));
        assert!(svg.contains("fill=\"rgb(200,40,40)\""));
        assert!(svg.contains("stroke-width=\"1.5\""));
        assert!(svg.contains("<linearGradient"));
        assert!(svg.contains("stop-color=\"rgb(255,0,0)\""));
        assert!(svg.contains("<g data-name=\"组\">"), "组序列化为 <g>");
    }

    #[test]
    fn export_blend_mode_uses_css_name() {
        assert_eq!(
            blend_attr(BlendMode::Multiply),
            " style=\"mix-blend-mode:multiply\""
        );
        assert_eq!(blend_attr(BlendMode::Normal), "");
    }

    #[test]
    fn import_roundtrip_shapes_and_colors() {
        let exported = export_svg(&demo_scene());
        let (scene, report) = import_svg(&exported).expect("导入");
        assert!(report.nodes >= 3, "组 + 2 路径,实际 {}", report.nodes);
        assert_eq!(report.skipped_text, 0);
        // 注:usvg 不保留 data-name,导入名统一 svg-node/自生成 id → 按颜色找
        let reds: Vec<Paint> = scene
            .nodes
            .iter()
            .filter_map(|(_, n)| match &n.content {
                NodeContent::Path(p) => p.fill.clone(),
                _ => None,
            })
            .collect();
        // v3.0 起渐变路径完整映射为 LinearGradient(不再降级首停红);
        // 精确断言矩形纯色无损保留
        assert!(
            reds.contains(&Paint::Solid([200, 40, 40, 255])),
            "矩形纯色应无损保留,实际 {reds:?}"
        );
    }

    #[test]
    fn import_rejects_oversize_and_garbage() {
        let big = format!("<svg>{}</svg>", "a".repeat(64));
        assert!(import_svg_with_limit(&big, 8).is_err(), "超限拒绝");
        let garbage = "this is not svg at all {{{";
        assert!(
            import_svg_with_limit(garbage, 1024).is_err(),
            "垃圾输入拒绝"
        );
    }

    #[test]
    fn import_counts_skipped_text_and_images() {
        // V4.0 T3 语义修正(旧注释声称"usvg 0.48 解析期把 Text 节点转成
        // 组+路径"——与 0.48.1 源码相反:解析后 Text 节点仍留在树里,轮廓
        // 在 `Text::flattened()` 组,导入侧现消费它)。文本轮廓组非空时产出
        // 路径节点(skipped_text=0);空轮廓组(无字体环境)才如实计数,
        // 两种情况都不 panic。位图字形之外的 <image> 仍跳过计数。
        let svg = concat!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\">",
            "<text x=\"1\" y=\"2\">hi</text>",
            "<rect width=\"4\" height=\"4\"/>",
            "<image xlink:href=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==\" width=\"2\" height=\"2\"/>",
            "</svg>"
        );
        let (scene, report) = import_svg(svg).expect("含文本/图像 SVG 可导入");
        assert!(report.nodes >= 1, "rect 至少导入,实际 {report:?}");
        assert_eq!(report.skipped_image, 1, "内嵌位图跳过计数");
        // 文本导入的三种环境态(CI ubuntu 与本机 Windows 字体集不同,三态都合法):
        if path_count(&scene) >= 1 {
            // (A) 字体匹配成功:轮廓组非空,产出路径节点
        } else if report.skipped_text == 1 {
            // (B) 树内 Text 节点轮廓组为空:导入侧如实计数
        } else {
            // (C) usvg 解析期字体匹配失败,Text 节点根本没进树:导入侧无从
            //     感知(skipped_text=0)。无内嵌字体资产前提下跨平台不可避免,
            //     如实记录而非误判(docs/10 T3 已知边界)
            assert_eq!(
                report.skipped_text, 0,
                "解析期被丢的文本不可能进计数,报告 {report:?}"
            );
            eprintln!("note: 文本被 usvg 解析期丢弃(无匹配字体),导入侧无从感知");
        }
    }

    /// 场景内路径节点数(文本轮廓化断言用)。
    fn path_count(scene: &Scene) -> usize {
        scene
            .nodes
            .iter()
            .filter(|(_, n)| matches!(n.content, NodeContent::Path(_)))
            .count()
    }

    // —— V4.0 T3:SVG 文本互通(G22,docs/10 T3)——

    /// 导出:Text 节点 → `<text x y font-size fill>`(XML 转义),
    /// 报告计 texts。
    #[test]
    fn export_text_node_to_svg_text_element() {
        let mut scene = Scene::new();
        scene
            .add_node(
                None,
                "标题",
                NodeContent::Text(TextNode {
                    text: "a<b & c\"d".to_string(),
                    font_size: 18.0,
                    color: [10, 20, 30, 255],
                }),
            )
            .expect("文本节点");
        let (svg, report) = export_svg_with_report(&scene);
        assert!(
            svg.contains(
                "<text x=\"0\" y=\"0\" font-size=\"18\" fill=\"rgb(10,20,30)\">a&lt;b &amp; c&quot;d</text>"
            ),
            "文本元素 + XML 转义,实际 {svg}"
        );
        assert_eq!(report.texts, 1, "文本计数");
        assert_eq!(report.nodes, 0, "文本不计入几何节点");
    }

    /// 导出定位:纯平移 transform 的平移即节点位置字段,直接写 x/y
    /// 且不再产生 transform 属性(锚点取位置字段)。
    #[test]
    fn export_text_position_uses_transform_translation() {
        let mut scene = Scene::new();
        let id = scene
            .add_node(
                None,
                "文本",
                NodeContent::Text(TextNode {
                    text: "hi".to_string(),
                    font_size: 12.0,
                    color: [0, 0, 0, 255],
                }),
            )
            .expect("文本节点");
        scene.node_mut(id).expect("节点").transform =
            Affine::new([1.0, 0.0, 0.0, 1.0, 100.0, 50.0]);
        let (svg, _) = export_svg_with_report(&scene);
        assert!(
            svg.contains("<text x=\"100\" y=\"50\" font-size=\"12\""),
            "平移写进 x/y,实际 {svg}"
        );
        assert!(
            !svg.contains("transform"),
            "纯平移不应残留 transform 属性,实际 {svg}"
        );
    }

    /// roundtrip(V4.0 T3 验收):含 Text 场景 → 导出含 `<text>` → 导入
    /// 得到轮廓路径节点(不再是双向静默丢)。导入侧按设计必然是路径/
    /// 组而非 Text 节点——与场景"文本渲染走字形轮廓"一致;文本色折入
    /// 路径填充保留。无字体环境分支如实计数不 panic。
    #[test]
    fn text_roundtrip_export_text_import_outline_paths() {
        let mut scene = Scene::new();
        scene
            .add_node(
                None,
                "标题",
                NodeContent::Text(TextNode {
                    text: "Sable".to_string(),
                    font_size: 24.0,
                    color: [200, 40, 40, 255],
                }),
            )
            .expect("文本节点");
        let (exported, export_report) = export_svg_with_report(&scene);
        assert!(
            exported.contains("<text"),
            "导出应含 <text> 元素,实际 {exported}"
        );
        assert_eq!(export_report.texts, 1, "导出报告文本计数");
        let (imported, import_report) = import_svg(&exported).expect("导入");
        // 同 import_counts_skipped_text_and_images:三环境态都合法
        if path_count(&imported) >= 1 {
            // (A) 字体匹配成功:轮廓组非空,产出组+路径,填充即文本色
            assert!(
                import_report.nodes >= 1,
                "轮廓组/路径计入节点数,报告 {import_report:?}"
            );
            assert!(
                fills_of(&imported).contains(&Paint::Solid([200, 40, 40, 255])),
                "文本色折入路径填充,实际 {:?}",
                fills_of(&imported)
            );
        } else if import_report.skipped_text == 1 {
            // (B) 树内空轮廓组:如实计数
            assert_eq!(import_report.nodes, 0, "无路径导入,报告 {import_report:?}");
        } else {
            // (C) usvg 解析期丢节点:导出已断言含 <text>,导入侧场景为空
            assert_eq!(
                import_report.skipped_text, 0,
                "解析期被丢的文本不可能进计数,报告 {import_report:?}"
            );
            assert_eq!(
                import_report.nodes, 0,
                "文本是唯一元素,被丢后场景为空,报告 {import_report:?}"
            );
            eprintln!("note: 文本被 usvg 解析期丢弃(无匹配字体),导入侧无从感知");
        }
    }

    /// G26:reflect/repeat spreadMethod 不再静默按 pad——各计一次
    /// simplified_spreads,渐变本身仍完整映射;pad 不计数。
    #[test]
    fn import_counts_non_pad_spread_methods() {
        let svg = concat!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs>",
            "<linearGradient id=\"a\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"0\" x2=\"10\" y2=\"0\" spreadMethod=\"reflect\">",
            "<stop offset=\"0\" stop-color=\"red\"/><stop offset=\"1\" stop-color=\"blue\"/></linearGradient>",
            "<radialGradient id=\"b\" gradientUnits=\"userSpaceOnUse\" cx=\"5\" cy=\"5\" r=\"5\" spreadMethod=\"repeat\">",
            "<stop offset=\"0\" stop-color=\"red\"/><stop offset=\"1\" stop-color=\"blue\"/></radialGradient>",
            "<linearGradient id=\"c\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"0\" x2=\"10\" y2=\"0\">",
            "<stop offset=\"0\" stop-color=\"red\"/><stop offset=\"1\" stop-color=\"blue\"/></linearGradient>",
            "</defs>",
            "<rect width=\"4\" height=\"4\" fill=\"url(#a)\"/>",
            "<rect x=\"10\" width=\"4\" height=\"4\" fill=\"url(#b)\"/>",
            "<rect x=\"20\" width=\"4\" height=\"4\" fill=\"url(#c)\"/></svg>"
        );
        let (scene, report) = import_svg(svg).expect("导入");
        assert_eq!(report.simplified_spreads, 2, "reflect + repeat 各计一次");
        let grads = fills_of(&scene);
        assert_eq!(
            grads
                .iter()
                .filter(|p| matches!(p, Paint::LinearGradient { .. }))
                .count(),
            2,
            "两条 linear 完整保留"
        );
        assert_eq!(
            grads
                .iter()
                .filter(|p| matches!(p, Paint::RadialGradient { .. }))
                .count(),
            1,
            "radial 完整保留"
        );
    }

    /// G26:渐变 defs id 改计数器单调派生(g0/g1/…),同名词节点不再
    /// 可能撞 id(旧式 defs 长度+名字节和可碰撞)。
    #[test]
    fn export_gradient_ids_counter_derived_unique() {
        let mut scene = Scene::new();
        let root = scene
            .add_node(None, "同名", NodeContent::Group)
            .expect("组");
        for _ in 0..2 {
            scene
                .add_node(
                    Some(root),
                    "同名",
                    NodeContent::Path(PathNode {
                        path: rect(0.0, 0.0, 4.0, 4.0),
                        fill: Some(Paint::LinearGradient {
                            start: [0.0, 0.0],
                            end: [4.0, 0.0],
                            stops: vec![
                                GradientStop {
                                    offset: 0.0,
                                    color: [255, 0, 0, 255],
                                },
                                GradientStop {
                                    offset: 1.0,
                                    color: [0, 0, 255, 255],
                                },
                            ],
                        }),
                        stroke: None,
                    }),
                )
                .expect("路径节点");
        }
        let (svg, _) = export_svg_with_report(&scene);
        assert!(
            svg.contains("id=\"g0\"") && svg.contains("url(#g0)"),
            "第一个渐变 g0,实际 {svg}"
        );
        assert!(
            svg.contains("id=\"g1\"") && svg.contains("url(#g1)"),
            "第二个渐变 g1,实际 {svg}"
        );
        assert!(!svg.contains("url(#g2)"), "计数恰好用尽,实际 {svg}");
    }

    // —— V3.0 T2:SVG 渐变完整映射(docs/09 T2;docs/08 G14)——

    /// 收集场景内全部路径填充(渐变断言用;与既有测试同款遍历)。
    fn fills_of(scene: &Scene) -> Vec<Paint> {
        scene
            .nodes
            .iter()
            .filter_map(|(_, n)| match &n.content {
                NodeContent::Path(p) => p.fill.clone(),
                _ => None,
            })
            .collect()
    }

    /// Linear + Radial 各一条的场景(几何值都取 3 位小数精确可表示点,
    /// 保证导出→导入无舍入差)。
    fn gradient_scene() -> Scene {
        let mut scene = Scene::new();
        let root = scene.add_node(None, "根", NodeContent::Group).expect("根");
        scene
            .add_node(
                Some(root),
                "线性",
                NodeContent::Path(PathNode {
                    path: rect(0.0, 0.0, 10.0, 10.0),
                    fill: Some(Paint::LinearGradient {
                        start: [0.0, 0.0],
                        end: [10.0, 0.0],
                        stops: vec![
                            GradientStop {
                                offset: 0.0,
                                color: [255, 0, 0, 255],
                            },
                            GradientStop {
                                offset: 0.5,
                                color: [0, 255, 0, 255],
                            },
                            GradientStop {
                                offset: 1.0,
                                color: [0, 0, 255, 255],
                            },
                        ],
                    }),
                    stroke: None,
                }),
            )
            .expect("线性");
        scene
            .add_node(
                Some(root),
                "径向",
                NodeContent::Path(PathNode {
                    path: rect(10.0, 0.0, 20.0, 10.0),
                    fill: Some(Paint::RadialGradient {
                        center: [15.0, 5.0],
                        radius: 6.0,
                        stops: vec![
                            GradientStop {
                                offset: 0.25,
                                color: [255, 255, 0, 255],
                            },
                            GradientStop {
                                offset: 0.75,
                                color: [255, 0, 255, 128],
                            },
                        ],
                    }),
                    stroke: None,
                }),
            )
            .expect("径向");
        scene
    }

    /// 渐变 roundtrip:场景(Linear+Radial)→ 导出 → 导入,两侧 Paint
    /// 结构化相等(几何/stops/颜色/透明度逐项);Radial 导出含
    /// `<radialGradient>` cx/cy/r。
    #[test]
    fn gradient_roundtrip_linear_radial_structural_equality() {
        let exported = export_svg(&gradient_scene());
        assert!(exported.contains("<radialGradient"), "径向导出补齐");
        assert!(
            exported.contains("cx=\"15\" cy=\"5\" r=\"6\""),
            "radialGradient 几何属性,实际 {exported}"
        );
        let (imported, report) = import_svg(&exported).expect("导入");
        assert_eq!(
            report.simplified_gradients, 0,
            "Linear/Radial 完整映射,不再计数"
        );
        let linear = Paint::LinearGradient {
            start: [0.0, 0.0],
            end: [10.0, 0.0],
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: [255, 0, 0, 255],
                },
                GradientStop {
                    offset: 0.5,
                    color: [0, 255, 0, 255],
                },
                GradientStop {
                    offset: 1.0,
                    color: [0, 0, 255, 255],
                },
            ],
        };
        let radial = Paint::RadialGradient {
            center: [15.0, 5.0],
            radius: 6.0,
            stops: vec![
                GradientStop {
                    offset: 0.25,
                    color: [255, 255, 0, 255],
                },
                GradientStop {
                    offset: 0.75,
                    color: [255, 0, 255, 128],
                },
            ],
        };
        let fills = fills_of(&imported);
        assert!(fills.contains(&linear), "线性渐变逐项相等,实际 {fills:?}");
        assert!(fills.contains(&radial), "径向渐变逐项相等,实际 {fills:?}");
    }

    /// stop-opacity:半透明 stop(alpha=128)导出为
    /// `stop-color="rgb(..)"` + `stop-opacity="0.502"`,导入回 alpha 128
    /// (255 的一半 ±1;本例 0.502*255 = 128.01 → 精确 128,无损)。
    #[test]
    fn stop_opacity_exported_and_alpha_roundtrips() {
        let mut scene = Scene::new();
        scene
            .add_node(
                None,
                "半透明渐变",
                NodeContent::Path(PathNode {
                    path: rect(0.0, 0.0, 8.0, 8.0),
                    fill: Some(Paint::LinearGradient {
                        start: [0.0, 0.0],
                        end: [8.0, 0.0],
                        // 双 stop:usvg 解析收尾会把单 stop 渐变优化成纯色填充
                        stops: vec![
                            GradientStop {
                                offset: 0.0,
                                color: [10, 20, 30, 128],
                            },
                            GradientStop {
                                offset: 1.0,
                                color: [10, 20, 30, 255],
                            },
                        ],
                    }),
                    stroke: None,
                }),
            )
            .expect("节点");
        let exported = export_svg(&scene);
        assert!(
            exported.contains("stop-color=\"rgb(10,20,30)\""),
            "stop-color 不再内嵌 alpha,实际 {exported}"
        );
        assert!(
            exported.contains("stop-opacity=\"0.502\""),
            "半透明 stop 写独立 stop-opacity,实际 {exported}"
        );
        let (imported, _) = import_svg(&exported).expect("导入");
        let expected = Paint::LinearGradient {
            start: [0.0, 0.0],
            end: [8.0, 0.0],
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: [10, 20, 30, 128],
                },
                GradientStop {
                    offset: 1.0,
                    color: [10, 20, 30, 255],
                },
            ],
        };
        let fills = fills_of(&imported);
        assert!(
            fills.contains(&expected),
            "alpha 128 往返无损,实际 {fills:?}"
        );
    }

    /// simplified_gradients 语义收窄:Pattern 输入才计数,Linear/Radial
    /// 渐变不再计数(Pattern 降级中性灰,渐变保留完整结构)。
    #[test]
    fn simplified_gradients_counts_only_pattern_fallbacks() {
        let svg = concat!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs>",
            "<pattern id=\"p\" width=\"4\" height=\"4\" patternUnits=\"userSpaceOnUse\">",
            "<rect width=\"4\" height=\"4\" fill=\"red\"/></pattern>",
            "<linearGradient id=\"lg\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"0\" x2=\"4\" y2=\"0\">",
            "<stop offset=\"0\" stop-color=\"red\"/><stop offset=\"1\" stop-color=\"blue\"/></linearGradient>",
            "<radialGradient id=\"rg\" gradientUnits=\"userSpaceOnUse\" cx=\"2\" cy=\"2\" r=\"2\">",
            "<stop offset=\"0\" stop-color=\"green\"/><stop offset=\"1\" stop-color=\"yellow\"/></radialGradient>",
            "</defs>",
            "<rect width=\"4\" height=\"4\" fill=\"url(#p)\"/>",
            "<rect x=\"10\" width=\"4\" height=\"4\" fill=\"url(#lg)\"/>",
            "<rect x=\"20\" width=\"4\" height=\"4\" fill=\"url(#rg)\"/></svg>"
        );
        let (scene, report) = import_svg(svg).expect("导入");
        assert_eq!(report.simplified_gradients, 1, "仅 Pattern 计数");
        let fills = fills_of(&scene);
        assert!(
            fills
                .iter()
                .any(|f| matches!(f, Paint::Solid([128, 128, 128, 255]))),
            "Pattern 降级中性灰,实际 {fills:?}"
        );
        assert!(
            fills
                .iter()
                .any(|f| matches!(f, Paint::LinearGradient { .. })),
            "Linear 完整映射,实际 {fills:?}"
        );
        assert!(
            fills
                .iter()
                .any(|f| matches!(f, Paint::RadialGradient { .. })),
            "Radial 完整映射,实际 {fills:?}"
        );
    }

    /// Conic 导出降级说明保留:SVG 1.1 无锥形 paint server,不产生任何
    /// 渐变元素,降级为首停纯色。
    #[test]
    fn conic_gradient_export_falls_back_to_first_stop() {
        let mut scene = Scene::new();
        scene
            .add_node(
                None,
                "锥形",
                NodeContent::Path(PathNode {
                    path: rect(0.0, 0.0, 8.0, 8.0),
                    fill: Some(Paint::ConicGradient {
                        center: [4.0, 4.0],
                        start_angle: 0.0,
                        end_angle: std::f64::consts::TAU,
                        stops: vec![
                            GradientStop {
                                offset: 0.0,
                                color: [255, 0, 0, 255],
                            },
                            GradientStop {
                                offset: 1.0,
                                color: [0, 0, 255, 255],
                            },
                        ],
                    }),
                    stroke: None,
                }),
            )
            .expect("节点");
        let exported = export_svg(&scene);
        assert!(
            !exported.contains("Gradient"),
            "SVG 1.1 无锥形,不产生渐变元素,实际 {exported}"
        );
        assert!(
            exported.contains("fill=\"rgb(255,0,0)\""),
            "降级为首停纯色,实际 {exported}"
        );
    }

    // —— V2.0 T3:解析器模糊友好化(docs/08 §3 T3;分册六 §1.3)——
    //
    // proptest 冒烟:随机/乱构输入只允许 Ok/Err,绝不 panic(防线次序:
    // 大小上限 → usvg 解析 Result → 导入深度上限 → .sable 魔数/版本/反序列化)。
    // .sable 侧的 load_project 以路径为注入点,经临时文件投喂(本文件因
    // 写冲突封锁只在此追加,project.rs 未动);CI 以 `prop_sv` 前缀过滤运行。

    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use proptest::prelude::*;

    use crate::project::{SABLE_MAGIC, SABLE_VERSION, load_project};

    /// 每个用例独占的临时文件路径(进程 id + 自增计数,零随机零 sleep,
    /// 与 project.rs tests 的 temp_dir 同款纪律)。
    fn fuzz_temp_path(tag: &str) -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "sable-foundation-svg-fuzz-{serial}-{tag}-{}",
            std::process::id()
        ))
    }

    /// 随机字节串(0..4096 任意 u8)→ String:非法 UTF-8 走替换字符,
    /// 与"用户把二进制文件拖进导入框"同型。
    fn any_svg_text() -> impl Strategy<Value = String> {
        proptest::collection::vec(any::<u8>(), 0..4096)
            .prop_map(|v| String::from_utf8_lossy(&v).into_owned())
    }

    /// `.sable` 模糊输入:纯随机字节(大多死在头部校验),或
    /// "合法 SABL 头 + 随机体"(深入 MessagePack 反序列化面)。
    fn any_project_file() -> impl Strategy<Value = Vec<u8>> {
        prop_oneof![
            proptest::collection::vec(any::<u8>(), 0..512),
            proptest::collection::vec(any::<u8>(), 0..512).prop_map(|body| {
                let mut bytes = Vec::with_capacity(SABLE_MAGIC.len() + 4 + body.len());
                bytes.extend_from_slice(SABLE_MAGIC);
                bytes.extend_from_slice(&SABLE_VERSION.to_le_bytes());
                bytes.extend_from_slice(&body);
                bytes
            }),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// 随机字节串喂导入:永不 panic,只 Ok/Err(命中大小上限或 usvg 解析错误皆合法)。
        #[test]
        fn prop_sv_random_bytes_never_panic(input in any_svg_text()) {
            let _ = import_svg_with_limit(&input, 4096);
        }

        /// 合法导出串随机截断(0..len,回退到 UTF-8 边界):永不 panic。
        #[test]
        fn prop_sv_truncated_export_never_panic(cut in 0usize..1024) {
            let exported = export_svg(&demo_scene());
            let mut end = cut.min(exported.len());
            while end > 0 && !exported.is_char_boundary(end) {
                end -= 1;
            }
            let _ = import_svg_with_limit(&exported[..end], 4096);
        }

        /// 随机字节喂 `.sable` 读路径(魔数/版本/反序列化三层防御):
        /// 永不 panic,只 Ok/Err;panic 即 proptest 判负。
        #[test]
        fn prop_sv_project_file_never_panic(bytes in any_project_file()) {
            let path = fuzz_temp_path("project");
            std::fs::write(&path, &bytes).expect("写模糊输入临时文件");
            // Ok/Err 皆合法;断言本体 = "不 panic"
            let _ = load_project(&path);
            let _ = std::fs::remove_file(&path);
        }
    }
}
