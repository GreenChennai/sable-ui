//! 效果原语:阴影合成器 + 静态阴影缓存(E4)、效果降级矩阵(E12)。
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
//! env 解析抽成纯函数 [`detect_with`] 供测试;[`detect`] 是薄包装
//! (读 env,不做别的,不测)。零 unsafe:env 读取用 `std::env::var`。

use std::collections::HashMap;
use std::collections::VecDeque;
use std::hash::Hasher;
use std::sync::Arc;

use kurbo::{Affine, BezPath, PathEl, Point};

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
}

impl ShadowCache {
    /// 指定容量上限(最小 1;容量 0 无意义)。
    pub fn new(capacity: usize) -> Self {
        ShadowCache {
            capacity: capacity.max(1),
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// 命中返回缓存的 `Arc`(与存入的是同一份分配),未命中 `None`。
    pub fn get(&self, key: u64) -> Option<Arc<Vec<u8>>> {
        self.entries.get(&key).cloned()
    }

    /// 插入(或覆盖同 key 并把它刷新为最新)。
    pub fn insert(&mut self, key: u64, pixels: Vec<u8>) {
        if self.entries.contains_key(&key) {
            // 覆盖语义:刷新插入序,替换内容
            self.order.retain(|&k| k != key);
            self.order.push_back(key);
            self.entries.insert(key, Arc::new(pixels));
            return;
        }
        if self.entries.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
        self.order.push_back(key);
        self.entries.insert(key, Arc::new(pixels));
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
        self.insert(key, pixels);
        // 刚插入必在表中(容量 ≥ 1 已在上文保证驱逐后仍留有空间)
        self.get(key).expect("insert 后同 key 必命中")
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
/// `horizontal=true` 沿 x,否则沿 y。朴素 O(n·r):阴影缓冲小 + 结果进
/// [`ShadowCache`] 静止零成本,清晰性优先(窗口滑动优化留 S3 真机集成)。
#[cfg(feature = "cpu")]
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
}
