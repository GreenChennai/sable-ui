# Release Checklist(v1.0 发布清单)

> 本清单区分"已完成(本仓库可验证)"与"真机-only 遗留"(需要 Win11 实机 /
> RenderDoc / 人工交互输入,无法在 CI 或本机沙箱自动验证)。

## 1. 已完成(可验证)

- [x] 门禁:fmt / clippy `-D warnings` / test(全 workspace)/ build --release
- [x] feature 矩阵:gpu / all-features / core-only / core+cpu-render(CI feature-matrix job)
- [x] 渲染回归:5 场景黄金 PNG,像素容差 0.1%(`SABLE_UPDATE_GOLDENS=1` 再生成)
- [x] 撤销正确性:proptest 随机命令序列回快照(core 256 默认 / 2000 加压) + `.sable` 存取 roundtrip
- [x] 动画契约:13 组黄金值快照(1e-9)
- [x] 基准体系:criterion 四组(命令/存取/命中/渲染)——`cargo bench` 人工跑,数据入 RELEASE NOTES
- [x] 降级矩阵:E12 三档(gpu/cpu-only/env off)+ sink 层 no-op 二重降级

## 2. 真机-only 遗留(发布说明须列明)

| # | 项 | 需要 | 状态 |
|---|---|---|---|
| T1 | DX11⇄wgpu 纹理桥零拷贝(分册六 §6.4) | Win11 + 支持 Vulkan/DX12 互操作的 GPU + RenderDoc 帧分析 | 实现与降级链已就绪;本机 GT 710 无 Vulkan,无法验证零拷贝通路 → 默认走读回兜底 |
| T2 | 降级链三档实测(读回 / vello_cpu / 强制 Vulkan) | 同上;`SABLE_GPU_BACKEND=vulkan` 与 `LUMINA_EFFECTS_LEVEL`(现 `SABLE_EFFECTS_LEVEL`) | 代码路径单测覆盖;帧时间数据待真机 |
| T3 | IME 中文输入(属性框/命名框,TD-03) | 真人交互 + 候选窗截图 | gpui 0.2.2 Windows IME 已知缺陷(zed#40300/#41881);降级路径:系统原生输入对话框(预留) |
| T4 | Mica/Acrylic 视觉验收(E3) | Win11 22621+ | API 调用与失败降级已实现;本机 Win10 19045 只能验降级路径 |
| T5 | Dual Kawase ≤1.5ms 性能验收(E1) | RenderDoc/nsight 计时 | WGSL 管线脚手架未落(CPU Reduced 档已可);真机 Sprint 落地 |
| T6 | 触控板/高 DPI 手感 | 物理设备 | winit 事件流已标准化,参数调优待反馈 |

## 3. crates.io 发布步骤

```bash
# 1. 账号:crates.io 需独立 token(github 登录 crates.io → Account → New Token)
export CARGO_REGISTRY_TOKEN="..."
# 2. 顺序(foundation 被 paint 依赖,paint 被 canvas 依赖,依金字塔自底向上):
cargo publish -p sable-foundation
cargo publish -p sable-paint
cargo publish -p sable-canvas      # 依赖 foundation+paint
cargo publish -p sable-video
cargo publish -p sable-widgets
cargo publish -p sable-dock        # 依赖 widgets
cargo publish -p sable             # 门面最后
# 3. 每步前:
cargo package -p <crate> --list    # 审文件清单,确认无多余文件
cargo publish --dry-run -p <crate> # 本地校验(docs.rs 链接/许可证/描述)
```

- 双许可文件(LICENSE-MIT/LICENSE-APACHE)已在仓库根,cargo package 自动打入。
- `repository` 字段指向 https://github.com/GreenChennai/sable-ui 。
- docs.rs 构建依赖:canvas 默认 feature 含 gpu 依赖(vello/wgpu),docs.rs 是 linux x64
  纯编译——vello/wgpu 可编译;若 docs.rs 失败,在 Cargo.toml 加
  `[package.metadata.docs.rs] no-default-features = true, features = ["core", "cpu-render"]`。

## 4. 版本纪律

- v1.0.0 起 pub API 面按语义化版本:破坏性改动 = minor+1(1.x 期间 MSRV/依赖大版本
  上移允许 minor,API 破坏必须 major)。
- `vello::wgpu` re-export 策略升级随 vello 升级窗口整体走(分册六 TD-01)。
