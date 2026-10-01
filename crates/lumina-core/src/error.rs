//! core 层统一错误类型:库层一律 thiserror 枚举(AGENTS.md §3.3、分册六 TD-06)。

use crate::scene::NodeId;

/// lumina-core 的错误集合。
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// 目标节点句柄在场景中不存在(可能已被删除,或句柄来自另一个场景)。
    #[error("节点不存在: {0:?}")]
    NodeNotFound(NodeId),

    /// 指定的父节点不存在。
    #[error("父节点不存在: {0:?}")]
    ParentNotFound(NodeId),

    /// 拒绝操作:会把节点移进它自己的子树,形成环。
    #[error("拒绝操作:会把节点 {0:?} 移进它自己的子树(成环)")]
    CycleDetected(NodeId),

    /// 子节点插入下标越界。
    #[error("子节点下标越界:index {index},有效范围 0..={len}")]
    IndexOutOfBounds { index: usize, len: usize },

    /// 场景结构损坏:节点存活但未挂在任何父节点/根列表中(正常使用不会出现)。
    #[error("场景结构损坏:节点 {0:?} 未挂在任何父节点/根列表中")]
    OrphanNode(NodeId),

    /// 文件系统错误(原子写/自动保存)。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// core 层统一 `Result` 别名。
pub type CoreResult<T> = Result<T, CoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_carries_context() {
        let err = CoreError::NodeNotFound(NodeId::default());
        assert!(err.to_string().contains("节点不存在"));

        let err = CoreError::ParentNotFound(NodeId::default());
        assert!(err.to_string().contains("父节点不存在"));

        let err = CoreError::CycleDetected(NodeId::default());
        assert!(err.to_string().contains("成环"));

        let err = CoreError::IndexOutOfBounds { index: 3, len: 1 };
        assert!(err.to_string().contains("越界"));
        assert!(err.to_string().contains("index 3"));

        let err = CoreError::OrphanNode(NodeId::default());
        assert!(err.to_string().contains("结构损坏"));

        let err = CoreError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file gone",
        ));
        assert!(err.to_string().contains("IO 错误"));
        // #[from] 转换链可用
        let CoreError::Io(ref inner) = err else {
            panic!("应为 Io 变体");
        };
        assert_eq!(inner.kind(), std::io::ErrorKind::NotFound);
    }
}
