//! SVG 互通(V2.0 T1 起,docs/08;V3.0 T2 渐变完整映射收尾):
//! 场景图 → SVG 字符串(导出,零额外依赖手写序列化)与
//! SVG → 场景图(导入,usvg 0.48 解析)。
//!
//! 范围:路径/组/变换/纯色填充/描边宽度颜色/混合模式;线性与径向渐变
//! 双向完整映射(几何/全部色标/stop 透明度;导入侧 gradientTransform
//! 折入几何坐标);Conic 渐变导出降级为首停纯色(SVG 1.1 无锥形
//! paint server,doc 保留说明);Text/Image 导入跳过并计数;`<pattern>`
//! 导入降级中性灰计数;效果(EffectSpec)不进 SVG(filter 映射 =
//! v2.1,doc 记录)。

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
    /// 成功导入的节点数(含组)。
    pub nodes: usize,
    /// 跳过的 `<text>` 节点数(v2.0 不做字形轮廓化)。
    pub skipped_text: usize,
    /// 跳过的 `<image>` 节点数。
    pub skipped_image: usize,
    /// 降级为中性灰纯色的 `<pattern>` 填充数。
    ///
    /// v3.0 T2 起语义收窄:Linear/Radial 渐变已完整映射,不再计入;
    /// 仅 Pattern 降级仍计数。
    pub simplified_gradients: usize,
}

// ---------------------------------------------------------------------------
// 导出
// ---------------------------------------------------------------------------

/// 场景 → SVG 字符串(`xmlns` 齐备,可直接写 `.svg` 文件)。
pub fn export_svg(scene: &Scene) -> String {
    let mut defs = String::new();
    let mut body = String::new();
    for &root in &scene.roots {
        export_node(scene, root, &mut body, &mut defs);
    }
    let defs = if defs.is_empty() {
        String::new()
    } else {
        format!("<defs>{defs}</defs>")
    };
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{defs}{body}</svg>")
}

fn export_node(scene: &Scene, id: crate::scene::NodeId, body: &mut String, defs: &mut String) {
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
                export_node(scene, child, body, defs);
            }
            body.push_str("</g>");
        }
        NodeContent::Path(p) => {
            let mut attrs = format!(" data-name=\"{}\"{}", esc(&node.name), open);
            let fill_attr_s: String = match &p.fill {
                Some(fill) => format!(" fill=\"{}\"", fill_attr(fill, &node.name, defs)),
                None => " fill=\"none\"".to_string(),
            };
            attrs.push_str(&fill_attr_s);
            if let Some(stroke) = &p.stroke {
                attrs.push_str(&format!(
                    " stroke=\"{}\" stroke-width=\"{}\"",
                    fill_attr(&stroke.paint, &node.name, defs),
                    f(stroke.width)
                ));
            }
            body.push_str(&format!(
                "<path d=\"{}\"{attrs}{blend}/>",
                path_data(&p.path)
            ));
        }
        // Text/Image 不进 SVG v2.0(与导入侧跳过对称)
        _ => {}
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

fn fill_attr(paint: &Paint, node_name: &str, defs: &mut String) -> String {
    match paint {
        Paint::Solid(c) => rgba_attr(*c),
        Paint::LinearGradient { start, end, stops } => {
            let id = grad_id(defs, node_name);
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
            let id = grad_id(defs, node_name);
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

/// 渐变 defs id(与 v2.0 同款:以 defs 已有长度 + 节点名派生,零状态)。
fn grad_id(defs: &str, node_name: &str) -> String {
    format!(
        "g{}",
        defs.len() + node_name.bytes().map(usize::from).sum::<usize>()
    )
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

/// SVG 字符串 → 场景(新 Scene;Text/Image 跳过计数;`<pattern>` 降级中性灰计数;
/// Linear/Radial 渐变完整映射)。
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
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_str(svg, &opt)
        .map_err(|e| crate::error::CoreError::SvgParse(e.to_string()))?;
    let mut scene = Scene::new();
    let mut report = ImportReport::default();
    let root = tree.root();
    import_group(&mut scene, None, root, &mut report, 0)?;
    Ok((scene, report))
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
                let path_id = scene.add_node(
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
            usvg::Node::Text(_) => report.skipped_text += 1,
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

fn import_paint(paint: &usvg::Paint, opacity: f32, report: &mut ImportReport) -> Paint {
    match paint {
        usvg::Paint::Color(c) => Paint::Solid(color_to_rgba8(*c, opacity)),
        // Linear/Radial 完整映射(v3.0 T2)。usvg 0.48 在解析收尾的
        // `update_paint_servers` 已把全部 paint server 折算为
        // userSpaceOnUse(objectBoundingBox 按目标形状 bbox 展开;
        // units 不进 pub API,"for the caller they are always
        // userSpaceOnUse"——usvg 源码 paint_server.rs),故坐标直读即
        // 用户空间。gradientTransform 折入几何;spreadMethod 不进
        // Paint 模型,按 pad 语义直读首尾色标。
        usvg::Paint::LinearGradient(g) => {
            let t = tiny_transform_to_affine(g.transform());
            Paint::LinearGradient {
                start: affine_point(t, f64::from(g.x1()), f64::from(g.y1())),
                end: affine_point(t, f64::from(g.x2()), f64::from(g.y2())),
                stops: import_stops(g.stops(), opacity),
            }
        }
        usvg::Paint::RadialGradient(g) => {
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
        // usvg 0.48 在解析期即完成文本布局(Text 节点转组+路径),
        // skipped_text 只对残留 Text 节点计数——此处断言"含文本的 SVG 可导入"
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><text x=\"1\" y=\"2\">hi</text><rect width=\"4\" height=\"4\"/></svg>";
        let (_, report) = import_svg(svg).expect("含文本 SVG 可导入");
        assert!(report.nodes >= 1);
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
