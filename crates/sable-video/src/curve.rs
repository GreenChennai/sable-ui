//! 关键帧求值(docs/04 §9):区间插值 + 预设缓动。
//!
//! 播放器每帧调用 [`evaluate`] 求动画值;缓动公式集中在 [`Easing::apply`]。

use crate::model::{Easing, Keyframe};

/// 关键帧求值(纯函数):
///
/// - 空表 → `None`;
/// - `t_ms` 落在首帧之前/末帧之后 → 钳制为首/末帧的值;
/// - 单关键帧 → 恒返回该值;
/// - 区间内 → 用**左关键帧的 easing** 作用于区间进度 u,再对两端值线性插值。
///
/// 约定:`keyframes` 按 `t_ms` 升序(未排序时结果未定义;二分查找假设有序)。
pub fn evaluate(keyframes: &[Keyframe], t_ms: u64) -> Option<f64> {
    let first = keyframes.first()?;
    let last = keyframes.last()?;
    if t_ms <= first.t_ms {
        return Some(first.value);
    }
    if t_ms >= last.t_ms {
        return Some(last.value);
    }
    // 上式保证 first.t < t < last.t,故 split ∈ [1, len-1]
    let split = keyframes.partition_point(|k| k.t_ms <= t_ms);
    let k0 = keyframes.get(split.wrapping_sub(1));
    let k1 = keyframes.get(split);
    let (Some(k0), Some(k1)) = (k0, k1) else {
        return Some(last.value); // 理论不可达,防御性钳制
    };
    if k1.t_ms <= k0.t_ms {
        return Some(k1.value); // 零宽段(重复时间戳):按跳变处理
    }
    let u = (t_ms - k0.t_ms) as f64 / (k1.t_ms - k0.t_ms) as f64;
    let u = k0.easing.apply(u.clamp(0.0, 1.0));
    Some(k0.value + (k1.value - k0.value) * u)
}

impl Easing {
    /// 作用于归一化区间进度 u ∈ [0,1](超出区间的值不保证有意义)。
    ///
    /// - cubic 三种为标准公式;
    /// - `Spring` 为欠阻尼弹簧的解析近似 `1 - e^(-6u)·cos(10u)`,
    ///   参数(ω=10、衰减率 6)按观感调出:起点 0、1s 处≈收敛、带一次
    ///   过冲回摆——非物理精确,docs/04 §9 只要求"弹簧观感"。
    pub fn apply(&self, u: f64) -> f64 {
        match self {
            Easing::Linear => u,
            Easing::InCubic => u * u * u,
            Easing::OutCubic => {
                let v = 1.0 - u;
                1.0 - v * v * v
            }
            Easing::InOutCubic => {
                if u < 0.5 {
                    4.0 * u * u * u
                } else {
                    let v = -2.0 * u + 2.0;
                    1.0 - v * v * v / 2.0
                }
            }
            Easing::Spring => 1.0 - (-6.0 * u).exp() * (10.0 * u).cos(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(t_ms: u64, value: f64, easing: Easing) -> Keyframe {
        Keyframe {
            t_ms,
            value,
            easing,
        }
    }

    #[test]
    fn evaluate_empty_returns_none() {
        assert_eq!(evaluate(&[], 0), None);
        assert_eq!(evaluate(&[], 9999), None);
    }

    #[test]
    fn evaluate_single_key_clamps_everywhere() {
        let keys = [key(500, 7.5, Easing::InCubic)];
        assert_eq!(evaluate(&keys, 0), Some(7.5));
        assert_eq!(evaluate(&keys, 500), Some(7.5));
        assert_eq!(evaluate(&keys, 100_000), Some(7.5));
    }

    #[test]
    fn evaluate_two_keys_linear_and_clamps() {
        let keys = [
            key(0, 10.0, Easing::Linear),
            key(1000, 20.0, Easing::Linear),
        ];
        assert_eq!(evaluate(&keys, 0), Some(10.0), "首帧值");
        assert_eq!(evaluate(&keys, 250), Some(12.5));
        assert_eq!(evaluate(&keys, 750), Some(17.5));
        assert_eq!(evaluate(&keys, 1000), Some(20.0));
        assert_eq!(evaluate(&keys, 2000), Some(20.0), "末帧外钳制");
        assert_eq!(evaluate(&keys, 999_999), Some(20.0));
    }

    #[test]
    fn evaluate_uses_left_key_easing() {
        // 区间 [0,100) 的缓动由左关键帧(InCubic)决定:u=0.5 → 0.125
        let keys = [
            key(0, 0.0, Easing::InCubic),
            key(100, 100.0, Easing::Linear),
        ];
        assert_eq!(evaluate(&keys, 50), Some(12.5));
        assert_eq!(evaluate(&keys, 25), Some(100.0 * 0.25_f64.powi(3)));
    }

    #[test]
    fn evaluate_zero_width_segment_is_jump() {
        // 重复时间戳:50ms 处从 1 跳到 9,之后按 (50,9)-(100,2) 插值
        let keys = [
            key(0, 1.0, Easing::Linear),
            key(50, 9.0, Easing::Linear),
            key(50, 9.0, Easing::Linear),
            key(100, 2.0, Easing::Linear),
        ];
        assert_eq!(evaluate(&keys, 50), Some(9.0));
        assert_eq!(evaluate(&keys, 75), Some(5.5));
        assert_eq!(evaluate(&keys, 25), Some(5.0), "跳变前按 (0,1)-(50,9) 插值");
    }

    #[test]
    fn cubic_easings_are_monotonic_with_unit_endpoints() {
        let easings = [
            Easing::Linear,
            Easing::InCubic,
            Easing::OutCubic,
            Easing::InOutCubic,
        ];
        for e in easings {
            let f = |u: f64| e.apply(u);
            assert_eq!(f(0.0), 0.0, "{e:?}: 端点 0");
            assert_eq!(f(1.0), 1.0, "{e:?}: 端点 1");
            let mut prev = f(0.0);
            for step in 1..=100 {
                let u = f64::from(step) / 100.0;
                let v = f(u);
                assert!(v >= prev, "{e:?}: 在 u={u} 处单调递减");
                prev = v;
            }
        }
    }

    #[test]
    fn spring_approximates_zero_to_one_with_overshoot() {
        let e = Easing::Spring;
        assert_eq!(e.apply(0.0), 0.0, "起点精确为 0");
        assert!((e.apply(1.0) - 1.0).abs() < 0.05, "终点≈1(容差 0.05)");
        let mut max = f64::MIN;
        let mut min = f64::MAX;
        for step in 0..=100 {
            let v = e.apply(f64::from(step) / 100.0);
            max = max.max(v);
            min = min.min(v);
        }
        assert!(max > 1.01, "弹簧必有过冲(峰 {max})");
        assert!(min < 1.0, "过冲后回摆穿回 1 以下(谷 {min})");
    }

    #[test]
    fn evaluate_across_many_segments_stays_in_range() {
        let keys = [
            key(0, 0.0, Easing::Linear),
            key(100, 10.0, Easing::InCubic),
            key(200, -10.0, Easing::OutCubic),
            key(300, 5.0, Easing::InOutCubic),
            key(400, 5.0, Easing::Spring),
        ];
        for t in (0..=400).step_by(10) {
            let v = evaluate(&keys, t).expect("非空表必有值");
            assert!(
                (-10.0 - 1e-9..=10.0 + 1e-9).contains(&v),
                "t={t} 值 {v} 越界(Spring 过冲除外段)"
            );
        }
        assert_eq!(evaluate(&keys, 400), Some(5.0));
    }
}
