//! A3 手势驱动动画——速度采样器 GestureTracker(08 迭代计划 A3)。
//!
//! # 组合方式(与组件层的分工)
//!
//! **拖拽期间 1:1 跟手(无插值、无累积延迟):位移直接取指针位置,本模块
//! 不参与渲染;松手一刻才把 [`GestureTracker::fling_velocity`] 交给
//! [`Spring::solve_with_velocity`](crate::anim::Spring::solve_with_velocity,
//! 初速度接续,不掉速)或 [`ScrollPhysics`](crate::anim::ScrollPhysics)。
//! 即"拖拽 1:1 + 松手切弹簧"由组件层组合,本模块只负责把脏的指针采样流
//! 变成一个干净的速度。
//!
//! # 速度估计(滑动窗口均值,防末端抖动)
//!
//! - 滑动窗口:最多 [`WINDOW_SAMPLES`] 个样本(≈5),且只保留距最新样本
//!   [`WINDOW_MS`] 以内的样本(双约束,先到先淘汰)。
//! - 均值:窗口内**相邻样本对的瞬时速度**取平均;样本对 ≥
//!   [`TRIM_MIN_PAIRS`] 时做修剪(去掉模长最大/最小的各一对)——手指抬起
//!   瞬间的异常大样本只污染一对,修剪后窗口均值不突变(A3 验收)。
//! - 速度单位 **px/s**(与 [`Spring::solve_with_velocity`] 的 v0 同量纲,
//!   直接衔接);时间戳倒流/同刻的样本对跳过。
//!
//! 时间由调用方注入(`t_ms`,通常为帧时钟毫秒),零 `Instant::now()`。

use std::collections::VecDeque;

use kurbo::Point;

/// 窗口样本数上限(≈5,任务书 A3)。
const WINDOW_SAMPLES: usize = 5;
/// 窗口时间跨度上限(ms)。
const WINDOW_MS: f64 = 120.0;
/// 触发修剪均值的最少样本对数(3 对 = 4 样本;修剪掉最大/最小各一对)。
const TRIM_MIN_PAIRS: usize = 3;
/// [`GestureTracker::fling_velocity`] 的最小样本数守卫(不足返回 0)。
const MIN_FLING_SAMPLES: usize = 2;

#[derive(Clone, Copy, Debug)]
struct GestureSample {
    x: f64,
    y: f64,
    t_ms: f64,
}

/// 手势速度采样器(A3):`push_sample` 喂指针流,`velocity`/`fling_velocity`
/// 取滑动窗口均值速度(px/s)。
#[derive(Clone, Debug, Default)]
pub struct GestureTracker {
    samples: VecDeque<GestureSample>,
}

impl GestureTracker {
    /// 空采样器。
    pub fn new() -> Self {
        GestureTracker {
            samples: VecDeque::new(),
        }
    }

    /// 喂入一个指针采样(拖拽进行中每帧/每事件调用)。
    ///
    /// `t_ms` 应单调不减;倒流样本被保留但速度计算时跳过(时间不回退)。
    pub fn push_sample(&mut self, pos: Point, t_ms: f64) {
        self.samples.push_back(GestureSample {
            x: pos.x,
            y: pos.y,
            t_ms,
        });
        // 双约束淘汰:样本数超限或最老样本超出时间窗(均相对最新样本)
        loop {
            let len = self.samples.len();
            if len <= 1 {
                break; // 至少保留刚按入的样本
            }
            let newest = self.samples.back().expect("len >= 2 已检查").t_ms;
            let oldest = self.samples.front().expect("len >= 2 已检查").t_ms;
            if len <= WINDOW_SAMPLES && newest - oldest <= WINDOW_MS {
                break;
            }
            self.samples.pop_front();
        }
    }

    /// 当前滑动窗口均值速度(px/s)。样本不足(无有效样本对)返回 (0, 0)。
    pub fn velocity(&self) -> (f64, f64) {
        let pairs = self.pair_velocities();
        if pairs.is_empty() {
            return (0.0, 0.0);
        }
        if pairs.len() < TRIM_MIN_PAIRS {
            let n = pairs.len() as f64;
            let sum = pairs
                .iter()
                .fold((0.0, 0.0), |(ax, ay), (vx, vy)| (ax + vx, ay + vy));
            return (sum.0 / n, sum.1 / n);
        }
        // 修剪均值:去掉模长最大/最小的各一对(末端抖动单点污染被修剪掉);
        // 全部等模长(稳态)时 max_i == min_i,只去掉一对
        let mut max_i = 0usize;
        let mut min_i = 0usize;
        let mut max_m = -1.0_f64;
        let mut min_m = f64::MAX;
        for (i, &(vx, vy)) in pairs.iter().enumerate() {
            let m = vx * vx + vy * vy; // 模长平方(单调,免开方)
            if m > max_m {
                max_m = m;
                max_i = i;
            }
            if m < min_m {
                min_m = m;
                min_i = i;
            }
        }
        let (mut sum, mut kept) = ((0.0_f64, 0.0_f64), 0_usize);
        for (i, &(vx, vy)) in pairs.iter().enumerate() {
            let trimmed = if min_i == max_i {
                i == max_i
            } else {
                i == max_i || i == min_i
            };
            if !trimmed {
                sum.0 += vx;
                sum.1 += vy;
                kept += 1;
            }
        }
        (sum.0 / kept as f64, sum.1 / kept as f64)
    }

    /// 释放时刻的速度(px/s)——交给
    /// [`Spring::solve_with_velocity`](crate::anim::Spring::solve_with_velocity)
    /// 的 v0。
    ///
    /// 最小样本数守卫:窗口内不足 [`MIN_FLING_SAMPLES`] 个样本(无法构成速度)
    /// 返回 (0, 0),避免误甩。
    pub fn fling_velocity(&self) -> (f64, f64) {
        if self.samples.len() < MIN_FLING_SAMPLES {
            return (0.0, 0.0);
        }
        self.velocity()
    }

    /// 清空(新一轮拖拽开始时调用)。
    pub fn reset(&mut self) {
        self.samples.clear();
    }

    /// 窗口内相邻样本对的瞬时速度(px/s;dt ≤ 0 的对跳过)。
    fn pair_velocities(&self) -> Vec<(f64, f64)> {
        let mut pairs = Vec::new();
        // VecDeque 无 slice 的 windows(2):用相邻迭代对等价实现
        let mut iter = self.samples.iter();
        let mut prev = match iter.next() {
            Some(first) => *first,
            None => return pairs,
        };
        for cur in iter {
            let dt = cur.t_ms - prev.t_ms;
            if dt > 0.0 {
                let dt_s = dt / 1000.0;
                pairs.push(((cur.x - prev.x) / dt_s, (cur.y - prev.y) / dt_s));
            }
            // 同刻/倒流:时间不回退,不计速度,但样本仍前移
            prev = *cur;
        }
        pairs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::anim::Spring;

    /// 匀速拖拽:每 16ms 走 10px → 625 px/s
    fn push_uniform(tracker: &mut GestureTracker, count: usize, step_px: f64) {
        for i in 0..count {
            tracker.push_sample(Point::new(step_px * i as f64, 0.0), 16.0 * i as f64);
        }
    }

    #[test]
    fn constant_velocity_samples_give_constant_velocity() {
        let mut t = GestureTracker::new();
        push_uniform(&mut t, 5, 10.0);
        let (vx, vy) = t.velocity();
        assert!((vx - 625.0).abs() < 1e-9, "10px/16ms = 625px/s,得到 {vx}");
        assert_eq!(vy, 0.0);
        // 继续匀速推进(窗口滚动,老样本被淘汰)后速度不变
        for i in 5..10 {
            t.push_sample(Point::new(10.0 * i as f64, 0.0), 16.0 * i as f64);
        }
        let (vx2, _) = t.velocity();
        assert!((vx2 - 625.0).abs() < 1e-9, "匀速流速度恒定,得到 {vx2}");
    }

    #[test]
    fn anomalous_last_sample_does_not_spike_window_mean() {
        // 前 4 样本匀速(625px/s),最后一样本异常跳 +200px:
        // 4 对速度 = [625, 625, 625, 12500],修剪掉最大/最小 → 均值仍 625
        let mut t = GestureTracker::new();
        push_uniform(&mut t, 4, 10.0);
        t.push_sample(Point::new(230.0, 0.0), 64.0);
        let (vx, _) = t.velocity();
        assert!(
            (vx - 625.0).abs() < 1.0,
            "末端异常样本不应突变窗口均值,得到 {vx}"
        );
        let (fx, _) = t.fling_velocity();
        assert!((fx - 625.0).abs() < 1.0, "释放速度同样免疫末端抖动");
    }

    #[test]
    fn insufficient_samples_return_zero() {
        let mut t = GestureTracker::new();
        assert_eq!(t.velocity(), (0.0, 0.0), "空采样器");
        assert_eq!(t.fling_velocity(), (0.0, 0.0), "0 样本守卫");
        t.push_sample(Point::new(10.0, 5.0), 0.0);
        assert_eq!(t.velocity(), (0.0, 0.0), "1 样本无法构成速度");
        assert_eq!(t.fling_velocity(), (0.0, 0.0), "1 样本 < 最小守卫");
        // 时间窗淘汰:t=200 时 t=0 的样本已超 120ms 窗 → 只剩 1 样本
        t.push_sample(Point::new(50.0, 5.0), 200.0);
        assert_eq!(t.velocity(), (0.0, 0.0), "窗口时间约束淘汰后不足");
    }

    #[test]
    fn duplicate_timestamps_are_skipped() {
        let mut t = GestureTracker::new();
        t.push_sample(Point::new(0.0, 0.0), 16.0);
        t.push_sample(Point::new(10.0, 0.0), 16.0); // 同刻:dt=0 跳过
        assert_eq!(t.velocity(), (0.0, 0.0));
        t.push_sample(Point::new(20.0, 0.0), 32.0);
        let (vx, _) = t.velocity();
        assert!((vx - 625.0).abs() < 1e-9, "有效对参与计算,得到 {vx}");
    }

    #[test]
    fn reset_clears_window() {
        let mut t = GestureTracker::new();
        push_uniform(&mut t, 5, 10.0);
        t.reset();
        assert_eq!(t.velocity(), (0.0, 0.0));
        assert_eq!(t.fling_velocity(), (0.0, 0.0));
    }

    #[test]
    fn vertical_motion_reports_y_velocity() {
        let mut t = GestureTracker::new();
        for i in 0..4 {
            t.push_sample(Point::new(0.0, -8.0 * i as f64), 16.0 * i as f64);
        }
        let (vx, vy) = t.velocity();
        assert!((vx - 0.0).abs() < 1e-9);
        assert!((vy - (-500.0)).abs() < 1e-9, "8px/16ms 向上 = -500px/s");
    }

    #[test]
    fn fling_velocity_feeds_spring_initial_velocity() {
        // 与 A2 的衔接:匀速 500px/s 的手势流,释放速度作为 v0 进弹簧,
        // t→0+ 的导数 ≈ 500(拖拽→弹簧速度不掉速)
        let mut t = GestureTracker::new();
        for i in 0..5 {
            t.push_sample(Point::new(1.0 * i as f64, 0.0), 2.0 * i as f64);
        }
        let (v0, _) = t.fling_velocity();
        assert!((v0 - 500.0).abs() < 1e-9, "1px/2ms = 500px/s,得到 {v0}");
        let spring = Spring::BOUNCY;
        let h = 1e-7;
        let slope = (spring.solve_with_velocity(h, v0) - spring.solve_with_velocity(0.0, v0)) / h;
        assert!(
            (slope - v0).abs() < 0.5,
            "弹簧初速度接续连续性:slope={slope}"
        );
    }
}
