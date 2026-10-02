# Changelog

本库遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与语义化版本。

## [Unreleased]

### Added
- **`sable-widgets::neon_card`**(V4.1 组件波次 1/1,对标 luminaui.in "Neon Card"):
  流动渐变描边(shimmer,窗口锚定卡心无缝循环)、环境辉光、光标跟随内外辉光、
  hover 漂移粒子(种子确定性)、hover 缩放 1.02 与入场动画,reduced_motion 全直切;
  纯函数渲染(可独立单测)+ 帧缓存(空闲零帧提交)。真 backdrop-blur 为 GPU 路线图
  项,卡体为"半透明底 + 高光内边"等效档(docs/11 对照表)
- story 第 9 分组:三配色 Neon Card 实机演示(粉紫对标默认/青蓝/金橙)
- widgets 新增内部依赖边 widgets → sable-paint(向下,离屏自绘底座;手册 §2.1 待补记)

## [4.0.0] - 2026-10-02

V4.0 迭代(docs/10-迭代计划V4.0.md):V3.0 复盘 review(3 路并行代码核查)实锤
"库层有、接线无"的八项声称差距(R1-R8)→ 本轮全部收口,**声称即现实**。

### Added
- **效果栈接入场景渲染**(G20 P0,T2):`render_scene` 真实消费 `node.effects`——
  EffectSurface 离屏 → `apply_effects_rgba` → `draw_rgba` 回贴,包裹在 push_blend
  层内(效果先于混合);Off 档/空栈/不支持回贴的 sink 走与 v3.0 字面相同的直绘路径
  (输出逐位一致);`VelloCpuSink` 补 `draw_rgba` 真实现;golden 第 6 基线
  `effects_shadow`(旧 5 张 sha1 逐位不变)。此前效果管线只有自测调用(review R1)
- **SVG 文本互通**(G22 P0,T3,收口 V3.0 T1.3):导入 `usvg Text::flattened()` 轮廓化
  (并修复真正静默丢因:默认 fontdb 空库致文本连解析树都进不了,补 load_system_fonts);
  导出 Text→`<text>`(XML 转义;纯平移拆 x/y)+ `export_svg_with_report`。
  此前文本双向静默丢(review R3)
- **主题接线**(G23 P1,T5):`theme::set_mode_animated`(reduced_motion 直切/过渡两路)+
  `advance_transition` 帧泵(落定零帧提交)+ `inject` 取消活动过渡;story 两处瞬切
  换过渡帧泵 + 紫 accent 注入/还原演示。此前 `ThemeTransition`/`inject` 是零调用死代码
  (review R5)
- **脚本沙盒资源限制**(G25 P1,T4):默认 `set_max_operations`(10^6,`while true{}`
  确定性报 `ScriptError::Limit` 不挂死宿主);坐标 coerce 后 `is_finite` 拒收;
  id 编码 u64↔i64 位型无损(高位 key 不再被负数守卫误拒)
- **文本精度与缓存治理**(G24 P1,T6):Text bbox 弃 0.6em/字符粗估改实测布局尺寸
  (CJK ≈1.0em/字),命中/框选/剔除/LOD/选中框/脏矩形全链路换实测口径;TD-10 布局
  缓存 256 条 LRU + evictions 计数;字号非有限/≤0 双入口钳制(NaN 不进缓存键)

### Fixed
- **事务 redo id 治理缺口**(G21 P0,T1):`BatchCommand` 覆写 `apply_heal` 与 revert
  对称——修复 `begin_transaction→AddNode→SetFill(新id)→undo→redo` 后 SetFill 静默
  失配(CHANGELOG 3.0 该修复的事务变体,review R2);随机事务序列 proptest 覆盖
  undo+redo 双向
- SVG spreadMethod reflect/repeat 降级不再静默(计入 `simplified_spreads`);导出
  渐变 defs id 改计数器单调派生(消除 `g{len+Σ字节}` 可碰撞)(G26,T3)
- a11y-notes §3 六组件虚标 "✅ V3.0" 改回 ☐ 遗留(v4.1 起)——`.with_label` 在
  widgets 中不存在(review R4)
- InvalidMagic 错误消息 `b"LUMI"` → `b"SABL"`(1.0 更名遗留)
- `atomic_write` 临时名含 pid(并发写同一目标不再共享冲突;原名残留 `.lumi`)
- 描边命中容差世界/局部坐标口径:节点缩放 k 时局部容差除 k(放大节点命中域不再虚大)
- 根 Cargo.toml 版本策略注释 gpui-component 0.7.0 → 0.5.1(以 Cargo.lock 为准;
  门面/dock 的 0.5.1 文档本就正确)

### Changed
- 文档诚实化:README Status v1.0→v4.0、效果栈措辞按实况(GPU 效果 pass 明示未做)、
  示例定位标注为"库能力演示"(vector_editor 画布撤销断裂、video_editor 数据模型演示
  均如实披露);docs/05 里程碑状态列冻结声明(以 CHANGELOG + 08/09/10 迭代计划为准)

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
