//! 布局助手(TOK-10:从 tokens.rs 迁入——tokens 只管"值",布局归布局;
//! `tokens::h_flex/v_flex` 路径经 re-export 保持兼容,prelude 不变)。

use gpui::{Div, Styled, div};

/// 水平弹性容器预设(行):flex row + 垂直居中。
///
/// gpui 0.2.2 / gpui-component 0.7.0 均未内置 `h_flex`/`v_flex`(已核实
/// 两份源码),本 crate 自备极薄预设;放在 tokens 模块因为全部组件都依赖
/// theme feature(见 lib.rs 的 mod 门控)。
pub fn h_flex() -> Div {
    div().flex().flex_row().items_center()
}

/// 垂直弹性容器预设(列)。
pub fn v_flex() -> Div {
    div().flex().flex_col()
}
#[cfg(test)]
mod tests {
    // TOK-10:tokens 路径兼容断言(re-export 在岗)
    #[test]
    fn tc_tok_layout_tokens_path_reexported() {
        #[allow(unused_imports)]
        use crate::tokens::{h_flex, v_flex};
        let _ = h_flex;
        let _ = v_flex;
    }
}
