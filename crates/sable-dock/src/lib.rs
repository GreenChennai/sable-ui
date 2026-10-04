//! # sable-dock — L4 面板系统
//!
//! 基于 gpui-component DockArea 的设计软件工作台:经典三段式(左图层素材/
//! 中画布/右属性)、面板便利层、布局持久化契约(原子写 + 坏文件回退 +
//! 版本迁移,用户工作区随偏好恢复,Illustrator "工作区" 功能)。
//! 手册:docs/03 §6;宿主契约见 [`workspace`] / [`persistence`] 模块文档。
//!
//! # 来源与署名
//!
//! DockArea 封装思路源自 **gpui-component(longbridge/gpui-kit,Apache-2.0)**,
//! NOTICE.md 已登记;本 crate 只做薄封装,未复制上游源码。锁 0.5.1 的
//! 理由见 [`workspace`] 模块文档(0.6+ 的 gpui-pre 类型世界与 zed gpui
//! 0.2.2 不互通)。
//!
//! # 模块速览
//!
//! | 模块 | 职责 |
//! |---|---|
//! | [`workspace`] | `WorkspacePresets` 三段式预设、`SablePanel` 面板包装、save/restore |
//! | [`persistence`] | 壳态持久化契约:原子写落盘 + 坏文件回退默认 + 版本迁移(RBT-02) |
//! | [`error`] | `DockError` / `DockResult` 统一错误 |
//! | [`window_effects`] | E3 窗口系统材质(Mica/Acrylic,需 `window-backdrop` feature)|
//!
//! # gpui / gpui-component 再导出
//!
//! `pub use gpui` / `pub use gpui_component`:门面 crate `sable` 经本 crate
//! 透出 `sable::gpui` / `sable::gpui_component`,让示例与上层应用免于各自
//! 声明同版本依赖(示例 Cargo.toml 只依赖 sable 的约定由此成立)。
//!
//! # 0.5.1 API 核实结论(2026-10,本地 registry 源码)
//!
//! - `DockArea::new(id, version, window, cx)` + `set_center` /
//!   `set_left_dock(DockItem, Option<Pixels>, bool, ..)` / `set_bottom_dock`
//!   / `set_right_dock`;`DockItem::tabs(Vec<Arc<dyn PanelView>>,
//!   &WeakEntity<DockArea>, window, cx)` / `DockItem::panel(..)` 链式
//!   `.size(px)` / `.active_index(ix)`;
//! - **单一 `Panel` trait**:`Entity<P: Panel>` 有 `PanelView` 毛毯实现,
//!   `Arc::new(entity)` 即对象安全句柄;
//! - 序列化:`dump`/`load`(serde)+ `register_panel`(名字 → 重建闭包);
//! - `DockArea` 自实现 `Render`,实体可直接作为元素上屏;外观由
//!   `ActiveTheme` 内置渲染(0.5.1 无 renderer seam)。

// deny 而非 forbid:E3 window_effects 调 windows-0.62 的 DwmSetWindowAttribute
// ——它是 `pub unsafe fn`(registry 源码核实,任务书"safe 绑定"假设不成立),
// unsafe 收口被精确 allow 在该文件唯一的 FFI 调用点内;workspace 纪律
// (AGENTS.md §3:#![deny(unsafe_code)])口径不变,其余位置出现 unsafe 仍会
// 被拒。
#![deny(unsafe_code)]

pub mod error;
pub mod persistence;
#[cfg(feature = "window-backdrop")]
pub mod window_effects;
pub mod workspace;

pub use error::{DockError, DockResult};
pub use persistence::{
    FallbackReason, LAYOUT_VERSION, LoadedLayout, load_layout, persist_layout, persist_layout_state,
};
pub use workspace::{
    SablePanel, WorkspacePresets, apply_layout, init, register_panel_factory, restore_layout,
    save_layout, tab_group,
};

/// gpui 再导出(见 crate 文档"再导出"节)。
pub use gpui;
/// gpui-component 再导出(见 crate 文档"再导出"节)。
pub use gpui_component;
