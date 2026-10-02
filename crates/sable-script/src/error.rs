//! 脚本层错误:thiserror 枚举(AGENTS.md §3.3,库层禁 `Box<dyn Error>`)。

/// 脚本宿主的错误集合。
///
/// 有意保持极简:脚本侧的失败只有两类——**求值失败**(语法错、运行时错,
/// 含宿主算子对非法参数/非法 id 的拒绝)与**取值失败**(脚本返回值转
/// Rust 类型不匹配)。
#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    /// 脚本求值失败:语法错误、运行时错误,或宿主算子拒绝执行
    /// (节点/父节点不存在、类型不合法等)。消息含 rhai 的行列定位信息。
    #[error("脚本执行失败: {0}")]
    Eval(String),

    /// 取值失败:`run` 的返回值(Dynamic)转成目标 Rust 类型不匹配。
    #[error("脚本返回值类型不匹配: {0}")]
    Cast(String),
}

/// 脚本层统一 `Result` 别名。
pub type ScriptResult<T> = Result<T, ScriptError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_carries_context() {
        let err = ScriptError::Eval("第 3 行:变量 x 未定义".into());
        assert!(err.to_string().contains("脚本执行失败"));
        assert!(err.to_string().contains("第 3 行"));

        let err = ScriptError::Cast("期望 i64,实际 ()".into());
        assert!(err.to_string().contains("类型不匹配"));
        assert!(err.to_string().contains("i64"));
    }
}
