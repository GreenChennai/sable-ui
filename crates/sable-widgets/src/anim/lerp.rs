//! A10 可插值类型全集(08 迭代计划 A10):标量 / Hsla / kurbo 几何 / 仿射变换。
//!
//! [`Lerp`] 是动画引擎对值的唯一要求(分册六 §4.2 `Animated<T: Lerp>`)。
//! kurbo 几何逐分量插值;**Transform(kurbo [`Affine`])走分解插值**:
//! `as_coeffs()` 得 2×3 矩阵 → QR 式分解为 平移·旋转·缩放(列向量约定,
//! 线性部分 L = [[a,c],[b,d]]:sx = ‖col1‖、θ = atan2(b,a)、
//! sy = det/sx、k = (a·c+b·d)/(sx·sy)),四要素分别 lerp(旋转走最短弧)
//! 后按 `T·R·Shear_x(k)·S` 合成——直接 lerp 矩阵系数会在旋转端点间塌缩
//! (缩放中途变负/形变),分解插值保证位移/旋转/缩放路径各自平滑。
//! 任一侧退化(第一列零向量、零面积、非有限)时回退 6 系数逐分量插值。
//! **镜像插值契约**:行列式变号(+1 ↔ -1)的插值必经零面积中点(GL(2,R)
//! 两个连通分量,连续路径绕不开 det=0);本实现把镜像承载在 sy 符号上,
//! 表现为标准"压扁-翻面",端点不受影响。

use gpui::Hsla;
use kurbo::{Affine, Point, Rect, Size, Vec2};

use super::color::lerp_hsla;

/// 可插值类型(Lerp):动画引擎对值的唯一要求(分册六 §4.2)。
pub trait Lerp: Clone {
    /// 线性插值:`self` 与 `other` 之间比例 `t ∈ [0,1]` 处的值。
    fn lerp(&self, other: &Self, t: f64) -> Self;
}

impl Lerp for f32 {
    fn lerp(&self, other: &Self, t: f64) -> Self {
        self + (other - self) * t as f32
    }
}

impl Lerp for f64 {
    fn lerp(&self, other: &Self, t: f64) -> Self {
        self + (other - self) * t
    }
}

impl Lerp for Hsla {
    fn lerp(&self, other: &Self, t: f64) -> Self {
        // A6 唯一实现:色相最短弧 + 预乘域(见 color 模块文档)
        lerp_hsla(*self, *other, t)
    }
}

impl Lerp for Point {
    fn lerp(&self, other: &Self, t: f64) -> Self {
        Point::new(self.x.lerp(&other.x, t), self.y.lerp(&other.y, t))
    }
}

impl Lerp for Vec2 {
    fn lerp(&self, other: &Self, t: f64) -> Self {
        Vec2::new(self.x.lerp(&other.x, t), self.y.lerp(&other.y, t))
    }
}

impl Lerp for Size {
    fn lerp(&self, other: &Self, t: f64) -> Self {
        Size::new(
            self.width.lerp(&other.width, t),
            self.height.lerp(&other.height, t),
        )
    }
}

impl Lerp for Rect {
    fn lerp(&self, other: &Self, t: f64) -> Self {
        Rect::new(
            self.x0.lerp(&other.x0, t),
            self.y0.lerp(&other.y0, t),
            self.x1.lerp(&other.x1, t),
            self.y1.lerp(&other.y1, t),
        )
    }
}

/// 仿射分解部件:`T·R·Shear_x(k)·S`(平移/旋转/斜切/缩放,列向量约定)。
#[derive(Clone, Copy, Debug)]
struct AffineParts {
    tx: f64,
    ty: f64,
    rotation: f64,
    shear_x: f64,
    scale_x: f64,
    scale_y: f64,
}

/// 分解阈值:缩放分量低于它视为退化(无法定义旋转/斜切)。
const DECOMPOSE_EPS: f64 = 1e-12;

/// `Affine` → [`AffineParts`](2×3 矩阵的 QR 式分解;见模块文档公式)。
/// 退化/非有限返回 `None`(调用方回退系数插值)。
fn decompose_affine(m: Affine) -> Option<AffineParts> {
    if !m.is_finite() {
        return None;
    }
    let [a, b, c, d, e, f] = m.as_coeffs();
    // 列向量约定:线性部分 L = [[a, c], [b, d]],col1 = (a, b) 是旋转后的 x 轴
    let sx = (a * a + b * b).sqrt();
    if sx < DECOMPOSE_EPS {
        return None; // 第一列零向量:旋转未定义
    }
    let det = a * d - b * c;
    let sy = det / sx;
    if !sy.is_finite() || sy.abs() < DECOMPOSE_EPS {
        return None; // 零面积:斜切未定义
    }
    let shear_x = (a * c + b * d) / (sx * sy);
    if !shear_x.is_finite() {
        return None;
    }
    Some(AffineParts {
        tx: e,
        ty: f,
        rotation: b.atan2(a),
        shear_x,
        scale_x: sx,
        scale_y: sy,
    })
}

/// [`AffineParts`] → `Affine`:按 `T·R·Shear_x(k)·S` 合成(kurbo 乘法为
/// 列向量约定,右因子先作用)。镜像(det < 0)由 sy < 0 承载:插值跨越
/// 镜像时 sy 线性穿过 0,中点为零面积(标准"压扁-翻面"动画),端点精确
/// 还原;塌缩轴随被翻转轴(契约见 `affine_reflection_interpolates_through_degenerate_midpoint` 测试)。
fn recompose_affine(p: AffineParts) -> Affine {
    let shear = Affine::new([1.0, 0.0, p.shear_x, 1.0, 0.0, 0.0]);
    Affine::translate((p.tx, p.ty))
        * Affine::rotate(p.rotation)
        * shear
        * Affine::scale_non_uniform(p.scale_x, p.scale_y)
}

impl Lerp for Affine {
    fn lerp(&self, other: &Self, t: f64) -> Self {
        match (decompose_affine(*self), decompose_affine(*other)) {
            (Some(x), Some(y)) => {
                let mix = |a: f64, b: f64| a + (b - a) * t;
                // 旋转走最短弧(与 Hsla 色相同款折返)
                let mut dth = y.rotation - x.rotation;
                if dth.abs() > std::f64::consts::PI {
                    dth -= dth.signum() * std::f64::consts::TAU;
                }
                recompose_affine(AffineParts {
                    tx: mix(x.tx, y.tx),
                    ty: mix(x.ty, y.ty),
                    rotation: x.rotation + dth * t,
                    shear_x: mix(x.shear_x, y.shear_x),
                    scale_x: mix(x.scale_x, y.scale_x),
                    scale_y: mix(x.scale_y, y.scale_y),
                })
            }
            // 任一侧退化(零缩放/非有限):6 系数逐分量插值兜底
            _ => {
                let (a, b) = (self.as_coeffs(), other.as_coeffs());
                Affine::new(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    fn assert_coeffs_close(m: Affine, expect: [f64; 6], what: &str) {
        for (i, (g, e)) in m.as_coeffs().iter().zip(expect.iter()).enumerate() {
            assert!((g - e).abs() < 1e-9, "{what} 系数 {i}:实际 {g} 期望 {e}");
        }
    }

    #[test]
    fn scalars_interpolate() {
        assert_eq!(0.0_f64.lerp(&10.0, 0.25), 2.5);
        assert_eq!(0.0_f32.lerp(&10.0, 0.5), 5.0);
    }

    #[test]
    fn hsla_lerp_shortest_arc_via_color_module() {
        // 委托 lerp_hsla:0.9 → 0.1 中点在 0.0(最短弧),饱和度中点 0.5
        let late = Hsla {
            h: 0.9,
            s: 0.0,
            l: 0.5,
            a: 1.0,
        };
        let early = Hsla {
            h: 0.1,
            s: 0.0,
            l: 0.5,
            a: 1.0,
        };
        assert!(
            (late.lerp(&early, 0.5).h - 0.0).abs() < 1e-6,
            "色相走最短弧"
        );
        let a = Hsla {
            h: 0.0,
            s: 0.0,
            l: 0.0,
            a: 1.0,
        };
        let b = Hsla {
            h: 0.2,
            s: 1.0,
            l: 1.0,
            a: 0.0,
        };
        // 预乘域:透明端贡献为 0 → 中间态保持不透明端本色(黑,s=0),
        // 不被透明端的 s=1/l=1 稀释(直通 lerp 会得 s=0.5 的脏色)
        assert!(
            (a.lerp(&b, 0.5).s - 0.0).abs() < 1e-6,
            "预乘域不被透明端稀释"
        );
    }

    #[test]
    fn geometry_types_lerp_componentwise() {
        let p0 = Point::new(0.0, 10.0);
        let p1 = Point::new(100.0, 110.0);
        // kurbo 对 Point/Vec2 有同名固有 lerp(按值取 other)会遮蔽 trait 方法,
        // 统一用全限定调用走本库 Lerp 语义(与其他几何类型一致)
        let mid = Lerp::lerp(&p0, &p1, 0.5);
        assert!((mid.x - 50.0).abs() < 1e-12 && (mid.y - 60.0).abs() < 1e-12);

        let v0 = Vec2::new(0.0, 0.0);
        let v = Lerp::lerp(&v0, &Vec2::new(8.0, -4.0), 0.25);
        assert!((v.x - 2.0).abs() < 1e-12 && (v.y - (-1.0)).abs() < 1e-12);

        let s = Size::new(100.0, 200.0).lerp(&Size::new(200.0, 400.0), 0.5);
        assert!((s.width - 150.0).abs() < 1e-12 && (s.height - 300.0).abs() < 1e-12);

        let r = Rect::new(0.0, 0.0, 100.0, 100.0).lerp(&Rect::new(10.0, 20.0, 30.0, 40.0), 0.5);
        assert_eq!(r, Rect::new(5.0, 10.0, 65.0, 70.0));

        // 端点还原(逐分量精确)
        assert_eq!(Lerp::lerp(&p0, &p1, 0.0), p0);
        assert_eq!(Lerp::lerp(&p0, &p1, 1.0), p1);
        assert_eq!(
            Rect::new(1.0, 2.0, 3.0, 4.0).lerp(&Rect::new(5.0, 6.0, 7.0, 8.0), 1.0),
            Rect::new(5.0, 6.0, 7.0, 8.0)
        );
    }

    #[test]
    fn affine_roundtrip_at_endpoints() {
        // A10 验收:各类型 roundtrip —— 分解插值的 t=0/t=1 必须还原端点
        let cases = [
            Affine::IDENTITY,
            Affine::translate((100.0, 50.0)),
            Affine::rotate(0.7),
            Affine::scale_non_uniform(2.0, 3.0),
            Affine::new([1.0, 0.0, 0.35, 1.0, 7.0, -2.0]), // 纯斜切
            Affine::translate((10.0, 20.0))
                * Affine::rotate(1.2)
                * Affine::scale_non_uniform(1.5, -2.0), // 含镜像
        ];
        for m in cases {
            assert_coeffs_close(m.lerp(&m, 0.0), m.as_coeffs(), "t=0 还原");
            assert_coeffs_close(m.lerp(&m, 1.0), m.as_coeffs(), "t=1 还原");
        }
    }

    #[test]
    fn affine_trs_midpoint_interpolates_each_factor() {
        // T(0)·R(0)·S(1) → T(100,50)·R(90°)·S(2,3);t=0.5 应为
        // T(50,25)·R(45°)·S(1.5,2):点 (1,0) 映到 (50+1.5cos45, 25+1.5sin45)
        let a = Affine::IDENTITY;
        let b = Affine::translate((100.0, 50.0))
            * Affine::rotate(FRAC_PI_2)
            * Affine::scale_non_uniform(2.0, 3.0);
        let mid = a.lerp(&b, 0.5);
        let mapped = mid * Point::new(1.0, 0.0);
        let c = (FRAC_PI_2 / 2.0).cos(); // 中点旋转 = 45°
        let s = (FRAC_PI_2 / 2.0).sin();
        assert!(
            (mapped.x - (50.0 + 1.5 * c)).abs() < 1e-9
                && (mapped.y - (25.0 + 1.5 * s)).abs() < 1e-9,
            "TRS 中点:{mapped:?}"
        );
    }

    #[test]
    fn affine_rotation_takes_shortest_arc() {
        // -170° → 170° 走 +20°(经 180°),中点 = 旋转 180°:点 (1,0) → (-1, 0)
        let a = Affine::rotate(-170.0_f64.to_radians());
        let b = Affine::rotate(170.0_f64.to_radians());
        let mid = a.lerp(&b, 0.5);
        let mapped = mid * Point::new(1.0, 0.0);
        assert!(
            (mapped.x - (-1.0)).abs() < 1e-9 && (mapped.y - 0.0).abs() < 1e-9,
            "旋转中点应过 180°,得到 {mapped:?}"
        );
    }

    #[test]
    fn affine_reflection_interpolates_through_degenerate_midpoint() {
        // 镜像插值契约:GL(2,R) 按行列式符号分两个连通分量,det 从 +1 连续
        // 变到 -1 必经 det=0(零面积)——中点塌缩在拓扑上不可避免,自由度
        // 只在塌缩落在哪根轴。本实现把镜像承载在 sy 的符号(sy = det/sx),
        // y 镜像插值因此表现为标准"压扁-翻面":|y| 沿 1 → 0.5 → 0 → 0.5 → 1
        let a = Affine::scale_non_uniform(1.0, 1.0);
        let b = Affine::scale_non_uniform(1.0, -1.0);
        // 端点精确还原
        assert_coeffs_close(a.lerp(&b, 0.0), a.as_coeffs(), "t=0 还原");
        assert_coeffs_close(a.lerp(&b, 1.0), b.as_coeffs(), "t=1 还原");
        // 退化路径:逐 t 检查 (0,1) 的像(sy 线性穿过 0)
        let expect = [(0.25, 0.5), (0.5, 0.0), (0.75, -0.5)];
        for (t, y) in expect {
            let mapped = a.lerp(&b, t) * Point::new(0.0, 1.0);
            assert!(
                (mapped.x - 0.0).abs() < 1e-9 && (mapped.y - y).abs() < 1e-9,
                "t={t}: 期望 (0, {y}) 实际 {mapped:?}"
            );
        }
        // 塌缩只发生在被翻转的轴,x 轴全程不受影响
        let quarter = a.lerp(&b, 0.25) * Point::new(1.0, 0.0);
        assert!(
            (quarter.x - 1.0).abs() < 1e-9 && (quarter.y - 0.0).abs() < 1e-9,
            "x 轴不受镜像插值影响:{quarter:?}"
        );
    }

    #[test]
    fn affine_degenerate_side_falls_back_to_coeffs() {
        // 零缩放(第一列零向量):不 panic,回退系数插值
        let zero = Affine::scale(0.0);
        let mid = zero.lerp(&Affine::IDENTITY, 0.5);
        assert_coeffs_close(mid, [0.5, 0.0, 0.0, 0.5, 0.0, 0.0], "零缩放兜底");
        // 非有限:同样兜底(NaN 系数按 IEEE 传播,不 panic 即可)
        let nan = Affine::new([f64::NAN; 6]);
        let _ = nan.lerp(&Affine::IDENTITY, 0.5);
    }
}
