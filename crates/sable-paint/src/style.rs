//! sable-foundation 场景类型 → peniko/kurbo 渲染类型的转换层。
//!
//! GPU(vello 0.10)与 CPU(vello_cpu 0.2)共用同一套转换:两者的绘制入口都吃
//! `peniko::Brush`/`peniko::Gradient`(vello_cpu 0.2 的 `PaintType` 就是
//! `peniko::Brush<ImageBrush<ImageSource>>` 的别名,实心色另有 blanket `From`)。
//!
//! peniko 0.6 API 依据:docs.rs/peniko/0.6.1(`Gradient::new_linear/new_radial`、
//! `ColorStop { offset: f32, color: DynamicColor }`、`ColorStops` 的 `DerefMut::push`、
//! `ColorStop::From<(f32, AlphaColor<Srgb>)>`)。

use kurbo::{Point, Stroke};
use sable_foundation::scene::{GradientStop, Paint, Rgba8, StrokeStyle};

/// `Rgba8` → `peniko::Color`(straight alpha,`from_rgba8`)。
pub fn to_color(rgba: Rgba8) -> peniko::Color {
    peniko::Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3])
}

/// `Paint` → `peniko::Brush`(GPU 路径用;Solid→`Brush::Solid`,渐变→`Brush::Gradient`)。
pub fn to_brush(paint: &Paint) -> peniko::Brush {
    match paint {
        Paint::Solid(c) => peniko::Brush::Solid(to_color(*c)),
        Paint::LinearGradient { start, end, stops } => {
            peniko::Brush::Gradient(linear_gradient(start, end, stops))
        }
        Paint::RadialGradient {
            center,
            radius,
            stops,
        } => peniko::Brush::Gradient(radial_gradient(center, *radius, stops)),
    }
}

/// 线性渐变 → `peniko::Gradient`(CPU 路径直接把 Gradient 交给 `set_paint`)。
pub fn linear_gradient(
    start: &[f64; 2],
    end: &[f64; 2],
    stops: &[GradientStop],
) -> peniko::Gradient {
    let mut gradient = peniko::Gradient::new_linear(point(start), point(end));
    push_stops(&mut gradient.stops, stops);
    gradient
}

/// 径向渐变 → `peniko::Gradient`(radius 钳制到 ≥ 0,负半径无意义)。
pub fn radial_gradient(center: &[f64; 2], radius: f64, stops: &[GradientStop]) -> peniko::Gradient {
    let mut gradient =
        peniko::Gradient::new_radial(point(center), (radius as f32).clamp(0.0, f32::MAX));
    push_stops(&mut gradient.stops, stops);
    gradient
}

/// `StrokeStyle` → `kurbo::Stroke`(世界坐标 f64,交 GPU 前才由后端降 f32)。
pub fn to_stroke(style: &StrokeStyle) -> Stroke {
    Stroke::new(style.width)
}

/// 节点不透明度:把 `Paint` 的每个 alpha 乘上 `opacity`(钳制到 [0, 1])。
///
/// 这是 `PaintSink::fill_with_opacity` 默认实现的数学核心,GPU/CPU 共享同一语义:
/// alpha 在笔刷里预先相乘,而不是走图层混合,因此对两套后端逐位一致。
pub fn with_opacity(paint: &Paint, opacity: f64) -> Paint {
    let opacity = opacity.clamp(0.0, 1.0);
    match paint {
        Paint::Solid(c) => Paint::Solid(scale_alpha(*c, opacity)),
        Paint::LinearGradient { start, end, stops } => Paint::LinearGradient {
            start: *start,
            end: *end,
            stops: scale_stops(stops, opacity),
        },
        Paint::RadialGradient {
            center,
            radius,
            stops,
        } => Paint::RadialGradient {
            center: *center,
            radius: *radius,
            stops: scale_stops(stops, opacity),
        },
    }
}

fn point(p: &[f64; 2]) -> Point {
    Point::new(p[0], p[1])
}

fn push_stops(dest: &mut peniko::ColorStops, stops: &[GradientStop]) {
    for stop in stops {
        // ColorStop: From<(f32, AlphaColor<Srgb>)>(peniko 0.6 实测签名)。
        dest.push(peniko::ColorStop::from((stop.offset, to_color(stop.color))));
    }
}

fn scale_alpha(color: Rgba8, opacity: f64) -> Rgba8 {
    let alpha = f64::from(color[3]) * opacity;
    [
        color[0],
        color[1],
        color[2],
        alpha.round().clamp(0.0, 255.0) as u8,
    ]
}

fn scale_stops(stops: &[GradientStop], opacity: f64) -> Vec<GradientStop> {
    stops
        .iter()
        .map(|stop| GradientStop {
            offset: stop.offset,
            color: scale_alpha(stop.color, opacity),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use sable_foundation::scene::Paint;

    use super::*;

    #[test]
    fn solid_to_brush_keeps_channels() {
        let brush = to_brush(&Paint::Solid([0x10, 0x20, 0x30, 0xF0]));
        match brush {
            peniko::Brush::Solid(color) => {
                assert_eq!(color.to_rgba8().to_u8_array(), [0x10u8, 0x20, 0x30, 0xF0])
            }
            _ => panic!("Solid 应转换为 Brush::Solid"),
        }
    }

    #[test]
    fn linear_gradient_keeps_geometry_and_stops() {
        let paint = Paint::LinearGradient {
            start: [0.0, 0.0],
            end: [100.0, 0.0],
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: [255, 0, 0, 255],
                },
                GradientStop {
                    offset: 1.0,
                    color: [0, 0, 255, 128],
                },
            ],
        };
        let brush = to_brush(&paint);
        match brush {
            peniko::Brush::Gradient(gradient) => {
                match gradient.kind {
                    peniko::GradientKind::Linear(position) => {
                        assert_eq!(position.start, Point::new(0.0, 0.0));
                        assert_eq!(position.end, Point::new(100.0, 0.0));
                    }
                    _ => panic!("LinearGradient 应映射到 GradientKind::Linear"),
                }
                assert_eq!(gradient.stops.len(), 2, "色标数必须一致");
                assert_eq!(gradient.stops[0].offset, 0.0);
                assert_eq!(gradient.stops[1].offset, 1.0);
                // ColorStop: PartialEq —— 用同样的 From 语义构造期望值逐位比较。
                assert_eq!(
                    gradient.stops[0],
                    peniko::ColorStop::from((0.0, to_color([255, 0, 0, 255])))
                );
                assert_eq!(
                    gradient.stops[1],
                    peniko::ColorStop::from((1.0, to_color([0, 0, 255, 128])))
                );
            }
            _ => panic!("渐变应转换为 Brush::Gradient"),
        }
    }

    #[test]
    fn radial_gradient_keeps_center_radius_and_stops() {
        let paint = Paint::RadialGradient {
            center: [32.0, 32.0],
            radius: 24.0,
            stops: vec![GradientStop {
                offset: 0.5,
                color: [10, 20, 30, 40],
            }],
        };
        let brush = to_brush(&paint);
        match brush {
            peniko::Brush::Gradient(gradient) => {
                match gradient.kind {
                    peniko::GradientKind::Radial(position) => {
                        assert_eq!(position.start_center, Point::new(32.0, 32.0));
                        assert_eq!(position.end_center, Point::new(32.0, 32.0));
                        assert!((position.end_radius - 24.0).abs() < 1e-6);
                    }
                    _ => panic!("RadialGradient 应映射到 GradientKind::Radial"),
                }
                assert_eq!(gradient.stops.len(), 1);
                assert_eq!(gradient.stops[0].offset, 0.5);
            }
            _ => panic!("渐变应转换为 Brush::Gradient"),
        }
    }

    #[test]
    fn with_opacity_scales_solid_alpha() {
        let scaled = with_opacity(&Paint::Solid([255, 0, 0, 255]), 0.5);
        assert_eq!(scaled, Paint::Solid([255, 0, 0, 128]));
    }

    #[test]
    fn with_opacity_scales_gradient_stop_alphas_only() {
        let paint = Paint::LinearGradient {
            start: [1.0, 2.0],
            end: [3.0, 4.0],
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: [255, 0, 0, 255],
                },
                GradientStop {
                    offset: 1.0,
                    color: [0, 0, 255, 128],
                },
            ],
        };
        let scaled = with_opacity(&paint, 0.5);
        match scaled {
            Paint::LinearGradient { start, end, stops } => {
                assert_eq!(start, [1.0, 2.0]);
                assert_eq!(end, [3.0, 4.0]);
                assert_eq!(stops[0].color, [255, 0, 0, 128]);
                assert_eq!(stops[1].color, [0, 0, 255, 64]);
            }
            _ => panic!("不应改变 Paint 的变体"),
        }
    }

    #[test]
    fn with_opacity_clamps_opacity() {
        // >1:不变;-1:alpha 全 0。
        let paint = Paint::Solid([255, 0, 0, 100]);
        assert_eq!(with_opacity(&paint, 2.0), paint);
        assert_eq!(with_opacity(&paint, -1.0), Paint::Solid([255, 0, 0, 0]));
    }
}
