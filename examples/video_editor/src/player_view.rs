//! 预览监视器 `PreviewMonitor`(分册四 §12 的 M0 版):
//! `Player` + `SyntheticSource(320×180)` → 当前 Frame → RGBA 上屏。
//!
//! # 上屏路径
//!
//! 与 lumina-canvas `gpui_element` 同款 `canvas()` + `Window::paint_image`
//! 读回兜底路径(gpui 0.2.2 已核实);帧构造走
//! `image::RgbaImage → image::Frame → RenderImage::new`
//! (gpui 0.2.2 的 RenderImage::new 只吃 image crate 裸帧,与
//! lumina-canvas "png" feature 的豁免同因,示例 Cargo.toml 已加 image)。
//!
//! # 播放泵(docs/03 §7)
//!
//! `Context::spawn` + `background_executor().timer(16ms)` 推进
//! `player.tick(dt, duration)` 后 `cx.notify()`;Player 是纯状态机,
//! 泵归 UI 层(本文件)。gpui 0.2.2 异步 API 已核实:
//! `Context::spawn(AsyncFnOnce(WeakEntity<T>, &mut AsyncApp) -> R)`、
//! `AsyncApp::background_executor()`、`timer(Duration) -> Task<()>`。
//!
//! M0 简化:监视器恒显示合成彩条在播放头处的一帧(不按 clip 分段取源),
//! 画面 16:9 直接拉伸铺满监视区(letterbox 留 M2)。

use std::sync::Arc;
use std::time::{Duration, Instant};

use lumina::gpui::{
    App, Bounds, ClickEvent, Context, Corners, Entity, IntoElement, ParentElement as _, Pixels,
    Render, RenderImage, Styled as _, Window, canvas, div, px,
};
use lumina::video::frame::{Frame, FrameSource, SyntheticSource};
use lumina::video::model::Timeline;
use lumina::video::prelude::Player;

use crate::palette::Palette;

/// 预览监视器。
pub struct PreviewMonitor {
    player: Player,
    source: SyntheticSource,
    /// 最近上屏帧(播放头未动则复用,避免每 16ms 重造缓冲)。
    frame: Option<Arc<RenderImage>>,
    rendered_ms: Option<u64>,
    /// 时间轴(读 duration 与未来分段取源)。
    timeline: Entity<Timeline>,
}

impl PreviewMonitor {
    /// 构造并启动播放泵。
    pub fn new(timeline: Entity<Timeline>, cx: &mut Context<Self>) -> Self {
        let mut monitor = Self {
            player: Player::new(),
            source: SyntheticSource::new(320, 180),
            frame: None,
            rendered_ms: None,
            timeline,
        };
        monitor.ensure_frame();
        monitor.start_pump(cx);
        monitor
    }

    /// 播放/暂停。
    pub fn toggle_play(&mut self, cx: &mut Context<Self>) {
        self.player.toggle();
        self.ensure_frame();
        cx.notify();
    }

    /// 当前播放头毫秒(时间轴宿主回推红线用)。
    pub fn position_ms(&self) -> u64 {
        self.player.position_ms
    }

    /// 跳转(时间轴 on_seek / 播放头点击)。
    pub fn seek(&mut self, ms: u64, cx: &mut Context<Self>) {
        self.player.seek(ms);
        self.ensure_frame();
        cx.notify();
    }

    /// 播放泵:16ms 一拍,实体消亡(窗口关闭)即退出。
    fn start_pump(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |monitor, cx| {
            let mut last = Instant::now();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let now = Instant::now();
                let dt_ms = now.duration_since(last).as_millis() as u64;
                last = now;
                // WeakEntity::update:实体已释放 → 返回 Err → 结束泵
                if monitor
                    .update(cx, |monitor, cx| monitor.tick(dt_ms, cx))
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();
    }

    /// 泵的一拍:暂停时不推进不重渲(docs/03 §7:时间码无变化不 notify)。
    fn tick(&mut self, dt_ms: u64, cx: &mut Context<Self>) {
        if !self.player.playing {
            return;
        }
        let duration = self.timeline.read(cx).duration();
        self.player.tick(dt_ms, duration);
        self.ensure_frame();
        cx.notify();
    }

    /// 播放头变化才重造 RGBA 缓冲(合成源恒有帧)。
    fn ensure_frame(&mut self) {
        let position = self.player.position_ms;
        if self.rendered_ms == Some(position) && self.frame.is_some() {
            return;
        }
        if let Some(frame) = self.source.frame_at(position) {
            if let Some(image) = frame_to_render_image(&frame) {
                self.frame = Some(image);
                self.rendered_ms = Some(position);
            }
        }
    }
}

/// `Frame`(RGBA8 直通 alpha)→ gpui `RenderImage`。
///
/// 合成源 alpha 恒 255,直通与预乘同值,无需转换(vello_cpu 路径才需
/// 预乘语义,见 lumina-canvas gpui_element)。
fn frame_to_render_image(frame: &Frame) -> Option<Arc<RenderImage>> {
    let buffer = image::RgbaImage::from_raw(frame.width, frame.height, frame.data.clone())?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// 时间码 `mm:ss.f`(十分之一秒;播放条与剪映习惯一致)。
pub fn format_timecode(ms: u64) -> String {
    let total_secs = ms / 1000;
    format!(
        "{:02}:{:02}.{}",
        total_secs / 60,
        total_secs % 60,
        (ms % 1000) / 100
    )
}

impl Render for PreviewMonitor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_frame();
        let palette = Palette::get(cx);
        let image = self.frame.clone();
        let timecode = format_timecode(self.player.position_ms);
        let total = format_timecode(self.timeline.read(cx).duration());
        let play_label = if self.player.playing {
            "暂停"
        } else {
            "播放"
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(palette.surface_0)
            // 监视区:合成帧铺满(letterbox 留 M2)
            .child(
                div().flex_1().bg(palette.surface_2).child(
                    canvas(
                        move |_bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut App| (),
                        move |bounds: Bounds<Pixels>, _: (), window: &mut Window, _cx: &mut App| {
                            if let Some(image) = &image {
                                // 与 lumina-canvas gpui_element 同款上屏调用
                                let _ = window.paint_image(
                                    bounds,
                                    Corners::all(px(0.)),
                                    image.clone(),
                                    0,
                                    false,
                                );
                            }
                        },
                    )
                    .size_full(),
                ),
            )
            // 传输条:播放/暂停 + 时间码
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .h(px(40.))
                    .bg(palette.surface_1)
                    .child(
                        lumina::gpui_component::button::Button::new("play-toggle")
                            .label(play_label)
                            .compact()
                            .on_click(cx.listener(|monitor, _: &ClickEvent, _, cx| {
                                monitor.toggle_play(cx);
                            })),
                    )
                    .child(
                        div()
                            .text_color(palette.text_primary)
                            .child(format!("{timecode} / {total}")),
                    ),
            )
    }
}
