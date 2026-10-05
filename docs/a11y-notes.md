# 无障碍(Accessibility)对接 notes — V3.0 T5(2026-10-04 修订)

> 目标:对照 egui(AccessKit 内置)/Qt(原生 UA)/Flutter(Semantics 树)的无障碍形态,
> 摸清 gpui 0.2.2 在 Windows 上的暴露面,给 Sable 组件补语义接口。
> 维护:每接入一个组件,更新 §3 清单。
> **2026-10-04 修订**:§1 的旧结论("gpui 0.2.2 自带无障碍骨架/AccessKit 集成")
> 经源码核实**不成立**(当年 V4.0 R4 复盘只纠正了"组件侧 ✅"虚标,未复核上游
> 记载本身),本版按实况改写。

## 1. 现状结论(2026-10-04,源码级核实)

- **gpui 0.2.2(crates.io 快照)没有任何语义树 API**:
  - crate 内无 `_accessibility` 模块(旧版本文件曾记载"自带 AccessKit 骨架",
    系与 Zed 主线 git 版混淆——crates.io 0.2.2 无此物);
  - `Cargo.toml` 无 accesskit 依赖;
  - `Interactivity`(elements/div.rs)无 role/label/accessible 字段与方法,
    全仓 grep `accessible|AccessNode|semantic` 仅命中无关词;
  - 平台层(Windows)无 UIA/MSAA 节点上报路径。
- **gpui 0.2.2 有的**:焦点体系完整——`FocusHandle`、`track_focus`、
  `tab_stop(bool)`、`tab_index(isize)`、`tab_group()`、`Window::focus_next/
  focus_prev/focus(&handle)`;hover/active/focus 样式 refinement;keydown/
  action 分发。**键盘可达性可以真做,读屏语义做不了**。
- **Sable 组件侧(第 4 组 A11Y 批次已落)**:
  - 库侧语义接口层(`interact::SemanticRole`/`Semantic`/`attach_semantics`
    单点 + 全部 pub 组件 `.label(...)` 槽)——**存态**,读屏器暂不可消费;
  - 焦点体系真接线(track_focus + tab_stop + interact::focus_ring 焦点环、
    列表键盘导航 `list_nav`、时间轴 `timeline_seek_step`、色标
    `gradient_stop_nav`、色轮 `color_step`、F6 助手 `focus_region`);
  - 门禁:TC-A11Y-FOCUS-01 / TC-A11Y-LABEL-01 / TC-A11Y-RM-01(tests/
    gate_a11y_*.rs)。

## 2. 对接点清单(按"gpui 0.2.2 实况"修订)

| 语义 | gpui 0.2.2 实况 | Sable 落点 |
|---|---|---|
| 名称(label) | **无语义树,不可挂** | 库侧 `.label(...)` 存态(`Semantic`);TD-01(gpui 升级)出语义树后在 `interact::attach_semantics` 单点落树 |
| 角色(role) | **无** | 库侧 `SemanticRole` 枚举(Button/TextField/ListItem/Slider/Tab/…),`as_str()` 稳定标识待映射 |
| 焦点 | `FocusHandle` + `track_focus`/`tab_stop` ✓ | 全部可交互 Entity 组件已接(见 §3);Tab 序即焦点环游序 |
| 焦点环 | focus 样式 refinement(自绘) | `interact::focus_ring` 两层环(accent 1.5px + 隔离环),两主题断言 TC-A11Y-RING-01 |
| 值变化播报 | 无(依赖语义树) | 待 TD-01;组件修改已 `cx.notify()`(渲染面就绪) |
| 层级(pane/group) | 无 | 待 TD-01;dock 面板 title 已有(宿主侧) |
| 读屏(UA/UIA) | **平台层无上报路径** | **真机走查在 TD-01 前不可行**——不虚标;下表"读屏实测"列保持 ☐ |

## 3. 组件接入进度(第 4 组 A11Y 批次,2026-10-04)

| 组件 | label 接口 | role(默认) | 焦点/Tab | 键盘导航 | 读屏实测 |
|---|---|---|---|---|---|
| Button / IconButton | ✅ `.label()`(缺省 = 可见文本/tooltip) | Button / IconButton | ✅ track_focus | Enter 触发(gpui click) | ☐ TD-01 |
| Choice(Checkbox/Switch/Radio) | ✅ 可见标签即可访问名 | Checkbox / Switch / Radio | ✅ | space/enter 切换 | ☐ |
| Select | ✅(缺省 = 选中项/占位符) | Button(触发钮) | ✅ | ↑↓/Home/End/Enter/Esc(关还焦点) | ☐ |
| Tabs / PanelTabs | ✅(页签名取可见文本) | Tab | ✅ + 焦点环 | ←/→ 换序 | ☐ |
| TextField | ✅(缺省 = 占位符) | TextField(编辑态) | ✅ + 焦点环 | 编辑态全套;Esc 还原 | ☐ |
| NumberField | ✅ `.label()` | TextField | ✅ | ↑↓ 步进(Shift ×10)、Enter 提交 | ☐ |
| ScrollArea | ✅ | ScrollRegion | ✅(本批补) | 轴向方向键滚动、Home/End | ☐ |
| LayerPanel | ✅(缺省"图层面板") | List;行 = ListItem(图层名) | ✅ 列表容器 + 焦点环 | ↑↓ 移选择、Enter 翻可见(`list_nav`) | ☐ |
| LayerTreePanel | ✅(缺省"图层树") | List;行 = ListItem | ✅ 同上 | 同上(扁平行序导航) | ☐ |
| EffectStackPanel | ✅(缺省 = 标题) | List;行 = ListItem(效果名) | ⚠ RenderOnce 无跨帧句柄——`list_nav` 纯函数 + 宿主壳契约,实体接线 = M2 | 行内钮(Compact 热区 24) | ☐ |
| InspectorPanel / PropertyRow | ✅(容器:Group;名称 = 分组/行标签) | Group | (子控件承担:NumberField ✅) | — | ☐ |
| ColorWell | ✅(缺省"颜色") | ColorWell | ⚠ RenderOnce(取色经 ColorWheel ✅) | — | ☐ |
| ColorWheel | ✅(缺省"颜色轮") | ColorPicker | ✅ + 焦点环 | ←→ 色相、↑↓ 明度(Shift 细档,`color_step`) | ☐ |
| GradientEditor | ✅(缺省"渐变") | ColorPicker | ✅ 色标条 + 焦点环 | ←→/Home/End 选色标、Delete 删(`gradient_stop_nav`) | ☐ |
| TimelineView | ✅(缺省"时间轴") | Slider(播放头) | ✅ + 焦点环 | ←→ ±刻度档、Home/End(`timeline_seek_step`) | ☐ |
| NeonCard / CurvePreview | ✅ 槽(装饰件) | Decoration(读屏应跳过) | —(非交互) | — | ☐ |
| Dock 面板/工具栏 | 🟡 title 已有(宿主侧) | Pane | F6 循环 = 宿主壳(`interact::focus_region` 环游助手已备) | — | ☐ |

## 4. 真机验收(release-checklist T 系列;**TD-01 后才可执行**)

- 前置:gpui 升级出语义树(TD-01)→ `attach_semantics` 单点接通原生节点;
- NVDA/Narrator 逐组件遍历:名称/角色/值播报正确(走查表按 §3 逐行核对);
- 键盘全操作可达(Tab 顺序 + 方向键列表导航)——**此项现在即可人工走查**;
- CJK label 无乱码;
- IME 中文输入全链路 = §5 走查表(TC-A11Y-IME-01;TextField 组合态数据
  通路与 Adapter 单点已备,真机回归待执行)。

## 5. IME 真机走查表(A11Y-08 第 4 组,2026-10-04;DirectWrite 链路)

> 落点:`input_method::InputMethodAdapter` 单点(TextField/NumberField 文本
> 事件共用;UTF-16 ↔ 字节换算/组合分支只此一份)。平台通路 = gpui 0.2.2
> Windows DirectWrite/IMM32(registry 源码核实:组合态平台吞 KeyDown,
> `marked_text_range` 为平台判定输入)。
> **TC-A11Y-IME-01 真机口径:本表逐行人工走查通过后方可标 ✅;"数据通路
> 已实现/单测全绿"不构成真机通过,不得预标。** 前置环境:Windows 10/11
> 真机 + 微软拼音(全拼/双拼各一轮)+ 一款第三方输入法(搜狗)。

| # | 场景 | 操作 | 期望 | 状态 |
|---|---|---|---|---|
| 1 | TextField 组合显示 | 聚焦 TextField,拼音逐键(如 `nihao`) | 组合串实时原位替换,accent 下划线;组合中选区高亮让位 | ☐ 待真机 |
| 2 | TextField 候选窗 | 组合中弹候选窗 | 候选窗出现且定位到控件 bounds(字符级定位 = M2 已知边界);不遮输入行 | ☐ 待真机 |
| 3 | TextField 选字提交 | 空格/数字选字 | 最终文本替换组合区间,`marked_text_range` 归空,光标落最终文本之后,不丢字不乱码 | ☐ 待真机 |
| 4 | TextField 组合取消 | 组合中按 Esc | 组合串移除、光标回落组合起点;**第二次 Esc 才触发取消编辑还原**(`edit_key` 组合门控) | ☐ 待真机 |
| 5 | TextField 组合中 Enter | 组合中直接按 Enter | 不产生 `on_submit`(平台吞键 + 组件侧纵深防御);候选确认路径正常 | ☐ 待真机 |
| 6 | TextField UTF-16 边界 | 组合区前后混排 CJK + emoji(代理对)后继续组合/拖选 | UTF-16 ↔ 字节换算(input_method 单点)无撕裂、无乱码 | ☐ 待真机 |
| 7 | NumberField 组合输入 | 双击进编辑态,输入"厘米"等 CJK 文本 | 组合文本直插 buffer(**组合区间不追踪 = 真实边界**,M2 随编辑器重构),显示不乱码 | ☐ 待真机 |
| 8 | NumberField 提交兜底 | IME 提交"12厘米"后 Enter | parse 失败回落旧值,不崩溃、不产生命令;纯数字 IME 提交正常 | ☐ 待真机 |
| 9 | 焦点切换残留 | 组合中点击别处 / Tab 跳走 | 无组合残留渲染(TextField 失焦快照同步;NumberField 失焦自动提交) | ☐ 待真机 |
| 10 | 禁用/只读拒收 | 禁用/只读控件上触发 IME | 不进组合态、不接受平台替换(`ime_editable` 门控单点) | ☐ 待真机 |

**DirectWrite 链路核查点**(走查时同步记录):候选窗位置精度(控件级 vs
字符级)、组合中光标相位、全角符号经 IME 提交、TSF/IMM32 两种模式、
多显示器 DPI 下候选窗偏移。

## 6. 参考

- AccessKit:https://accesskit.dev/ (数据模型:Node/Role/Action)
- egui 的 AccessKit 集成(同语言参照,widgets→AccessKit Node 映射表)
- Windows UI Automation(最终消费端)
- 组件侧落点源码:`crates/sable-widgets/src/interact.rs`(语义层/焦点环/导航
  状态机单点)+ 各组件 `.label(...)`/`resolved_semantic()`
