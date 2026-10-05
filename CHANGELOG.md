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
- **GPU 设备丢失恢复接线**(RBT-01,迭代审查报告 2026-10-04):`sable-paint::gpu_frame::GpuFrameRenderer`
  生产帧入口——资源按 `(generation, resource)` 配对缓存,每帧经 `render_with_recovery`
  幂等闭包;首次设备丢失→重建重试(代数自增),二连败→`PaintError::GpuDeviceLostFatal`
  (宿主契约:自动保存→切 cpu-render→提示用户);`SABLE_GPU_BACKEND` 运行期后端覆盖
  (dx12/vulkan/gl/auto,识别失败显式告警);TC-GATE-GPU-01 生产调用点静态门禁 +
  可注入恢复状态机测试(真机冒烟 `#[ignore]` 通过)。此前 GpuGuard 全仓零调用方
- **sable-dock 壳态持久化契约**(RBT-02):`persist_layout`/`load_layout`——原子写
  (复用 foundation::persistence)、坏文件→默认布局+显式回退+warn、版本 v1+迁移桩、
  `LoadedLayout::{Restored, Fallback}`;旧 `load_layout(area,…)` 更名 `restore_layout`;
  workspace.rs 载明宿主契约(壳态落盘必须经本契约)。此前布局落盘完全由宿主自理
- **门禁四件**(GATE-01/04/08/09 + docs/12):TC-GATE-PANIC-01 全 workspace 生产段
  零 `unwrap/expect/panic!/unreachable` 静态门禁(反例单测齐备);GATE-04 硬编码色
  门禁(`crates/*/src` 除 tokens/theme 零颜色字面量,反例单测);CI 增 cargo tree
  许可红线(出现 `vb_*`/`cutforge-*` 节点即红)、CHANGELOG 门禁(源码变更必须伴随
  CHANGELOG 修改)、独立 cargo-deny job(deny.toml,allow 清单按全 lock 树实测预演);
  docs/12 载明三条纪律(宣称即证据/门禁反面测试/扫描面显式化)与门禁清单

### Changed
- **Timeline/Clip 加载即校验**(RBT-07):反序列化路径统一守卫——speed 有限且 >0、
  in≤out,坏数据结构化拒收(`VideoError::InvalidClip`),修复 speed=0/负/NaN/inf 可经
  `.sable` 静默注入;`set_speed(±inf)` 由静默 Ok 改为 Err;新增 `Clip::validate`/
  `Timeline::validate` 公开校验入口(两入口同一谓词)
- **Timeline 不变量 release 生效**(RBT-08):全部结构修改收尾经 `finish_mutation`
  校验(非法操作显式 Err),`debug_assert!(is_valid)` 降为成功路径的 debug 额外校验;
  闭合波纹删除下溢与切割起点加法回绕的 release 静默回绕
- 文档诚实化(DOC-01/02/03/04):docs/00/03/04/05/06、release-checklist 中把已实现的
  主题/令牌/动画/命中测试/选中框/钢笔/图层面板/检查器/基准等从"未开始/占位"订正为
  已实现(附模块路径);黄金基线场景数 5→6 订正
- **令牌骨架四件**(TOK-08/05/01/04,第 2 组):`docs/design/sable-tokens.json` 真相源
  (W3C 格式,color/spacing/radius/elevation/state-layer/motion 六表)+ `gate_tokens_sync`
  门禁(代码↔JSON 逐值对拍,双侧反面测试);中性色阶 surface_0..4 重排 + 文字 6 档
  (primary/strong/secondary/tertiary/disabled/placeholder,深浅独立调校,存量名全保留);
  elevation 死代码接活(`theme::shadow/shadow_quads/elevated`,L2→NumberField、L3→NeonCard
  辉光收敛至令牌,消费门禁防再死);`InteractState` + `state_layer`(alpha 叠加,修复
  浅色 hover 钳 1.0 失效,全部组件 hover/press/selected 切换);动效令牌(时长四档 +
  SpringPreset 三档,anim/spring 重指向令牌)
- **排版系统 + WCAG 对比度门禁**(TOK-02/03,第 2 组):字号 7 档(display/title/
  body-strong/body/label/caption/mono,字号/行高/字重三值)进令牌与 JSON typography 表;
  字体随包 assets/fonts/(Inter 400/500/600 + JetBrains Mono,OFL 许可登记 NOTICE.md,
  `fonts::install(cx)` 注册,fallback 链 UI→系统 CJK);NumberField 数值接 mono 族;
  WCAG 相对亮度/对比度纯函数 + `gate_contrast` 门禁(两主题 × 文字/功能色/焦点环/
  选中底全矩阵 ≥4.5:1/3:1,含注入反面)——**13 处不达标色值当场修正**(深色 tertiary/
  disabled/placeholder/danger/success,浅色 tertiary/disabled/placeholder/accent/
  danger/warning;浅色 accent #4F9FFF→#2264C8,深色 accent 未动;JSON 与六档强弱序
  同步),新增 `ColorTokens.info` 与 WCAG 公开函数
- **注入语义与禁用规则**(TOK-06/07,第 2 组):`inject_with(…, canvas: Option<CanvasTheme>)`
  支持第三方定制画布语义色(None=旧 inject 逐位兼容,`inject` 内部委托);禁用态统一规则
  "仅前景降级、容器不变"——`interact::disabled_foreground` 纯函数 + NumberField `.disabled()`
  (容器四通道与静止态逐位同源)+ effect_stack mini_button 禁用分支修正(旧:容器整体褪色)
- **基础控件层批 1**(CMP-01/02/04/11 + COUP-08,第 3 组,`sable-widgets::controls`,
  feature `controls` 默认入 full):`Button`/`IconButton`(四变体×三尺寸,Entity 与
  RenderOnce 内联双形态单一规格源,press 0.94 弹簧,focus ring,禁用容器逐位不变)、
  `TextField`(光标/选区/词边界/IME 组合区间一等状态)、`Select`(受控协议+键盘状态机+
  L3 下拉翻边)、`Choice` 三形态(Checkbox/Switch/Radio,自绘+键盘)、`Tabs`/`PanelTabs`
  (accent 2px 下划线 200ms 滑动)、`Tooltip`+`key_badge_text`(「名称 (快捷键)」文案单源,
  400ms 延迟+边界避让+L4 材质)、`ScrollArea`(细滚动条三态+惯性复用 ScrollPhysics)——
  TC-CMP-BTN-01/TF-01/SEL-01/CHOICE-01/TABS-01/TIP-01/02/SCROLL-01 全部落地;
  CMP-11 四处私有按钮 helper 收口归零(TC-GATE-DUP-01 静态门禁防复发);
  删除 gpui-component 死依赖(COUP-08);story 增"基础按钮"分组
- **可访问性主体**(A11Y-01/02/03/05/06/04/08,第 4 组):`interact::focus_ring` 单点 +
  全部可交互组件入 Tab 序(track_focus,ScrollArea/TimelineView/GradientEditor/ColorWheel/
  两图层面板本批补),列表 ↑↓+Enter 键盘导航、时间轴 ←→ 步进 seek、渐变条/色轮键盘操作;
  语义接口层 `Semantic`/`SemanticRole`(16 角色)+ 全部 pub 组件 `.label/.role` 槽 +
  `attach_semantics` 唯一挂接点——**gpui 0.2.2 经源码级核实无语义树 API,读屏如实录为
  TD-01 升级项**,docs/a11y-notes 全文按实况改写(键盘可达性现在可走查);
  命中区全面 ≥24px(10px 眼睛/锁/色标、12px 箭头、20/22px 图标/紧凑钮,视觉不变热区扩容);
  reduced-motion 全仓帧泵入口审计 + `gate_a11y_reduced_motion` 静态门禁;
  三态(hover/press/focus)全组件覆盖;`keymap` 绑定层(ActionSpec/KeymapRegistry/
  chord_display 单源复用 tooltip/速查表生成)+ `input_method::InputMethodAdapter`
  (TextField/NumberField IME 全部事件经 Adapter 单点路由,UTF-16 换算收敛,
  NumberField 无组合态的真实边界如实标注;IME 真机走查 10 场景入库待真机)
- **浮层与状态件**(CMP-03/05 + ANI-01 #4/#5/#13,第 5 组):`ToastHost`(右下堆叠,
  TTL 2.5/5/10s 分级,滑入滑出 120ms,错误可复制经宿主剪贴板钩子)、`DialogHost`
  (标题/内容/底部按钮右对齐,Esc=取消 Enter=确认,焦点 trap+归还打开者,8px+fade 进出场)、
  `CommandPalette`(KeymapRegistry 单源数据,模糊子序列匹配+别名槽(拼音容错经宿主别名),
  最近执行置顶,↑↓/Enter/Esc,下滑 120ms+背板 80ms)、`EmptyState`(图标+display 标题+
  引导+主行动)、`Spinner`/`Progress`(1.2s 扫描 pill;线性进度+完成对勾 pop 120ms)、
  `Skeleton`(行/块/卡三预设,1.2s 灰阶呼吸)、`ErrorBar`(danger 左描边+可展开详情+
  重试+错误码徽章)——全部 reduced_motion 直通、语义槽接入;ANI-01 #4/#5/#13 随组件接线
- **性能专项**(PERF-01/02/03/04/05/06/07/11,第 6 组):
  NumberField 展示文本缓存(值未变帧零格式化零分配,PERF-01);InspectorPanel 转 Entity
  + NumberField 实体按位池化复用(rebind 重定向绑定,焦点/编辑态保留——CMP-06/PERF-02,
  **公开 API 变化**:`InspectorPanel { sections }` 内联构造 → `InspectorPanel::new()` +
  `set_sections(sections, cx)`,vector_editor 宿主已同步);effect_stack 标签 Cow 化
  (静态文案零分配,PERF-03);图层面板 selection 存 Rc(渲染帧克隆零分配,PERF-04);
  **效果离屏缓存**(PERF-05):`RenderOpts::effects_cache` 新字段(默认 None 逐位不变,
  gpui_element 实体默认启用)——内容指纹(路径逐段坐标位/Paint 全字段/文本/图像)+ 效果栈
  + 变换/透明度/窗口几何为键,静止场景命中零光栅化,ShadowCache 增 hits/misses 计量;
  TimelineView 可视窗口裁剪 + 素材名/片段标签双缓存(值未变零分配,PERF-06);
  `FrameCache<V>` 有界 LRU 帧缓存进 crate(story 三卡迁入,PERF-07a)+ 色轮 36 扇形
  几何按 bounds 记忆化(PERF-07b 半,纹理光栅登记 docs/12 归属);RGBA→RenderImage 桥
  收口 `sable_canvas::gpui_element::rgba_to_render_image/premultiplied_rgba_to_render_image`
  单点 + TC-GATE-DUP-02 静态门禁(PERF-11,video_editor/story 已迁移,image 直接依赖移除);
  PERF-08 预算测试(本地 #[ignore] 口径)与 PERF-09/10 处置登记 docs/12 §3.5
- **内核鲁棒性与解耦**(第 8 组):场景遍历 `walk`/`collect_subtree`/`subtree_eq`
  改**显式栈迭代**(RBT-04,十万级深链不栈溢出,底→顶序不变);SVG 导出深度帽
  512(`ExportReport::depth_exceeded_nodes` 计数可观测,RBT-03);畸形 `.sable`
  字节流 proptest 512 例零 panic(RBT-12);命中测试几何不变量 proptest
  (内部⟺命中/顶优先/锁定跳过,GATE-07);`reduced_motion` 改 **thread_local**
  (RBT-09/COUP-10:并行测试竞态根除,**CI 解除 RUST_TEST_THREADS=1**);
  `CancelToken`/`Deadline` 协作式取消原语(RBT-10)+ `MemoryBudget` 分域
  FIFO 驱逐预算器(RBT-13)+ render_scene/svg/project `tracing` span(RBT-11);
  行数天花板门禁(2800 行,禁恶化,GATE-02)+ 依赖金字塔 Cargo.toml 真实键
  解析断言(COUP-01);**公共 API 快照门禁**(COUP-02:docs/api-snapshot/sable.txt
  + CI nightly cargo-public-api diff job,breaking 变更必须同步快照)

### Fixed
- **生产 panic 面清零**(RBT-05):foundation scene/project、paint effects 共 7 处
  `expect` 全部清偿为结构化错误路径(`ok_or(CoreError::…)`/`try_into()`/insert 返回值
  复用),另摸底清偿 widgets anim/gesture 2 处;合法输入行为不变
- paint 关闭 cpu feature 时的 effects.rs 未用导入告警(`--no-default-features` 全绿)

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
