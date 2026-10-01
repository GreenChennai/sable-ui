//! # lumina-video — 时间轴数据模型
//!
//! Track/Clip/Keyframe、时间↔像素换算、吸附对齐、播放时钟、帧源 trait(解码进程隔离,分册六 §1.3)。
//! 数据模型与 UI 完全解耦:一切修改走 lumina-core 的 Command(分册四 §8)。
