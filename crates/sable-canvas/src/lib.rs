//! # sable-canvas — L3 画布内核
//!
//! 场景渲染调度、无限画布换算、命中测试、自适应网格、脏矩形、LOD、
//! Parley 文本管线、framework-free 工具状态机、GPUI 胶水。
//!
//! 手册:docs/02(全册)、docs/03 §4(工具状态机)、docs/04 §3(钢笔)。
//! 场景图/命令系统已下沉 sable-foundation(见 sable-foundation lib.rs 的架构决策注记),
//! 本 crate 是其上的**行为层**。
//!
//! # 模块速览
//!
//! | 模块 | 职责 | 手册 |
//! |---|---|---|
//! | [`render`] | 场景 → PaintSink 调度、视锥剔除、选中框/控制柄覆盖层 | docs/02 §4.2 |
//! | [`grid`] | 自适应背景网格 | docs/02 §4.3 |
//! | [`hit_test`] | 点选/框选(extension trait `SceneHitTest`) | docs/02 §5 |
//! | [`damage`] | 脏矩形 | docs/02 §7.1 |
//! | [`lod`] | 细节层次(4px 阈值) | docs/02 §7.2 |
//! | [`text`] | Parley 0.11 排版/测量(字形绘制 = M2) | docs/02 §6 |
//! | [`input`] | 滚轮缩放/平移、命中容差(纯函数) | docs/02 §8 |
//! | [`tool`] | 工具状态机(Select/Pen/Hand;不引 gpui) | docs/03 §4.2、docs/04 §3 |
//! | [`gpui_element`] | `SableCanvas` Entity(feature `gpui`+`cpu`) | docs/02 §4.2 |
//!
//! # 坐标纪律(全 crate 一致)
//!
//! 一切公共 API 为 f64 世界坐标;屏幕坐标只在事件处理与绘制瞬间出现;
//! f32 只存在于后端内部(vello/vello_cpu/gpui)。
//!
//! # feature 矩阵
//!
//! - `cpu`(默认):转发 sable-paint/cpu,`render` 逐像素测试由此驱动;
//! - `gpu`:转发 sable-paint/gpu(本 crate 只面向 `PaintSink`,天然兼容);
//! - `gpui` + `cpu`:`gpui_element` 模块(GPUI Entity 胶水);
//! - `png`:workspace `image` crate(optional;**任务豁免项**)——gpui 0.2.2
//!   的 `RenderImage::new` 只吃 `image` crate 的 `animation::Frame`,裸帧
//!   上屏必需(见 `gpui_element` 模块 doc;无 PNG 编码步骤)。

#![forbid(unsafe_code)]

pub mod damage;
pub mod grid;
pub mod hit_test;
pub mod input;
pub mod lod;
pub mod render;
pub mod text;
pub mod tool;

/// GPUI 胶水:需要 `gpui` feature 且至少一个渲染后端(当前 `cpu`)。
#[cfg(all(feature = "gpui", feature = "cpu"))]
pub mod gpui_element;

/// 常用类型一站式 re-export:`use sable_canvas::prelude::*;`
pub mod prelude {
    pub use crate::damage::DamageTracker;
    pub use crate::grid::{draw_grid, grid_step_world};
    pub use crate::hit_test::SceneHitTest;
    pub use crate::input::{on_scroll, screen_tolerance};
    pub use crate::lod::{DetailLevel, detail_level, should_draw_detail};
    pub use crate::render::{OverlayTheme, RenderOpts, render_scene};
    pub use crate::text::TextPipeline;
    pub use crate::tool::{
        CursorStyle, HandTool, Mods, PenTool, SelectTool, ToolBehavior, ToolCtx,
    };
}

#[cfg(test)]
mod tests {
    /// 冒烟:prelude 一站式导入可用、公共面完整。
    #[test]
    fn prelude_imports_resolve() {
        use crate::prelude::*;

        // 类型可默认构造
        let _tracker = DamageTracker::default();
        let _opts = RenderOpts::default();
        let _theme = OverlayTheme::default();
        let _pipeline = TextPipeline::new();
        let _select = SelectTool::default();
        let _pen = PenTool::default();
        let _hand = HandTool::default();

        // 自由函数与 trait 方法可解析
        assert!(grid_step_world(1.0) > 0.0);
        assert!((screen_tolerance(2.0) - 2.0).abs() < 1e-12);
        assert_eq!(detail_level(10.0, 1.0), DetailLevel::Full);
        let scene = sable_foundation::scene::Scene::new();
        let _ = scene.hit_test(kurbo::Point::new(0.0, 0.0), 1.0);
        let mut vp = sable_foundation::viewport::Viewport {
            zoom: 1.0,
            pan: kurbo::Vec2::ZERO,
        };
        on_scroll(
            &mut vp,
            kurbo::Point::ZERO,
            kurbo::Vec2::new(0.0, 5.0),
            true,
        );
        assert!((vp.zoom - 1.1).abs() < 1e-12);
    }
}
