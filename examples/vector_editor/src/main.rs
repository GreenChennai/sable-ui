//! 迷你 Illustrator 示例(M0 壳,分册五 §1 任务 0.2):
//! 窗口 + gpui_component Root + Dock 三段式 + Vello 无限画布 +
//! 选择/钢笔/抓手 + 属性检查器 + 撤销重做。
//!
//! 运行:`cargo run -p vector_editor`
//!
//! # 组装样板来源
//!
//! `Application::new().run(...)` → `cx.open_window(WindowOptions, |window, cx|
//! 业务视图)` → `gpui_component::Root` 包根 —— gpui 0.2.2 与 gpui-component
//! 0.7.0 story 示例的公共形态(本地 registry 源码核实)。

mod app;
mod document;
mod inspector;
mod layers;
mod palette;
mod seed;

use sable::gpui;
use sable::gpui::{App, AppContext as _, Bounds, WindowBounds, WindowOptions, px, size};
use sable::gpui_component::Root;

fn main() {
    gpui::Application::new().run(|cx: &mut App| {
        // 主题 + dock 子系统(幂等;内部含 gpui_component::init 与 widgets 主题)
        sable::dock::init(cx);
        app::bind_keys(cx);

        let bounds = Bounds::centered(None, size(px(1360.), px(860.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..app::window_options()
        };

        cx.open_window(options, |window, cx| {
            let editor = app::build_editor(window, cx);
            // gpui-component 0.7:窗口根 = gpui_base::Root(经 gpui_component
            // 再导出),业务视图挂其内;tooltip/对话框等插件浮层由 Root 挂载
            // (其 root::init 已在 sable::dock::init 里注册)。
            cx.new(|cx| Root::new(editor, window, cx))
        })
        .expect("主窗口打开失败:gpui 平台层初始化异常(显卡驱动/显示服务)");

        cx.activate(true);
    });
}
