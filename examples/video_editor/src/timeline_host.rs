//! 时间轴宿主:widgets `TimelineView` 挂进底部 dock。
//!
//! widgets 真实签名(2026-10 以 crates/lumina-widgets/src/timeline_view.rs
//! 为准):`TimelineView::new(Entity<Timeline>)`(无 cx)+ 消费式 builder
//! `.on_seek(Fn(u64, &mut App))` / `.on_move_clip(Fn(ClipId, u64, &mut App))`
//! / `.on_select_clip(Fn(ClipId, &mut App))`;播放头红线经
//! `set_playhead(u64)` 由应用层回推。clip 拖拽已由组件换算+吸附,
//! 碰撞裁决在应用层(走 `TimelineHistory::exec(MoveClip)`)。

use gpui::{
    App, AppContext as _, Entity, IntoElement, ParentElement as _, Render, Styled as _, div,
};
use lumina::gpui;
use lumina::gpui::WeakEntity;
use lumina::video::command::MoveClip;
use lumina::video::model::ClipId;
use lumina::widgets::timeline_view::TimelineView;

use crate::player_view::PreviewMonitor;
use crate::project::TimelineProject;

/// 时间轴宿主。
pub struct TimelineHost {
    panel: Entity<TimelineView>,
    #[allow(dead_code)] // M0:undo/redo 入口尚未上工具栏,保留句柄待 M4 接线
    project: Entity<TimelineProject>,
    monitor: WeakEntity<PreviewMonitor>,
}

impl TimelineHost {
    /// 构造宿主并挂接 seek / move_clip 回调(选中回调 v0.1 不接,默认空操作)。
    pub fn new(
        project: &Entity<TimelineProject>,
        monitor: &Entity<PreviewMonitor>,
        cx: &mut App,
    ) -> Entity<Self> {
        let timeline = project.read(cx).timeline.clone();

        let timeline_for_move = project.clone();
        let monitor_for_seek = monitor.downgrade();
        let panel = cx.new(|_| {
            TimelineView::new(timeline)
                .on_seek(move |ms: u64, cx: &mut App| {
                    if let Some(monitor) = monitor_for_seek.upgrade() {
                        monitor.update(cx, |monitor, cx| monitor.seek(ms, cx));
                    }
                })
                .on_move_clip(move |id: ClipId, to_ms: u64, cx: &mut App| {
                    timeline_for_move.update(cx, |project, cx| {
                        project.exec(
                            Box::new(MoveClip {
                                id,
                                from_ms: None,
                                to_ms,
                            }),
                            cx,
                        );
                    });
                })
        });

        cx.new(|cx| {
            // 监视器每拍 notify(播放/seek)→ 本宿主同步播放头红线
            cx.observe(monitor, |_, _, cx| cx.notify()).detach();
            Self {
                panel,
                project: project.clone(),
                monitor: monitor.downgrade(),
            }
        })
    }
}

impl Render for TimelineHost {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        // 播放头真相在 PreviewMonitor 的 Player,回推时间轴画红线
        let playhead = self
            .monitor
            .upgrade()
            .map(|monitor| monitor.read(cx).position_ms())
            .unwrap_or(0);
        self.panel
            .update(cx, |panel, _| panel.set_playhead(playhead));
        div().size_full().child(self.panel.clone())
    }
}
