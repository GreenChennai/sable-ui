//! 时间轴视图 TimelineView(分册四 §8 的 v0.1 子集)。
//!
//! 消费 `Entity<sable_video::Timeline>`(只读),一切修改经回调交给应用层
//! (走 TimelineHistory,与 Binding 同纪律):
//!
//! - 顶部时间标尺:px_per_second 自适应刻度(目标间距 60~120px,取
//!   1/2/5×10ⁿ 秒档,见 [`nice_tick_step_ms`]);标尺区 = 拖拽/点击 scrub;
//! - 每轨一行(TrackKind 决定基色),clip = 圆角矩形 + 时长文本(素材内
//!   in/out 文本 = 可选项,v0.1 显示 duration);
//! - 播放头红线(可拖,on_seek);
//! - clip 拖拽:dx ÷ px_per_second × 1000 换算 ms,snap_enabled 时经
//!   `Timeline::snap`(容差 8px → ms,拖动排除自身),`on_move_clip` 回调。
//!
//! # v0.1 边界(doc 注明,留 M2)
//! - 横向滚动未接(内容超宽被裁剪;ScrollHandle 接线 = M2);
//! - 纵向滚动用 Stateful div 的 `overflow_y_scroll`(gpui 0.2.2 已核实);
//! - 波形图/缩略图/拖拽投影动画 = M2(分册四 §8 性能红线:拖动不重解码)。

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, Context, Entity, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, ParentElement, Render, RenderImage,
    SharedString, StatefulInteractiveElement, Styled, StyledImage as _, Window, canvas, div, img,
    px,
};
use sable_video::model::{ClipId, Timeline, TrackKind};

use gpui::prelude::FluentBuilder as _;

use crate::interact::{self, Semantic, SemanticRole, semantic_slot};
use crate::theme::theme;
use crate::tokens::{FONT_SIZE_CAPTION, RadiusTokens, SpacingTokens, v_flex};

/// 标尺高度(时间码 11px 行高 + 刻度区 + 播放头把手位)。
pub const RULER_HEIGHT_PX: f32 = 28.0;
/// 轨道行高(NLE 常规档:clip 36px + 上下留白)。
/// 播放头把手宽/高(G-UI-F 常量单点)。
const PLAYHEAD_HANDLE_W_PX: f32 = 8.0;
const PLAYHEAD_HANDLE_H_PX: f32 = 6.0;
/// 胶片缩略图条单帧显示宽(SET_CLIP_THUMBS 契约侧同值)。
const THUMB_TILE_W_PX: f32 = 52.0;

pub const TRACK_HEIGHT_PX: f32 = 44.0;
/// clip 拖拽的吸附容差(屏幕像素,分册四 §8)。
pub const SNAP_TOLERANCE_PX: f64 = 8.0;
/// 刻度目标间距下限(像素)。
const TICK_MIN_SPACING_PX: f64 = 60.0;
/// 空时间轴的最小显示时长(ms,保证有标尺可看)。
const MIN_DISPLAY_MS: u64 = 60_000;
/// 纵向滚动区可见轨道数上限(超出出滚动条)。
const VISIBLE_TRACKS: f32 = 8.0;

/// 播放头跳转/scrub 回调:`(ms, &mut App)`(全部 `Rc<dyn Fn>`,可克隆进
/// 'static 闭包,与 Binding 同纪律:文档修改与撤销由应用层负责)。
pub type SeekFn = Rc<dyn Fn(u64, &mut App)>;
/// clip 移动回调:`(ClipId, 吸附后的新起点 ms, &mut App)`。拖拽中**持续**
/// 触发——应用层应做本地乐观预览(改视图模型),勿逐帧提交后端。
pub type MoveClipFn = Rc<dyn Fn(ClipId, u64, &mut App)>;
/// clip 松手回调:`(ClipId, 最终吸附落点 ms, &mut App)`。拖拽结束时触发
/// 一次——应用层在此提交后端(乐观预览的落地帧)。
pub type DropClipFn = Rc<dyn Fn(ClipId, u64, &mut App)>;
/// clip 单击选中回调:`(ClipId, &mut App)`。
pub type SelectClipFn = Rc<dyn Fn(ClipId, &mut App)>;

/// 时间轴视图(有状态 Entity):
/// `cx.new(|cx| TimelineView::new(timeline_entity).on_seek(...).on_move_clip(...))`
pub struct TimelineView {
    timeline: Entity<Timeline>,
    /// 播放头(本地镜像;真相在应用层 Player)
    playhead_ms: u64,
    /// 选中片段(应用层同步;高亮描边)。存裸值:ClipId 外部不可构造,
    /// 上层只有 id_map 的反向裸值可比对。
    selected: Option<u64>,
    /// 片段缩略图条(视频轨胶片条;键 = ClipId 裸值,值 = 等高帧序列,
    /// 渲染时在块内水平平铺、超出裁剪)。应用层按素材异步抽帧后灌入。
    thumbs: HashMap<u64, Vec<Arc<RenderImage>>>,
    /// 吸附开关(视图层 flag;拖拽换算与指示线读这里,应用层可切换)
    snap_on: bool,
    /// 拖拽中最后一次吸附落点((ClipId 裸值, ms));松手时经 on_drop_clip 提交
    drag_last: Option<(u64, u64)>,
    /// 吸附命中指示线位置(ms;拖拽中吸附生效时显示,松手清除)
    snap_indicator: Option<u64>,
    clip_drag: Option<ClipDrag>,
    seeking: bool,
    /// 标尺区 bounds(prepaint 回写;scrub 换算基准)
    ruler_bounds: Rc<Cell<gpui::Bounds<gpui::Pixels>>>,
    /// PERF-06 素材展示名缓存(键 = ClipId 裸值)。素材路径每帧 clone 是
    /// 旧实现的主要每帧分配;命中返回 SharedString(Arc 计数,零分配)。
    /// 上限见 [`NAME_CACHE_CAP`],超限整表清空(重链素材的陈旧名有界)。
    name_cache: HashMap<u64, SharedString>,
    /// PERF-06 片段标签缓存(键 = (ClipId, 入点 ms, 时长 ms);trim 换键
    /// 自然重算)。上限见 [`LABEL_CACHE_CAP`]。
    label_cache: HashMap<(u64, u64, u64), SharedString>,
    on_seek: SeekFn,
    on_move_clip: MoveClipFn,
    on_drop_clip: DropClipFn,
    on_select_clip: SelectClipFn,
    /// A11Y-01:键盘 seek 焦点(容器 track_focus,←→/Home/End)
    focus: Option<FocusHandle>,
    /// A11Y-02 语义槽(可访问名;role 默认 Slider——播放头连续量语义)
    semantic: Semantic,
}

#[derive(Clone, Copy, Debug)]
struct ClipDrag {
    clip: ClipId,
    start_x: f64,
    orig_start_ms: u64,
}

impl TimelineView {
    /// 绑定时间轴数据的视图(回调默认空操作)。
    pub fn new(timeline: Entity<Timeline>) -> Self {
        TimelineView {
            timeline,
            playhead_ms: 0,
            selected: None,
            thumbs: HashMap::new(),
            snap_on: true,
            drag_last: None,
            snap_indicator: None,
            clip_drag: None,
            seeking: false,
            ruler_bounds: Rc::new(Cell::new(gpui::Bounds::default())),
            name_cache: HashMap::new(),
            label_cache: HashMap::new(),
            on_seek: Rc::new(|_, _| {}),
            on_move_clip: Rc::new(|_, _, _| {}),
            on_drop_clip: Rc::new(|_, _, _| {}),
            on_select_clip: Rc::new(|_, _| {}),
            focus: None,
            semantic: Semantic::new(),
        }
    }

    /// 应用层同步播放头(渲染红线用)。
    pub fn set_playhead(&mut self, ms: u64) {
        self.playhead_ms = ms;
    }

    /// 应用层同步选中片段(高亮描边用;裸值口径,见结构体注释;None = 清除)。
    pub fn set_selected(&mut self, selected: Option<u64>) {
        self.selected = selected;
    }

    /// 整批替换片段缩略图条(键 = ClipId 裸值;壳侧按素材异步抽帧)。
    pub fn set_clip_thumbs(&mut self, thumbs: HashMap<u64, Vec<Arc<RenderImage>>>) {
        self.thumbs = thumbs;
    }

    /// 播放头跳转/scrub。
    pub fn on_seek(mut self, f: impl Fn(u64, &mut App) + 'static) -> Self {
        self.on_seek = Rc::new(f);
        self
    }

    /// clip 移动(已吸附;拖拽中持续触发,应用层做本地乐观预览)。
    pub fn on_move_clip(mut self, f: impl Fn(ClipId, u64, &mut App) + 'static) -> Self {
        self.on_move_clip = Rc::new(f);
        self
    }

    /// clip 松手(拖拽结束,最终落点;应用层在此提交后端)。
    pub fn on_drop_clip(mut self, f: impl Fn(ClipId, u64, &mut App) + 'static) -> Self {
        self.on_drop_clip = Rc::new(f);
        self
    }

    /// 吸附开关(应用层同步;影响拖拽换算与指示线)。
    pub fn set_snap_enabled(&mut self, on: bool) {
        self.snap_on = on;
    }

    /// clip 单击选中。
    pub fn on_select_clip(mut self, f: impl Fn(ClipId, &mut App) + 'static) -> Self {
        self.on_select_clip = Rc::new(f);
        self
    }

    // —— 事件 ——

    /// 键盘 seek(A11Y-01):←/→ = ±一个刻度档(nice_tick_step_ms,不另立
    /// 手感系数)、Home/End = 首尾;步进语义 = [`interact::timeline_seek_step`]
    /// 纯函数,修改经 on_seek 上报(与拖拽 scrub 同一通道)。
    fn on_seek_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let (pps, duration_ms) = {
            let tl = self.timeline.read(cx);
            (tl.px_per_second, tl.duration())
        };
        let step_ms = nice_tick_step_ms(pps);
        if let Some(next) = interact::timeline_seek_step(
            self.playhead_ms,
            step_ms,
            duration_ms,
            &event.keystroke.key,
        ) {
            self.playhead_ms = next;
            (self.on_seek)(next, cx);
            cx.notify();
        }
    }

    fn seek_to(&mut self, x_px: f64, cx: &mut Context<Self>) {
        let pps = self.timeline.read(cx).px_per_second;
        let ms = px_to_ms_clamped(x_px, pps);
        self.playhead_ms = ms;
        (self.on_seek)(ms, cx);
        cx.notify();
    }

    fn on_ruler_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left {
            return;
        }
        self.seeking = true;
        let origin_x = f64::from(self.ruler_bounds.get().origin.x);
        self.seek_to(f64::from(event.position.x) - origin_x, cx);
    }

    fn on_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        if self.seeking {
            let origin_x = f64::from(self.ruler_bounds.get().origin.x);
            self.seek_to(f64::from(event.position.x) - origin_x, cx);
            return;
        }
        let Some(drag) = self.clip_drag else { return };
        // 借用域限定:时间轴的不可变借用只覆盖 snap 换算,出块即释放,
        // 随后的回调需要 &mut(cx)——不用 drop() 表达这一意图。
        let (snapped, hit) = {
            let t = self.timeline.read(cx);
            let dx = f64::from(event.position.x) - drag.start_x;
            let d_ms = dx / t.px_per_second * 1000.0;
            let raw = (drag.orig_start_ms as i64 + d_ms as i64).max(0) as u64;
            let tol_ms = tolerance_ms(t.px_per_second, SNAP_TOLERANCE_PX);
            if !self.snap_on {
                (raw, None)
            } else {
                // 候选 = 相邻片段起止(排除自身)+ 0 + 播放头(剪映同款)
                let mut targets = t.snap_targets(Some(drag.clip));
                if !targets.contains(&self.playhead_ms) {
                    targets.push(self.playhead_ms);
                }
                let hit = targets
                    .iter()
                    .copied()
                    .min_by_key(|t| t.abs_diff(raw))
                    .filter(|t| t.abs_diff(raw) <= tol_ms);
                let snapped = hit.unwrap_or(raw);
                (snapped, hit)
            }
        };
        self.drag_last = Some((drag.clip.value(), snapped));
        self.snap_indicator = hit;
        (self.on_move_clip)(drag.clip, snapped, cx);
        cx.notify();
    }

    fn on_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.seeking = false;
        // 松手落地:最终吸附位置交应用层提交(乐观预览的落地帧)
        if let Some((id, ms)) = self.drag_last.take() {
            let cid = self
                .timeline
                .read(cx)
                .tracks
                .iter()
                .flat_map(|t| t.clips.iter())
                .find(|c| c.id.value() == id)
                .map(|c| c.id);
            if let Some(cid) = cid {
                (self.on_drop_clip)(cid, ms, cx);
            }
        }
        self.clip_drag = None;
        self.snap_indicator = None;
        cx.notify();
    }

    /// 轨道行空白点击 = seek(行与标尺同一横滚内容坐标系,origin 一致)。
    fn on_row_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left {
            return;
        }
        self.seeking = true;
        let origin_x = f64::from(self.ruler_bounds.get().origin.x);
        self.seek_to(f64::from(event.position.x) - origin_x, cx);
    }

    fn on_clip_down(
        &mut self,
        clip: ClipId,
        orig_start_ms: u64,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left {
            return;
        }
        (self.on_select_clip)(clip, cx);
        self.clip_drag = Some(ClipDrag {
            clip,
            start_x: f64::from(event.position.x),
            orig_start_ms,
        });
        cx.notify();
    }
}

impl Render for TimelineView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let colors = t.colors;
        let (pps, tracks) = {
            let tl = self.timeline.read(cx);
            (tl.px_per_second, tl.tracks.len())
        };
        let content_ms = self.timeline.read(cx).duration().max(MIN_DISPLAY_MS);
        let content_w = ms_to_px(content_ms, pps);
        let playhead_x = ms_to_px(self.playhead_ms, pps);
        let step_ms = nice_tick_step_ms(pps);
        let tick_list = ticks(content_ms, step_ms);
        let ruler_bounds = self.ruler_bounds.clone();
        // px() 收 f32;几何一律 f64,画元素前一刻才降 f32
        #[allow(clippy::cast_possible_truncation)]
        fn pxv(v: f64) -> gpui::Pixels {
            px(v as f32)
        }

        // —— 标尺(scrub 热区)——
        let mut ruler = div()
            .relative()
            .h(px(RULER_HEIGHT_PX))
            .w(pxv(content_w))
            .border_b_1()
            .border_color(colors.border_strong)
            .bg(colors.surface_2)
            .cursor_pointer()
            .child(
                canvas(
                    move |bounds: gpui::Bounds<gpui::Pixels>, _w: &mut Window, _cx: &mut App| {
                        ruler_bounds.set(bounds);
                    },
                    |_b: gpui::Bounds<gpui::Pixels>, _s: (), _w: &mut Window, _cx: &mut App| {},
                )
                .size_full(),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_ruler_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up));
        for (i, ms) in tick_list.iter().enumerate() {
            let major = i % 5 == 0;
            let x = ms_to_px(*ms, pps);
            ruler = ruler.child(
                div()
                    .absolute()
                    .left(pxv(x))
                    .top(px(if major {
                        RULER_HEIGHT_PX * 0.4
                    } else {
                        RULER_HEIGHT_PX * 0.65
                    }))
                    .w(px(1.0))
                    .h(px(RULER_HEIGHT_PX * if major { 0.6 } else { 0.35 }))
                    .bg(if major {
                        colors.text_secondary
                    } else {
                        colors.border_strong
                    }),
            );
            if major {
                ruler = ruler.child(
                    div()
                        .absolute()
                        .left(pxv(x + 2.0))
                        .top(px(1.0))
                        .text_size(px(FONT_SIZE_CAPTION))
                        .text_color(colors.text_secondary)
                        .child(fmt_timecode(*ms)),
                );
            }
        }
        // 播放头把手(标尺上的红色小帽;scrub 拖红线时视觉锚点)
        ruler = ruler.child(
            div()
                .absolute()
                .left(pxv((playhead_x - 4.0).max(0.0)))
                .top(px(0.0))
                .w(px(PLAYHEAD_HANDLE_W_PX))
                .h(px(PLAYHEAD_HANDLE_H_PX))
                .rounded(px(2.0))
                .bg(colors.danger),
        );

        // —— 轨道区(横向随标尺同步滚动,纵向独立滚动)+ 播放头 ——
        // PERF-06:可视窗口裁剪 + 标签缓存。窗口起点 = -origin.x(内容滚出
        // 视口的部分),宽度用 window.viewport_size() **过估**(只少裁不闪断);
        // 首帧 ruler bounds 未回写时 origin=0 → 窗口从 0 起仍正确。
        let origin_x = f64::from(self.ruler_bounds.get().origin.x);
        let viewport_w = f64::from(window.viewport_size().width);
        let window_ms = (
            px_to_ms_clamped((origin_x - 240.0).max(0.0), pps),
            px_to_ms_clamped(origin_x.abs() + viewport_w + 240.0, pps),
        );
        let mut rows = v_flex();
        for track in 0..tracks {
            let kind = self.timeline.read(cx).tracks[track].kind;
            let mut row = div()
                .relative()
                .w(pxv(content_w))
                .h(px(TRACK_HEIGHT_PX))
                .mt(px(SpacingTokens::XS))
                .bg(colors.surface_1)
                // 点轨道空白 = scrub 到该点(clip 已 stop_propagation 不冲突)
                .on_mouse_down(MouseButton::Left, cx.listener(Self::on_row_down));
            let timeline = self.timeline.clone();
            let t = timeline.read(cx);
            for c in &t.tracks[track].clips {
                let (id, start_ms, dur_ms, in_ms) = (c.id, c.start_ms, c.duration_ms, c.in_ms);
                if !clip_visible(start_ms, dur_ms, window_ms) {
                    continue; // PERF-06:视口外零渲染数据
                }
                let name = cached_asset_name(&mut self.name_cache, id.value(), &c.asset.path);
                let thumbs = self.thumbs.get(&id.value()).cloned();
                let tint = kind_tint(kind, colors);
                let selected = self.selected == Some(id.value());
                // 内容层:有缩略图条 = 胶片平铺(名称叠底),否则文字标签
                let content: gpui::AnyElement =
                    match thumbs.filter(|t| !t.is_empty()) {
                        Some(frames) => div()
                            .size_full()
                            .flex()
                            .overflow_hidden()
                            .child(div().flex().h_full().children(frames.into_iter().map(
                                |frame| {
                                    img(frame)
                                        .h_full()
                                        .w(px(THUMB_TILE_W_PX))
                                        .flex_shrink_0()
                                        .object_fit(ObjectFit::Cover)
                                },
                            )))
                            .child(
                                div()
                                    .absolute()
                                    .left(px(0.0))
                                    .bottom(px(0.0))
                                    .w_full()
                                    .px(px(SpacingTokens::XS))
                                    .py(px(1.0))
                                    .bg(gpui::black().opacity(0.45))
                                    .text_size(px(FONT_SIZE_CAPTION))
                                    .text_color(colors.text_primary)
                                    .overflow_hidden()
                                    .child(cached_clip_label(
                                        &mut self.label_cache,
                                        id.value(),
                                        in_ms,
                                        dur_ms,
                                        &name,
                                    )),
                            )
                            .into_any_element(),
                        None => div()
                            .size_full()
                            .flex()
                            .items_center()
                            .px(px(SpacingTokens::XS))
                            .text_size(px(FONT_SIZE_CAPTION))
                            .text_color(colors.text_primary)
                            .overflow_hidden()
                            .child(cached_clip_label(
                                &mut self.label_cache,
                                id.value(),
                                in_ms,
                                dur_ms,
                                &name,
                            ))
                            .into_any_element(),
                    };
                row = row.child(
                    div()
                        .absolute()
                        .left(pxv(ms_to_px(start_ms, pps)))
                        .top(px(2.0))
                        .w(pxv(ms_to_px(dur_ms, pps).max(2.0)))
                        .h(px(TRACK_HEIGHT_PX - 4.0))
                        .rounded(px(RadiusTokens::SM))
                        .bg(tint.opacity(if selected { 0.65 } else { 0.35 }))
                        .when(selected, |c| c.border_2())
                        .when(!selected, |c| c.border_1())
                        .border_color(if selected {
                            colors.accent
                        } else {
                            colors.border_subtle
                        })
                        // A11Y-06:clip 悬停态(未选中提描边;按下即选中/拖拽
                        // 跟手 = press 反馈,逐帧重建行不做按压底)
                        .when(!selected, |c| {
                            let hover_border = colors.border_strong;
                            c.hover(move |s| s.border_color(hover_border))
                        })
                        .cursor_pointer()
                        .overflow_hidden()
                        .relative()
                        .child(content)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, ev: &MouseDownEvent, win, cx| {
                                cx.stop_propagation();
                                this.on_clip_down(id, start_ms, ev, win, cx)
                            }),
                        ),
                );
            }
            rows = rows.child(row);
        }
        // 横向:标尺与轨道同一滚动容器(同步);纵向:轨道区独立滚动,
        // 标尺恒在顶部(NLE 惯例)。播放头线随内容横向滚动。
        let body = div()
            .id("timeline-body-x")
            .flex_1()
            .min_h_0()
            .overflow_x_scroll()
            .bg(colors.surface_0)
            .child(
                v_flex().w(pxv(content_w)).child(ruler).child(
                    div()
                        .id("timeline-body-y")
                        .overflow_y_scroll()
                        .max_h(px(TRACK_HEIGHT_PX * VISIBLE_TRACKS))
                        .child(
                            div()
                                .relative()
                                .w(pxv(content_w))
                                .child(rows)
                                // 播放头红线(拖拽/点击 scrub 在标尺上)
                                .child(
                                    div()
                                        .absolute()
                                        .left(pxv(playhead_x))
                                        .top(px(0.0))
                                        .w(px(1.0))
                                        .h_full()
                                        .bg(colors.danger),
                                )
                                // 吸附指示线(拖拽中吸附命中时显示)
                                .when_some(self.snap_indicator, |c, at| {
                                    c.child(
                                        div()
                                            .absolute()
                                            .left(pxv(ms_to_px(at, pps)))
                                            .top(px(0.0))
                                            .w(px(2.0))
                                            .h_full()
                                            .bg(colors.accent),
                                    )
                                }),
                        ),
                ),
            );

        // A11Y-01:键盘 seek 焦点(容器级 track_focus + tab_stop)+ 焦点环
        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let focused = focus.is_focused(window);
        let mut root = v_flex()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(colors.surface_0)
            .track_focus(&focus)
            .on_key_down(cx.listener(Self::on_seek_key))
            .child(body)
            // 根容器收 mouse move/up:拖动中出标尺/clip 仍持续
            .on_mouse_move(cx.listener(Self::on_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up));
        if focused {
            root = root.children(interact::focus_ring(colors.accent, RadiusTokens::SM));
        }
        root
    }
}

// A11Y-02 语义槽:可访问名缺省"时间轴"、role 缺省 Slider(播放头连续量)。
semantic_slot!(TimelineView);

impl TimelineView {
    /// 解析语义(A11Y-02):显式 `.label(...)`/`.role(...)` 优先,缺省 =
    /// ("时间轴", Slider)。
    #[must_use]
    pub fn resolved_semantic(&self) -> Semantic {
        let sem = match self.semantic.label() {
            Some(_) => self.semantic.clone(),
            None => self.semantic.clone().with_label("时间轴"),
        };
        let role = sem.role().unwrap_or(SemanticRole::Slider);
        sem.with_role(role)
    }
}

// —— 纯函数(单测覆盖)——

/// 毫秒 → 像素(与 `Timeline::ms_to_px` 同式;此处独立纯函数便于无实体测试)。
pub fn ms_to_px(ms: u64, px_per_second: f64) -> f64 {
    ms as f64 / 1000.0 * px_per_second
}

/// 屏幕像素 x → 毫秒(负值钳 0)。
pub fn px_to_ms_clamped(x_px: f64, px_per_second: f64) -> u64 {
    // NaN 防御:px_per_second 是 pub 字段,NaN 必须与 0/负数一同钳 0。
    // !(NaN > 0.0) 为 true 而 NaN <= 0.0 为 false,否定比较不能改写。
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(px_per_second > 0.0) {
        return 0;
    }
    ((x_px / px_per_second) * 1000.0).max(0.0) as u64
}

/// 吸附容差换算:屏幕像素 → ms。
pub fn tolerance_ms(px_per_second: f64, tol_px: f64) -> u64 {
    // 同 px_to_ms_clamped:保留否定比较以拒绝 NaN(改写即行为变更)。
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(px_per_second > 0.0) {
        return 0;
    }
    (tol_px / px_per_second * 1000.0) as u64
}

/// 自适应刻度步长(1/2/5×10ⁿ 秒档,取像素间距 ≥ [`TICK_MIN_SPACING_PX`] 的最小档)。
pub fn nice_tick_step_ms(px_per_second: f64) -> u64 {
    // (尾数, 指数)走 1→2→5→1(进位)循环:100ms, 200ms, 500ms, 1s, 2s, 5s, ...
    let (mut mantissa, mut exp) = (1u64, 100u64);
    loop {
        let step_ms = mantissa * exp;
        if ms_to_px(step_ms, px_per_second) >= TICK_MIN_SPACING_PX || step_ms >= 3_600_000 {
            return step_ms;
        }
        match mantissa {
            1 => mantissa = 2,
            2 => mantissa = 5,
            _ => {
                mantissa = 1;
                exp *= 10;
            }
        }
    }
}

/// 标尺刻度表:0..=duration,按 step。
pub fn ticks(duration_ms: u64, step_ms: u64) -> Vec<u64> {
    if step_ms == 0 {
        return vec![0];
    }
    (0..=duration_ms).step_by(step_ms as usize).collect()
}

/// 时间码(纯函数):`m:ss.t`(0.1s 精度)。
pub fn fmt_timecode(ms: u64) -> String {
    let total_tenths = ms / 100;
    let tenths = total_tenths % 10;
    let total_seconds = total_tenths / 10;
    let (m, s) = (total_seconds / 60, total_seconds % 60);
    format!("{m}:{s:02}.{tenths}")
}

/// clip 文本(纯函数):素材名 + 素材内入点。
fn clip_label(name: &str, in_ms: u64, duration_ms: u64) -> String {
    let _ = duration_ms;
    if name.is_empty() {
        format!("{} +{}ms", fmt_timecode(in_ms), duration_ms)
    } else {
        format!("{name} · {}", fmt_timecode(in_ms))
    }
}

/// PERF-06 缓存上限(键数;超限整表清空——简单有界,防 trim/重链缓慢泄漏)。
pub(crate) const NAME_CACHE_CAP: usize = 1024;
pub(crate) const LABEL_CACHE_CAP: usize = 4096;

/// PERF-06:素材展示名缓存纯函数。命中零分配(SharedString = Arc 计数);
/// 未命中恰一次格式化。
fn cached_asset_name(
    cache: &mut HashMap<u64, SharedString>,
    clip: u64,
    path: &str,
) -> SharedString {
    if let Some(name) = cache.get(&clip) {
        return name.clone();
    }
    let name = SharedString::from(asset_display_name(path));
    if cache.len() >= NAME_CACHE_CAP {
        cache.clear();
    }
    cache.insert(clip, name.clone());
    name
}

/// PERF-06:片段标签缓存纯函数(键含入点/时长:trim 后自然换键)。
fn cached_clip_label(
    cache: &mut HashMap<(u64, u64, u64), SharedString>,
    clip: u64,
    in_ms: u64,
    duration_ms: u64,
    name: &str,
) -> SharedString {
    let key = (clip, in_ms, duration_ms);
    if let Some(label) = cache.get(&key) {
        return label.clone();
    }
    let label = SharedString::from(clip_label(name, in_ms, duration_ms));
    if cache.len() >= LABEL_CACHE_CAP {
        cache.clear();
    }
    cache.insert(key, label.clone());
    label
}

/// PERF-06:可见性判定纯函数。片段与可视窗口 [ms0, ms1) 相交才算
/// (视口外不生成渲染数据;窗口过估只会少裁,不会闪断)。
fn clip_visible(start_ms: u64, duration_ms: u64, window: (u64, u64)) -> bool {
    let (ms0, ms1) = window;
    start_ms < ms1 && start_ms.saturating_add(duration_ms) > ms0
}

/// 素材路径 → 展示名(取文件名,去扩展名;空路径回落空串)。
fn asset_display_name(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    file.split_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(file)
        .to_string()
}

/// 轨道类型 → 主题语义色(低透明覆盖在 surface 之上)。
fn kind_tint(kind: TrackKind, colors: crate::tokens::ColorTokens) -> gpui::Hsla {
    let base = match kind {
        TrackKind::Video => colors.accent,
        TrackKind::Audio => colors.success,
        TrackKind::Sticker => colors.warning,
        TrackKind::Subtitle => colors.text_secondary,
    };
    base.opacity(0.35)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_tick_step_targets_60_to_120px() {
        // pps = 100:0.5s = 50px < 60,1s = 100px ✓
        assert_eq!(nice_tick_step_ms(100.0), 1000);
        // pps = 30:1s = 30px,2s = 60px ✓
        assert_eq!(nice_tick_step_ms(30.0), 2000);
        // pps = 600:0.1s = 60px ✓
        assert_eq!(nice_tick_step_ms(600.0), 100);
        // pps = 10:5s = 50px,10s = 100px ✓
        assert_eq!(nice_tick_step_ms(10.0), 10_000);
        // 极端:pps = 1 → 1/2/5 档下 5s=50px 不足,下一档 100s=100px
        // (60s 不在 1/2/5×10ⁿ 阶梯里)
        assert_eq!(nice_tick_step_ms(1.0), 100_000);
    }

    #[test]
    fn px_ms_roundtrip_and_clamp() {
        assert_eq!(ms_to_px(1500, 100.0), 150.0);
        assert_eq!(px_to_ms_clamped(150.0, 100.0), 1500);
        assert_eq!(px_to_ms_clamped(-40.0, 100.0), 0, "负像素钳 0");
        assert_eq!(px_to_ms_clamped(100.0, 0.0), 0, "零 pps 防御除零");
        assert_eq!(tolerance_ms(100.0, 8.0), 80, "8px @100pps = 80ms");
        assert_eq!(tolerance_ms(0.0, 8.0), 0);
    }

    #[test]
    fn ticks_cover_duration_with_step() {
        assert_eq!(ticks(1000, 250), vec![0, 250, 500, 750, 1000]);
        assert_eq!(ticks(0, 500), vec![0]);
        assert_eq!(ticks(1100, 500), vec![0, 500, 1000], "尾部不足一步不补");
        assert_eq!(ticks(1000, 0), vec![0], "零步长防御");
    }

    #[test]
    fn timecode_format() {
        assert_eq!(fmt_timecode(0), "0:00.0");
        assert_eq!(fmt_timecode(61_250), "1:01.2");
        assert_eq!(fmt_timecode(599_900), "9:59.9");
    }

    #[test]
    fn clip_label_prefers_asset_name() {
        assert_eq!(clip_label("geo01", 0, 5_600), "geo01 · 0:00.0");
        assert_eq!(clip_label("geo01", 61_250, 5_600), "geo01 · 1:01.2");
        // 空素材名回落入点 + 时长的旧格式
        assert_eq!(clip_label("", 0, 12_000), "0:00.0 +12000ms");
    }

    #[test]
    fn asset_display_name_stems_and_flattens() {
        assert_eq!(asset_display_name("03_assets/a/geo01.mp4"), "geo01");
        assert_eq!(asset_display_name("clipB"), "clipB");
        assert_eq!(asset_display_name(""), "");
        assert_eq!(asset_display_name("C:\\x\\vo.wav"), "vo");
    }
    #[test]
    fn tc_perf_tl_01_culling_and_label_cache() {
        // PERF-06 裁剪:与可视窗口相交判定(半开区间;越界左右两侧都裁)
        assert!(clip_visible(1_000, 2_000, (0, 5_000)));
        assert!(
            !clip_visible(0, 1_000, (1_000, 5_000)),
            "左出界:终点贴窗口起点 = 裁"
        );
        assert!(
            !clip_visible(5_000, 1_000, (0, 5_000)),
            "右出界:起点贴窗口终点 = 裁"
        );
        assert!(clip_visible(4_999, 2_000, (0, 5_000)), "跨界 = 留");
        assert!(
            clip_visible(0, 1, (0, 5_000)),
            "最短片段(时长≥1,validate 保证)在窗内 = 留"
        );

        // PERF-06 标签缓存:命中零新分配(指针同一),换键自然重算,超限清表
        let mut cache = HashMap::new();
        let a = cached_asset_name(&mut cache, 7, "/media/clip01.mp4");
        assert_eq!(a.as_ref(), "clip01");
        let b = cached_asset_name(&mut cache, 7, "/media/clip01.mp4");
        assert_eq!(a.as_ptr(), b.as_ptr(), "命中帧不得产生新分配");

        let mut labels: HashMap<(u64, u64, u64), SharedString> = HashMap::new();
        let l1 = cached_clip_label(&mut labels, 7, 500, 1_500, "clip01");
        let l2 = cached_clip_label(&mut labels, 7, 500, 1_500, "clip01");
        assert_eq!(l1.as_ptr(), l2.as_ptr());
        let _l3 = cached_clip_label(&mut labels, 7, 600, 1_500, "clip01"); // 键变 → 重算

        for i in 0..(LABEL_CACHE_CAP as u64) {
            let _ = cached_clip_label(&mut labels, i, 0, 0, "x");
        }
        assert!(labels.len() <= LABEL_CACHE_CAP, "缓存有界");
    }
}
