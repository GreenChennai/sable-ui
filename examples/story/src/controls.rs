//! 基础按钮分组(CMP-01 批 1):`controls::button` 的 Button/IconButton
//! 演示——三尺寸 × 四变体 × 禁用态 + 图标按钮(tooltip 槽位)。
//!
//! 全部走 **Entity 形态**(section 持有,press 弹簧/hover 插值/焦点环齐备;
//! 内联形态见面板四处收口点:图层面板工具行/效果栈/渐变编辑器/图层树)。
//! 点击任意演示按钮,页内计数 +1,说明回调接线(`WeakEntity` 上行,与
//! 面板回调约定同纪律)。

use sable::gpui::{
    App, AppContext as _, ClickEvent, Context, ElementId, Entity, IntoElement, ParentElement,
    Render, Styled, WeakEntity, Window, div, px,
};
use sable::widgets::controls::button::{
    Button, ButtonSize, ButtonVariant, IconButton, IconButtonSize,
};
use sable::widgets::prelude::{SpacingTokens, h_flex, v_flex};
use sable::widgets::theme::theme;
use sable::widgets::tokens::{FONT_SIZE_BODY, FONT_SIZE_CAPTION};

use crate::ui::card;

/// 行标题列宽(4px 网格上的 24 档;演示布局用,非组件规格)。
const ROW_LABEL_W: f32 = 96.0;

/// 四变体(固定展示序)。
const VARIANTS: [(ButtonVariant, &str); 4] = [
    (ButtonVariant::Primary, "Primary"),
    (ButtonVariant::Secondary, "Secondary"),
    (ButtonVariant::Ghost, "Ghost"),
    (ButtonVariant::Danger, "Danger"),
];

/// 三尺寸(固定展示序;高度 = tokens 派生下限)。
const SIZES: [(ButtonSize, &str); 3] = [
    (ButtonSize::Compact, "Compact(22)"),
    (ButtonSize::Default, "Default(26)"),
    (ButtonSize::Roomy, "Roomy(32)"),
];

/// 基础按钮分组视图。
pub struct ControlsSection {
    /// 点击计数(回调接线演示)。
    clicks: usize,
    /// 三尺寸行:(行标题, 该行的四变体按钮)。
    size_rows: Vec<(&'static str, Vec<Entity<Button>>)>,
    /// 禁用态行(前两个禁用,第三个启用对照)。
    disabled_row: Vec<Entity<Button>>,
    /// 图标按钮行(20/24 两档 + tooltip 槽位)。
    icon_row: Vec<Entity<IconButton>>,
}

/// 构造一条"点击 +1"上行闭包(每个按钮一份,WeakEntity 防循环持有)。
fn make_bump(
    weak: WeakEntity<ControlsSection>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_ev, _win, cx| {
        let _ = weak.update(cx, |this, cx| {
            this.clicks += 1;
            cx.notify();
        });
    }
}

impl ControlsSection {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe_global::<sable::widgets::theme::SableTheme>(|_, cx| cx.notify())
                .detach();
            let weak = cx.entity().downgrade();
            // 三尺寸 × 四变体(Compact 行带图标,演示"图标+文本"内容形态)
            let size_rows = (0..SIZES.len())
                .map(|size_ix| {
                    let (size, title) = SIZES[size_ix];
                    let row = (0..VARIANTS.len())
                        .map(|var_ix| {
                            let (variant, label) = VARIANTS[var_ix];
                            let mut btn = Button::new(
                                ElementId::named_usize("story-btn", size_ix * 10 + var_ix),
                                label,
                            )
                            .variant(variant)
                            .size(size)
                            .on_press(make_bump(weak.clone()));
                            if size == ButtonSize::Compact {
                                btn = btn.icon("▸");
                            }
                            cx.new(|_| btn)
                        })
                        .collect();
                    (title, row)
                })
                .collect();
            // 禁用态(TOK-07:容器不变、仅前景降级、不响应)
            let disabled_row = [
                Button::new("story-btn-dis-primary", "Primary 禁用")
                    .variant(ButtonVariant::Primary)
                    .disabled(true),
                Button::new("story-btn-dis-ghost", "Ghost 禁用")
                    .variant(ButtonVariant::Ghost)
                    .disabled(true),
                Button::new("story-btn-dis-ref", "启用对照")
                    .variant(ButtonVariant::Secondary)
                    // A11Y-02 演示:语义槽覆写可访问名(视觉文本不变)
                    .label("启用对照按钮"),
            ]
            .into_iter()
            .map(|b| cx.new(|_| b))
            .collect();
            // 图标按钮:20/24 两档;tooltip 槽位(本批 = gpui 原生占位浮层)
            let icon_row = [
                IconButton::new("story-ib-20", "◉")
                    .size(IconButtonSize::Icon20)
                    .tooltip("切换效果启用")
                    .tooltip_shortcut("Mod+E")
                    .on_press(make_bump(weak.clone())),
                IconButton::new("story-ib-24", "◐")
                    .size(IconButtonSize::Icon24)
                    .tooltip("半透明预览")
                    .on_press(make_bump(weak.clone())),
            ]
            .into_iter()
            .map(|b| cx.new(|_| b))
            .collect();
            ControlsSection {
                clicks: 0,
                size_rows,
                disabled_row,
                icon_row,
            }
        })
    }
}

impl Render for ControlsSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let clicks = self.clicks;
        let labeled = |title: &'static str, buttons: Vec<Entity<Button>>| {
            h_flex()
                .gap(px(SpacingTokens::SM))
                .child(
                    div()
                        .w(px(ROW_LABEL_W))
                        .text_size(px(FONT_SIZE_BODY))
                        .text_color(colors.text_secondary)
                        .child(title),
                )
                .children(buttons)
        };
        let content = v_flex()
            .gap(px(SpacingTokens::MD))
            .child(
                div()
                    .text_size(px(FONT_SIZE_CAPTION))
                    .text_color(colors.text_disabled)
                    .child(format!(
                        "已点击 {clicks} 次(按下并在按钮内释放触发;focus = Tab 聚焦看焦点环)"
                    )),
            )
            .children(
                self.size_rows
                    .iter()
                    .map(|(title, buttons)| labeled(title, buttons.clone())),
            )
            .child(labeled("禁用", self.disabled_row.clone()))
            .child(
                h_flex()
                    .gap(px(SpacingTokens::SM))
                    .child(
                        div()
                            .w(px(ROW_LABEL_W))
                            .text_size(px(FONT_SIZE_BODY))
                            .text_color(colors.text_secondary)
                            .child("IconButton"),
                    )
                    .children(self.icon_row.iter().cloned())
                    .child(
                        div()
                            .text_size(px(FONT_SIZE_CAPTION))
                            .text_color(colors.text_disabled)
                            .child("悬停看 tooltip(名称 + 快捷键说明)"),
                    ),
            );
        card(
            cx,
            "基础按钮 Button / IconButton",
            "三尺寸(高度 = tokens 派生制)× 四变体(色全走语义令牌);\
             hover/press/focus/disabled 四态走 state_layer;Entity 形态带 \
             press 缩放弹簧(reduced_motion 直通)与焦点环。IconButton \
             20/24 两档,tooltip 槽位已定型(批 2 Tooltip 接管渲染)。",
            content,
        )
    }
}
