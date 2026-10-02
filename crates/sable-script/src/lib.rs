//! # sable-script — 自动化脚本地基(迭代计划 09 T4 / G16)
//!
//! Rhai 沙盒 + 场景算子:**对标 Blender Python console / Figma plugin 的
//! 最小形态——沙盒内可编程操作场景**。v0.1 面向宿主侧批处理(测试、生成式
//! 排版、自动化验收),交互式 REPL 由后续迭代在 [`ScriptHost`] 之上搭建。
//!
//! # 架构铁律(AGENTS.md §3.1)
//!
//! **一切写操作走 Command 进 History,天然可撤销。** 脚本算子不直接改
//! `Scene`:每个算子在内部组装对应的内置命令(`AddNode`/`SetFill`/
//! `SetName`/`RemoveNode`)后经 `History::exec` 生效——脚本操作与 UI 操作
//! 共用同一撤销栈,`undo()`/`redo()` 算子就是 `History::undo`/`History::redo`
//! 本身。proptest 既有机制由此直接覆盖脚本路径。
//!
//! # 沙盒边界
//!
//! - rhai 默认标准包**零文件/网络访问**:接入方不得注册任何提供文件/
//!   网络能力的包(如官方插件 crate 的 fs/http 包),否则沙盒承诺失效;
//! - `print`/`debug` 仅写 stdout(可用 [`ScriptHost::engine_mut`] 经
//!   `on_print`/`on_debug` 重定向到宿主日志);
//! - 资源限制默认沿用 rhai 出厂值(表达式深度/调用层级有界,操作数不限),
//!   需要更严的沙盒经 [`ScriptHost::engine_mut`] 的
//!   `set_max_operations`/`set_max_call_levels`/`set_max_expr_depths`
//!   收紧(0 = 不限);
//! - 算子闭包不注册任何 panic 路径:可失败一律返回 `Result`,rhai 转成
//!   脚本运行时错误。
//!
//! # 脚本速览
//!
//! ```rhai
//! let g = add_group("棋盘");
//! for i in 0..8 {
//!     for j in 0..8 {
//!         let v = (i + j) % 2 == 0 ? 230 : 30;
//!         let id = add_rect(i * 40.0, j * 40.0, (i + 1) * 40.0, (j + 1) * 40.0,
//!                           v, v, v, 255, g);
//!     }
//! }
//! undo_len();   // 步数 = 已执行的算子数(每个算子各占一步撤销)
//! ```
//!
//! 数值参数接受整数/浮点任意混合(内部统一转 f64);颜色通道钳到 0..=255;
//! 节点 id 以 `i64` 形态进出脚本(slotmap key 的 ffi 表示)。撤销删除后
//! 恢复的节点换发新 id,脚本持有的旧 id 失效——与 UI 撤销同一既定语义
//! (sable-foundation command.rs)。
//!
//! # 编译时长代价
//!
//! rhai 是本工作区最重的编译依赖(全量编译约 30s 级)。本 crate 不进任何
//! 默认编译路径,feature 矩阵接入由门面(`crates/sable`)决定;workspace
//! 里 rhai 只开 default features,不引 serde/internals/decimal 等扩展面。

#![deny(unsafe_code)]

pub mod api;
pub mod error;

#[cfg(test)]
mod tests;

/// 常用类型一站式 re-export:`use sable_script::prelude::*;`
pub mod prelude {
    pub use crate::api::ScriptHost;
    pub use crate::error::{ScriptError, ScriptResult};
}
