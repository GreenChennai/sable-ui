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
use sable_foundation::scene::{BlendMode, GradientStop, Paint, Rgba8, StrokeStyle};

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
        Paint::ConicGradient {
            center,
            start_angle,
            end_angle,
            stops,
        } => peniko::Brush::Gradient(sweep_gradient(center, *start_angle, *end_angle, stops)),
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

/// 锥形渐变(迭代计划 08 E9)→ `peniko::Gradient`。
///
/// 角度 f64 弧度 → f32 弧度一次性降位:peniko 0.6.1 的
/// `Gradient::new_sweep(center, start_angle: f32, end_angle: f32)` 以 f32
/// 存储,自正 X 轴起、Y 向下坐标系顺时针(与 CSS conic 的视觉约定一致)。
pub fn sweep_gradient(
    center: &[f64; 2],
    start_angle: f64,
    end_angle: f64,
    stops: &[GradientStop],
) -> peniko::Gradient {
    let mut gradient =
        peniko::Gradient::new_sweep(point(center), start_angle as f32, end_angle as f32);
    push_stops(&mut gradient.stops, stops);
    gradient
}

/// 场景 [`BlendMode`] → `peniko::BlendMode`(迭代计划 08 E5)。
///
/// 16 种 `Mix` 逐一映射、`Compose` 恒为 `SrcOver`(Illustrator 图层混合
/// 语义只涉及 Mix 轴)。返回值用本 crate 直接依赖的 `peniko::BlendMode`:
/// workspace 与 vello 0.10 / vello_cpu 0.2 锁同一 peniko 0.6.1,三种路径下
/// 是**同一类型**(cargo 版本统一),GPU/CPU 两个 Sink 共用本函数。
pub fn to_vello_blend(mode: BlendMode) -> peniko::BlendMode {
    let mix = match mode {
        BlendMode::Normal => peniko::Mix::Normal,
        BlendMode::Multiply => peniko::Mix::Multiply,
        BlendMode::Screen => peniko::Mix::Screen,
        BlendMode::Overlay => peniko::Mix::Overlay,
        BlendMode::Darken => peniko::Mix::Darken,
        BlendMode::Lighten => peniko::Mix::Lighten,
        BlendMode::ColorDodge => peniko::Mix::ColorDodge,
        BlendMode::ColorBurn => peniko::Mix::ColorBurn,
        BlendMode::HardLight => peniko::Mix::HardLight,
        BlendMode::SoftLight => peniko::Mix::SoftLight,
        BlendMode::Difference => peniko::Mix::Difference,
        BlendMode::Exclusion => peniko::Mix::Exclusion,
        BlendMode::Hue => peniko::Mix::Hue,
        BlendMode::Saturation => peniko::Mix::Saturation,
        BlendMode::Color => peniko::Mix::Color,
        BlendMode::Luminosity => peniko::Mix::Luminosity,
    };
    peniko::BlendMode::new(mix, peniko::Compose::SrcOver)
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
        Paint::ConicGradient {
            center,
            start_angle,
            end_angle,
            stops,
        } => Paint::ConicGradient {
            center: *center,
            start_angle: *start_angle,
            end_angle: *end_angle,
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

    #[test]
    fn sweep_gradient_keeps_center_angles_and_stops() {
        let paint = Paint::ConicGradient {
            center: [32.0, 32.0],
            start_angle: 0.25,
            end_angle: 6.0,
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
        };
        let brush = to_brush(&paint);
        match brush {
            peniko::Brush::Gradient(gradient) => {
                match gradient.kind {
                    peniko::GradientKind::Sweep(position) => {
                        assert_eq!(position.center, Point::new(32.0, 32.0));
                        // f32 降位:0.25/6.0 均可被 f32 精确表示
                        assert_eq!(position.start_angle, 0.25f32);
                        assert_eq!(position.end_angle, 6.0f32);
                    }
                    _ => panic!("ConicGradient 应映射到 GradientKind::Sweep"),
                }
                assert_eq!(gradient.stops.len(), 2, "色标数必须一致");
                assert_eq!(gradient.stops[0].offset, 0.0);
                assert_eq!(gradient.stops[1].offset, 1.0);
            }
            _ => panic!("渐变应转换为 Brush::Gradient"),
        }
    }

    #[test]
    fn with_opacity_scales_conic_stop_alphas() {
        let paint = Paint::ConicGradient {
            center: [0.0, 0.0],
            start_angle: 0.0,
            end_angle: 1.0,
            stops: vec![GradientStop {
                offset: 0.5,
                color: [10, 20, 30, 255],
            }],
        };
        let scaled = with_opacity(&paint, 0.5);
        match scaled {
            Paint::ConicGradient {
                center,
                start_angle,
                end_angle,
                stops,
            } => {
                assert_eq!(center, [0.0, 0.0]);
                assert_eq!(start_angle, 0.0);
                assert_eq!(end_angle, 1.0);
                assert_eq!(stops[0].color, [10, 20, 30, 128], "只缩 alpha,几何不动");
            }
            _ => panic!("不应改变 Paint 的变体"),
        }
    }

    /// 16 种混合模式逐一映射到同名 peniko `Mix`,Compose 恒为 SrcOver。
    #[test]
    fn to_vello_blend_maps_all_sixteen_modes() {
        let pairs = [
            (BlendMode::Normal, peniko::Mix::Normal),
            (BlendMode::Multiply, peniko::Mix::Multiply),
            (BlendMode::Screen, peniko::Mix::Screen),
            (BlendMode::Overlay, peniko::Mix::Overlay),
            (BlendMode::Darken, peniko::Mix::Darken),
            (BlendMode::Lighten, peniko::Mix::Lighten),
            (BlendMode::ColorDodge, peniko::Mix::ColorDodge),
            (BlendMode::ColorBurn, peniko::Mix::ColorBurn),
            (BlendMode::HardLight, peniko::Mix::HardLight),
            (BlendMode::SoftLight, peniko::Mix::SoftLight),
            (BlendMode::Difference, peniko::Mix::Difference),
            (BlendMode::Exclusion, peniko::Mix::Exclusion),
            (BlendMode::Hue, peniko::Mix::Hue),
            (BlendMode::Saturation, peniko::Mix::Saturation),
            (BlendMode::Color, peniko::Mix::Color),
            (BlendMode::Luminosity, peniko::Mix::Luminosity),
        ];
        for (mode, mix) in pairs {
            let blend = to_vello_blend(mode);
            assert_eq!(blend.mix, mix, "{mode:?} 应映射到 {mix:?}");
            assert_eq!(blend.compose, peniko::Compose::SrcOver);
        }
        // 默认(Normal)与 peniko 默认逐位一致
        assert_eq!(
            to_vello_blend(BlendMode::Normal),
            peniko::BlendMode::default()
        );
    }
}
