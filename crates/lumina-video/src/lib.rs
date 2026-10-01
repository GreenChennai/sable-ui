//! # lumina-video — 时间轴数据模型
//!
//! Track/Clip/Keyframe 数据模型(docs/04 §8)、关键帧缓动求值(docs/04 §9)、
//! 吸附对齐、撤销重做、播放时钟(docs/03 §7)、帧源 trait。数据模型与 UI 完全
//! 解耦:一切时间轴修改走 [`command::TimelineCommand`],一切类型可 serde
//! (未来直接进 .lumi/.cutforge 工程文件)。
//!
//! # 架构决策
//!
//! ## 1. 时间轴撤销不复用 lumina-core 的 Command/History(同构而独立)
//!
//! core 的 History 为 slotmap 场景图内置 id 治愈机制(`remap_ids`/`IdRemap`
//! 广播):节点删除再撤销会**换发 id**,历史条目必须跟着重映射。时间轴没有这个
//! 问题——[`model::ClipId`] 是稳定自增 u64、**永不复用**(连跳号都不回收),
//! 因此撤销机制简单得多:
//!
//! - 撤销不需要 id 重映射,[`command::TimelineCommand::revert`] 无返回值;
//! - 历史条目的失效引用 = 目标已删除 = 静默跳过(core 同款 LIFO 约定),
//!   且**永不误伤别的 clip**(id 永不复用保证);
//! - 涉及新 id 的命令(`PlaceClip`/`SplitClip`)apply 时缓存铸造结果,redo
//!   复用同一 id。
//!
//! 对应手册 docs/03 的分工说明:core 管场景图撤销,video 管时间轴撤销。
//!
//! ## 2. v0.1 不接 ffmpeg/symphonia
//!
//! 解码走 [`frame::FrameSource`] trait 抽象(依赖契约:不新增运行时依赖);
//! [`frame::SyntheticSource`] 彩条合成源让无 ffmpeg 环境下的示例/测试有帧可放。
//! 真解码器(进程隔离,分册六 §1.3)留给 v0.2。
//!
//! ## 3. 播放泵不在本 crate(docs/03 §7)
//!
//! [`clock::Player`] 只是纯状态机:`tick` 由 UI 泵(timer 驱动、16ms 一拍)
//! 调用,UI 只订阅"时间码变化"。本 crate 不引线程、不引 timer。
//!
//! ## 4. 关键帧时间域
//!
//! [`model::Keyframe::t_ms`] 为 **clip 本地时间**(clip 起点 = 0):移动 clip
//! 关键帧随之移动;分割 clip 时按本地分割点切分两半。
//!
//! # 模块速览
//!
//! | 模块 | 职责 | 手册 |
//! |---|---|---|
//! | [`model`] | Timeline/Track/Clip 不变量维护、吸附、时间↔像素换算 | docs/04 §8 |
//! | [`command`] | TimelineCommand/TimelineHistory + 内置命令集 | docs/04 §8、docs/03 §2 |
//! | [`curve`] | 关键帧求值、预设缓动公式 | docs/04 §9 |
//! | [`clock`] | 播放时钟纯状态机 | docs/03 §7 |
//! | [`frame`] | FrameSource 帧源抽象、彩条合成源 | docs/04 §12 |
//! | [`error`] | `VideoError` / `VideoResult` 统一错误 | — |

pub mod clock;
pub mod command;
pub mod curve;
pub mod error;
pub mod frame;
pub mod model;

/// 常用类型一站式 re-export:`use lumina_video::prelude::*;`
pub mod prelude {
    pub use crate::clock::Player;
    pub use crate::command::{
        BatchCommand, MoveClip, PlaceClip, RemoveClip, RemoveClipRipple, SetMuted, SetSpeed,
        SplitClip, TimelineCommand, TimelineHistory, TrimClip,
    };
    pub use crate::curve::evaluate;
    pub use crate::error::{VideoError, VideoResult};
    pub use crate::frame::{Frame, FrameSource, SyntheticSource};
    pub use crate::model::{
        AssetRef, Clip, ClipId, Easing, Keyframe, Timeline, Track, TrackKind, snap_time,
    };
}
