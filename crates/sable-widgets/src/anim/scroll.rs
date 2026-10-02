//! A5 平滑滚动 + 橡皮筋 ScrollPhysics(08 迭代计划 A5;分册六 §4.2 L2 物理层)。
//!
//! # 模型(纯函数可测,时间显式注入)
//!
//! - **速度积分器**:[`ScrollPhysics::push_scroll_delta`] 把滚轮增量累积为
//!   fling 初速度(px/s;组件把滚轮像素按手感系数换算成速度增量后传入);
//!   [`ScrollPhysics::offset_at(t_ms)`] 返回自锚点起 `t_ms` 内的**位移**:
//!   `v₀·τ·(1 - e^(-t/τ))`(速度指数衰减 `v(t) = v₀·e^(-t/τ)`,自然停止,
//!   总位移收敛于 `v₀·τ`)——"滚轮输入 → 速度积分器 → eased 停止"。
//! - **橡皮筋**:[`ScrollPhysics::overscroll`] 处理一次滚动步:界内 1:1 返回
//!   `current + delta`(跟手);过界时位移 = 过界量经 [`ScrollPhysics::rubber_band`]
//!   阻尼曲线([`OVERSCROLL_DAMP`] 包络,饱和长度 [`OVERSCROLL_SATURATION_PX`]),
//!   同时把内部速度置为回弹速度——后续 [`ScrollPhysics::offset_at`] 驱动
//!   平滑回到边界(`offset_at(∞) = -伸长量`,撞墙即终止 fling、开始回弹)。
//!
//! 时间零内嵌(`offset_at` 收 `t_ms`,锚点时刻由组件持有),A8:
//! [`crate::anim::reduced_motion`] 为真时 [`ScrollPhysics::offset_at`] 直通
//! 收敛位移(瞬时停/回弹)。
//!
//! # 组件层用法(约定)
//!
//! ```text
//! 滚轮事件:  base = physics.overscroll(delta, offset, min, max)   // 立即应用
//! 每帧渲染:  render = base + physics.offset_at(now_ms - anchor_ms) // fling/回弹位移
//! 新的 push/overscroll 后组件自行重置 anchor(base 亦随之重定基)。
//! ```

use super::reduced_motion;

/// 指数衰减时间常数(ms)。约 0.35s:一格滚轮的滑行尾感接近主流平台
/// 平滑滚动,同时总位移 = v₀·τ 有界(分册六 §4.2 "eased 停止")。
const DECAY_MS: f64 = 350.0;
/// 橡皮筋阻尼包络(过界位移 ≤ 过界量 × 0.35,08-A5)。
const OVERSCROLL_DAMP: f64 = 0.35;
/// 橡皮筋饱和长度(px):过界量远超它时伸长量趋于 `DAMP·SAT`(上限感)。
const OVERSCROLL_SATURATION_PX: f64 = 120.0;

/// 滚动物理(A5):速度积分器 + 指数衰减停止 + 边界橡皮筋。
#[derive(Clone, Debug, Default)]
pub struct ScrollPhysics {
    /// fling/回弹速度(px/s,带符号)。
    velocity: f64,
}

impl ScrollPhysics {
    /// 静止的滚动物理。
    pub fn new() -> Self {
        ScrollPhysics { velocity: 0.0 }
    }

    /// 累积滚动速度(速度积分器):`delta` 为本次滚轮事件贡献的速度增量
    /// (px/s,组件按滚轮像素 × 手感系数换算)。
    pub fn push_scroll_delta(&mut self, delta_px_per_s: f64) {
        self.velocity += delta_px_per_s;
    }

    /// 当前内部速度(px/s)。
    pub fn velocity(&self) -> f64 {
        self.velocity
    }

    /// 自锚点起 `t_ms` 内的位移(px;速度指数衰减,`t → ∞` 收敛于
    /// `v₀·τ`,`τ = [`DECAY_MS`]`)。
    ///
    /// 纯函数:同一 (velocity, t_ms) 恒得同值。负 t / NaN → 0。
    /// A8:reduced_motion 直通收敛位移(瞬时完成滑行/回弹)。
    pub fn offset_at(&self, t_ms: f64) -> f64 {
        if reduced_motion() {
            return self.velocity * (DECAY_MS / 1000.0);
        }
        if t_ms.is_nan() || t_ms <= 0.0 || self.velocity == 0.0 {
            return 0.0;
        }
        let tau_s = DECAY_MS / 1000.0;
        self.velocity * tau_s * (1.0 - (-t_ms / DECAY_MS).exp())
    }

    /// 一次滚动步(A5 完整入口):界内 1:1 跟手返回 `current + delta`
    /// (不动速度,平滑滚动走 [`Self::push_scroll_delta`]);过界返回
    /// 边界 + [`Self::rubber_band`] 伸长量,并把速度置为回弹速度——
    /// 之后 [`Self::offset_at`] 驱动从伸长位置平滑回到边界(撞墙即终止
    /// fling)。
    ///
    /// 区间非法(NaN 或 min > max)时防御性原样返回 `current`。
    pub fn overscroll(&mut self, delta: f64, current: f64, min: f64, max: f64) -> f64 {
        if min.is_nan() || max.is_nan() || min > max {
            return current;
        }
        let proposed = current + delta;
        if proposed > max {
            let stretch = Self::rubber_band(proposed - max);
            // 回弹:offset_at(∞) = velocity·τ = -stretch → 回到边界
            self.velocity = -stretch * 1000.0 / DECAY_MS;
            max + stretch
        } else if proposed < min {
            let stretch = Self::rubber_band(min - proposed);
            self.velocity = stretch * 1000.0 / DECAY_MS;
            min - stretch
        } else {
            proposed
        }
    }

    /// 橡皮筋曲线(纯函数):过界量 `over_px` → 边界外的伸长量。
    ///
    /// `DAMP·SAT·(1 - e^(-over/SAT))`:小过界近似线性(斜率 = DAMP,跟手),
    /// 大过界饱和于 `DAMP·SAT`;恒 ≤ `DAMP·over`(峰值验收线:
    /// 过界量 × 0.35 × 1.05)。负值 / NaN → 0。
    pub fn rubber_band(over_px: f64) -> f64 {
        if over_px.is_nan() || over_px <= 0.0 {
            return 0.0;
        }
        OVERSCROLL_DAMP
            * OVERSCROLL_SATURATION_PX
            * (1.0 - (-over_px / OVERSCROLL_SATURATION_PX).exp())
    }

    /// 清零速度(到达边界静止、内容尺寸变化后调用)。
    pub fn reset(&mut self) {
        self.velocity = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_input_keeps_offset_constant() {
        let p = ScrollPhysics::new();
        for t in [0.0, 1.0, 16.7, 350.0, 1e9] {
            assert_eq!(p.offset_at(t), 0.0, "无输入时位移恒为 0,t={t}");
        }
        // 界内滚动不引入速度 → 位移仍恒 0
        let mut p = p;
        assert_eq!(p.overscroll(10.0, 50.0, 0.0, 100.0), 60.0);
        assert_eq!(p.offset_at(100.0), 0.0);
        // 纯函数:同一 t 两次调用同值
        p.push_scroll_delta(100.0);
        assert_eq!(p.offset_at(16.0), p.offset_at(16.0));
    }

    #[test]
    fn velocity_decays_monotonically() {
        let mut p = ScrollPhysics::new();
        p.push_scroll_delta(1000.0);
        // 差分速度(位移斜率)应单调递减:指数衰减
        let mut prev_slope = f64::INFINITY;
        for i in 0..20 {
            let t0 = f64::from(i) * 20.0;
            let t1 = t0 + 20.0;
            let slope = (p.offset_at(t1) - p.offset_at(t0)) / 20.0;
            assert!(
                slope < prev_slope,
                "衰减斜率应单调递减:t={t0} slope={slope} prev={prev_slope}"
            );
            prev_slope = slope;
        }
        // 位移收敛于 v₀·τ = 1000·0.35 = 350
        assert!((p.offset_at(1e9) - 350.0).abs() < 1e-6);
    }

    #[test]
    fn rubber_band_peak_is_damped_and_monotonic() {
        // 有界饱和曲线在 FP 域的严格单调性只到渐近线附近为止:
        // 1 - e^(-over/120) 在 e^-x < 2^-53(over ≳ 4.4e3 px)时恒等于 1.0,
        // 此后伸长量恒为渐近值 42.0 —— 故全域只断言非递减,严格递增断言
        // 亚饱和区([1, 10_000] 内各步差远大于 ulp)
        let mut prev = 0.0;
        for over in [1.0, 5.0, 10.0, 50.0, 100.0, 120.0, 500.0, 10_000.0, 1e7] {
            let stretch = ScrollPhysics::rubber_band(over);
            assert!(
                stretch <= over * 0.35 * 1.05,
                "橡皮筋峰值 ≤ 过界量×0.35×1.05:over={over} stretch={stretch}"
            );
            assert!(stretch >= prev, "伸长量不得回退:over={over}");
            assert!(
                stretch <= 0.35 * OVERSCROLL_SATURATION_PX + 1e-9,
                "饱和上限"
            );
            prev = stretch;
        }
        // 亚饱和区严格递增(阻尼曲线真的在"增长",不是平的)
        let mut prev = 0.0;
        for over in [1.0, 5.0, 10.0, 50.0, 100.0, 120.0, 500.0, 10_000.0] {
            let stretch = ScrollPhysics::rubber_band(over);
            assert!(stretch > prev, "亚饱和区应严格递增:over={over}");
            prev = stretch;
        }
        // 饱和区:超大过界量恒等于渐近值(不再增长也不回退)
        assert_eq!(
            ScrollPhysics::rubber_band(1e7),
            ScrollPhysics::rubber_band(10_000.0),
            "饱和后恒为渐近值"
        );
        assert_eq!(ScrollPhysics::rubber_band(0.0), 0.0);
        assert_eq!(ScrollPhysics::rubber_band(-5.0), 0.0);
        assert_eq!(ScrollPhysics::rubber_band(f64::NAN), 0.0);
        // 小过界近似线性(斜率 ≈ 0.35,跟手)
        let small = ScrollPhysics::rubber_band(1.0);
        assert!(
            (small - 0.35).abs() < 0.01,
            "1px 过界应伸长 ≈0.35px,得到 {small}"
        );
    }

    #[test]
    fn overscroll_stretches_and_primes_bounce_back() {
        let mut p = ScrollPhysics::new();
        // 顶端越界 40px:伸长 = rubber(40) ≤ 40·0.35·1.05
        let stretched = p.overscroll(40.0, 100.0, 0.0, 100.0);
        let expect_stretch = ScrollPhysics::rubber_band(40.0);
        assert!((stretched - (100.0 + expect_stretch)).abs() < 1e-12);
        assert!(expect_stretch <= 40.0 * 0.35 * 1.05);
        // 回弹由 offset_at 驱动:位移从 0 收敛到 -stretch(回到边界)
        assert_eq!(p.offset_at(0.0), 0.0);
        let converged = p.offset_at(1e9);
        assert!(
            (converged + expect_stretch).abs() < 1e-9,
            "回弹收敛位移 = -伸长量,得到 {converged}"
        );
        // 中途回弹单调(朝边界走)
        assert!(p.offset_at(100.0) < 0.0 && p.offset_at(100.0) > converged);
        // 底端对称(从边界起滚:过界量 = |delta| = 30,返回负伸长量)
        let mut q = ScrollPhysics::new();
        let low = q.overscroll(-30.0, 0.0, 0.0, 100.0);
        assert!((low - (0.0 - ScrollPhysics::rubber_band(30.0))).abs() < 1e-12);
        assert!((q.offset_at(1e9) - ScrollPhysics::rubber_band(30.0)).abs() < 1e-9);
        // 从界内 5px 起滚同一 delta:过界量只有 25(任务书"过界量"= 越过
        // 边界的部分,不是 delta 全额)
        let mut r = ScrollPhysics::new();
        let low25 = r.overscroll(-30.0, 5.0, 0.0, 100.0);
        assert!((low25 - (0.0 - ScrollPhysics::rubber_band(25.0))).abs() < 1e-12);
    }

    #[test]
    fn overscroll_in_bounds_is_pure_passthrough() {
        let mut p = ScrollPhysics::new();
        assert_eq!(p.overscroll(10.0, 0.0, 0.0, 100.0), 10.0);
        assert_eq!(p.overscroll(-50.0, 60.0, 0.0, 100.0), 10.0);
        assert_eq!(p.velocity(), 0.0, "界内不动速度(平滑滚动走 push)");
    }

    #[test]
    fn push_accumulates_velocity_and_reset_clears() {
        let mut p = ScrollPhysics::new();
        p.push_scroll_delta(120.0);
        p.push_scroll_delta(80.0);
        assert_eq!(p.velocity(), 200.0);
        assert!((p.offset_at(1e9) - 70.0).abs() < 1e-9, "总位移 = 200·0.35");
        p.reset();
        assert_eq!(p.velocity(), 0.0);
    }

    #[test]
    fn invalid_bounds_return_current_defensively() {
        let mut p = ScrollPhysics::new();
        assert_eq!(p.overscroll(10.0, 42.0, f64::NAN, 100.0), 42.0);
        assert_eq!(p.overscroll(10.0, 42.0, 100.0, 0.0), 42.0);
        assert_eq!(p.velocity(), 0.0);
    }

    #[test]
    fn reduced_motion_jumps_to_converged_offset() {
        super::super::set_reduced_motion(true);
        let mut p = ScrollPhysics::new();
        p.push_scroll_delta(1000.0);
        assert_eq!(p.offset_at(0.0), 350.0, "减弱动态:滑行瞬时完成");
        assert_eq!(p.offset_at(16.0), 350.0);
        // 橡皮筋同样瞬时回弹到位
        let stretched = p.overscroll(40.0, 100.0, 0.0, 100.0);
        let back = p.offset_at(0.0);
        assert!(
            (stretched + back - 100.0).abs() < 1e-9,
            "减弱动态:回弹瞬时到位"
        );
        super::super::set_reduced_motion(false);
    }
}
