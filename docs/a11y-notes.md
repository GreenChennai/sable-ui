# 无障碍(Accessibility)对接 notes — V3.0 T5

> 目标:对照 egui(AccessKit 内置)/Qt(原生 UA)/Flutter(Semantics 树)的无障碍形态,
> 摸清 gpui 0.2.2 在 Windows 上的暴露面,给 Sable 组件补语义接口。
> 维护:每接入一个组件,更新 §3 清单。

## 1. 现状结论

- **gpui 0.2.2 自带无障碍骨架**:crate 内含 `_accessibility` 模块与 AccessKit 集成
  (Zed 主线 2024 年合并的 AccessKit 支持随 0.2 快照发布);元素可通过
  `InteractiveElement` 系 API 暴露语义树节点。
- **Sable v2.0 缺口**:组件全部是"视觉优先",未挂任何 label/role/value 语义——
  读屏器(NVDA/Narrator)只能看到空节点。
- **风险(TD-04)**:后补成本指数增长;V3.0 至少把**接口面**铺好,读屏实测留真机 Sprint。

## 2. 对接点清单(gpui 0.2.2,源码核实待 T5.2 编译期确认)

| 语义 | gpui 入口 | Sable 落点 |
|---|---|---|
| 名称(label) | 元素语义树节点的 label 字段 | `NumberField/ColorWell/PropertyRow/按钮` 的 `.with_label(...)` builder(v4.1 起,与真机读屏同批) |
| 角色(role) | 语义节点 role(Button/TextField/…) | 组件类型映射:按钮→Button、NumberField 编辑态→TextField、LayerPanel 行→ListItem |
| 焦点 | `FocusHandle`(已用:editor root/NumberField 编辑态) | 保持;Tab 顺序由 focus 顺序决定 |
| 值变化播报 | 语义树更新(组件 notify 时自动) | SetFill/SetTransform 等 Command 执行后组件已 `cx.notify()` ✓ |
| 层级 | Dock/面板容器 → group 语义 | `sable-dock` 面板 title 已有,补 role=PaneInfo |

## 3. 组件接入进度

> **V4.0 复盘修正(2026-10-02)**:本表曾把下列 6 个组件的 label 接口标为
> "✅ V3.0";V4.0 review(R4)全仓 grep 实锤 `with_label`/accessible 语义
> 接口在 sable-widgets 中**根本不存在**,属虚标。全部改回遗留项,接口面
> 与读屏真机实测同批列 v4.1(见迭代计划 V4.0 §1 R4 / T5.3)。

| 组件 | label 接口 | role | 读屏实测 |
|---|---|---|---|
| NumberField | ☐ 遗留(v4.1 起) | TextField | ☐ 真机 |
| ColorWell / ColorWheel | ☐ 遗留(v4.1 起) | Button | ☐ |
| PropertyRow / InspectorPanel | ☐ 遗留(v4.1 起,透传子件) | Group | ☐ |
| LayerPanel / LayerTreePanel | ☐ 遗留(v4.1 起,ListItem 逐行) | List/ListItems | ☐ |
| TimelineView | ☐ 遗留(v4.1 起,播放头 Slider 语义) | Slider | ☐ |
| EffectStackPanel | ☐ 遗留(v4.1 起,逐效果行) | List/ListItems | ☐ |
| Dock 面板/工具栏 | 🟡 title 已有 | Pane/Button | ☐ |

## 4. 真机验收(release-checklist T 系列)

- NVDA/Narrator 逐组件遍历:名称/角色/值播报正确
- 键盘全操作可达(Tab 顺序 + 方向键列表导航)
- CJK label 无乱码

## 5. 参考

- AccessKit:https://accesskit.dev/ (数据模型:Node/Role/Action)
- egui 的 AccessKit 集成(同语言参照,widgets→AccessKit Node 映射表)
- Windows UI Automation(最终消费端)
