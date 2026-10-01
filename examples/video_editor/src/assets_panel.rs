//! 素材占位面板(M0):列出工程预置的合成假素材名,拖入时间轴等交互留 M4
//! (分册四 §10 AssetPanel 的完整版归 widgets)。

use gpui::{
    App, AppContext as _, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, div, px,
};
use lumina::gpui;

use crate::palette::Palette;

/// 素材占位面板。
pub struct AssetsPanel {
    items: Vec<SharedString>,
}

impl AssetsPanel {
    /// 构造(与 project.rs 的预置 clip 名一一对应)。
    pub fn new(cx: &mut App) -> Entity<Self> {
        let items = vec![
            SharedString::from("synthetic://colorbars-a"),
            SharedString::from("synthetic://colorbars-b"),
            SharedString::from("synthetic://colorbars-c"),
            SharedString::from("synthetic://tone-a"),
        ];
        cx.new(|_| Self { items })
    }
}

impl Render for AssetsPanel {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let palette = Palette::get(cx);
        let rows: Vec<_> = self
            .items
            .iter()
            .map(|name| {
                div()
                    .flex()
                    .items_center()
                    .px_2()
                    .h(px(26.))
                    .text_color(palette.text_secondary)
                    .child(name.clone())
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .size_full()
            .p_2()
            .gap_1()
            .bg(palette.surface_1)
            .text_color(palette.text_primary)
            .child(
                div()
                    .px_2()
                    .text_color(palette.text_secondary)
                    .child("素材(SyntheticSource 假素材)"),
            )
            .children(rows)
    }
}
