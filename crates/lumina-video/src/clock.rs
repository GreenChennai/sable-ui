//! 播放时钟(docs/03 §7):纯状态机,不含任何线程/timer。
//!
//! UI 泵(`timer()` 驱动、16ms 一拍)留在 widgets/示例层:泵测量真实
//! dt 后调用 [`Player::tick`],UI 只订阅"时间码变化"。解码线程同样不在
//! 本 crate(见 [`crate::frame::FrameSource`])。

use serde::{Deserialize, Serialize};

/// 播放头状态。字段公开,UI 可直接读;修改走方法以保证状态一致。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Player {
    pub position_ms: u64,
    pub playing: bool,
    /// 播放速率(> 0;<= 0 在 tick 中按 0 处理,v0.1 不支持倒放)。
    pub speed: f64,
}

impl Default for Player {
    fn default() -> Self {
        Player {
            position_ms: 0,
            playing: false,
            speed: 1.0,
        }
    }
}

impl Player {
    pub fn new() -> Self {
        Self::default()
    }

    /// 推进一帧(纯函数,由 UI 泵每拍调用):
    /// `position += dt * speed` 取整;到达 `duration_ms` 尽头自动停
    /// (位置钳到时长、`playing = false`)。暂停/时长为 0 时不推进。
    pub fn tick(&mut self, dt_ms: u64, duration_ms: u64) {
        if !self.playing {
            return;
        }
        let advance = if self.speed > 0.0 {
            (dt_ms as f64 * self.speed).round() as u64
        } else {
            0
        };
        let next = self.position_ms.saturating_add(advance);
        if duration_ms == 0 || next >= duration_ms {
            self.position_ms = duration_ms;
            self.playing = false;
        } else {
            self.position_ms = next;
        }
    }

    /// 跳转(负值在类型层就不可能:u64)。
    pub fn seek(&mut self, ms: u64) {
        self.position_ms = ms;
    }

    pub fn play(&mut self) {
        self.playing = true;
    }

    pub fn pause(&mut self) {
        self.playing = false;
    }

    pub fn toggle(&mut self) {
        self.playing = !self.playing;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_advances_by_speed() {
        let mut p = Player::new();
        p.play();
        p.tick(100, 10_000);
        assert_eq!(p.position_ms, 100);
        p.speed = 2.0;
        p.tick(100, 10_000);
        assert_eq!(p.position_ms, 300, "100ms × 2x = 200ms");
        p.speed = 0.5;
        p.tick(100, 10_000);
        assert_eq!(p.position_ms, 350, "100ms × 0.5x = 50ms");
        assert!(p.playing);
    }

    #[test]
    fn paused_player_does_not_advance() {
        let mut p = Player {
            position_ms: 500,
            ..Player::new()
        };
        p.tick(100, 10_000);
        assert_eq!(p.position_ms, 500, "暂停时不推进");
        p.play();
        p.pause();
        p.tick(100, 10_000);
        assert_eq!(p.position_ms, 500);
        assert!(!p.playing);
    }

    #[test]
    fn stops_at_duration_end_and_stays() {
        let mut p = Player::new();
        p.play();
        p.tick(400, 1000);
        assert_eq!(p.position_ms, 400);
        assert!(p.playing);
        p.tick(500, 1000);
        assert_eq!(p.position_ms, 900);
        assert!(p.playing);
        // 跨过尽头:钳到时长并自动停
        p.tick(500, 1000);
        assert_eq!(p.position_ms, 1000);
        assert!(!p.playing, "尽头自动停");
        // 之后继续 tick 不再变化,也不会重启
        p.tick(100, 1000);
        assert_eq!(p.position_ms, 1000);
        assert!(!p.playing);
    }

    #[test]
    fn zero_duration_or_zero_speed_never_plays() {
        let mut p = Player::new();
        p.play();
        p.tick(100, 0);
        assert_eq!(p.position_ms, 0);
        assert!(!p.playing, "空时间轴(时长 0)一拍即停");

        let mut p2 = Player::new();
        p2.play();
        p2.speed = 0.0;
        p2.tick(100, 1000);
        assert_eq!(p2.position_ms, 0, "速度 0 不推进但保持播放态");
        assert!(p2.playing);
    }

    #[test]
    fn seek_play_pause_toggle() {
        let mut p = Player::new();
        p.seek(4321);
        assert_eq!(p.position_ms, 4321);
        // u64 类型层面拒绝负值(编译期保证)
        p.toggle();
        assert!(p.playing);
        p.toggle();
        assert!(!p.playing);
        p.play();
        assert!(p.playing);
        p.pause();
        assert!(!p.playing);
    }

    #[test]
    fn player_state_serializes() {
        let p = Player {
            position_ms: 1234,
            playing: true,
            speed: 1.5,
        };
        let json = serde_json::to_string(&p).expect("serialize");
        let back: Player = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, p);
    }
}
