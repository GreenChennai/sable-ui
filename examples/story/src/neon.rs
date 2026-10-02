//! Neon Card 分组(V4.1 对标):luminaui.in "Neon Card" 的 Sable 等效组件演示。
//!
//! 三张配色卡(粉紫 = 对标默认 / 青蓝 / 金橙),hover 时流动渐变描边
//! (shimmer)、光标跟随内外辉光、漂移粒子与 1.02 缩放;入场动画,
//! reduced_motion 全部直切。逐帧位图经 `CpuRenderer` 离屏 → gpui
//! `RenderImage` 上屏(pixels 同款桥);空闲零帧提交(帧泵仅在动画中请求)。
//!
//! # 诚实边界
//!
//! 对标组件的 `backdrop-filter: blur(12px)`(真实背景毛玻璃)依赖 GPU
//! 后处理管线,仍是路线图项;本组件卡体为"半透明深底 + 高光内边"等效档。

use std::sync::Arc;

use sable::gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, RenderImage, Styled,
    Window, div, px,
};
use sable::widgets::interact::now_ms;
use sable::widgets::neon_card::{
    NEON_PAD_PX, NeonCardState, NeonCardStyle, frame_cache_key, neon_card, render_neon_card,
};
use sable::widgets::prelude::{SpacingTokens, h_flex, v_flex};
use sable::widgets::theme::theme;
use sable::widgets::tokens::FONT_SIZE_CAPTION;

use crate::pixels::premul_to_render_image;
use crate::ui::card;

/// 卡体可视尺寸(不含外边距;缓冲 = 卡体 + 2×[`NEON_PAD_PX`])。
const CARD_W: f32 = 280.0;
const CARD_H: f32 = 168.0;
const FRAME_W: f32 = CARD_W + 2.0 * NEON_PAD_PX;
const FRAME_H: f32 = CARD_H + 2.0 * NEON_PAD_PX;

/// 单卡槽位:样式 + 状态 + 帧缓存(键 = [`frame_cache_key`],键稳定零重渲)。
struct CardSlot {
    id: &'static str,
    title: &'static str,
    desc: &'static str,
    style: NeonCardStyle,
    state: Entity<NeonCardState>,
    cache: Option<(u64, Option<Arc<RenderImage>>)>,
}

/// Neon Card 演示分组。
pub struct NeonSection {
    cards: Vec<CardSlot>,
}

impl NeonSection {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe_global::<sable::widgets::theme::SableTheme>(|_, cx| cx.notify())
                .detach();
            // 三套配色:对标默认粉紫 + 两组扩展(青蓝 / 金橙)。
            // 渐变停格式 = (offset, rgba8),首停即辉光主色(primary)。
            let pink = vec![
                (0.0, [0xFF, 0x00, 0x80, 0xFF]),
                (0.5, [0x79, 0x28, 0xCA, 0xFF]),
                (1.0, [0xFF, 0x00, 0x80, 0xFF]),
            ];
            let cyan = vec![
                (0.0, [0x00, 0xE5, 0xFF, 0xFF]),
                (0.5, [0x7C, 0x4D, 0xFF, 0xFF]),
                (1.0, [0x00, 0xE5, 0xFF, 0xFF]),
            ];
            let amber = vec![
                (0.0, [0xFF, 0xB3, 0x00, 0xFF]),
                (0.5, [0xFF, 0x52, 0x52, 0xFF]),
                (1.0, [0xFF, 0xB3, 0x00, 0xFF]),
            ];
            let slot = |cx: &mut Context<Self>,
                        id: &'static str,
                        title: &'static str,
                        desc: &'static str,
                        stops: Vec<(f32, [u8; 4])>| CardSlot {
                id,
                title,
                desc,
                style: NeonCardStyle {
                    gradient_stops: stops,
                    ..NeonCardStyle::default()
                },
                state: cx.new(|_| NeonCardState::new(0x5EED)),
                cache: None,
            };
            let cards = vec![
                slot(
                    cx,
                    "neon-pink",
                    "霓虹卡片",
                    "对标默认:粉紫流动描边 · 辉光 · 粒子",
                    pink,
                ),
                slot(
                    cx,
                    "neon-cyan",
                    "Neon Card",
                    "青蓝变体:同一组件,只换渐变停与辉光色",
                    cyan,
                ),
                slot(
                    cx,
                    "neon-amber",
                    "Neon Card",
                    "金橙变体:reduced_motion 下全部直切",
                    amber,
                ),
            ];
            NeonSection { cards }
        })
    }
}

impl Render for NeonSection {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = now_ms();
        let colors = theme(cx).colors;

        // 帧泵:同步 reduced_motion → 推进状态机;任一卡在动画中则继续
        // 请求动画帧,空闲零帧提交(缓存键稳定,位图零重渲)。
        let mut any_animating = false;
        for slot in &self.cards {
            let style = &slot.style;
            let animating = slot.state.update(cx, |state, _| {
                state.reduced_motion = sable::widgets::anim::reduced_motion();
                state.tick(now)
                    || state.is_animating(now)
                    || frame_cache_key(style, state, &colors, now, true)
                        != frame_cache_key(style, state, &colors, now, false)
            });
            any_animating |= animating;
        }
        if any_animating {
            window.request_animation_frame();
        }

        // 键变化才重渲位图(shimmer 相位 / hover 进度 / 缩放都会翻键)。
        let mut frames = Vec::with_capacity(self.cards.len());
        for slot in &mut self.cards {
            let key = {
                let style = &slot.style;
                let state = slot.state.read(cx);
                frame_cache_key(style, state, &colors, now, state.is_animating(now))
            };
            let stale = match &slot.cache {
                Some((cached, _)) => *cached != key,
                None => true,
            };
            if stale {
                let buf = {
                    let style = &slot.style;
                    let state = slot.state.read(cx);
                    render_neon_card(
                        u32::from(FRAME_W as u16),
                        u32::from(FRAME_H as u16),
                        style,
                        state,
                        &colors,
                        now,
                    )
                };
                let image = premul_to_render_image(buf, FRAME_W as u16, FRAME_H as u16);
                slot.cache = Some((key, image));
            }
            frames.push(slot.cache.as_ref().and_then(|(_, image)| image.clone()));
        }

        // 三卡一排;卡内容 = 标题 + 说明(gpui 文本层,token 上色)。
        let mut row = h_flex().flex_wrap().gap(px(SpacingTokens::LG));
        for (slot, frame) in self.cards.iter().zip(frames) {
            let content = v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap(px(SpacingTokens::SM))
                .child(
                    div()
                        .text_size(px(20.0))
                        .text_color(colors.text_primary)
                        .child(slot.title),
                )
                .child(
                    div()
                        .text_size(px(FONT_SIZE_CAPTION))
                        .text_color(colors.text_secondary)
                        .child(slot.desc),
                );
            row = row.child(neon_card(
                slot.id,
                &slot.style,
                &slot.state,
                frame,
                FRAME_W,
                FRAME_H,
                content,
            ));
        }

        card(
            cx,
            "Neon Card(对标 luminaui.in)",
            "hover 卡片:流动描边 / 光标辉光 / 粒子 / 缩放;真 backdrop-blur 列 GPU 路线图",
            row,
        )
    }
}
