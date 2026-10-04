# NOTICE — 致谢与第三方代码移植登记

Sable UI 采用 **MIT OR Apache-2.0** 双许可(见 LICENSE-MIT / LICENSE-APACHE)。
本仓库中从下列开源项目移植或深度参考的代码,均在源文件头部注释标注来源;按文件逐项登记如下(v0.1 初建):

## 移植/深度参考来源

| 上游 | 许可证 | 移植内容 | 主要落点 |
|---|---|---|---|
| [longbridge/gpui-kit](https://github.com/longbridge/gpui-kit)(gpui-component 0.7.0) | Apache-2.0 | Dock 工作台封装思路、主题/组件三态交互约定 | crates/sable-dock, crates/sable-widgets |
| [zed-industries/zed](https://github.com/zed-industries/zed)(gpui 0.2.2) | Apache-2.0 | Entity/Context 编程模型用法、动作与快捷键注册方式 | crates/sable-widgets, examples/* |
| [linebender/vello](https://github.com/linebender/vello) 及 linebender 全家(kurbo/peniko/parley/vello_cpu) | Apache-2.0 OR MIT | 场景构建、字形绘制调用方式 | crates/sable-paint, crates/sable-canvas |
| [GraphiteEditor/Graphite](https://github.com/GraphiteEditor/Graphite) | Apache-2.0 | 工具状态机(ToolBehavior)设计 | crates/sable-canvas/src/tool.rs |
| [VellumBench vb_ui](https://github.com/GreenChennai/VellumBench)(自研) | ACL-1.0(上游自有) | 仅**设计令牌语义与组件交互规范**的思想对齐(控件高度档/间距网格/三态),未复制源码 | crates/sable-widgets/src/tokens.rs |

## 随包字体(assets/fonts/,TOK-02)

| 字体 | 版本 | 许可证 | 来源 | 落点 |
|---|---|---|---|---|
| [Inter](https://rsms.me/inter/) | 4.1(Inter-Regular / -Medium / -SemiBold.ttf) | SIL Open Font License 1.1(全文:assets/fonts/Inter-LICENSE.txt) | https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip | crates/sable-widgets/src/fonts.rs(include_bytes! 嵌入,UI 字体) |
| [JetBrains Mono](https://www.jetbrains.com/lp/mono/) | 2.304(JetBrainsMono-Regular.ttf) | SIL Open Font License 1.1(全文:assets/fonts/JetBrainsMono-OFL.txt) | https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip | 同上(等宽数值字体) |

字体二进制不做源码级修改(仅重命名入库);OFL 允许随软件分发,保留许可文件与上述出处即满足其条款。

## 纪律

1. 上表"移植内容"列只登记思想级与代码级两类;纯 API 调用不算移植,无需登记。
2. 任何直接复制的源文件必须保留原许可头,并在上表追加一行"文件级"登记。
3. **上游许可红线(阻断级,docs/upstream/00 §风险1)**:
   - VellumBench 是 **ACL-1.0**(自定义协议,实质部分再分发须整体同协议开源)——对 vb_ui/vb_app **只许语义/设计思想对齐,禁止复制源码、禁止逐行翻译**;实现者只读其规格类文档(design/、CONTEXT.md),不对照 .rs 写代码。
   - CutForge 原创部分是 **ARL-1.0**(CORE-FILES 36 个核心文件修改须回传开源)——对 crates/cutforge-* **零源码复制**;本库只经其 MCP 工具/CLI/文件格式对接,不 import 其源码。
   - 两条红线的机器可查验证:CI 的 `cargo tree` 依赖图里不得出现 vb_* / cutforge-* 任何节点。
4. 依赖 VellumBench 时注意:其 ACL-1.0 为自定义协议,**不复制其源码**,只对齐设计语义。
