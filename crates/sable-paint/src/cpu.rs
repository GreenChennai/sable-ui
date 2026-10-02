//! CPU 兜底渲染(vello_cpu 0.2,分册六 §6.4 降级链第 4 级)。
//!
//! 无 GPU 环境/服务器渲染/渲染回归测试的最终兜底:整条链路纯软件,
//! 输出预乘 alpha 的 RGBA8 缓冲,便于逐像素断言(渲染回归测试的基石)。
//!
//! API 依据(docs.rs/vello_cpu/0.2.0,已核实签名):
//! - `RenderContext::new(width: u16, height: u16)`
//! - `set_transform(&mut self, transform: kurbo::Affine)` / `set_stroke(&mut self, kurbo::Stroke)`
//! - `set_paint(&mut self, paint: impl Into<PaintType>)`,`PaintType =
//!   peniko::Brush<Image, Gradient>`(vello_common 0.2 src/paint.rs 实测)——
//!   实心色(`AlphaColor<Srgb>`)与 `peniko::Gradient` 都有 blanket `From` 实现;
//! - `fill_path(&mut self, path: &BezPath)` / `stroke_path(&mut self, path: &BezPath)`
//! - `flush(&mut self)` → `render<'a>(&self, target: impl Into<PixmapMut<'a>>, resources: &mut Resources)`
//! - `Pixmap::new(width: u16, height: u16)` → `data_as_u8_slice() -> &[u8]`(预乘 RGBA8,行主序)

use kurbo::{Affine, BezPath, Shape};
use sable_foundation::scene::{BlendMode, Paint, Rgba8, StrokeStyle};

use crate::sink::PaintSink;
use crate::style;

/// 基于 `vello_cpu::RenderContext` 的 [`PaintSink`] 实现。
///
/// # 混合模式(E5)与锥形渐变(E9)的 CPU 支持度(源码已核实)
///
/// vello_cpu 0.2.0 **原生支持混合层**:`RenderContext::push_blend_layer(
/// peniko::BlendMode)` + `pop_layer()`(vello_cpu src/render.rs,混合在
/// dispatch/single_threaded 合成路径真实现),因此本 Sink 对 E5 是**真实现**,
/// 不存在"E12 降级为 Normal"的损失;`GradientKind::Sweep` 在
/// vello_cpu src/fine/common/gradient/sweep.rs 有专用光栅化,E9 锥形渐变
/// 同样为真实现。
pub struct VelloCpuSink {
    ctx: vello_cpu::RenderContext,
}

impl VelloCpuSink {
    /// 新建画布上下文,初始内容为全透明。
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            ctx: vello_cpu::RenderContext::new(width, height),
        }
    }

    /// 透出底层上下文,便于上层使用 PaintSink 之外的能力(文字/图层等)。
    pub fn context(&mut self) -> &mut vello_cpu::RenderContext {
        &mut self.ctx
    }

    fn set_cpu_paint(&mut self, paint: &Paint) {
        // 不直接传完整 Brush:PaintType 的 Image 变体载荷是 vello_cpu 自己的
        // ImageSource,按变体分别转 Color/Gradient 走 peniko 的 blanket From,最稳。
        match paint {
            Paint::Solid(c) => self.ctx.set_paint(style::to_color(*c)),
            Paint::LinearGradient { start, end, stops } => {
                self.ctx
                    .set_paint(style::linear_gradient(start, end, stops));
            }
            Paint::RadialGradient {
                center,
                radius,
                stops,
            } => {
                self.ctx
                    .set_paint(style::radial_gradient(center, *radius, stops));
            }
            Paint::ConicGradient {
                center,
                start_angle,
                end_angle,
                stops,
            } => {
                self.ctx.set_paint(style::sweep_gradient(
                    center,
                    *start_angle,
                    *end_angle,
                    stops,
                ));
            }
        }
    }
}

impl PaintSink for VelloCpuSink {
    fn fill(&mut self, paint: &Paint, transform: Affine, path: &BezPath) {
        self.ctx.set_transform(transform);
        self.set_cpu_paint(paint);
        self.ctx.fill_path(path);
    }

    fn stroke(&mut self, style: &StrokeStyle, transform: Affine, path: &BezPath) {
        self.ctx.set_transform(transform);
        self.set_cpu_paint(&style.paint);
        self.ctx.set_stroke(style::to_stroke(style));
        self.ctx.stroke_path(path);
    }

    /// 真实现:vello_cpu 0.2 的 `push_blend_layer`(混合层覆盖整个画布,
    /// 后续绘制与已绘背景按 `mode` 混合,直到配对 `pop_layer`)。
    fn push_blend(&mut self, mode: BlendMode) {
        self.ctx.push_blend_layer(style::to_vello_blend(mode));
    }

    fn pop_blend(&mut self) {
        self.ctx.pop_layer();
    }

    /// 真实现:vello_cpu image paint(peniko `ImageBrush`)。像素按**预乘
    /// RGBA8**(`AlphaPremultiplied`,与 [`crate::effects::apply_effects_rgba`]
    /// 的输出同语义)喂给 `ImageSource::from_peniko_image_data`——该转换对
    /// 预乘数据逐位直通,不做二次预乘。变换取整数平移 `(dx, dy)`:vello_common
    /// 对"纯整数平移 + Medium 采样"自动降为最近邻(`encode.rs` 的 quality
    /// 优化),像素 1:1 落位,无双线性渗色;填充矩形恰好覆盖图像域
    /// `(0,0)-(w,h)`,越界部分由光栅器按瓦片裁剪。
    fn draw_rgba(&mut self, rgba: &[u8], w: u16, h: u16, dx: i32, dy: i32) {
        let image_data = peniko::ImageData {
            data: peniko::Blob::new(std::sync::Arc::new(rgba.to_vec())),
            format: peniko::ImageFormat::Rgba8,
            alpha_type: peniko::ImageAlphaType::AlphaPremultiplied,
            width: u32::from(w),
            height: u32::from(h),
        };
        let source = vello_cpu::ImageSource::from_peniko_image_data(&image_data);
        self.ctx.set_paint(vello_cpu::Image {
            image: source,
            sampler: peniko::ImageSampler::default(),
        });
        self.ctx
            .set_transform(Affine::translate((f64::from(dx), f64::from(dy))));
        let rect = kurbo::Rect::new(0.0, 0.0, f64::from(w), f64::from(h)).to_path(0.1);
        self.ctx.fill_path(&rect);
    }

    /// 本后端以 vello_cpu image paint 支持像素回贴(效果离屏管线门槛)。
    fn supports_draw_rgba(&self) -> bool {
        true
    }
}

/// 独立的 CPU 渲染器:管理画布尺寸、底色与资源,渲染后取回 RGBA8 缓冲。
pub struct CpuRenderer {
    width: u16,
    height: u16,
    sink: VelloCpuSink,
    resources: vello_cpu::Resources,
}

impl CpuRenderer {
    /// 新建渲染器并铺底色(在任何用户绘制之前整幅填充一次)。
    pub fn new(width: u16, height: u16, base_color: Rgba8) -> Self {
        let mut sink = VelloCpuSink::new(width, height);
        sink.fill(
            &Paint::Solid(base_color),
            Affine::IDENTITY,
            &full_canvas_path(width, height),
        );
        Self {
            width,
            height,
            sink,
            resources: vello_cpu::Resources::new(),
        }
    }

    /// 画布宽度(像素)。
    pub fn width(&self) -> u16 {
        self.width
    }

    /// 画布高度(像素)。
    pub fn height(&self) -> u16 {
        self.height
    }

    /// 取绘制入口,接收 [`PaintSink`] 指令流。
    pub fn sink(&mut self) -> &mut VelloCpuSink {
        &mut self.sink
    }

    /// 渲染并取回像素:预乘 alpha 的 RGBA8,行主序,长度 `width * height * 4`。
    ///
    /// 注意 vello_cpu 的输出是**预乘**语义:半透明红的像素形如 `(128, 0, 0, 128)`,
    /// 消费方(PNG/纹理上传)需按预乘处理或先 un-premultiply。
    pub fn finish(mut self) -> Vec<u8> {
        let mut pixmap = vello_cpu::Pixmap::new(self.width, self.height);
        self.sink.ctx.flush();
        self.sink.ctx.render(&mut pixmap, &mut self.resources);
        pixmap.data_as_u8_slice().to_vec()
    }
}

fn full_canvas_path(width: u16, height: u16) -> BezPath {
    kurbo::Rect::new(0.0, 0.0, f64::from(width), f64::from(height)).to_path(0.1)
}

#[cfg(test)]
mod tests {
    use sable_foundation::scene::{GradientStop, StrokeStyle};

    use super::*;

    const W: u16 = 64;
    const H: u16 = 64;
    const WHITE: Rgba8 = [255, 255, 255, 255];
    const RED: Rgba8 = [255, 0, 0, 255];
    const BLACK: Rgba8 = [0, 0, 0, 255];
    const TRANSPARENT: Rgba8 = [0, 0, 0, 0];

    fn pixel(buf: &[u8], x: u16, y: u16) -> [u8; 4] {
        let i = 4 * (usize::from(y) * usize::from(W) + usize::from(x));
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    /// 居中 32×32 矩形(画布 64×64)。
    fn centered_rect() -> BezPath {
        kurbo::Rect::new(16.0, 16.0, 48.0, 48.0).to_path(0.1)
    }

    #[test]
    fn solid_fill_colors_expected_pixels() {
        let mut renderer = CpuRenderer::new(W, H, WHITE);
        renderer
            .sink()
            .fill(&Paint::Solid(RED), Affine::IDENTITY, &centered_rect());
        let buf = renderer.finish();
        assert_eq!(pixel(&buf, 32, 32), [255, 0, 0, 255], "矩形中心应为红色");
        assert_eq!(pixel(&buf, 2, 2), WHITE, "矩形外应保持底色");
    }

    #[test]
    fn stroke_draws_line_pixels() {
        let mut renderer = CpuRenderer::new(W, H, WHITE);
        let mut path = BezPath::new();
        path.move_to((8.0, 32.0));
        path.line_to((56.0, 32.0));
        renderer.sink().stroke(
            &StrokeStyle {
                paint: Paint::Solid(BLACK),
                width: 2.0,
            },
            Affine::IDENTITY,
            &path,
        );
        let buf = renderer.finish();
        assert_ne!(pixel(&buf, 32, 32), WHITE, "线中心像素应为非底色");
        let non_base = (8..=56u16).filter(|&x| pixel(&buf, x, 32) != WHITE).count();
        assert!(non_base >= 1, "线上应至少有一个非底色像素");
    }

    #[test]
    fn fill_with_opacity_halves_alpha_on_transparent_base() {
        // 透明底:合成结果即笔刷本身(预乘),0.5 红应得到约 (128, 0, 0, 128)。
        let mut renderer = CpuRenderer::new(W, H, TRANSPARENT);
        renderer.sink().fill_with_opacity(
            &Paint::Solid(RED),
            0.5,
            Affine::IDENTITY,
            &centered_rect(),
        );
        let buf = renderer.finish();
        let p = pixel(&buf, 32, 32);
        // 预乘 alpha 语义断言(±2 容差吸收 u8 量化舍入)。
        assert!(
            (128i32 - i32::from(p[0])).abs() <= 2,
            "预乘 r 应约 128,实际 {p:?}"
        );
        assert!(p[1] <= 2 && p[2] <= 2, "g/b 应为 0,实际 {p:?}");
        assert!(
            (128i32 - i32::from(p[3])).abs() <= 2,
            "alpha 应约减半到 128,实际 {p:?}"
        );
        assert_eq!(pixel(&buf, 2, 2), TRANSPARENT, "矩形外应保持全透明");
    }

    #[test]
    fn base_color_underlies_user_drawing() {
        // 深灰底 + 不覆盖的区域必须保持深灰(验证底色先于用户绘制铺下)。
        let base: Rgba8 = [32, 32, 32, 255];
        let mut renderer = CpuRenderer::new(W, H, base);
        renderer
            .sink()
            .fill(&Paint::Solid(RED), Affine::IDENTITY, &centered_rect());
        let buf = renderer.finish();
        assert_eq!(pixel(&buf, 2, 2), base);
        assert_eq!(pixel(&buf, 32, 32), [255, 0, 0, 255]);
    }

    /// 同色灰的 multiply 混合必须把背景压暗(s·d < max(s,d)),
    /// 且混合层外的像素不受影响(混合层覆盖全画布但绘制有形状边界)。
    #[test]
    fn multiply_blend_darkens_backdrop() {
        const GRAY: Rgba8 = [180, 180, 180, 255];
        let mut renderer = CpuRenderer::new(W, H, GRAY);
        {
            let sink = renderer.sink();
            sink.push_blend(BlendMode::Multiply);
            sink.fill(&Paint::Solid(GRAY), Affine::IDENTITY, &centered_rect());
            sink.pop_blend();
        }
        let buf = renderer.finish();
        let blended = pixel(&buf, 32, 32);
        assert!(
            i32::from(blended[1]) < 160,
            "multiply(s=d=180) 应显著暗于 180(W3C compositing 的 multiply 语义,实际 {blended:?})"
        );
        assert!(
            i32::from(blended[1]) > 80,
            "multiply 不应把不透明同色压成黑(排除实现把混合当清空),实际 {blended:?}"
        );
        assert_eq!(pixel(&buf, 2, 2), GRAY, "混合矩形外保持底色");
    }

    /// Normal 混合(push_blend 默认语义)不得改变普通合成结果:同色覆盖
    /// 不产生视觉差(对照 multiply_blend_darkens_backdrop)。
    #[test]
    fn normal_blend_layer_is_passthrough() {
        const GRAY: Rgba8 = [180, 180, 180, 255];
        let with_layer = {
            let mut renderer = CpuRenderer::new(W, H, GRAY);
            {
                let sink = renderer.sink();
                sink.push_blend(BlendMode::Normal);
                sink.fill(&Paint::Solid(GRAY), Affine::IDENTITY, &centered_rect());
                sink.pop_blend();
            }
            renderer.finish()
        };
        let without_layer = {
            let mut renderer = CpuRenderer::new(W, H, GRAY);
            renderer
                .sink()
                .fill(&Paint::Solid(GRAY), Affine::IDENTITY, &centered_rect());
            renderer.finish()
        };
        assert_eq!(with_layer, without_layer, "Normal 混合层必须逐位等价");
    }

    /// draw_rgba 整数平移回贴:预乘像素 1:1 落位、边界外不渗色(效果链
    /// 合成回画布的门槛真实现,见 `PaintSink::draw_rgba`)。
    #[test]
    fn draw_rgba_pastes_pixels_at_device_offset() {
        let mut renderer = CpuRenderer::new(W, H, WHITE);
        let rgba = [255u8, 0, 0, 255].repeat(4 * 2); // 4×2 全红预乘块
        renderer.sink().draw_rgba(&rgba, 4, 2, 10, 20);
        let buf = renderer.finish();
        assert_eq!(pixel(&buf, 12, 21), [255, 0, 0, 255], "块内像素原样落位");
        assert_eq!(pixel(&buf, 9, 21), WHITE, "块左缘外保持底色");
        assert_eq!(pixel(&buf, 14, 21), WHITE, "块右缘外保持底色");
        assert_eq!(pixel(&buf, 12, 19), WHITE, "块上缘外保持底色");
        assert_eq!(pixel(&buf, 12, 22), WHITE, "块下缘外保持底色");
    }

    /// draw_rgba 半透明预乘像素按 src-over 与底色合成(0.5 红 → 白底变粉)。
    #[test]
    fn draw_rgba_src_over_composites_semitransparent_pixels() {
        let mut renderer = CpuRenderer::new(W, H, WHITE);
        let rgba = vec![128u8, 0, 0, 128]; // 预乘 0.5 红(单像素)
        renderer.sink().draw_rgba(&rgba, 1, 1, 32, 32);
        let buf = renderer.finish();
        let p = pixel(&buf, 32, 32);
        assert!(
            (255i32 - i32::from(p[0])).abs() <= 2 && (128i32 - i32::from(p[1])).abs() <= 2,
            "白底叠预乘 0.5 红应得 ≈(255,128,128),实际 {p:?}"
        );
    }

    /// 锥形渐变(E9):全周扫描红→蓝,+X 方向取首色、+90°(Y 向下顺时针)
    /// 取 1/4 处混色、180° 取中点混色。
    #[test]
    fn conic_gradient_sweeps_by_angle() {
        let paint = Paint::ConicGradient {
            center: [32.0, 32.0],
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
        };
        let mut renderer = CpuRenderer::new(W, H, TRANSPARENT);
        renderer
            .sink()
            .fill(&paint, Affine::IDENTITY, &full_canvas_path(W, H));
        let buf = renderer.finish();

        // t=0(+X 方向):纯红
        let at_zero = pixel(&buf, 48, 32);
        assert!(
            at_zero[0] >= 240 && at_zero[1] <= 15 && at_zero[2] <= 15,
            "0° 应为纯红,实际 {at_zero:?}"
        );
        // t=0.25(90°):红 3/4 + 蓝 1/4 → r≈191, b≈64
        let at_quarter = pixel(&buf, 32, 48);
        assert!(
            at_quarter[0] > 150 && at_quarter[0] < 230 && at_quarter[2] > 30 && at_quarter[2] < 110,
            "90° 应为红蓝 3:1 混色,实际 {at_quarter:?}"
        );
        // t=0.5(180°):中点混色 r≈b≈127
        let at_half = pixel(&buf, 16, 32);
        assert!(
            (i32::from(at_half[0]) - 128).abs() <= 40 && (i32::from(at_half[2]) - 128).abs() <= 40,
            "180° 应为红蓝中点,实际 {at_half:?}"
        );
    }
}
