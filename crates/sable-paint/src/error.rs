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
    /// 渲染过程中失败(含设备重建后重试仍失败)。
    #[error("render operation failed: {0}")]
    Render(String),
    /// 请求了当前后端/设备不支持的能力。
    #[error("unsupported feature: {0}")]
    UnsupportedFeature(String),
}

/// 便捷别名:`Result<T, PaintError>`。
pub type PaintResult<T> = Result<T, PaintError>;
