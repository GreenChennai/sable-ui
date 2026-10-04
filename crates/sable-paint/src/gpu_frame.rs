//! 生产 GPU 帧渲染入口(RBT-01 接线,迭代审查报告 §4.6 / §6 RB-06)。
//!
//! `GpuGuard` 的恢复抽象此前全仓零调用方(驱动更新/显卡休眠/显存耗尽时无任何
//! 恢复路径)。本模块把它接成真实帧循环:
//!
//! - **持有**设备会话([`GpuGuard`],含 device/queue/代数/丢失信号)与
//!   **代数配对**的渲染资源缓存([`GenerationCache`]:vello `Renderer` + 目标纹理);
//! - **每帧入口** [`GpuFrameRenderer::render_frame`] 经
//!   [`crate::gpu::render_with_recovery`] 包装幂等帧闭包(每次尝试全新 `Scene`,
//!   可安全重入):首败(设备丢失)→ 重建设备、代数自增、按新代数重建全部资源
//!   后重试;**连续二次失败 → [`PaintError::GpuDeviceLostFatal`]**;
//! - **降级状态**([`GpuDegradation`])供宿主查询;设备创建失败显式告警,不许静默。
//!
//! # 宿主接入契约(RBT-01;自动保存本体属宿主职责,库只给契约与信号)
//!
//! ```text
//! 初始化:GpuFrameRenderer::create()
//!   ├─ Ok                → 进入 GPU 帧循环
//!   └─ Err(NoAdapter/…)  → 显式提示 + 渲染路径切 cpu-render(不许静默)
//!
//! 每帧:render_frame(w, h, base_color, |sink| { … })
//!   ├─ Ok(FrameOutput)   → 宿主把 output.texture 上屏(blit)或读回
//!   └─ Err(GpuDeviceLostFatal { reason, .. })
//!       → 宿主按序执行三步(缺一不可):
//!         ① 落自动保存(用户工作不因设备丢失而丢)
//!         ② 渲染路径切 cpu-render(crate::cpu::CpuRenderer,降级链第 4 级)
//!         ③ 提示用户(设备已重置,工作已保存)
//! ```
//!
//! # API 依据(本地 registry 源码实测)
//!
//! - vello 0.10 `Renderer::render_to_texture` 是**同步** fn(非 async;async 版
//!   `render_to_texture_async` 已废弃),签名
//!   `(&mut self, device, queue, scene, texture: &TextureView, params) -> Result<(), vello::Error>`;
//!   目标纹理契约:`Rgba8Unorm` + `STORAGE_BINDING`(lib.rs 渲染入口 doc);
//! - wgpu 29 无 `Device::is_lost`,离屏设备丢失经
//!   `Device::set_device_lost_callback(Fn(DeviceLostReason, String))` 捕获;
//! - `Backends::NOOP` 需 wgpu `noop` feature(vello 未启用),无头设备丢失注入
//!   不可行——恢复语义由注入端口的恢复状态机测试覆盖,真机走查见
//!   `tests/gpu_recovery.rs` 的 `#[ignore]` 冒烟测试。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use peniko::Color;
use vello::wgpu;

use crate::error::{PaintError, PaintResult};
use crate::gpu::{
    BackendSource, DeviceLossSignal, FrameError, GpuGuard, VelloSink, create_device,
    create_instance, select_backends_detailed,
};

/// GPU 降级状态(宿主可查询;分册六 §6.4 / RB-06:降级必须显式,不许静默)。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum GpuDegradation {
    /// 无降级:GPU 渲染按预期工作。
    #[default]
    None,
    /// `SABLE_GPU_BACKEND` 覆盖了编译期后端偏好(启动信息,设置界面可展示)。
    BackendOverride { requested: String },
    /// `SABLE_GPU_BACKEND` 值无法识别,已回落编译期默认(启动时已显式告警)。
    BackendUnrecognized { requested: String },
    /// 设备丢失且自动恢复耗尽:GPU 渲染不可用。
    /// 宿主契约三步:① 落自动保存 → ② 渲染路径切 cpu-render → ③ 提示用户。
    DeviceLostFatal { reason: String, generation: u64 },
}

/// 一帧 GPU 渲染的产物:vello 渲染到的目标纹理。
///
/// 纹理为 `Rgba8Unorm`、`STORAGE_BINDING | TEXTURE_BINDING | COPY_SRC`:
/// `STORAGE_BINDING` 是 vello 计算管线的写入要求,`TEXTURE_BINDING` 供宿主
/// blit 上屏,`COPY_SRC` 供读回兜底(降级链第 2 级)。
#[derive(Debug, Clone)]
pub struct FrameOutput {
    /// 本帧渲染结果(宿主上屏/读回用;纹理归 [`GenerationCache`] 持有,跨帧复用)。
    pub texture: wgpu::Texture,
    /// 帧宽(像素)。
    pub width: u32,
    /// 帧高(像素)。
    pub height: u32,
}

/// 代数配对的 GPU 资源缓存(RBT-01):资源只在 `(generation, resource)` 对上可见。
///
/// 设备重建后代数自增,旧代数条目即刻不可见([`Self::get_if`] 永不命中),
/// 并在下次写入时被释放——这就是"GpuGuard 代数失效一切 GPU 缓存"的缓存侧实现。
pub struct GenerationCache<V> {
    generation: Arc<AtomicU64>,
    entry: Option<(u64, V)>,
}

impl<V> GenerationCache<V> {
    /// 以代数句柄建缓存(句柄通常来自 [`GpuGuard::generation_handle`])。
    pub fn new(generation: Arc<AtomicU64>) -> Self {
        Self {
            generation,
            entry: None,
        }
    }

    /// 当前设备代数(重建一次自增 1)。
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// 代数句柄(交给上游失效联动)。
    pub fn generation_handle(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.generation)
    }

    /// 命中条件:条目存在、**代数与当前一致**、`valid` 通过;任一不满足 → `None`
    /// (调用方按当前代数重建后 [`Self::insert`])。
    /// 返回**独占引用**:vello `Renderer::render_to_texture` 需要 `&mut self`。
    pub fn get_if(&mut self, valid: impl FnOnce(&V) -> bool) -> Option<&mut V> {
        let current = self.generation();
        let (generation, value) = self.entry.as_mut()?;
        if *generation == current && valid(value) {
            Some(value)
        } else {
            None
        }
    }

    /// 写入:打上当前代数戳,并丢弃一切异代条目(代数配对失效纪律)。
    pub fn insert(&mut self, value: V) {
        self.entry = Some((self.generation(), value));
    }

    /// 显式丢弃条目(诊断/测试用),返回 `(条目所属代数, 资源)`。
    pub fn take(&mut self) -> Option<(u64, V)> {
        self.entry.take()
    }
}

/// 一次帧尝试持有的 GPU 资源(vello 渲染器 + 目标纹理),整体按代数配对缓存。
struct FrameResources {
    renderer: vello::Renderer,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
}

impl FrameResources {
    fn matches(&self, width: u32, height: u32) -> bool {
        self.width == width && self.height == height
    }

    /// 按当前设备与帧尺寸新建(设备丢失恢复后由新代数触发)。
    fn create(device: &wgpu::Device, width: u32, height: u32) -> PaintResult<Self> {
        let renderer = vello::Renderer::new(device, vello::RendererOptions::default())
            .map_err(|e| PaintError::Render(format!("vello Renderer 创建失败: {e}")))?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sable-paint-gpu-frame-target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // vello 0.10 render_to_texture 契约:Rgba8Unorm + STORAGE_BINDING;
            // TEXTURE_BINDING 供宿主上屏 blit,COPY_SRC 供读回兜底(降级链第 2 级)。
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Ok(Self {
            renderer,
            texture,
            view,
            width,
            height,
        })
    }
}

/// 生产 GPU 帧渲染入口(RBT-01):持有设备会话([`GpuGuard`])与代数配对的
/// 渲染资源缓存;每帧入口经 [`gpu::render_with_recovery`] 包装幂等帧闭包。
///
/// 宿主契约见模块文档(自动保存 → 切 cpu-render → 提示用户三步)。
pub struct GpuFrameRenderer {
    adapter: wgpu::Adapter,
    guard: GpuGuard,
    cache: GenerationCache<FrameResources>,
    degradation: GpuDegradation,
}

impl GpuFrameRenderer {
    /// 生产初始化入口(分册六 §6.4 降级链第 1 级):
    /// 选后端(读 [`crate::gpu::BACKEND_ENV`])→ 建 `Instance` → 建设备。
    /// 失败一律显式告警(`create_device` 内,不许静默)并返回 `Err`;
    /// 宿主应据此切 cpu-render 并提示用户。
    pub async fn create() -> PaintResult<Self> {
        let selection = select_backends_detailed();
        tracing::info!(
            backends = ?selection.backends,
            source = ?selection.source,
            "GPU 后端选择"
        );
        let instance = create_instance();
        Self::create_with_instance(&instance).await
    }

    /// 以既有 `Instance` 初始化(宿主自管 Instance 时用;告警纪律同 [`Self::create`])。
    pub async fn create_with_instance(instance: &wgpu::Instance) -> PaintResult<Self> {
        let (adapter, device, queue) = create_device(instance).await?;
        let guard = GpuGuard::new(device, queue);
        // 降级状态记录(宿主可查询; unrecognized 属降级,须显式可见,不许静默)。
        let degradation = match select_backends_detailed().source {
            BackendSource::EnvOverride(requested) => GpuDegradation::BackendOverride { requested },
            BackendSource::EnvUnrecognized(requested) => {
                GpuDegradation::BackendUnrecognized { requested }
            }
            BackendSource::CompileFeature | BackendSource::PlatformDefault => GpuDegradation::None,
        };
        Ok(Self {
            adapter,
            cache: GenerationCache::new(guard.generation_handle()),
            guard,
            degradation,
        })
    }

    /// 当前设备代数(重建一次自增 1;恢复后资源按新代数整体重建)。
    pub fn generation(&self) -> u64 {
        self.guard.generation()
    }

    /// 降级状态(宿主可查询;`DeviceLostFatal` 即宿主契约三步的触发信号)。
    pub fn degradation(&self) -> &GpuDegradation {
        &self.degradation
    }

    /// 设备丢失信号句柄(宿主如需在帧循环外提前感知设备丢失可探测;
    /// 帧闭包在每次尝试开始时也会探测)。
    pub fn loss_handle(&self) -> Arc<DeviceLossSignal> {
        self.guard.loss_handle()
    }

    /// 每帧入口(RBT-01):帧构建闭包经 [`gpu::render_with_recovery`] 包装,
    /// **幂等可重入**(每次尝试全新 `Scene`/`VelloSink`,不依赖上次残迹)。
    ///
    /// - 设备丢失(wgpu 29 无 `is_lost`,经 device-lost 回调信号在帧前探测)→
    ///   自动重建设备、代数自增、按新代数重建全部资源(vello `Renderer` + 纹理)后重试;
    /// - 连续二次失败 → [`PaintError::GpuDeviceLostFatal`],同时
    ///   [`Self::degradation`] 置为 [`GpuDegradation::DeviceLostFatal`]。
    ///
    /// `build_scene` 在一次恢复周期内至多被调用两次,必须只依赖入参 `VelloSink`
    /// 与外部不变状态(即"帧构建是纯函数")。
    pub async fn render_frame(
        &mut self,
        width: u32,
        height: u32,
        base_color: Color,
        build_scene: impl Fn(&mut VelloSink),
    ) -> PaintResult<FrameOutput> {
        // wgpu 对 0 尺寸纹理直接 panic;库层纪律是结构化错误,先拦下。
        if width == 0 || height == 0 {
            return Err(PaintError::Render(format!(
                "GPU 帧尺寸不得为 0:{width}x{height}"
            )));
        }
        let GpuFrameRenderer {
            adapter,
            guard,
            cache,
            degradation,
        } = self;
        let loss = guard.loss_handle();
        let result = guard
            .render_with_recovery(&*adapter, |device, queue| {
                frame_attempt(
                    cache,
                    &loss,
                    device,
                    queue,
                    width,
                    height,
                    base_color,
                    &build_scene,
                )
            })
            .await;
        if let Err(PaintError::GpuDeviceLostFatal { reason }) = &result {
            *degradation = GpuDegradation::DeviceLostFatal {
                reason: reason.clone(),
                generation: guard.generation(),
            };
        }
        result
    }
}

/// 单次帧尝试(RBT-01 的"幂等帧闭包"本体;状态机可在新设备上再次调用):
/// ① 丢失探测 → ② 代数配对资源取用/重建 → ③ 全新 Scene 构建 → ④ vello 提交。
#[allow(clippy::too_many_arguments)]
fn frame_attempt(
    cache: &mut GenerationCache<FrameResources>,
    loss: &DeviceLossSignal,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    base_color: Color,
    build_scene: &impl Fn(&mut VelloSink),
) -> Result<FrameOutput, FrameError> {
    // ① 设备丢失探测(wgpu 29 无 is_lost;device-lost 回调信号,一次消费)。
    if let Some(reason) = loss.take() {
        return Err(FrameError::Retryable(format!("设备丢失信号: {reason}")));
    }
    // ② 资源按 (generation, resource) 配对取用;未命中/尺寸不符 → 按当前代数重建。
    let resources = match cache.get_if(|r| r.matches(width, height)) {
        Some(resources) => resources,
        None => {
            tracing::debug!(
                generation = cache.generation(),
                "GPU 帧资源未命中(代数翻转或尺寸变更),按当前代数重建"
            );
            let fresh = FrameResources::create(device, width, height)
                .map_err(|e| FrameError::Fatal(format!("GPU 帧资源重建失败: {e}")))?;
            cache.insert(fresh);
            match cache.get_if(|r| r.matches(width, height)) {
                Some(resources) => resources,
                // 刚写入即未命中只可能因代数被并发翻转;按帧失败交状态机裁决。
                None => {
                    return Err(FrameError::Fatal(String::from(
                        "GPU 帧资源写入后未命中(代数竞态)",
                    )));
                }
            }
        }
    };
    // ③ 场景构建:全新 Scene(经 VelloSink 指令流),天然幂等可重入。
    let mut sink = VelloSink::new();
    build_scene(&mut sink);
    let scene = sink.into_inner();
    // ④ vello 编码 + GPU 提交(vello 0.10 render_to_texture 为同步 API)。
    let params = vello::RenderParams {
        base_color,
        width,
        height,
        antialiasing_method: vello::AaConfig::Area,
    };
    resources
        .renderer
        .render_to_texture(device, queue, &scene, &resources.view, &params)
        .map_err(|e| FrameError::Fatal(format!("vello render_to_texture 失败: {e}")))?;
    Ok(FrameOutput {
        texture: resources.texture.clone(),
        width,
        height,
    })
}
