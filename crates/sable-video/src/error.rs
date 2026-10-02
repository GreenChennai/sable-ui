//! video 层统一错误类型:库层一律 thiserror 枚举(与 sable-foundation 同构,分册六 TD-06)。

use crate::model::TrackKind;

/// sable-video 的错误集合。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VideoError {
    /// 目标 clip id 不存在(可能已被删除;ClipId 永不复用,不会误指别的 clip)。
    #[error("clip 不存在: {0}")]
    ClipNotFound(u64),

    /// 轨道下标越界。
    #[error("轨道不存在: {0}")]
    TrackNotFound(usize),

    /// 放不下:目标区间与本轨已有 clip 重叠(时间轴铁律:同轨 clip 互不重叠)。
    #[error("区间重叠:同轨 clip 互不重叠,操作被拒绝")]
    Overlap,

    /// 非法参数/区间(out<=in、speed<=0、duration==0、分割点不在 clip 内部等)。
    #[error("非法区间或参数: {0}")]
    InvalidRange(&'static str),

    /// 找不到该类型的轨道(如拖入音频素材但时间轴上没有音频轨)。
    #[error("没有可用的 {0:?} 轨道")]
    NoActiveTrack(TrackKind),

    /// 时间轴为空(或只剩最后一条轨道,拒绝删空)。
    #[error("时间轴为空")]
    EmptyTimeline,
}

/// video 层统一 `Result` 别名。
pub type VideoResult<T> = Result<T, VideoError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_carries_context() {
        assert!(
            VideoError::ClipNotFound(7)
                .to_string()
                .contains("clip 不存在")
        );
        assert!(VideoError::ClipNotFound(7).to_string().contains('7'));

        assert!(
            VideoError::TrackNotFound(3)
                .to_string()
                .contains("轨道不存在")
        );

        assert!(VideoError::Overlap.to_string().contains("重叠"));

        let err = VideoError::InvalidRange("出点必须大于入点");
        assert!(err.to_string().contains("非法区间"));
        assert!(err.to_string().contains("出点必须大于入点"));

        assert!(
            VideoError::NoActiveTrack(TrackKind::Audio)
                .to_string()
                .contains("Audio")
        );

        assert!(VideoError::EmptyTimeline.to_string().contains("时间轴为空"));
    }
}
