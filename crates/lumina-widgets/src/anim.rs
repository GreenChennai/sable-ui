//! 动画引擎(分册六 §4.2,M0 骨架):缓动/弹簧/插值/动画值,~200 行纯逻辑。
//!
//! L1 微交互(Tween)与 L2 物理(Spring)共用本模块;L3 内容层关键帧插值
//! 在 lumina-video(docs/04 §9)。**纯逻辑,不引 cx**:时间由调用方注入
//! ([`Animated::value_at`] 收 `Instant`),确定性可单测;UI 侧在
//! `is_running()` 时自行 `cx.request_animation_frame()`(动画运行才请求帧,
//! 静止时零帧提交,分册六 §4.4 性能军规)。
//!
//! # 与分册六 §4.2 原文的两处适配
//! 1. `Easing` 从「`fn easing(name) -> impl Fn` + kurbo 控制点」改为自有
//!    枚举 + 解析求值:CSS cubic-bezier 用牛顿迭代解 x(s)=t(标准做法),
//!    不经 kurbo 三次贝塞尔的隐式参数化;
//! 2. `Animated` 增加 `*_at(now)` 显式时间版本(单测确定性),`value()`/
//!    `is_running()` 是 `Instant::now()` 的便捷封装。

use std::time::{Duration, Instant};

use gpui::Hsla;

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

/// 弹簧(质量-阻尼-刚度;分册六 §4.2)。[`Spring::solve`] 是解析式
/// (欠阻尼 = 衰减余弦;过阻尼/临界退化为非振荡公式),0 次迭代、
/// 随时可求任意 t,不维护步进状态。
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
    /// 面板归位(分册六 §4.2:近乎临界,几乎无过冲)。
    pub const SNAPPY: Spring = Spring {
        stiffness: 400.0,
        damping: 28.0,
        mass: 1.0,
    };
    /// 弹窗回弹(欠阻尼,可见过冲)。
    pub const BOUNCY: Spring = Spring {
        stiffness: 300.0,
        damping: 15.0,
        mass: 1.0,
    };

    /// 从 0→1 的单位弹簧在秒时刻 `t_sec` 的值(解析解)。
    ///
    /// - 欠阻尼(ζ<1):`1 - e^(-ζωt)·(cos(ω_d t) + (ζω/ω_d)·sin(ω_d t))`
    /// - 临界(ζ≈1):`1 - (1 + ωt)·e^(-ωt)`
    /// - 过阻尼(ζ>1):双指数和(ω_d 取 i·|ω_d| 的实数化形式)
    pub fn solve(&self, t_sec: f64) -> f64 {
        // 防御 NaN:字段是 pub f64,调用方可能传 NaN。!(NaN > 0.0) 为 true 而
        // NaN <= 0.0 为 false,否定比较不能改写,否则 NaN 会漏进 sqrt 产出 NaN。
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(self.stiffness > 0.0) || !(self.mass > 0.0) {
            return 1.0; // 非法参数:直接到位(防御,不 panic)
        }
        let omega = (self.stiffness / self.mass).sqrt();
        let zeta = self.damping / (2.0 * (self.stiffness * self.mass).sqrt());
        if zeta < 1.0 {
            let w_d = omega * (1.0 - zeta * zeta).sqrt();
            let decay = (-zeta * omega * t_sec).exp();
            1.0 - decay * ((w_d * t_sec).cos() + (zeta * omega / w_d) * (w_d * t_sec).sin())
        } else if (zeta - 1.0).abs() < 1e-9 {
            1.0 - (1.0 + omega * t_sec) * (-omega * t_sec).exp()
        } else {
            // 过阻尼:两个实特征根 λ± = -ζω ± ω√(ζ²-1)
            // x(t) = 1 - [(ζω+λ)e^{(λ-ζω)t} + (λ-ζω)e^{-(λ+ζω)t}] / 2λ
            let lam = omega * (zeta * zeta - 1.0).sqrt();
            let e_pos = ((lam - zeta * omega) * t_sec).exp();
            let e_neg = (-(zeta * omega + lam) * t_sec).exp();
            1.0 - (e_pos * (zeta * omega + lam) + e_neg * (lam - zeta * omega)) / (2.0 * lam)
        }
    }
}

/// 可插值类型(Lerp):动画引擎对值的唯一要求(分册六 §4.2 `Animated<T: Lerp>`)。
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
        let mix = |a: f32, b: f32| a + (b - a) * t as f32;
        // 色相走最短弧(避免 0.99→0.01 绕远路)
        let dh = other.h - self.h;
        let dh = if dh.abs() > 0.5 { dh - dh.signum() } else { dh };
        Hsla {
            h: (self.h + dh * t as f32).rem_euclid(1.0),
            s: mix(self.s, other.s),
            l: mix(self.l, other.l),
            a: mix(self.a, other.a),
        }
    }
}

/// 一个动画值:组件持有它,每帧读 [`Animated::value`](分册六 §4.2)。
///
/// `set` 打断进行中的动画时**从当前值接续**(from = 此刻 value),视觉无跳变。
#[derive(Clone, Debug)]
pub struct Animated<T: Lerp> {
    from: T,
    to: T,
    started: Instant,
    duration: Duration,
    ease: Easing,
}

impl<T: Lerp> Animated<T> {
    /// 静止在 `initial` 的新动画值。
    pub fn new(initial: T) -> Self {
        Animated {
            from: initial.clone(),
            to: initial,
            started: Instant::now(),
            duration: Duration::ZERO,
            ease: Easing::Linear,
        }
    }

    /// 目标值(动画终点)。
    pub fn target(&self) -> &T {
        &self.to
    }

    /// 发起新动画:从**当前值**接续到 `to`(打断也平滑)。
    pub fn set(&mut self, to: T, duration: Duration, ease: Easing) {
        self.set_at(to, Instant::now(), duration, ease);
    }

    /// 显式时间版本(确定性测试/与帧时钟同步用)。
    pub fn set_at(&mut self, to: T, now: Instant, duration: Duration, ease: Easing) {
        self.from = self.value_at(now);
        self.to = to;
        self.started = now;
        self.duration = duration;
        self.ease = ease;
    }

    /// 当前值(便捷封装 = [`Self::value_at`]`(Instant::now())`)。
    pub fn value(&self) -> T {
        self.value_at(Instant::now())
    }

    /// 显式时间下的当前值;进度走 [`self.ease`],超时钳在终点。
    pub fn value_at(&self, now: Instant) -> T {
        let total = self.duration.as_secs_f64();
        let progress = if total <= 0.0 {
            1.0
        } else {
            (now.saturating_duration_since(self.started).as_secs_f64() / total).min(1.0)
        };
        self.from.lerp(&self.to, self.ease.apply(progress))
    }

    /// 动画是否仍在进行(便捷封装 = [`Self::is_running_at`]`(Instant::now())`)。
    pub fn is_running(&self) -> bool {
        self.is_running_at(Instant::now())
    }

    /// 显式时间版本;为真时宿主应 `cx.request_animation_frame()`。
    pub fn is_running_at(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started) < self.duration
    }
}

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
    fn lerp_implementations_interpolate() {
        assert_eq!(0.0_f64.lerp(&10.0, 0.25), 2.5);
        assert_eq!(0.0_f32.lerp(&10.0, 0.5), 5.0);
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
        let mid = a.lerp(&b, 0.5);
        assert!((mid.s - 0.5).abs() < 1e-6);
        // 色相最短弧:0.9 → 0.1 应经过 1.0/0.0,中点在 0.0
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
    }

    #[test]
    fn animated_interrupt_continues_from_current_value() {
        let t0 = Instant::now();
        let mut anim = Animated::new(0.0_f64);
        anim.set_at(100.0, t0, Duration::from_secs(1), Easing::Linear);
        // 走到一半 = 50
        let half = t0 + Duration::from_millis(500);
        assert_eq!(anim.value_at(half), 50.0);
        assert!(anim.is_running_at(half));
        // 半途打断改目标:从当前值 50 接续,而不是跳回 0
        anim.set_at(60.0, half, Duration::from_secs(1), Easing::Linear);
        assert_eq!(anim.value_at(half), 50.0, "打断瞬间值不跳变");
        let quarter = half + Duration::from_millis(250);
        assert_eq!(anim.value_at(quarter), 52.5, "从 50 向 60 推进 1/4");
        // 结束钳在终点
        let done = half + Duration::from_secs(2);
        assert_eq!(anim.value_at(done), 60.0);
        assert!(!anim.is_running_at(done));
    }

    #[test]
    fn animated_zero_duration_is_immediate() {
        let t0 = Instant::now();
        let mut anim = Animated::new(1.0_f32);
        anim.set_at(9.0, t0, Duration::ZERO, Easing::InCubic);
        assert_eq!(anim.value_at(t0), 9.0, "零时长立刻到位");
        assert!(!anim.is_running_at(t0));
        // target 可读
        assert_eq!(*anim.target(), 9.0);
    }
}
