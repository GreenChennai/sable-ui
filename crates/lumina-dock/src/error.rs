//! `DockError` / `DockResult`:lumina-dock 的统一错误(AGENTS.md §3.3:
//! 库层错误一律 thiserror 枚举,pub API 禁 `unwrap`/`Box<dyn Error>`)。

/// lumina-dock 的统一错误。
#[derive(Debug, thiserror::Error)]
pub enum DockError {
    /// 布局 JSON 序列化/反序列化失败([`save_layout`](crate::save_layout) /
    /// [`load_layout`](crate::load_layout) 的 serde 环节)。
    #[error("布局序列化失败: {0}")]
    Json(#[from] serde_json::Error),

    /// `DockArea::load` 拒绝了该布局(结构非法/上游重建失败)。
    #[error("布局加载失败: {0}")]
    Load(String),
}

/// 便捷别名。
pub type DockResult<T> = std::result::Result<T, DockError>;
