//! 动画分组(任务 4.3 分组 6):HoverState 悬停演示、PulseState 脉冲、
//! 减弱动态开关、Spring 对比条(SNAPPY vs BOUNCY)。

use sable::gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement,
    Render, StatefulInteractiveElement as _, Styled, Window, div, px,
};
use sable::widgets::anim::{Spring, lerp_hsla, reduced_motion, set_reduced_motion};
use sable::widgets::interact::{HoverState, PulseState, hover_tint, now_ms};
use sable::widgets::prelude::{RadiusTokens, SpacingTokens, h_flex, v_flex};
use sable::widgets::theme::theme;
use sable::widgets::tokens::{FONT_SIZE_BODY, HEIGHT_DEFAULT};

use crate::ui::{card, story_button};

/// 弹簧演示时长(solve 的渐近曲线在 1.5s 处截停;SNAPPY/BOUNCY 均已收敛)。
const SPRING_SETTLE_SEC: f64 = 1.5;
/// 弹簧条轨道宽(px,4px 网格)。
const TRACK_WIDTH: f32 = 288.0;
/// 弹簧滑块尺寸。
const KNOB: f32 = 24.0;
/// 滑块内芯尺寸(16 = KNOB - 2×4px 内边距)。
const KNOB_INNER: f32 = 16.0;

/// 动画分组视图。
pub struct MotionSection {
    hover: HoverState,
    pulse: PulseState,
    /// 弹簧演示起始时刻(ms,now_ms 时钟);None = 未触发。
    springs_started: Option<f64>,
}

impl MotionSection {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe_global::<sable::widgets::theme::SableTheme>(|_, cx| cx.notify())
                .detach();
            MotionSection {
                hover: HoverState::new(),
                pulse: PulseState::new(),
                springs_started: None,
            }
        })
    }

    fn on_hover_changed(&mut self, hovered: &bool, _window: &mut Window, cx: &mut Context<Self>) {
        let now = now_ms();
        if *hovered {
            self.hover.on_enter(now);
        } else {
            self.hover.on_leave(now);
        }
        cx.notify();
    }
}

/// 弹簧条:轨道 + 滑块(progress 0..1 从左到右)。
fn spring_track(cx: &App, label: &'static str, progress: f64) -> sable::gpui::Div {
    let c = theme(cx).colors;
    let knob_x = (progress.clamp(0.0, 1.0) * f64::from(TRACK_WIDTH - KNOB - 8.0)) as f32;
    h_flex()
        .gap(px(SpacingTokens::SM))
        .child(
            div()
                .w(px(72.0))
                .text_size(px(FONT_SIZE_BODY))
                .text_color(c.text_secondary)
                .child(label),
        )
        .child(
            div()
                .w(px(TRACK_WIDTH))
                .h(px(KNOB))
                .rounded(px(RadiusTokens::SM))
                .bg(c.surface_2)
                .border_1()
                .border_color(c.border_subtle)
                .child(
                    div()
                        .ml(px(knob_x))
                        .mt(px((KNOB - KNOB_INNER) / 2.0))
                        .w(px(KNOB_INNER))
                        .h(px(KNOB_INNER))
                        .rounded(px(RadiusTokens::SM))
                        .bg(c.accent),
                ),
        )
}

impl Render for MotionSection {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let now = now_ms();

        // —— 悬停卡:HoverState 驱动 120ms 提亮 ——
        let hover_bg = lerp_hsla(
            colors.surface_2,
            hover_tint(colors.surface_2),
            self.hover.progress_at(now),
        );
        let hover_card = div()
            .id("motion-hover-demo")
            .h(px(64.0))
            .w_full()
            .rounded(px(RadiusTokens::MD))
            .bg(hover_bg)
            .border_1()
            .border_color(colors.border_subtle)
            .cursor_pointer()
            .on_hover(cx.listener(Self::on_hover_changed))
            .child(
                h_flex().justify_center().size_full().child(
                    div()
                        .text_size(px(FONT_SIZE_BODY))
                        .text_color(colors.text_secondary)
                        .child("悬停我:120ms ease-out 提亮(拖出回落)"),
                ),
            );

        // —— 脉冲:300ms accent 描边闪一次 ——
        let pulse_progress = self.pulse.progress_at(now);
        let pulse_chip = div()
            .h(px(HEIGHT_DEFAULT))
            .px(px(SpacingTokens::MD))
            .rounded(px(RadiusTokens::MD))
            .bg(colors.surface_2)
            .border_1()
            .border_color(if pulse_progress > 0.0 {
                colors.accent.opacity(pulse_progress as f32)
            } else {
                colors.border_subtle
            })
            .child(
                div()
                    .text_size(px(FONT_SIZE_BODY))
                    .text_color(colors.text_secondary)
                    .child("脉冲目标"),
            );

        // —— 弹簧对比条:SNAPPY vs BOUNCY(solve 采样,1.5s 截停)——
        let elapsed_sec = self.springs_started.map(|start| (now - start) / 1000.0);
        let spring_value = |spring: Spring| match elapsed_sec {
            Some(t) if t < SPRING_SETTLE_SEC => {
                if reduced_motion() {
                    1.0
                } else {
                    spring.solve(t)
                }
            }
            Some(_) => 1.0,
            None => 0.0,
        };
        let springs_row = v_flex()
            .gap(px(SpacingTokens::SM))
            .child(spring_track(cx, "SNAPPY", spring_value(Spring::SNAPPY)))
            .child(spring_track(cx, "BOUNCY", spring_value(Spring::BOUNCY)));

        // 运行中的动画续帧;静止零帧提交(分册六 §4.4)
        let animating = self.hover.is_running(now)
            || self.pulse.is_active(now)
            || elapsed_sec.is_some_and(|t| t < SPRING_SETTLE_SEC && !reduced_motion());
        if animating {
            window.request_animation_frame();
        }

        let content = v_flex()
            .gap(px(SpacingTokens::LG))
            .child(hover_card)
            .child(
                h_flex()
                    .gap(px(SpacingTokens::SM))
                    .child(
                        story_button(cx, "motion-pulse", "触发脉冲").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.pulse.begin(now_ms());
                                cx.notify();
                            },
                        )),
                    )
                    .child(pulse_chip),
            )
            .child(
                story_button(cx, "motion-reduced", {
                    if reduced_motion() {
                        "减弱动态:开(点击关闭)"
                    } else {
                        "减弱动态:关(点击开启)"
                    }
                })
                .on_click(cx.listener(|_, _, _, cx| {
                    set_reduced_motion(!reduced_motion());
                    // 全局开关影响所有组件:整窗刷新立见
                    cx.refresh_windows();
                })),
            )
            .child(
                h_flex()
                    .gap(px(SpacingTokens::SM))
                    .child(
                        story_button(cx, "motion-spring", "弹一下").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.springs_started = Some(now_ms());
                                cx.notify();
                            },
                        )),
                    )
                    .child(springs_row),
            );

        card(
            cx,
            "动画 — HoverState / PulseState / 减弱动态 / Spring",
            "悬停 120ms ease-out;脉冲 300ms 三角波(撤销反馈同款);弹簧对比 SNAPPY(几乎无过冲)vs BOUNCY(明显回弹),均被减弱动态开关直通终值。",
            content,
        )
    }
}
