# Lumina UI(流明)—— 面向创作工具的高性能 Rust 前端库

> 不是又一个通用 UI 框架,而是**创作工具专用 UI 库**(Creative Tool UI Kit)。
> 目标形态:Adobe Illustrator(矢量设计)+ 剪映(视频剪辑)级 Windows 桌面创作软件的前端层。
> 手册:[docs/00 总纲](docs/00-总纲.md) → [docs/06 技术债](docs/06-鲁棒性解耦审美动画Vulkan与技术债.md)

## 一句话架构

**GPUI 负责窗口与面板,Vello 负责画布,Lumina 把它们粘合成 Illustrator/剪映能用的东西。**

```
┌─────────────────────────────────────────────────────┐
│ L6 应用层    你的软件(矢量设计模块 / 视频剪辑模块)      │
├─────────────────────────────────────────────────────┤
│ L5 专用组件  画布视图/图层面板/时间轴/属性检查器/色轮     │  lumina-widgets
├─────────────────────────────────────────────────────┤
│ L4 面板系统  Dock 布局/工具栏/菜单/快捷键/命令面板       │  lumina-dock (gpui-component)
├─────────────────────────────────────────────────────┤
│ L3 画布内核  场景图/视口变换/命中测试/增量重绘/LOD       │  lumina-canvas
├─────────────────────────────────────────────────────┤
│ L2 渲染层    Vello (GPU 2D) / Kurbo (曲线) / vello_cpu │  lumina-paint
├─────────────────────────────────────────────────────┤
│ L1 平台层    GPUI (窗口/事件/状态) / wgpu / winit       │
└─────────────────────────────────────────────────────┘
        L0 基础类型:视口/场景图类型/命令系统(撤销重做)     lumina-core
```

## Crate 一览

| crate | 职责 | 依赖纪律 |
|---|---|---|
| `lumina-core` | 视口变换、颜色/几何基础类型、Command/History 撤销重做、原子写持久化 | 仅 kurbo/peniko/serde/slotmap,**零 UI 框架依赖** |
| `lumina-paint` | 绘制原语(画笔/渐变/描边/滤镜)、GPU 后端选择(auto/DX12/Vulkan)、GpuGuard 设备丢失恢复、vello_cpu 兜底 | core + vello(可换 vello_cpu) |
| `lumina-canvas` | 场景图(slotmap)、无限画布、命中测试、网格、脏矩形、LOD、文本管线 | paint + core |
| `lumina-widgets` | 设计 token、主题、动画引擎、NumberField/色轮/图层面板/属性检查器/时间轴视图 | canvas + core(+ gpui) |
| `lumina-dock` | Dock 工作台布局(基于 gpui-component DockArea)、布局序列化 | widgets(+ gpui-component) |
| `lumina-video` | 时间轴数据模型、轨道/clip/关键帧、吸附、播放时钟 | core(+ canvas) |
| `lumina` | 门面 crate,re-export 全部,feature 按场景点菜 | 全部可选 |

## 快速开始

```toml
[dependencies]
lumina = { path = "crates/lumina" }          # 完整套件
# 只想要命令系统/数据模型(CLI 工具):
# lumina = { default-features = false, features = ["core"] }
# 服务器端渲染,无 GPU:
# lumina = { default-features = false, features = ["core", "cpu-render"] }
```

```bash
cargo run -p vector_editor    # 迷你 Illustrator 示例
cargo run -p video_editor     # 迷你剪映示例
```

## 上游计划(本库的存在理由)

| 上游项目 | 现状 | Lumina 接管方式 |
|---|---|---|
| [CutForge](https://github.com/GreenChennai/cutforge) | 视频剪辑内核(schema/core/io/render),尚无桌面 UI | `apps/desktop` 直接建在 lumina-dock + lumina-video 上 |
| [VellumBench](https://github.com/GreenChennai/VellumBench) | egui 0.35 自研主题/组件 | vb_ui 的设计令牌/组件语义迁移到 lumina-widgets,egui 逐步退役 |

详见 [docs/upstream/](docs/upstream/) 上游 UI 分析。

## 构建

- Windows 10/11 + MSVC(Rust 1.85+);GPU 需要 DX12 或 Vulkan(无 GPU 自动落 `cpu-render`)
- 本机开发约定见 [AGENTS.md](AGENTS.md)(cargo 缓存在 D 盘、`-j2` 防内存耗尽)
- 门禁:`cargo fmt --check` → `cargo clippy --all-targets -- -D warnings` → `cargo test` → `cargo build --release`

## 许可

MIT OR Apache-2.0 双许可。对 gpui-component(Apache-2.0)、egui、Graphite、Gausian 等参考实现的借鉴与移植在 [NOTICE.md](NOTICE.md) 中逐项致谢。
