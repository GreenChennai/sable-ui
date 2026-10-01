//! 时间轴工程:`Timeline` + `TimelineHistory` 的撤销一体封装(docs/04 §8)。
//!
//! 预置 2 条视频轨 + 1 条音频轨,clip 用 `SyntheticSource` 彩条假素材
//! (v0.1 不接 ffmpeg,离线可跑)。初始工程内容按"打开工程"语义直接构建
//! (不进撤销栈),此后的一切修改走 [`TimelineProject::exec`]。

use gpui::AppContext as _;
use lumina::gpui;
use lumina::gpui::{App, Context, Entity};
use lumina::video::command::{TimelineCommand, TimelineHistory};
use lumina::video::model::{AssetRef, Timeline, TrackKind};

/// 时间轴工程(权威编辑态)。
pub struct TimelineProject {
    /// 时间轴(Entity 形态:时间轴视图契约要 `Entity<Timeline>`)。
    pub timeline: Entity<Timeline>,
    /// 时间轴撤销栈(lumina_video 自有体系;ClipId 永不复用,无需 id 治愈)。
    pub history: TimelineHistory,
}

impl TimelineProject {
    /// 新建工程并预置轨道与 clip。
    pub fn create(cx: &mut App) -> Entity<Self> {
        let timeline = cx.new(|_| seed_timeline());
        cx.new(|_| Self {
            timeline,
            history: TimelineHistory::new(),
        })
    }

    /// 执行一条时间轴命令(先 apply 后入栈,清 redo)。
    pub fn exec(&mut self, cmd: Box<dyn TimelineCommand>, cx: &mut Context<Self>) {
        let history = &mut self.history;
        self.timeline
            .update(cx, |timeline, _| history.exec(cmd, timeline));
    }

    /// 撤销一步。
    #[allow(dead_code)] // M2 编辑工具栏接线预留
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        let history = &mut self.history;
        self.timeline
            .update(cx, |timeline, _| history.undo(timeline));
    }

    /// 重做一步。
    #[allow(dead_code)] // M2 编辑工具栏接线预留
    pub fn redo(&mut self, cx: &mut Context<Self>) {
        let history = &mut self.history;
        self.timeline
            .update(cx, |timeline, _| history.redo(timeline));
    }
}

/// 预置工程:2 视频轨 + 1 音频轨,若干彩条假 clip(总长 7s)。
fn seed_timeline() -> Timeline {
    let mut timeline = Timeline::new();
    timeline.add_track(TrackKind::Video);
    timeline.add_track(TrackKind::Video);
    timeline.add_track(TrackKind::Audio);

    // SyntheticSource 假素材:AssetRef 只存路径名 + 哈希,预览不真正解码
    let clips = [
        (0usize, "synthetic://colorbars-a", 0u64, 4_000u64),
        (0, "synthetic://colorbars-b", 4_000, 3_000),
        (1, "synthetic://colorbars-c", 2_000, 5_000),
        (2, "synthetic://tone-a", 0, 6_000),
    ];
    for (track, name, start, duration) in clips {
        timeline
            .place_clip(
                track,
                AssetRef::new(name, start + duration),
                start,
                duration,
            )
            .unwrap_or_else(|err| panic!("预置 clip {name} 失败:工程种子数据不得重叠({err})"));
    }
    debug_assert!(timeline.is_valid(), "预置工程必须满足时间轴不变量");
    timeline
}
