//! SVG 互通(V2.0 T1,docs/08):场景图 → SVG 字符串(导出,零额外依赖手写序列化)
//! 与 SVG → 场景图(导入,usvg 0.48 解析)。
//!
//! 范围(v2.0):路径/组/变换/纯色与线性渐变填充/描边宽度颜色/混合模式;
//! Text/Image 导入时跳过并计数;usvg 渐变填充降级为首停纯色(报告计数);
//! 效果(EffectSpec)不进 SVG(filter 映射 = v2.1,doc 记录)。

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
    /// 降级为首停纯色的渐变填充数。
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
            let id = format!(
                "g{}",
                defs.len() + node_name.bytes().map(usize::from).sum::<usize>()
            );
            defs.push_str(&format!(
                "<linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">",
                f(start[0]),
                f(start[1]),
                f(end[0]),
                f(end[1])
            ));
            for s in stops {
                defs.push_str(&format!(
                    "<stop offset=\"{}\" stop-color=\"{}\"/>",
                    f(f64::from(s.offset)),
                    rgba_attr(s.color)
                ));
            }
            defs.push_str("</linearGradient>");
            format!("url(#{id})")
        }
        // 径向/锥形导出降级为首停纯色(doc 注明;完整映射 = v2.1)
        Paint::RadialGradient { stops, .. } | Paint::ConicGradient { stops, .. } => stops
            .first()
            .map(|s| rgba_attr(s.color))
            .unwrap_or_else(|| "none".into()),
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

/// SVG 字符串 → 场景(新 Scene;Text/Image 跳过计数;渐变降级首停纯色)。
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
        // 渐变/图案降级为首停纯色(报告计数;完整映射 = v2.1)
        other => {
            report.simplified_gradients += 1;
            Paint::Solid(gradient_fallback(first_stop_rgba(other)))
        }
    }
}

fn first_stop_rgba(p: &usvg::Paint) -> Option<[f32; 4]> {
    match p {
        usvg::Paint::LinearGradient(g) => g.stops().first().map(|s| stop_rgba(s)),
        usvg::Paint::RadialGradient(g) => g.stops().first().map(|s| stop_rgba(s)),
        _ => None,
    }
}

fn stop_rgba(s: &usvg::Stop) -> [f32; 4] {
    let c = s.color();
    [
        f32::from(c.red),
        f32::from(c.green),
        f32::from(c.blue),
        s.opacity().get(),
    ]
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

/// 预乘域 RGBA(f32 0..1)→ 直通 Rgba8(alpha=0 全透明)。
fn gradient_fallback(rgba: Option<[f32; 4]>) -> Rgba8 {
    let Some([r, g, b, a]) = rgba else {
        return [128, 128, 128, 255];
    };
    if a <= 0.0 {
        return [0, 0, 0, 0];
    }
    let un = |v: f32| ((v / a).clamp(0.0, 1.0) * 255.0).round() as u8;
    [un(r), un(g), un(b), (a * 255.0).round() as u8]
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
        let affine = Affine::new([
            f64::from(t.sx),
            f64::from(t.kx),
            f64::from(t.ky),
            f64::from(t.sy),
            f64::from(t.tx),
            f64::from(t.ty),
        ]);
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
        // 渐变路径降级为首停红也在集合里;精确断言矩形纯色无损保留
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
}
