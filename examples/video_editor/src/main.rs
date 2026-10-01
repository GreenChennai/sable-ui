//! 迷你剪映示例(M0 壳,分册五 §1 任务 0.2 的视频侧):
//! 窗口 + gpui_component Root + DockArea(左素材占位/中预览监视器/下时间轴)
//! + 播放泵 + 时间轴撤销,全离线(SyntheticSource,不引 ffmpeg)。
//!
//! 运行:`cargo run -p video_editor`

mod app;
mod assets_panel;
mod palette;
mod player_view;
mod project;
mod timeline_host;

use lumina::gpui;
use lumina::gpui::{App, AppContext as _, Bounds, WindowBounds, WindowOptions, px, size};
use lumina::gpui_component::Root;

fn main() {
    gpui::Application::new().run(|cx: &mut App| {
        lumina::dock::init(cx);

        let bounds = Bounds::centered(None, size(px(1360.), px(860.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(lumina::gpui::TitlebarOptions {
                title: Some("Lumina · 迷你剪映(M0)".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        cx.open_window(options, |window, cx| {
            let editor = app::build_video_editor(window, cx);
            // editor: Entity<VideoApp>,经 gpui 的 From<Entity<V: Render>> for AnyView 进 Root::new
            cx.new(|cx| Root::new(editor, window, cx))
        })
        .expect("主窗口打开失败:gpui 平台层初始化异常(显卡驱动/显示服务)");

        cx.activate(true);
    });
}
