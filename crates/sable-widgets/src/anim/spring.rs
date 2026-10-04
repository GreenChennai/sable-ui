//! 弹簧(质量-阻尼-刚度;分册六 §4.2;A2 初速度接续)。
//!
//! [`Spring::solve_with_velocity`] 是解析式(欠阻尼/临界/过阻尼三段公式,
//! 含初速度 v0 项),0 次迭代、随时可求任意 t,不维护步进状态。
//!
//! # 公式来源(A2)
//!
//! 二阶线性 ODE 的单位阶跃响应 + 非零初条件:
//! `m·ẍ + c·ẋ + k·x = k`,`x(0) = 0`、`ẋ(0) = v0`。
//! 通解 = 特解(1)+ 齐次通解(按阻尼比 ζ = c/(2√(km)) 分三段),系数由
//! 初条件解二元一次方程组确定;即经典受迫振动教材结果(如 S. S. Rao
//! 《Mechanical Vibrations》阻尼自由振动章)。记 ω = √(k/m):
//!
//! - 欠阻尼(ζ<1,ω_d = ω√(1-ζ²)):
//!   `x(t) = 1 - e^(-ζωt)·[cos(ω_d t) + (ζω - v0)/ω_d·sin(ω_d t)]`
//! - 临界(ζ≈1):`x(t) = 1 - (1 + (ω - v0)·t)·e^(-ωt)`
//! - 过阻尼(ζ>1,λ = ω√(ζ²-1),λ± = -ζω ± λ):
//!   `x(t) = 1 + C₊·e^(λ₊t) + C₋·e^(λ₋t)`,
//!   `C₊ = (v0 - ζω - λ)/(2λ)`,`C₋ = (ζω - λ - v0)/(2λ)`
//!
//! v0 = 0 时三段公式分别退化为 M0 版本(行为兼容)。v0 的单位与 x 的单位/秒
//! 一致(位置量纲 px 时即 px/s),与 [`crate::anim::gesture::GestureTracker`]
//! 的 `fling_velocity()` 输出(px/s)直接衔接——**拖拽甩出松手时刻,以当前
//! 速度进入弹簧,不掉速(iOS 手感的关键,08-A2)**。

/// 弹簧(质量-阻尼-刚度)。[`Spring::solve`] 系列是解析式,随时可求任意 t。
///
/// # 档位单点(ANI-06/TOK-08)
///
/// [`Spring::SNAPPY`]/[`Spring::SOFT`]/[`Spring::BOUNCY`] 的数值单一源自
/// `crate::tokens::SPRING_SNAPPY`/`SPRING_SOFT`/`SPRING_BOUNCY`(theme
/// feature 关闭的降级编译才落到本文件字面量);数值与
/// `docs/design/sable-tokens.json` 的 motion.spring 表逐值一致
/// (TC-ANI-SPRING-01,门禁 tests/gate_tokens_sync.rs)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    /// 刚度 k
    pub stiffness: f64,
    /// 阻尼 c
    pub damping: f64,
    /// 质量 m
    pub mass: f64,
}

impl Spring {
    /// 面板归位(分册六 §4.2:近乎临界,几乎无过冲;§5.8 弹簧利落档)。
    #[cfg(feature = "theme")]
    pub const SNAPPY: Spring = Spring {
        stiffness: crate::tokens::SPRING_SNAPPY.stiffness,
        damping: crate::tokens::SPRING_SNAPPY.damping,
        mass: crate::tokens::SPRING_SNAPPY.mass,
    };
    /// theme-less 降级编译的字面量回退(与 tokens::SPRING_SNAPPY 同值)。
    #[cfg(not(feature = "theme"))]
    pub const SNAPPY: Spring = Spring {
        stiffness: 400.0,
        damping: 28.0,
        mass: 1.0,
    };
    /// 柔和跟随(§5.8 弹簧柔和档:面板拖拽跟手,轻微过冲的柔顺跟随)。
    #[cfg(feature = "theme")]
    pub const SOFT: Spring = Spring {
        stiffness: crate::tokens::SPRING_SOFT.stiffness,
        damping: crate::tokens::SPRING_SOFT.damping,
        mass: crate::tokens::SPRING_SOFT.mass,
    };
    /// theme-less 降级编译的字面量回退(与 tokens::SPRING_SOFT 同值)。
    #[cfg(not(feature = "theme"))]
    pub const SOFT: Spring = Spring {
        stiffness: 180.0,
        damping: 22.0,
        mass: 1.0,
    };
    /// 弹窗回弹(欠阻尼,可见过冲;历史档位,数值保持契约不变)。
    #[cfg(feature = "theme")]
    pub const BOUNCY: Spring = Spring {
        stiffness: crate::tokens::SPRING_BOUNCY.stiffness,
        damping: crate::tokens::SPRING_BOUNCY.damping,
        mass: crate::tokens::SPRING_BOUNCY.mass,
    };
    /// theme-less 降级编译的字面量回退(与 tokens::SPRING_BOUNCY 同值)。
    #[cfg(not(feature = "theme"))]
    pub const BOUNCY: Spring = Spring {
        stiffness: 300.0,
        damping: 15.0,
        mass: 1.0,
    };

    /// 从 0→1 的单位弹簧在秒时刻 `t_sec` 的值(解析解,初速度 = 0)。
    ///
    /// 便捷入口,等价于 [`Self::solve_with_velocity`]`(t_sec, 0.0)`(M0 API,
    /// 语义保持)。
    pub fn solve(&self, t_sec: f64) -> f64 {
        self.solve_with_velocity(t_sec, 0.0)
    }

    /// 从 0→1 的单位弹簧在秒时刻 `t_sec` 的值,初速度 `v0`(位置量纲/秒)。
    ///
    /// 三段公式见模块文档;三段在 ζ=1 边界连续,且 `∂x/∂t|₀ = v0`(初速度
    /// 连续性,A2 验收:拖拽甩出以当前速度进入弹簧,不掉速)。
    ///
    /// 非法参数(刚度/质量 ≤ 0 或 NaN)防御性直接到位(返回 1,不 panic);
    /// `v0` 为 NaN/∞ 时结果未定义(调用方保证,GestureTracker 已守卫)。
    pub fn solve_with_velocity(&self, t_sec: f64, v0: f64) -> f64 {
        // 防御 NaN:字段是 pub f64,调用方可能传 NaN。!(NaN > 0.0) 为 true 而
        // NaN <= 0.0 为 false,否定比较不能改写,否则 NaN 会漏进 sqrt 产出 NaN。
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(self.stiffness > 0.0) || !(self.mass > 0.0) {
            return 1.0; // 非法参数:直接到位(防御,不 panic)
        }
        let omega = (self.stiffness / self.mass).sqrt();
        let zeta = self.damping / (2.0 * (self.stiffness * self.mass).sqrt());
        if zeta < 1.0 {
            // 欠阻尼:x(t) = 1 - e^(-ζωt)·[cos(ω_d t) + (ζω - v0)/ω_d·sin(ω_d t)]
            let w_d = omega * (1.0 - zeta * zeta).sqrt();
            let decay = (-zeta * omega * t_sec).exp();
            let b = (zeta * omega - v0) / w_d;
            1.0 - decay * ((w_d * t_sec).cos() + b * (w_d * t_sec).sin())
        } else if (zeta - 1.0).abs() < 1e-9 {
            // 临界:x(t) = 1 - (1 + (ω - v0)·t)·e^(-ωt)
            1.0 - (1.0 + (omega - v0) * t_sec) * (-omega * t_sec).exp()
        } else {
            // 过阻尼:两个实特征根 λ± = -ζω ± λ,λ = ω√(ζ²-1)
            // x(t) = 1 + C₊·e^(λ₊t) + C₋·e^(λ₋t)
            let lam = omega * (zeta * zeta - 1.0).sqrt();
            let e_pos = ((lam - zeta * omega) * t_sec).exp();
            let e_neg = (-(zeta * omega + lam) * t_sec).exp();
            let c_pos = (v0 - zeta * omega - lam) / (2.0 * lam);
            let c_neg = (zeta * omega - lam - v0) / (2.0 * lam);
            1.0 + c_pos * e_pos + c_neg * e_neg
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_tiers_match_tokens_and_soft_follows_gently() {
        // 档位数值 = tokens 单点(theme 开启的常规编译面;TC-ANI-SPRING-01
        // 在 tests/gate_tokens_sync.rs 另与 JSON 逐值对拍)
        for (tier, preset) in [
            (Spring::SNAPPY, crate::tokens::SPRING_SNAPPY),
            (Spring::SOFT, crate::tokens::SPRING_SOFT),
            (Spring::BOUNCY, crate::tokens::SPRING_BOUNCY),
        ] {
            assert_eq!(
                (tier.stiffness, tier.damping, tier.mass),
                (preset.stiffness, preset.damping, preset.mass)
            );
        }
        // SOFT 端点正确、轻微过冲(ζ ≈ 0.82,峰约 1.01——柔顺跟随不松垮)
        assert_eq!(Spring::SOFT.solve(0.0), 0.0);
        assert!((Spring::SOFT.solve(5.0) - 1.0).abs() < 1e-6);
        let peak = (0..=400)
            .map(|i| Spring::SOFT.solve(f64::from(i) / 400.0 * 2.0))
            .fold(f64::MIN, f64::max);
        assert!(peak > 1.0 && peak < 1.05, "SOFT 应轻微过冲,实际 {peak}");
    }

    #[test]
    fn spring_solve_endpoints_and_overshoot() {
        for spring in [Spring::SNAPPY, Spring::BOUNCY] {
            assert_eq!(spring.solve(0.0), 0.0, "起点 0");
            assert!((spring.solve(5.0) - 1.0).abs() < 1e-6, "5s 后应收敛到 1");
        }
        // BOUNCY 必有过冲回摆
        let max_v = (0..=400)
            .map(|i| Spring::BOUNCY.solve(f64::from(i) / 400.0 * 2.0))
            .fold(f64::MIN, f64::max);
        assert!(max_v > 1.05, "BOUNCY 峰值应明显过冲,实际 {max_v}");
        // SNAPPY 几乎无过冲(ζ≈0.7,峰 < 5%)
        let max_s = (0..=400)
            .map(|i| Spring::SNAPPY.solve(f64::from(i) / 400.0 * 2.0))
            .fold(f64::MIN, f64::max);
        assert!(max_s < 1.05, "SNAPPY 应近乎无过冲,实际 {max_s}");
        // 非法参数不 panic、直接到位
        assert_eq!(
            Spring {
                stiffness: 0.0,
                damping: 1.0,
                mass: 1.0
            }
            .solve(0.3),
            1.0
        );
    }

    #[test]
    fn initial_velocity_zero_matches_legacy_path() {
        // v0 = 0 与旧 solve(t) 完全一致(旧 API = 新 API 的便捷入口)
        for spring in [Spring::SNAPPY, Spring::BOUNCY] {
            for i in 0..=50 {
                let t = f64::from(i) / 50.0;
                assert_eq!(
                    spring.solve(t),
                    spring.solve_with_velocity(t, 0.0),
                    "v0=0 必须与旧路径逐位一致,t={t}"
                );
            }
        }
    }

    #[test]
    fn initial_velocity_derivative_at_zero_equals_v0() {
        // A2 验收:v0=500(px/s)时 x(t) 在 t→0+ 的导数 ≈ 500(数值差分)。
        // 误差 ~ h/2·|x''(0)|:h=1e-7 时约 4e-4,取 0.5 容差宽裕。
        for spring in [Spring::SNAPPY, Spring::BOUNCY] {
            let h = 1e-7;
            let slope =
                (spring.solve_with_velocity(h, 500.0) - spring.solve_with_velocity(0.0, 500.0)) / h;
            assert!(
                (slope - 500.0).abs() < 0.5,
                "{spring:?} 初速度连续性:slope={slope}"
            );
            // t = 0 处值恒为 0(与 v0 无关)
            assert_eq!(spring.solve_with_velocity(0.0, 500.0), 0.0);
        }
    }

    #[test]
    fn initial_velocity_overshoot_curve_is_continuous_and_settles() {
        // v0=500 的 BOUNCY 轨迹:过冲回摆曲线连续可导(采样无跳变)且最终收敛。
        let spring = Spring::BOUNCY;
        let steps = 2000;
        let mut prev = spring.solve_with_velocity(0.0, 500.0);
        let mut max_v = f64::MIN;
        for i in 1..=steps {
            let t = f64::from(i) / f64::from(steps) * 1.5;
            let v = spring.solve_with_velocity(t, 500.0);
            assert!(v.is_finite(), "t={t} 处发散");
            // 相邻 0.75ms 采样差有界(连续性;速度上限 ~ ω·幅度量级,取宽松 2.0)
            assert!((v - prev).abs() < 2.0, "t={t} 出现跳变:{prev} → {v}");
            max_v = max_v.max(v);
            prev = v;
        }
        assert!(max_v > 1.5, "大初速度应产生显著过冲,峰 {max_v}");
        assert!((prev - 1.0).abs() < 1e-3, "1.5s 后应收敛回 1,实际 {prev}");
    }

    #[test]
    fn overdamped_with_velocity_has_no_overshoot() {
        // 过阻尼 + 初速度:单调趋近 1,不越过(过阻尼定义)。
        let spring = Spring {
            stiffness: 100.0,
            damping: 80.0, // ζ = 80/(2·√100) = 4 > 1
            mass: 1.0,
        };
        let mut prev = spring.solve_with_velocity(0.0, 50.0);
        for i in 1..=500 {
            let t = f64::from(i) / 100.0;
            let v = spring.solve_with_velocity(t, 50.0);
            assert!(v >= prev - 1e-12, "过阻尼在 t={t} 回退:{prev} → {v}");
            assert!(v <= 1.0 + 1e-12, "过阻尼在 t={t} 过冲:{v}");
            prev = v;
        }
        // ζ=1 边界(临界)与两侧公式衔接连续:t=1s 处三段差异应在 1e-4 内
        // (ζ 偏移 1e-8 时欠阻尼公式的 ω_d² 项贡献 ~7e-6 误差,1e-4 宽裕)
        let crit = Spring {
            stiffness: 100.0,
            damping: 20.0, // ζ = 1
            mass: 1.0,
        };
        let left = Spring {
            stiffness: 100.0,
            damping: 20.0 * (1.0 - 1e-8),
            mass: 1.0,
        }
        .solve_with_velocity(1.0, 30.0);
        let right = Spring {
            stiffness: 100.0,
            damping: 20.0 * (1.0 + 1e-8),
            mass: 1.0,
        }
        .solve_with_velocity(1.0, 30.0);
        let mid = crit.solve_with_velocity(1.0, 30.0);
        assert!(
            (left - mid).abs() < 1e-4 && (right - mid).abs() < 1e-4,
            "ζ=1 边界不连续: {left} / {mid} / {right}"
        );
    }

    fn assert_golden(name: &str, spring: Spring, v0: f64, golden: &[f64; 11]) {
        for (i, &g) in golden.iter().enumerate() {
            let t = f64::from(i as u32) / 10.0;
            let actual = spring.solve_with_velocity(t, v0);
            assert!(
                (actual - g).abs() < 1e-9,
                "{name}: t={t}s 实际 {actual} 期望 {g}"
            );
        }
    }

    // ===== A12 黄金值快照(黄金值,改动即破坏动画手感契约)=====
    // 生成方式:与实现逐字同构的独立 rustc 程序在同机求值(t = i/10.0 秒,
    // i = 0..=10),输出最短往返字面量;快照容差 1e-9。

    /// 黄金值,改动即破坏动画手感契约(A12)。SNAPPY,t=0..1s
    const GOLDEN_SNAPPY: [f64; 11] = [
        0.0,
        0.7257131307952929,
        1.0415968937186537,
        1.0195931691201117,
        0.9988429470475908,
        0.9987274892413708,
        0.9999812239893879,
        1.0000760662422514,
        1.0000064700369682,
        0.9999958276183616,
        0.9999993142908399,
    ];

    /// 黄金值,改动即破坏动画手感契约(A12)。BOUNCY,t=0..1s
    const GOLDEN_BOUNCY: [f64; 11] = [
        0.0,
        0.7685832553919976,
        1.2210429970933683,
        1.0536296568197125,
        0.9511623301108102,
        0.9875931358829206,
        1.0107852587686443,
        1.0028656185455924,
        0.9976193286885089,
        0.9993391226416491,
        1.0005252390740185,
    ];

    /// 黄金值,改动即破坏动画手感契约(A12)。BOUNCY + 初速度 500px/s(甩出接续)
    const GOLDEN_BOUNCY_V0_500: [f64; 11] = [
        0.0,
        15.895730672600063,
        1.357475748626312,
        -2.320462672884722,
        0.8902889046390112,
        1.7399058762808868,
        1.031153114727681,
        0.8351856553503157,
        0.9915623289542169,
        1.03669895119849,
        1.0022136891803959,
    ];

    /// 黄金值,改动即破坏动画手感契约(A12)。SNAPPY + 初速度 500px/s
    const GOLDEN_SNAPPY_V0_500: [f64; 11] = [
        0.0,
        9.270820661451863,
        1.6401621651342837,
        0.5418927755958431,
        0.9289823335779721,
        1.0228829036273785,
        1.0059214838930162,
        0.9990232756235103,
        0.9995714968812204,
        1.0000293790054358,
        1.0000281152337376,
    ];

    #[test]
    fn spring_golden_value_snapshots() {
        assert_golden("SNAPPY", Spring::SNAPPY, 0.0, &GOLDEN_SNAPPY);
        assert_golden("BOUNCY", Spring::BOUNCY, 0.0, &GOLDEN_BOUNCY);
        assert_golden(
            "BOUNCY v0=500",
            Spring::BOUNCY,
            500.0,
            &GOLDEN_BOUNCY_V0_500,
        );
        assert_golden(
            "SNAPPY v0=500",
            Spring::SNAPPY,
            500.0,
            &GOLDEN_SNAPPY_V0_500,
        );
    }
}
