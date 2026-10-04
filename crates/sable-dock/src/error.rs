//! `DockError` / `DockResult`:sable-dock 的统一错误(AGENTS.md §3.3:
//! 库层错误一律 thiserror 枚举,pub API 禁 `unwrap`/`Box<dyn Error>`)。

/// sable-dock 的统一错误。
#[derive(Debug, thiserror::Error)]
pub enum DockError {
    /// 布局 JSON 序列化/反序列化失败([`save_layout`](crate::save_layout) /
    /// [`restore_layout`](crate::restore_layout) 的 serde 环节)。
    #[error("布局序列化失败: {0}")]
    Json(#[from] serde_json::Error),

    /// 布局文件读写失败([`persist_layout`](crate::persist_layout) 落盘 /
    /// [`load_layout`](crate::load_layout) 读盘的 IO 环节;原子写语义由
    /// `sable_foundation::persistence::atomic_write` 保证,失败不留半截文件)。
    #[error("布局文件 IO 失败: {0}")]
    Io(#[from] std::io::Error),

    /// `DockArea::load` 拒绝了该布局(结构非法/上游重建失败)。
    #[error("布局加载失败: {0}")]
    Load(String),
}

/// 便捷别名。
pub type DockResult<T> = std::result::Result<T, DockError>;
