//! GPU 渲染后端(vello 0.10 + 其 re-export 的 wgpu 29.x),`gpu` feature 门控,默认关闭。
//!
//! v0.1 为**骨架**:后端选择(分册六 §6.3)、设备创建、[`GpuGuard`] 设备丢失恢复
//! (分册六 §1.1)、[`VelloSink`](`PaintSink` → `vello::Scene`)。CI 默认不开 gpu,
//! 本模块不携带需要 GPU 才能跑的测试。
//!
//! # 版本对齐纪律(2026-10-02 S0 勘误,07 报告 P0)
//!
//! 本模块的 wgpu 类型**一律经 `vello::wgpu` re-export 使用**(vello lib.rs
//! `pub use wgpu;`),sable-paint 不直接依赖 wgpu —— 单一真相源,workspace
//! 与 vello 的 wgpu 版本永远不可能错配(错配即编译错误,而非运行期类型坑)。
//! 升级窗口更换 vello 版本时,本模块随其 wgpu 自动跟进。
//!
//! wgpu 29.0.4 实测 API(与 30 的差异比预期小):
//! - `Instance::new` 按**值**取 `InstanceDescriptor`;无 `Default`,基座用
//!   `new_without_display_handle()`;
//! - **flags 常量名是 `DEBUG`**(wgpu 29/30 相同;旧名 `DEBUG_MARKERS` 已废弃,
//!   手册分册六 §6.3 原稿的写法在两代都编译不过);
//! - `RequestAdapterOptions` 无 `apply_limit_buckets` 字段(wgpu 30 独有,勿加);
//! - `request_adapter` 返回 `Result`;`request_device` 返回 `(Device, Queue)`;
//! - `Surface::get_current_texture` 返回 `CurrentSurfaceTexture` 枚举(非
//!   `Result<_, SurfaceError>`),恢复逻辑用自有 [`FrameError`] 分类。
//!
//! API 依据:本地 registry 源码 wgpu-29.0.4 / wgpu-types-29.0.4 / vello-0.10.0。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use kurbo::{Affine, BezPath};
use sable_foundation::scene::{Paint, StrokeStyle};
use vello::wgpu;

use crate::error::{PaintError, PaintResult};
use crate::sink::PaintSink;
use crate::style;

/// 运行期后端覆盖的环境变量(分册六 §6.3:设置界面写入后映射为此变量,重启生效)。
pub const BACKEND_ENV: &str = "SABLE_GPU_BACKEND";

/// 编译期 feature 决定的默认后端:
/// `backend-vulkan` > `backend-dx12` > `backend-auto`/无(= `PRIMARY`)。
/// 同时开 vulkan+dx12 两个 feature 时 vulkan 优先。
fn compile_time_backends() -> wgpu::Backends {
    #[cfg(feature = "backend-vulkan")]
    let selected = wgpu::Backends::VULKAN;
    #[cfg(all(feature = "backend-dx12", not(feature = "backend-vulkan")))]
    let selected = wgpu::Backends::DX12;
    #[cfg(all(not(feature = "backend-vulkan"), not(feature = "backend-dx12")))]
    let selected = wgpu::Backends::PRIMARY;
    selected
}

/// 后端选择:环境变量 `SABLE_GPU_BACKEND` > 编译期 feature > 平台默认。
/// 环境值大小写不敏感,接受 `vulkan`/`dx12`(其余值回落编译期默认)。
pub fn select_backends() -> wgpu::Backends {
    match std::env::var(BACKEND_ENV) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "vulkan" => wgpu::Backends::VULKAN,
            "dx12" => wgpu::Backends::DX12,
            _ => compile_time_backends(),
        },
        Err(_) => compile_time_backends(),
    }
}

/// 创建 wgpu 实例。debug 构建开校验/调试标记,release 关闭。
pub fn create_instance() -> wgpu::Instance {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = select_backends();
    descriptor.flags = if cfg!(debug_assertions) {
        wgpu::InstanceFlags::VALIDATION | wgpu::InstanceFlags::DEBUG
    } else {
        wgpu::InstanceFlags::empty()
    };
    // Instance::new 按值取 descriptor(wgpu 29/30 相同)。
    wgpu::Instance::new(descriptor)
}

/// 请求 HighPerformance adapter 并创建设备/队列(离屏渲染,无 surface)。
/// 启动日志记录 adapter 名/后端/驱动 —— 排查用户机器问题的第一现场。
pub async fn create_device(
    instance: &wgpu::Instance,
) -> PaintResult<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
        .map_err(|_| PaintError::NoAdapter)?;

    let info = adapter.get_info();
    tracing::info!(
        adapter = %info.name,
        backend = ?info.backend,
        driver = %info.driver,
        "选中 GPU adapter"
    );

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .map_err(|e| PaintError::DeviceRequest(e.to_string()))?;
    install_error_handler(&device);
    Ok((adapter, device, queue))
}

/// 设备丢失恢复守卫(分册六 §1.1):uncaptured error 只记录不 panic,
/// device 按代数(generation)失效一切 GPU 缓存 —— 缓存侧存 `(generation, resource)`,
/// 代数对不上即重建,这就是"device lost 后用户无感"的全部秘密。
pub struct GpuGuard {
    device: wgpu::Device,
    queue: wgpu::Queue,
    generation: Arc<AtomicU64>,
}

/// 单帧渲染的可重试分类:设备丢失类失败可重建后重试,其余一律致命。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// 设备丢失/过时(`CurrentSurfaceTexture::Lost/Outdated` 等),可重试。
    Retryable(String),
    /// 其他失败(校验错误、超时、OOM 等),不重试。
    Fatal(String),
}

/// 把 `CurrentSurfaceTexture` 分类成 [`FrameError`]。
/// `None` 表示本帧可用(`Success`/`Suboptimal` 携带纹理)或无需恢复的瞬态(遮挡)。
pub fn classify_surface_result(status: &wgpu::CurrentSurfaceTexture) -> Option<FrameError> {
    use wgpu::CurrentSurfaceTexture as C;
    match status {
        C::Success(_) | C::Suboptimal(_) | C::Occluded => None,
        C::Lost | C::Outdated => Some(FrameError::Retryable(String::from(
            "surface texture lost/outdated",
        ))),
        C::Timeout => Some(FrameError::Fatal(String::from("surface timeout"))),
        C::Validation => Some(FrameError::Fatal(String::from("surface validation error"))),
        // wgpu 29.0.4 的 CurrentSurfaceTexture 恰为以上 7 变体,无其他;
        // 升级窗口若上游新增变体,此处会编译报错提醒补分类(有意不留 catch-all)
    }
}

impl GpuGuard {
    /// 包装现有 device/queue 并挂接 uncaptured error 处理(只记录,不 panic)。
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        install_error_handler(&device);
        Self {
            device,
            queue,
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 当前设备代数;每次重建自增 1。
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// 代数句柄:交给一切 GPU 资源缓存做 `(generation, resource)` 失效配对。
    pub fn generation_handle(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.generation)
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// 每帧渲染入口:失败时若属可重试(设备丢失),重建设备、代数 +1、重试一次;
    /// 二次失败才算致命。`f` 会被调用至多两次,须为幂等的帧构建闭包(`Fn`)。
    ///
    /// 与分册六 §1.1 原稿的差异:wgpu 29/30 的取帧结果都是 `CurrentSurfaceTexture`
    /// 而非 `Result<_, SurfaceError>`,可重试性由 [`FrameError`] 表达(可经
    /// [`classify_surface_result`] 得到);恢复异步化以复用 `request_device`。
    pub async fn render_with_recovery<T>(
        &mut self,
        adapter: &wgpu::Adapter,
        f: impl Fn(&wgpu::Device, &wgpu::Queue) -> Result<T, FrameError>,
    ) -> PaintResult<T> {
        match f(&self.device, &self.queue) {
            Ok(value) => Ok(value),
            Err(FrameError::Fatal(msg)) => Err(PaintError::Render(msg)),
            Err(FrameError::Retryable(reason)) => {
                tracing::warn!(
                    reason = %reason,
                    generation = self.generation(),
                    "GPU 帧失败,重建设备后重试一次"
                );
                self.rebuild(adapter).await?;
                self.generation.fetch_add(1, Ordering::SeqCst);
                f(&self.device, &self.queue)
                    .map_err(|e| PaintError::Render(format!("设备重建后重试仍失败: {e:?}")))
            }
        }
    }

    async fn rebuild(&mut self, adapter: &wgpu::Adapter) -> PaintResult<()> {
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| PaintError::DeviceRequest(e.to_string()))?;
        install_error_handler(&device);
        self.device = device;
        self.queue = queue;
        Ok(())
    }
}

fn install_error_handler(device: &wgpu::Device) {
    // handler 参数为 Arc<dyn UncapturedErrorHandler>(wgpu 29/30 相同;Error 只记录不 panic)。
    device.on_uncaptured_error(Arc::new(|error: wgpu::Error| {
        tracing::error!(?error, "GPU uncaptured error(仅记录,不 panic)");
    }));
}

/// 创建 vello 渲染器(vello 0.10 的 `RendererOptions` 默认即支持全部 AA 模式)。
pub fn create_renderer(device: &wgpu::Device) -> PaintResult<vello::Renderer> {
    vello::Renderer::new(device, vello::RendererOptions::default())
        .map_err(|e| PaintError::Render(e.to_string()))
}

/// `PaintSink` → `vello::Scene` 的实现(零拷贝路径第 1 级)。
///
/// fill rule 统一 NonZero,与 CPU 路径的默认 fill rule 一致;
/// brush_transform 传 `None` = 笔刷随路径的用户空间(transform 已含视口变换)。
pub struct VelloSink {
    scene: vello::Scene,
}

impl Default for VelloSink {
    fn default() -> Self {
        Self::new()
    }
}

impl VelloSink {
    pub fn new() -> Self {
        Self {
            scene: vello::Scene::new(),
        }
    }

    pub fn scene(&self) -> &vello::Scene {
        &self.scene
    }

    pub fn scene_mut(&mut self) -> &mut vello::Scene {
        &mut self.scene
    }

    /// 取出场景交给 `Renderer::render_to_texture`。
    pub fn into_inner(self) -> vello::Scene {
        self.scene
    }
}

impl PaintSink for VelloSink {
    fn fill(&mut self, paint: &Paint, transform: Affine, path: &BezPath) {
        let brush = style::to_brush(paint);
        self.scene
            .fill(vello::peniko::Fill::NonZero, transform, &brush, None, path);
    }

    fn stroke(&mut self, style: &StrokeStyle, transform: Affine, path: &BezPath) {
        let brush = style::to_brush(&style.paint);
        let stroke = style::to_stroke(style);
        self.scene.stroke(&stroke, transform, &brush, None, path);
    }
}
