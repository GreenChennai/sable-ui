//! 宿主组装:窗口 + Root + DockArea(左素材占位/中预览监视器/下时间轴)。
//!
//! 三段式预设覆盖不到底部 dock,构建后追加 `set_bottom_dock(DockItem,
//! size, open, ..)`(gpui-component 0.5.1 API;tab 组经 lumina-dock
//! `tab_group(panels, &WeakEntity<DockArea>, ..)` 描述)。

use lumina::dock::{LuminaPanel, WorkspacePresets};
use lumina::gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Styled as _, Window, div, px,
};
use lumina::gpui_component::dock::DockArea;

use crate::assets_panel::AssetsPanel;
use crate::palette::Palette;
use crate::player_view::PreviewMonitor;
use crate::project::TimelineProject;
use crate::timeline_host::TimelineHost;

/// 视频编辑器业务根视图。
pub struct VideoApp {
    #[allow(dead_code)] // M4 时间轴 undo/redo 接线时使用
    project: Entity<TimelineProject>,
    #[allow(dead_code)] // 传输条在监视器内部;M4 工具栏接播放控制时使用
    monitor: Entity<PreviewMonitor>,
    dock: Entity<DockArea>,
    focus: FocusHandle,
}

/// 组装业务根(在 `cx.open_window` 的 build 闭包里调用,&mut App 阶段)。
pub fn build_video_editor(window: &mut Window, cx: &mut App) -> Entity<VideoApp> {
    // 1. 工程(预置 2 视频轨 + 1 音频轨假 clip)
    let project = TimelineProject::create(cx);

    // 2. 预览监视器(内部自启 16ms 播放泵)
    let timeline = project.read(cx).timeline.clone();
    let monitor = cx.new(|cx| PreviewMonitor::new(timeline, cx));

    // 3. 三段式(左素材 / 中预览;右 dock 留空)
    let left = vec![LuminaPanel::create("素材", AssetsPanel::new(cx).into(), cx)];
    let center = LuminaPanel::create("预览", monitor.clone().into(), cx);
    let dock =
        WorkspacePresets::build_workspace("lumina-video-editor", left, center, vec![], window, cx);

    // 4. 底部时间轴 dock(0.5.1:set_bottom_dock 直接收 size/open)
    let timeline_panel = LuminaPanel::create(
        "时间轴",
        TimelineHost::new(&project, &monitor, cx).into(),
        cx,
    );
    dock.update(cx, |area, cx| {
        let dock_area = cx.entity().downgrade();
        area.set_bottom_dock(
            lumina::dock::tab_group(vec![timeline_panel], &dock_area, window, cx),
            Some(px(220.)),
            true,
            window,
            cx,
        );
    });

    let focus = cx.focus_handle();
    window.focus(&focus);
    cx.new(|_| VideoApp {
        project,
        monitor,
        dock,
        focus,
    })
}

impl Focusable for VideoApp {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for VideoApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::get(cx);
        div()
            .id("video-root")
            .size_full()
            .bg(palette.surface_0)
            .text_color(palette.text_primary)
            .child(self.dock.clone())
    }
}
