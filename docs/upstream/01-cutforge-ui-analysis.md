# 01 · CutForge 上游 UI 分析(内核能力 → 桌面壳需求 → Sable 接入)

> 分析基线:2026-10-02,`D:\Github\cutforge` 工作区快照。
> 结论先行:**CutForge 是"一个内核,多个壳"的视频剪辑软件,当前唯一真实 UI 是 Web 壳(apps/web,无构建 ESM);桌面壳 apps/desktop 仅为占位 README,因 GPUI 0.2.2 生态不成熟被 ADR 降级为观察项——这正是 Sable 要补的位置。**

---

## 1. CutForge 是什么(定位与证据)

- **定位**:Rust 视频编辑器**内核**,"可独立起步,也为 AI 而生"。新建空工程 → 导入素材 → 多轨编辑 → 导出全程不依赖任何管线;同时直接读写 CutFlow(Python 视频管线)的工程文件;内置 MCP server(72 工具)与脚本宿主。见 `README.md:10-22`。
- **架构口号**:"一个内核,多个壳——时间线模型、命令与撤销、操作日志(OpLog)、渲染调度只实现一次,Web 壳与桌面壳都是薄壳"(README.md:19);"文件是真相源"(README.md:20)。
- **里程碑状态**:M0–M4 已完成,册一(A1 内核重构)~册四(A4 核心工具)完成,路线到 M5 多端壳 / M6 渲染后端 / M7 开源发布(README.md:36)。
- **许可(重要)**:混合授权——原创部分 **ARL-1.0**(弱传染:核心文件修改须回传;插件、壳、商业应用可闭源,README.md:185-193);派生自 OpenCut 的部分遵守 MIT(README.md:3-8)。核心文件清单在 `CORE-FILES`(36 个文件:core/io/schema/render/mcp/cli/script/wasm 的主体 + 7 份 schemas + capability-matrix.json)。
- **与 OpenCut 关系**:领域模型与工程结构参考 OpenCut(MIT)/opencut-classic(已归档),非分支(README.md:3-8)。

## 2. Crate 地图(8 crates + 2 apps)

| crate | 职责(证据:各 `src/lib.rs` 头注释) | 关键文件 |
|---|---|---|
| `cutforge-schema` | 契约层:五份 JSON Schema 编译期嵌入 + v1→v2 迁移 + 双端对拍 | `lib.rs:5-17`(SCHEMA_SOURCES:project/wordline/cutlist/notes/oplog) |
| `cutforge-core` | 领域模型/命令通道/撤销栈/OpLog/三路合并/锚点/关键帧;**不碰文件系统、不调 ffmpeg、可在 wasm32 构建** | `lib.rs:4-8` |
| `cutforge-io` | Workspace:八步落盘状态机(open→锁→apply→oplog→rev→解锁),`atomic.rs` 唯一写入点,锁/备份/ffprobe 探测/watcher | `lib.rs:3-17` |
| `cutforge-render` | ffmpeg 渲染后端:RenderPlan 七步(probe→segment→compose→overlay→mix→subtitle→encode)+ 内容寻址缓存 + `render_frame` 单帧 | `lib.rs:3-8` |
| `cutforge-mcp` | MCP 层:单注册表双通道(stdio + 内嵌 HTTP 127.0.0.1+token),/rpc /media /events(SSE) /assets /ui-fields /catalogs | `lib.rs:4-19` |
| `cutforge-cli` | serve/new/查询/应用/撤销重做/cache/doctor/门禁判定器(check-shell-purity 等) | `lib.rs:4-19` |
| `cutforge-script` | 脚本宿主与沙箱:批式 JSON 步骤,只能经 ToolDispatch 调工具,越界能力结构上不存在 | `lib.rs:3-16` |
| `cutforge-wasm` | wasm 绑定:壳只做展示,所有投影在内核算好以 JSON 交给 JS;`cross_shell_equivalence` 保证原生/wasm 投影逐语义相等 | `lib.rs:3-8` |
| `apps/web` | **现有 UI**:四层无构建 ESM(core/render/panels/ui),六 store + 只读投影 + keyed 增量渲染 | README.md:44-51 |
| `apps/desktop` | **仅占位**:GPUI 桌面壳降级为观察项,待 GPUI 0.3+ 复评(见 §4) | `apps/desktop/README.md` |

注:docs/adr/ 目录实际只落盘了 0001–0020;`apps/desktop/README.md` 引用的 ADR-0039 与 capability-matrix 引用的 ADR-0021+ 未在 `docs/adr/` 找到对应文件(引用与文件存在漂移,以 README 正文口径为准)。

## 3. 内核能力清单(UI 相关)

### 3.1 时间轴数据模型(cutforge-core/src/model.rs,785 行)

- 顶层 `Project`(model.rs:408):`version/schemaVersion/slug/fps/canvas/backends/notes/tracks/bgm/outputs/markers/subtitle/joinCrossfadeMs/font/effects`(与 `schemas/project.schema.json` properties 一致)。
- `Track`(model.rs:375)/`TrackKind`(model.rs:14)/`Role`(model.rs:37):视频/覆盖/音频分轨,轨道带 name/locked/mute/solo/hidden/heightPx/color/eq/dyn 九字段(`schemas/ui-fields.json` trackEditable 组)。
- `Clip`(model.rs:139):startMs/durationMs/sourceInMs/src/role/volume/speed/speedCurve/reverse/scale/opacity/rotation/crop/flip/transition/motion/fx/textStyle/text/huazi/freezeMs/grade/keyframes/compound…(字段集由 `check-ui-fields` 门禁保证"壳可编辑 ⊆ 内核 ClipPatch 可写")。
- 辅助结构:`Canvas`(model.rs:63,64–7680 偶数约束)、`Bgm`(model.rs:70,带 ducking)、`Marker`(:119)、`Subtitle`(:126)、`Position`(:238)、`SpeedPoint`(:247)、`Crop`(:254)、`Reframe`(:265)、`Motion`(:271)、`Transition`(:291)、`FxEntry/FxSpec`(:307/:318)、`Overlay`(:328)、`Fade`(:350)、`PunchIn`(:359)。
- 关键帧:`keyframes.rs`(782 行)独立模块;复合片段 `compound_ir.rs`(嵌套子时间线,深度≤两级)、调色 `grade_ir.rs`、文本 `text_style.rs`、OTIO/EDL/剪映互操作 `interop.rs`。

### 3.2 命令与撤销(cutforge-core/src/command.rs:205-251)

`Command` 枚举 18 个变体,全部单 Op 原子、可撤销、可审计:
`ClipUpdate / ClipSplit / ClipDelete / ClipMove / ClipInsert / ClipMerge / TrackAdd / BgmSet / BgmClear / ClipTrim(trim/roll/slip/slide 四模式,:228)/ ClipSplitAll / TrackUpdate / ClipGapDelete / ClipsInsert / ClipsPatch / CompoundCreate / CompoundUnbind / TrackSplitAt`。
引擎面:`Engine::query`(纯投影可并发)与 `Engine::apply`(唯一写入口)严格分离(core/lib.rs:6-8);OpLog 回放等价有三路合并 12,000 组属性测试背书(README.md:80)。

### 3.3 渲染与预览(cutforge-render)

- 整片:RenderPlan 七步分解,每步 = steps.rs 纯函数(生成 ffmpeg 参数)+ 执行器,中间产物全量内容寻址缓存(render/lib.rs:3-7)。
- **单帧精确预览**:`frame.rs` 的 `render_frame` 对指定时间点出一帧 PNG/JPEG 合成画面(含转场/叠加/字幕烧录);缓存键 = 工作区指纹 + atMs(**100ms 量化**)+ 画幅 + 渲染版本 + ASS 字节哈希(frame.rs:13-16)。
- 能力矩阵 42 项对剪映逐项判定(`docs/capability-matrix.md`,唯一真相源 capability-matrix.json):转场库 58、fx 注册表 11(combo≤3)、motion 19 项真实渲染、调色一级校色+曲线+LUT、示波器 scope_data(JSON 数据后端,壳 canvas 绘制登记为 FE 活)、轨道 EQ/动态、响度、渲染队列、复合片段、调整层、多机位、场景检测、OTIO/EDL。
- 媒体面:`media_peaks`(波形数据)/`media_thumbnail`(缩略图)/`media_proxy`(代理画质)工具(README.md:81)。

### 3.4 数据通道与 UI 契约(桌面壳直接可复用的三件套)

1. **`schemas/ui-fields.json`(v9)**:检查器可编辑字段的单一真相源,分组为 editable(基础/画面/音频/变速/文本/转场/动效/特效/调色 九组)+ trackEditable(轨道 九字段)+ readonly(position/keyframes/compound);服务端经 `GET /ui-fields` 下发,壳不读本文件(文件 _doc 字段)。**这是属性检查器"字段组 → 控件"的直接数据驱动源。**
2. **`GET /catalogs`**:转场全量目录(tr.* 58)、动效目录(motion.* 19)、特效、花字目录(huazi.*)——库类面板的数据源。
3. **`/rpc` + `/events`(SSE)**:所有写经命令通道(带 OpLog/rev/冲突检测),改动经 SSE 实时推送;查询类只读打开不持排他锁(README.md:30-31)。

## 4. UI 现状

### 4.1 Web 壳(apps/web)——功能上已经是"剪映级"三栏 NLE

- 演进史:`docs/UI-PLAN-v0.6.md` 记录了从"IR 原始字段表单"(调试面板)重写为剪映/达芬奇式三栏 NLE 的决策(左媒体池/中预览/右属性/底部多轨时间线/工具条)。
- 结构:core(14 文件:store/projector/commands/api/event-bus…)/render/panels(**29 个面板**)/ui(**19 个通用件**)无构建 ESM。
- panels 清单(= 桌面壳的功能需求清单,证据 `apps/web/js/panels/`):media-panel/media-card、preview、inspector/insp-groups、**curve/kf-curve/kf-editor/kf-row/kf-watch(关键帧曲线编辑器)**、grade/grade-wheel/scopes/compare(调色轮/示波器/分屏)、mixer、subtitles/textool、transitions/fxlib、bgm、history(历史面板)、diff/conflicts/notes、export/queue、multicam/compound/scenetool、wizard。
- ui 清单:menu/dialog/toast/tooltip/banner/help-panel/keymap/keymap-registry/shortcuts/markers/onboarding/perf/perf-panel/prefs/settings-panel/view-ops/dom/wire-wave3。
- 关键交互规格:精确拖拽(ghost 跟手 ≤1 帧、Esc 取消零 Op、拖拽 P95 60.2fps)、trim 四件套手势(Alt=slip/Ctrl=slide/Shift+边缘=roll,一次手势恰一 Op)、45 条可重绑定快捷键(冲突检测+「?」帮助面板)、Shift+D 性能面板、时间线 1k clips 虚拟化(README.md:56-61)。
- 时间线实现:canvas 重绘层 + DOM 交互层混合(ADR-0012,docs/adr/0012);主题为深色达文西灰阶单强调色,色值唯一定义点 tokens.css(check-shell-purity R5,README.md:107)。
- 纯度纪律:**壳不持有真相、不算时间线语义**(M5-5 门禁;cutforge-wasm/lib.rs:5-7)——投影全部由内核算好。

### 4.2 桌面壳(apps/desktop)——占位,恢复条件明确

`apps/desktop/README.md` 全文要点:
- 状态:**降级为观察项**(引 ADR-0039,计划书 7.6 风险 R2 预案);原因是上游 OpenCut 桌面壳自述 "just a window that opens",GPUI 0.2.2 生态尚在演进且对 Linux/WSL 有平台约束。
- 预定形态:wasm 内核 + Web 壳先交付多端;桌面壳待 **GPUI 0.3+ 或上游 crates 落地后复评**,"以 `cutforge-wasm` 同源投影接入 GPUI"。
- **恢复为阻断门禁(M5-4)的条件:GPUI 稳定版 + apps/desktop 二进制 + Windows 冒烟(开窗 + 加载工程 + 渲染时间线)。**

## 5. 未来 apps/desktop 的 UI 需求 → Sable 能力映射

按"Sable 只做前端层,内核/数据语义仍归 cutforge"的分工:

| CutForge 桌面壳需求(源自 Web 壳实证) | 需要的 Sable 能力 | 归属 crate |
|---|---|---|
| 三栏 + 底部时间线的 NLE 工作台;面板坞/Tab 分组;布局持久化(cutforge 已有 `migrate_layout` 工具与 workspace 布局概念) | Dock 布局、面板 Tab、布局序列化 | **sable-dock**(gpui-component DockArea) |
| 多轨时间轴:轨头(可见/锁/静音/独奏/隐藏/高度/颜色)、clip 块拖拽/trim 四件套/分割/波纹删、标尺+播放头、磁吸、1k clip 虚拟化 | 时间轴模型(轨道/clip/关键帧/吸附/播放时钟)+ 时间轴视图组件 | **sable-video** + **sable-widgets**(timeline 组件) |
| 检查器:ui-fields.json 驱动的九组字段(数值 scrubby/表达式/色板/下拉/曲线) | NumberField/ColorField/SectionHeader/属性检查器 | **sable-widgets** |
| 关键帧曲线/变速曲线编辑器(kf-editor/curve) | 曲线编辑器组件 + kurbo 曲线求值 | sable-widgets + **sable-paint**(kurbo) |
| 预览播放器:Web 用 video 标签段级预览 + render_frame 精确单帧;桌面需把帧(PNG/JPEG/解码帧)上屏、播放时钟驱动 | 画布视图/视口变换/纹理上屏;播放时钟在 sable-video | **sable-canvas** + sable-video(render/解码仍走 cutforge-render) |
| 示波器/波形/缩略图绘制(scope_data/media_peaks 已给 JSON 数据) | 自绘图形原语(vello)| **sable-paint** + widgets 容器 |
| 撤销/重做 UI(历史面板回跳 N 笔)、命令面板、45 条可重绑定快捷键、菜单/对话框/toast | Command/History(undo 合并会话)、命令面板、keymap | **sable-foundation**(Command/History)+ sable-dock |
| 深色达文西风、色值单一定义点、AA 对比度 | 设计 token 体系 | sable-widgets(tokens.rs) |
| "壳不持真相":一切编辑经命令通道产生 Op | Sable 的 Command 系统只管 **UI 层**命令(视图态),文档语义命令直通 cutforge Engine | sable-foundation 与 cutforge-core 的边界(见 §6) |

## 6. 接入顺序建议

**原则:桌面壳 = Sable 前端 + cutforge 内核,经同一条命令通道说话;先远后近(先 HTTP 后进程内),先只读后可写。**

- **P0 骨架(sable-dock 可用性验证)**:GPUI 窗口 + DockArea 三栏布局;HTTP 客户端连 `cutforge-cli serve`(Bearer token);`forge_open`/`/rpc` 查询投影 → 只读时间轴渲染(轨道行 + clip 块 + 播放头)。对应 cutforge 恢复门禁的三步冒烟:开窗 ✓ / 加载工程 ✓ / 渲染时间线 ✓。
- **P1 时间轴可写(sable-video 对齐)**:拖拽/trim/分割映射为 `ClipMove/ClipTrim/ClipSplit` 等命令经 `/rpc` 提交,SSE 收 Op 回流更新投影;磁吸/吸附由 sable-video 提供,提交前本地预演、被拒(InvariantViolation)即回弹——与 Web 壳 ghost 手势同规格。
- **P2 检查器与库面板(sable-widgets 组件验收场)**:`GET /ui-fields` 动态生成检查器分组;NumField scrubby+表达式、ColorField 对接 grade 组;`GET /catalogs` 驱动转场/特效/动效库面板。
- **P3 预览闭环**:「精确预览」走 `render_frame`(PNG→GPUI 纹理,注意 100ms 量化=预览最高 ~10fps,适合单帧);连续播放预览按段级 source 预览(Web 壳同款)或后续接解码帧流(M6 渲染后端阶段再评估)。
- **P4 专业面板**:波形(media_peaks)/缩略图(media_thumbnail)/示波器(scope_data)绘制;历史面板接 OpLog 查询;差异/冲突/标注面板。
- **P5 进程内化(可选)**:桌面壳从 HTTP 改为进程内链接 cutforge-core/io(或内嵌 cutforge-mcp dispatch),消除序列化延迟;此步才考虑 workspace 锁的 UI 呈现(只读打开 vs 排他)。

## 7. 风险清单

1. **许可交叉(最高优先)**:CutForge 原创部分为 ARL-1.0,`CORE-FILES` 所列 36 个核心文件**修改须回传开源**。Sable(MIT OR Apache-2.0)若复制/改写这些文件的代码进入 sable-* crate,会被传染。**纪律:Sable 库侧零复制,只经 /rpc、Command JSON 契约、schema 消费 CutForge;apps/desktop 作为"壳"按 ARL-1.0 第 1.3 条属可闭源侧,但建议独立目录/仓库,不混入 sable-* 源码。** 证据:`README.md:185-193`、`CORE-FILES`。
2. **壳语义漂移**:Web 壳有 check-shell-purity 门禁看住;桌面壳是第二个壳,若在 UI 侧手写时间线语义(吸附计算、重叠判定)就会产生第二真相。必须复用内核投影+命令,UI 侧只留"手势→命令"翻译。证据:`cutforge-wasm/lib.rs:5-8`(壳不持有真相)、README.md:107(R1 持久化禁令)。
3. **GPUI 成熟度**:CutForge 桌面壳正是因 GPUI 0.2.2 不成熟而降级(apps/desktop/README.md:5-8);Sable 押注同一技术栈(gpui 0.2.2 + gpui-component 0.7.0),该风险由 Sable 主动承接,CutForge 的恢复条件(GPUI 稳定版)即 Sable 的交付门槛。
4. **预览帧率**:render_frame 以 100ms 量化(frame.rs:15),连续高帧率预览不能依赖它;桌面端播放预览需要段级原始素材预览(画质代理,无转场/特效)或未来解码帧流——Web 壳同样如此(README.md:92),桌面端并不更差,但预期要管理。
5. **双命令体系映射**:CutForge 45 条可重绑定快捷键 + 命令注册表(README.md:58)与 sable-dock(gpui-component)的 keymap/动作体系是两套表;接入时要决定 keymap 单源放哪(建议:cutforge 命令 ID 为单源,Sable keymap 只做绑定层)。
6. **ffmpeg 外部依赖**:渲染/探测全部依赖 ffmpeg/ffprobe 子进程(cutforge-render/lib.rs:4-5);桌面壳分发需捆绑或检测 ffmpeg——`cutforge-cli doctor` 已给出就绪自检范式(cutforge-cli/src/doctor.rs),桌面壳应复用而非重造。
