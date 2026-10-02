//! A4 FLIP 布局动画(08 迭代计划 A4;分册六 §4.3 #12 "其他行让位 160ms")。
//!
//! First-Last-Invert-Play 的框架无关纯逻辑件:组件每帧报告元素位置
//! ([`FlipTracker::measure`],First/Last——上一帧位置即 First,本帧即 Last),
//! 位置变化后 [`FlipTracker::invert_play`] 给出该元素本帧应加的 y 偏移
//! (Invert:旧→新的 160ms ease 插值,Play:随时间衰减到 0 后自动清除)。
//! 消费方式由组件渲染形态决定:普通布局用 `div().mt(px(offset))`,canvas
//! 绘制用 y 平移;LayerPanel 走前者(uniform_list 的 item 各自是布局根,
//! 行内 margin 只移动本行,不牵动其他行)。
//!
//! # 调用契约(同帧内先 measure 后 invert_play)
//!
//! ```text
//! 每帧渲染:
//!   for 每个元素(含不可见的!):tracker.measure(key, y)   // 更新 First/Last
//!   for 每个可见元素:offset = tracker.invert_play(key, now_ms) → 应用偏移
//! ```
//!
//! 对**全部**元素 measure(而非仅可见者)是虚拟化列表的正确姿势:滚出视口
//! 的行没有渲染,若只 measure 可见行,重排后重新滚入的行会拿陈旧 First
//! 触发假动画。偏移动画只对 invert_play 过的元素播放,滚出视口的动画惰性
//! 过期(下次查询到点即清)。
//!
//! # 语义细节
//!
//! - 偏移方向:`offset = 旧y - 新y`,加在新位置上即"先画在旧位,再滑到新位";
//! - 打断接续:动画进行中再次检出位移,把**当前剩余偏移**与新增偏移叠加后
//!   重新起表(同 [`crate::anim::Animated`] 的"从当前值接续"语义,连续
//!   拖动重排不跳变);
//! - A8:[`reduced_motion`] 为真时偏移恒 0(布局直接落位);
//! - 0.2 版只跟踪 y 轴(消费方——图层列表——是纯垂直列表);任务书草稿的
//!   (x, y) 双轴收窄为 y 单轴,x 轴让位需要时再扩。
//!
//! 时间由调用方注入(`now_ms`),确定性可单测。

use std::collections::HashMap;

use crate::anim::{Easing, reduced_motion};

/// 让位时长(分册六 §4.3 #12:160ms)。
pub const FLIP_DURATION_MS: f64 = 160.0;

/// 让位缓动(ease-out:先行迅速归位,尾部收敛)。
const FLIP_EASE: Easing = Easing::OutCubic;

/// 一条进行中的让位动画。
#[derive(Clone, Copy, Debug, PartialEq)]
struct FlipAnim {
    /// 起始偏移(= 旧y - 新y,含打断时的剩余偏移叠加)。
    from_offset: f64,
    /// 起始时刻(ms,调用方时钟)。
    started_ms: f64,
}

/// FLIP 布局动画跟踪器(A4):记录元素旧位置,位置变化后给出本帧 y 偏移。
#[derive(Clone, Debug, Default)]
pub struct FlipTracker {
    /// 上一帧报告的 y(key → y)。
    prev: HashMap<u64, f64>,
    /// 进行中的让位动画。
    anims: HashMap<u64, FlipAnim>,
    /// 已检出、待 invert_play 消费的位移差(同帧多次检出时累加)。
    pending: HashMap<u64, f64>,
}

impl FlipTracker {
    /// 空跟踪器。
    pub fn new() -> Self {
        FlipTracker::default()
    }

    /// First/Last:组件每帧报告元素 y(所有元素都应报告,含滚出视口的,
    /// 见模块 doc)。位置变化时记下位移差,等 [`Self::invert_play`] 消费。
    pub fn measure(&mut self, key: u64, y: f64) {
        match self.prev.insert(key, y) {
            // NaN != NaN 恒假:NaN 位置不会误触发动画(防御,不 panic)
            Some(old) if old != y => {
                *self.pending.entry(key).or_insert(0.0) += old - y;
            }
            _ => {}
        }
    }

    /// Invert/Play:返回该元素当前应加的 y 偏移(旧→新 160ms ease 插值)。
    ///
    /// - 检出过位移差:以"当前剩余偏移 + 新位移差"为起点起表(打断接续);
    /// - 动画到点或 [`reduced_motion`]:返回 0 并清除该元素的动画;
    /// - 位置从未变化:恒 0(零开销路径)。
    pub fn invert_play(&mut self, key: u64, now_ms: f64) -> f64 {
        if let Some(delta) = self.pending.remove(&key) {
            let base = match self.anims.get(&key) {
                Some(anim) => Self::anim_offset(*anim, now_ms),
                None => 0.0,
            };
            self.anims.insert(
                key,
                FlipAnim {
                    from_offset: base + delta,
                    started_ms: now_ms,
                },
            );
        }
        let Some(anim) = self.anims.get(&key).copied() else {
            return 0.0;
        };
        if reduced_motion() {
            self.anims.remove(&key);
            return 0.0;
        }
        let elapsed = (now_ms - anim.started_ms).max(0.0);
        if elapsed >= FLIP_DURATION_MS {
            self.anims.remove(&key); // 到点清除(任务书"动画结束返回 0 并清除")
            return 0.0;
        }
        Self::anim_offset(anim, now_ms)
    }

    /// 是否仍有让位动画在跑:为真时宿主应继续请求动画帧;到点的动画惰性
    /// 清除(与 invert_play 同一清理规则)。
    pub fn is_animating(&mut self, now_ms: f64) -> bool {
        if reduced_motion() {
            self.anims.clear();
            self.pending.clear();
            return false;
        }
        self.anims
            .retain(|_, anim| (now_ms - anim.started_ms).max(0.0) < FLIP_DURATION_MS);
        !self.anims.is_empty()
    }

    /// 清空全部状态(列表整体重建等场景)。
    pub fn clear(&mut self) {
        self.prev.clear();
        self.anims.clear();
        self.pending.clear();
    }

    /// 时刻 `now_ms` 的偏移:`from_offset × (1 - ease(u))`——u=0 在旧位,
    /// u=1 归新位。纯函数。
    fn anim_offset(anim: FlipAnim, now_ms: f64) -> f64 {
        let elapsed = (now_ms - anim.started_ms).max(0.0);
        let u = (elapsed / FLIP_DURATION_MS).min(1.0);
        anim.from_offset * (1.0 - FLIP_EASE.apply(u))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROW: f64 = 28.0;

    #[test]
    fn two_rows_swapping_slide_from_full_row_height_to_zero() {
        // A4 验收:两行交换位置 → 偏移从 ±行高 衰减到 0(160ms)
        let mut t = FlipTracker::new();
        t.measure(1, 0.0);
        t.measure(2, ROW);
        // 交换
        t.measure(1, ROW);
        t.measure(2, 0.0);
        let now = 1000.0;
        let off1 = t.invert_play(1, now);
        let off2 = t.invert_play(2, now);
        assert_eq!(off1, -ROW, "行 1:旧 0 新 28 → 偏移 -28(画在旧位)");
        assert_eq!(off2, ROW);
        assert!(t.is_animating(now));
        // 中点:OutCubic(0.5) = 0.875 → 剩余 12.5%
        assert!((t.invert_play(1, now + 80.0) + ROW * 0.125).abs() < 1e-9);
        // 160ms 到点归 0 并清除
        assert_eq!(t.invert_play(1, now + 160.0), 0.0);
        assert_eq!(t.invert_play(1, now + 400.0), 0.0, "清除后恒 0");
        assert!(!t.is_animating(now + 400.0), "全部到点后停止续帧");
    }

    #[test]
    fn unchanged_keys_never_offset() {
        let mut t = FlipTracker::new();
        t.measure(7, 42.0);
        t.measure(7, 42.0);
        assert_eq!(t.invert_play(7, 0.0), 0.0);
        assert_eq!(t.invert_play(7, 80.0), 0.0);
        assert!(!t.is_animating(80.0));
        // 新元素首次出现不算位移(无旧位置可比)
        t.measure(8, 10.0);
        assert_eq!(t.invert_play(8, 5.0), 0.0);
    }

    #[test]
    fn reduced_motion_collapses_to_zero() {
        crate::anim::set_reduced_motion(true);
        let mut t = FlipTracker::new();
        t.measure(1, 0.0);
        t.measure(2, ROW);
        t.measure(1, ROW); // 交换
        assert_eq!(t.invert_play(1, 0.0), 0.0, "减弱动态:布局直接落位");
        assert!(!t.is_animating(0.0));
        crate::anim::set_reduced_motion(false);
    }

    #[test]
    fn interrupt_while_playing_adds_remaining_offset() {
        // 连续重排:动画中再次检出位移 → 剩余偏移 + 新位移差 一起接续
        let mut t = FlipTracker::new();
        t.measure(1, 0.0);
        t.measure(1, ROW); // 下移一格:偏移 -28
        let now = 1000.0;
        assert_eq!(t.invert_play(1, now), -ROW);
        // 80ms 后(动画剩 12.5% = -3.5)又下移一格:接续 -3.5 + (-28)
        t.measure(1, 2.0 * ROW);
        let resumed = t.invert_play(1, now + 80.0);
        assert!(
            (resumed - (ROW * -0.125 - ROW)).abs() < 1e-9,
            "剩余偏移叠加后接续,得到 {resumed}"
        );
        // 从新的起点完整走完 160ms
        assert!((t.invert_play(1, now + 80.0 + 80.0) - (resumed * 0.125)).abs() < 1e-9);
        assert_eq!(t.invert_play(1, now + 240.0 + 80.0), 0.0);
    }

    #[test]
    fn pending_measures_without_play_accumulate() {
        // 两次 measure 都未 invert_play(元素滚出视口):位移差累加,滚回
        // 视口时一次性从正确旧位滑入
        let mut t = FlipTracker::new();
        t.measure(3, 0.0);
        t.measure(3, ROW);
        t.measure(3, 2.0 * ROW);
        assert_eq!(t.invert_play(3, 1000.0), -2.0 * ROW);
    }

    #[test]
    fn clear_resets_everything() {
        let mut t = FlipTracker::new();
        t.measure(1, 0.0);
        t.measure(1, ROW);
        let _ = t.invert_play(1, 0.0);
        t.clear();
        assert_eq!(t.invert_play(1, 100.0), 0.0, "清空后旧动画不再续");
        assert!(!t.is_animating(100.0));
        // clear 后同一 key 重新出现按新元素处理(无假动画)
        t.measure(1, ROW);
        assert_eq!(t.invert_play(1, 200.0), 0.0);
    }

    #[test]
    fn clock_rollback_clamps_to_start() {
        let mut t = FlipTracker::new();
        t.measure(1, 0.0);
        t.measure(1, ROW);
        let now = 1000.0;
        let _ = t.invert_play(1, now);
        // now_ms 倒流:钳在起始时刻(不产生超前偏移,不 panic)
        assert_eq!(t.invert_play(1, now - 50.0), -ROW);
    }
}
