//! # lumina-paint — L2 渲染层
//!
//! 画笔/渐变/描边绘制原语(对 Vello/vello_cpu 的统一封装)、
//! GPU 后端选择(auto/DX12/Vulkan,分册六 §6.3)、GpuGuard 设备丢失恢复(§1.1)。
//!
//! 手册:docs/02 §4(渲染管线)、docs/06 §1.1/§6。
