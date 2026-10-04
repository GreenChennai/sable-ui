//! 基础控件层(迭代审查报告 §5.6 批 1,CMP-01)。
//!
//! 全部控件:纯 gpui 自绘(gpui-component 0.5.1 与 zed gpui 0.2.2 类型世界不互通,
//! 见 docs 报告 CMP-01),遵守 tokens 令牌(零硬编码色)、state_layer 三态、
//! disabled"仅前景降级"规则、reduced_motion 直通纪律。
pub mod button;
pub mod choice;
pub mod scroll_area;
pub mod select;
pub mod tabs;
pub mod text_field;
pub mod tooltip;
