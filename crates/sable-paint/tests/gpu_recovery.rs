#![cfg(feature = "gpu")]
//! RBT-01 设备丢失恢复测试(可注入设备丢失源,无需真实 GPU)。
//!
//! - **恢复状态机**(TC-RBT-GPU-01/02):经 [`FrameRecovery`] 端口注入故障脚本,
//!   驱动与生产([`GpuGuard::render_with_recovery`])**同一条**状态机路径
//!   ([`render_with_recovery`] 自由函数是单一真相源);
//! - **代数配对资源缓存**([`GenerationCache`]):纯数据结构,零 GPU 依赖;
//! - **后端选择**([`resolve_backends`]):edition 2024 下 `set_var` 为 `unsafe`
//!   且 crate `forbid(unsafe_code)`,因此覆盖逻辑经参数注入测试(零全局 env 污染),
//!   真实 env 只做只读一致性断言;
//! - **TC-RBT-GPU-01 真机冒烟**:`#[ignore]` 测试(vello 的 wgpu 未启用 `noop`
//!   feature,无头设备丢失注入不可行)——真机走查步骤见该测试的 ignore 理由。

use std::collections::VecDeque;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use sable_paint::error::PaintError;
use sable_paint::gpu::{
    self, BACKEND_ENV, BackendSelection, BackendSource, FrameError, FrameRecovery,
    render_with_recovery, resolve_backends, select_backends, select_backends_detailed,
};
use sable_paint::gpu_frame::{GenerationCache, GpuDegradation};
use sable_paint::prelude::{GpuFrameRenderer, to_color};

/// 极小同步执行器(测试专用):轮询到 ready 为止。
/// mock future 首轮即 ready;真机冒烟里 wgpu 的 future 由内部线程驱动,
/// 1ms 轮询间隔只影响空闲等待,不烧 CPU。
fn block_on<F: Future>(future: F) -> F::Output {
    let waker = std::task::Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

// ---------------------------------------------------------------------------
// 恢复状态机(可注入设备丢失源)
// ---------------------------------------------------------------------------

/// 帧脚本动作:一次 `run_frame` 的预编结果(设备丢失源注入)。
enum Step {
    /// 帧成功(返回当时的 device_token,用于断言"帧跑在新设备上")。
    Ok,
    /// 设备丢失类失败(可重试)。
    Retryable(&'static str),
    /// 与设备丢失无关的帧失败(不可重试)。
    Fatal(&'static str),
}

struct MockPort {
    script: VecDeque<Step>,
    /// 前 N 次 `rebuild_device` 失败(设备重建失败注入开关)。
    rebuild_failures: u32,
    frames_run: u32,
    rebuilds: u32,
    /// 设备令牌:重建成功 +1,模拟"新设备";帧成功时随帧返回。
    device_token: u64,
}

impl MockPort {
    fn new(script: Vec<Step>) -> Self {
        Self {
            script: VecDeque::from(script),
            rebuild_failures: 0,
            frames_run: 0,
            rebuilds: 0,
            device_token: 1,
        }
    }

    fn with_rebuild_failures(mut self, failures: u32) -> Self {
        self.rebuild_failures = failures;
        self
    }
}

impl FrameRecovery for MockPort {
    type Frame = u64;

    fn run_frame(&mut self) -> Result<u64, FrameError> {
        self.frames_run += 1;
        match self.script.pop_front() {
            Some(Step::Ok) => Ok(self.device_token),
            Some(Step::Retryable(reason)) => Err(FrameError::Retryable(String::from(reason))),
            Some(Step::Fatal(reason)) => Err(FrameError::Fatal(String::from(reason))),
            None => Err(FrameError::Fatal(String::from("脚本耗尽(测试配置错误)"))),
        }
    }

    async fn rebuild_device(&mut self) -> Result<(), String> {
        self.rebuilds += 1;
        if self.rebuild_failures > 0 {
            self.rebuild_failures -= 1;
            Err(String::from("注入的设备重建失败"))
        } else {
            self.device_token += 1;
            Ok(())
        }
    }
}

fn drive(port: &mut MockPort, generation: &AtomicU64) -> Result<u64, PaintError> {
    block_on(render_with_recovery(port, generation))
}

/// 首败(设备丢失)→ 重建 → 代数自增 → 重试成功;且重试帧确实跑在新设备上。
#[test]
fn tc_rbt_gpu_recovery_first_failure_rebuilds_then_succeeds() {
    let generation = AtomicU64::new(0);
    let mut port = MockPort::new(vec![Step::Retryable("注入:设备丢失"), Step::Ok]);
    let frame = drive(&mut port, &generation).expect("首败重建后必须恢复成功");
    assert_eq!(frame, 2, "重试帧必须跑在重建后的新设备上(token 1→2)");
    assert_eq!(generation.load(Ordering::SeqCst), 1, "恢复后代数必须自增 1");
    assert_eq!(port.frames_run, 2, "帧闭包至多调用两次");
    assert_eq!(port.rebuilds, 1, "设备重建恰好一次");
}

/// TC-RBT-GPU-02:连续二次帧失败 → 致命变体,载荷(reason)可提取,不再致命二次。
#[test]
fn tc_rbt_gpu_02_double_failure_returns_fatal_with_payload() {
    let generation = AtomicU64::new(0);
    let mut port = MockPort::new(vec![
        Step::Retryable("注入:首帧设备丢失"),
        Step::Retryable("注入:重试仍丢失"),
    ]);
    let err = drive(&mut port, &generation).expect_err("二连败必须致命");
    // 载荷可提取:reason 携带两次失败的上下文。
    let PaintError::GpuDeviceLostFatal { reason } = &err else {
        panic!("二连败必须是 GpuDeviceLostFatal,实际 {err:?}");
    };
    assert!(
        reason.contains("设备重建后重试仍失败") && reason.contains("Retryable"),
        "载荷必须携带重试仍失败的语义,实际 {reason:?}"
    );
    assert_eq!(
        generation.load(Ordering::SeqCst),
        1,
        "重建成功过一次,代数已自增"
    );
    assert_eq!(port.frames_run, 2);
}

/// TC-RBT-GPU-02:恢复路径上重试帧即使报 Fatal 类错误也同样致命(设备上下文已不可信)。
#[test]
fn tc_rbt_gpu_02_fatal_after_loss_in_recovery_is_also_fatal() {
    let generation = AtomicU64::new(0);
    let mut port = MockPort::new(vec![
        Step::Retryable("注入:设备丢失"),
        Step::Fatal("注入:超时"),
    ]);
    let err = drive(&mut port, &generation).expect_err("恢复路径上的再失败必须致命");
    assert!(
        matches!(err, PaintError::GpuDeviceLostFatal { .. }),
        "恢复路径任何再失败都走 GpuDeviceLostFatal,实际 {err:?}"
    );
    assert_eq!(generation.load(Ordering::SeqCst), 1);
}

/// TC-RBT-GPU-02:重建设备本身失败 → 致命变体;代数不得自增(没有新设备)。
#[test]
fn tc_rbt_gpu_02_rebuild_failure_is_fatal_without_generation_bump() {
    let generation = AtomicU64::new(0);
    let mut port = MockPort::new(vec![Step::Retryable("注入:设备丢失")]).with_rebuild_failures(1);
    let err = drive(&mut port, &generation).expect_err("重建失败必须致命");
    let PaintError::GpuDeviceLostFatal { reason } = &err else {
        panic!("重建失败必须是 GpuDeviceLostFatal,实际 {err:?}");
    };
    assert!(
        reason.contains("设备重建失败"),
        "载荷必须携带重建失败语义,实际 {reason:?}"
    );
    assert_eq!(
        generation.load(Ordering::SeqCst),
        0,
        "没有新设备就没有新代数"
    );
    assert_eq!(port.frames_run, 1, "重建失败不得重试帧");
}

/// 首帧即 Fatal(与设备丢失无关)→ 普通 Render 错误,不触发恢复、代数不动。
#[test]
fn tc_rbt_gpu_fatal_first_attempt_skips_recovery() {
    let generation = AtomicU64::new(0);
    let mut port = MockPort::new(vec![Step::Fatal("注入:校验错误")]);
    let err = drive(&mut port, &generation).expect_err("帧失败必须报错");
    assert_eq!(
        err,
        PaintError::Render(String::from("注入:校验错误")),
        "Fatal 类必须是普通 Render 错误(非设备丢失语义)"
    );
    assert_eq!(generation.load(Ordering::SeqCst), 0, "未走恢复,代数不动");
    assert_eq!(port.frames_run, 1, "Fatal 不重试");
    assert_eq!(port.rebuilds, 0, "Fatal 不重建设备");
}

/// 多轮恢复:代数严格单调递增,每轮都拿到新设备令牌。
#[test]
fn tc_rbt_gpu_generation_monotonic_across_repeated_recoveries() {
    let generation = AtomicU64::new(0);
    let mut port = MockPort::new(vec![
        Step::Retryable("丢失#1"),
        Step::Ok,
        Step::Retryable("丢失#2"),
        Step::Ok,
        Step::Retryable("丢失#3"),
        Step::Ok,
    ]);
    assert_eq!(drive(&mut port, &generation).ok(), Some(2));
    assert_eq!(generation.load(Ordering::SeqCst), 1);
    assert_eq!(drive(&mut port, &generation).ok(), Some(3));
    assert_eq!(generation.load(Ordering::SeqCst), 2);
    assert_eq!(drive(&mut port, &generation).ok(), Some(4));
    assert_eq!(generation.load(Ordering::SeqCst), 3, "三代数,严格单调");
    assert_eq!(port.frames_run, 6, "每轮恢复各多跑一帧");
}

// ---------------------------------------------------------------------------
// 代数配对资源缓存
// ---------------------------------------------------------------------------

#[test]
fn tc_rbt_gpu_generation_cache_pairs_resource_with_generation() {
    let handle = Arc::new(AtomicU64::new(0));
    let mut cache = GenerationCache::new(Arc::clone(&handle));
    cache.insert(String::from("res-gen0"));
    assert!(
        cache.get_if(|v| v == "res-gen0").is_some(),
        "当前代数的资源必须命中"
    );
    // 代数翻转(设备重建)→ 旧代数条目即刻不可见:
    handle.store(1, Ordering::SeqCst);
    assert!(
        cache.get_if(|_| true).is_none(),
        "代数对不上必须未命中(RBT-01 配对失效纪律)"
    );
    // 按新代数重建后可见。
    cache.insert(String::from("res-gen1"));
    assert_eq!(cache.generation(), 1);
    assert!(
        cache.get_if(|v| v == "res-gen1").is_some(),
        "新代数资源必须命中"
    );
    assert!(
        cache.get_if(|v| v == "res-gen0").is_none(),
        "旧代数资源不得再出现"
    );
}

struct DropProbe(Arc<AtomicUsize>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn tc_rbt_gpu_generation_cache_drops_stale_generation_resources() {
    let drops = Arc::new(AtomicUsize::new(0));
    let handle = Arc::new(AtomicU64::new(0));
    let mut cache = GenerationCache::new(handle.clone());
    cache.insert(DropProbe(Arc::clone(&drops)));
    // 代数翻转:旧条目逻辑失效,但写入前不得提前释放(仍可能被读)。
    handle.store(1, Ordering::SeqCst);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    // 新代数写入 → 旧代数条目被释放("全部按代数配对失效")。
    cache.insert(DropProbe(Arc::clone(&drops)));
    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "旧代数资源必须随新代数写入释放"
    );
    drop(cache);
    assert_eq!(drops.load(Ordering::SeqCst), 2, "缓存销毁释放余下资源");
}

#[test]
fn tc_rbt_gpu_generation_cache_validity_check_gates_hits() {
    let handle = Arc::new(AtomicU64::new(7));
    let mut cache = GenerationCache::new(handle);
    cache.insert((64u32, 64u32));
    assert!(
        cache.get_if(|r| r.0 == 64 && r.1 == 64).is_some(),
        "尺寸匹配命中"
    );
    assert!(
        cache.get_if(|r| r.0 == 128).is_none(),
        "valid 不通过(如帧尺寸变更)必须未命中 → 触发重建"
    );
}

// ---------------------------------------------------------------------------
// 后端选择:SABLE_GPU_BACKEND 覆盖(参数注入,零全局 env 污染)
// ---------------------------------------------------------------------------

#[test]
fn tc_rbt_gpu_backend_env_override_values() {
    // 显式覆盖:vulkan / dx12 / gl / auto(大小写不敏感,首尾空白忽略)。
    let sel = resolve_backends(Some("vulkan"));
    assert_eq!(sel.backends, gpu::wgpu::Backends::VULKAN);
    assert_eq!(
        sel.source,
        BackendSource::EnvOverride(String::from("vulkan"))
    );

    let sel = resolve_backends(Some("  DX12 "));
    assert_eq!(sel.backends, gpu::wgpu::Backends::DX12);
    assert_eq!(
        sel.source,
        BackendSource::EnvOverride(String::from("DX12")),
        "来源记录 trim 后的原文"
    );

    assert_eq!(
        resolve_backends(Some("gl")).backends,
        gpu::wgpu::Backends::GL
    );
    assert_eq!(
        resolve_backends(Some("Auto")).backends,
        gpu::wgpu::Backends::PRIMARY,
        "auto = 平台默认,显式覆盖编译期偏好"
    );

    // 无法识别 → 回落编译期默认 + EnvUnrecognized 标注(调用方据此显式告警)。
    let fallback = resolve_backends(None);
    let sel = resolve_backends(Some("bogus"));
    assert_eq!(
        sel.backends, fallback.backends,
        "无法识别必须回落编译期默认"
    );
    assert_eq!(
        sel.source,
        BackendSource::EnvUnrecognized(String::from("bogus"))
    );

    // 空白值视同未设置。
    assert_eq!(resolve_backends(Some("   ")).source, fallback.source);
    assert_eq!(resolve_backends(Some("   ")).backends, fallback.backends);
}

/// 真实 env 只读一致性:env 值(如有)与纯函数语义一致、两个入口互洽。
/// edition 2024 `set_var` 为 unsafe 且 crate 禁 unsafe → 不写全局 env,两分支都断言。
#[test]
fn tc_rbt_gpu_backend_env_unset_path_is_consistent() {
    let expected: BackendSelection = match std::env::var(BACKEND_ENV) {
        Ok(value) => resolve_backends(Some(&value)),
        Err(_) => resolve_backends(None),
    };
    assert_eq!(
        select_backends_detailed(),
        expected,
        "detailed 必须与纯函数一致"
    );
    assert_eq!(
        select_backends(),
        expected.backends,
        "select_backends 必须取 backends 字段"
    );
}

// ---------------------------------------------------------------------------
// TC-RBT-GPU-01:真机冒烟(设备丢失注入属硬件验证,常规路径零恢复副作用)
// ---------------------------------------------------------------------------

/// 真机走查(需要本机存在可用 GPU adapter;无头 CI 无 wgpu noop——vello 未启用
/// 该 feature)。触发:`cargo test -p sable-paint --features gpu -- --ignored`。
///
/// 本测试覆盖"真实设备上帧循环可用且不误触发恢复"。**真机设备丢失注入步骤**
/// (硬件验证,报告 TC-RBT-GPU-01 验收):Windows 上以 dxcap/驱动安装包触发
/// TDR,或设备管理器禁用→启用显卡;期间持续跑本帧循环,预期:
/// 帧入口先返回 `Ok`(丢失信号下一帧探测 → 重建 → 重试),日志出现
/// "GPU 设备丢失回调触发"与"重建设备后重试一次",`generation()` 自增;
/// 连续两次丢失(禁用显卡不恢复)→ 返回 `GpuDeviceLostFatal`(宿主契约三步)。
#[test]
#[ignore = "真机冒烟:需要可用 GPU adapter(TC-RBT-GPU-01 硬件部分;走查步骤见 doc)"]
fn tc_rbt_gpu_01_real_device_render_without_spurious_recovery() {
    let mut renderer =
        block_on(GpuFrameRenderer::create()).expect("真机走查:本机必须有可用 GPU adapter");
    assert_eq!(renderer.generation(), 0, "初始代数 0");
    assert_eq!(
        *renderer.degradation(),
        GpuDegradation::None,
        "正常初始化无降级"
    );

    // 空场景 + 底色:完整走 vello 编码 → GPU 提交 → 纹理产出。
    let output = block_on(renderer.render_frame(64, 64, to_color([32, 96, 160, 255]), |_| {}))
        .expect("真机首帧渲染必须成功");
    assert_eq!(output.width, 64);
    assert_eq!(output.height, 64);
    assert_eq!(renderer.generation(), 0, "正常帧不得触发恢复(代数不动)");

    // 同尺寸第二帧:资源缓存必须命中复用(不重建)。
    let again = block_on(renderer.render_frame(64, 64, to_color([32, 96, 160, 255]), |_| {}))
        .expect("真机第二帧渲染必须成功");
    assert_eq!(renderer.generation(), 0);
    assert_eq!(again.width, 64);
}
