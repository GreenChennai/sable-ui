# NOTICE — 致谢与第三方代码移植登记

Lumina UI 采用 **MIT OR Apache-2.0** 双许可(见 LICENSE-MIT / LICENSE-APACHE)。
本仓库中从下列开源项目移植或深度参考的代码,均在源文件头部注释标注来源;按文件逐项登记如下(v0.1 初建):

## 移植/深度参考来源

| 上游 | 许可证 | 移植内容 | 主要落点 |
|---|---|---|---|
| [longbridge/gpui-kit](https://github.com/longbridge/gpui-kit)(gpui-component 0.7.0) | Apache-2.0 | Dock 工作台封装思路、主题/组件三态交互约定 | crates/lumina-dock, crates/lumina-widgets |
| [zed-industries/zed](https://github.com/zed-industries/zed)(gpui 0.2.2) | Apache-2.0 | Entity/Context 编程模型用法、动作与快捷键注册方式 | crates/lumina-widgets, examples/* |
| [linebender/vello](https://github.com/linebender/vello) 及 linebender 全家(kurbo/peniko/parley/vello_cpu) | Apache-2.0 OR MIT | 场景构建、字形绘制调用方式 | crates/lumina-paint, crates/lumina-canvas |
| [GraphiteEditor/Graphite](https://github.com/GraphiteEditor/Graphite) | Apache-2.0 | 工具状态机(ToolBehavior)设计 | crates/lumina-canvas/src/tool.rs |
| [VellumBench vb_ui](https://github.com/GreenChennai/VellumBench)(自研) | ACL-1.0(上游自有) | 仅**设计令牌语义与组件交互规范**的思想对齐(控件高度档/间距网格/三态),未复制源码 | crates/lumina-widgets/src/tokens.rs |

## 纪律

1. 上表"移植内容"列只登记思想级与代码级两类;纯 API 调用不算移植,无需登记。
2. 任何直接复制的源文件必须保留原许可头,并在上表追加一行"文件级"登记。
3. 依赖 VellumBench 时注意:其 ACL-1.0 为自定义协议,**不复制其源码**,只对齐设计语义。
