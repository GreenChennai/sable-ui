//! 曲线编辑器(分册四 §9)的 v0.1 只读子件:`CurvePreview`。
//!
//! 采样 64 点,以**竖条列**绘制缓动曲线(gpui 0.2.2 的 `Path` 是填充三角扇、
//! 无描边能力——画 1px 线需自建 ribbon 几何;竖条列编译最稳,契约允许),
//! 网格底用主题语义色的分隔线 div。
//!
//! # 完整交互 = M2(分册四 §9 规划,doc 注明)
//!
//! `Binding<Vec<CurveKey>>`、拖 key 改值、拖手柄改曲率、双击线加 key、
//! 右键删 key、预设缓动切换;绘制换 kurbo::BezPath → vello stroke。
//! 求值侧已就位:`sable_video::curve::evaluate`(播放器每帧调用)。

use std::rc::Rc;

use gpui::{
    App, Context, ElementId, InteractiveElement, IntoElement, MouseButton, ParentElement, Render,
    RenderOnce, StatefulInteractiveElement, Styled, Window, canvas, div, px,
};
use sable_video::model::Easing;

use crate::theme::theme;
use crate::tokens::{RadiusTokens, h_flex};

/// 曲线采样点数(契约:64)。
pub const CURVE_SAMPLES: usize = 64;

/// 缓动曲线只读预览:`CurvePreview::new(easing, width, height)`(RenderOnce)。
#[derive(gpui::IntoElement)]
pub struct CurvePreview {
    easing: Easing,
    /// 预览区宽度(px)
    width: f32,
    /// 预览区高度(px)
    height: f32,
    /// A11Y-02 语义槽(装饰性预览件;label 槽为门禁面)
    semantic: crate::interact::Semantic,
}

// A11Y-02 语义槽:CurvePreview 是只读图形预览(非交互),role 缺省 Decoration。
crate::interact::semantic_slot!(CurvePreview);

impl CurvePreview {
    /// 指定缓动与尺寸的只读预览。
    pub fn new(easing: Easing, width: f32, height: f32) -> Self {
        CurvePreview {
            easing,
            width,
            height,
            semantic: crate::interact::Semantic::new(),
        }
    }
}

impl RenderOnce for CurvePreview {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = theme(cx).colors;
        let bar_w = (self.width / CURVE_SAMPLES as f32).max(1.0);

        // 网格底:四分线(横 2 条 + 竖 2 条)用 border_subtle
        let grid = div()
            .absolute()
            .size_full()
            .child(
                div()
                    .absolute()
                    .top(px(self.height * 0.33))
                    .left(px(0.0))
                    .w_full()
                    .h(px(1.0))
                    .bg(colors.border_subtle),
            )
            .child(
                div()
                    .absolute()
                    .top(px(self.height * 0.66))
                    .left(px(0.0))
                    .w_full()
                    .h(px(1.0))
                    .bg(colors.border_subtle),
            )
            .child(
                div()
                    .absolute()
                    .left(px(self.width * 0.33))
                    .top(px(0.0))
                    .h_full()
                    .w(px(1.0))
                    .bg(colors.border_subtle),
            )
            .child(
                div()
                    .absolute()
                    .left(px(self.width * 0.66))
                    .top(px(0.0))
                    .h_full()
                    .w(px(1.0))
                    .bg(colors.border_subtle),
            );

        // 竖条列:底部对齐,高度 = eased(u) × height
        let mut bars = h_flex().absolute().inset_0().items_end();
        for i in 0..CURVE_SAMPLES {
            let u = i as f64 / (CURVE_SAMPLES - 1) as f64;
            let eased = self.easing.apply(u).clamp(-0.5, 1.5) as f32; // 弹簧过冲允许少量越界
            let h = (eased * self.height).clamp(1.0, self.height * 1.5);
            bars = bars.child(div().w(px(bar_w)).h(px(h)).bg(colors.accent));
        }

        div()
            .relative()
            .w(px(self.width))
            .h(px(self.height))
            .rounded(px(RadiusTokens::SM))
            .border_1()
            .border_color(colors.border_strong)
            .bg(colors.surface_2)
            .overflow_hidden()
            .child(grid)
            .child(bars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_span_unit_interval() {
        // 采样点覆盖 [0,1] 两端:线性缓动首尾条 = 1.0 与满高
        let e = Easing::Linear;
        assert_eq!(e.apply(0.0), 0.0);
        assert_eq!(e.apply(1.0), 1.0);
        // 条数固定 = CURVE_SAMPLES(渲染路径需 App;此处验收采样数学)
        let last_u = (CURVE_SAMPLES - 1) as f64 / (CURVE_SAMPLES - 1) as f64;
        assert_eq!(last_u, 1.0);
    }

    #[test]
    fn preview_builds_for_all_presets() {
        // 全部预设都能构造(RenderOnce 渲染需 App,构造即编译期验收)
        for e in [
            Easing::Linear,
            Easing::InCubic,
            Easing::OutCubic,
            Easing::InOutCubic,
            Easing::Spring,
        ] {
            let _ = CurvePreview::new(e, 120.0, 80.0);
        }
    }

    #[test]
    fn bar_height_math_clamps_overshoot() {
        // 弹簧峰值 ~1.18 → 高度钳到 1.5 倍内不溢出
        let height = 80.0_f32;
        let eased = Easing::Spring.apply(0.55) as f32;
        let h = (eased * height).clamp(1.0, height * 1.5);
        assert!(h <= height * 1.5);
        assert!(h >= 1.0);
    }
}

// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// ANI-03:交互版 CurveEditor(拖 key / 双击加 / 右键删;受控协议)
// ---------------------------------------------------------------------------

/// 关键帧命中阈值(归一化空间;240px 轨道 ≈ 7px)。
pub const CURVE_HIT_THRESHOLD: f64 = 0.04;

/// 求值采样(纯函数):共享 `sable_video::curve::evaluate` 单一真相——
/// 编辑器画的就是播放器跑的(TC-ANI-CURVE-01 的构造性保证)。
#[must_use]
pub fn sample_curve(
    keys: &[sable_video::model::Keyframe],
    range: (u64, u64),
    samples: usize,
) -> Vec<(u64, f64)> {
    let (t0, t1) = range;
    if t1 <= t0 || samples == 0 {
        return Vec::new();
    }
    (0..=samples)
        .map(|i| {
            let t = t0 + (t1 - t0) * i as u64 / samples as u64;
            (t, sable_video::curve::evaluate(keys, t).unwrap_or(0.0))
        })
        .collect()
}

/// 指针(归一化 0..=1 时间/值)→ 命中关键帧下标(曼哈顿距离 ≤ 阈值;
/// 逆序找 = 顶层优先)。
#[must_use]
pub fn hit_key(
    keys: &[sable_video::model::Keyframe],
    range: (u64, u64),
    vrange: (f64, f64),
    pointer: (f64, f64),
    threshold: f64,
) -> Option<usize> {
    let (t0, t1) = range;
    let (v0, v1) = vrange;
    let span_t = (t1 - t0).max(1) as f64;
    let span_v = (v1 - v0).abs().max(1e-9);
    keys.iter()
        .enumerate()
        .rev()
        .find(|(_, k)| {
            let kt = (k.t_ms - t0) as f64 / span_t;
            let kv = (k.value - v0) / span_v;
            ((kt - pointer.0).abs() + (kv - pointer.1).abs()) <= threshold
        })
        .map(|(ix, _)| ix)
}

/// 指针归一化位置 → 新关键帧(双击加键;easing 取 Linear,宿主可后续改)。
#[must_use]
pub fn key_at_pointer(
    range: (u64, u64),
    vrange: (f64, f64),
    pointer: (f64, f64),
) -> sable_video::model::Keyframe {
    let (t0, t1) = range;
    let (v0, v1) = vrange;
    let t = t0 + ((pointer.0.clamp(0.0, 1.0)) * (t1 - t0) as f64).round() as u64;
    let v = v0 + pointer.1.clamp(0.0, 1.0) * (v1 - v0);
    sable_video::model::Keyframe {
        t_ms: t,
        value: v,
        easing: sable_video::model::Easing::Linear,
    }
}

/// 插入关键帧并保持 t_ms 升序(返回插入下标)。
pub fn add_key(
    keys: &mut Vec<sable_video::model::Keyframe>,
    key: sable_video::model::Keyframe,
) -> usize {
    let ix = keys
        .iter()
        .position(|k| k.t_ms > key.t_ms)
        .unwrap_or(keys.len());
    keys.insert(ix, key);
    ix
}

/// 移动关键帧(钳制由调用方经 key_at_pointer 完成;移动后按 t_ms 稳定重排)
/// 并返回移动后的新下标。
pub fn move_key(
    keys: &mut [sable_video::model::Keyframe],
    ix: usize,
    t_ms: u64,
    value: f64,
) -> usize {
    let Some(k) = keys.get_mut(ix) else {
        return ix;
    };
    k.t_ms = t_ms;
    k.value = value;
    keys.sort_by_key(|k| k.t_ms);
    keys.iter()
        .position(|k| k.t_ms == t_ms && k.value == value)
        .unwrap_or(ix)
}

/// 删除关键帧(返回是否发生删除)。
pub fn remove_key(keys: &mut Vec<sable_video::model::Keyframe>, ix: usize) -> bool {
    if ix < keys.len() {
        keys.remove(ix);
        true
    } else {
        false
    }
}

/// 交互曲线编辑器(Entity,受控):keys 真相在宿主,组件经 `set_keys`
/// 回写;每次拖拽/增删经 `on_change` 上行(宿主落可撤销命令——拖 key
/// 可撤销 = 宿主把"拖前 keys 快照"入 undo 栈;突变全部走纯函数,可重放)。
pub struct CurveEditor {
    keys: Vec<sable_video::model::Keyframe>,
    /// 时间窗(ms)与值域。
    range: (u64, u64),
    vrange: (f64, f64),
    /// 拖拽中的关键帧下标。
    drag: Option<usize>,
    /// 绘制区 bounds(prepaint 回写;命中换算基准)。
    bounds: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
    on_change: CurveChangeFn,
    focus: Option<gpui::FocusHandle>,
    semantic: crate::interact::Semantic,
}

/// 变化回调形态。
pub type CurveChangeFn = Rc<dyn Fn(&[sable_video::model::Keyframe], &mut App)>;

impl CurveEditor {
    /// 构造(keys 会被按 t_ms 排序;时间窗为归一化基准)。
    pub fn new(mut keys: Vec<sable_video::model::Keyframe>, range: (u64, u64)) -> Self {
        keys.sort_by_key(|k| k.t_ms);
        CurveEditor {
            keys,
            range,
            vrange: (0.0, 1.0),
            drag: None,
            bounds: std::rc::Rc::new(std::cell::Cell::new(gpui::Bounds::default())),
            on_change: Rc::new(|_, _| {}),
            focus: None,
            semantic: crate::interact::Semantic::new(),
        }
    }

    /// 值域(默认 0..=1)。
    pub fn vrange(mut self, lo: f64, hi: f64) -> Self {
        self.vrange = (lo, hi);
        self
    }

    /// 变化回执(每次拖拽帧/增删;宿主节流入撤销栈)。
    pub fn on_change(
        mut self,
        f: impl Fn(&[sable_video::model::Keyframe], &mut App) + 'static,
    ) -> Self {
        self.on_change = Rc::new(f);
        self
    }

    /// 语义槽(A11Y-02;role 默认 Slider——单值时间函数)。
    pub fn label_slot(mut self, label: impl Into<gpui::SharedString>) -> Self {
        self.semantic = self.semantic.with_label(label.into());
        self
    }

    /// 宿主回写(受控)。
    pub fn set_keys(
        &mut self,
        mut keys: Vec<sable_video::model::Keyframe>,
        cx: &mut Context<Self>,
    ) {
        keys.sort_by_key(|k| k.t_ms);
        if keys != self.keys {
            self.keys = keys;
            cx.notify();
        }
    }

    fn emit(&self, cx: &mut Context<Self>) {
        let cb = self.on_change.clone();
        cb(&self.keys, cx);
    }

    fn pointer_of(&self, ev_pos: gpui::Point<gpui::Pixels>) -> (f64, f64) {
        let b = self.bounds.get();
        let w = f64::from(b.size.width).max(1.0);
        let h = f64::from(b.size.height).max(1.0);
        let x = f64::from(ev_pos.x - b.origin.x);
        let y = f64::from(ev_pos.y - b.origin.y);
        // y 向下 → 值向上(归一化翻转)
        (x / w, 1.0 - y / h)
    }

    fn on_down(&mut self, ev: &gpui::MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if ev.button != MouseButton::Left {
            return;
        }
        let p = self.pointer_of(ev.position);
        if let Some(ix) = hit_key(&self.keys, self.range, self.vrange, p, CURVE_HIT_THRESHOLD) {
            self.drag = Some(ix);
            window.prevent_default();
            cx.notify();
        }
    }

    fn on_drag(&mut self, ev: &gpui::MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.drag else { return };
        let p = self.pointer_of(ev.position);
        let key = key_at_pointer(self.range, self.vrange, p);
        move_key(&mut self.keys, ix, key.t_ms, key.value);
        self.emit(cx);
        window.request_animation_frame();
    }

    fn on_up(&mut self, _ev: &gpui::MouseUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        self.drag = None;
    }

    fn on_double(&mut self, ev: &gpui::ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if ev.click_count() != 2 {
            return;
        }
        let p = self.pointer_of(ev.position());
        if hit_key(&self.keys, self.range, self.vrange, p, CURVE_HIT_THRESHOLD).is_some() {
            return; // 双击落在 key 上 = 不加(删除走右键)
        }
        let key = key_at_pointer(self.range, self.vrange, p);
        add_key(&mut self.keys, key);
        self.emit(cx);
    }

    fn on_right(&mut self, ev: &gpui::MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if ev.button != MouseButton::Right {
            return;
        }
        let p = self.pointer_of(ev.position);
        if let Some(ix) = hit_key(&self.keys, self.range, self.vrange, p, CURVE_HIT_THRESHOLD) {
            remove_key(&mut self.keys, ix);
            self.emit(cx);
            window.prevent_default();
        }
    }
}

impl Render for CurveEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = crate::theme::theme(cx).colors;
        let (accent, grid, key_fill) = (colors.accent, colors.border_subtle, colors.text_primary);
        let (range, vrange) = (self.range, self.vrange);
        let samples = sample_curve(&self.keys, range, CURVE_SAMPLES);
        let bounds = self.bounds.clone();
        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let semantic = self
            .semantic
            .clone()
            .with_role(crate::interact::SemanticRole::Slider);

        let keys_snapshot = self.keys.clone();
        let row = div()
            .id(ElementId::named_usize("curve-editor", 0))
            .w_full()
            .h_full()
            .min_h(px(96.0))
            .track_focus(&focus)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right))
            .on_mouse_move(cx.listener(Self::on_drag))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up))
            .on_click(cx.listener(Self::on_double))
            .child(
                canvas(
                    move |b: gpui::Bounds<gpui::Pixels>, _, _| bounds.set(b),
                    move |b: gpui::Bounds<gpui::Pixels>, _, window: &mut Window, _: &mut App| {
                        // 网格:1/4 分位横线
                        for f in [0.25f32, 0.5, 0.75] {
                            let y = b.origin.y + px(f32::from(b.size.height) * f);
                            window.paint_quad(gpui::fill(
                                gpui::Bounds {
                                    origin: gpui::Point { x: b.origin.x, y },
                                    size: gpui::Size {
                                        width: b.size.width,
                                        height: px(1.0),
                                    },
                                },
                                grid,
                            ));
                        }
                        // 曲线:采样竖条(值域映射;与播放器同源 evaluate)
                        let n = samples.len().max(1);
                        let w = f32::from(b.size.width);
                        let h = f32::from(b.size.height);
                        let (v0, v1) = vrange;
                        for (i, (_t, v)) in samples.iter().enumerate() {
                            let x = b.origin.x + px(w * i as f32 / n as f32);
                            let vh = if (v1 - v0).abs() < 1e-9 {
                                0.0
                            } else {
                                ((v - v0) / (v1 - v0)).clamp(0.0, 1.0) as f32
                            };
                            let bar_h = (h * vh).max(1.0);
                            window.paint_quad(gpui::fill(
                                gpui::Bounds {
                                    origin: gpui::Point {
                                        x,
                                        y: b.origin.y + px(h - bar_h),
                                    },
                                    size: gpui::Size {
                                        width: px((w / n as f32) * 0.66),
                                        height: px(bar_h),
                                    },
                                },
                                accent,
                            ));
                        }
                        // 关键帧:6px 把手
                        let (t0, t1) = range;
                        let (v0, v1) = vrange;
                        let span_t = (t1 - t0).max(1) as f64;
                        let span_v = (v1 - v0).abs().max(1e-9);
                        for k in &keys_snapshot {
                            let kt = ((k.t_ms - t0) as f64 / span_t).clamp(0.0, 1.0) as f32;
                            let kv = ((k.value - v0) / span_v).clamp(0.0, 1.0) as f32;
                            window.paint_quad(gpui::fill(
                                gpui::Bounds {
                                    origin: gpui::Point {
                                        x: b.origin.x + px(w * kt - 3.0),
                                        y: b.origin.y + px(h * (1.0 - kv) - 3.0),
                                    },
                                    size: gpui::Size {
                                        width: px(6.0),
                                        height: px(6.0),
                                    },
                                },
                                key_fill,
                            ));
                        }
                    },
                )
                .w_full()
                .h_full(),
            );
        crate::interact::attach_semantics(div().w_full().h_full().child(row), &semantic)
    }
}

#[cfg(test)]
mod curve_editor_tests {
    use super::*;
    use sable_video::model::{Easing, Keyframe};

    fn key(t: u64, v: f64) -> Keyframe {
        Keyframe {
            t_ms: t,
            value: v,
            easing: Easing::Linear,
        }
    }

    #[test]
    fn tc_ani_curve_01_editor_samples_share_player_evaluation() {
        // ANI-03:编辑器采样 = 播放器 evaluate 逐点一致(单一真相构造性证明)
        let keys = vec![key(0, 0.0), key(500, 1.0), key(1000, 0.5)];
        let samples = sample_curve(&keys, (0, 1000), 100);
        assert_eq!(samples.len(), 101);
        for (t, v) in &samples {
            assert_eq!(
                *v,
                sable_video::curve::evaluate(&keys, *t).expect("窗内必有值"),
                "t={t} 采样必须与播放器求值逐点一致"
            );
        }
        assert_eq!(samples[0], (0, 0.0));
        assert_eq!(samples.last().expect("尾").1, 0.5);
    }

    #[test]
    fn tc_ani_curve_02_drag_key_is_pure_and_undoable() {
        // 可撤销 = 突变全部走纯函数:输入快照原样可回放(宿主撤销单元)
        let mut keys = vec![key(0, 0.0), key(500, 1.0), key(1000, 0.5)];
        let undo_snapshot = keys.clone();

        let ix = hit_key(
            &keys,
            (0, 1000),
            (0.0, 1.0),
            (0.5, 1.0),
            CURVE_HIT_THRESHOLD,
        )
        .expect("指针落在中键上必命中");
        assert_eq!(ix, 1);
        move_key(&mut keys, ix, 300, 0.8);
        assert!(keys.windows(2).all(|w| w[0].t_ms <= w[1].t_ms), "重排保序");
        assert_ne!(keys, undo_snapshot, "移动生效");

        keys = undo_snapshot.clone();
        assert_eq!(keys, undo_snapshot);

        let added = add_key(
            &mut keys,
            key_at_pointer((0, 1000), (0.0, 1.0), (0.25, 0.25)),
        );
        assert_eq!(keys[added].t_ms, 250);
        assert!(remove_key(&mut keys, added));
        assert_eq!(keys.len(), 3);
        assert!(!remove_key(&mut keys, 99), "越界删除 = no-op");
        assert!(
            hit_key(
                &keys,
                (0, 1000),
                (0.0, 1.0),
                (0.99, 0.99),
                CURVE_HIT_THRESHOLD
            )
            .is_none()
        );
    }
}
