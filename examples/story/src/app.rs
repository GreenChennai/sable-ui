//! story 宿主视图:标题栏(主题切换)+ 滚动长页(八个组件分组卡片)。

use sable::gpui::{
    App, Context, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement, Render, StatefulInteractiveElement as _, Styled, Window, div, px,
};
use sable::widgets::interact::now_ms;
use sable::widgets::prelude::{SpacingTokens, h_flex, v_flex};
use sable::widgets::theme::{ThemeMode, advance_transition, set_mode_animated, theme};
use sable::widgets::tokens::{FONT_SIZE_BODY, FONT_SIZE_HEADING};

use crate::controls::ControlsSection;
use crate::inputs::{ColorSection, GradientSection, NumberSection};
use crate::motion::MotionSection;
use crate::neon::NeonSection;
use crate::panels::{LayersSection, TimelineSection};
use crate::pixels::{EffectsSection, TokensSection};
use crate::ui::{group_title, story_button};

/// story 宿主:滚动长页,每个分组一个卡片。
pub struct StoryApp {
    focus: FocusHandle,
    number: Entity<NumberSection>,
    controls: Entity<ControlsSection>,
    color: Entity<ColorSection>,
    gradient: Entity<GradientSection>,
    layers: Entity<LayersSection>,
    timeline: Entity<TimelineSection>,
    motion: Entity<MotionSection>,
    effects: Entity<EffectsSection>,
    tokens: Entity<TokensSection>,
    neon: Entity<NeonSection>,
}

impl StoryApp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let number = NumberSection::new(cx);
        let controls = ControlsSection::new(cx);
        let color = ColorSection::new(cx);
        let gradient = GradientSection::new(cx);
        let layers = LayersSection::new(cx);
        let timeline = TimelineSection::new(cx);
        let motion = MotionSection::new(cx);
        let effects = EffectsSection::new(cx);
        let tokens = TokensSection::new(cx);
        let neon = NeonSection::new(cx);
        StoryApp {
            focus,
            number,
            controls,
            color,
            gradient,
            layers,
            timeline,
            motion,
            effects,
            tokens,
            neon,
        }
    }
}

impl Focusable for StoryApp {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for StoryApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // V4.0 T5.2 主题过渡帧泵:有活动过渡(set_mode_animated 登记在 widgets
        // 全局槽位)则推进一帧(set_global 插值态);返回真即继续请求动画帧,
        // 静止零帧提交(分册六 §4.4)。必须先推进再读 theme(cx),本帧画的
        // 就是本帧插值。
        if advance_transition(cx, now_ms()) {
            window.request_animation_frame();
        }
        let colors = theme(cx).colors;
        let mode_label = if colors_mode(cx) == ThemeMode::Dark {
            "主题:深色(点击切浅色)"
        } else {
            "主题:浅色(点击切深色)"
        };

        // 页首标题栏 + 主题模式按钮
        let header = h_flex()
            .w_full()
            .h(px(48.0))
            .px(px(SpacingTokens::XL))
            .gap(px(SpacingTokens::MD))
            .bg(colors.surface_1)
            .border_b_1()
            .border_color(colors.border_subtle)
            .child(
                div()
                    .text_size(px(FONT_SIZE_HEADING))
                    .text_color(colors.text_primary)
                    .child("Sable UI · 组件 story"),
            )
            .child(
                div()
                    .text_size(px(FONT_SIZE_BODY))
                    .text_color(colors.text_disabled)
                    .child("输入 / 色彩 / 渐变 / 图层 / 时间轴 / 动画 / 效果 / token"),
            )
            .child(h_flex().flex_1().justify_end().child(
                story_button(cx, "header-theme", mode_label).on_click(|_event, _window, cx| {
                    let next = if theme(cx).mode == ThemeMode::Dark {
                        ThemeMode::Light
                    } else {
                        ThemeMode::Dark
                    };
                    // V4.0 T5.2:模式切换走过渡(reduced_motion 开启时内部
                    // 直切并返回 None)。过渡登记在 widgets 全局槽位,由根视图
                    // render 顶部的 advance_transition 帧泵逐帧推进。
                    set_mode_animated(cx, next, now_ms());
                    cx.refresh_windows();
                }),
            ));

        // 滚动长页:overflow_y_scroll 需要有 id 的 Stateful 元素
        div()
            .id("story-scroll")
            .size_full()
            .overflow_y_scroll()
            .bg(colors.surface_0)
            .text_color(colors.text_primary)
            .track_focus(&self.focus)
            .child(header)
            .child(
                v_flex()
                    .max_w(px(1040.0))
                    .mx_auto()
                    .px(px(SpacingTokens::XL))
                    .py(px(SpacingTokens::XL))
                    .gap(px(SpacingTokens::LG))
                    .child(group_title(cx, "输入"))
                    .child(self.number.clone())
                    .child(group_title(cx, "基础按钮"))
                    .child(self.controls.clone())
                    .child(group_title(cx, "色彩"))
                    .child(self.color.clone())
                    .child(self.gradient.clone())
                    .child(group_title(cx, "面板"))
                    .child(self.layers.clone())
                    .child(self.timeline.clone())
                    .child(group_title(cx, "动画"))
                    .child(self.motion.clone())
                    .child(group_title(cx, "效果与 token"))
                    .child(self.effects.clone())
                    .child(self.tokens.clone())
                    .child(group_title(cx, "Neon Card"))
                    .child(self.neon.clone()),
            )
    }
}

/// 当前主题模式(标题栏按钮文案用)。
fn colors_mode(cx: &App) -> ThemeMode {
    theme(cx).mode
}
