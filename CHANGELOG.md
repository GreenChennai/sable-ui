# Changelog

本库遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与语义化版本。

## [3.0.0] - 2026-10-02

V3.0 迭代(docs/09-迭代计划V3.0.md):第二轮对标(Inkscape/Figma/Qt QTextDocument/
Blender/Makepad)新维度差距 → 四主题落地。

### Added
- **画布真文本**(G13 P0,T1):parley 布局 + skrifa 字形轮廓 → `PaintSink` 路径绘制
  (CPU/GPU 同代码);TD-10 布局缓存(命中/未命中计数);Text 节点参与渲染/LOD/透明度
- **SVG 渐变完整映射**(G14 P0,T2):LinearGradient/RadialGradient 导入逐 stop 映射
  (gradientTransform 折入、stop-opacity 合成);导出补 `<radialGradient>` 与
  `stop-opacity`;Pattern 仍降级计数
- **自动化脚本地基**(G16 P1,T4):新 crate `sable-script`(rhai 1.26 沙盒):
  12 算子全走 Command 撤销栈(禁直改 Scene),Dynamic 手工 coerce(中文错误消息),
  零 IO/panic 路径;7 测试含 10 矩形循环端到端
- **主题定制 + 切换动画**(G15 P1,T3):`theme::inject` 自定义调色板注入;
  `ThemeTransition` 纯函数状态机(15 token lerp_hsla 最短弧 + OutCubic,200ms)
- **无障碍对接 notes**(G17 P1,T5.1):gpui AccessKit 暴露面 + 逐组件语义挂接清单
  (docs/a11y-notes.md);读屏实测列真机遗留

### Fixed
- **redo 方向 id 治理缺口**(script 端到端实测揪出):重做 AddNode 换发新 id 时,
  redo 栈后继命令(SetFill 等)持旧 id 静默失配 → trait 增 `apply_heal` 钩子,
  History::redo 广播新映射;AddNode.revert 保留退役 id 供映射

### Changed
- 版本 3.0.0;workspace 新成员 sable-script

## [2.0.0] - 2026-10-02

V2.0 迭代(docs/08-迭代计划V2.0.md):同类库对标(Qt/egui/Flutter/SwiftUI/ImGui/Konva)
后的差距分析 → T1 SVG 互通落地;其余主题按"主会话可独立小步合入"排序推进。

### Added
- **SVG 互通**(foundation,feature `svg`):`export_svg(scene) -> String`
  (路径/组/变换/纯色与线性渐变/描边/混合模式 `mix-blend-mode` 序列化,零额外依赖)
  + `import_svg`(usvg 0.48 解析;Text/Image 跳过计数;渐变降级首停纯色并计数;
  10MB/深度 64 恶意输入防御,全 Result 无 panic)
- 门面 `svg` feature 转发;CI 增 SVG 测试 job
- 手册:`docs/08-迭代计划V2.0.md`(对标差距矩阵 G1-G12 + 主题 T1-T6 + 诚实边界)

## [1.0.0] - 2026-10-02

v1.0 目标(分册五 §3 M5"性能与打磨"+ 08 号迭代计划 S3/S4 收束):矢量编辑核心闭环、
效果系统产品化、动画全接线、工程化基线(基准/回归/存取)。**真机-only 遗留项**见
[docs/release-checklist.md](docs/release-checklist.md)。

### Added(矢量核心,S3)
- 钢笔工具五行为收尾:已有路径续接、锚点 hover 闭合提示;锚点编辑模式(拖锚点/拖手柄/
  转直角平滑/删除/段上插锚点吸附),锚点编辑统一走 `SetAnchorPoints` 事务(一步撤销)
- `NumberField` 真文本编辑态(自绘轻量编辑器:caret/选中/校验回落/Enter 提交 Esc 取消)
- 效果栈数据模型:`EffectEntry{spec, enabled}` 挂载 `Node.effects`(serde default 前向兼容),
  五条效果命令(Add/Remove/Move/SetEnabled/SetSpec,全可撤销、Spec 修改可 merge)
- 效果渲染管线(CPU):GaussianBlur/DropShadow/Glow/ColorMatrix 按序作用于节点离屏渲染
  结果并合成回画布;无效果节点零开销;`E12 Off` 档全跳过
- `ColorMatrix` 预设:brightness/saturate/hue_rotate/contrast(SVG feColorMatrix 对齐)
- 组件 story 示例(`cargo run -p story`):输入组件/色轮/渐变/图层 FLIP/时间轴/动画/效果/token 八组演示
- 基准体系(criterion):命令执行/工程存取/命中测试/场景渲染 四组微基准
- 窗口系统材质 Mica/Acrylic/Tabbed(`window-backdrop` feature,Win11 22621+,旧系统静默降级)

### Added(S1 动画,v0.2→v1.0 收束)
- GSAP 式 `AnimationTimeline`(then/together/stagger/repeat/yoyo)、弹簧初速度(fling 接续)、
  `GestureTracker` 修剪均值速度、滚动物理(惯性+橡皮筋)、`lerp_hsla` 色相最短弧、
  `Lerp` 全类型(含 Affine 分解插值)、`AnimScheduler`(同屏 32 上限)、
  13 组黄金值快照(动画手感契约)、`reduced_motion` 全局开关、A7 微交互 14 项接线、
  A4 FLIP 图层让位、A11 关键帧桥(与 video 曲线求值逐点一致)

### Added(效果系统,S2)
- `BlendMode` 16 种入场景图(serde 前向兼容);Conic/Mesh 渐变模型;`squircle` 平滑圆角;
  5 级 elevation token + `render_shadow_rgba` + `ShadowCache`;`BLUE_NOISE_64` 蓝噪点;
  `EffectLevel/EffectCaps` 降级矩阵(`SABLE_EFFECTS_LEVEL`)
- 面板级毛玻璃:Reduced 档(半透明 surface + 蓝噪点 + 1px 高光边)已可;Dual Kawase GPU
  管线见 release-checklist(真机项)

### Changed
- 工程格式 `.lumi` → `.sable`(魔数 `SABL`);环境变量前缀 `LUMINA_*` → `SABLE_*`
- 项目更名 **Sable UI**(原 Lumina UI);crate 家族 sable-foundation/paint/canvas/widgets/dock/video/ui

### Fixed
- GPU 路径编译失败(wgpu 30/29 错配 + flags 改名):paint 统一经 `vello::wgpu` re-export
- CI:feature 矩阵 job(gpu/all-features/core-only/cpu-render)+ wgpu 单版本检查
- timeline `value_at` 语义(活动段)、橡皮筋 FP 饱和单调性、Affine 镜像插值契约等 17 项验收修复

### Performance
- 撤销栈跨保存(roundtrip 测试锁定);wgpu 单版本;黄金渲染基线 5 场景(像素容差 0.1%);
  criterion 微基准四组(命令/存取/命中/渲染)

## [0.2.0] - 2026-10-02

- 效果数据模型+CPU 层(E4/E5/E8/E9/E10/E12)、动画引擎(A1-A12)、`.sable` 工程存取、
  渲染黄金基线、更名 Sable UI

## [0.1.0] - 2026-10-02

- 7 crate 架构(视口/场景图/命令撤销系统/PaintSink 双后端/画布内核/时间轴模型/
  设计 token+组件/Dock 工作台/门面 feature 矩阵)、双示例、手册 00-07
