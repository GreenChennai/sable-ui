//! # sable — Sable UI 门面
//!
//! 用户唯一需要依赖的 crate。feature 矩阵见 Cargo.toml(分册六 §2.2),
//! 按 feature 把子 crate 逐一 re-export,不想要的部分不进编译产物。
//!
//! # feature 点菜表(Cargo.toml 注释的镜像)
//!
//! | 想要 | feature 写法 | 得到 |
//! |---|---|---|
//! | 完整设计软件 | `sable = "0.1"`(default `full`) | core+canvas+widgets+dock+video |
//! | CLI 只要命令系统 | `default-features = false, features = ["core"]` | sable-foundation 视口/场景图/命令 |
//! | 服务器 SVG→PNG(无 GPU) | `default-features = false, features = ["core", "cpu-render"]` | + sable-paint cpu 兜底 |
//! | 小工具要画布不要视频 | `default-features = false, features = ["canvas"]` | + gpui 链路画布(gpui+png) |
//! | 时间轴模型 | `features = ["video"]`(含 canvas) | + sable-video |
//! | GPU 后端三选一 | `backend-auto` / `backend-vulkan` / `backend-dx12` | sable-paint 后端 |
//!
//! # gpui / gpui-component 从哪来
//!
//! 本 crate 的 Cargo.toml 无直接 gpui 依赖(依赖白名单纪律);gpui 经
//! `sable-dock` 的 `pub use gpui` / `pub use gpui_component` 链式再导出为
//! **`sable::gpui`** / **`sable::gpui_component`**(需 `dock` feature,
//! 默认 full 恒开)。只点 `canvas` 不点 `dock` 的场景请自行声明 gpui 依赖。
//!
//! # 惯用法
//!
//! ```ignore
//! use sable::prelude::*;          // core/canvas/video 常用类型一站导入
//! use sable::gpui;                // gpui 0.2.2(Application/Window/…)
//! use sable::gpui_component;      // DockArea/Root/Button/主题
//! let doc = sable::core::scene::Scene::new();
//! sable::dock::init(cx);          // 主题 + dock 子系统(幂等)
//! ```

#![forbid(unsafe_code)]

// —— 子 crate 按(feature)逐一 re-export(短名,与文档/示例一致)——
/// L2 渲染层(基础绘制原语,随 paint 依赖恒可用)。
pub use sable_paint as paint;

/// L3 画布内核(feature `canvas`,连带开启 `core`)。
#[cfg(feature = "canvas")]
pub use sable_canvas as canvas;
/// L4 Dock 工作台(feature `dock`,连带开启 `widgets`)。
#[cfg(feature = "dock")]
pub use sable_dock as dock;
/// L0 视口/场景图/命令系统(feature `core`)。
#[cfg(feature = "core")]
pub use sable_foundation as core;
/// 时间轴模型(feature `video`,连带开启 `canvas`)。
#[cfg(feature = "video")]
pub use sable_video as video;
/// L5 专用组件层(feature `widgets`,连带开启 `canvas`)。
#[cfg(feature = "widgets")]
pub use sable_widgets as widgets;

// —— 基础类型直通(07 报告 P2-2)——
/// 2D 几何库(BezPath/Affine/Rect/Point…),版本与本库内部完全对齐;
/// 用户代码应 `use sable::kurbo;` 而非自行依赖 kurbo(免版本对齐负担)。
pub use kurbo;
/// 样式原语(Color/Gradient/Brush),与渲染层同源。
pub use peniko;

// —— 平台层链式再导出(经 sable-dock,见 crate 文档)——
/// gpui 0.2.2(feature `dock`;`canvas`-only 场景不导出,自行声明依赖)。
#[cfg(feature = "dock")]
pub use sable_dock::gpui;
/// gpui-component 0.5.1(feature `dock`;0.6+ 已迁 gpui-pre 类型世界,本库不跟)。
#[cfg(feature = "dock")]
pub use sable_dock::gpui_component;

/// 常用类型一站式导入:`use sable::prelude::*;`
///
/// 只汇总各子 crate 已有的 prelude 与 dock 顶层惯用件;widgets 组件
/// (LayerPanel/InspectorPanel/TimelineView 等)直接经 `sable::widgets::`
/// 使用,不在此展开(其导出面由 sable-widgets 定)。
pub mod prelude {
    /// L3 画布行为层常用类型(工具/渲染/命中测试)。
    #[cfg(feature = "canvas")]
    pub use sable_canvas::prelude::*;
    /// L0 场景图/命令系统常用类型。
    #[cfg(feature = "core")]
    pub use sable_foundation::prelude::*;
    /// 时间轴模型常用类型(Player/Timeline/TimelineHistory/命令集)。
    ///
    /// 显式列表而非 glob:video prelude 的 `BatchCommand` 与 core prelude 的
    /// `BatchCommand` 是两个不同类型,双重 glob 再导出会使命名歧义;该名请经
    /// `sable::video::prelude::BatchCommand`(或 `sable::video::command::`)限定使用。
    #[cfg(feature = "video")]
    pub use sable_video::prelude::{
        AssetRef, Clip, ClipId, Frame, FrameSource, Keyframe, MoveClip, PlaceClip, Player,
        RemoveClip, RemoveClipRipple, SetMuted, SetSpeed, SplitClip, SyntheticSource, Timeline,
        TimelineCommand, TimelineHistory, Track, TrackKind, TrimClip, VideoError, VideoResult,
        evaluate, snap_time,
    };

    /// dock 工作台惯用件(重命名 `init` 避免与将来其它 init 撞名)。
    /// `load_layout`/`persist_layout` 为壳态持久化契约(RBT-02);
    /// `restore_layout`/`save_layout` 为 JSON 字符串层便利(非落盘契约)。
    #[cfg(feature = "dock")]
    pub use sable_dock::{
        LoadedLayout, SablePanel, WorkspacePresets, init as init_dock, load_layout, persist_layout,
        restore_layout, save_layout,
    };
}

#[cfg(test)]
mod tests {
    /// 冒烟:full 组合下门面的再导出面完整可解析。
    #[test]
    fn facade_reexports_resolve() {
        // 子 crate 命名空间可达(widgets 顶层无 init,入口在 theme 模块)
        let _scene = crate::core::scene::Scene::new();
        let _timeline = crate::video::model::Timeline::new();
        let _theme_init = crate::widgets::theme::init;
        let _theme_get = crate::widgets::theme::theme;

        // 平台层链式再导出(经 sable-dock)
        let _ = crate::gpui::px(1.0);
        let _ = std::any::TypeId::of::<crate::gpui_component::dock::DockArea>();
    }
}
