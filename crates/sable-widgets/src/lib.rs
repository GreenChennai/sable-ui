//! # sable-widgets — L5 专用组件层
//!
//! 设计 token(分册六 §3)、主题(分册三 §5)、动画引擎(§4.2)、
//! NumberField/色轮/渐变编辑器/图层面板/属性检查器/时间轴视图/曲线预览
//! (分册四)。组件三件套约定:无状态 RenderOnce / 有状态 Render+Entity /
//! 受控 Binding(分册三 §3)。
//!
//! # 模块与 feature 门控(分册六 §2.2/§2.3 点菜制)
//!
//! | 模块 | 需要 feature |
//! |---|---|
//! | [`tokens`] / [`theme`] | `theme` |
//! | [`anim`] | `anim` |
//! | [`anim::bridge`] | `anim` + `timeline`(A11 关键帧桥) |
//! | [`flip`] / [`interact`] | `anim`(A4 FLIP / A7 微交互接线) |
//! | [`binding`] | `binding` |
//! | [`property_row`] | `inspector` + `theme` |
//! | [`number_field`] | `number-field` + `theme` |
//! | [`color`] | `color` + `theme` |
//! | [`gradient_editor`] | `color` + `binding` + `theme` |
//! | [`layer_panel`] | `layer-panel` + `theme` |
//! | [`inspector`] | `inspector` + `color` + `theme`(number-field/binding 由 Cargo 特性依赖保证)|
//! | [`timeline_view`] | `timeline` + `theme` |
//! | [`curve_editor`] | `curve-editor` + `theme` |
//!
//! 说明:`default = ["full"]` 下全部可用。组件零硬编码颜色的纪律(AGENTS.md
//! §3.4)使**一切组件依赖 `theme`**——单独点菜某组件而不开 `theme` 时,该
//! 模块编译期缺席(而非暗色兜底),这是显式契约。Cargo.toml 不可改(上层
//! 统一管理),故以 mod 门控落地。
//!
//! # gpui 生态适配(2026-10 源码级核实,详见各模块 doc)
//!
//! - gpui 0.2.2(crates.io 版,非 Zed git 版):`Pixels` 字段 crate 私有,
//!   取值用 `f32::from/f64::from(px)`;`uniform_list` 闭包无 view 参数;
//!   无 `impl IntoElement for Option`;`h_flex/v_flex` 未内置(见 [`tokens`])。
//! - gpui-component 0.7.0 内部跑在 gpui-pre 0.3.7 类型世界,与 gpui 0.2.2
//!   **类型不互通**,本 crate 刻意全部纯 gpui 实现,不 import 之。

#![deny(unsafe_code)]

#[cfg(feature = "anim")]
pub mod anim;
#[cfg(feature = "binding")]
pub mod binding;
#[cfg(all(feature = "color", feature = "theme"))]
pub mod color;
#[cfg(all(feature = "curve-editor", feature = "theme"))]
pub mod curve_editor;
#[cfg(feature = "anim")]
pub mod flip;
#[cfg(all(feature = "color", feature = "binding", feature = "theme"))]
pub mod gradient_editor;
#[cfg(all(feature = "inspector", feature = "color", feature = "theme"))]
pub mod inspector;
#[cfg(feature = "anim")]
pub mod interact;
#[cfg(all(feature = "layer-panel", feature = "theme"))]
pub mod layer_panel;
#[cfg(all(feature = "number-field", feature = "theme"))]
pub mod number_field;
#[cfg(all(feature = "inspector", feature = "theme"))]
pub mod property_row;
#[cfg(feature = "theme")]
pub mod theme;
#[cfg(all(feature = "timeline", feature = "theme"))]
pub mod timeline_view;
#[cfg(feature = "theme")]
pub mod tokens;

/// 常用类型一站式 re-export:`use sable_widgets::prelude::*;`
pub mod prelude {
    #[cfg(feature = "anim")]
    pub use crate::anim::{
        AnimScheduler, Animated, AnimationTimeline, Easing, GestureTracker, Lerp, ScrollPhysics,
        Spring, cubic_bezier_y, lerp_hsla, reduced_motion, set_reduced_motion,
    };
    #[cfg(feature = "binding")]
    pub use crate::binding::Binding;
    #[cfg(all(feature = "color", feature = "theme"))]
    pub use crate::color::{ColorWell, ColorWheel};
    #[cfg(all(feature = "curve-editor", feature = "theme"))]
    pub use crate::curve_editor::CurvePreview;
    #[cfg(feature = "anim")]
    pub use crate::flip::FlipTracker;
    #[cfg(all(feature = "color", feature = "binding", feature = "theme"))]
    pub use crate::gradient_editor::GradientEditor;
    #[cfg(all(feature = "inspector", feature = "color", feature = "theme"))]
    pub use crate::inspector::{InspectorPanel, RowSpec, SectionSpec};
    #[cfg(feature = "anim")]
    pub use crate::interact::{
        DUR_INTERACT_MS, DUR_OVERLAY_MS, DUR_PANEL_MS, DUR_PULSE_MS, DUR_VIEW_JUMP_MS, HoverState,
        PulseState, hover_tint, pressed_tint,
    };
    #[cfg(all(feature = "layer-panel", feature = "theme"))]
    pub use crate::layer_panel::LayerPanel;
    #[cfg(all(feature = "number-field", feature = "theme"))]
    pub use crate::number_field::NumberField;
    #[cfg(all(feature = "inspector", feature = "theme"))]
    pub use crate::property_row::PropertyRow;
    #[cfg(all(feature = "inspector", feature = "theme"))]
    pub use crate::property_row::section;
    #[cfg(feature = "theme")]
    pub use crate::theme::{
        CanvasTheme, SableTheme, ThemeMode, init as init_theme, theme as sable_theme,
    };
    #[cfg(all(feature = "timeline", feature = "theme"))]
    pub use crate::timeline_view::TimelineView;
    #[cfg(feature = "theme")]
    pub use crate::tokens::{
        ColorTokens, RadiusTokens, SpacingTokens, control_height, h_flex, v_flex,
    };
}
