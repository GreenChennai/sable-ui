//! A9 动画调度器 AnimScheduler(08 迭代计划 A9;分册六 §4.4 性能军规)。
//!
//! # 职责
//!
//! 统一管理同屏进行中的动画:**同屏上限 [`MAX_CONCURRENT_ANIMS`](32)**
//! (分册六 §4.4),超出时把**最早开始**的动画"合并为即时"(立即视为完成,
//! 其包围盒纳入本帧脏区)——相比 §4.4 原文"超出排队",排队会造成可感知
//! 延迟,此处按 08-A9 裁决改为即时合并(全选 500 对象触发选中脉冲不掉帧)。
//!
//! [`AnimScheduler::advance`] 返回本帧需重绘的脏包围盒:**仍在运行的动画
//! 逐个列出 + 本帧结束/被合并者的包围盒合并为一块**(最终态各画一次,由
//! canvas 脏矩形体系做 union,与 canvas 打通)。动画结束即弹出。
//!
//! [`AnimScheduler::is_idle`] 为真 = 无动画在跑,UI 停止请求帧(运行才请求
//! 帧,静止零帧提交)。时间全部显式注入(`now_ms`),零 `Instant::now()`。

use kurbo::Rect;

/// 同屏动画上限(分册六 §4.4:防"全选 1000 个对象各闪一下"卡死)。
pub const MAX_CONCURRENT_ANIMS: usize = 32;

/// 一个进行中的动画登记项。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveAnim {
    /// 动画标识(组件自定,如实体 id);同 id 重复 begin 视为刷新。
    pub id: u64,
    /// 动画对象的包围盒(本帧脏区来源)。
    pub bbox: Rect,
    /// 开始时刻(ms,调用方时钟)。
    pub started_at_ms: f64,
    /// 结束时刻(ms)。
    pub ends_at_ms: f64,
}

/// 动画调度器(A9):上限内登记、超限合并最早者、按帧产出脏包围盒。
#[derive(Clone, Debug)]
pub struct AnimScheduler {
    active: Vec<ActiveAnim>,
    /// 本帧之前被合并为即时的动画包围盒(union 累积,advance 时一次性输出)。
    pending_dirty: Option<Rect>,
    max_concurrent: usize,
}

impl Default for AnimScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl AnimScheduler {
    /// 新调度器(同屏上限 [`MAX_CONCURRENT_ANIMS`])。
    pub fn new() -> Self {
        AnimScheduler {
            active: Vec::new(),
            pending_dirty: None,
            max_concurrent: MAX_CONCURRENT_ANIMS,
        }
    }

    /// 自定上限的调度器(测试/受限场景);`max = 0` 按 1 处理。
    pub fn with_max_concurrent(max: usize) -> Self {
        AnimScheduler {
            active: Vec::new(),
            pending_dirty: None,
            max_concurrent: max.max(1),
        }
    }

    /// 当前上限。
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    /// 登记一个动画(时长 `duration_ms`,自 `now_ms` 起算)。
    ///
    /// - `duration_ms <= 0` 或任一参数非有限:合并为即时——不占名额,包围盒
    ///   并入下一帧脏区(终态本来就已由组件在本帧画出,无需追加帧);
    /// - 同 `id` 已在跑:刷新(重定起止时刻与包围盒),不占新名额;
    /// - 名额已满:淘汰**最早开始**者(平局取小 id,确定性),其包围盒并入
    ///   下一帧脏区(合并为即时,分册六 §4.4 / 08-A9)。
    pub fn begin(&mut self, id: u64, bbox: Rect, duration_ms: f64, now_ms: f64) {
        if !bbox.is_finite() {
            return; // 非有限包围盒无法绘制,直接拒收(不进脏区)
        }
        if !duration_ms.is_finite() || !now_ms.is_finite() || duration_ms <= 0.0 {
            self.merge_pending(bbox);
            return;
        }
        if let Some(running) = self.active.iter_mut().find(|a| a.id == id) {
            running.bbox = bbox;
            running.started_at_ms = now_ms;
            running.ends_at_ms = now_ms + duration_ms;
            return;
        }
        while self.active.len() >= self.max_concurrent {
            let oldest = self.active.iter().enumerate().min_by(|a, b| {
                a.1.started_at_ms
                    .total_cmp(&b.1.started_at_ms)
                    .then(a.1.id.cmp(&b.1.id))
            });
            match oldest {
                Some((idx, victim)) => {
                    let bbox = victim.bbox;
                    self.active.remove(idx);
                    self.merge_pending(bbox);
                }
                None => break,
            }
        }
        self.active.push(ActiveAnim {
            id,
            bbox,
            started_at_ms: now_ms,
            ends_at_ms: now_ms + duration_ms,
        });
    }

    /// 推进一帧:返回本帧需重绘的脏包围盒(顺序:被合并者的并区 → 本帧
    /// 结束者逐个 → 仍在运行者逐个;数量受 max_concurrent + 1 约束)。
    /// 结束的动画在此弹出。
    pub fn advance(&mut self, now_ms: f64) -> Vec<Rect> {
        let mut dirty = Vec::new();
        if let Some(pending) = self.pending_dirty.take() {
            dirty.push(pending);
        }
        let mut i = 0;
        while i < self.active.len() {
            // NaN now 比较恒 false → 不结束(防御,不 panic)
            if self.active[i].ends_at_ms <= now_ms {
                let done = self.active.remove(i);
                dirty.push(done.bbox); // 终态最后一画
            } else {
                i += 1;
            }
        }
        dirty.extend(self.active.iter().map(|a| a.bbox));
        dirty
    }

    /// 是否空闲(无动画在跑):为真时 UI 停止请求动画帧。
    pub fn is_idle(&self) -> bool {
        self.active.is_empty()
    }

    /// 在跑动画数(≤ max_concurrent)。
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// 某动画是否在跑。
    pub fn is_active(&self, id: u64) -> bool {
        self.active.iter().any(|a| a.id == id)
    }

    /// 并入一块待输出脏区(union 累积)。
    fn merge_pending(&mut self, bbox: Rect) {
        self.pending_dirty = Some(match self.pending_dirty {
            Some(pending) => pending.union(bbox),
            None => bbox,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64) -> Rect {
        Rect::new(x, 0.0, x + 10.0, 10.0)
    }

    #[test]
    fn advance_returns_running_bboxes_and_pops_ended() {
        let mut s = AnimScheduler::new();
        s.begin(1, rect(0.0), 100.0, 0.0);
        s.begin(2, rect(20.0), 200.0, 0.0);
        assert!(!s.is_idle());
        assert_eq!(s.active_count(), 2);
        // 帧中:两个都在跑 → 各自包围盒
        let dirty = s.advance(50.0);
        assert_eq!(dirty, vec![rect(0.0), rect(20.0)]);
        // 1 号到点:弹出 + 终态一画
        let dirty = s.advance(100.0);
        assert_eq!(dirty, vec![rect(0.0), rect(20.0)]);
        assert_eq!(s.active_count(), 1);
        assert!(!s.is_active(1));
        // 2 号到点后空闲
        assert!(!s.advance(300.0).is_empty());
        assert!(s.is_idle(), "全部结束 → idle=true,UI 停止请求帧");
        assert_eq!(s.advance(400.0), vec![] as Vec<Rect>);
    }

    #[test]
    fn evicts_oldest_beyond_capacity_deterministically() {
        let mut s = AnimScheduler::with_max_concurrent(2);
        s.begin(10, rect(0.0), 100.0, 0.0);
        s.begin(20, rect(20.0), 100.0, 10.0);
        s.begin(30, rect(40.0), 100.0, 20.0); // 挤掉最早开始的 10 号
        assert_eq!(s.active_count(), 2);
        assert!(!s.is_active(10));
        // 被合并者的包围盒并入下一帧脏区(pending union 优先)
        let dirty = s.advance(0.0);
        assert_eq!(dirty.first(), Some(&rect(0.0)), "被合并者终态一画");
        assert_eq!(dirty.len(), 3, "合并区 + 2 个在跑");
    }

    #[test]
    fn same_id_refreshes_instead_of_duplicating() {
        let mut s = AnimScheduler::new();
        s.begin(7, rect(0.0), 100.0, 0.0);
        s.begin(7, rect(50.0), 100.0, 50.0);
        assert_eq!(s.active_count(), 1);
        let dirty = s.advance(60.0);
        assert_eq!(dirty, vec![rect(50.0)], "刷新后用新包围盒");
        assert!(!s.advance(150.0).is_empty());
        assert!(s.is_idle());
    }

    #[test]
    fn zero_or_invalid_duration_merges_instantly() {
        let mut s = AnimScheduler::new();
        s.begin(1, rect(5.0), 0.0, 0.0);
        assert!(s.is_idle(), "零时长不占名额");
        let dirty = s.advance(0.0);
        assert_eq!(dirty, vec![rect(5.0)], "终态并入下一帧脏区");
        // NaN 防御
        s.begin(2, rect(9.0), f64::NAN, 0.0);
        assert!(s.is_idle());
        s.begin(3, Rect::new(f64::NAN, 0.0, 1.0, 1.0), 100.0, 0.0);
        assert_eq!(s.active_count(), 0, "非有限包围盒拒收");
    }

    #[test]
    fn five_hundred_objects_keep_frame_dirty_count_bounded() {
        // A9 验收场景:全选 500 对象触发选中脉冲——批量 begin → 逐帧 advance
        let mut s = AnimScheduler::new();
        for i in 0..500_u64 {
            // 每对象错峰 1ms 起播,时长 100ms(f64 无 From<u64>,显式 cast)
            let i_f = i as f64;
            s.begin(i, rect(i_f), 100.0, i_f);
        }
        let mut now = 500.0;
        let mut frames = 0_u32;
        let mut max_dirty = 0_usize;
        while !s.is_idle() {
            let dirty = s.advance(now);
            max_dirty = max_dirty.max(dirty.len());
            assert!(
                dirty.len() <= s.max_concurrent() + 1,
                "帧内脏区数量有界:{}",
                dirty.len()
            );
            now += 16.0;
            frames += 1;
            assert!(frames < 10_000, "调度器未收敛(BUG)");
        }
        assert!(s.is_idle());
        assert!(max_dirty <= 33, "500 对象下每帧脏区有界:{max_dirty}");
    }

    #[test]
    fn five_hundred_same_frame_begins_still_bounded() {
        // 更极端:500 个 begin 挤在同一帧之间(无 advance 排空)
        let mut s = AnimScheduler::new();
        for i in 0..500_u64 {
            s.begin(i, rect(i as f64), 1000.0, 0.0);
        }
        assert_eq!(s.active_count(), 32, "同屏上限 32");
        let dirty = s.advance(0.0);
        assert!(
            dirty.len() <= 33,
            "合并脏区(1)+ 在跑(32)有界,实际 {}",
            dirty.len()
        );
        // 全部跑完后收敛
        let mut now = 0.0;
        while !s.is_idle() {
            now += 16.0;
            s.advance(now);
        }
        assert!(s.is_idle());
    }
}
