//! CPU 兜底渲染(vello_cpu 0.2,分册六 §6.4 降级链第 4 级)。
//!
//! 无 GPU 环境/服务器渲染/渲染回归测试的最终兜底:整条链路纯软件,
//! 输出预乘 alpha 的 RGBA8 缓冲,便于逐像素断言(渲染回归测试的基石)。
//!
//! API 依据(docs.rs/vello_cpu/0.2.0,已核实签名):
//! - `RenderContext::new(width: u16, height: u16)`
//! - `set_transform(&mut self, transform: kurbo::Affine)` / `set_stroke(&mut self, kurbo::Stroke)`
//! - `set_paint(&mut self, paint: impl Into<PaintType>)`,`PaintType = peniko::Brush<ImageBrush<ImageSource>>`
//!   —— 实心色(`AlphaColor<Srgb>`)与 `peniko::Gradient` 都有 blanket `From` 实现;
//! - `fill_path(&mut self, path: &BezPath)` / `stroke_path(&mut self, path: &BezPath)`
//! - `flush(&mut self)` → `render<'a>(&self, target: impl Into<PixmapMut<'a>>, resources: &mut Resources)`
//! - `Pixmap::new(width: u16, height: u16)` → `data_as_u8_slice() -> &[u8]`(预乘 RGBA8,行主序)

use kurbo::{Affine, BezPath, Shape};
use lumina_core::scene::{Paint, Rgba8, StrokeStyle};

use crate::sink::PaintSink;
use crate::style;

/// 基于 `vello_cpu::RenderContext` 的 [`PaintSink`] 实现。
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
    use lumina_core::scene::StrokeStyle;

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
}
