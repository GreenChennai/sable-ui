# AGENTS.md — AI Agent 构建与协作规约(必读)

## 0. 本机构建环境(硬约束)

```bash
# 每个新 shell 必须先执行(C 盘只剩 ~1GB,一切 cargo 产物必须落 D 盘):
export PATH="/c/Users/Administrator/.cargo/bin:$PATH"
export CARGO_HOME=/d/cargo-home
export TEMP=/d/Temp TMP=/d/Temp
```

- **并发锁**:`.cargo/config.toml` 已设 `build.jobs = 2` —— 本机 16GB 内存且页面文件紧张,rustc 并发更高会 OOM(0xc0000409)。**同一时间只允许一个 cargo 进程**,多 Agent 并行写代码可以,跑 cargo 必须串行。
- Rust 1.98 stable-msvc;VS Build Tools 2022(链接器)已装。

## 1. 语言门禁(发布前必须全绿,如实引用退出码)

```
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test  --workspace
cargo build --release
```

## 2. Workspace 地图

```
crates/lumina-core     L0 视口/命令系统/颜色/原子写   —— 零 UI 依赖,CI 单独 --no-default-features 编译
crates/lumina-paint    L2 画笔/渐变/后端选择/GpuGuard
crates/lumina-canvas   L3 场景图/命中测试/网格/脏矩形/LOD
crates/lumina-widgets  L5 token/主题/动画/专用组件
crates/lumina-dock     L4 Dock 工作台(gpui-component)
crates/lumina-video    时间轴模型/吸附/播放时钟
crates/lumina          门面(feature 矩阵)
examples/vector_editor 迷你 Illustrator
examples/video_editor  迷你剪映
docs/00..06            项目手册(活文档,代码进 main 必须同步更新对应章节)
```

依赖方向严格单向:`video/widgets → canvas → paint → core`。禁止 widgets 直接依赖 video;禁止 core 出现任何 gpui/egui 依赖(CI 有 --no-default-features 单独编译把关)。

## 3. 编码纪律(违者 review 打回)

1. **一切文档修改走 Command**,直接改 `Scene` 的路径不允许存在(撤销重做是产品的存在理由)。
2. `#![deny(unsafe_code)]` 全 workspace,无豁免(v0.1 不接 ffmpeg 原生绑定)。
3. 库层错误一律 `thiserror` 枚举,禁止 `unwrap`/`Box<dyn Error>` 出现在 pub API;内部可用 `expect` 但必须带原因字符串。
4. 组件内**禁止硬编码颜色/魔法数字**,一切走 `tokens.rs` 设计 token(间距 4px 网格,圆角 4/6/8/12)。**控件高度体系(docs/upstream/02 实证裁决)**:22/26/32 三档是**下限**,实际控件高度 = max(档位值, 文本实际行高 + 2×垂直 padding)——VellumBench 实测固定行高会压 CJK 文字,必须按内容派生。
5. lumina-* 内禁止业务名词("剪映/贴纸/CutForge"),业务词留在应用层。
6. 世界坐标一律 f64,交给 GPU 前一刻才降 f32 且原点归位。
7. 新 pub API 同步更新 docs/ 对应分册;从参考库移植的代码在文件头注释标注来源仓库与许可证。

## 4. 参考与移植来源(全部可抄,注意署名)

| 来源 | 学什么 | 许可证 |
|---|---|---|
| zed-industries/zed(gpui 0.2.2) | 窗口/事件/Entity 模型 | Apache-2.0 |
| longbridge/gpui-kit(gpui-component 0.7.0) | 60+ 组件、DockArea、主题 | Apache-2.0 |
| linebender(vello/kurbo/peniko/parley) | 渲染/曲线/文本 | Apache-2.0 OR MIT |
| GraphiteEditor/Graphite | 矢量编辑器工具状态机 | Apache-2.0 |
| emilk/egui + VellumBench vb_ui | 备选后端、token 语义 | MIT OR Apache-2.0 |

移植代码放行规则:保留原文件头许可声明,并在 `NOTICE.md` 登记。

## 5. 手册(00→06)与代码的对应

| 手册 | 内容 | 对应 crate |
|---|---|---|
| docs/00-总纲 | 选型/架构/路线图 | 全局 |
| docs/01-生态调研 | 框架评估矩阵 | 全局 |
| docs/02-渲染与画布 | 视口/场景图/命中测试/LOD | core+paint+canvas |
| docs/03-UI框架层 | 命令系统/Binding/动作/主题 | core+widgets+dock |
| docs/04-专用组件 | 12 组件设计与验收 | widgets+video |
| docs/05-路线图与工程化 | 里程碑/测试/打包 | 工程 |
| docs/06-鲁棒性与技术债 | 六道防线/feature矩阵/token/动画/后端/TD台账 | 全局 |
