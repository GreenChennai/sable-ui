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
//! | [`layer_tree`] | `layer-panel` + `theme`(V2.0 T2,与 layer_panel 同 feature)|
//! | [`neon_card`] | `anim` + `theme`(V4.0 对标组件,luminaui.in Neon Card 移植)|
//! | [`effect_stack`] | `inspector` + `theme`(V2.0 T4,效果栈面板)|
//! | [`inspector`] | `inspector` + `color` + `theme`(number-field/binding 由 Cargo 特性依赖保证)|
//! | [`timeline_view`] | `timeline` + `theme` |
//! | [`curve_editor`] | `curve-editor` + `theme` |
//! | [`input_method`] | —(IME 适配纯层,零新依赖,A11Y-08) |
//! | [`keymap`] | `controls` + `theme`(chord 展示复用 tooltip 单点,A11Y-04) |
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
/// 基础控件层(迭代审查报告 §5.6 批 1,CMP-01:Button/TextField/Select/… 此前全缺)。
#[cfg(all(feature = "controls", feature = "theme"))]
pub mod controls;
#[cfg(all(feature = "curve-editor", feature = "theme"))]
pub mod curve_editor;
/// 效果栈面板(V2.0 T4;inspector 特性链带来 number-field→anim,interact 的
/// hover_tint 随之可用,同 property_row 的门控口径)。
#[cfg(all(feature = "inspector", feature = "theme"))]
pub mod effect_stack;
#[cfg(feature = "anim")]
pub mod flip;
/// 字体随包(TOK-02):Inter/JetBrains Mono 嵌入与 gpui 文本系统注册,
/// fallback 链声明与真机走查步骤见模块 doc;无 feature 门控(纯资产,零新依赖)。
pub mod fonts;
#[cfg(all(feature = "color", feature = "binding", feature = "theme"))]
pub mod gradient_editor;
/// IME 输入法适配层(A11Y-08 第 4 组收口):平台事件 → 控件回调单点接线,
/// TextField/NumberField 的 EntityInputHandler 共用(UTF-16 换算/分支只此一份)。
pub mod input_method;
#[cfg(all(feature = "inspector", feature = "color", feature = "theme"))]
pub mod inspector;
#[cfg(feature = "anim")]
pub mod interact;
/// 键位注册绑定层(A11Y-04 第 4 组收口):键位以宿主注册表为单源
/// (docs/upstream/00 §4.7),库不做全局监听,只提供 ActionSpec 注册表、
/// chord 展示(单源复用 tooltip)与速查表生成。
#[cfg(all(feature = "controls", feature = "theme"))]
pub mod keymap;
#[cfg(all(feature = "layer-panel", feature = "theme"))]
pub mod layer_panel;
/// 树形图层面板(V2.0 T2)。
#[cfg(all(feature = "layer-panel", feature = "theme"))]
pub mod layer_tree;
#[cfg(feature = "theme")]
pub mod layout;
/// Neon Card(luminaui.in 对标移植,V4.0 对标组件;离屏自绘 + 纯函数动画)。
#[cfg(all(feature = "anim", feature = "theme"))]
pub mod neon_card;
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
    /// 批 3(ValueOverlay/Badge/Card/Slider)一站式导出。
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::badge::{BadgeVariant, badge, badge_style};
    /// 基础控件层(§5.6 批 1,CMP-01/02/04)一站式导出。
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::button::{
        Button, ButtonSize, ButtonStyle, ButtonVariant, IconButton, IconButtonSize, PRESS_SCALE,
        PressFn, button_element, button_height, button_style, button_text, icon_button_element,
        icon_button_style, solid_variant_foreground,
    };
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::card::{card, card_style};
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::choice::{
        Choice, ChoiceKind, choice_visual, control_knob, hit_height, key_checked_intent,
    };
    /// 浮层与状态件(CMP-03/05)一站式导出。
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::command_palette::{
        CommandEntry, CommandPalette, RankedEntry, fuzzy_match, palette_nav, push_recent,
        rank_entries,
    };
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::dialog::{
        DialogButton, DialogButtonKind, DialogHost, DialogSpec, dialog_key, focus_trap_next,
    };
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::empty_state::{EmptyState, EmptyStateSpec};
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::error_bar::{ErrorBar, ErrorVariant};
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::scroll_area::{
        ScrollArea, ScrollAxis, ScrollState, clamp_offset, max_offset, offset_from_drag,
        thumb_geometry,
    };
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::select::{Select, dropdown_opens_upward, select_nav};
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::skeleton::{Skeleton, SkeletonKind};
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::slider::{Slider, slider_key, value_fraction};
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::spinner::{Progress, Spinner};
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::tabs::{
        PanelTabs, Tabs, tab_colors, tab_height, tabs_nav, underline_fraction,
    };
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::text_field::{
        TextField, TextFieldBuffer, TextFieldSize, edit_key, text_field_height, word_range_at,
    };
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::toast::{
        OverlayCorner, ToastHost, ToastLevel, ToastSpec, evict_oldest, expired_ids,
    };
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::tooltip::{
        TooltipHost, TooltipSpec, format_keystroke, key_badge_text, place_tooltip, render_shortcut,
        tooltip_slot, tooltip_view,
    };
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::controls::value_overlay::{value_overlay, value_overlay_style};
    #[cfg(all(feature = "curve-editor", feature = "theme"))]
    pub use crate::curve_editor::{
        CurveEditor, CurvePreview, add_key, hit_key, key_at_pointer, move_key, remove_key,
        sample_curve,
    };
    #[cfg(all(feature = "inspector", feature = "theme"))]
    pub use crate::effect_stack::{
        EffectStackCallbacks, EffectStackPanel, EffectStackSpec, add_presets, demo_entries,
        effect_label, effect_stack_panel, row_move_enabled,
    };
    #[cfg(feature = "anim")]
    pub use crate::flip::FlipTracker;
    pub use crate::fonts::install as install_fonts;
    #[cfg(all(feature = "color", feature = "binding", feature = "theme"))]
    pub use crate::gradient_editor::GradientEditor;
    /// IME 适配层(A11Y-08)一站式导出:控件实现 [`ImeEditTarget`] 回调面,
    /// 平台事件经 [`UTF16_ADAPTER`] 路由。
    pub use crate::input_method::{
        ImeEditTarget, ImeEvent, InputMethodAdapter, UTF16_ADAPTER, Utf16InputMethodAdapter,
    };
    #[cfg(all(feature = "inspector", feature = "color", feature = "theme"))]
    pub use crate::inspector::{InspectorPanel, RowSpec, SectionSpec};
    #[cfg(feature = "anim")]
    pub use crate::interact::{
        DUR_INTERACT_MS, DUR_OVERLAY_MS, DUR_PANEL_MS, DUR_PULSE_MS, DUR_VIEW_JUMP_MS, HoverState,
        ListNavIntent, PulseState, focus_region, gradient_stop_nav, hit_size, hover_tint, list_nav,
        pressed_tint, timeline_seek_step,
    };
    #[cfg(all(feature = "anim", feature = "theme"))]
    pub use crate::interact::{
        InteractState, MIN_HIT_PX, Semantic, SemanticRole, attach_semantics, focus_ring,
        focus_ring_spec, hit_slot, state_layer,
    };
    /// 键位绑定层(A11Y-04)一站式导出:宿主注册 ActionSpec → 命令面板/
    /// 菜单/速查表共用,键位注册(`App::bind_keys`)属宿主。
    #[cfg(all(feature = "controls", feature = "theme"))]
    pub use crate::keymap::{
        ACTION_COMMAND_PALETTE, ACTION_CYCLE_TOOLS, ACTION_REDO, ACTION_UNDO, ActionSpec,
        CATEGORY_EDIT, CATEGORY_TOOLS, CATEGORY_VIEW, KeymapRegistry, SUGGESTED_CORE_ACTIONS,
        cheat_sheet, chord_display,
    };
    #[cfg(all(feature = "layer-panel", feature = "theme"))]
    pub use crate::layer_panel::{LayerPanel, layer_row_bg};
    #[cfg(all(feature = "layer-panel", feature = "theme"))]
    pub use crate::layer_tree::LayerTreePanel;
    #[cfg(all(feature = "anim", feature = "theme"))]
    pub use crate::neon_card::{
        NeonCardState, NeonCardStyle, frame_cache_key, neon_card, render_neon_card,
    };
    #[cfg(all(feature = "number-field", feature = "theme"))]
    pub use crate::number_field::NumberField;
    #[cfg(all(feature = "inspector", feature = "theme"))]
    pub use crate::property_row::PropertyRow;
    #[cfg(all(feature = "inspector", feature = "theme"))]
    pub use crate::property_row::section;
    #[cfg(feature = "theme")]
    pub use crate::theme::{
        CanvasTheme, SableTheme, ThemeMode, elevated as theme_elevated, init as init_theme,
        shadow as theme_shadow, shadow_quads as theme_shadow_quads, theme as sable_theme,
    };
    #[cfg(all(feature = "timeline", feature = "theme"))]
    pub use crate::timeline_view::TimelineView;
    #[cfg(feature = "theme")]
    pub use crate::tokens::{
        ColorTokens, ELEVATIONS, Elevation, MONO_FONT, MotionTokens, RadiusTokens, SPRING_BOUNCY,
        SPRING_SNAPPY, SPRING_SOFT, SpacingTokens, SpringPreset, StateLayerTokens, TEXT_SIZES,
        TextSize, UI_FONT, control_height, h_flex, v_flex,
    };
}
