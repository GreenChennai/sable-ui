//! 组件 story 示例(分册五 §3"每个组件一个示例"的聚合形态):
//! 单窗口滚动长页,按分组展示 Sable 全部专用组件。
//!
//! 运行:`cargo run -p story`
//!
//! # 组装形态
//!
//! `Application::new().run(...)` → `cx.open_window(WindowOptions, ...)` →
//! 纯 gpui div 根视图(不经 gpui-component `Root`:本示例刻意只演示
//! sable 自有组件与 token,不引 dock/主题耦合——gpui 0.2.2 根视图
//! 无需额外包装,示例形态已在 vector_editor/video_editor 核实)。
//!
//! # 页面分组(任务 4.3)
//!
//! 1. NumberField(编辑态/拖拽/单位/范围)+ 撤销步数;
//! 2. ColorWell + ColorWheel(绑定同色);
//! 3. GradientEditor(渐变 stops 编辑);
//! 4. LayerPanel(20 行假场景)+ FlipTracker 让位动画(上移/下移触发);
//! 5. TimelineView(假 Timeline)+ CurvePreview;
//! 6. 动画:HoverState / PulseState / 减弱动态开关 / Spring 对比条;
//! 7. 效果:DropShadow/Glow/ColorMatrix 离屏渲染上屏;
//! 8. token:5 级 ELEVATIONS 阴影卡 + 色板(深/浅过渡切换 + inject
//!    自定义 accent 注入演示);
//! 9. Neon Card(对标 luminaui.in):流动渐变描边/光标辉光/粒子三卡演示。

mod app;
mod inputs;
mod motion;
mod neon;
mod panels;
mod pixels;
mod ui;

use sable::gpui::{App, AppContext as _, Bounds, WindowBounds, WindowOptions, px, size};

fn main() {
    sable::gpui::Application::new().run(|cx: &mut App| {
        // 主题(全局 token;幂等)
        sable::widgets::theme::init(cx);

        let bounds = Bounds::centered(None, size(px(1180.), px(920.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(sable::gpui::TitlebarOptions {
                title: Some("Sable · 组件 story".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        cx.open_window(options, |_window, cx| {
            // 根视图组装无需窗口句柄(焦点在各组件自身)
            cx.new(app::StoryApp::new)
        })
        .expect("story 主窗口打开失败:gpui 平台层初始化异常(显卡驱动/显示服务)");

        cx.activate(true);
    });
}
