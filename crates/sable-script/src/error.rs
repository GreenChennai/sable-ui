//! 脚本层错误:thiserror 枚举(AGENTS.md §3.3,库层禁 `Box<dyn Error>`)。

/// 脚本宿主的错误集合。
///
/// 有意保持极简:脚本侧的失败只有三类——**求值失败**(语法错、运行时错,
/// 含宿主算子对非法参数/非法 id 的拒绝)、**资源超限**(如死循环撞上
/// 操作数上限)与**取值失败**(脚本返回值转 Rust 类型不匹配)。
#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    /// 脚本求值失败:语法错误、运行时错误,或宿主算子拒绝执行
    /// (节点/父节点不存在、类型不合法、坐标非有限等)。消息含 rhai 的
    /// 行列定位信息。
    #[error("脚本执行失败: {0}")]
    Eval(String),

    /// 脚本执行超出沙盒资源上限(目前唯一来源是 rhai 操作数超限,常见于
    /// 死循环)。单列变体让调用方可程序化区分"失控脚本被拦下";rhai 原始
    /// 错误文本(含定位)附在消息尾——本枚举持 `String`、不链
    /// `Box<dyn Error>`(AGENTS.md §3.3),附注即保留原始信息的形态。
    #[error("脚本执行超过资源上限: {0}")]
    Limit(String),

    /// 取值失败:`run` 的返回值(Dynamic)转成目标 Rust 类型不匹配。
    #[error("脚本返回值类型不匹配: {0}")]
    Cast(String),
}

/// 脚本层统一 `Result` 别名。
pub type ScriptResult<T> = Result<T, ScriptError>;

impl ScriptError {
    /// 消息正文(不含变体前缀)。
    ///
    /// 宿主算子错误要经 `with_state` 塞进 rhai 运行时错误、再由 `run` 包回
    /// [`ScriptError::Eval`]:两层包装若都走 `to_string()`,"脚本执行失败"
    /// 前缀会重复出现,故内层只取正文,前缀由最外层变体文案统一给。
    pub(crate) fn message(&self) -> &str {
        match self {
            Self::Eval(msg) | Self::Limit(msg) | Self::Cast(msg) => msg,
        }
    }
}

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

    #[test]
    fn limit_display_carries_resource_semantics_and_original_text() {
        let err = ScriptError::Limit(
            "已达操作数上限 1000000 步(常见于死循环);rhai 原始错误: Too many operations".into(),
        );
        let msg = err.to_string();
        assert!(msg.contains("资源上限"), "变体文案应有上限语义:{msg}");
        assert!(msg.contains("操作数上限"), "附注应含上限语义关键字:{msg}");
        assert!(
            msg.contains("Too many operations"),
            "原始 rhai 错误文本应保留在附注里:{msg}"
        );
    }
}
