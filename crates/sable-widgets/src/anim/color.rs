//! A6 颜色插值器:Hsla 域的最短弧色相 + alpha 预乘(08 迭代计划 A6)。
//!
//! # 两个语义要点(主题切换 200ms 全程无脏色的保证)
//!
//! 1. **色相最短弧**:350°→10° 走 +20° 而非 -340°(h ∈ [0,1] 周期域上取
//!    |Δh| ≤ 0.5 的方向);
//! 2. **alpha 预乘防中间态发灰**:插值在预乘域进行——s/l 先乘各自的 alpha,
//!    混合后再按混合 alpha 还原(`s = Σ(sᵢ·aᵢ·wᵢ)/a_mix`)。纯 lerp 在两端
//!    alpha 不同时会得到"被透明端稀释"的中间饱和度/亮度(视觉发灰);
//!    预乘域插值保证**可见贡献随 alpha 连续缩放**,淡出中的颜色保持本色。
//!
//! 与 [`crate::tokens::lerp_rgba8`](0~255 字节域)语义对齐:t 越界钳制、
//! 端点还原;tokens.rs 保持不动,本模块为 Hsla(组件渲染主域)版本。
//! [`Lerp for Hsla`](crate::anim::Lerp) 委托到本实现,主题插值自动受益。

use gpui::Hsla;

/// 两个 Hsla 的动画插值(A6):色相最短弧 + 预乘域 s/l + 直通域 alpha。
///
/// `t` 越界钳到 [0,1](对齐 [`crate::tokens::lerp_rgba8`]);**端点精确还原**
/// `t=0` → a、`t=1` → b——透明端的 s/l 在预乘域退化(×0 后不可还原),必须
/// 直通,动画终帧才能精确停在目标色。NaN t 视为 0;输入颜色应为合法 Hsla。
pub fn lerp_hsla(a: Hsla, b: Hsla, t: f64) -> Hsla {
    if t.is_nan() {
        return a;
    }
    let t = t.clamp(0.0, 1.0);
    if t >= 1.0 {
        return b;
    }
    if t <= 0.0 {
        return a;
    }
    let t = t as f32;
    // 色相最短弧:Δh 折到 (-0.5, 0.5](|Δh| 恰 0.5 时两向等长,取原方向)
    let dh = b.h - a.h;
    let dh = if dh.abs() > 0.5 { dh - dh.signum() } else { dh };
    let mix = |x: f32, y: f32| x + (y - x) * t;
    let alpha = mix(a.a, b.a);
    // 预乘域:s/l 各乘自身 alpha 再混合,除回混合 alpha(直通域还原)
    let s_premul = mix(a.s * a.a, b.s * b.a);
    let l_premul = mix(a.l * a.a, b.l * b.a);
    let (s, l) = if alpha.abs() < f32::EPSILON {
        (0.0, 0.0) // 全透明:RGB 无意义,归零防 0/0
    } else {
        (s_premul / alpha, l_premul / alpha)
    };
    Hsla {
        h: (a.h + dh * t).rem_euclid(1.0),
        s,
        l,
        a: alpha,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hsla(h: f32, s: f32, l: f32, a: f32) -> Hsla {
        Hsla { h, s, l, a }
    }

    #[test]
    fn hue_takes_shortest_arc_350_to_10() {
        // 350°→10° 走 +20°:中间值 355°(t=.25)→ 0°(t=.5)→ 5°(t=.75),单调
        let a = hsla(350.0 / 360.0, 1.0, 0.5, 1.0);
        let b = hsla(10.0 / 360.0, 1.0, 0.5, 1.0);
        let steps = [
            (0.25, 355.0 / 360.0),
            (0.5, 0.0),
            (0.75, 5.0 / 360.0),
            (1.0, 10.0 / 360.0),
        ];
        for (t, expect) in steps {
            let mid = lerp_hsla(a, b, t);
            assert!(
                (mid.h - expect).abs() < 1e-5,
                "t={t}: 期望 {expect}(turn)实际 {}",
                mid.h
            );
        }
        // 反方向 10°→350° 走 -20°,中点同为 0°(圆上等价类:rem_euclid
        // 后可能落在 0 或 1-ε,按圆距断言)
        let rev = lerp_hsla(b, a, 0.5);
        let d = rev.h.rem_euclid(1.0);
        let circular_dist = d.min(1.0 - d);
        assert!(
            circular_dist < 1e-5,
            "反向中点也在 0°,得到 {}(圆距 {circular_dist})",
            rev.h
        );
        // 端点还原
        assert_eq!(lerp_hsla(a, b, 0.0), a);
        assert_eq!(lerp_hsla(a, b, 1.0), b);
        // t 越界钳制
        assert_eq!(lerp_hsla(a, b, -2.0), a);
        assert_eq!(lerp_hsla(a, b, 2.0), b);
    }

    #[test]
    fn premultiplied_alpha_keeps_fading_color_true() {
        // 不透明红 → 透明白:直通 lerp 会得半灰粉(发灰);预乘域插值
        // 中间态保持纯红本色(h/s/l 不变),只有 alpha 在降
        let red = hsla(0.0, 1.0, 0.5, 1.0);
        let clear_white = hsla(0.0, 0.0, 1.0, 0.0);
        for t in [0.25, 0.5, 0.75] {
            let mid = lerp_hsla(red, clear_white, t);
            assert!((mid.h - 0.0).abs() < 1e-6, "t={t} 色相漂移");
            assert!(
                (mid.s - 1.0).abs() < 1e-6,
                "t={t} 饱和度被稀释:{}(发灰)",
                mid.s
            );
            assert!((mid.l - 0.5).abs() < 1e-6, "t={t} 亮度被稀释:{}", mid.l);
            assert!((mid.a - (1.0 - t as f32)).abs() < 1e-6);
        }
    }

    #[test]
    fn alpha_never_exceeds_endpoint_max() {
        // 两透明色插值:任意 t 的 alpha ≤ max(a1, a2) + ε(线性组合上界)
        let a = hsla(0.9, 0.8, 0.3, 0.2);
        let b = hsla(0.1, 0.6, 0.7, 0.8);
        for i in 0..=20 {
            let t = f64::from(i) / 20.0;
            let mid = lerp_hsla(a, b, t);
            assert!(
                mid.a <= 0.8 + 1e-6,
                "t={t}: alpha {} 超过端点上界 0.8",
                mid.a
            );
            assert!(mid.a >= 0.2 - 1e-6);
        }
    }

    #[test]
    fn opaque_endpoints_match_straight_lerp() {
        // 两端不透明时预乘退化回普通插值(s/l 逐点一致)
        let a = hsla(0.3, 0.8, 0.4, 1.0);
        let b = hsla(0.6, 0.2, 0.9, 1.0);
        let mid = lerp_hsla(a, b, 0.5);
        assert!((mid.s - 0.5).abs() < 1e-6);
        assert!((mid.l - 0.65).abs() < 1e-6);
        assert!((mid.a - 1.0).abs() < 1e-6);
        // 色相 0.3→0.6 直接 +0.3(不跨 0/1 边界,无最短弧翻转)
        assert!((mid.h - 0.45).abs() < 1e-6);
    }
}
