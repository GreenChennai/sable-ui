//! 效果与 token 分组(任务 4.3 分组 7~8):
//! - 效果页:DropShadow / Glow / ColorMatrix 离屏渲一个演示矩形,以 gpui
//!   image 上屏;
//! - token 页:5 级 ELEVATIONS 阴影卡(离屏)+ 色板(深/浅过渡切换
//!   + theme::inject 自定义 accent 注入,V4.0 T5.2)。
//!
//! # 与效果系统的关系(偏差注明,见任务报告)
//!
//! 任务书提到的 `apply_effects_rgba` 统一入口属于并行中的效果管线
//! (S3 效果栈),本快照的 sable-paint 尚未提供;本页用其**已落地的底层
//! 原语**等价演示:`render_shadow_rgba`(DropShadow/Glow)+ `CpuRenderer`
//! (形状)+ 本文件内的 40 行像素后处理(Glow 着色 / ColorMatrix 简化版)。
//! 统一入口落地后,本页可原样切到 `apply_effects_rgba`。

use std::sync::Arc;

use sable::core::prelude::{Paint, Rgba8};
use sable::gpui::{
    App, AppContext as _, Bounds, Context, Corners, Entity, Hsla, IntoElement, ParentElement,
    Pixels, Render, RenderImage, StatefulInteractiveElement as _, Styled, Window, canvas, div, px,
    rgba,
};
use sable::kurbo::{Affine, Rect, Shape as _};
use sable::paint::prelude::PaintSink as _;
use sable::paint::prelude::{CpuRenderer, ShadowParams, render_shadow_rgba};
use sable::widgets::interact::now_ms;
use sable::widgets::prelude::{RadiusTokens, SpacingTokens, h_flex, v_flex};
use sable::widgets::theme::{ThemeMode, inject, set_mode_animated, theme};
use sable::widgets::tokens::{ColorTokens, ELEVATIONS, FONT_SIZE_CAPTION, rgba8_from_hsla};

use crate::ui::{card, story_button};

/// 演示缓冲尺寸(效果页)。
const TILE: u16 = 96;
/// 阴影卡缓冲尺寸(token 页)。
const SHADOW_W: u16 = 120;
const SHADOW_H: u16 = 88;

/// 预乘 RGBA8 → gpui `RenderImage`。
///
/// PERF-11:un-premultiply 与裸帧构造都收口到
/// `sable_canvas::gpui_element::premultiplied_rgba_to_render_image` 单点
/// (本文件不再自写 `RenderImage::new`;保留本包装以稳定 5 个调用点)。
pub(crate) fn premul_to_render_image(
    data: Vec<u8>,
    width: u16,
    height: u16,
) -> Option<Arc<RenderImage>> {
    sable_canvas::gpui_element::premultiplied_rgba_to_render_image(
        data,
        u32::from(width),
        u32::from(height),
    )
}

/// 把一张 RenderImage 铺到固定尺寸的元素上(canvas paint 阶段上屏,
/// player_view 同款 `Window::paint_image` 调用)。
fn image_tile(image: Option<Arc<RenderImage>>, size: f32) -> impl IntoElement {
    canvas(
        move |_, _, _| {},
        move |bounds: Bounds<Pixels>, _: (), window: &mut Window, _: &mut App| {
            if let Some(image) = &image {
                let _ = window.paint_image(bounds, Corners::all(px(0.0)), image.clone(), 0, false);
            }
        },
    )
    .w(px(size))
    .h(px(size))
}

/// src-over 合成(双方均为**预乘** RGBA8):src 盖在 dst 上。
fn over(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let sa = f32::from(s[3]) / 255.0;
        let da = f32::from(d[3]) / 255.0;
        for channel in 0..3 {
            // 预乘域:out = src + dst × (1 - src_a)
            let out = f32::from(s[channel]) + f32::from(d[channel]) * (1.0 - sa);
            d[channel] = out.round().clamp(0.0, 255.0) as u8;
        }
        let out_a = sa + da * (1.0 - sa);
        d[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
    }
}

/// 预乘黑(阴影)按 RGB 着色 → 彩色 Glow(预乘域逐通道缩放,合法)。
fn tint_premul_black(data: &mut [u8], color: Rgba8) {
    for px in data.chunks_exact_mut(4) {
        for (channel, tint) in px[..3].iter_mut().zip(&color[..3]) {
            *channel = ((u16::from(*channel) * u16::from(*tint)) / 255) as u8;
        }
    }
}

/// ColorMatrix 简化版(演示语义):亮度 ×1.25 + 饱和度 ×0.35,作用在
/// 预乘缓冲上(演示矩形的内部 alpha = 255,预乘 == 直通;边缘半透明像素
/// 的轻微色差为演示可接受范围)。SVG feColorMatrix 对齐版见效果管线。
fn color_matrix_demo(data: &mut [u8]) {
    const BRIGHTNESS: f32 = 1.25;
    const SATURATION: f32 = 0.35;
    for px in data.chunks_exact_mut(4) {
        if px[3] == 0 {
            continue;
        }
        let rgb: [f32; 3] = [f32::from(px[0]), f32::from(px[1]), f32::from(px[2])];
        let luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        for (channel, value) in px[..3].iter_mut().zip(rgb) {
            let out = (luma + (value - luma) * SATURATION) * BRIGHTNESS;
            *channel = out.round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// 演示矩形(圆角方形,画布居中)的路径。
fn demo_rect() -> sable::kurbo::BezPath {
    Rect::new(24.0, 24.0, 72.0, 72.0)
        .to_rounded_rect(10.0)
        .to_path(0.1)
}

/// 不透明演示矩形(透明底,CpuRenderer 离屏)。
fn demo_shape_rgba(color: Rgba8) -> Vec<u8> {
    let mut renderer = CpuRenderer::new(TILE, TILE, [0, 0, 0, 0]);
    renderer
        .sink()
        .fill(&Paint::Solid(color), Affine::IDENTITY, &demo_rect());
    renderer.finish()
}

// —— 效果分组视图 ——

/// 效果瓦片缓存条目:(渲染时模式, 渲染时 accent, 三张瓦片)— 模式切换
/// 或 inject 定制 accent 后重渲(颜色随 token;V4.0 T5.2 起缓存键含 accent)。
type EffectTiles = (ThemeMode, Hsla, [Option<Arc<RenderImage>>; 3]);

/// 效果分组:三张离屏渲染的演示瓦片。
pub struct EffectsSection {
    rendered: Option<EffectTiles>,
}

impl EffectsSection {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe_global::<sable::widgets::theme::SableTheme>(|_, cx| cx.notify())
                .detach();
            EffectsSection { rendered: None }
        })
    }

    /// 五张瓦片:DropShadow / Glow / ColorMatrix(颜色全部来自 token)。
    fn render_tiles(colors: &ColorTokens) -> [Option<Arc<RenderImage>>; 3] {
        let accent = rgba8_from_hsla(colors.accent);
        let shape = demo_shape_rgba(accent);

        // DropShadow:黑影(blur 8 / 偏移 (0,4) / alpha 0.32)垫底,形状盖顶
        let mut shadow = render_shadow_rgba(
            &demo_rect(),
            Affine::IDENTITY,
            ShadowParams {
                blur_px: 8.0,
                offset: (0.0, 4.0),
                alpha: 0.32,
            },
            TILE,
            TILE,
        );
        over(&mut shadow, &shape);
        let drop_shadow = premul_to_render_image(shadow, TILE, TILE);

        // Glow:无偏移大模糊黑影 → 着色成 accent 光晕 → 形状盖顶
        let mut glow = render_shadow_rgba(
            &demo_rect(),
            Affine::IDENTITY,
            ShadowParams {
                blur_px: 12.0,
                offset: (0.0, 0.0),
                alpha: 0.9,
            },
            TILE,
            TILE,
        );
        tint_premul_black(&mut glow, accent);
        over(&mut glow, &shape);
        let glow_tile = premul_to_render_image(glow, TILE, TILE);

        // ColorMatrix:形状直出后过亮度/饱和度矩阵
        let mut matrix = shape;
        color_matrix_demo(&mut matrix);
        let matrix_tile = premul_to_render_image(matrix, TILE, TILE);

        [drop_shadow, glow_tile, matrix_tile]
    }
}

impl Render for EffectsSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let accent = colors.accent;
        // 缓存键 = (模式, accent):模式切换与 inject 定制 accent 都触发重渲,
        // 过渡帧泵逐帧改 accent 时瓦片随之逐帧重渲(演示的可见途径)。
        if self
            .rendered
            .as_ref()
            .is_none_or(|(mode, cached_accent, _)| {
                *mode != colors_mode(cx) || *cached_accent != accent
            })
        {
            self.rendered = Some((colors_mode(cx), accent, Self::render_tiles(&colors)));
        }
        let tiles = self
            .rendered
            .as_ref()
            .map(|(_, _, tiles)| tiles.clone())
            .unwrap_or_default();

        let captions = ["DropShadow", "Glow", "ColorMatrix(亮度/饱和)"];
        let mut row = h_flex().gap(px(SpacingTokens::LG));
        for (tile, caption) in tiles.into_iter().zip(captions) {
            row = row.child(
                v_flex()
                    .gap(px(SpacingTokens::XS))
                    .items_center()
                    .child(image_tile(tile, f32::from(TILE)))
                    .child(
                        div()
                            .text_size(px(FONT_SIZE_CAPTION))
                            .text_color(colors.text_secondary)
                            .child(caption),
                    ),
            );
        }

        card(
            cx,
            "效果 — DropShadow / Glow / ColorMatrix(CPU 离屏)",
            "演示矩形经 CpuRenderer 离屏渲染 + render_shadow_rgba 阴影/光晕 + 像素矩阵后处理,以 gpui image 上屏;颜色随主题 token。统一入口 apply_effects_rgba(效果管线)落地后原样替换。",
            row,
        )
    }
}

/// 当前主题模式(缓存失效判定用)。
fn colors_mode(cx: &App) -> ThemeMode {
    theme(cx).mode
}

// —— token 分组视图 ——

/// 注入演示的自定义 accent(品牌紫):`demo_purple_accent` 返回
/// (accent, accent_muted) 一对。这是演示"用户调色板"数据(与色轮的
/// 用户色同性质),不是组件样式色——组件仍只从 theme 全局态读色。
fn demo_purple_accent() -> (Hsla, Hsla) {
    (rgba(0x9B59FFFF).into(), rgba(0x9B59FF33).into())
}

/// token 分组:5 级海拔阴影卡 + 色板 + 深/浅切换 + 自定义 accent 注入。
pub struct TokensSection {
    /// (渲染时的主题模式, 五张阴影卡)。阴影卡无 accent 成分,键只需模式。
    rendered: Option<(ThemeMode, [Option<Arc<RenderImage>>; 5])>,
}

impl TokensSection {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe_global::<sable::widgets::theme::SableTheme>(|_, cx| cx.notify())
                .detach();
            TokensSection { rendered: None }
        })
    }

    /// 五级海拔:黑色环境影离屏(ELEVATIONS 参数,shadow 越界裁切即降级)。
    fn shadow_cards() -> [Option<Arc<RenderImage>>; 5] {
        let mut cards = [None, None, None, None, None];
        let path = Rect::new(35.0, 25.0, 85.0, 63.0)
            .to_rounded_rect(8.0)
            .to_path(0.1);
        for (index, elevation) in ELEVATIONS.iter().enumerate() {
            let data = render_shadow_rgba(
                &path,
                Affine::IDENTITY,
                ShadowParams {
                    blur_px: elevation.blur,
                    offset: (0.0, elevation.offset_y),
                    alpha: elevation.alpha,
                },
                SHADOW_W,
                SHADOW_H,
            );
            cards[index] = premul_to_render_image(data, SHADOW_W, SHADOW_H);
        }
        cards
    }
}

impl Render for TokensSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        if self
            .rendered
            .as_ref()
            .is_none_or(|(mode, _)| *mode != colors_mode(cx))
        {
            self.rendered = Some((colors_mode(cx), Self::shadow_cards()));
        }
        let cards = self
            .rendered
            .as_ref()
            .map(|(_, cards)| cards.clone())
            .unwrap_or_default();

        // 阴影卡:e0..e4(卡片底色 = surface_2,阴影图叠其上)
        let mut elevation_row = h_flex().gap(px(SpacingTokens::MD));
        for (index, card_image) in cards.into_iter().enumerate() {
            elevation_row = elevation_row.child(
                v_flex()
                    .gap(px(SpacingTokens::XS))
                    .items_center()
                    .child(
                        div()
                            .w(px(f32::from(SHADOW_W)))
                            .h(px(f32::from(SHADOW_H)))
                            .rounded(px(RadiusTokens::MD))
                            .bg(colors.surface_2)
                            .border_1()
                            .border_color(colors.border_subtle)
                            .child(image_tile(card_image, f32::from(SHADOW_W))),
                    )
                    .child(
                        div()
                            .text_size(px(FONT_SIZE_CAPTION))
                            .text_color(colors.text_secondary)
                            .child(format!("e{index}")),
                    ),
            );
        }

        // 色板:15 个语义 token 一一展示
        let swatches: [(&str, sable::gpui::Hsla); 15] = [
            ("surface_0", colors.surface_0),
            ("surface_1", colors.surface_1),
            ("surface_2", colors.surface_2),
            ("surface_3", colors.surface_3),
            ("surface_4", colors.surface_4),
            ("border_subtle", colors.border_subtle),
            ("border_strong", colors.border_strong),
            ("text_primary", colors.text_primary),
            ("text_secondary", colors.text_secondary),
            ("text_disabled", colors.text_disabled),
            ("accent", colors.accent),
            ("accent_muted", colors.accent_muted),
            ("danger", colors.danger),
            ("warning", colors.warning),
            ("success", colors.success),
        ];
        let mut palette = v_flex().gap(px(SpacingTokens::XS));
        for group in swatches.chunks(5) {
            let mut line = h_flex().gap(px(SpacingTokens::XS));
            for (name, color) in group {
                line = line.child(
                    v_flex()
                        .gap(px(SpacingTokens::XS))
                        .items_center()
                        .child(
                            div()
                                .w(px(64.0))
                                .h(px(24.0))
                                .rounded(px(RadiusTokens::SM))
                                .bg(*color)
                                .border_1()
                                .border_color(colors.border_subtle),
                        )
                        .child(
                            div()
                                .text_size(px(FONT_SIZE_CAPTION))
                                .text_color(colors.text_disabled)
                                .child(*name),
                        ),
                );
            }
            palette = palette.child(line);
        }

        let mode_label = if colors_mode(cx) == ThemeMode::Dark {
            "切换浅色"
        } else {
            "切换深色"
        };
        // 注入演示(V4.0 T5.2,计划 T3.1 验收):当前 accent 恰为演示紫 =
        // 已注入(模式切换走预设会自然还原,按钮文案随之自愈,无状态可失同步)
        let (purple_accent, _) = demo_purple_accent();
        let injected_now = colors.accent == purple_accent;
        let inject_label = if injected_now {
            "还原默认 token"
        } else {
            "注入紫色 accent"
        };
        let content =
            v_flex()
                .gap(px(SpacingTokens::LG))
                .child(elevation_row)
                .child(palette)
                .child(
                    h_flex()
                        .gap(px(SpacingTokens::SM))
                        .child(story_button(cx, "tokens-toggle", mode_label).on_click(
                            |_event, _window, cx| {
                                let next = if theme(cx).mode == ThemeMode::Dark {
                                    ThemeMode::Light
                                } else {
                                    ThemeMode::Dark
                                };
                                // V4.0 T5.2:同 app.rs——模式切换走 200ms 过渡
                                // (reduced_motion 开启时内部直切),帧泵在根视图。
                                set_mode_animated(cx, next, now_ms());
                                // 主题是全局态:整窗刷新让所有分区同帧换肤
                                cx.refresh_windows();
                            },
                        ))
                        .child(story_button(cx, "tokens-inject", inject_label).on_click(
                            cx.listener(|_, _, _, cx| {
                                let mode = theme(cx).mode;
                                let mut next = match mode {
                                    ThemeMode::Dark => ColorTokens::dark(),
                                    ThemeMode::Light => ColorTokens::light(),
                                };
                                let (purple, muted) = demo_purple_accent();
                                if theme(cx).colors.accent != purple {
                                    next.accent = purple;
                                    next.accent_muted = muted;
                                } // 已是演示紫:按原样注入预设套 = 还原
                                // 注入即取消进行中的过渡(widgets 侧互斥语义),
                                // 组件下一帧全量换肤
                                inject(cx, next, mode);
                                cx.refresh_windows();
                            }),
                        )),
                );

        card(
            cx,
            "token — 5 级海拔阴影 + 色板(深/浅 + 注入)",
            "ELEVATIONS e0(无影)→ e4(对话框),离屏 render_shadow_rgba 渲染;色板为 ColorTokens 全部 15 个语义色;切换按钮走 200ms 过渡(reduced_motion 直切);注入按钮演示 theme::inject 自定义紫色 accent,再点还原。",
            content,
        )
    }
}
