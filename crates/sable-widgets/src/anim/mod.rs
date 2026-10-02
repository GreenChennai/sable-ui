//! 动画引擎(分册六 §4,S1 迭代计划 08 号文件 A 系列:动画与视觉效果专篇)。
//!
//! # 三层动画模型(分册六 §4.1)
//!
//! ```text
//! L1 微交互层:hover/按压/面板展开 —— Tween:Animated + Easing(120~250ms)
//! L2 物理层:拖拽惯性/滚动回弹/面板吸附 —— Spring + GestureTracker + ScrollPhysics
//! L3 内容层:UI 编排 AnimationTimeline;关键帧数据模型在 sable-video(分册四 §9)
//! ```
//!
//! 各子模块:A1 编排 [`timeline`]、A2 弹簧初速度 [`spring`]、A3 手势 [`gesture`]、
//! A5 滚动物理 [`scroll`]、A6 颜色插值 [`color`]、A8 减弱动态(本模块)、
//! A9 调度 [`scheduler`]、A10 可插值类型 [`lerp`]、A12 黄金值快照(各模块测试)。
//!
//! # 纯逻辑,不引 cx(分册六 §4.4 性能军规)
//!
//! 时间一律由调用方显式注入(`*_at` 系列收 `Instant` 或 `f64` 毫秒),新增模块
//! 零 `Instant::now()`,确定性可单测。动画运行才请求帧、静止零帧提交:组件层以
//! [`Animated::is_running_at`] / [`AnimScheduler::is_idle`] 自行判断。
//!
//! # A8 减弱动态效果(库侧开关)
//!
//! [`set_reduced_motion`] / [`reduced_motion`]:置位后**一切 value_at 立即返回
//! 目标值**(`set` 不变,值直通 `to`),`is_running_at` 恒为假(UI 随之停止请求帧)。
//! 生效点:[`Animated::value_at`]、[`AnimationTimeline::value_at`]/[`AnimationTimeline::sample_at`]、
//! [`ScrollPhysics::offset_at`];直接采样弹簧的组件应在调用
//! [`Spring::solve_with_velocity`] 前自行检查 [`reduced_motion`] 取终值。
//! 启动时的系统探测(Windows:SPI_GETCLIENTAREAANIMATION /
//! `SystemParametersInfoW`;accesskit 亦可查询)由后续波接入 sable-dock,
//! 本库只提供开关与语义。
//!
//! # 与分册六 §4.2 原文的适配(沿 M0 review 结论)
//!
//! 1. `Easing` 用自有枚举 + 解析求值(CSS cubic-bezier 牛顿迭代),不经 kurbo
//!    三次贝塞尔的隐式参数化(见 [`easing`] 模块文档);
//! 2. `Animated` 的 `*_at(now)` 显式时间版本为主入口,`value()`/`set()`/`is_running()`
//!    是 `Instant::now()` 的便捷封装(唯一保留的 `Instant::now()`,历史 API)。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub mod color;
pub mod easing;
pub mod gesture;
pub mod lerp;
pub mod scheduler;
pub mod scroll;
pub mod spring;
pub mod timeline;

pub use color::lerp_hsla;
pub use easing::{Easing, cubic_bezier_y};
pub use gesture::GestureTracker;
pub use lerp::Lerp;
pub use scheduler::{ActiveAnim, AnimScheduler, MAX_CONCURRENT_ANIMS};
pub use scroll::ScrollPhysics;
pub use spring::Spring;
pub use timeline::{AnimationTimeline, Segment, TimelineEntry};

/// A8 全局开关(`false` = 正常动画)。`Relaxed`:单布尔,无跨变量排序需求,
/// 最终可见即可(下一帧生效)。
static REDUCED_MOTION: AtomicBool = AtomicBool::new(false);

/// 开启/关闭"减弱动态效果"(A8)。幂等;进程级全局,由宿主(dock 层)在
/// 启动时按系统设置调用。
pub fn set_reduced_motion(on: bool) {
    REDUCED_MOTION.store(on, Ordering::Relaxed);
}

/// 当前是否处于"减弱动态效果"(A8)。为真时一切 value_at 直通终值。
pub fn reduced_motion() -> bool {
    REDUCED_MOTION.load(Ordering::Relaxed)
}

/// 一个动画值:组件持有它,每帧读 [`Animated::value`](分册六 §4.2)。
///
/// `set` 打断进行中的动画时**从当前值接续**(from = 此刻 value),视觉无跳变。
/// 时间注入风格:`*_at` 系列显式收 `Instant`(单测确定性/与帧时钟同步),
/// `set`/`value`/`is_running` 为 `Instant::now()` 便捷封装。
///
/// A8:[`reduced_motion`] 为真时 [`Animated::value_at`] 直通目标值、
/// [`Animated::is_running_at`] 恒假。
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
    ///
    /// A8:`reduced_motion()` 为真时立即返回目标值(直通,不经插值)。
    pub fn value_at(&self, now: Instant) -> T {
        if reduced_motion() {
            return self.to.clone();
        }
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
    ///
    /// A8:`reduced_motion()` 为真时恒假(动画即时化,UI 停止请求帧)。
    pub fn is_running_at(&self, now: Instant) -> bool {
        !reduced_motion() && now.saturating_duration_since(self.started) < self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn reduced_motion_flag_roundtrip() {
        // 进程级全局:置位/复位必须成对,窗口内不做其他断言以外的操作。
        // (并行测试读到瞬时 true 的概率为纳秒级窗口,可忽略;纪律:本测试
        // 之外的任何测试不得遗留开关为 true。)
        set_reduced_motion(true);
        assert!(reduced_motion());
        set_reduced_motion(false);
        assert!(!reduced_motion());
    }

    #[test]
    fn reduced_motion_makes_value_at_jump_to_target() {
        // A8 语义:set 不变,value_at 直通 to,is_running_at 恒假。
        set_reduced_motion(true);
        let mut anim = Animated::new(0.0_f64);
        // 10s 时长:复位断言只要求 elapsed ∈ (0, 10s),CI 线程毛刺也不翻车
        anim.set_at(
            100.0,
            Instant::now(),
            Duration::from_secs(10),
            Easing::InCubic,
        );
        let any_time = Instant::now() + Duration::from_millis(123);
        assert_eq!(anim.value_at(any_time), 100.0, "减弱动态:直通目标值");
        assert!(!anim.is_running_at(any_time), "减弱动态:不再运行");
        set_reduced_motion(false);
        // 复位后恢复正常插值(set_at 时 to 仍是初值 0,故 from=0、目标 100 照常推进)
        let mid = Instant::now() + Duration::from_millis(500);
        let v = anim.value_at(mid);
        assert!(v > 0.0 && v < 100.0, "复位后应回到正常插值,得到 {v}");
    }
}
