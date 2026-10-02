//! 绘制指令抽象 —— 本 crate 的灵魂。
//!
//! 上层(sable-canvas)只面向 [`PaintSink`] 编程,不感知后端:
//! GPU(vello::Scene)与 CPU(vello_cpu::RenderContext)各给一份实现,
//! 降级时只是换 Sink,场景图遍历代码零改动。
//!
//! 坐标纪律:transform 与路径一律 kurbo(f64 世界坐标),f32 降级留在后端内部。

use kurbo::{Affine, BezPath};
use sable_foundation::scene::{Paint, StrokeStyle};

/// 后端无关的绘制指令流。
pub trait PaintSink {
    /// 无不透明度修饰的填充(fill rule:NonZero,与 GPU 路径默认一致)。
    fn fill(&mut self, paint: &Paint, transform: Affine, path: &BezPath);

    /// 描边(线宽为世界坐标宽度,屏幕等宽描边由上层除以 zoom 后传入)。
    fn stroke(&mut self, style: &StrokeStyle, transform: Affine, path: &BezPath);

    /// 带 opacity 的 fill(节点不透明度:alpha 相乘)。
    ///
    /// 默认实现把 `Paint` 的 alpha 逐项乘上 `opacity` 后转调 [`PaintSink::fill`],
    /// GPU/CPU 共享同一数学,不依赖任何后端的图层混合:
    /// - `opacity >= 1.0`:原样转调,不重建 Paint;
    /// - `opacity <= 0.0`:完全透明,直接跳过绘制(视觉等价,省一次光栅化)。
    fn fill_with_opacity(
        &mut self,
        paint: &Paint,
        opacity: f64,
        transform: Affine,
        path: &BezPath,
    ) {
        if opacity >= 1.0 {
            self.fill(paint, transform, path);
            return;
        }
        if opacity <= 0.0 {
            return;
        }
        let scaled = crate::style::with_opacity(paint, opacity);
        self.fill(&scaled, transform, path);
    }
}

#[cfg(test)]
mod tests {
    use sable_foundation::scene::GradientStop;

    use super::*;

    /// 只记录每次 fill 收到的 alpha 序列(不依赖 sable-foundation 的 Clone)。
    struct RecordingSink {
        fills: Vec<Vec<u8>>,
        strokes: usize,
    }

    fn alphas(paint: &Paint) -> Vec<u8> {
        match paint {
            Paint::Solid(c) => vec![c[3]],
            Paint::LinearGradient { stops, .. } | Paint::RadialGradient { stops, .. } => {
                stops.iter().map(|s| s.color[3]).collect()
            }
        }
    }

    impl PaintSink for RecordingSink {
        fn fill(&mut self, paint: &Paint, _transform: Affine, _path: &BezPath) {
            self.fills.push(alphas(paint));
        }

        fn stroke(&mut self, _style: &StrokeStyle, _transform: Affine, _path: &BezPath) {
            self.strokes += 1;
        }
    }

    fn sample_gradient() -> Paint {
        Paint::LinearGradient {
            start: [0.0, 0.0],
            end: [1.0, 0.0],
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
        }
    }

    #[test]
    fn fill_with_opacity_multiplies_alpha_into_paint() {
        let mut sink = RecordingSink {
            fills: Vec::new(),
            strokes: 0,
        };
        let paint = sample_gradient();
        sink.fill_with_opacity(&paint, 0.5, Affine::IDENTITY, &BezPath::new());
        assert_eq!(sink.fills, vec![vec![128, 64]], "每个 alpha 都应乘 0.5");
    }

    #[test]
    fn fill_with_opacity_one_is_passthrough() {
        let mut sink = RecordingSink {
            fills: Vec::new(),
            strokes: 0,
        };
        let paint = sample_gradient();
        sink.fill_with_opacity(&paint, 1.0, Affine::IDENTITY, &BezPath::new());
        assert_eq!(sink.fills, vec![vec![255, 128]], "opacity=1 保持原 alpha");
    }

    #[test]
    fn fill_with_opacity_zero_skips_drawing() {
        let mut sink = RecordingSink {
            fills: Vec::new(),
            strokes: 0,
        };
        let paint = sample_gradient();
        sink.fill_with_opacity(&paint, 0.0, Affine::IDENTITY, &BezPath::new());
        sink.fill_with_opacity(&paint, -1.0, Affine::IDENTITY, &BezPath::new());
        assert!(sink.fills.is_empty(), "opacity<=0 不应产生绘制调用");
    }
}
