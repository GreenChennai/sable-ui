//! 统一错误类型(AGENTS.md 纪律 3:库层错误一律 thiserror 枚举,禁止 unwrap)。

/// sable-paint 的统一错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaintError {
    /// 请求不到任何可用 GPU adapter(分册六 §6.4 降级链第 3 级失败后的终点)。
    #[error("no suitable GPU adapter found")]
    NoAdapter,
    /// GPU 设备请求/重建失败(携底层错误文本)。
    #[error("GPU device request failed: {0}")]
    DeviceRequest(String),
    /// 渲染过程中失败(不含设备丢失恢复耗尽——那走 [`PaintError::GpuDeviceLostFatal`])。
    #[error("render operation failed: {0}")]
    Render(String),
    /// GPU 设备丢失且自动恢复耗尽(RBT-01)——**宿主契约信号**,不是普通渲染错误。
    ///
    /// 宿主收到此变体必须按序执行三步(缺一不可;自动保存本体属宿主职责,
    /// 库只负责信号与载荷,不做任何 UI/IO):
    /// 1. **落自动保存**(用户工作不因设备丢失而丢);
    /// 2. **渲染路径切 cpu-render**([`crate::cpu::CpuRenderer`],降级链第 4 级);
    /// 3. **提示用户**(设备已重置,工作已保存)。
    #[error(
        "GPU device lost: automatic recovery exhausted ({reason}); \
         host contract: autosave -> switch to cpu-render -> notify the user"
    )]
    GpuDeviceLostFatal { reason: String },
    /// 请求了当前后端/设备不支持的能力。
    #[error("unsupported feature: {0}")]
    UnsupportedFeature(String),
}

/// 便捷别名:`Result<T, PaintError>`。
pub type PaintResult<T> = Result<T, PaintError>;
