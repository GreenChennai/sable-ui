//! GPUI 胶水:`SableCanvas` Entity,把画布嵌进 Dock 面板(源自 docs/02 §4.2)。
//!
//! # 上屏路径(当前 = 分册六 §6.4 降级链第 2 级"读回兜底")
//!
//! `render()` 里用 [`sable_paint::cpu::CpuRenderer`] 把场景渲成 RGBA8,
//! 经 `RenderImage` 上屏。**NT handle 零拷贝桥 = M2+**(分册六 §6.4 第 1 级:
//! 独立 wgpu 实例 + 跨 API 共享纹理);每帧整幅重渲 + CPU→GPU 上传是本路径
//! 的已知成本,渲染器缓存/脏矩形裁剪留 M2。
//!
//! 上屏需要 feature 组合 **`gpui + cpu + png`**:
//! - `cpu`:CpuRenderer 渲染(`lib.rs` 已把本模块门控在 gpui+cpu 下);
//! - `png`:把 workspace 的 `image` crate 以 `optional = true` 引入(任务豁免
//!   项),用于 RGBA8 → `image::RgbaImage` → `image::animation::Frame` →
//!   `RenderImage::new` 的裸帧构造(gpui 0.2.2 的 `RenderImage::new` 只吃
//!   `image` crate 的 Frame,无 PNG 编码步骤;docs.rs/gpui/0.2.2 已核实)。
//!   只开 `gpui` 不开 `png` 时画布渲染结果不上屏(空白),上层应同时启用。
//!
//! # gpui 0.2.2 API 核实结论(2026-10,docs.rs/gpui/0.2.2)
//!
//! - `Render::render(&mut self, &mut Window, &mut Context<Self>) -> impl IntoElement`;
//! - `canvas(prepaint: FnOnce(Bounds<Pixels>, &mut Window, &mut App) -> T,
//!   paint: FnOnce(Bounds<Pixels>, T, &mut Window, &mut App))`;
//! - `InteractiveElement` 的 `on_mouse_down/on_mouse_up/on_mouse_move/
//!   on_scroll_wheel` 都不需要 `.id()`;闭包型监听器经 `cx.listener` 生成;
//! - `Window::paint_image(bounds, corner_radii, Arc<RenderImage>, frame_index,
//!   grayscale) -> Result<()>`;
//! - 拖动检测用 `MouseMoveEvent::pressed_button`(无需 Stateful/on_drag)。
//!
//! # v0.1 未接交互(留 M2,均为上层职责)
//!
//! - Esc/键位:gpui 动作系统(docs/03 §4.1)绑定后才谈得上键位,钢笔取消
//!   可经 `CanvasTool::Pen(t) => t.cancel_draft()` 手动触发;
//! - 光标样式:提供了 [`SableCanvas::cursor`],应用侧映射
//!   (`Default→Arrow,Hand→PointingHand,Crosshair→Crosshair,Text→IBeam,
//!   Move→ClosedHand`,gpui 0.2.2 无 Move/Text 变体);
//! - 鼠标拖出元素边界即丢拖动事件(on_drag_move/全局捕获留 M2)。

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, Bounds, Context, Corners, InteractiveElement, IntoElement, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render, ScrollWheelEvent,
    Styled, Window, canvas, div, px,
};
use kurbo::{Point as KPoint, Rect, Vec2 as KVec2};

use sable_foundation::command::History;
use sable_foundation::scene::{NodeId, Rgba8, Scene};
use sable_foundation::viewport::Viewport;

use crate::damage::DamageTracker;
use crate::input;
use crate::render::{OverlayTheme, RenderOpts, render_scene};
use crate::tool::{CursorStyle, HandTool, Mods, PenTool, SelectTool, ToolBehavior, ToolCtx};

#[cfg(feature = "cpu")]
use sable_paint::cpu::CpuRenderer;

/// 画布底色(docs/02 §4.2 的 `#1e1e24`)。
pub const CANVAS_BASE_COLOR: Rgba8 = [0x1e, 0x1e, 0x24, 0xff];
/// CPU 渲染目标的最大边长(px;防御极端 bounds,超出的部分被裁剪)。
const MAX_RENDER_EDGE_PX: f64 = 8192.0;

/// 当前激活工具(选择/钢笔/抓手)。
#[derive(Debug)]
pub enum CanvasTool {
    Select(SelectTool),
    Pen(PenTool),
    Hand(HandTool),
}

impl Default for CanvasTool {
    // #[default] 只能标 unit 变体;SelectTool: Default 手动桥接
    fn default() -> Self {
        CanvasTool::Select(SelectTool::default())
    }
}

impl CanvasTool {
    fn as_behavior(&self) -> &dyn ToolBehavior {
        match self {
            CanvasTool::Select(tool) => tool,
            CanvasTool::Pen(tool) => tool,
            CanvasTool::Hand(tool) => tool,
        }
    }

    fn as_behavior_mut(&mut self) -> &mut dyn ToolBehavior {
        match self {
            CanvasTool::Select(tool) => tool,
            CanvasTool::Pen(tool) => tool,
            CanvasTool::Hand(tool) => tool,
        }
    }
}

/// 画布视图状态(docs/02 §4.2 `SableCanvas` 的落地版)。
///
/// 构造:`let canvas = cx.new(|cx| SableCanvas::new(cx));`
pub struct SableCanvas {
    pub scene: Scene,
    pub viewport: Viewport,
    pub history: History,
    pub damage: DamageTracker,
    pub active_tool: CanvasTool,
    /// 是否画背景网格
    pub show_grid: bool,
    /// 覆盖层主题(UI 侧应换成 sable-widgets tokens 的语义色)
    pub overlay: OverlayTheme,
    /// 画布底色
    pub base_color: Rgba8,
    /// 元素 bounds(每帧 prepaint 回写;事件换算的元素局部化基准)。
    /// 首帧前为全零 → 渲染 1×1,第一帧后即为真实尺寸。
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl SableCanvas {
    /// 空文档 + 单位视口 + 选择工具。
    pub fn new(_cx: &mut Context<Self>) -> Self {
        SableCanvas {
            scene: Scene::new(),
            viewport: Viewport {
                zoom: 1.0,
                pan: KVec2::ZERO,
            },
            history: History::new(),
            damage: DamageTracker::default(),
            active_tool: CanvasTool::default(),
            show_grid: true,
            overlay: OverlayTheme::default(),
            base_color: CANVAS_BASE_COLOR,
            bounds: Rc::new(Cell::new(zero_bounds())),
        }
    }

    /// 切换工具(选择/钢笔/抓手;快捷键 V/P/H 归上层动作系统)。
    pub fn set_tool(&mut self, tool: CanvasTool) {
        self.active_tool = tool;
    }

    /// 当前选中集(选择工具持有;其余工具视为空)。
    pub fn selection(&self) -> &[NodeId] {
        match &self.active_tool {
            CanvasTool::Select(tool) => tool.selection(),
            _ => &[],
        }
    }

    /// 当前光标样式(应用侧映射到 gpui::CursorStyle,见模块 doc)。
    pub fn cursor(&self) -> CursorStyle {
        self.active_tool.as_behavior().cursor()
    }

    /// 取走本帧脏区(交给宿主决定局部/全量重绘策略)。
    pub fn take_damage(&mut self) -> Option<Rect> {
        self.damage.take()
    }

    // —— 事件换算(gpui 坐标 → 元素局部屏幕坐标 → 世界坐标)——

    /// 窗口坐标 → 元素局部屏幕坐标。
    fn to_screen(&self, position: gpui::Point<Pixels>) -> KPoint {
        let bounds = self.bounds.get();
        KPoint::new(
            f64::from(position.x) - f64::from(bounds.origin.x),
            f64::from(position.y) - f64::from(bounds.origin.y),
        )
    }

    /// 窗口坐标 → 世界坐标。
    fn to_world(&self, position: gpui::Point<Pixels>) -> KPoint {
        self.viewport.screen_to_world(self.to_screen(position))
    }

    fn modifiers(m: Modifiers) -> Mods {
        Mods {
            shift: m.shift,
            ctrl: m.control,
            alt: m.alt,
        }
    }

    /// 把一次工具事件在 ToolCtx 上分发(字段拆借:scene/history/viewport/
    /// damage 与 active_tool 互不重叠)。
    fn dispatch(&mut self, f: impl FnOnce(&mut dyn ToolBehavior, &mut ToolCtx)) {
        let mut ctx = ToolCtx {
            scene: &mut self.scene,
            history: &mut self.history,
            viewport: &mut self.viewport,
            damage: &mut self.damage,
        };
        f(self.active_tool.as_behavior_mut(), &mut ctx);
    }

    // —— 事件入口(cx.listener 目标)——

    fn handle_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let world = self.to_world(event.position);
        let mods = Self::modifiers(event.modifiers);
        self.dispatch(|tool, ctx| tool.mouse_down(world, mods, ctx));
        cx.notify();
    }

    fn handle_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 左键按住才视为拖动(悬停追踪留 M2)
        if event.pressed_button == Some(MouseButton::Left) {
            let world = self.to_world(event.position);
            let mods = Self::modifiers(event.modifiers);
            self.dispatch(|tool, ctx| tool.mouse_drag(world, mods, ctx));
            cx.notify();
        }
    }

    fn handle_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let world = self.to_world(event.position);
        let mods = Self::modifiers(event.modifiers);
        self.dispatch(|tool, ctx| tool.mouse_up(world, mods, ctx));
        cx.notify();
    }

    /// 右键:钢笔工具取消草稿(docs/04 §3 的"右键取消"入口)。
    fn handle_right_down(
        &mut self,
        _event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let CanvasTool::Pen(pen) = &mut self.active_tool {
            pen.cancel_draft();
            cx.notify();
        }
    }

    fn handle_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cursor_screen = self.to_screen(event.position);
        // 触控板"行"→像素折算系数 24(与 Zed 惯例一致;精确滚动不受影响)
        let delta_px = event.delta.pixel_delta(px(24.0));
        let delta = KVec2::new(f64::from(delta_px.x), f64::from(delta_px.y));
        input::on_scroll(
            &mut self.viewport,
            cursor_screen,
            delta,
            event.modifiers.control,
        );
        cx.notify();
    }
}

impl Render for SableCanvas {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bounds = self.bounds.get();
        let (width, height) = render_size(bounds);

        // 读回兜底:整幅渲成 RGBA8(预乘,行主序)
        let selection = self.selection().to_vec();
        let opts = RenderOpts {
            selection,
            show_grid: self.show_grid,
            overlay: self.overlay,
            screen_size: (f64::from(width), f64::from(height)),
        };
        let mut renderer = CpuRenderer::new(width, height, self.base_color);
        render_scene(&self.scene, &self.viewport, renderer.sink(), &opts);
        // 工具预览层(橡皮筋/钢笔草稿)画在场景之上
        self.active_tool
            .as_behavior()
            .preview(renderer.sink(), &self.viewport);
        let rgba = renderer.finish();

        #[cfg(feature = "png")]
        let frame_image = rgba_to_render_image(rgba, width, height);
        #[cfg(not(feature = "png"))]
        let frame_image = {
            let _ = rgba; // 无 image 依赖:渲染结果不上屏(见模块 doc 的 feature 组合说明)
            None::<std::sync::Arc<gpui::RenderImage>>
        };

        let bounds_cell = self.bounds.clone();
        div()
            .size_full()
            .child(canvas(
                move |element_bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut App| {
                    bounds_cell.set(element_bounds);
                },
                move |paint_bounds: Bounds<Pixels>,
                      _state: (),
                      window: &mut Window,
                      _cx: &mut App| {
                    if let Some(image) = frame_image {
                        let _ = window.paint_image(
                            paint_bounds,
                            Corners::all(px(0.0)),
                            image,
                            0,
                            false,
                        );
                    }
                },
            ))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::handle_mouse_down))
            .on_mouse_move(cx.listener(Self::handle_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::handle_mouse_up))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::handle_right_down))
            .on_scroll_wheel(cx.listener(Self::handle_scroll_wheel))
    }
}

/// 元素 bounds → CPU 渲染尺寸(夹到 [1, 8192],防御零尺寸/超大 bounds)。
fn render_size(bounds: Bounds<Pixels>) -> (u16, u16) {
    let clamp = |v: f64| v.clamp(1.0, MAX_RENDER_EDGE_PX) as u16;
    (
        clamp(f64::from(bounds.size.width)),
        clamp(f64::from(bounds.size.height)),
    )
}

/// 全零 bounds(prepaint 首帧前的占位)。
fn zero_bounds() -> Bounds<Pixels> {
    Bounds {
        origin: gpui::Point {
            x: px(0.0),
            y: px(0.0),
        },
        size: gpui::Size {
            width: px(0.0),
            height: px(0.0),
        },
    }
}

/// RGBA8(非预乘、行主序)→ gpui `RenderImage`。
///
/// `RenderImage::new(impl Into<SmallVec<[image::Frame; 1]>>)`(gpui
/// 0.2.2 已核实;image 0.25 的 `animation` 模块私有,但 `Frame` 在 crate 根再导出):
/// 经 `image::RgbaImage` → `Frame::new` 裸帧构造,无 PNG 编码。
#[cfg(feature = "png")]
fn rgba_to_render_image(
    rgba: Vec<u8>,
    width: u16,
    height: u16,
) -> Option<std::sync::Arc<gpui::RenderImage>> {
    let buffer = image::RgbaImage::from_raw(u32::from(width), u32::from(height), rgba)?;
    Some(std::sync::Arc::new(gpui::RenderImage::new(vec![
        image::Frame::new(buffer),
    ])))
}
