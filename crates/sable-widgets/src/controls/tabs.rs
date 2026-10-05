//! 标签页 Tabs / PanelTabs(迭代审查报告 §5.6 组件矩阵 #6,CMP-01 批 1)。
//!
//! # 形态(受控组件:换序协议走宿主回调)
//!
//! [`Tabs`] 是**受控**组件:选中序号的最终裁决在宿主——点击/键盘产生
//! [`Tabs::on_change`]`(index, &mut App)` 回执,组件**不自持换序**;宿主
//! 在回调里落自身状态后经 [`Tabs::set_selected`] 回写,下划线随之滑动。
//! [`PanelTabs`] 是面板坞场景的类型别名(同规格:accent 下划线、等宽页签)。
//!
//! ```ignore
//! let tabs_entity = cx.new(|cx| {
//!     let weak = cx.entity().downgrade();
//!     Tabs::new(["图层", "效果", "导出"]).on_change(move |i, cx| {
//!         // 宿主裁决(可否决/联动),再回写组件:
//!         weak.update(cx, |tabs, cx| tabs.set_selected(i, cx));
//!     })
//! });
//! ```
//!
//! # 下划线(规格要点)
//!
//! 选中页签底部 accent **2px** 下划线,切换时以 **PANEL 200ms**
//! ([`MotionTokens::DUR_PANEL_MS`])OutCubic 滑到新页签([`UnderlineSlide`]
//! 状态机,anim 引擎 [`crate::anim::Animated`] 插值;`reduced_motion` 直切
//! ——`value_at` 直通目标、`is_running_at` 恒假)。页签等宽(`flex_1` +
//! 最小宽),下划线几何 = 内容宽的分数([`underline_fraction`],与像素
//! 解耦,溢出滚动下依然对位)。
//!
//! # 溢出(选简实现)
//!
//! 内容 `min_w_full` + 页签最小宽:放得下则等宽铺满;放不下内容按最小宽
//! 溢出——给出 [`Tabs::element_id`] 的实例以横向滚动消化
//! (`overflow_x_scroll`,需要 Stateful 元素),未给 id 的实例裁剪(报告 #6
//! 允许"可横向滚动或渐隐提示"二选一,取滚动)。
//!
//! # 键盘(A11Y,报告 §5.10.2)
//!
//! `track_focus` + `tab_stop(true)` 入 Tab 序;**←/→ 切换页签**
//! ([`tabs_nav`] 纯函数,端点钳制不回绕),换序同样走 `on_change` 回执。

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    App, Context, ElementId, FocusHandle, FontWeight, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px, relative,
};

use crate::anim::{Animated, Easing};
use crate::interact::{InteractState, Semantic, semantic_slot, state_layer};
use crate::theme::theme;
use crate::tokens::{
    ColorTokens, HEIGHT_DEFAULT, MotionTokens, RadiusTokens, SpacingTokens, TextSize, UI_FONT,
    control_height, h_flex,
};

// 页签/下划线几何为具名常量(非令牌表的组件本体尺寸,同 choice.rs 惯例)。

/// 选中下划线高(px,报告 #6 规格:accent 下划线 2px)。
const UNDERLINE_HEIGHT_PX: f32 = 2.0;
/// 页签最小宽(px):面板页签可读下限(内容超出即横向溢出)。
const TAB_MIN_WIDTH_PX: f32 = 64.0;
/// 下划线圆头半径(px):高的一半(胶囊端头)。
const UNDERLINE_RADIUS_PX: f32 = UNDERLINE_HEIGHT_PX / 2.0;

/// 页签条高度(px,派生制):`max(26, 18 + 2×4) = 26`(LABEL 档行高 +
/// XS 上下衬,与选择控件命中行同高,面板节奏一致)。
#[must_use]
pub fn tab_height() -> f32 {
    control_height(
        HEIGHT_DEFAULT,
        TextSize::LABEL.line_height,
        SpacingTokens::XS,
    )
}

/// 页签三态配色(纯函数,A11Y-06 · TC-A11Y-TRISTATE-01 的断言面):
/// 返回 `(底色, 文字色)`。选中 = 强文字、底透明(选中指示由 accent 下划线
/// 承担);悬停(仅未选中页签)= state-layer 中性叠加 + 主文字;静止 =
/// 次级文字。press 为瞬时选中(页签点击即换序),不做按压底色。
#[must_use]
pub fn tab_colors(
    colors: &ColorTokens,
    is_selected: bool,
    is_hovered: bool,
) -> (gpui::Hsla, gpui::Hsla) {
    let fg = if is_selected {
        colors.text_strong
    } else if is_hovered {
        colors.text_primary
    } else {
        colors.text_secondary
    };
    let bg = if !is_selected && is_hovered {
        state_layer(colors.surface_1, InteractState::Hover, colors.accent)
    } else {
        gpui::Hsla::transparent_black()
    };
    (bg, fg)
}

// ---------------------------------------------------------------------------
// 纯函数层(导航状态机 + 下划线几何;可无 App 单测)
// ---------------------------------------------------------------------------

/// 键盘导航(纯函数,键盘状态机单点):`left`/`right` → 相邻页签,端点
/// 钳制不回绕(已在端点时返回当前值,由调用方同值过滤);空表/其余键
/// 返回 `None`。
#[must_use]
pub fn tabs_nav(selected: usize, count: usize, key: &str) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let last = count - 1;
    match key {
        "left" => Some(selected.saturating_sub(1).min(last)),
        "right" => Some(selected.saturating_add(1).min(last)),
        _ => None,
    }
}

/// 下划线几何(纯函数):`(left, width)`,均为**内容宽的分数**(0..1)。
/// 页签等宽 → 第 i 页签占据 `[i/n, (i+1)/n)`;越界 selected 钳入末页,
/// 空表返回 `(0.0, 0.0)`(下划线不可见)。
#[must_use]
pub fn underline_fraction(selected: usize, count: usize) -> (f64, f64) {
    if count == 0 {
        return (0.0, 0.0);
    }
    let n = count as f64;
    let i = (selected as f64).min(n - 1.0);
    (i / n, 1.0 / n)
}

/// 下划线滑动状态机(TC-CMP-TABS-01 的被测单点):left 分数经 **PANEL
/// 200ms OutCubic** 插值;`reduced_motion` 由 [`Animated`] 直通目标值。
#[derive(Clone, Debug)]
struct UnderlineSlide(Animated<f64>);

impl UnderlineSlide {
    /// 静止在 `fraction` 的初始状态(无入场滑动)。
    fn at(fraction: f64) -> Self {
        Self(Animated::new(fraction))
    }

    /// 发起滑动:从**当前位置**接续到 `fraction`(打断平滑,同 Animated 语义)。
    /// 时长 = PANEL 档(毫秒令牌 ÷ 1000 → [`Duration`],令牌以毫秒计)。
    fn slide_to(&mut self, fraction: f64, now: Instant) {
        self.0.set_at(
            fraction,
            now,
            Duration::from_secs_f64(MotionTokens::DUR_PANEL_MS / 1000.0),
            Easing::OutCubic,
        );
    }

    /// 当前位置(0..1 分数;`reduced_motion` 直通目标)。
    fn position_at(&self, now: Instant) -> f64 {
        self.0.value_at(now)
    }

    /// 滑动是否进行中(为真时宿主应续帧)。
    fn is_running_at(&self, now: Instant) -> bool {
        self.0.is_running_at(now)
    }
}

// ---------------------------------------------------------------------------
// 组件(builder + Entity,受控协议)
// ---------------------------------------------------------------------------

/// 换序回执(受控协议,同 `effect_stack::ClickFn` 的别名惯例):
/// `fn(index, &mut App)`——点击页签与 ←/→ 键盘统一经此上报宿主。
pub type TabChangeFn = Rc<dyn Fn(usize, &mut App)>;

/// 标签页(受控 Entity 组件):`cx.new(|_| Tabs::new(["图层", "效果"])
/// .selected(0).on_change(|i, _cx| { .. }))`;宿主经 [`Self::set_selected`]
/// 回写最终选中。
pub struct Tabs {
    tabs: Vec<SharedString>,
    selected: usize,
    /// 下划线滑动状态机(值 = left 分数;width 分数由选中页签静态决定)
    underline: UnderlineSlide,
    /// 焦点句柄(首帧惰性创建,`tab_stop(true)` 进 Tab 环游)
    focus: Option<FocusHandle>,
    /// 换序回执(点击/键盘统一走此;组件不自持换序)
    on_change: Option<TabChangeFn>,
    /// 元素 id(给出则溢出横向滚动)
    element_id: Option<ElementId>,
    /// A11Y-02 语义槽(可访问名 = 标签条名称;页签名取可见文本)
    semantic: Semantic,
}

/// 面板页签条(面板坞场景的 [`Tabs`] 别名:等宽页签 + accent 下划线滑动,
/// 报告 §5.6 #6 同一规格)。
pub type PanelTabs = Tabs;

impl Tabs {
    /// 由页签标题构造(初始选中第 0 页;空表合法,渲染为空条)。
    pub fn new(labels: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        Tabs {
            tabs: labels.into_iter().map(Into::into).collect(),
            selected: 0,
            underline: UnderlineSlide::at(0.0),
            focus: None,
            on_change: None,
            element_id: None,
            semantic: Semantic::new(),
        }
    }

    /// 初始选中页(越界由渲染钳制;不播滑动——初始即位)。
    #[must_use]
    pub fn selected(mut self, index: usize) -> Self {
        self.selected = index;
        let (left, _) = underline_fraction(index, self.tabs.len());
        self.underline = UnderlineSlide::at(left);
        self
    }

    /// 换序回执:`on_change(index, &mut App)`。点击页签与 ←/→ 键盘统一走此;
    /// 组件不自持换序——宿主落自身状态后应调 [`Self::set_selected`] 回写。
    pub fn on_change(mut self, f: impl Fn(usize, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    /// 元素 id(给出则溢出横向滚动;同屏多实例各给唯一 id)。
    pub fn element_id(mut self, id: impl Into<ElementId>) -> Self {
        self.element_id = Some(id.into());
        self
    }

    /// 页签数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// 是否无页签。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// 当前选中页(组件侧渲染值;最终裁决在宿主)。
    #[must_use]
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// 宿主回写(受控协议落地点):钳制 → 下划线 PANEL 200ms 滑动 → 通知。
    /// 同值幂等(不重播动画)。
    pub fn set_selected(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let clamped = index.min(self.tabs.len() - 1);
        if clamped == self.selected {
            return;
        }
        self.selected = clamped;
        let (left, _) = underline_fraction(clamped, self.tabs.len());
        self.underline.slide_to(left, Instant::now());
        cx.notify();
    }

    /// 换序上报(点击/键盘共用;无宿主回调时为无操作)。
    fn emit(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(cb) = self.on_change.clone() {
            cb(index, cx);
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(next) = tabs_nav(self.selected, self.tabs.len(), &event.keystroke.key)
            .filter(|next| *next != self.selected)
        {
            self.emit(next, cx);
        }
    }
}

// A11Y-02 语义槽(label/role/semantic 三件;role 默认 Tab——页签条以页签
// 为语义单元,可见页签文本即各页签的可访问名)。
semantic_slot!(Tabs);

impl Render for Tabs {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &theme(cx).colors;
        let count = self.tabs.len();
        let selected = self.selected.min(count.saturating_sub(1));
        let (_, width_frac) = underline_fraction(selected, count);
        let underline_left = self.underline.position_at(Instant::now()) as f32;
        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let focused = focus.is_focused(window);

        // 页签(等宽 flex_1 + 最小宽;三态 = tab_colors 纯函数,TC-A11Y-
        // TRISTATE-01 断言面;hover = 中性叠加即时切换;选中 = 强文字 +
        // 下划线,选中页签不再叠 hover)
        let on_change = self.on_change.clone();
        let mut content = h_flex().min_w_full().relative();
        for (i, label) in self.tabs.iter().enumerate() {
            let is_selected = i == selected;
            // 静止/悬停两套配色都出自 tab_colors(hover 样式只挂未选中页签)
            let (_, idle_fg) = tab_colors(colors, is_selected, false);
            let (hover_bg, hover_fg) = tab_colors(colors, is_selected, true);
            let mut tab = h_flex()
                .flex_1()
                .min_w(px(TAB_MIN_WIDTH_PX))
                .justify_center()
                .h(px(tab_height()))
                .font_family(UI_FONT)
                .text_size(px(TextSize::LABEL.size))
                .font_weight(FontWeight(if is_selected {
                    TextSize::BODY_STRONG.weight
                } else {
                    TextSize::LABEL.weight
                }))
                .text_color(idle_fg)
                .cursor_pointer()
                .child(label.clone());
            if !is_selected {
                tab = tab.hover(move |style| style.bg(hover_bg).text_color(hover_fg));
            }
            if let Some(on_change) = on_change.clone() {
                tab = tab.on_mouse_down(
                    MouseButton::Left,
                    move |_event: &MouseDownEvent, _window, cx| on_change(i, cx),
                );
            }
            content = content.child(tab);
        }

        // 下划线:accent 2px 圆头,贴内容底缘,left/width 均为内容宽分数
        content = content.child(
            div()
                .absolute()
                .bottom_0()
                .left(relative(underline_left))
                .w(relative(width_frac as f32))
                .h(px(UNDERLINE_HEIGHT_PX))
                .rounded(px(UNDERLINE_RADIUS_PX))
                .bg(colors.accent),
        );

        // 视口:给 id 的实例横向滚动消化溢出;否则裁剪(选简实现,见模块 doc)
        let viewport = div().min_w_full().child(content);
        let scroller = match self.element_id.clone() {
            Some(id) => viewport.id(id).overflow_x_scroll().into_any_element(),
            None => viewport.overflow_hidden().into_any_element(),
        };

        // 根:焦点 + 键盘(←/→)+ 焦点环(§5.3.3 同规格,画在内容下层)
        let mut root = div()
            .relative()
            .w_full()
            .track_focus(&focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .child(scroller);
        if focused {
            root = root.children(crate::interact::focus_ring(colors.accent, RadiusTokens::SM));
        }

        // 动画帧泵:下划线滑动中才续帧(静止零帧提交)
        if self.underline.is_running_at(Instant::now()) {
            window.request_animation_frame();
        }

        root.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TC-CMP-TABS-01(下划线位置插值状态机):0→200ms 逐帧位置单调、
    /// 200ms 落定精确并沉降、半途打断从当前位置接续。
    #[test]
    fn tc_cmp_tabs_01_underline_slide_monotonic_exact_and_interrupt() {
        let t0 = Instant::now();
        let mut slide = UnderlineSlide::at(0.0);
        assert!(!slide.is_running_at(t0), "初始静止");
        slide.slide_to(0.5, t0);
        // PANEL 时长令牌 = 200ms
        let total = MotionTokens::DUR_PANEL_MS;
        assert_eq!(total, 200.0);
        // 0→200ms 每 10ms 采样:位置单调不减、始终在 [起点, 终点] 内
        let mut prev = slide.position_at(t0);
        assert!(prev.abs() < 1e-12, "起滑瞬间仍在起点");
        for step in 1..=20 {
            let now = t0 + Duration::from_millis(10 * step);
            let p = slide.position_at(now);
            assert!(p >= prev, "位置应单调不减:t={step}0ms, {p} < {prev}");
            assert!((0.0..=0.5).contains(&p), "位置不得越过区间:{p}");
            prev = p;
            assert!(
                slide.is_running_at(now) == (step < 20),
                "滑动恰在 200ms 沉降"
            );
        }
        assert!(
            (prev - 0.5).abs() < 1e-12,
            "200ms 落定精确(容差 1 ulp 级),得 {prev}"
        );
        assert_eq!(
            slide.position_at(t0 + Duration::from_millis(10_000)),
            0.5,
            "超时钳在终点"
        );
        assert!(
            !slide.is_running_at(t0 + Duration::from_millis(10_000)),
            "沉降后不再续帧"
        );
        // 半途打断:0→0.5 走到 100ms(OutCubic(0.5)=0.875 → 0.4375),改目标
        // 0.25:打断瞬间不跳变,之后向新目标推进
        slide.slide_to(0.5, t0);
        let mid = t0 + Duration::from_millis(100);
        let current = slide.position_at(mid);
        assert!(
            (current - 0.4375).abs() < 1e-9,
            "OutCubic 半程 = 0.875,得 {current}"
        );
        slide.slide_to(0.25, mid);
        assert_eq!(slide.position_at(mid), current, "打断瞬间不跳变");
        let done = slide.position_at(mid + Duration::from_millis(200));
        assert!((done - 0.25).abs() < 1e-12, "新目标落定:{done}");
    }

    /// TC-CMP-TABS-01(reduced_motion 直切):滑动立即落到目标、不再运行;
    /// 复位后恢复插值语义。
    #[test]
    fn tc_cmp_tabs_01_underline_reduced_motion_direct() {
        crate::anim::set_reduced_motion(true);
        let t0 = Instant::now();
        let mut slide = UnderlineSlide::at(0.0);
        slide.slide_to(0.75, t0);
        assert_eq!(
            slide.position_at(t0 + Duration::from_millis(1)),
            0.75,
            "减弱动态:直切目标"
        );
        assert!(
            !slide.is_running_at(t0 + Duration::from_millis(1)),
            "减弱动态:不再续帧"
        );
        crate::anim::set_reduced_motion(false);
        // 复位后恢复插值语义:全新状态机 0→0.75,半程(100ms,OutCubic(0.5)
        // = 0.875)落在 (0, 0.75) 开区间内
        let t1 = Instant::now();
        let mut normal = UnderlineSlide::at(0.0);
        normal.slide_to(0.75, t1);
        let mid = normal.position_at(t1 + Duration::from_millis(100));
        assert!(
            (mid - 0.656_25).abs() < 1e-9,
            "复位后回到插值(半程 = 0.875),得 {mid}"
        );
    }

    /// TC-CMP-TABS-01(下划线几何映射):等宽分数、越界钳制、空表不可见、
    /// left+width 恒不越界。
    #[test]
    fn tc_cmp_tabs_01_underline_fraction_mapping() {
        assert_eq!(underline_fraction(0, 4), (0.0, 0.25));
        assert_eq!(underline_fraction(2, 4), (0.5, 0.25));
        assert_eq!(underline_fraction(3, 4), (0.75, 0.25));
        assert_eq!(underline_fraction(9, 4), (0.75, 0.25), "越界钳入末页");
        assert_eq!(underline_fraction(0, 1), (0.0, 1.0));
        assert_eq!(underline_fraction(0, 0), (0.0, 0.0), "空表下划线不可见");
        for count in 1..=8 {
            for i in 0..count {
                let (left, width) = underline_fraction(i, count);
                assert!(left >= 0.0 && width > 0.0 && left + width <= 1.0 + 1e-12);
            }
        }
    }

    /// TC-CMP-TABS-01(键盘导航状态机):←/→ 相邻移动、端点钳制不回绕、
    /// 空表与其余键无意图。
    #[test]
    fn tc_cmp_tabs_01_keyboard_nav_clamps() {
        assert_eq!(tabs_nav(0, 4, "right"), Some(1));
        assert_eq!(tabs_nav(2, 4, "left"), Some(1));
        assert_eq!(tabs_nav(0, 4, "left"), Some(0), "左端钳制(不回绕)");
        assert_eq!(tabs_nav(3, 4, "right"), Some(3), "右端钳制(不回绕)");
        assert_eq!(tabs_nav(0, 0, "right"), None, "空表无导航");
        assert_eq!(tabs_nav(0, 0, "left"), None);
        assert_eq!(tabs_nav(1, 4, "up"), None);
        assert_eq!(tabs_nav(1, 4, "down"), None);
        assert_eq!(tabs_nav(1, 4, "enter"), None);
        // 越界 selected(宿主未及时回写)也不外溢
        assert_eq!(tabs_nav(9, 4, "right"), Some(3));
    }

    /// TC-CMP-TABS-01(builder → 字段链路):selected 初值同步下划线即位
    /// (不播滑动)、on_change/element_id 存位、len/is_empty/selected_index。
    #[test]
    fn tc_cmp_tabs_01_builders_shape() {
        let tabs = Tabs::new(["图层", "效果", "导出"]).selected(2);
        assert_eq!(tabs.len(), 3);
        assert!(!tabs.is_empty());
        assert_eq!(tabs.selected_index(), 2);
        // 初值即位:静止状态,位置 = 2/3 分数
        let now = Instant::now();
        assert!(!tabs.underline.is_running_at(now), "初始不播滑动");
        let (left, width) = underline_fraction(2, 3);
        assert!((tabs.underline.position_at(now) - left).abs() < 1e-12);
        assert!((width - 1.0 / 3.0).abs() < 1e-12);
        // 回执存位
        let with_cb = Tabs::new(["a"]).on_change(|_i, _cx| {});
        assert!(with_cb.on_change.is_some());
        assert!(Tabs::new(["a"]).on_change.is_none());
        let empty = Tabs::new(Vec::<&str>::new());
        assert!(empty.is_empty());
        assert_eq!(empty.selected_index(), 0);
        assert_eq!(tab_height(), 26.0, "派生制:max(26, 18+2×4) = 26");
    }

    /// TC-A11Y-TRISTATE-01(Tab 三态样式映射,纯函数):hover 可见且文字
    /// 提档、press/选中走强文字 + 下划线、三态互异;语义槽 label/role 存态。
    #[test]
    fn tc_a11y_tristate_01_tab_colors_and_semantic_slot() {
        for colors in [ColorTokens::dark(), ColorTokens::light()] {
            let (idle_bg, idle_fg) = tab_colors(&colors, false, false);
            let (hover_bg, hover_fg) = tab_colors(&colors, false, true);
            let (sel_bg, sel_fg) = tab_colors(&colors, true, true);
            // 静止:透明底 + 次级文字
            assert_eq!(idle_bg.a, 0.0, "未选中静止底透明");
            assert_eq!(idle_fg, colors.text_secondary);
            // hover:state-layer 中性叠加 + 主文字(可见变化)
            assert_ne!(hover_bg, idle_bg, "hover 底可见");
            assert_eq!(hover_fg, colors.text_primary, "hover 文字提档");
            // 选中:强文字;底透明(选中指示 = accent 下划线,几何另行承担)
            assert_eq!(sel_fg, colors.text_strong);
            assert_eq!(sel_bg.a, 0.0);
            // 选中页签不叠 hover(选中/悬停配色互异)
            assert_ne!(sel_fg, hover_fg);
        }
        // 语义槽(A11Y-02):label 存态、role 默认映射 + 覆写
        let tabs = Tabs::new(["图层", "效果"]).label("面板页签");
        assert_eq!(
            tabs.semantic().label().map(|s| s.as_ref()),
            Some("面板页签")
        );
        assert_eq!(tabs.semantic().role(), None, "未显式给角色");
        let overridden = tabs.role(crate::interact::SemanticRole::List);
        assert_eq!(
            overridden.semantic().role(),
            Some(crate::interact::SemanticRole::List)
        );
    }
}
