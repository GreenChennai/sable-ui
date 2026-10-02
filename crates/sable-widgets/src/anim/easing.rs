//! 缓动函数(CSS 同款语义;A12 黄金值快照在此)。
//!
//! 与分册六 §4.2 原文的适配(M0 review 通过,语义保持):从「`fn easing(name)
//! -> impl Fn` + kurbo 控制点」改为自有枚举 + 解析求值——CSS cubic-bezier 用
//! 牛顿迭代解 `x(s) = t` 再取 `y(s)`(标准做法,见 W3C CSS Easing Functions
//! Level 1 的求解约定),不经 kurbo 三次贝塞尔的隐式参数化。

/// 缓动函数(CSS 同款语义)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Easing {
    /// 线性
    Linear,
    /// 缓入 u³
    InCubic,
    /// 缓出 1-(1-u)³
    OutCubic,
    /// 缓入缓出
    InOutCubic,
    /// CSS `cubic-bezier(x1,y1,x2,y2)`:控制点 x ∈ [0,1],y 不限
    /// (y 超出 [0,1] 即"回弹"曲线,如 ease-back)。
    CubicBezier {
        /// 控制点 1 的 x
        x1: f64,
        /// 控制点 1 的 y
        y1: f64,
        /// 控制点 2 的 x
        x2: f64,
        /// 控制点 2 的 y
        y2: f64,
    },
}

impl Easing {
    /// 求值归一化进度 u ∈ [0,1] → 输出(通常 ∈ [0,1];CubicBezier 的 y 可越界)。
    pub fn apply(&self, u: f64) -> f64 {
        let u = u.clamp(0.0, 1.0);
        match *self {
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
            Easing::CubicBezier { x1, y1, x2, y2 } => cubic_bezier_y(x1, y1, x2, y2, u),
        }
    }
}

/// 三次贝塞尔曲线 y 值:给定进度 x ∈ [0,1],先解 `bezier_x(s) = x` 得参数 s,
/// 再取 `bezier_y(s)`(CSS cubic-bezier 的标准求值;牛顿迭代 + 二分兜底)。
///
/// 控制点 x1/x2 必须在 [0,1](CSS 规范如此,x 单调才有唯一解);
/// 端点固定 P0=(0,0)、P3=(1,1)。
pub fn cubic_bezier_y(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    // 端点直读(也保证 x=0/1 时不受迭代误差影响)
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let bx = |s: f64| bezier1(0.0, x1, x2, 1.0, s);
    let by = |s: f64| bezier1(0.0, y1, y2, 1.0, s);
    let bdx = |s: f64| bezier1_deriv(0.0, x1, x2, 1.0, s);
    // 牛顿迭代(初值 = x 本身,足够接近)
    let mut s = x;
    for _ in 0..NEWTON_ITERS {
        let err = bx(s) - x;
        if err.abs() < EPSILON {
            return by(s);
        }
        let d = bdx(s);
        if d.abs() < EPSILON {
            break;
        }
        s -= err / d;
    }
    // 二分兜底(x(s) 在控制点合法时单调)
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for _ in 0..BISECT_ITERS {
        s = (lo + hi) / 2.0;
        if bx(s) < x {
            lo = s;
        } else {
            hi = s;
        }
    }
    by(s)
}

/// 一维三次贝塞尔 B(s) = (1-s)³p0 + 3(1-s)²s·p1 + 3(1-s)s²·p2 + s³p3。
fn bezier1(p0: f64, p1: f64, p2: f64, p3: f64, s: f64) -> f64 {
    let t = 1.0 - s;
    t * t * t * p0 + 3.0 * t * t * s * p1 + 3.0 * t * s * s * p2 + s * s * s * p3
}

/// [`bezier1`] 对 s 的导数。
fn bezier1_deriv(p0: f64, p1: f64, p2: f64, p3: f64, s: f64) -> f64 {
    let t = 1.0 - s;
    3.0 * t * t * (p1 - p0) + 6.0 * t * s * (p2 - p1) + 3.0 * s * s * (p3 - p2)
}

const NEWTON_ITERS: usize = 8;
const BISECT_ITERS: usize = 24;
const EPSILON: f64 = 1e-7;

#[cfg(test)]
mod tests {
    use super::*;

    fn sampled(ease: &Easing, steps: usize) -> Vec<f64> {
        (0..=steps)
            .map(|i| ease.apply(f64::from(i as u32) / f64::from(steps as u32)))
            .collect()
    }

    #[test]
    fn easing_endpoints_are_zero_and_one() {
        let easings = [
            Easing::Linear,
            Easing::InCubic,
            Easing::OutCubic,
            Easing::InOutCubic,
            Easing::CubicBezier {
                x1: 0.25,
                y1: 0.1,
                x2: 0.25,
                y2: 1.0,
            }, // CSS "ease"
            Easing::CubicBezier {
                x1: 0.34,
                y1: 1.56,
                x2: 0.64,
                y2: 1.0,
            }, // ease-back(y 越界)
        ];
        for e in &easings {
            assert_eq!(e.apply(0.0), 0.0, "{e:?}: 端点 0");
            assert_eq!(e.apply(1.0), 1.0, "{e:?}: 端点 1");
            // 越界输入被钳制
            assert_eq!(e.apply(-0.5), 0.0);
            assert_eq!(e.apply(1.5), 1.0);
        }
    }

    #[test]
    fn cubic_curves_are_monotonic() {
        // 单调递增(允许相等):标准 cubic 系 + CSS ease
        for e in [
            Easing::InCubic,
            Easing::OutCubic,
            Easing::InOutCubic,
            Easing::CubicBezier {
                x1: 0.25,
                y1: 0.1,
                x2: 0.25,
                y2: 1.0,
            },
        ] {
            let ys = sampled(&e, 100);
            for pair in ys.windows(2) {
                assert!(pair[1] >= pair[0], "{e:?} 在采样处单调递减: {pair:?}");
            }
        }
    }

    #[test]
    fn cubic_bezier_solver_matches_reference_samples() {
        // 与 W3C 规范参考实现的公认采样对齐:ease(0.5) ≈ 0.8024
        let ease = Easing::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.0,
        };
        assert!(
            (ease.apply(0.5) - 0.8024).abs() < 1e-3,
            "ease(0.5) = {}",
            ease.apply(0.5)
        );
        // 线性贝塞尔:y=x 的曲线(控制点在对角线上)应当恒等
        let diag = Easing::CubicBezier {
            x1: 1.0 / 3.0,
            y1: 1.0 / 3.0,
            x2: 2.0 / 3.0,
            y2: 2.0 / 3.0,
        };
        for i in 0..=10 {
            let x = f64::from(i) / 10.0;
            assert!((diag.apply(x) - x).abs() < 1e-6, "对角线控制点应退化为线性");
        }
        // ease-back 有回摆:y 先冲过 1 再回落到 1
        let back = Easing::CubicBezier {
            x1: 0.34,
            y1: 1.56,
            x2: 0.64,
            y2: 1.0,
        };
        let max_y = sampled(&back, 200).into_iter().fold(f64::MIN, f64::max);
        assert!(max_y > 1.01, "ease-back 应有回摆,峰 {max_y}");
    }

    /// A12 黄金值快照断言:u = 0.0, 0.1, .., 1.0 共 11 点,容差 1e-9。
    fn assert_golden(name: &str, ease: &Easing, golden: &[f64; 11]) {
        for (i, &g) in golden.iter().enumerate() {
            let u = f64::from(i as u32) / 10.0;
            assert!(
                (ease.apply(u) - g).abs() < 1e-9,
                "{name}: u={u} 实际 {} 期望 {g}",
                ease.apply(u)
            );
        }
    }

    // ===== A12 黄金值快照(黄金值,改动即破坏动画手感契约)=====
    // 生成方式:与实现逐字同构的独立 rustc 程序在同机求值(u = i/10.0,
    // i = 0..=10),输出最短往返字面量;快照容差 1e-9。

    const GOLDEN_LINEAR: [f64; 11] = [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0];

    /// 黄金值,改动即破坏动画手感契约(A12)
    const GOLDEN_IN_CUBIC: [f64; 11] = [
        0.0,
        0.0010000000000000002,
        0.008000000000000002,
        0.027,
        0.06400000000000002,
        0.125,
        0.216,
        0.3429999999999999,
        0.5120000000000001,
        0.7290000000000001,
        1.0,
    ];

    /// 黄金值,改动即破坏动画手感契约(A12)
    const GOLDEN_OUT_CUBIC: [f64; 11] = [
        0.0,
        0.2709999999999999,
        0.4879999999999999,
        0.657,
        0.784,
        0.875,
        0.9359999999999999,
        0.973,
        0.992,
        0.999,
        1.0,
    ];

    /// 黄金值,改动即破坏动画手感契约(A12)
    const GOLDEN_IN_OUT_CUBIC: [f64; 11] = [
        0.0,
        0.004000000000000001,
        0.03200000000000001,
        0.108,
        0.25600000000000006,
        0.5,
        0.744,
        0.8919999999999999,
        0.968,
        0.996,
        1.0,
    ];

    /// 黄金值,改动即破坏动画手感契约(A12)。CSS `ease`(0.25,0.1,0.25,1)
    const GOLDEN_CSS_EASE: [f64; 11] = [
        0.0,
        0.094796305715106,
        0.29524455671683436,
        0.5133153550526887,
        0.682540506014571,
        0.8024033876954126,
        0.8852293098934654,
        0.9407646142979403,
        0.9756253688544969,
        0.994316477509521,
        1.0,
    ];

    /// 黄金值,改动即破坏动画手感契约(A12)。CSS `ease-in`(0.42,0,1,1)
    const GOLDEN_CSS_EASE_IN: [f64; 11] = [
        0.0,
        0.01702661076560586,
        0.062282001325569394,
        0.1295767564385426,
        0.2148609387527464,
        0.3153568125058931,
        0.4291197633963204,
        0.554814032528637,
        0.6916339332833451,
        0.8394277819903987,
        1.0,
    ];

    /// 黄金值,改动即破坏动画手感契约(A12)。CSS `ease-out`(0,0,0.58,1)
    const GOLDEN_CSS_EASE_OUT: [f64; 11] = [
        0.0,
        0.16057221800960117,
        0.3083660667166552,
        0.44518596747136274,
        0.5708802366036795,
        0.6846431874941069,
        0.7851390612472535,
        0.8704232435614573,
        0.9377179986744306,
        0.9829733892343941,
        1.0,
    ];

    /// 黄金值,改动即破坏动画手感契约(A12)。CSS `ease-in-out`(0.42,0,0.58,1)
    const GOLDEN_CSS_EASE_IN_OUT: [f64; 11] = [
        0.0,
        0.019722447263855636,
        0.08165982204916353,
        0.1873958911579093,
        0.3318838697203919,
        0.5,
        0.6681161302796081,
        0.8126041088420908,
        0.9183401779508366,
        0.9802775527361445,
        1.0,
    ];

    /// 黄金值,改动即破坏动画手感契约(A12)。ease-back(0.34,1.56,0.64,1,y 越界回摆)
    const GOLDEN_EASE_BACK: [f64; 11] = [
        0.0,
        0.4039327859634652,
        0.7030400000000003,
        0.9073613805611253,
        1.0301814292837779,
        1.0874006702187318,
        1.0965748500505899,
        1.0757760615526468,
        1.0424743802320982,
        1.0126155791451297,
        1.0,
    ];

    #[test]
    fn easing_golden_value_snapshots() {
        assert_golden("Linear", &Easing::Linear, &GOLDEN_LINEAR);
        assert_golden("InCubic", &Easing::InCubic, &GOLDEN_IN_CUBIC);
        assert_golden("OutCubic", &Easing::OutCubic, &GOLDEN_OUT_CUBIC);
        assert_golden("InOutCubic", &Easing::InOutCubic, &GOLDEN_IN_OUT_CUBIC);
        assert_golden(
            "CSS ease",
            &Easing::CubicBezier {
                x1: 0.25,
                y1: 0.1,
                x2: 0.25,
                y2: 1.0,
            },
            &GOLDEN_CSS_EASE,
        );
        assert_golden(
            "CSS ease-in",
            &Easing::CubicBezier {
                x1: 0.42,
                y1: 0.0,
                x2: 1.0,
                y2: 1.0,
            },
            &GOLDEN_CSS_EASE_IN,
        );
        assert_golden(
            "CSS ease-out",
            &Easing::CubicBezier {
                x1: 0.0,
                y1: 0.0,
                x2: 0.58,
                y2: 1.0,
            },
            &GOLDEN_CSS_EASE_OUT,
        );
        assert_golden(
            "CSS ease-in-out",
            &Easing::CubicBezier {
                x1: 0.42,
                y1: 0.0,
                x2: 0.58,
                y2: 1.0,
            },
            &GOLDEN_CSS_EASE_IN_OUT,
        );
        assert_golden(
            "ease-back",
            &Easing::CubicBezier {
                x1: 0.34,
                y1: 1.56,
                x2: 0.64,
                y2: 1.0,
            },
            &GOLDEN_EASE_BACK,
        );
    }
}
