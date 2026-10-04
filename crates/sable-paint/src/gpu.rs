//! GPU 渲染后端(vello 0.10 + 其 re-export 的 wgpu 29.x),`gpu` feature 门控,默认关闭。
//!
//! v0.1 为**骨架**:后端选择(分册六 §6.3)、设备创建、[`GpuGuard`] 设备丢失恢复
//! (分册六 §1.1)、[`VelloSink`](`PaintSink` → `vello::Scene`)。生产帧入口见
//! [`crate::gpu_frame::GpuFrameRenderer`](RBT-01 接线)。CI 默认不开 gpu,
//! 本模块不携带需要 GPU 才能跑的测试。
//!
//! # 宿主接入契约(RBT-01,迭代审查报告 §4.6 / §6 RB-06)
//!
//! 1. **初始化**:[`crate::gpu_frame::GpuFrameRenderer::create`] → `Err`
//!    ([`PaintError::NoAdapter`]/[`PaintError::DeviceRequest`])→ 宿主显式提示 +
//!    渲染路径切 cpu-render(降级链第 4 级),**不许静默**;
//! 2. **每帧**:`GpuFrameRenderer::render_frame`(内部经 [`render_with_recovery`]
//!    包装幂等帧闭包)→ `Ok` 正常上屏;
//!    `Err(PaintError::GpuDeviceLostFatal { .. })` → 宿主按序执行三步:
//!    ① 落自动保存 → ② 渲染路径切 cpu-render → ③ 提示用户;
//! 3. **后端覆盖**:环境变量 [`BACKEND_ENV`](`vulkan`/`dx12`/`gl`/`auto`,
//!    大小写不敏感)运行期覆盖后端选择;无法识别的值回落编译期默认并**显式告警**;
//!    降级状态经 [`BackendSelection`]/[`crate::gpu_frame::GpuDegradation`]
//!    供宿主查询(设置界面/日志展示)。
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

use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use kurbo::{Affine, BezPath, Rect};
use sable_foundation::scene::{BlendMode, Paint, StrokeStyle};

use crate::error::{PaintError, PaintResult};
use crate::sink::PaintSink;
use crate::style;

/// 运行期后端覆盖的环境变量(分册六 §6.3:设置界面写入后映射为此变量,重启生效)。
pub const BACKEND_ENV: &str = "SABLE_GPU_BACKEND";

/// wgpu 单一真相源 re-export(经 `vello::wgpu`):本 crate 的调用方与宿主一律
/// 用此路径引用 wgpu 类型,workspace 禁止出现第二份 wgpu 依赖(版本错配即编译错)。
pub use vello::wgpu;

/// 后端选择的来源(宿主可查询:设置界面/日志展示"当前后端从哪来",分册六 §6.3)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendSource {
    /// `SABLE_GPU_BACKEND` 显式覆盖(值为环境变量原文,trim 后)。
    EnvOverride(String),
    /// 编译期 feature(`backend-vulkan` / `backend-dx12`)。
    CompileFeature,
    /// 平台默认(`PRIMARY`)。
    PlatformDefault,
    /// `SABLE_GPU_BACKEND` 值无法识别,已回落编译期默认(调用方必须显式告警,RB-06)。
    EnvUnrecognized(String),
}

/// 后端选择结果(宿主启动时可查询)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendSelection {
    /// 实际生效的 wgpu 后端集合。
    pub backends: wgpu::Backends,
    /// 生效来源。
    pub source: BackendSource,
}

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

/// 编译期默认的来源(与 [`compile_time_backends`] 配套)。
fn compile_time_source() -> BackendSource {
    #[cfg(any(feature = "backend-vulkan", feature = "backend-dx12"))]
    let source = BackendSource::CompileFeature;
    #[cfg(not(any(feature = "backend-vulkan", feature = "backend-dx12")))]
    let source = BackendSource::PlatformDefault;
    source
}

fn compile_time_selection() -> BackendSelection {
    BackendSelection {
        backends: compile_time_backends(),
        source: compile_time_source(),
    }
}

/// 后端选择纯函数核心(单一真相源):`env_value` 注入 `SABLE_GPU_BACKEND` 的值
/// (`None` = 未设置)。
///
/// 为什么公开且参数化注入:edition 2024 下 `std::env::set_var` 是 `unsafe`,
/// 本 crate `forbid(unsafe_code)`,测试无法安全地改全局 env——覆盖逻辑全部经
/// 本函数的参数注入测试(见 `tests/gpu_recovery.rs`),真实 env 读取只经
/// [`select_backends_detailed`] 薄封装。
///
/// 语义(值大小写不敏感,首尾空白忽略):
/// - `vulkan` / `dx12` / `gl` → 对应后端(`BackendSource::EnvOverride`);
/// - `auto` → `PRIMARY`(wgpu 平台自动挑;**显式覆盖**编译期偏好);
/// - 其他非空值 → 回落编译期默认(`BackendSource::EnvUnrecognized`,调用方显式告警);
/// - `None`/空白 → 编译期默认([`BackendSource::CompileFeature`] 或
///   [`BackendSource::PlatformDefault`])。
pub fn resolve_backends(env_value: Option<&str>) -> BackendSelection {
    let Some(raw) = env_value else {
        return compile_time_selection();
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return compile_time_selection();
    }
    let backends = match trimmed.to_ascii_lowercase().as_str() {
        "vulkan" => Some(wgpu::Backends::VULKAN),
        "dx12" => Some(wgpu::Backends::DX12),
        "gl" => Some(wgpu::Backends::GL),
        "auto" => Some(wgpu::Backends::PRIMARY),
        _ => None,
    };
    match backends {
        Some(backends) => BackendSelection {
            backends,
            source: BackendSource::EnvOverride(String::from(trimmed)),
        },
        None => BackendSelection {
            backends: compile_time_backends(),
            source: BackendSource::EnvUnrecognized(String::from(trimmed)),
        },
    }
}

/// 后端选择(读取真实环境变量 + 显式告警,不许静默,RB-06)。
pub fn select_backends_detailed() -> BackendSelection {
    let selection = match std::env::var(BACKEND_ENV) {
        Ok(value) => resolve_backends(Some(&value)),
        Err(_) => resolve_backends(None),
    };
    if let BackendSource::EnvUnrecognized(value) = &selection.source {
        tracing::warn!(
            env_value = %value,
            supported = "vulkan/dx12/gl/auto",
            backends = ?selection.backends,
            "SABLE_GPU_BACKEND 值无法识别,已回落编译期默认(显式告警,不许静默)"
        );
    }
    selection
}

/// 后端选择:环境变量 `SABLE_GPU_BACKEND` > 编译期 feature > 平台默认。
/// 需要来源/降级信息时改用 [`select_backends_detailed`]。
pub fn select_backends() -> wgpu::Backends {
    select_backends_detailed().backends
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
/// 失败一律显式告警(RB-06 不许静默),宿主据此走降级链(切 cpu-render + 提示)。
pub async fn create_device(
    instance: &wgpu::Instance,
) -> PaintResult<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let selection = select_backends_detailed();
    let adapter = match instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
    {
        Ok(adapter) => adapter,
        Err(err) => {
            tracing::warn!(
                backends = ?selection.backends,
                source = ?selection.source,
                adapter_err = %err,
                "GPU adapter 请求失败(显式告警,RB-06):宿主应按降级链切 cpu-render 并提示用户"
            );
            return Err(PaintError::NoAdapter);
        }
    };

    let info = adapter.get_info();
    tracing::info!(
        adapter = %info.name,
        backend = ?info.backend,
        driver = %info.driver,
        "选中 GPU adapter"
    );

    let (device, queue) = match adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
    {
        Ok(pair) => pair,
        Err(err) => {
            tracing::warn!(
                adapter = %info.name,
                device_err = %err,
                "GPU 设备请求失败(显式告警,RB-06):宿主应按降级链切 cpu-render 并提示用户"
            );
            return Err(PaintError::DeviceRequest(err.to_string()));
        }
    };
    // 设备丢失回调由 GpuGuard::new 挂接(信号归 guard 持有,供帧闭包探测)。
    Ok((adapter, device, queue))
}

/// 设备丢失信号(RBT-01):wgpu 29 **没有** `Device::is_lost`;离屏路径的设备丢失
/// 经 `Device::set_device_lost_callback` 捕获到此信号,帧闭包在每帧提交前探测
/// (见 [`crate::gpu_frame::GpuFrameRenderer::render_frame`])。
///
/// 探测语义是**一次消费**([`DeviceLossSignal::take`]):取走原因并复位,
/// 避免同一事件跨帧反复触发重建。
#[derive(Debug, Default)]
pub struct DeviceLossSignal {
    lost: AtomicBool,
    reason: Mutex<Option<String>>,
}

impl DeviceLossSignal {
    fn new_handle() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn fire(&self, reason: String) {
        self.lost.store(true, Ordering::SeqCst);
        if let Ok(mut slot) = self.reason.lock() {
            *slot = Some(reason);
        }
    }

    /// 取走丢失原因并复位;`None` = 当前无未消费的丢失信号。
    pub fn take(&self) -> Option<String> {
        if !self.lost.swap(false, Ordering::SeqCst) {
            return None;
        }
        // 极小竞态窗口:swap 与取 reason 之间 fire 再次发生 → 原因被并发覆盖,
        // 此时按"已丢失但无细节"兜底,不丢事件本体。
        Some(
            self.reason
                .lock()
                .ok()
                .and_then(|mut slot| slot.take())
                .unwrap_or_else(|| String::from("device lost(未携带原因)")),
        )
    }

    /// 复位(重建设备成功后调用,丢弃旧设备迟到的信号)。
    pub fn reset(&self) {
        self.lost.store(false, Ordering::SeqCst);
        if let Ok(mut slot) = self.reason.lock() {
            *slot = None;
        }
    }
}

fn install_callbacks(device: &wgpu::Device, loss: &Arc<DeviceLossSignal>) {
    // handler 参数为 Arc<dyn UncapturedErrorHandler>(wgpu 29/30 相同;Error 只记录不 panic)。
    device.on_uncaptured_error(Arc::new(|error: wgpu::Error| {
        tracing::error!(?error, "GPU uncaptured error(仅记录,不 panic)");
    }));
    let sink = Arc::clone(loss);
    device.set_device_lost_callback(move |reason: wgpu::DeviceLostReason, message: String| {
        tracing::error!(
            ?reason,
            message = %message,
            "GPU 设备丢失回调触发(恢复状态机将在下一次帧探测中重建设备)"
        );
        sink.fire(message);
    });
}

/// 帧恢复端口(RBT-01):设备丢失恢复状态机的**可注入执行面**。
///
/// 生产实现:`GpuGuard` 内部的 wgpu 端口(adapter 重建设备);
/// 测试实现:`tests/gpu_recovery.rs` 的故障注入源(无需真实 GPU 即可驱动
/// 恢复语义——首败重建、二连败致命、代数单调)。
pub trait FrameRecovery {
    /// 一帧的产物。
    type Frame;
    /// 执行一帧。**必须幂等可重入**:被状态机再次调用时从零构建帧状态,
    /// 不依赖上次调用的残迹(生产侧即"每次全新 `Scene`")。
    /// 至多被连续调用两次(`FnMut`;不并发、不交错)。
    fn run_frame(&mut self) -> Result<Self::Frame, FrameError>;
    /// 重建设备。`Ok` 后 [`Self::run_frame`] 将在新设备上执行;
    /// `Err(reason)` = 重建失败,状态机转致命变体(宿主契约触发)。
    fn rebuild_device(&mut self) -> impl Future<Output = Result<(), String>>;
}

/// 设备丢失恢复状态机(RBT-01,单一真相源):[`GpuGuard::render_with_recovery`]
/// 与测试注入端口共用同一路径,无第二套实现。
///
/// 语义(与分册六 §1.1 一致):
/// 1. 首帧 `Ok` → 直接返回;
/// 2. 首帧 [`FrameError::Fatal`](与设备丢失无关)→ [`PaintError::Render`],不重试;
/// 3. 首帧 [`FrameError::Retryable`](设备丢失类)→ [`FrameRecovery::rebuild_device`]:
///    - 重建失败 → [`PaintError::GpuDeviceLostFatal`](宿主契约三步触发);
///    - 重建成功 → `generation` 自增 1 → 重试一次;
/// 4. 重试再败(无论分类,此时设备上下文已不可信)→ [`PaintError::GpuDeviceLostFatal`]。
pub async fn render_with_recovery<F: FrameRecovery>(
    port: &mut F,
    generation: &AtomicU64,
) -> PaintResult<F::Frame> {
    match port.run_frame() {
        Ok(frame) => Ok(frame),
        Err(FrameError::Fatal(reason)) => Err(PaintError::Render(reason)),
        Err(FrameError::Retryable(reason)) => {
            tracing::warn!(
                reason = %reason,
                generation = generation.load(Ordering::SeqCst),
                "GPU 帧失败(设备丢失类),重建设备后重试一次"
            );
            if let Err(rebuild_reason) = port.rebuild_device().await {
                tracing::error!(
                    reason = %rebuild_reason,
                    generation = generation.load(Ordering::SeqCst),
                    "GPU 设备重建失败(显式告警,RB-06):宿主必须自动保存 + 切 cpu-render + 提示用户"
                );
                return Err(PaintError::GpuDeviceLostFatal {
                    reason: format!("设备重建失败: {rebuild_reason}"),
                });
            }
            generation.fetch_add(1, Ordering::SeqCst);
            match port.run_frame() {
                Ok(frame) => Ok(frame),
                Err(again) => {
                    tracing::error!(
                        error = ?again,
                        generation = generation.load(Ordering::SeqCst),
                        "GPU 设备重建后重试仍失败(显式告警,RB-06):宿主必须自动保存 + 切 cpu-render + 提示用户"
                    );
                    Err(PaintError::GpuDeviceLostFatal {
                        reason: format!("设备重建后重试仍失败: {again:?}"),
                    })
                }
            }
        }
    }
}

/// [`FrameRecovery`] 的生产端口:把 `GpuGuard` 的字段拆借给状态机
/// (设备/队列可变、adapter 只读、丢失信号共享),帧闭包原样透传。
struct GuardFramePort<'a, T, F>
where
    F: FnMut(&wgpu::Device, &wgpu::Queue) -> Result<T, FrameError>,
{
    device: &'a mut wgpu::Device,
    queue: &'a mut wgpu::Queue,
    adapter: &'a wgpu::Adapter,
    loss: &'a Arc<DeviceLossSignal>,
    f: F,
}

impl<T, F> FrameRecovery for GuardFramePort<'_, T, F>
where
    F: FnMut(&wgpu::Device, &wgpu::Queue) -> Result<T, FrameError>,
{
    type Frame = T;

    fn run_frame(&mut self) -> Result<T, FrameError> {
        (self.f)(self.device, self.queue)
    }

    async fn rebuild_device(&mut self) -> Result<(), String> {
        let (device, queue) = self
            .adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| e.to_string())?;
        install_callbacks(&device, self.loss);
        *self.device = device;
        *self.queue = queue;
        // 旧设备迟到的丢失信号不得污染新设备的第一帧。
        self.loss.reset();
        Ok(())
    }
}

/// 设备丢失恢复守卫(分册六 §1.1):uncaptured error 只记录不 panic,
/// device 按代数(generation)失效一切 GPU 缓存 —— 缓存侧存 `(generation, resource)`,
/// 代数对不上即重建,这就是"device lost 后用户无感"的全部秘密。
pub struct GpuGuard {
    device: wgpu::Device,
    queue: wgpu::Queue,
    generation: Arc<AtomicU64>,
    loss: Arc<DeviceLossSignal>,
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
    /// 包装现有 device/queue 并挂接 uncaptured error 处理与设备丢失回调(只记录,不 panic)。
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let loss = DeviceLossSignal::new_handle();
        install_callbacks(&device, &loss);
        Self {
            device,
            queue,
            generation: Arc::new(AtomicU64::new(0)),
            loss,
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

    /// 设备丢失信号句柄(帧闭包在提交前探测;见 [`DeviceLossSignal::take`])。
    pub fn loss_handle(&self) -> Arc<DeviceLossSignal> {
        Arc::clone(&self.loss)
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// 每帧渲染入口:失败时若属可重试(设备丢失),重建设备、代数 +1、重试一次;
    /// 二次失败返回致命变体 [`PaintError::GpuDeviceLostFatal`]。
    /// `f` 会被**连续**调用至多两次(`FnMut`,不并发不交错),须为幂等可重入的
    /// 帧构建闭包:第二次调用不得依赖第一次的残迹。
    ///
    /// 状态机本体在自由函数 [`render_with_recovery`](可注入端口版,测试与生产
    /// 共用同一路径);本方法只是 wgpu 类型的薄适配(拆字段借给 [`GuardFramePort`])。
    ///
    /// 与分册六 §1.1 原稿的差异:wgpu 29/30 的取帧结果都是 `CurrentSurfaceTexture`
    /// 而非 `Result<_, SurfaceError>`,可重试性由 [`FrameError`] 表达(可经
    /// [`classify_surface_result`] 得到);恢复异步化以复用 `request_device`。
    pub async fn render_with_recovery<T>(
        &mut self,
        adapter: &wgpu::Adapter,
        f: impl FnMut(&wgpu::Device, &wgpu::Queue) -> Result<T, FrameError>,
    ) -> PaintResult<T> {
        let GpuGuard {
            device,
            queue,
            generation,
            loss,
        } = self;
        let mut port = GuardFramePort {
            device,
            queue,
            adapter,
            loss,
            f,
        };
        render_with_recovery(&mut port, generation).await
    }
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

    /// 开混合层(迭代计划 08 E5):vello 0.10 的
    /// `Scene::push_layer(clip_style, blend, alpha, transform, clip)` 实测签名
    /// (vello 0.10 src/scene.rs,`impl Into<StyleRef>` 接受 `Fill`,`impl
    /// Into<BlendMode>` 接受 `BlendMode` 本体)。
    ///
    /// 覆盖形状用**巨大矩形**(±1e6):混合层必须罩住节点可能出现的全部
    /// 世界坐标(视口变换后仍远超任何屏幕),否则层会被裁出可见区;
    /// alpha 传 1.0(不透明度由节点自身 opacity 在绘制侧另行处理)。
    fn push_blend(&mut self, mode: BlendMode) {
        const COVER_HALF: f64 = 1.0e6;
        let cover = Rect::new(-COVER_HALF, -COVER_HALF, COVER_HALF, COVER_HALF);
        self.scene.push_layer(
            vello::peniko::Fill::NonZero,
            style::to_vello_blend(mode),
            1.0,
            Affine::IDENTITY,
            &cover,
        );
    }

    /// 关闭最近一次开启的混合层。
    fn pop_blend(&mut self) {
        self.scene.pop_layer();
    }
}
