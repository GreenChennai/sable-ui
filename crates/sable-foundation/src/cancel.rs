//! RBT-10:长任务"可取消 + 超时"原语(§6 RB-04;导出/缩略图/解码的
//! 统一协作式取消契约)。纯 foundation,零依赖,单测钉死。

use std::time::Instant;

/// 协作式取消令牌:长任务在**检查点**轮询 [`CancelToken::is_cancelled`],
/// 取消延迟 < 1s 由检查点密度保证(契约见模块 doc;库内长任务每节点/
/// 每帧一个检查点)。克隆共享同一布尔(Arc)。
#[derive(Clone, Default)]
pub struct CancelToken {
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl CancelToken {
    /// 新令牌(未取消)。
    pub fn new() -> Self {
        Self::default()
    }

    /// 触发取消(幂等;已启动的任务在下个检查点退出,产物不留半截)。
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// 是否已取消。
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// 截止时间(RBT-10 超时半边):长任务构造时给定,检查点轮询
/// [`Deadline::expired`];超时上报阶段名由调用方携带。
#[derive(Clone, Copy, Debug)]
pub struct Deadline {
    start: Instant,
    limit_ms: u64,
}

impl Deadline {
    /// 从现在起限时 `limit_ms`。
    pub fn within(limit_ms: u64) -> Self {
        Deadline {
            start: Instant::now(),
            limit_ms,
        }
    }

    /// 是否已超时。
    pub fn expired(&self) -> bool {
        self.start.elapsed().as_millis() as u64 >= self.limit_ms
    }

    /// 已流逝毫秒。
    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_rbt_cancel_01_token_and_deadline_semantics() {
        // 取消:默认未取消;触发后可见(克隆共享);幂等
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
        let clone = token.clone();
        token.cancel();
        assert!(token.is_cancelled());
        assert!(clone.is_cancelled(), "克隆共享同一布尔");
        token.cancel();
        assert!(token.is_cancelled(), "幂等");

        // 截止:限定 30ms,sleep 60ms → 过期;elapsed 单调且 ≥30
        let d = Deadline::within(30);
        assert!(!d.expired(), "构造即查不过期(时序裕度)");
        std::thread::sleep(std::time::Duration::from_millis(60));
        assert!(d.expired());
        assert!(d.elapsed_ms() >= 30);
    }
}
