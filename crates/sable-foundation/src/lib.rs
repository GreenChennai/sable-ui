//! # sable-foundation — L0 基础层
//!
//! 视口变换、场景图(纯数据)、命令系统(撤销重做)、原子写持久化。
//!
//! # 零 UI 框架依赖纪律(分册六 §2.1)
//!
//! 依赖白名单 = `kurbo` / `slotmap`(serde)/ `serde` / `thiserror`;
//! 出现 gpui/egui/wgpu 依赖视为架构事故,CI 以 `--no-default-features`
//! 单独编译把关。原白名单中的 `peniko` 因本 crate 类型设计(自有
//! [`scene::Paint`] 枚举)用不到,已从 Cargo.toml 移除——不为用而用。
//!
//! # 架构决策:Scene 场景图从 sable-canvas 下沉进本 crate
//!
//! 分册二原文把场景图放在 sable-canvas;这里下沉到 sable-foundation,原因:
//! **Command/History 必须能命名被编辑的状态**(AGENTS.md §3.1:一切文档修改
//! 走 Command),而 Scene 是纯数据(kurbo/slotmap/serde 全在 core 白名单内),
//! 不依赖任何行为层。canvas crate 保留行为层(命中测试/网格/渲染调度)。
//!
//! # 模块速览
//!
//! | 模块 | 职责 | 手册 |
//! |---|---|---|
//! | [`viewport`] | 世界坐标 ↔ 屏幕坐标换算、锚点缩放 | docs/02 §2 |
//! | [`scene`] | 场景图:节点/子树/渲染列表/包围盒,随时可 serde | docs/02 §3 |
//! | [`command`] | Command trait、History 撤销栈、内置命令集 | docs/03 §2 |
//! | [`persistence`] | atomic_write 原子写、自动保存目录与轮换 | docs/06 §1.2 |
//! | [`error`] | `CoreError` / `CoreResult` 统一错误 | — |

pub mod command;
pub mod error;
pub mod persistence;
pub mod project;
pub mod scene;
pub mod viewport;

/// 常用类型一站式 re-export:`use sable_foundation::prelude::*;`
pub mod prelude {
    pub use crate::command::{
        AddNode, BatchCommand, Command, History, RemoveNode, Reparent, SetFill, SetName,
        SetOpacity, SetStroke, SetTransform, SetVisibility,
    };
    pub use crate::error::{CoreError, CoreResult};
    pub use crate::persistence::{AutosavePlan, atomic_write, autosave_dir};
    pub use crate::scene::{
        BlendMode, GradientStop, IdRemap, ImageNode, MeshGradient, Node, NodeContent, NodeId,
        Paint, PathNode, RemovedSubtree, Rgba8, Scene, StrokeStyle, TextNode,
    };
    pub use crate::viewport::Viewport;
}
