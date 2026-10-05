//! 基础控件层(迭代审查报告 §5.6 批 1,CMP-01;批 2:CMP-03/05 浮层与状态件;
//! 批 3:ValueOverlay/Badge/Card/Slider)。
//!
//! 全部控件:纯 gpui 自绘(gpui-component 0.5.1 与 zed gpui 0.2.2 类型世界不互通,
//! 见 docs 报告 CMP-01),遵守 tokens 令牌(零硬编码色)、state_layer 三态、
//! disabled"仅前景降级"规则、reduced_motion 直通纪律。
pub mod badge;
pub mod button;
pub mod card;
pub mod choice;
pub mod command_palette;
pub mod dialog;
pub mod empty_state;
pub mod error_bar;
pub mod scroll_area;
pub mod select;
pub mod skeleton;
pub mod slider;
pub mod spinner;
pub mod tabs;
pub mod text_field;
pub mod toast;
pub mod tooltip;
pub mod value_overlay;
