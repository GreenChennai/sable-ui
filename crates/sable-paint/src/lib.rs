//! # sable-paint — L2 渲染层
//!
//! 画笔/渐变/描边绘制原语(对 Vello/vello_cpu 的统一封装)、
//! GPU 后端选择(auto/DX12/Vulkan,分册六 §6.3)、GpuGuard 设备丢失恢复(§1.1)。
//!
//! 手册:docs/02 §4(渲染管线)、docs/06 §1.1/§6。
//!
//! ## 降级链(分册六 §6.4,按序尝试)
//!
//! 1. **GPU 零拷贝**:wgpu 30 + vello 0.10 渲染到纹理,跨 API 共享(NT handle)上屏;
//! 2. **读回兜底**:共享失败 → `copy_texture_to_buffer` 读回 + 图片上传;
//! 3. **切 DX12**:无 Vulkan 设备 → `SABLE_GPU_BACKEND=dx12` 重试;
//! 4. **vello_cpu 软渲染**:全部失败 → [`cpu::CpuRenderer`] + 提示升级驱动。
//!
//! v0.1 落地:cpu 全量([`sink::PaintSink`] 双实现中的 CPU 侧)、gpu 全链路接线
//! (后端选择/设备创建/`GpuGuard`/`VelloSink`,以及生产帧入口
//! [`gpu_frame::GpuFrameRenderer`]:每帧经 `render_with_recovery` 包装幂等帧闭包,
//! 资源按 `(generation, resource)` 配对缓存,二连败返回宿主契约信号
//! `GpuDeviceLostFatal`——RBT-01)。
//!
//! 上层(sable-canvas)只依赖 [`sink::PaintSink`],降级 = 换 Sink,场景图零改动。

#![forbid(unsafe_code)]

pub mod error;
pub mod noise;
pub mod shape;
pub mod sink;
pub mod style;

#[cfg(feature = "cpu")]
pub mod cpu;
pub mod effects;
#[cfg(feature = "gpu")]
pub mod gpu;
#[cfg(feature = "gpu")]
pub mod gpu_frame;

pub use error::{PaintError, PaintResult};

/// 一站式导入:上层面向此 prelude 编程。
pub mod prelude {
    #[cfg(feature = "cpu")]
    pub use crate::cpu::{CpuRenderer, VelloCpuSink};
    #[cfg(feature = "cpu")]
    pub use crate::effects::{ShadowCache, ShadowParams, render_shadow_rgba};
    pub use crate::error::{PaintError, PaintResult};
    #[cfg(feature = "gpu")]
    pub use crate::gpu::{
        FrameError, FrameRecovery, GpuGuard, VelloSink, create_device, create_instance,
        render_with_recovery,
    };
    #[cfg(feature = "gpu")]
    pub use crate::gpu_frame::{FrameOutput, GpuDegradation, GpuFrameRenderer};
    pub use crate::sink::PaintSink;
    pub use crate::style::{to_brush, to_color, to_stroke, to_vello_blend, with_opacity};
    pub use sable_foundation::scene::{BlendMode, GradientStop, Paint, Rgba8, StrokeStyle};
}
