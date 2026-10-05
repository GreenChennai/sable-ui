//! 树形图层面板 LayerTreePanel(迭代计划 V2.0 T2,分册四 §1 的层级树形态)。
//!
//! 与 [`crate::layer_panel::LayerPanel`](拍平 roots 的 v0.1 版)同 feature
//! (`layer-panel`)同纪律,按场景树递归渲染:嵌套缩进(每层 12px,4px 网格)、
//! 组行展开/折叠箭头、可见性/锁眼睛列、行选择透传(Shift 加选语义与
//! LayerPanel 完全一致,复用其 [`crate::layer_panel::apply_select`])。
//!
//! # 重排(V2.0 T2 v1 简化,**真拖拽 = v2.1**)
//!
//! 每行四个按钮回调:提升一级(`←`)/ 降一级(`→`)/ 上移(`↑`)/ 下移
//! (`↓`),全部由应用层经 [`Reparent::capture`](sable_foundation::command)
//! 落成可撤销命令(AGENTS.md §3.1,文档修改与撤销由应用层负责)。本模块的
//! [`promote_target`] / [`demote_target`] / [`sibling_move_target`] 是
//! "当前场景 → Reparent 目标 (parent, index)" 的纯函数,应用层接线与单测
//! 共用同一套目标计算;目标为 `None`(已在边界)时按钮应保持空操作。
//!
//! 语义(outliner 惯例,Workflowy/Xcode 同款):
//! - 提升一级:挂到父节点的父节点,紧跟父节点之后;
//! - 降一级:挂到**前一个兄弟**的 children 尾部(无前一个兄弟 = 不可降);
//! - 上移/下移:同父兄弟列表内 index ± 1。
//!
//! # 视图态不入撤销栈(T2 验收明示)
//!
//! 展开/折叠状态存面板本地 `HashMap<NodeId, bool>`(absent = 展开),是
//! **纯视图态**:切换只改本面板的渲染下钻,不产生任何 Command、不进
//! [`History`](sable_foundation::command)(与 Qt QUndoStack 只收文档命令
//! 同理——折叠一棵树不改变文档数据)。测试
//! `view_state_toggle_never_touches_history` 锁定该契约。
//!
//! # FLIP 让位(A4,同 LayerPanel 的 mt 方案)
//!
//! 折叠/展开/重排引起行序变化时,[`FlipTracker`] 按 key(NodeId 经
//! `slotmap::Key::as_ffi` u64 化,重排序稳定)对**全部**扁平行(含滚出
//! 视口者)报告 y = index × 行高,重排后 160ms ease 让位;消费方式 = 行
//! `.mt()`(uniform_list 的 item 各自是布局根,行内 margin 只移动本行,
//! gpui 0.2.2 源码核实同 LayerPanel);减弱动态(A8)下偏移恒 0。
//!
//! # 回调约定(与 LayerPanel 同款)
//!
//! 全部 `Rc<dyn Fn(...)>`(可克隆进 uniform_list 的 'static 闭包),签名
//! 以 `&mut App` 收尾。折叠箭头是唯一**不走回调**的交互:视图态面板内部
//! 自治,见上文「视图态不入撤销栈」。
//!
//! # v1 边界(全部 doc 注明)
//! - 真拖拽(on_drag/拖放目标)= v2.1(T2 验收 2.2 的四按钮替代即本文件);
//! - 行内缩略图省略(宽度让给四个重排按钮;LayerPanel 已有占位块实现,
//!   树形态要补时按 LayerPanel 的 surface_3 色块回填即可);
//! - 双击改名 = M2(需文本输入,同 LayerPanel 边界);
//! - 眼睛/锁/箭头图标 = 文字字形/几何占位(Lucide 接入 = M2,同 LayerPanel)。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    App, ClickEvent, Context, ElementId, Entity, FocusHandle, Hsla, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div, px, uniform_list,
};
use sable_foundation::scene::{NodeId, Scene};
use slotmap::Key;

use crate::controls::button::{ButtonVariant, IconButton, icon_button_element};
use crate::flip::FlipTracker;
use crate::interact::{
    self, HoverState, ListNavIntent, PulseState, Semantic, SemanticRole, list_nav, semantic_slot,
};
use crate::layer_panel::layer_row_bg;
use crate::theme::theme;
use crate::tokens::{
    FONT_SIZE_BODY, FONT_SIZE_CAPTION, RadiusTokens, SpacingTokens, control_height, h_flex, v_flex,
};

/// 列表行的估行高基准(正文字号 12 的行高,与 LayerPanel 同值)。
const ROW_LINE_HEIGHT_PX: f32 = 16.0;
/// 行垂直内边距(16 + 2×6 = 28,与属性行/LayerPanel 行同高)。
const ROW_V_PADDING_PX: f32 = 6.0;
/// 每层缩进宽度(T2 任务书:每层 12px,4px 网格上的 MD 档)。
const INDENT_PX: f32 = SpacingTokens::MD;
/// 展开/折叠箭头列的方形占位边长(MD 档,4px 网格)。
const GLYPH_SIZE_PX: f32 = 12.0;
/// 眼睛/锁列的方形占位边长(LayerPanel 行内既有占位规格,原样沿用)。
const EYE_LOCK_SIZE_PX: f32 = 10.0;

/// 眼睛 toggle 回调:`(NodeId, &mut App)`(约定同 LayerPanel)。
pub type ToggleVisibleFn = Rc<dyn Fn(NodeId, &mut App)>;
/// 锁 toggle 回调:`(NodeId, &mut App)`。
pub type ToggleLockFn = Rc<dyn Fn(NodeId, &mut App)>;
/// 行选择回调:`(NodeId, shift 是否加选, &mut App)`(语义与 LayerPanel 的
/// [`crate::layer_panel::SelectFn`] 一致,应用层用同一个 `apply_select`)。
pub type SelectFn = Rc<dyn Fn(NodeId, bool, &mut App)>;
/// 上移一格回调:`(NodeId, &mut App)`。
pub type MoveUpFn = Rc<dyn Fn(NodeId, &mut App)>;
/// 下移一格回调:`(NodeId, &mut App)`。
pub type MoveDownFn = Rc<dyn Fn(NodeId, &mut App)>;
/// 提升一级回调:`(NodeId, &mut App)`。
pub type PromoteFn = Rc<dyn Fn(NodeId, &mut App)>;
/// 降一级回调:`(NodeId, &mut App)`。
pub type DemoteFn = Rc<dyn Fn(NodeId, &mut App)>;

/// 重排目标:`(新父节点, 目标下标)`,直接喂
/// [`Reparent::capture`](sable_foundation::command)(`None` 父 = roots;
/// `None` 下标 = 追加到尾)。目标不可得时相关纯函数返回 `Option::None`。
pub type ReparentTarget = (Option<NodeId>, Option<usize>);

/// 扁平化后的一行(T2 验收:depth 供嵌套缩进,has_children 供箭头)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeRow {
    /// 场景节点。
    pub id: NodeId,
    /// 嵌套深度(roots = 0,每层 +1)。
    pub depth: usize,
    /// 是否有子节点(有 = 渲染展开/折叠箭头)。
    pub has_children: bool,
}

/// 折叠状态切换(纯函数):absent(默认)= 展开,切换后在 true(折叠)/
/// false(展开)间翻转。**只动视图态表,不可能产生命令**(T2 契约,见
/// 模块 doc 与 `view_state_toggle_never_touches_history` 测试)。
pub fn toggle_collapsed(collapsed: &mut HashMap<NodeId, bool>, id: NodeId) {
    let entry = collapsed.entry(id).or_insert(false);
    *entry = !*entry;
}

/// 场景树 → 扁平行数组(T2 验收:递归下钻,只展开的组继续深入;roots 依
/// 渲染序,children 依数组序;depth 逐层 +1)。纯函数,确定性可单测。
pub fn flatten_tree(scene: &Scene, collapsed: &HashMap<NodeId, bool>) -> Vec<TreeRow> {
    let mut out = Vec::with_capacity(scene.len());
    for &root in &scene.roots {
        flatten_walk(scene, root, 0, collapsed, &mut out);
    }
    out
}

/// [`flatten_tree`] 的递归腿:先记本行,折叠则截断,否则依序下钻。
fn flatten_walk(
    scene: &Scene,
    id: NodeId,
    depth: usize,
    collapsed: &HashMap<NodeId, bool>,
    out: &mut Vec<TreeRow>,
) {
    let Some(node) = scene.node(id) else {
        return;
    };
    let has_children = !node.children.is_empty();
    out.push(TreeRow {
        id,
        depth,
        has_children,
    });
    // absent = 展开(默认全展开,Illustrator 图层面板同款)
    if collapsed.get(&id).copied().unwrap_or(false) {
        return;
    }
    for &child in &node.children {
        flatten_walk(scene, child, depth + 1, collapsed, out);
    }
}

/// 提升一级的目标:挂到父节点的父节点、紧跟父节点之后。根级节点不可再
/// 提升 → `None`(UI 侧按钮空操作)。
pub fn promote_target(scene: &Scene, id: NodeId) -> Option<ReparentTarget> {
    let (parent, _) = scene.position(id).ok()?;
    let p = parent?;
    let (grand, p_index) = scene.position(p).ok()?;
    Some((grand, Some(p_index + 1)))
}

/// 降一级的目标:挂到**前一个兄弟**的 children 尾部(outliner 的 Tab 语义)。
/// 首位(无前一个兄弟)→ `None`。
pub fn demote_target(scene: &Scene, id: NodeId) -> Option<ReparentTarget> {
    let (parent, index) = scene.position(id).ok()?;
    if index == 0 {
        return None;
    }
    let prev = match parent {
        Some(p) => scene.node(p)?.children[index - 1],
        None => scene.roots[index - 1],
    };
    Some((Some(prev), None))
}

/// 同父上/下移的目标(`delta = -1` 上移、`+1` 下移);已在边界 → `None`。
pub fn sibling_move_target(scene: &Scene, id: NodeId, delta: isize) -> Option<ReparentTarget> {
    let (parent, index) = scene.position(id).ok()?;
    let len = match parent {
        Some(p) => scene.node(p)?.children.len(),
        None => scene.roots.len(),
    };
    let next = index as isize + delta;
    if next < 0 || next >= len as isize {
        return None;
    }
    Some((parent, Some(next as usize)))
}

/// 树形图层面板(有状态 Entity):
/// `cx.new(|cx| LayerTreePanel::new(scene_entity).on_select(...).on_promote(...))`
pub struct LayerTreePanel {
    /// 场景数据源(只读;修改走应用层回调)
    scene: Entity<Scene>,
    /// 选中集本地副本(真相在应用层;这里仅作高亮显示与按钮目标)
    selection: std::rc::Rc<Vec<NodeId>>,
    /// 展开/折叠视图态(absent = 展开;**不入撤销栈**,见模块 doc)
    collapsed: HashMap<NodeId, bool>,
    on_toggle_visible: ToggleVisibleFn,
    on_toggle_lock: ToggleLockFn,
    on_select: SelectFn,
    on_move_up: MoveUpFn,
    on_move_down: MoveDownFn,
    on_promote: PromoteFn,
    on_demote: DemoteFn,
    /// A7:悬停进度(悬停互斥 → 面板级单一状态机)+ 当前悬停行
    hover: HoverState,
    hovered_node: Option<NodeId>,
    /// A4:行让位跟踪(Rc<RefCell> 以共享进 uniform_list 闭包)
    flip: Rc<RefCell<FlipTracker>>,
    /// A7:撤销脉冲表(节点 → 脉冲;查询即清理过期项,无驻留)
    pulses: Rc<RefCell<HashMap<NodeId, PulseState>>>,
    /// A11Y-01:列表键盘导航焦点(容器 track_focus,↑↓/Enter 导航)
    focus: Option<FocusHandle>,
    /// A11Y-06:按压中的行(按下高亮,松开清除)
    pressed_node: Option<NodeId>,
    /// A11Y-02 语义槽(可访问名/角色;缺省"图层树"/List)
    semantic: Semantic,
}

impl LayerTreePanel {
    /// 绑定场景的空面板(全展开、选中为空、回调为空操作)。
    pub fn new(scene: Entity<Scene>) -> Self {
        LayerTreePanel {
            scene,
            selection: std::rc::Rc::new(Vec::new()),
            collapsed: HashMap::new(),
            on_toggle_visible: Rc::new(|_, _| {}),
            on_toggle_lock: Rc::new(|_, _| {}),
            on_select: Rc::new(|_, _, _| {}),
            on_move_up: Rc::new(|_, _| {}),
            on_move_down: Rc::new(|_, _| {}),
            on_promote: Rc::new(|_, _| {}),
            on_demote: Rc::new(|_, _| {}),
            hover: HoverState::new(),
            hovered_node: None,
            flip: Rc::new(RefCell::new(FlipTracker::new())),
            pulses: Rc::new(RefCell::new(HashMap::new())),
            focus: None,
            pressed_node: None,
            semantic: Semantic::new(),
        }
    }

    /// 列表键盘导航(A11Y-01,导航面 = 扁平化行序):↑↓ 移动选择(走
    /// on_select 上报)、Enter 激活 = 翻转首选中行可见、Esc 交宿主。状态机
    /// = [`list_nav`] 纯函数(与 LayerPanel 同一单点,TC-A11Y-NAV-01)。
    fn on_list_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let rows = flatten_tree(self.scene.read(cx), &self.collapsed);
        let current = self
            .selection
            .first()
            .and_then(|id| rows.iter().position(|r| r.id == *id))
            .unwrap_or(0);
        match list_nav(current, rows.len(), &event.keystroke.key) {
            Some(ListNavIntent::Move(ix)) => {
                if let Some(row) = rows.get(ix) {
                    (self.on_select)(row.id, false, cx);
                }
            }
            Some(ListNavIntent::Activate) => {
                if let Some(&id) = self.selection.first() {
                    (self.on_toggle_visible)(id, cx);
                }
            }
            Some(ListNavIntent::Escape) | None => {}
        }
        cx.notify();
    }

    /// A7 撤销脉冲(同 [`crate::layer_panel::LayerPanel::pulse_for`] 约定):
    /// command 撤销/重做后对受影响节点调用,对应行 300ms accent 描边闪一次。
    pub fn pulse_for(&mut self, node: NodeId) {
        self.pulses
            .borrow_mut()
            .entry(node)
            .or_default()
            .begin(interact::now_ms());
    }

    /// 应用层同步选中集(渲染高亮用)。
    pub fn set_selection(&mut self, selection: Vec<NodeId>) {
        // PERF-04:内部存 Rc——渲染帧的 `selection.clone()` 是引用计数自增,
        // 零堆分配(万级图层滚动时每帧只余 roots 一处 Vec 收集)。
        self.selection = std::rc::Rc::new(selection);
    }

    /// 行点击(Shift = 加选)。
    pub fn on_select(mut self, f: impl Fn(NodeId, bool, &mut App) + 'static) -> Self {
        self.on_select = Rc::new(f);
        self
    }

    /// 眼睛 toggle。
    pub fn on_toggle_visible(mut self, f: impl Fn(NodeId, &mut App) + 'static) -> Self {
        self.on_toggle_visible = Rc::new(f);
        self
    }

    /// 锁 toggle。
    pub fn on_toggle_lock(mut self, f: impl Fn(NodeId, &mut App) + 'static) -> Self {
        self.on_toggle_lock = Rc::new(f);
        self
    }

    /// 上移一格(同父 Reparent,应用层落命令)。
    pub fn on_move_up(mut self, f: impl Fn(NodeId, &mut App) + 'static) -> Self {
        self.on_move_up = Rc::new(f);
        self
    }

    /// 下移一格。
    pub fn on_move_down(mut self, f: impl Fn(NodeId, &mut App) + 'static) -> Self {
        self.on_move_down = Rc::new(f);
        self
    }

    /// 提升一级(挂到父的父,紧跟父之后;应用层落 Reparent 命令)。
    pub fn on_promote(mut self, f: impl Fn(NodeId, &mut App) + 'static) -> Self {
        self.on_promote = Rc::new(f);
        self
    }

    /// 降一级(挂到前一个兄弟的 children 尾;应用层落 Reparent 命令)。
    pub fn on_demote(mut self, f: impl Fn(NodeId, &mut App) + 'static) -> Self {
        self.on_demote = Rc::new(f);
        self
    }

    /// 行高(派生制):max(26, 16 + 12) = 28,与 LayerPanel/属性行同高。
    pub fn row_height() -> f32 {
        control_height(
            crate::tokens::HEIGHT_DEFAULT,
            ROW_LINE_HEIGHT_PX,
            ROW_V_PADDING_PX,
        )
    }
}

impl Render for LayerTreePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let colors = t.colors;
        // 扁平行数组每帧重算(视图态折叠表参与下钻);虚拟化列表只吃行数
        let collapsed = self.collapsed.clone();
        let rows: Vec<TreeRow> = flatten_tree(self.scene.read(cx), &collapsed);
        let selection = self.selection.clone();
        let scene = self.scene.clone();
        let weak = cx.entity().downgrade();
        let flip = self.flip.clone();
        let pulses = self.pulses.clone();
        // A11Y-01:列表焦点(首帧惰性创建,tab_stop 进 Tab 环游)+ 按压快照
        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let list_focused = focus.is_focused(window);
        let pressed_node = self.pressed_node;

        // A7/A4 帧时钟与悬停快照(悬停互斥,单一进度值服务当前悬停行)
        let now = interact::now_ms();
        let hover_progress = self.hover.progress_at(now);
        let hovered_node = self.hovered_node;

        // A4 First/Last:对**全部**扁平行报告行位置(含滚出视口者与被折叠
        // 隐藏前的幸存行;折叠后行 y 集体前移 → 幸存行检出位移播放让位,
        // 与 LayerPanel 根级列表同一公式 y = index × 行高)。
        let row_h = f64::from(LayerTreePanel::row_height());
        {
            let mut tracker = flip.borrow_mut();
            for (ix, row) in rows.iter().enumerate() {
                tracker.measure(row.id.data().as_ffi(), ix as f64 * row_h);
            }
        }

        // 工具行:行数/选中数(纯信息,无操作按钮——四个重排按钮在每行行尾)
        let selected_count = self.selection.len();
        let toolbar = h_flex().gap(px(SpacingTokens::XS)).child(
            div()
                .flex_1()
                .text_size(px(FONT_SIZE_CAPTION))
                .text_color(colors.text_disabled)
                .child(format!("{} 行 · {} 选中", rows.len(), selected_count)),
        );

        // 虚拟化列表(gpui 0.2.2:闭包收 (range, window, app),无 view 参数)
        let count = rows.len();
        // 行内用的共享状态再克隆一份(flip/pulses 本体留给下方续帧判断)
        let (flip_rows, pulses_rows) = (flip.clone(), pulses.clone());
        let list = uniform_list("layer-tree", count, move |range, _window, cx| {
            let scene = scene.read(cx);
            let mut items = Vec::with_capacity(range.end - range.start);
            for ix in range {
                let Some(row) = rows.get(ix) else { continue };
                let id = row.id;
                let Some(node) = scene.node(id) else { continue };
                let (visible, locked) = (node.visible, node.locked);
                let name = node.name.clone();
                let (depth, has_children) = (row.depth, row.has_children);
                let selected = selection.contains(&id);
                // FLIP 元素键 = NodeId 的稳定 u64 重排序时不变
                let key = id.data().as_ffi();

                // A4 Invert/Play:本帧该行的让位偏移(折叠/展开/重排检出后
                // 160ms 衰减;减弱动态恒 0)
                let flip_offset = flip_rows.borrow_mut().invert_play(key, now);
                // A7:悬停高亮(TOK-04:选中行走 state-layer selected,悬停
                // 不与选中叠加)
                let hover_p = if hovered_node == Some(id) {
                    hover_progress
                } else {
                    0.0
                };
                // A7:撤销脉冲进度(查询即清理过期项);描边常挂、无脉冲透明
                let pulse_p = pulses_rows
                    .borrow_mut()
                    .entry(id)
                    .or_default()
                    .progress_at(now);

                let panel = weak.clone();
                let on_select = {
                    let panel = panel.clone();
                    move |ev: &MouseDownEvent, _win: &mut Window, cx: &mut App| {
                        let shift = ev.modifiers.shift;
                        let _ = panel.update(cx, |this, cx| {
                            this.pressed_node = Some(id); // A11Y-06 按压态
                            (this.on_select)(id, shift, cx);
                        });
                    }
                };
                let eye = {
                    let panel = panel.clone();
                    move |_ev: &MouseDownEvent, _win: &mut Window, cx: &mut App| {
                        let _ = panel.update(cx, |this, cx| (this.on_toggle_visible)(id, cx));
                    }
                };
                let lock = {
                    let panel = panel.clone();
                    move |_ev: &MouseDownEvent, _win: &mut Window, cx: &mut App| {
                        let _ = panel.update(cx, |this, cx| (this.on_toggle_lock)(id, cx));
                    }
                };
                // 折叠箭头:面板内部自治的视图态(不入撤销栈,见模块 doc)
                let arrow = {
                    let panel = panel.clone();
                    move |_ev: &MouseDownEvent, _win: &mut Window, cx: &mut App| {
                        let _ = panel.update(cx, |this, cx| {
                            toggle_collapsed(&mut this.collapsed, id);
                            cx.notify();
                        });
                    }
                };
                // 四个重排按钮(↑↓←→;应用层经 Reparent::capture 落命令)
                let promote = {
                    let panel = panel.clone();
                    move |_ev: &ClickEvent, _win: &mut Window, cx: &mut App| {
                        let _ = panel.update(cx, |this, cx| (this.on_promote)(id, cx));
                    }
                };
                let demote = {
                    let panel = panel.clone();
                    move |_ev: &ClickEvent, _win: &mut Window, cx: &mut App| {
                        let _ = panel.update(cx, |this, cx| (this.on_demote)(id, cx));
                    }
                };
                let up = {
                    let panel = panel.clone();
                    move |_ev: &ClickEvent, _win: &mut Window, cx: &mut App| {
                        let _ = panel.update(cx, |this, cx| (this.on_move_up)(id, cx));
                    }
                };
                let down = {
                    let panel = panel.clone();
                    move |_ev: &ClickEvent, _win: &mut Window, cx: &mut App| {
                        let _ = panel.update(cx, |this, cx| (this.on_move_down)(id, cx));
                    }
                };
                // A7:悬停进出(on_hover 需要 Stateful 元素,id 用稳定节点键)
                let on_hover = move |hovered: &bool, _win: &mut Window, cx: &mut App| {
                    let _ = panel.update(cx, |this, cx| {
                        let tick = interact::now_ms();
                        if *hovered {
                            this.hovered_node = Some(id);
                            this.hover.on_enter(tick);
                        } else if this.hovered_node == Some(id) {
                            this.hovered_node = None;
                            this.hover.on_leave(tick);
                        }
                        cx.notify();
                    });
                };
                let t = theme(cx);
                let colors = t.colors;
                let pressed = pressed_node == Some(id);
                let row_semantic = Semantic::new()
                    .with_role(SemanticRole::ListItem)
                    .with_label(name.clone());
                let mut row = h_flex()
                    .id(ElementId::NamedInteger("tree-row".into(), key))
                    .w_full()
                    .h(px(LayerTreePanel::row_height()))
                    .mt(pxv(flip_offset))
                    .px(px(SpacingTokens::XS))
                    .gap(px(SpacingTokens::XS))
                    .rounded(px(RadiusTokens::SM))
                    .border_1()
                    .border_color(if pulse_p > 0.0 {
                        colors.accent.opacity(f32v(pulse_p))
                    } else {
                        Hsla::transparent_black()
                    })
                    // TOK-04 / A11Y-06:三态 = layer_row_bg 纯函数(与
                    // LayerPanel 行同一单点)
                    .bg(layer_row_bg(&colors, selected, pressed, hover_p))
                    // 嵌套缩进:每层 12px(T2 任务书)
                    .child(div().w(pxv(depth as f64 * f64::from(INDENT_PX))).h_full());
                // 展开/折叠箭头(有子节点才有交互;叶子放同宽占位保持列对齐)
                // A11Y-03:视觉 12px、命中 ≥24px(监听挂热区容器)
                row = row.child(if has_children {
                    interact::hit_slot(
                        div()
                            .size(px(GLYPH_SIZE_PX))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(FONT_SIZE_CAPTION))
                            .text_color(colors.text_secondary)
                            // ▸ 折叠 / ▾ 展开(几何图标占位,Lucide = M2)
                            .child(if collapsed_contains(&collapsed, id) {
                                "▸"
                            } else {
                                "▾"
                            }),
                    )
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, arrow)
                } else {
                    div().size(px(GLYPH_SIZE_PX))
                });
                row = row
                    // 眼睛:填充方块 = 可见 / 透明 = 隐藏(与 LayerPanel 同款;
                    // A11Y-03:视觉 10px、命中 ≥24px)
                    .child(
                        interact::hit_slot(
                            div()
                                .size(px(EYE_LOCK_SIZE_PX))
                                .rounded(px(RadiusTokens::SM))
                                .border_1()
                                .border_color(colors.text_secondary)
                                .bg(if visible {
                                    colors.text_secondary
                                } else {
                                    Hsla::transparent_black()
                                }),
                        )
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, eye),
                    )
                    // 锁:填充 = 锁定(A11Y-03 同上)
                    .child(
                        interact::hit_slot(
                            div()
                                .size(px(EYE_LOCK_SIZE_PX))
                                .rounded(px(RadiusTokens::SM))
                                .border_1()
                                .border_color(colors.text_disabled)
                                .bg(if locked {
                                    colors.warning
                                } else {
                                    Hsla::transparent_black()
                                }),
                        )
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, lock),
                    )
                    // 名字 = 行选择热区(避免与眼睛/锁/箭头的事件冲突)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .text_size(px(FONT_SIZE_BODY))
                            .text_color(if selected {
                                colors.text_primary
                            } else {
                                colors.text_secondary
                            })
                            .child(name)
                            .on_mouse_down(MouseButton::Left, on_select),
                    )
                    // 行尾重排按钮组:← 提升一级 / → 降一级 / ↑ 上移 / ↓ 下移
                    // (真拖拽 = v2.1;目标计算走 promote_target 等纯函数)
                    // CMP-11 收口:私有 tree_glyph_button 已删,统一走
                    // controls::button 的 IconButton 内联形态(Secondary 凸起
                    // 底沿用旧视觉;16px → 20px 命中区,A11Y-03 方向更优)
                    .child(icon_button_element(
                        IconButton::new(ElementId::NamedInteger("tree-promote".into(), key), "←")
                            .variant(ButtonVariant::Secondary)
                            .on_press(promote),
                        cx,
                    ))
                    .child(icon_button_element(
                        IconButton::new(ElementId::NamedInteger("tree-demote".into(), key), "→")
                            .variant(ButtonVariant::Secondary)
                            .on_press(demote),
                        cx,
                    ))
                    .child(icon_button_element(
                        IconButton::new(ElementId::NamedInteger("tree-move-up".into(), key), "↑")
                            .variant(ButtonVariant::Secondary)
                            .on_press(up),
                        cx,
                    ))
                    .child(icon_button_element(
                        IconButton::new(ElementId::NamedInteger("tree-move-down".into(), key), "↓")
                            .variant(ButtonVariant::Secondary)
                            .on_press(down),
                        cx,
                    ))
                    .on_hover(on_hover);
                // A11Y-02:行语义挂接(ListItem + 节点名;单点透传待 TD-01)
                let row = interact::attach_semantics(row, &row_semantic);
                items.push(row);
            }
            items
        })
        .flex_1()
        .min_h_0()
        .bg(colors.surface_1)
        .track_focus(&focus)
        .on_key_down(cx.listener(Self::on_list_key));

        // 悬停过渡 / 让位 / 脉冲任一在跑就续帧(动画运行才请求帧,静止零帧
        // 提交;脉冲表的过期项借本次遍历清理,不驻留)
        let needs_frames = self.hover.is_running(now)
            || flip.borrow_mut().is_animating(now)
            || pulses.borrow_mut().values_mut().any(|p| p.is_active(now));
        if needs_frames {
            window.request_animation_frame();
        }

        // A11Y-01:焦点环画在列表区(容器级,两主题 accent,interact 单点)
        let mut list_area = div().flex_1().min_h_0().relative().child(list);
        if list_focused {
            list_area = list_area.children(interact::focus_ring(colors.accent, RadiusTokens::SM));
        }

        v_flex()
            .size_full()
            .gap(px(SpacingTokens::XS))
            .p(px(SpacingTokens::XS))
            .bg(colors.surface_1)
            .child(toolbar)
            .child(list_area)
            // A11Y-06:按压态松手即清
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _ev, _win, cx| {
                    if this.pressed_node.take().is_some() {
                        cx.notify();
                    }
                }),
            )
    }
}

// A11Y-02 语义槽:面板可访问名缺省"图层树"、role 缺省 List。
semantic_slot!(LayerTreePanel);

impl LayerTreePanel {
    /// 解析语义(A11Y-02):显式 `.label(...)`/`.role(...)` 优先,缺省 =
    /// ("图层树", List);行语义见 render 的 ListItem 挂接。
    #[must_use]
    pub fn resolved_semantic(&self) -> Semantic {
        let sem = match self.semantic.label() {
            Some(_) => self.semantic.clone(),
            None => self.semantic.clone().with_label("图层树"),
        };
        let role = sem.role().unwrap_or(SemanticRole::List);
        sem.with_role(role)
    }
}

/// 折叠表只读查询(渲染侧用;absent = 展开)。
fn collapsed_contains(collapsed: &HashMap<NodeId, bool>, id: NodeId) -> bool {
    collapsed.get(&id).copied().unwrap_or(false)
}

/// f64 几何 → 像素(与 LayerPanel 同款惯例:几何 f64,画元素前一刻降 f32;
/// 行偏移/行高/缩进均为像素级小量,截断无碍)。
#[allow(clippy::cast_possible_truncation)]
fn pxv(v: f64) -> gpui::Pixels {
    px(v as f32)
}

/// f64 动画进度 → f32(透明度插值入 GPU 域的收口,0..1 进度截断无碍)。
#[allow(clippy::cast_possible_truncation)]
fn f32v(v: f64) -> f32 {
    v as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer_panel::{self, LayerPanel};
    use sable_foundation::command::{History, Reparent};
    use sable_foundation::scene::NodeContent;

    /// 5 层嵌套 + 根级叶:L0 > L1 > L2 > L3 > 叶4,外加根级叶 R。
    /// 返回 (scene, [L0..L3, 叶4, R])。
    fn deep_scene() -> (Scene, [NodeId; 6]) {
        let mut scene = Scene::new();
        let l0 = scene.add_node(None, "L0", NodeContent::Group).expect("L0");
        let l1 = scene
            .add_node(Some(l0), "L1", NodeContent::Group)
            .expect("L1");
        let l2 = scene
            .add_node(Some(l1), "L2", NodeContent::Group)
            .expect("L2");
        let l3 = scene
            .add_node(Some(l2), "L3", NodeContent::Group)
            .expect("L3");
        let leaf = scene
            .add_node(Some(l3), "叶4", NodeContent::Group)
            .expect("叶4");
        let r = scene.add_node(None, "R", NodeContent::Group).expect("R");
        (scene, [l0, l1, l2, l3, leaf, r])
    }

    fn ids(rows: &[TreeRow]) -> Vec<NodeId> {
        rows.iter().map(|r| r.id).collect()
    }

    fn depths(rows: &[TreeRow]) -> Vec<usize> {
        rows.iter().map(|r| r.depth).collect()
    }

    #[test]
    fn flatten_walks_five_levels_with_increasing_depth() {
        // T2 验收:5 层嵌套渲染正确;depth 递增(roots = 0)
        let (scene, [l0, l1, l2, l3, leaf, r]) = deep_scene();
        let rows = flatten_tree(&scene, &HashMap::new());
        assert_eq!(ids(&rows), vec![l0, l1, l2, l3, leaf, r], "DFS 下钻序");
        assert_eq!(depths(&rows), vec![0, 1, 2, 3, 4, 0], "depth 每层 +1");
        // 前 4 行(l0..l3)是含子组 → 有箭头;叶4 与 R 是无子 Group → 无箭头
        assert!(rows[..4].iter().all(|row| row.has_children));
        assert!(!rows[4].has_children);
        assert!(!rows[5].has_children);
    }

    #[test]
    fn collapse_middle_group_hides_only_its_subtree() {
        // 折叠中间组 L1 → 只藏 L1 的子树(L2/L3/叶4),L0 与 R 照常
        let (scene, [l0, l1, l2, l3, leaf, r]) = deep_scene();
        let mut collapsed = HashMap::new();
        collapsed.insert(l1, true);
        let rows = flatten_tree(&scene, &collapsed);
        assert_eq!(ids(&rows), vec![l0, l1, r], "L1 子树整体不出行");
        assert_eq!(depths(&rows), vec![0, 1, 0]);
        assert!(!ids(&rows).contains(&l2));
        assert!(!ids(&rows).contains(&l3));
        assert!(!ids(&rows).contains(&leaf));

        // 再折叠 L0 → 只剩两行;切换回展开(toggle 两次)恢复全量
        collapsed.insert(l0, true);
        assert_eq!(ids(&flatten_tree(&scene, &collapsed)), vec![l0, r]);
        toggle_collapsed(&mut collapsed, l1);
        toggle_collapsed(&mut collapsed, l0);
        assert!(collapsed.values().all(|v| !*v), "toggle 两轮后全部回展开");
        assert_eq!(flatten_tree(&scene, &collapsed).len(), 6);
    }

    #[test]
    fn view_state_toggle_never_touches_history() {
        // T2 验收:折叠状态可撤销?否——视图态不入撤销栈
        let (mut scene, [l0, l1, _l2, _l3, _leaf, _r]) = deep_scene();
        let mut history = History::new();
        let mut collapsed = HashMap::new();
        for _ in 0..8 {
            toggle_collapsed(&mut collapsed, l0);
            toggle_collapsed(&mut collapsed, l1);
            let _ = flatten_tree(&scene, &collapsed);
        }
        assert_eq!(history.undo_len(), 0);
        assert!(!history.can_undo());
        // 对照组:真正的重排命令才进栈(场景 + History 手工驱动)
        let target = promote_target(&scene, l1).expect("L1 可提升");
        let cmd = Reparent::capture(&scene, l1, target.0, target.1).expect("capture");
        history.exec(Box::new(cmd), &mut scene);
        assert_eq!(history.undo_len(), 1);
    }

    #[test]
    fn reorder_targets_produce_undoable_reparent_commands() {
        // 逐 exec 断言目标与中间态;最终以"全撤销结构化还原"收口
        // (逐条 undo 的中间顺序断言易把叙事顺序当应用顺序,proptest 已覆盖还原性)
        // T2 验收 2.2:重排按钮的目标 → Reparent 命令,一次撤销还原
        let mut scene = Scene::new();
        let g = scene.add_node(None, "G", NodeContent::Group).expect("g");
        let a = scene.add_node(Some(g), "A", NodeContent::Group).expect("a");
        let b = scene.add_node(Some(g), "B", NodeContent::Group).expect("b");
        let c = scene.add_node(Some(g), "C", NodeContent::Group).expect("c");
        let _d = scene.add_node(None, "D", NodeContent::Group).expect("d");
        let mut history = History::new();
        let exec_target =
            |scene: &mut Scene, history: &mut History, id: NodeId, target: ReparentTarget| {
                let (parent, index) = target;
                let cmd = Reparent::capture(scene, id, parent, index).expect("capture");
                history.exec(Box::new(cmd), scene);
            };

        // 降一级:B 挂进前一个兄弟 A 的 children 尾
        let before = scene.clone();
        let target = demote_target(&scene, b).expect("B 有前兄弟 A");
        assert_eq!(target, (Some(a), None));
        exec_target(&mut scene, &mut history, b, target);
        assert_eq!(scene.node(b).expect("b").parent, Some(a));
        assert!(scene.node(a).expect("a").children.contains(&b));

        // 提升一级:B(现挂 A 下)回到 G、紧跟 A 之后
        let target = promote_target(&scene, b).expect("B 有父 A");
        assert_eq!(target, (Some(g), Some(1)));
        exec_target(&mut scene, &mut history, b, target);
        assert_eq!(scene.node(b).expect("b").parent, Some(g));
        assert_eq!(scene.node(g).expect("g").children, vec![a, b, c]);

        // 上移:B 与 A 换位(同父 index -1)
        let target = sibling_move_target(&scene, b, -1).expect("B 非.首位");
        assert_eq!(target, (Some(g), Some(0)));
        exec_target(&mut scene, &mut history, b, target);
        assert_eq!(scene.node(g).expect("g").children, vec![b, a, c]);

        // 下移:B 再回到中间(同父 index +1)
        let target = sibling_move_target(&scene, b, 1).expect("B 非末位");
        assert_eq!(target, (Some(g), Some(1)));
        exec_target(&mut scene, &mut history, b, target);
        assert_eq!(scene.node(g).expect("g").children, vec![a, b, c]);
        assert_eq!(history.undo_len(), 4);

        // 全撤销:四次重排结构化还原(到降级前;Scene 相等忽略 slotmap key)
        while history.can_undo() {
            history.undo(&mut scene);
        }
        assert_eq!(scene, before, "四次重排全部撤销后结构化还原");
        assert_eq!(history.undo_len(), 0);
    }

    #[test]
    fn reorder_targets_are_none_at_boundaries() {
        // 边界:根级不可再提升;首位不可降/不可上移;末位不可下移
        let (scene, [l0, _l1, l2, l3, leaf, r]) = deep_scene();
        assert_eq!(promote_target(&scene, l0), None, "根级无父可提升");
        assert_eq!(promote_target(&scene, r), None);
        assert_eq!(sibling_move_target(&scene, l3, -1), None, "L3 是首位");
        assert_eq!(demote_target(&scene, l3), None, "首位无前兄弟可降");
        assert_eq!(sibling_move_target(&scene, leaf, 1), None, "叶4 是末位");
        // 唯一孩子 L2:有父可提升,但无兄弟可降/移
        assert!(promote_target(&scene, l2).is_some());
        assert_eq!(demote_target(&scene, l2), None);
        assert_eq!(sibling_move_target(&scene, l2, -1), None);
        assert_eq!(sibling_move_target(&scene, l2, 1), None);
    }

    #[test]
    fn tree_row_swap_flip_offsets_decay_and_reduced_motion_zeros() {
        // T2 验收:交换两行 key → 偏移从 ±行高衰减到 0;减弱动态直接 0
        let mut scene = Scene::new();
        let a = scene.add_node(None, "A", NodeContent::Group).expect("a");
        let b = scene.add_node(None, "B", NodeContent::Group).expect("b");
        let key = |id: NodeId| id.data().as_ffi();
        let row_h = f64::from(LayerTreePanel::row_height());
        let collapsed: HashMap<NodeId, bool> = HashMap::new();
        let mut tracker = FlipTracker::new();

        // 帧 1:根序 [A, B]
        for (ix, row) in flatten_tree(&scene, &collapsed).iter().enumerate() {
            tracker.measure(key(row.id), ix as f64 * row_h);
        }
        // 交换两行(同 LayerPanel 公式 y = index × 行高,重排检出)
        scene.roots.swap(0, 1);
        for (ix, row) in flatten_tree(&scene, &collapsed).iter().enumerate() {
            tracker.measure(key(row.id), ix as f64 * row_h);
        }
        let now = 1000.0;
        assert_eq!(tracker.invert_play(key(a), now), -row_h, "A:旧 0 新 行高");
        assert_eq!(tracker.invert_play(key(b), now), row_h, "B:旧行高 新 0");
        assert!(tracker.is_animating(now));
        // 160ms 到点归 0 并停止续帧(OutCubic 全程衰减,flip.rs 已验中点)
        assert_eq!(tracker.invert_play(key(a), now + 160.0), 0.0);
        assert_eq!(tracker.invert_play(key(b), now + 400.0), 0.0);
        assert!(!tracker.is_animating(now + 400.0));

        // 减弱动态(A8):交换检出的位移直接落位,偏移恒 0、不续帧
        crate::anim::set_reduced_motion(true);
        let mut tracker = FlipTracker::new();
        tracker.measure(key(a), 0.0);
        tracker.measure(key(b), row_h);
        tracker.measure(key(b), 0.0); // 交换
        tracker.measure(key(a), row_h);
        assert_eq!(tracker.invert_play(key(a), 0.0), 0.0);
        assert_eq!(tracker.invert_play(key(b), 0.0), 0.0);
        assert!(!tracker.is_animating(0.0));
        crate::anim::set_reduced_motion(false);
    }

    #[test]
    fn row_height_matches_layer_panel() {
        // 派生公式同源:行高与拍平版/属性行一致(28)
        assert_eq!(LayerTreePanel::row_height(), LayerPanel::row_height());
        assert_eq!(LayerTreePanel::row_height(), 28.0);
        assert_eq!(
            LayerTreePanel::row_height(),
            crate::property_row::PropertyRow::row_height()
        );
        assert_eq!(f64::from(INDENT_PX), 12.0, "每层缩进 12px(T2 任务书)");
    }

    #[test]
    fn select_semantics_reuse_layer_panel_apply_select() {
        // 选中透传:回调拿到 (id, shift) 后,应用层用同一个 apply_select
        // (此处锁住两面板共享同一纯函数,防分叉)
        let mut scene = Scene::new();
        let a = scene.add_node(None, "A", NodeContent::Group).expect("a");
        let b = scene.add_node(None, "B", NodeContent::Group).expect("b");
        assert_eq!(layer_panel::apply_select(&[], a, false), vec![a]);
        assert_eq!(layer_panel::apply_select(&[a], b, true), vec![a, b]);
    }
}
