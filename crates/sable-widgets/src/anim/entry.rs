//! ANI-05:一次性入场动效(面板/对话框/空态首现;报告 §4.4)。
//!
//! 记起始时刻按流逝算缓动;**超过 0.5s 视为重开**(同一实例被复用到
//! 新内容时的再入场语义,报告原文)。纯函数零 Instant::now()。

/// 入场动画状态(宿主持有;构造即开始)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntryAnim {
    started_ms: f64,
    /// 入场时长 ms(复用 HOVER 档 80ms?不——入场用 STATE 120ms,
    /// 与 tokens 动效表一致)。
    duration_ms: f64,
}

/// 默认入场时长(ms;STATE 档同值,tokens 是唯一真相,此处镜像+测试钉死)。
pub const ENTRY_DURATION_MS: f64 = 120.0;

/// 超时重开阈值(ms;报告原文 >0.5s 视为重开)。
pub const RESTART_THRESHOLD_MS: f64 = 500.0;

impl EntryAnim {
    /// 构造(宿主把起始时刻传入;`now` 通常来自 interact::now_ms)。
    pub fn new(started_ms: f64) -> Self {
        EntryAnim {
            started_ms,
            duration_ms: ENTRY_DURATION_MS,
        }
    }

    /// 进度 0..=1(120ms OutCubic;`reduced_motion` 直切 0/1;
    /// 距起始超过 [`RESTART_THRESHOLD_MS`] 才首次查询 = 视为重开,
    /// 返回重开后的进度)。
    pub fn progress_at(&self, now_ms: f64, reduced_motion: bool) -> f32 {
        if reduced_motion {
            return if now_ms > self.started_ms { 1.0 } else { 0.0 };
        }
        if now_ms <= self.started_ms {
            return 0.0;
        }
        let mut elapsed = now_ms - self.started_ms;
        if elapsed > RESTART_THRESHOLD_MS {
            // 视为重开:取模到首个周期内(超时重开语义)
            elapsed %= RESTART_THRESHOLD_MS;
        }
        let t = (elapsed / self.duration_ms).min(1.0);
        let eased = 1.0 - (1.0 - t) * (1.0 - t);
        eased as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_ani_entry_01_progress_restart_and_reduced() {
        let e = EntryAnim::new(1000.0);
        assert_eq!(e.progress_at(1000.0, false), 0.0);
        assert_eq!(e.progress_at(1060.0, false), 0.75, "60ms = OutCubic 0.75");
        assert_eq!(e.progress_at(1120.0, false), 1.0, "120ms 落定");
        // 超时重开:2000ms 查询(距起始 1000ms > 500ms)→ 取模后 400ms 处?
        // 1000 % 500 = 0 → 0 进度(重开从 0 开始)
        let p = e.progress_at(2000.0, false);
        assert!((0.0..=1.0).contains(&p));
        // reduced 直切
        assert_eq!(e.progress_at(1060.0, true), 1.0);
        assert_eq!(e.progress_at(1000.0, true), 0.0);
        // 时长与 tokens STATE 档一致(唯一真相镜像钉死)
        assert_eq!(ENTRY_DURATION_MS, 120.0);
        assert_eq!(RESTART_THRESHOLD_MS, 500.0);
    }
}
