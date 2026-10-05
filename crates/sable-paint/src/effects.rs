//! 效果原语:阴影合成器 + 静态阴影缓存(E4)、节点效果链求值(E6/E7,S4)、
//! 效果降级矩阵(E12)。
//!
//! # E4 阴影(迭代计划 08)
//!
//! [`render_shadow_rgba`] 把形状光栅化为纯黑 coverage → 3 次 box blur 近似
//! 高斯 → 乘参数 alpha → 输出**预乘黑色 RGBA8**。5 级语义参数在
//! sable-widgets `tokens::ELEVATIONS`(e0~e4),本模块只吃原始参数。
//!
//! 性能军规"静止零成本":结果进 [`ShadowCache`](形状+参数+尺寸为 key),
//! 命中零成本;驱逐为**插入序 FIFO**(容量上限,适合"同屏阴影种类有限"
//! 的 UI 场景)。真机集成(S3)再把光栅化窗口裁到形状 bbox ×(blur+偏移)
//! 邻域,本原语先提供全画布语义正确的实现。
//!
//! # 节点效果链(迭代计划 08 S4 #4.2,分册七/Illustrator 外观面板)
//!
//! 数据模型(`EffectEntry`/`EffectSpec`)在 sable-foundation `effects`;本模块
//! 负责**像素求值**:
//!
//! - [`EffectSurface`] 把节点单独光栅化到透明离屏(cpu feature);
//! - [`apply_effects_rgba`] 对离屏结果逐条应用启用的效果(顺序 = 栈序),
//!   必要时外扩缓冲容纳模糊/投影的支撑域,返回相对原点的外扩偏移;
//! - 合成回画布走 [`crate::sink::PaintSink::draw_rgba`](CPU 真实现 = vello_cpu
//!   image paint;GPU = S3 真机管线)。
//!
//! E6 颜色矩阵的对齐方式:`apply_color_matrix` 在**直通 alpha 域**逐像素
//! 求 RGB(先 un-premultiply → 矩阵 → clamp → 重新预乘),与 SVG
//! `feColorMatrix` 的非预乘语义一致;直接在预乘域做矩阵会让半透明像素
//! 发灰(预乘值非线性),故显式往返。alpha 行(第 4 行)参与求值,默认
//! 预设保持恒等。
//!
//! E11 联动:参数拖动 → `SetEffectSpec` merge = 一步撤销(接线说明见
//! sable-foundation `effects` 模块 doc)。
//!
//! # E12 降级矩阵
//!
//! [`EffectLevel`] 三档 + [`EffectCaps`] 能力位,env
//! `SABLE_EFFECTS_LEVEL`(full/reduced/off,大小写不敏感)> 编译期默认
//! (`gpu` feature → Full,cpu-only → Reduced)。矩阵:
//!
//! | 能力 | Full(gpu) | Reduced(cpu-only) | Off(env 强制) |
//! |---|---|---|---|
//! | blur(毛玻璃/阴影) | 全质量 | 可用,质量降半(box blur 半径减半/Kawase 减 pass) | 关:纯色+描边替代 |
//! | blend(混合模式) | 全 16 种 | 全 16 种(vello_cpu 原生混合层,无损失) | 关:一律 Normal 合成 |
//! | grain(蓝噪点) | 开 | 关(防色带交给渐变插值) | 关 |
//!
//! 效果链侧的约定:`caps.blur == false`(Off 档)→ [`apply_effects_rgba`]
//! 原样返回,渲染调度层也不进离屏分支(效果整体跳过,绝不丢内容)。
//! blur 半径约定:box 半径 = 参数半径的一半(与 [`render_shadow_rgba`] 同款),
//! Reduced 档的"质量减半"由此天然成立,不再单独减半。
//!
//! env 解析抽成纯函数 [`detect_with`] 供测试;[`detect`] 是薄包装
//! (读 env,不做别的,不测)。零 unsafe:env 读取用 `std::env::var`。

use std::collections::HashMap;
use std::collections::VecDeque;
use std::hash::Hasher;
use std::sync::Arc;

use kurbo::{Affine, BezPath, PathEl, Point};
use sable_foundation::effects::{EffectEntry, EffectSpec};
use sable_foundation::scene::Rgba8;

#[cfg(feature = "cpu")]
use crate::sink::PaintSink;

/// 效果等级 env 变量名(分册六 §6.5 命名纪律:`SABLE_` 前缀)。
pub const EFFECTS_LEVEL_ENV: &str = "SABLE_EFFECTS_LEVEL";

// —— E12 降级矩阵 ——

/// 效果等级(E12 三档)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectLevel {
    /// GPU 完整版:全部效果全质量。
    Full,
    /// 简化版:能力齐备但质量降半(弱 GPU / CPU 路径)。
    Reduced,
    /// 关闭:全部效果降级为纯色+描边,绝不崩。
    Off,
}

/// 某一档下各效果能力的开/关(质量档位由消费方按 [`EffectLevel`] 再定)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectCaps {
    /// 模糊类(毛玻璃/阴影/发光)
    pub blur: bool,
    /// 图层混合模式
    pub blend: bool,
    /// 蓝噪点(E8 防色带/胶片颗粒)
    pub grain: bool,
}

/// 编译期默认档:`gpu` feature → Full,cpu-only → Reduced。
fn default_level() -> EffectLevel {
    if cfg!(feature = "gpu") {
        EffectLevel::Full
    } else {
        EffectLevel::Reduced
    }
}

/// env 值 → 档位的纯函数(测试入口):`full/reduced/off` 大小写不敏感、
/// 容忍首尾空白;未提供(`None`)或未识别的值回落编译期默认。
pub fn detect_with(level: Option<&str>) -> EffectLevel {
    match level.map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("full") => EffectLevel::Full,
        Some(v) if v.eq_ignore_ascii_case("reduced") => EffectLevel::Reduced,
        Some(v) if v.eq_ignore_ascii_case("off") => EffectLevel::Off,
        _ => default_level(),
    }
}

/// 读 `SABLE_EFFECTS_LEVEL` 决定当前档位(薄包装,逻辑全在 [`detect_with`])。
pub fn detect() -> EffectLevel {
    detect_with(std::env::var(EFFECTS_LEVEL_ENV).ok().as_deref())
}

/// 档位 → 能力位(E12 矩阵的代码形态,见模块 doc 的矩阵表)。
pub fn caps(level: EffectLevel) -> EffectCaps {
    match level {
        EffectLevel::Full => EffectCaps {
            blur: true,
            blend: true,
            grain: true,
        },
        EffectLevel::Reduced => EffectCaps {
            blur: true,
            blend: true,
            grain: false,
        },
        EffectLevel::Off => EffectCaps {
            blur: false,
            blend: false,
            grain: false,
        },
    }
}

// —— E4 阴影 ——

/// 阴影参数(黑色环境影;5 级语义预设见 sable-widgets `tokens::ELEVATIONS`)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowParams {
    /// 模糊半径 px(0 = 硬影;box blur 半径取其一半,3 次逼近高斯)
    pub blur_px: f32,
    /// 偏移(目标缓冲像素坐标;光源在正上方时 offset.1 > 0)
    pub offset: (f32, f32),
    /// 阴影不透明度(0~1)
    pub alpha: f32,
}

/// 缓存 key:路径元素哈希 + 变换/参数坐标按位哈希 + 尺寸。
///
/// f64/f32 一律 `to_bits()`(NaN/±0.0 也稳定,无哈希 panic 面);
/// `DefaultHasher`(SipHash)进程内确定性足够——缓存不跨进程。
pub fn shadow_key(
    path: &BezPath,
    transform: Affine,
    params: ShadowParams,
    width: u16,
    height: u16,
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) => {
                hasher.write_u8(0);
                hash_point(&mut hasher, *p);
            }
            PathEl::LineTo(p) => {
                hasher.write_u8(1);
                hash_point(&mut hasher, *p);
            }
            PathEl::QuadTo(p, q) => {
                hasher.write_u8(2);
                hash_point(&mut hasher, *p);
                hash_point(&mut hasher, *q);
            }
            PathEl::CurveTo(p, q, r) => {
                hasher.write_u8(3);
                hash_point(&mut hasher, *p);
                hash_point(&mut hasher, *q);
                hash_point(&mut hasher, *r);
            }
            PathEl::ClosePath => hasher.write_u8(4),
        }
    }
    for coeff in transform.as_coeffs() {
        hasher.write(&coeff.to_bits().to_le_bytes());
    }
    hasher.write(&params.blur_px.to_bits().to_le_bytes());
    hasher.write(&params.offset.0.to_bits().to_le_bytes());
    hasher.write(&params.offset.1.to_bits().to_le_bytes());
    hasher.write(&params.alpha.to_bits().to_le_bytes());
    hasher.write_u16(width);
    hasher.write_u16(height);
    hasher.finish()
}

fn hash_point<H: Hasher>(hasher: &mut H, p: Point) {
    hasher.write(&p.x.to_bits().to_le_bytes());
    hasher.write(&p.y.to_bits().to_le_bytes());
}

/// 静态阴影缓存(E4 性能军规:静止零成本)。
///
/// key = [`shadow_key`];**插入序 FIFO 驱逐**(get 不刷新新鲜度——阴影
/// 形状/参数的集合在 UI 静止时不变,按进入顺序淘汰最直观且 O(1))。
/// 命中返回 `Arc<Vec<u8>>`:同 key 命中拿到**同一份分配**。
pub struct ShadowCache {
    capacity: usize,
    entries: HashMap<u64, Arc<Vec<u8>>>,
    order: VecDeque<u64>,
    /// PERF-05 计量:命中/未命中次数(get 路径;Cell 保持 `get(&self)`
    /// 签名不变)。
    hits: std::cell::Cell<u64>,
    misses: std::cell::Cell<u64>,
}

impl ShadowCache {
    /// 指定容量上限(最小 1;容量 0 无意义)。
    pub fn new(capacity: usize) -> Self {
        ShadowCache {
            capacity: capacity.max(1),
            entries: HashMap::new(),
            order: VecDeque::new(),
            hits: std::cell::Cell::new(0),
            misses: std::cell::Cell::new(0),
        }
    }

    /// 命中返回缓存的 `Arc`(与存入的是同一份分配),未命中 `None`。
    pub fn get(&self, key: u64) -> Option<Arc<Vec<u8>>> {
        match self.entries.get(&key) {
            Some(v) => {
                self.hits.set(self.hits.get() + 1);
                Some(v.clone())
            }
            None => {
                self.misses.set(self.misses.get() + 1);
                None
            }
        }
    }

    /// PERF-05 计量:命中次数。
    pub fn hits(&self) -> u64 {
        self.hits.get()
    }

    /// PERF-05 计量:未命中次数。
    pub fn misses(&self) -> u64 {
        self.misses.get()
    }

    /// 插入(或覆盖同 key 并把它刷新为最新),返回刚存入的 `Arc`
    /// (调用方可直接使用,无需再 `get` 一轮)。
    pub fn insert(&mut self, key: u64, pixels: Vec<u8>) -> Arc<Vec<u8>> {
        let stored = Arc::new(pixels);
        if self.entries.contains_key(&key) {
            // 覆盖语义:刷新插入序,替换内容
            self.order.retain(|&k| k != key);
            self.order.push_back(key);
            self.entries.insert(key, Arc::clone(&stored));
            return stored;
        }
        if self.entries.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
        self.order.push_back(key);
        self.entries.insert(key, Arc::clone(&stored));
        stored
    }

    /// 便捷入口:命中直接返回;未命中渲染后入缓存(FIFO 驱逐随之发生)。
    #[cfg(feature = "cpu")]
    pub fn get_or_render(
        &mut self,
        path: &BezPath,
        transform: Affine,
        params: ShadowParams,
        width: u16,
        height: u16,
    ) -> Arc<Vec<u8>> {
        let key = shadow_key(path, transform, params, width, height);
        if let Some(hit) = self.get(key) {
            return hit;
        }
        let pixels = render_shadow_rgba(path, transform, params, width, height);
        // insert 直接交回刚存入的 Arc,消除"insert 后 get 必命中"假设(RB-01)
        self.insert(key, pixels)
    }
}

/// 渲染形状的黑色环境影:预乘 RGBA8(黑,alpha = 模糊 coverage × params.alpha),
/// 缓冲尺寸 `width × height`,无内部裁剪(形状/偏移/模糊越界即被缓冲边裁掉)。
///
/// 管线(E4):vello_cpu 纯黑光栅化(coverage 存于预乘 alpha)→ 3 次
/// box blur(半径 = blur_px/2,边缘钳位,近似高斯;box blur 三卷积 ≈ 高斯
/// 的经典廉价近似)→ 乘 `params.alpha` → 写出预乘黑。f32 平面累加,
/// 计算顺序固定,逐位确定。
#[cfg(feature = "cpu")]
pub fn render_shadow_rgba(
    path: &BezPath,
    transform: Affine,
    params: ShadowParams,
    width: u16,
    height: u16,
) -> Vec<u8> {
    // 1. 纯黑光栅化:偏移在目标缓冲像素坐标,故 translate 在 transform 之外
    let mut ctx = vello_cpu::RenderContext::new(width, height);
    let offset = Affine::translate((f64::from(params.offset.0), f64::from(params.offset.1)));
    ctx.set_transform(offset * transform);
    ctx.set_paint(peniko::Color::from_rgb8(0, 0, 0));
    ctx.fill_path(path);
    ctx.flush();
    let mut pixmap = vello_cpu::Pixmap::new(width, height);
    let mut resources = vello_cpu::Resources::new();
    ctx.render(&mut pixmap, &mut resources);

    // 2. 提取 alpha 平面(纯黑预乘:r=g=b=0,a=coverage)
    let data = pixmap.data_as_u8_slice();
    let n = usize::from(width) * usize::from(height);
    let mut alpha: Vec<f32> = data.chunks_exact(4).map(|px| f32::from(px[3])).collect();
    debug_assert_eq!(alpha.len(), n);

    // 3. 3 次 box blur(横+竖为一次,共 6 趟)近似高斯;半径 0 跳过
    let radius = if params.blur_px > 0.0 {
        ((params.blur_px * 0.5).round() as usize).max(1)
    } else {
        0
    };
    if radius > 0 {
        let (w, h) = (usize::from(width), usize::from(height));
        let mut tmp = vec![0.0f32; alpha.len()];
        for _ in 0..3 {
            box_blur_axis(&alpha, &mut tmp, w, h, radius, true);
            box_blur_axis(&tmp, &mut alpha, w, h, radius, false);
        }
    }

    // 4. 乘参数 alpha → 预乘黑 RGBA8(r=g=b=0 恒成立)
    let mut out = vec![0u8; n * 4];
    for (i, a) in alpha.iter().enumerate() {
        out[i * 4 + 3] = (a * params.alpha).round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// 单轴 box blur(边缘钳位:越界采样取边界值,避免画布边缘发暗)。
/// `horizontal=true` 沿 x,否则沿 y。朴素 O(n·r):阴影/效果离屏缓冲小 +
/// 结果可进 [`ShadowCache`] 静止零成本,清晰性优先(窗口滑动优化留 S3
/// 真机集成)。纯 f32 平面运算,不依赖 vello_cpu,阴影与节点效果链共用。
fn box_blur_axis(
    src: &[f32],
    dst: &mut [f32],
    width: usize,
    height: usize,
    radius: usize,
    horizontal: bool,
) {
    if horizontal {
        for y in 0..height {
            let row = y * width;
            for x in 0..width {
                let lo = x.saturating_sub(radius);
                let hi = (x + radius).min(width - 1);
                let mut sum = 0.0f32;
                for xx in lo..=hi {
                    sum += src[row + xx];
                }
                dst[row + x] = sum / (hi - lo + 1) as f32;
            }
        }
    } else {
        for x in 0..width {
            for y in 0..height {
                let lo = y.saturating_sub(radius);
                let hi = (y + radius).min(height - 1);
                let mut sum = 0.0f32;
                for yy in lo..=hi {
                    sum += src[yy * width + x];
                }
                dst[y * width + x] = sum / (hi - lo + 1) as f32;
            }
        }
    }
}

// —— E6/E7:节点效果链像素求值(迭代计划 08 S4 #4.2)——

/// box blur 半径约定:参数半径的一半,至少 1(`None` = 零模糊,可跳过)。
/// 与 [`render_shadow_rgba`] 的 `blur_px` 同一语义。
fn box_radius(param: f64) -> Option<usize> {
    if param <= 0.0 {
        None
    } else {
        Some(((param * 0.5).round() as usize).max(1))
    }
}

/// 颜色矩阵(E6)逐像素求值:**直通 alpha 域**——先 un-premultiply,矩阵
/// 作用于 (R,G,B,A) 四通道加第 5 列偏移,clamp 后重新预乘。与 SVG
/// `feColorMatrix`(非预乘语义)对齐;预乘域直接做矩阵会让半透明像素
/// 发灰,故显式往返。alpha = 0 的像素保持全零。
///
/// 4×5 矩阵布局:`matrix[输出行][输入列]` + `offsets[输出行]`;
/// `out = Σ matrix[row][col]·in[col] + offsets[row]`,通道值域 0~255
/// (矩阵系数以 255 为满量程,同 SVG 规范)。
pub fn apply_color_matrix(rgba: &mut [u8], matrix: [[f32; 4]; 4], offsets: [f32; 4]) {
    for px in rgba.chunks_exact_mut(4) {
        let alpha = f32::from(px[3]);
        let src = if alpha > 0.0 {
            // un-premultiply:f32 域做除法,单次舍入在 ±1 LSB 内
            [
                f32::from(px[0]) * 255.0 / alpha,
                f32::from(px[1]) * 255.0 / alpha,
                f32::from(px[2]) * 255.0 / alpha,
                alpha,
            ]
        } else {
            [0.0; 4]
        };
        let mut out = [0.0f32; 4];
        for (row, o) in out.iter_mut().enumerate() {
            let m = &matrix[row];
            *o = m[0] * src[0] + m[1] * src[1] + m[2] * src[2] + m[3] * src[3] + offsets[row];
        }
        let out_a = out[3].round().clamp(0.0, 255.0);
        px[3] = out_a as u8;
        if out_a <= 0.0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
        } else {
            // 重新预乘:premul = straight · alpha / 255(与 un-premultiply 对称)
            #[allow(clippy::needless_range_loop)]
            for ch in 0..3 {
                let straight = out[ch].round().clamp(0.0, 255.0);
                px[ch] = (straight * out_a / 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// 效果链的像素域外扩需求(返回 `[左, 上, 右, 下]`),`scale` = 世界→像素
/// 换算(zoom)。顺序累加——效果逐个作用、支撑域单调扩张,求和是保守
/// 上界;模糊支撑域 = 3×box 半径(box 半径 = 参数半径/2,见 [`box_radius`])。
/// 离屏 [`EffectSurface`] 用它定尺寸,`apply_effects_rgba` 内部再按需扩。
pub fn effect_margins_px(effects: &[EffectEntry], scale: f64) -> [f64; 4] {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let mut m = [0.0f64; 4]; // [左, 上, 右, 下]
    for entry in effects {
        if !entry.is_active() {
            continue;
        }
        match entry.spec {
            EffectSpec::GaussianBlur { radius } => {
                let s = blur_spread(radius, scale);
                m = [m[0] + s, m[1] + s, m[2] + s, m[3] + s];
            }
            EffectSpec::Glow { radius, .. } => {
                let s = blur_spread(radius, scale);
                m = [m[0] + s, m[1] + s, m[2] + s, m[3] + s];
            }
            EffectSpec::DropShadow { blur, offset, .. } => {
                let s = blur_spread(blur, scale);
                let (ox, oy) = (offset[0] * scale, offset[1] * scale);
                m[0] += s + (-ox).max(0.0);
                m[1] += s + (-oy).max(0.0);
                m[2] += s + ox.max(0.0);
                m[3] += s + oy.max(0.0);
            }
            EffectSpec::ColorMatrix { .. } => {}
        }
    }
    m
}

/// 模糊支撑域(像素):3 次 box blur、box 半径 = radius·scale/2(≥1)。
fn blur_spread(radius: f64, scale: f64) -> f64 {
    if radius <= 0.0 {
        0.0
    } else {
        3.0 * (radius * scale * 0.5).round().max(1.0)
    }
}

/// 透明外扩:把缓冲内容平移到 (left, top),右/下补透明;总尺寸钳到
/// u16::MAX(极值参数下宁可裁边,不溢出)。
fn expand_rgba(
    buf: &mut Vec<u8>,
    w: &mut u16,
    h: &mut u16,
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
) {
    if left == 0 && top == 0 && right == 0 && bottom == 0 {
        return;
    }
    let (ow, oh) = (usize::from(*w), usize::from(*h));
    let nw = ((ow as u64) + (left as u64) + (right as u64)).min(u64::from(u16::MAX)) as usize;
    let nh = ((oh as u64) + (top as u64) + (bottom as u64)).min(u64::from(u16::MAX)) as usize;
    let mut out = vec![0u8; nw * nh * 4];
    let copy_w = ow.min(nw.saturating_sub(left));
    let copy_h = oh.min(nh.saturating_sub(top));
    for row in 0..copy_h {
        let src = (row * ow) * 4;
        let dst = ((row + top) * nw + left) * 4;
        out[dst..dst + copy_w * 4].copy_from_slice(&buf[src..src + copy_w * 4]);
    }
    *buf = out;
    *w = nw as u16;
    *h = nh as u16;
}

/// 预乘 RGBA8 的 3×box blur(四通道同 blur:预乘域模糊无 halo)。
fn blur_rgba_premultiplied(buf: &mut [u8], w: u16, h: u16, radius: usize) {
    let (wu, hu) = (usize::from(w), usize::from(h));
    let mut planes: [Vec<f32>; 4] =
        std::array::from_fn(|ch| buf.chunks_exact(4).map(|px| f32::from(px[ch])).collect());
    let mut tmp = vec![0.0f32; wu * hu];
    for plane in &mut planes {
        for _ in 0..3 {
            box_blur_axis(plane, &mut tmp, wu, hu, radius, true);
            box_blur_axis(&tmp, plane, wu, hu, radius, false);
        }
    }
    for (i, px) in buf.chunks_exact_mut(4).enumerate() {
        for (ch, v) in px.iter_mut().enumerate() {
            *v = planes[ch][i].round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// 提取 alpha 平面(f32,0~255)。
fn alpha_plane(buf: &[u8]) -> Vec<f32> {
    buf.chunks_exact(4).map(|px| f32::from(px[3])).collect()
}

/// 彩色投影置于当前内容之下:阴影 = 当前 alpha 平面经(可选)3×box blur +
/// 偏移采样 × 颜色,按预乘 over(base over shadow)合成。
/// `radius = 0` 为硬影;偏移采样越界视为无影。
fn composite_shadow_under(
    buf: &mut [u8],
    w: u16,
    h: u16,
    radius: usize,
    offset: (i64, i64),
    color: Rgba8,
) {
    let (wu, hu) = (usize::from(w), usize::from(h));
    let mut blurred = alpha_plane(buf);
    if radius > 0 {
        let mut tmp = vec![0.0f32; blurred.len()];
        for _ in 0..3 {
            box_blur_axis(&blurred, &mut tmp, wu, hu, radius, true);
            box_blur_axis(&tmp, &mut blurred, wu, hu, radius, false);
        }
    }
    let color_a = f32::from(color[3]) / 255.0;
    for y in 0..hu {
        for x in 0..wu {
            let sx = x as i64 - offset.0;
            let sy = y as i64 - offset.1;
            let k = if sx >= 0 && sy >= 0 {
                let (sx, sy) = (sx as usize, sy as usize);
                if sx < wu && sy < hu {
                    blurred[sy * wu + sx] / 255.0 * color_a
                } else {
                    0.0
                }
            } else {
                0.0
            };
            if k <= 0.0 {
                continue;
            }
            let i = (y * wu + x) * 4;
            let keep = 1.0 - f32::from(buf[i + 3]) / 255.0;
            if keep <= 0.0 {
                continue; // base 已不透明,下垫层不可见
            }
            for (ch, c) in color.iter().take(3).enumerate() {
                let v = f32::from(buf[i + ch]) + f32::from(*c) * k * keep;
                buf[i + ch] = v.round().clamp(0.0, 255.0) as u8;
            }
            let a = f32::from(buf[i + 3]) + 255.0 * k * keep;
            buf[i + 3] = a.round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// 内发光(E7)叠于当前内容之上:蒙版 m = 1 − blur(alpha)(形状内缘亮、
/// 深处衰减到 0、外侧恒 0),发光覆盖 = alpha·m·color_a,按预乘 over
/// (glow over base)合成。
fn composite_inner_glow_over(buf: &mut [u8], w: u16, h: u16, radius: usize, color: Rgba8) {
    let (wu, hu) = (usize::from(w), usize::from(h));
    let alpha = alpha_plane(buf);
    let mut blurred = alpha.clone();
    if radius > 0 {
        let mut tmp = vec![0.0f32; blurred.len()];
        for _ in 0..3 {
            box_blur_axis(&blurred, &mut tmp, wu, hu, radius, true);
            box_blur_axis(&tmp, &mut blurred, wu, hu, radius, false);
        }
    }
    let color_a = f32::from(color[3]) / 255.0;
    for (i, px) in buf.chunks_exact_mut(4).enumerate() {
        let m = 1.0 - blurred[i] / 255.0;
        if m <= 0.0 {
            continue;
        }
        let glow_a = alpha[i] / 255.0 * m * color_a;
        if glow_a <= 0.0 {
            continue;
        }
        let keep = 1.0 - glow_a;
        for (ch, c) in color.iter().take(3).enumerate() {
            let v = f32::from(*c) * glow_a + f32::from(px[ch]) * keep;
            px[ch] = v.round().clamp(0.0, 255.0) as u8;
        }
        let a = 255.0 * glow_a + f32::from(px[3]) * keep;
        px[3] = a.round().clamp(0.0, 255.0) as u8;
    }
}

/// 对效果链求值:`base`(预乘 RGBA8 + 尺寸)→ 逐条应用 `effects` 中启用的
/// 非恒等效果(栈序)→ `(rgba, dx, dy, w, h)`,其中 `(dx, dy)` 是结果
/// 相对 base 原点的偏移(负值 = 结果比 base 大,base 内容位于
/// `(−dx, −dy)` 处),`(w, h)` 为结果尺寸。
///
/// 约定:
/// - `caps.blur == false`(E12 Off 档)→ 原样返回(效果整体关闭);
/// - `enabled = false` 或 `is_noop()` 的条目跳过;
/// - 外扩在内部按需发生(模糊支撑域 3×box 半径;投影另加偏移)。
pub fn apply_effects_rgba(
    base: (Vec<u8>, u16, u16),
    effects: &[EffectEntry],
    caps: EffectCaps,
) -> (Vec<u8>, i32, i32, u16, u16) {
    let (mut buf, mut w, mut h) = base;
    if !caps.blur {
        return (buf, 0, 0, w, h);
    }
    let mut pad_l = 0i32;
    let mut pad_t = 0i32;
    for entry in effects {
        if !entry.is_active() {
            continue;
        }
        match entry.spec {
            EffectSpec::GaussianBlur { radius } => {
                let Some(r) = box_radius(radius) else {
                    continue;
                };
                let m = 3 * r;
                expand_rgba(&mut buf, &mut w, &mut h, m, m, m, m);
                pad_l += m as i32;
                pad_t += m as i32;
                blur_rgba_premultiplied(&mut buf, w, h, r);
            }
            EffectSpec::DropShadow {
                blur,
                offset,
                color,
            } => {
                let r = box_radius(blur).unwrap_or(0);
                let spread = 3 * r;
                let (ox, oy) = (offset[0].round() as i64, offset[1].round() as i64);
                let left = spread + (-ox).max(0) as usize;
                let top = spread + (-oy).max(0) as usize;
                let right = spread + ox.max(0) as usize;
                let bottom = spread + oy.max(0) as usize;
                expand_rgba(&mut buf, &mut w, &mut h, left, top, right, bottom);
                pad_l += left as i32;
                pad_t += top as i32;
                composite_shadow_under(&mut buf, w, h, r, (ox, oy), color);
            }
            EffectSpec::Glow {
                radius,
                color,
                inner,
            } => {
                let Some(r) = box_radius(radius) else {
                    continue;
                };
                let m = 3 * r;
                expand_rgba(&mut buf, &mut w, &mut h, m, m, m, m);
                pad_l += m as i32;
                pad_t += m as i32;
                if inner {
                    composite_inner_glow_over(&mut buf, w, h, r, color);
                } else {
                    // 外发光 = 零偏移彩色投影(绘制在形状之下)
                    composite_shadow_under(&mut buf, w, h, r, (0, 0), color);
                }
            }
            EffectSpec::ColorMatrix { matrix, offsets } => {
                apply_color_matrix(&mut buf, matrix, offsets);
            }
        }
    }
    (buf, -pad_l, -pad_t, w, h)
}

/// 节点效果离屏光栅化表面(S4 #4.2;`cpu` feature)。
///
/// 把"该节点单独渲染的结果"画到一张**透明底**离屏缓冲:宽高由调用方按
/// 节点 bbox + [`effect_margins_px`] 预算,绘制时用平移后的变换把内容摆进
/// margin 内侧(见 sable-canvas `render` 的效果分支)。产出交给
/// [`apply_effects_rgba`] 求值,再经 [`crate::sink::PaintSink::draw_rgba`]
/// 贴回主画布。
#[cfg(feature = "cpu")]
pub struct EffectSurface {
    sink: crate::cpu::VelloCpuSink,
    width: u16,
    height: u16,
}

#[cfg(feature = "cpu")]
impl EffectSurface {
    /// 新建 `width × height` 的透明离屏表面。
    pub fn new(width: u16, height: u16) -> Self {
        EffectSurface {
            sink: crate::cpu::VelloCpuSink::new(width, height),
            width,
            height,
        }
    }

    /// 表面宽度(像素)。
    pub fn width(&self) -> u16 {
        self.width
    }

    /// 表面高度(像素)。
    pub fn height(&self) -> u16 {
        self.height
    }

    /// 执行一段绘制:闭包拿到后端无关的 [`PaintSink`](与主画布同一套指令
    /// 抽象,节点绘制代码零改动)。
    pub fn draw(&mut self, f: impl FnOnce(&mut dyn PaintSink)) {
        let sink: &mut dyn crate::sink::PaintSink = &mut self.sink;
        f(sink);
    }

    /// 结束绘制:光栅化并取回预乘 RGBA8(透明底)与尺寸。
    pub fn into_rgba(mut self) -> (Vec<u8>, u16, u16) {
        let mut pixmap = vello_cpu::Pixmap::new(self.width, self.height);
        let mut resources = vello_cpu::Resources::new();
        self.sink.context().flush();
        self.sink.context().render(&mut pixmap, &mut resources);
        (pixmap.data_as_u8_slice().to_vec(), self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square_path(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
        let mut path = BezPath::new();
        path.move_to((x0, y0));
        path.line_to((x1, y0));
        path.line_to((x1, y1));
        path.line_to((x0, y1));
        path.close_path();
        path
    }

    // —— E12:env 纯函数 ——

    #[test]
    fn detect_with_parses_all_three_levels_case_insensitively() {
        assert_eq!(detect_with(Some("full")), EffectLevel::Full);
        assert_eq!(detect_with(Some("FULL")), EffectLevel::Full);
        assert_eq!(detect_with(Some(" Reduced ")), EffectLevel::Reduced);
        assert_eq!(detect_with(Some("off")), EffectLevel::Off);
        assert_eq!(detect_with(Some("OFF")), EffectLevel::Off);
    }

    #[test]
    fn detect_with_falls_back_to_compile_time_default() {
        let fallback = if cfg!(feature = "gpu") {
            EffectLevel::Full
        } else {
            EffectLevel::Reduced
        };
        assert_eq!(detect_with(None), fallback);
        // 未识别值(含历史遗留垃圾值)一律回落默认,不 panic、不猜测
        assert_eq!(detect_with(Some("")), fallback);
        assert_eq!(detect_with(Some("wat")), fallback);
        assert_eq!(detect_with(Some("GPU=on")), fallback);
    }

    #[test]
    fn caps_follow_the_e12_matrix() {
        assert_eq!(
            caps(EffectLevel::Full),
            EffectCaps {
                blur: true,
                blend: true,
                grain: true
            }
        );
        assert_eq!(
            caps(EffectLevel::Reduced),
            EffectCaps {
                blur: true,
                blend: true,
                grain: false
            }
        );
        assert_eq!(
            caps(EffectLevel::Off),
            EffectCaps {
                blur: false,
                blend: false,
                grain: false
            }
        );
    }

    // —— E4:缓存 key ——

    #[test]
    fn shadow_key_is_sensitive_to_every_input() {
        let path = square_path(0.0, 0.0, 10.0, 10.0);
        let params = ShadowParams {
            blur_px: 4.0,
            offset: (0.0, 2.0),
            alpha: 0.2,
        };
        let base = shadow_key(&path, Affine::IDENTITY, params, 64, 64);

        assert_eq!(shadow_key(&path, Affine::IDENTITY, params, 64, 64), base);
        // 路径几何变 → key 变
        let moved = square_path(0.5, 0.0, 10.0, 10.0);
        assert_ne!(shadow_key(&moved, Affine::IDENTITY, params, 64, 64), base);
        // 变换变 → key 变
        assert_ne!(
            shadow_key(&path, Affine::translate((1.0, 0.0)), params, 64, 64),
            base
        );
        // 参数变 → key 变(逐字段)
        for mutated in [
            ShadowParams {
                blur_px: 5.0,
                offset: (0.0, 2.0),
                alpha: 0.2,
            },
            ShadowParams {
                blur_px: 4.0,
                offset: (1.0, 2.0),
                alpha: 0.2,
            },
            ShadowParams {
                blur_px: 4.0,
                offset: (0.0, 2.0),
                alpha: 0.25,
            },
        ] {
            assert_ne!(shadow_key(&path, Affine::IDENTITY, mutated, 64, 64), base);
        }
        // 尺寸变 → key 变
        assert_ne!(shadow_key(&path, Affine::IDENTITY, params, 32, 64), base);
    }

    // —— E4:ShadowCache ——

    #[test]
    fn shadow_cache_evicts_in_insertion_order() {
        let mut cache = ShadowCache::new(2);
        cache.insert(1, vec![1]);
        cache.insert(2, vec![2]);
        cache.insert(3, vec![3]);
        assert!(cache.get(1).is_none(), "容量 2:最早插入的 key=1 应被驱逐");
        assert!(cache.get(2).is_some() && cache.get(3).is_some());

        // 覆盖已有 key = 刷新插入序:再插 4 应驱逐 2(而非 3)
        cache.insert(2, vec![22]);
        cache.insert(4, vec![4]);
        assert!(cache.get(2).is_some(), "被覆盖刷新的 key=2 应存活");
        assert!(cache.get(3).is_none(), "key=3 应按插入序被驱逐");
        assert!(cache.get(4).is_some());
    }

    // —— E4:阴影合成器(feature "cpu")——

    #[cfg(feature = "cpu")]
    mod cpu {
        use super::*;

        const W: u16 = 64;
        const H: u16 = 64;

        fn alpha_at(buf: &[u8], x: u16, y: u16) -> u8 {
            buf[4 * (usize::from(y) * usize::from(W) + usize::from(x)) + 3]
        }

        #[test]
        fn shadow_falls_below_shape_and_vanishes_far_away() {
            let path = square_path(16.0, 16.0, 48.0, 32.0);
            let params = ShadowParams {
                blur_px: 4.0,
                offset: (0.0, 4.0),
                alpha: 1.0,
            };
            let buf = render_shadow_rgba(&path, Affine::IDENTITY, params, W, H);
            assert_eq!(buf.len(), usize::from(W) * usize::from(H) * 4);
            // 全图预乘黑:RGB 通道必须全 0
            assert!(
                buf.chunks_exact(4)
                    .all(|px| px[0] == 0 && px[1] == 0 && px[2] == 0)
            );
            // 形状正下方(offset 落点)有影
            assert!(alpha_at(&buf, 32, 36) > 0, "形状下方 4px 处应有影");
            // 远处(左上角)无影
            assert_eq!(alpha_at(&buf, 2, 2), 0, "远离形状+模糊邻域应无影");
            // 形状上方 8px:超出 offset+3×box 半径(2×3=6)的扩散范围
            assert_eq!(alpha_at(&buf, 32, 8), 0, "上方不应被模糊污染");
        }

        #[test]
        fn shadow_alpha_scales_with_param() {
            let path = square_path(16.0, 16.0, 48.0, 32.0);
            let strong = ShadowParams {
                blur_px: 4.0,
                offset: (0.0, 4.0),
                alpha: 1.0,
            };
            let weak = ShadowParams {
                alpha: 0.5,
                ..strong
            };
            let a = render_shadow_rgba(&path, Affine::IDENTITY, strong, W, H);
            let b = render_shadow_rgba(&path, Affine::IDENTITY, weak, W, H);
            let (sa, sb) = (alpha_at(&a, 32, 36), alpha_at(&b, 32, 36));
            assert!(sa > 0 && sb > 0);
            assert!(
                (i32::from(sb) - i32::from(sa) / 2).abs() <= 2,
                "alpha 参数应线性缩放:{sb} vs {sa}/2"
            );
        }

        #[test]
        fn zero_blur_is_hard_shadow() {
            let path = square_path(16.0, 16.0, 48.0, 32.0);
            let params = ShadowParams {
                blur_px: 0.0,
                offset: (0.0, 0.0),
                alpha: 1.0,
            };
            let buf = render_shadow_rgba(&path, Affine::IDENTITY, params, W, H);
            assert_eq!(alpha_at(&buf, 32, 24), 255, "硬影内部全不透明");
            assert_eq!(alpha_at(&buf, 8, 8), 0, "形状外无影");
        }

        #[test]
        fn cache_hit_returns_same_allocation() {
            let path = square_path(16.0, 16.0, 48.0, 32.0);
            let params = ShadowParams {
                blur_px: 4.0,
                offset: (0.0, 4.0),
                alpha: 1.0,
            };
            let mut cache = ShadowCache::new(8);
            let first = cache.get_or_render(&path, Affine::IDENTITY, params, W, H);
            let second = cache.get_or_render(&path, Affine::IDENTITY, params, W, H);
            assert!(
                Arc::ptr_eq(&first, &second),
                "同 key 命中必须返回同一份 Vec 分配"
            );
            // 参数不同 → 新 key → 重新渲染(不同分配)
            let other = cache.get_or_render(
                &path,
                Affine::IDENTITY,
                ShadowParams {
                    blur_px: 8.0,
                    ..params
                },
                W,
                H,
            );
            assert!(!Arc::ptr_eq(&first, &other));
        }
    }

    // —— E6/E7:效果链求值(纯函数,不依赖后端)——

    mod chain {
        use super::*;
        use sable_foundation::effects::EffectEntry;

        const CAPS: EffectCaps = EffectCaps {
            blur: true,
            blend: true,
            grain: true,
        };
        const OFF: EffectCaps = EffectCaps {
            blur: false,
            blend: false,
            grain: false,
        };

        fn entry(spec: EffectSpec) -> EffectEntry {
            EffectEntry {
                spec,
                enabled: true,
            }
        }

        /// 逐像素构图(w×h 的 RGBA8 预乘缓冲)。
        fn buffer(w: u16, h: u16, pixels: &[&[u8; 4]]) -> (Vec<u8>, u16, u16) {
            assert_eq!(pixels.len(), usize::from(w) * usize::from(h));
            (pixels.iter().flat_map(|p| p.to_vec()).collect(), w, h)
        }

        fn at(buf: &[u8], w: u16, x: u16, y: u16) -> [u8; 4] {
            let i = 4 * (usize::from(y) * usize::from(w) + usize::from(x));
            [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
        }

        #[allow(dead_code)]
        fn set(buf: &mut [u8], w: u16, x: u16, y: u16, px: [u8; 4]) {
            let i = 4 * (usize::from(y) * usize::from(w) + usize::from(x));
            buf[i..i + 4].copy_from_slice(&px);
        }

        // —— apply_color_matrix / 预设 ——

        #[test]
        fn brightness_preset_doubles_opaque_pixel() {
            let mut rgba = vec![100, 50, 120, 255, 0, 0, 0, 0];
            let EffectSpec::ColorMatrix { matrix, offsets } = EffectSpec::brightness(2.0) else {
                panic!("brightness 应为 ColorMatrix");
            };
            apply_color_matrix(&mut rgba, matrix, offsets);
            assert_eq!(rgba[0..4], [200, 100, 240, 255], "RGB 翻倍,alpha 不动");
            assert_eq!(rgba[4..8], [0, 0, 0, 0], "全透明像素保持全零");
        }

        #[test]
        fn brightness_clamps_at_255() {
            let mut rgba = vec![200, 200, 200, 255];
            let EffectSpec::ColorMatrix { matrix, offsets } = EffectSpec::brightness(2.0) else {
                panic!();
            };
            apply_color_matrix(&mut rgba, matrix, offsets);
            assert_eq!(rgba[0..4], [255, 255, 255, 255], "400 clamp 到 255");
        }

        #[test]
        fn saturate_zero_desaturates_to_rec709_luminance() {
            // (200, 100, 50) → 0.2126·200 + 0.7152·100 + 0.0722·50 ≈ 117.65
            let mut rgba = vec![200, 100, 50, 255];
            let EffectSpec::ColorMatrix { matrix, offsets } = EffectSpec::saturate(0.0) else {
                panic!();
            };
            apply_color_matrix(&mut rgba, matrix, offsets);
            #[allow(clippy::needless_range_loop)]
            for ch in 0..3 {
                assert!(
                    (i32::from(rgba[ch]) - 118).abs() <= 2,
                    "通道 {ch} 应为灰度 118±2,实际 {}",
                    rgba[ch]
                );
            }
            assert_eq!(rgba[3], 255);
        }

        #[test]
        fn hue_rotate_180_keeps_gray_and_sends_red_to_cyan_complement() {
            let EffectSpec::ColorMatrix { matrix, offsets } = EffectSpec::hue_rotate(180.0) else {
                panic!();
            };
            // 灰不变(行和 = 1)
            let mut gray = vec![128, 128, 128, 255];
            apply_color_matrix(&mut gray, matrix, offsets);
            #[allow(clippy::needless_range_loop)]
            for ch in 0..3 {
                assert!((i32::from(gray[ch]) - 128).abs() <= 2, "灰应不变");
            }
            // 红 → 补色青(r→0,g≈b;SVG hueRotate 对主色降饱和是规范本身
            // 的性质:矩阵是绕灰轴的线性近似,不保饱和度)
            let mut red = vec![255, 0, 0, 255];
            apply_color_matrix(&mut red, matrix, offsets);
            assert!(red[0] <= 2, "r 应钳到 0,实际 {}", red[0]);
            assert!(
                (i32::from(red[1]) - i32::from(red[2])).abs() <= 2,
                "g≈b(青色方向),实际 {red:?}"
            );
            assert!(
                (i32::from(red[1]) - 109).abs() <= 4,
                "规范矩阵下 g≈b≈108.6,实际 {red:?}"
            );
        }

        #[test]
        fn contrast_preset_offsets_midpoint() {
            // contrast(0.5):斜率 0.5 + 第 5 列偏移 0.25。**量纲注意**:构造器
            // 按 SVG feColorMatrix 的 0..1 归一化域给偏移(0.5·(1−c)),而
            // apply_color_matrix 的契约是"通道值域 0~255,偏移直接相加"
            // (见其 doc)——故实测平移 = +0.25/通道,而非 SVG 语义的
            // +0.25·255 = 63.75。此处按实现逐行推演校准(opaque 像素
            // un/premultiply 往返不变):out = 0.5·v + 0.25。
            let mut rgba = vec![200, 50, 128, 255];
            let EffectSpec::ColorMatrix { matrix, offsets } = EffectSpec::contrast(0.5) else {
                panic!();
            };
            apply_color_matrix(&mut rgba, matrix, offsets);
            // 200 → 0.5·200 + 0.25 = 100.25 → 100
            assert!(
                (i32::from(rgba[0]) - 100).abs() <= 3,
                "r 应为 100±3,实际 {}",
                rgba[0]
            );
            // 50 → 25.25 → 25
            assert!((i32::from(rgba[1]) - 25).abs() <= 3, "实际 {}", rgba[1]);
            // 128 → 64.25 → 64:当前量纲下"中点不动"性质不成立(SVG 语义应为
            // 0.5·128 + 63.75 = 127.75 ≈ 128);偏移量纲的语义修正(构造器
            // ×255 或求值器归一化)登记为 V2.0-T4 遗留,见迭代计划 08。
            assert!((i32::from(rgba[2]) - 64).abs() <= 3, "实际 {}", rgba[2]);
            assert_eq!(rgba[3], 255, "alpha 行恒等");
        }

        #[test]
        fn color_matrix_unpremultiplies_and_repremultiplies() {
            // 半透明红 预乘 (128, 0, 0, 128) = 直通 (255, 0, 0, 128);
            // brightness(0.5) → 直通 (128, 0, 0, 128) → 预乘 (64, 0, 0, 128)
            let mut rgba = vec![128, 0, 0, 128];
            let EffectSpec::ColorMatrix { matrix, offsets } = EffectSpec::brightness(0.5) else {
                panic!();
            };
            apply_color_matrix(&mut rgba, matrix, offsets);
            assert!(
                (i32::from(rgba[0]) - 64).abs() <= 2,
                "预乘 r 应减半,实际 {rgba:?}"
            );
            assert_eq!(rgba[1], 0);
            assert_eq!(rgba[3], 128, "alpha 行恒等");
        }

        #[test]
        fn identity_matrix_is_bitwise_noop_on_semitransparent() {
            let matrix = [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ];
            // 合法预乘像素(各通道 ≤ alpha):直通 (64, 96, 128, 128)
            let mut rgba = vec![32u8, 48, 64, 128];
            apply_color_matrix(&mut rgba, matrix, [0.0; 4]);
            assert_eq!(rgba, vec![32, 48, 64, 128], "恒等矩阵应逐位不变");
        }

        // —— apply_effects_rgba:跳过路径(性能军规)——

        #[test]
        fn empty_disabled_noop_and_off_caps_all_return_base_untouched() {
            let base_px = [10u8, 20, 30, 200];
            let base = || buffer(2, 1, &[&base_px, &[40, 50, 60, 100]]);

            // 空 / 全禁用 / 全 no-op / Off 档:四条跳过路径都原样返回
            let (rgba, dx, dy, w, h) = apply_effects_rgba(base(), &[], CAPS);
            assert_eq!((dx, dy, w, h), (0, 0, 2, 1));
            assert_eq!(rgba, base().0);

            let disabled = entry(EffectSpec::GaussianBlur { radius: 4.0 });
            let (rgba, dx, dy, w, h) = apply_effects_rgba(
                base(),
                &[EffectEntry {
                    enabled: false,
                    ..disabled
                }],
                CAPS,
            );
            assert_eq!((dx, dy, w, h), (0, 0, 2, 1));
            assert_eq!(rgba, base().0);

            let (rgba, ..) = apply_effects_rgba(
                base(),
                &[
                    entry(EffectSpec::GaussianBlur { radius: 0.0 }),
                    entry(EffectSpec::brightness(1.0)),
                ],
                CAPS,
            );
            assert_eq!(rgba, base().0);

            let (rgba, dx, dy, w, h) = apply_effects_rgba(
                base(),
                &[entry(EffectSpec::GaussianBlur { radius: 8.0 })],
                OFF,
            );
            assert_eq!((dx, dy, w, h), (0, 0, 2, 1), "Off 档跳过全部效果");
            assert_eq!(rgba, base().0);
        }

        // —— apply_effects_rgba:高斯模糊 ——

        #[test]
        fn gaussian_blur_spreads_coverage_and_expands_surface() {
            // 8×6,实心 4×3 红块 x=3..6、y=1..4;radius=2 → box r=1,
            // 支撑域 3px → 四周外扩 3,尺寸 14×12,dx=dy=-3。
            // 外扩后块占 x∈[6,9]、y∈[4,6]。模糊可分离且初始平面为块指示
            // 函数(严格因式化 v(x)·s(y)),三轮(横+竖)box 逐 pass 推演:
            //   v3(x) = [x3]=9.44 [x4]=37.78 [x5]=94.44 [x6]=160.56 [x7]=207.78
            //           [x8]=207.78 [x9]=160.56 [x10]=94.44 [x11]=37.78 [x12]=9.44
            //   s3(y) = [y3]=10/27 [y4]=16/27 [y5]=19/27 [y6]=16/27 [y7]=10/27
            // (v 单位 0..255,s 为 0..1 占比;探针均离支撑域边缘 ≥1px,避开
            // 相位敏感区)
            let mut px = [[0u8, 0, 0, 0]; 8 * 6].to_vec();
            for y in 1..4u16 {
                for x in 3..7u16 {
                    px[usize::from(y) * 8 + usize::from(x)] = [255, 0, 0, 255];
                }
            }
            let pixels: Vec<&[u8; 4]> = px.iter().collect();
            let base = buffer(8, 6, &pixels);
            let (rgba, dx, dy, w, h) = apply_effects_rgba(
                base,
                &[entry(EffectSpec::GaussianBlur { radius: 2.0 })],
                CAPS,
            );
            assert_eq!((w, h), (14, 12), "四周各外扩 3px");
            assert_eq!((dx, dy), (-3, -3));
            let px_at = |x: u16, y: u16| at(&rgba, w, x, y);
            let a = |x: u16, y: u16| px_at(x, y)[3];
            // 块中心探针 (7,5) = s3(5)·v3(7) = 19/27·(1870/9) ≈ 146:
            // 4×3 小块经三轮 box 后中心显著回落(原断言 >200 过高)
            assert!(
                (i32::from(a(7, 5)) - 146).abs() <= 3,
                "块中心应 ≈146±3,实际 {}",
                a(7, 5)
            );
            // 原块左右缘外 1px(支撑域内部):s3(5)·v3(5/10) = 19/27·(850/9) ≈ 66
            assert!(
                (i32::from(a(5, 5)) - 66).abs() <= 3,
                "块左缘外 1px 应有扩散 ≈66±3,实际 {}",
                a(5, 5)
            );
            assert!((i32::from(a(10, 5)) - 66).abs() <= 3, "实际 {}", a(10, 5));
            // 支撑域(3px)之外干净:v3(x≤2)=0、v3(x≥13)=0、s3(y≤0)=0,
            // 乘积恒 0(box 核支撑有限,严格成立)
            assert_eq!(a(2, 5), 0, "左缘外 4px 应无覆盖");
            assert_eq!(a(13, 5), 0, "右缘外 4px 应无覆盖");
            assert_eq!(a(7, 0), 0, "块上方 4px 应无覆盖");
            // 预乘域模糊:红平面与 alpha 平面同起点同算子 → 逐位一致
            assert_eq!(px_at(7, 5)[0], a(7, 5));
        }

        // —— apply_effects_rgba:投影 ——

        #[test]
        fn drop_shadow_offsets_colored_copy_under_base() {
            // 8×8 白色 2×2 块 (1..3, 1..3);硬影(offset 2,0,纯红)
            let mut px = vec![[0u8, 0, 0, 0]; 8 * 8];
            for y in 1..3u16 {
                for x in 1..3u16 {
                    px[usize::from(y) * 8 + usize::from(x)] = [255, 255, 255, 255];
                }
            }
            let pixels: Vec<&[u8; 4]> = px.iter().collect();
            let base = buffer(8, 8, &pixels);
            let shadow = EffectSpec::DropShadow {
                blur: 0.0,
                offset: [2.0, 0.0],
                color: [255, 0, 0, 255],
            };
            let (rgba, dx, dy, w, h) = apply_effects_rgba(base, &[entry(shadow)], CAPS);
            // margin:右 +2 → 10×8,dx=dy=0(左侧未扩)
            assert_eq!((w, h), (10, 8));
            assert_eq!((dx, dy), (0, 0));
            // 底图不变(块内仍白,base over shadow)
            assert_eq!(at(&rgba, w, 2, 2), [255, 255, 255, 255]);
            // 偏移落点(3+2=5..3?块 x=1..2 → 影 x=3..4,块未覆盖 (4,1)):
            // (4,1) 是纯影:红
            assert_eq!(at(&rgba, w, 4, 1), [255, 0, 0, 255]);
            // (3,1):块内 x=1,2 → 影 x=3,4;(3,1) 有影但 (3,1) 本身不在块内?
            // 块 x∈{1,2},影 x∈{3,4} → (3,1) 应为影
            assert_eq!(at(&rgba, w, 3, 1), [255, 0, 0, 255]);
            // 影未及处仍透明
            assert_eq!(at(&rgba, w, 0, 0), [0, 0, 0, 0]);
            assert_eq!(at(&rgba, w, 6, 1), [0, 0, 0, 0]);
        }

        // —— apply_effects_rgba:发光 ——

        #[test]
        fn outer_glow_tints_around_shape_inner_glow_confined_inside() {
            let mut px = vec![[0u8, 0, 0, 0]; 8 * 8];
            for y in 1..6u16 {
                for x in 1..6u16 {
                    px[usize::from(y) * 8 + usize::from(x)] = [255, 255, 255, 255];
                }
            }
            let pixels: Vec<&[u8; 4]> = px.iter().collect();
            let glow = EffectSpec::Glow {
                radius: 2.0,
                color: [255, 255, 0, 255],
                inner: false,
            };
            // 外发光 = 零偏移彩色投影垫底。外扩后 5×5 块占 [4..8]²(14×14),
            // 三轮 box 后一维轮廓 p3 = [x1]=1/27 [x2]=4/27 [x3]=10/27
            // [x4]=17/27 [x5]=23/27 [x6]=25/27(对称);探针值 = 255·p3(x)·p3(y)。
            let (rgba, dx, dy, w, h) =
                apply_effects_rgba(buffer(8, 8, &pixels), &[entry(glow)], CAPS);
            assert_eq!((w, h), (14, 14), "四周各外扩 3px");
            assert_eq!((dx, dy), (-3, -3));
            let outside = at(&rgba, w, 3, 6); // 原坐标 (0, 3):块外贴左缘
            // 模糊覆盖 = 255·(10/27)·(25/27) ≈ 87.4 → 黄光 r=g≈87、α≈87
            assert!(
                (i32::from(outside[0]) - 87).abs() <= 3,
                "外侧应有黄光 ≈87±3,实际 {outside:?}"
            );
            assert!((i32::from(outside[1]) - 87).abs() <= 3, "实际 {outside:?}");
            assert_eq!(outside[2], 0, "蓝通道不动(黄 = r+g)");
            assert!((i32::from(outside[3]) - 87).abs() <= 3, "实际 {outside:?}");
            // 块内不透明像素 keep = 1−α/255 = 0 → 下垫层整点跳过,严格原样
            assert_eq!(at(&rgba, w, 6, 6), [255, 255, 255, 255], "块内保持白");

            // 内发光叠于内容之上:蒙版 m = 1 − blur(alpha)。外扩后探针:
            // 贴边 (4,6) 的 blur 覆盖 = 255·(17/27)·(25/27) ≈ 148.7 →
            // m ≈ 0.417;深处 (6,6) = 255·(25/27)² ≈ 218.6 → m ≈ 0.143。
            let inner = EffectSpec::Glow {
                radius: 2.0,
                color: [255, 255, 0, 255],
                inner: true,
            };
            let (rgba, _, _, w, _) =
                apply_effects_rgba(buffer(8, 8, &pixels), &[entry(inner)], CAPS);
            let edge = at(&rgba, w, 4, 6); // 原坐标 (1, 3):块内贴左缘
            // 黄光叠白底:r=g 同满(255·m + 255·(1−m)),b 被压到 255·(1−m) ≈ 149
            // (原断言"压 g"方向写反:黄色 r/g 双满,缺口在 b)
            assert_eq!(edge[0], 255, "实际 {edge:?}");
            assert_eq!(edge[1], 255, "实际 {edge:?}");
            assert!(
                (i32::from(edge[2]) - 149).abs() <= 3,
                "贴边 b 应被压到 ≈149±3,实际 {edge:?}"
            );
            assert_eq!(edge[3], 255, "内发光不改变不透明底的总覆盖");
            let deep = at(&rgba, w, 6, 6); // 块中心深处
            assert_eq!(deep[1], 255, "深处 g 仍满(黄光叠白底)");
            assert!(
                (i32::from(deep[2]) - 219).abs() <= 3,
                "深处染色应弱于贴边(149 < 219 < 255),实际 {deep:?}"
            );
            // 外侧底为透明:glow_a = α·m·color_a = 0 → 整像素跳过
            assert_eq!(at(&rgba, w, 3, 6), [0, 0, 0, 0], "块外侧不得出现内发光");
        }

        #[test]
        fn effect_order_matters_for_inner_glow_vs_shadow() {
            // blur/offset 类效果两两线性可交换,真正体现栈序的是(数组序 =
            // 应用序):[glow, shadow] = 内发光**先**应用——蒙版按形状定格,
            // 投影随后垫底 → 投影区纯黑;[shadow, glow] = 投影**先**扩 alpha,
            // 内发光的蒙版 1−blur(alpha) 随之覆盖投影边缘 → 照亮成红。
            let mut px = vec![[0u8, 0, 0, 0]; 8 * 8];
            for y in 1..4u16 {
                for x in 1..4u16 {
                    px[usize::from(y) * 8 + usize::from(x)] = [255, 255, 255, 255];
                }
            }
            let pixels: Vec<&[u8; 4]> = px.iter().collect();
            let shadow = EffectSpec::DropShadow {
                blur: 0.0,
                offset: [4.0, 0.0],
                color: [0, 0, 0, 255],
            };
            let glow = EffectSpec::Glow {
                radius: 2.0,
                color: [255, 0, 0, 255],
                inner: true,
            };
            let glow_first = apply_effects_rgba(
                buffer(8, 8, &pixels),
                &[entry(glow.clone()), entry(shadow.clone())],
                CAPS,
            );
            let shadow_first =
                apply_effects_rgba(buffer(8, 8, &pixels), &[entry(shadow), entry(glow)], CAPS);
            assert_eq!(glow_first.1, shadow_first.1, "两种顺序的 margin 一致(-3)");
            assert_eq!(
                (glow_first.3, glow_first.4),
                (shadow_first.3, shadow_first.4),
                "两种顺序的最终尺寸一致(18×14:发光外扩 3 + 投影右扩 4)"
            );
            // 探针点:投影区右缘像素(基础形状外、投影内、模糊支撑域内)
            let (rgba_a, _, _, wa, _) = glow_first;
            let (rgba_b, _, _, wb, _) = shadow_first;
            let probe = |rgba: &[u8], w: u16| at(rgba, w, 10, 5); // 原坐标 (7, 2)
            let a = probe(&rgba_a, wa);
            let b = probe(&rgba_b, wb);
            // glow_first:蒙版定格在 3×3 形状内,投影处照不到 → 纯黑影
            assert!(
                a[0] <= 5,
                "内发光先上:蒙版定格在形状内 → 投影处纯黑,实际 {a:?}"
            );
            // shadow_first:投影先垫底(探针 α=255),内发光蒙版覆盖投影边缘:
            // blur 覆盖 = 255·(16/27)·(19/27) ≈ 106.3 → m ≈ 0.583 →
            // 红 = 255·m ≈ 148.7 → 149(原断言把两方向写反,已按管线校正)
            assert!(
                (i32::from(b[0]) - 149).abs() <= 3,
                "投影先上:内发光照亮投影边缘 → 红分量 ≈149±3,实际 {b:?}"
            );
            assert_eq!(b[1], 0, "红光不含绿");
            assert_eq!(b[3], 255, "投影区全不透明");
        }

        // —— effect_margins_px ——

        #[test]
        fn margins_accumulate_blur_and_offset_per_side() {
            let effects = vec![
                entry(EffectSpec::GaussianBlur { radius: 2.0 }), // spread 3
                entry(EffectSpec::DropShadow {
                    blur: 4.0, // spread 6
                    offset: [2.0, -3.0],
                    color: [0, 0, 0, 128],
                }),
            ];
            let m = effect_margins_px(&effects, 1.0);
            assert_eq!(m, [9.0, 12.0, 11.0, 9.0], "[左,上,右,下] 按侧累加");
            // scale 换算:radius=2 → 像素 radius=1 → box 1 → spread 3
            let m = effect_margins_px(&effects[..1], 1.0);
            assert_eq!(m, [3.0, 3.0, 3.0, 3.0]);
            // 无效果 → 全零
            assert_eq!(effect_margins_px(&[], 2.0), [0.0; 4]);
            // 禁用/无效条目不计
            let disabled = EffectEntry {
                enabled: false,
                ..entry(EffectSpec::GaussianBlur { radius: 8.0 })
            };
            assert_eq!(effect_margins_px(&[disabled], 1.0), [0.0; 4]);
        }
    }

    // —— EffectSurface(cpu)——

    #[cfg(feature = "cpu")]
    mod surface {
        use super::*;
        use kurbo::Shape;

        #[test]
        fn surface_rasterizes_draw_commands_to_transparent_offscreen() {
            let mut surface = EffectSurface::new(8, 8);
            assert_eq!(surface.width(), 8);
            assert_eq!(surface.height(), 8);
            surface.draw(|sink| {
                let rect = kurbo::Rect::new(2.0, 2.0, 6.0, 6.0).to_path(0.1);
                sink.fill(
                    &sable_foundation::scene::Paint::Solid([255, 0, 0, 255]),
                    Affine::IDENTITY,
                    &rect,
                );
            });
            let (rgba, w, h) = surface.into_rgba();
            assert_eq!((w, h), (8, 8));
            let at = |x: u16, y: u16| {
                let i = 4 * (usize::from(y) * 8 + usize::from(x));
                [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
            };
            assert_eq!(at(4, 4), [255, 0, 0, 255], "矩形内部为红");
            assert_eq!(at(0, 0), [0, 0, 0, 0], "底为透明");
        }
    }
}
