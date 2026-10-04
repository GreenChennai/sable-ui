//! 工作台预设与面板便利层(docs/03 §6 的落地版)。
//!
//! # 来源与署名
//!
//! DockArea 封装思路源自 **gpui-component(longbridge/gpui-kit,Apache-2.0)**,
//! NOTICE.md 已登记;本 crate 只做薄封装,未复制上游源码。
//!
//! 锁 **0.5.1**:0.6+ 的 gpui-component 迁往 gpui-pre 类型世界,与 zed
//! gpui 0.2.2 类型不互通(已核实其 Cargo.toml `[dependencies.gpui]
//! package = "gpui-pre"`);0.5.1 是最后一个直接依赖 gpui 0.2.2 的版本
//! (已核实其 `[dependencies.gpui] version = "0.2.2"`),也正是 docs/03 §6
//! 所述的 `DockItem` API 世代。
//!
//! # 0.5.1 API 核实结论(2026-10,本地 registry 源码)
//!
//! - `DockArea::new(id, version, window, &mut Context<Self>)`,区域安装:
//!   `set_center(DockItem, ..)` / `set_left_dock(DockItem, size:
//!   Option<Pixels>, open, ..)` / `set_bottom_dock` / `set_right_dock`;
//! - `DockItem::tabs(Vec<Arc<dyn PanelView>>, &WeakEntity<DockArea>,
//!   window, &mut App)`、`DockItem::panel(Arc<dyn PanelView>)`,链式
//!   `.size(px)` / `.active_index(ix)`;
//! - **单一 `Panel` trait**(`EventEmitter<PanelEvent> + Render +
//!   Focusable`):panel_name/title/closable/visible/dump 等全在一个 trait;
//!   `Entity<P: Panel>` 有 `PanelView` 毛毯实现,`Arc::new(entity)` 即
//!   对象安全句柄(0.7 的 BasePanel/panel_handle 双层拆分不存在);
//! - 序列化:`DockArea::dump(&App) -> DockAreaState`(serde)、
//!   `DockArea::load(DockAreaState, ..) -> anyhow::Result<()>`;重建经
//!   `register_panel(cx, name, |WeakEntity<DockArea>, &PanelState,
//!   &PanelInfo, &mut Window, &mut App| -> Box<dyn PanelView>)`;
//! - `DockArea` 自实现 `Render`;0.5.1 没有 renderer seam/DockSkin,
//!   外观(标题栏/缩放钮)由 `ActiveTheme` 内置渲染。
//!
//! # 宿主契约:壳态持久化(RBT-02 / §6 RB-03、RB-08)
//!
//! **壳态(布局/键位/主题/面板开关)落盘必须经本 crate 的持久化契约,
//! 宿主禁自写**(禁 `std::fs::write` 直写、禁静默吞错):
//!
//! - 布局:写经 [`persist_layout`](crate::persistence::persist_layout)
//!   (原子写,失败不留半截文件),读经
//!   [`load_layout`](crate::persistence::load_layout)(缺失 → 默认;损坏 →
//!   默认 + 显式"已回退";版本迁移契约见 `crate::persistence` 模块文档);
//! - 键位/主题等其余壳态:经 `sable_foundation::persistence::atomic_write`
//!   落盘,解析失败回退默认并告警,纪律与布局一致;
//! - 本模块的 [`save_layout`] / [`restore_layout`] 只是 JSON 字符串层的
//!   便利封装(剪贴板/调试/宿主自管存储),**不构成落盘契约**。
//!
//! 违反形态(上游 VellumBench UI-12 / R-17 教训):多窗口最后写入胜、
//! 坏文件崩壳、壳态无持久化。

use std::sync::Arc;

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, Render, SharedString, WeakEntity, Window, px,
};
use gpui_component::dock::{
    DockArea, DockAreaState, DockItem, Panel, PanelEvent, PanelInfo, PanelState, PanelView,
    register_panel,
};

use crate::error::{DockError, DockResult};

/// 初始化 dock 子系统(幂等):转发 `gpui_component::init`(gpui-component
/// 主题/全局态/PanelRegistry)与 `sable_widgets::theme::init`(Sable
/// 设计 token 主题)。应用入口调用一次即可,重复调用无副作用。
pub fn init(cx: &mut App) {
    if cx.has_global::<InitMarker>() {
        return;
    }
    cx.set_global(InitMarker);
    gpui_component::init(cx);
    sable_widgets::theme::init(cx);
}

/// [`init`] 的幂等标记。
struct InitMarker;

impl gpui::Global for InitMarker {}

/// 把「名字 + 任意视图」包装成可停靠面板的便利层(名字进 tab 标题与
/// 布局序列化;`dump` 用上游默认:只记录 panel_name)。
///
/// 面板本体是任意 `Render` 视图的 [`AnyView`],因此图层/检查器/时间轴等
/// 宿主视图无需为停靠单独实现 Panel trait。
pub struct SablePanel {
    name: &'static str,
    view: AnyView,
    focus_handle: FocusHandle,
}

impl SablePanel {
    /// 构造面板实体。
    pub fn new(name: &'static str, view: AnyView, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            name,
            view,
            focus_handle: cx.focus_handle(),
        })
    }

    /// 包装成 dock 布局可接收的对象安全句柄。
    ///
    /// 0.5.1 的 `Entity<P: Panel>` 自带 `PanelView` 毛毯实现,`Arc::new`
    /// 一步到位(无 0.7 的 panel_handle 表现层包装层)。
    pub fn into_handle(entity: Entity<Self>) -> Arc<dyn PanelView> {
        Arc::new(entity)
    }

    /// 一步到位:`new` + `into_handle`。
    pub fn create(name: &'static str, view: AnyView, cx: &mut App) -> Arc<dyn PanelView> {
        Self::into_handle(Self::new(name, view, cx))
    }
}

impl Panel for SablePanel {
    fn panel_name(&self) -> &'static str {
        self.name
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from(self.name)
    }
}

impl EventEmitter<PanelEvent> for SablePanel {}

impl Focusable for SablePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SablePanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.view.clone()
    }
}

/// 设计软件经典三段式工作台构建器(docs/03 §6)。
///
/// 结构:左 dock(面板组,260px)| 中画布区 | 右 dock(面板组,300px),
/// 每个区域是一个 tab 组;面板可拖拽重排/关闭/缩放,布局可
/// [`persist_layout`](crate::persistence::persist_layout) /
/// [`load_layout`](crate::persistence::load_layout) 持久化(壳态契约)。
/// 右侧为空时不创建右 dock。
pub struct WorkspacePresets;

impl WorkspacePresets {
    /// 构建经典三段式 `DockArea`。
    pub fn build_workspace(
        area_name: &str,
        left_panels: Vec<Arc<dyn PanelView>>,
        center: Arc<dyn PanelView>,
        right_panels: Vec<Arc<dyn PanelView>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<DockArea> {
        // zed SharedString 只有 From<&'static str>/From<String>;运行期 &str 必须经 String
        let area_name: SharedString = area_name.to_string().into();
        let area = cx.new(|cx| DockArea::new(area_name, Some(1), window, cx));
        area.update(cx, |area, cx| {
            let dock_area = cx.entity().downgrade();
            if !left_panels.is_empty() {
                area.set_left_dock(
                    tab_group(left_panels, &dock_area, window, cx),
                    Some(px(260.)),
                    true,
                    window,
                    cx,
                );
            }
            area.set_center(tab_group(vec![center], &dock_area, window, cx), window, cx);
            if !right_panels.is_empty() {
                area.set_right_dock(
                    tab_group(right_panels, &dock_area, window, cx),
                    Some(px(300.)),
                    true,
                    window,
                    cx,
                );
            }
        });
        area
    }
}

/// 把一组面板句柄描述成一个 tab 组(布局 API 的惯用最小单元)。
///
/// 独立成 pub fn 是因为三段式预设覆盖不到的区域(如剪映示例的底部时间轴
/// dock)也要用同一形态描述;`dock_area` 弱句柄由上游 `DockItem::tabs`
/// 用于订阅面板事件。
pub fn tab_group(
    panels: Vec<Arc<dyn PanelView>>,
    dock_area: &WeakEntity<DockArea>,
    window: &mut Window,
    cx: &mut App,
) -> DockItem {
    DockItem::tabs(panels, dock_area, window, cx)
}

/// 序列化 `DockArea` 当前布局为 JSON 字符串(`DockArea::dump` + serde,上游
/// 真实序列化面)。
///
/// **这只是字符串层便利**(剪贴板/调试/宿主自管存储),不是落盘契约:
/// 落盘必须走 [`persist_layout`](crate::persistence::persist_layout)(原子写
/// + 版本归一,见 `crate::persistence` 模块文档的宿主契约)。
pub fn save_layout(area: &Entity<DockArea>, cx: &App) -> DockResult<String> {
    let state = area.read(cx).dump(cx);
    Ok(serde_json::to_string(&state)?)
}

/// 把布局状态应用到 `DockArea`(上游 `DockArea::load` 的错误归一封装)。
///
/// 持久化契约的"读"半边(`load_layout` 得到 [`DockAreaState`] 后的落地点);
/// 未注册工厂的面板名会被上游落 `InvalidPanel` 占位(上游宽容策略,占位
/// 面板原样携带其 PanelState,下次 save 不丢数据)。
pub fn apply_layout(
    area: &Entity<DockArea>,
    state: DockAreaState,
    window: &mut Window,
    cx: &mut App,
) -> DockResult<()> {
    area.update(cx, |area, cx| {
        area.load(state, window, cx)
            .map_err(|err| DockError::Load(err.to_string()))
    })
}

/// 从 JSON 字符串恢复布局(原 `load_layout`;持久化契约落地后改现名以免与
/// 文件级 [`load_layout`](crate::persistence::load_layout) 撞名)。
///
/// 内部 = serde 反序列化 + [`apply_layout`]。**注意**:字符串来源不受契约
/// 保护,宿主从文件读布局必须走 `crate::persistence` 契约(原子写半边保证
/// 文件完整,损坏回退与版本迁移在读半边)。
pub fn restore_layout(
    area: &Entity<DockArea>,
    json: &str,
    window: &mut Window,
    cx: &mut App,
) -> DockResult<()> {
    let state: DockAreaState = serde_json::from_str(json)?;
    apply_layout(area, state, window, cx)
}

/// 注册面板重建工厂(转发 gpui-component 0.5.1 `register_panel`):
/// `load_layout` 恢复布局时按 `panel_name` 调用,把持久化的
/// [`PanelState`] 重建为面板。
pub fn register_panel_factory<F>(cx: &mut App, panel_name: &str, build: F)
where
    F: Fn(
            WeakEntity<DockArea>,
            &PanelState,
            &PanelInfo,
            &mut Window,
            &mut App,
        ) -> Box<dyn PanelView>
        + 'static,
{
    register_panel(cx, panel_name, build);
}
