//! 命令面板 CommandPalette(迭代审查报告 2026-10-04 CMP-03,§5.6 组件矩阵
//! #12、§5.8 动效;`Mod+K`,建议键位见 [`crate::keymap::ACTION_COMMAND_PALETTE`])。
//!
//! # 形态(Entity 宿主 + 注册表单源)
//!
//! [`CommandPalette`] 是 **Entity 形态浮层宿主**:挂在应用根视图;库提供
//! [`Self::open`]/[`Self::close`] API,**`Mod+K` 由宿主键位绑定**(键位以
//! 宿主注册表为单源,库不做全局监听——[`crate::keymap`] 模块 doc 的契约;
//! 打开动作在宿主 `on_action` 里调 `palette.update(cx, |p, cx| p.open(...))`)。
//!
//! ```ignore
//! let palette = cx.new(|_| CommandPalette::new("palette"));
//! palette.update(cx, |p, cx| {
//!     p.set_entries(CommandEntry::from_registry(&registry));   // 注册表单源
//!     p.with_on_execute(...)                                   // 执行回执
//! });
//! ```
//!
//! # 数据源与执行(§5.6 #12)
//!
//! 条目 [`CommandEntry`] = id/label/keystroke/category/aliases;
//! [`CommandEntry::from_registry`] 把 [`KeymapRegistry`](crate::keymap) 的
//! [`ActionSpec`](crate::keymap)(label/keystroke/category 三字段同源)批量
//! 转成条目;宿主自定义条目走 [`CommandEntry::new`] 追加。执行统一经
//! [`Self::on_execute`] 单点回执(`fn(&CommandEntry, &mut Window, &mut App)`,
//! 宿主按 entry.id 分发)。
//!
//! # 模糊搜索(纯函数,TC-CMP-CMD-01 断言面)
//!
//! [`fuzzy_match`]:大小写不敏感的**子序列匹配**(字符按序全中才算命中),
//! 评分 = 命中字符 + 连续命中奖励 + 词首奖励;命中下标集供行内高亮
//! ([`render_query_label`] 把 label 切成匹配/未匹配连续段)。**拼音首字母
//! 容错未内置**(选做项,如实声明):内置拼音表是数据源问题,超出本轮三
//! 文件预算;库侧给出 [`CommandEntry::aliases`] 槽——宿主为中文条目配
//! `"dc"`/`"daochu"` 等别名,匹配对 label + aliases 全量跑(别名命中同分
//! 评级,仅高亮不适用)。覆盖面:ASCII/UTF-8 字面子序列 + 别名容错;中文
//! 拼音匹配依赖宿主供别名。
//!
//! # 最近执行置顶
//!
//! [`Self::note_execute`] 记最近 id([`push_recent`] 纯函数:去重置顶 +
//! 容量 [`PALETTE_RECENT_CAP`] 截断);[`rank_entries`] 排序 = **最近执行
//! 优先**(按新近序)→ 评分降序 → 注册序稳定。
//!
//! # 键盘(唯一状态机 [`palette_nav`])
//!
//! ↑/↓ 移动高亮(端点钳制,select 同款)、Home/End 跳端、Enter 执行高亮、
//! Esc/Tab 关闭;字符/退格/删除/光标移动走
//! [`edit_key`](crate::controls::text_field)(text_field 键盘映射单点复用,
//! 编辑缓冲复用 [`TextFieldBuffer`](crate::controls::text_field))。
//!
//! **搜索框形态(如实)**:查询行复用 text_field 的**纯核**
//! ([`TextFieldBuffer`] + `edit_key`,编辑语义单一真相),不复用
//! `TextField` Entity——其焦点句柄惰性建于自身渲染,外部无法编程聚焦,
//! `Mod+K → 键入 → Enter` 的纯键盘契约保不住;代价是无 IME 组合态
//! (中文检索请配拼音别名,见上)与无行内光标渲染(v0.1 边界)。
//!
//! # 动效(§5.6 #12)
//!
//! 下滑 120ms(STATE 档,[`PALETTE_SLIDE_MS`],面板 -8px → 0 落位)+
//! 背板 80ms fade(HOVER 档,[`PALETTE_SCRIM_MS`]);`reduced_motion` 直切
//! ([`Animated`] 直通目标,无插值)。

use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    App, Context, Div, ElementId, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::anim::{Animated, Easing};
use crate::controls::text_field::{
    EditKey, TextFieldBuffer, TextFieldSize, edit_key, text_field_style,
};
use crate::controls::toast::{OVERLAY_MARGIN_PX, overlay_scrim};
use crate::controls::tooltip::key_badge_text;
use crate::interact::{InteractState, Semantic, SemanticRole, semantic_slot, state_layer};
use crate::keymap::{KeymapRegistry, chord_display};
use crate::theme::{elevated, theme};
use crate::tokens::{MotionTokens, RadiusTokens, SpacingTokens, TextSize, UI_FONT, h_flex, v_flex};

// ---------------------------------------------------------------------------
// 常量域(几何/时序具名常量;颜色一律令牌,零字面量)
// ---------------------------------------------------------------------------

/// 面板下滑时长 = STATE 档(120ms,§5.6 #12)。
pub const PALETTE_SLIDE_MS: f64 = MotionTokens::DUR_STATE_MS;
/// 背板淡变时长 = HOVER 档(80ms,§5.6 #12"背板 fade 80ms")。
pub const PALETTE_SCRIM_MS: f64 = MotionTokens::DUR_HOVER_MS;
/// 下滑位移(px;单源复用 select 下拉的 8px 浮层位移常量)。
pub const PALETTE_SLIDE_PX: f32 = super::select::MENU_OPEN_OFFSET_PX;
/// 面板距视口顶(px,4 网格 24×4;顶部居中惯例)。
pub const PALETTE_TOP_PX: f32 = 96.0;
/// 面板宽(px,4 网格;窄窗由渲染钳进视口)。
pub const PALETTE_WIDTH_PX: f32 = 560.0;
/// 最大可见行数(超出纵向滚动)。
pub const PALETTE_MAX_ROWS: usize = 8;
/// 最近执行记忆容量。
pub const PALETTE_RECENT_CAP: usize = 8;

/// 面板行高(派生制,**复用 select 下拉行高单点**——同一浮层行节奏)。
#[must_use]
pub fn palette_row_height() -> f32 {
    super::select::menu_row_height()
}

/// 面板列表区高度(px,纯函数):`min(命中数, PALETTE_MAX_ROWS) × 行高`。
#[allow(clippy::cast_possible_truncation)]
#[must_use]
pub fn palette_list_height(hit_count: usize) -> f32 {
    hit_count.min(PALETTE_MAX_ROWS) as f32 * palette_row_height()
}

// ---------------------------------------------------------------------------
// 命令条目(数据源;注册表单源 + 宿主自定义)
// ---------------------------------------------------------------------------

/// 一条命令(id/label/键位/分组/别名;label + keystroke 展示文案 =
/// [`key_badge_text`] 单源「名称 (快捷键)」)。
#[derive(Clone, Default)]
pub struct CommandEntry {
    /// 稳定 id(执行回执与最近执行的键)
    pub id: SharedString,
    /// 展示名
    pub label: SharedString,
    /// 键位(gpui keystroke 语法;渲染经 [`chord_display`] 惯用名化;空 = 未绑定)
    pub keystroke: SharedString,
    /// 分组(空 = 不分组)
    pub category: SharedString,
    /// 匹配备名(拼音首字母/缩写容错槽;宿主为中文条目自配,库不内置拼音表)
    pub aliases: Vec<SharedString>,
}

impl CommandEntry {
    /// 指定 id 与展示名的条目(keystroke/category/aliases 空)。
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        CommandEntry {
            id: id.into(),
            label: label.into(),
            keystroke: SharedString::default(),
            category: SharedString::default(),
            aliases: Vec::new(),
        }
    }

    /// 由 [`ActionSpec`](crate::keymap) 转换(label/keystroke/category 三字段
    /// 同源;执行回执走面板级 [`Self::on_execute`],条目不携带闭包)。
    #[must_use]
    pub fn from_spec(spec: &crate::keymap::ActionSpec) -> Self {
        CommandEntry {
            id: spec.id.into(),
            label: spec.label.into(),
            keystroke: spec.default_keystroke.into(),
            category: spec.category.into(),
            aliases: Vec::new(),
        }
    }

    /// 批量转换(注册表 → 条目;保持注册顺序)。
    #[must_use]
    pub fn from_registry(registry: &KeymapRegistry) -> Vec<Self> {
        registry.actions().iter().map(Self::from_spec).collect()
    }

    /// 追加备名(链式;多别名逐个)。
    #[must_use]
    pub fn alias(mut self, alias: impl Into<SharedString>) -> Self {
        let alias = alias.into();
        if !alias.trim().is_empty() {
            self.aliases.push(alias);
        }
        self
    }

    /// 「名称 (快捷键)」行文案([`key_badge_text`] 单源;**原始 gpui 语法**
    /// 传入、函数内单次渲染——已渲染串回喂会把大写键再翻成 `Shift+X`,
    /// gpui `Keystroke::parse` 的大写键语义,见 tooltip 模块 doc)。
    #[must_use]
    pub fn badge_text(&self) -> String {
        let keys = self.keystroke.trim();
        key_badge_text(self.label.as_ref(), (!keys.is_empty()).then_some(keys))
    }

    /// 键位惯用展示(空串 = 未绑定)。
    #[must_use]
    pub fn rendered_keystroke(&self) -> String {
        chord_display(self.keystroke.as_ref())
    }
}

// ---------------------------------------------------------------------------
// 模糊匹配(纯函数;TC-CMP-CMD-01 断言面)
// ---------------------------------------------------------------------------

/// 一次模糊命中的结果:评分与命中字符下标(char 序,供高亮切段)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FuzzyMatch {
    /// 评分(越大越好;仅用于同面板内相对排序)
    pub score: i64,
    /// 命中字符的下标(char 序,升序)
    pub indices: Vec<usize>,
}

/// 子序列模糊匹配(纯函数,大小写不敏感):`query` 全部字符按序出现在
/// `text` 中才命中。评分:每命中字符 +2,**与上一命中连续**再 +1(连续段
/// 优先),**命中在词首**(文本首/前一字符非字母数字)再 +1。空 query 视为
/// 命中全部(score 0,无高亮下标)。
#[must_use]
pub fn fuzzy_match(query: &str, text: &str) -> Option<FuzzyMatch> {
    let needle: Vec<char> = query.chars().flat_map(|c| c.to_lowercase()).collect();
    if needle.is_empty() {
        return Some(FuzzyMatch::default());
    }
    let haystack: Vec<char> = text.chars().flat_map(|c| c.to_lowercase()).collect();
    let mut score: i64 = 0;
    let mut indices: Vec<usize> = Vec::with_capacity(needle.len());
    let mut needle_index = 0usize;
    for (index, ch) in haystack.iter().enumerate() {
        if needle_index >= needle.len() {
            break;
        }
        if *ch != needle[needle_index] {
            continue;
        }
        // 连续命中奖励(上一命中的下一个位置)
        let contiguous = indices.last().is_some_and(|prev| *prev + 1 == index);
        // 词首奖励:文本首 或 前一字符非字母数字
        let word_start = index == 0 || !haystack[index - 1].is_alphanumeric();
        score += 2 + i64::from(contiguous) + i64::from(word_start);
        indices.push(index);
        needle_index += 1;
    }
    (needle_index == needle.len()).then_some(FuzzyMatch { score, indices })
}

/// 排序后的命中项(面板条目下标 + 评分)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RankedEntry {
    /// [`CommandEntry`] 在原列表中的下标
    pub index: usize,
    /// 命中评分(label 直配);别名命中另扣 [`ALIAS_MATCH_PENALTY`]
    pub score: i64,
}

/// 别名命中相对 label 直配的评分折价(别名是容错路径,排序让位直配)。
pub const ALIAS_MATCH_PENALTY: i64 = 4;

/// 排序(纯函数):过滤命中(label 或任一 alias)→ **最近执行置顶**(按
/// [`recent`] 中的新近序,未执行的排后)→ 评分降序 → 注册序稳定。
/// 空 query = 全部命中(顺序 = 最近置顶 + 注册序)。
#[must_use]
pub fn rank_entries(
    entries: &[CommandEntry],
    query: &str,
    recent: &[SharedString],
) -> Vec<RankedEntry> {
    let mut ranked: Vec<RankedEntry> = entries
        .iter()
        .enumerate()
        .filter_map(
            |(index, entry)| match fuzzy_match(query, entry.label.as_ref()) {
                Some(hit) => Some(RankedEntry {
                    index,
                    score: hit.score,
                }),
                None => entry
                    .aliases
                    .iter()
                    .find_map(|alias| fuzzy_match(query, alias.as_ref()))
                    .map(|hit| RankedEntry {
                        index,
                        score: hit.score - ALIAS_MATCH_PENALTY,
                    }),
            },
        )
        .collect();
    let recent_rank = |index: usize| {
        recent
            .iter()
            .position(|id| entries[index].id == *id)
            .unwrap_or(usize::MAX)
    };
    ranked.sort_by(|a, b| {
        recent_rank(a.index)
            .cmp(&recent_rank(b.index))
            .then(b.score.cmp(&a.score))
            .then(a.index.cmp(&b.index))
    });
    ranked
}

/// 最近执行记录(纯函数):去重(已有则先摘除)→ 置顶 → 超容量从尾部
/// 截断。
pub fn push_recent(recent: &mut VecDeque<SharedString>, id: &str, cap: usize) {
    if let Some(pos) = recent.iter().position(|seen| seen.as_ref() == id) {
        recent.remove(pos);
    }
    recent.push_front(SharedString::from(id.to_string()));
    while recent.len() > cap {
        recent.pop_back();
    }
}

/// 键盘导航意图([`palette_nav`] 的输出)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteNav {
    /// 移动高亮(↑/↓/Home/End,已钳制)
    Highlight(usize),
    /// 执行高亮项(Enter)
    Execute(usize),
    /// 关闭面板(Esc/Tab)
    Close,
    /// 其余情形:无意图(字符/编辑键走 `edit_key` 通道)
    None,
}

/// 面板键盘状态机(纯函数,唯一列表导航语义单点):
/// ↑/↓ 相邻移动(端点钳制不回绕)、Home/End 跳端、Enter →
/// [`PaletteNav::Execute`]`(高亮项)`、Esc/Tab → Close;空列表仅 Esc/Tab
/// 关闭;越界 `highlighted` 钳入界内(防御)。
#[must_use]
pub fn palette_nav(highlighted: usize, count: usize, key: &str) -> PaletteNav {
    if count == 0 {
        return match key {
            "escape" | "tab" => PaletteNav::Close,
            _ => PaletteNav::None,
        };
    }
    let last = count - 1;
    match key {
        "down" => PaletteNav::Highlight(highlighted.min(last).saturating_add(1).min(last)),
        "up" => PaletteNav::Highlight(highlighted.min(last).saturating_sub(1)),
        "home" => PaletteNav::Highlight(0),
        "end" => PaletteNav::Highlight(last),
        "enter" => PaletteNav::Execute(highlighted.min(last)),
        "escape" | "tab" => PaletteNav::Close,
        _ => PaletteNav::None,
    }
}

/// 命中行高亮切段(纯函数):label 按 [`FuzzyMatch::indices`](连续下标归并)
/// 切成 (文本, 是否命中) 连续段;无命中的空 query → 单段全否。
#[must_use]
pub fn render_query_label(label: &str, hit: &FuzzyMatch) -> Vec<(String, bool)> {
    if hit.indices.is_empty() {
        return vec![(label.to_string(), false)];
    }
    let mut segments: Vec<(String, bool)> = Vec::new();
    for (char_index, ch) in label.chars().enumerate() {
        let matched = hit.indices.contains(&char_index);
        match segments.last_mut() {
            Some((text, is_match)) if *is_match == matched => text.push(ch),
            _ => segments.push((ch.to_string(), matched)),
        }
    }
    segments
}

// ---------------------------------------------------------------------------
// 执行回执
// ---------------------------------------------------------------------------

/// 执行回执(唯一分发单点;宿主按 `entry.id` 分发业务动作)。
pub type CommandExecuteFn = Rc<dyn Fn(&CommandEntry, &mut Window, &mut App)>;
/// 关闭回执(宿主可用来同步自身"面板已关"状态;可选)。
pub type CommandCloseFn = Rc<dyn Fn(&mut Window, &mut App)>;

// ---------------------------------------------------------------------------
// CommandPalette(Entity 形态,挂宿主根)
// ---------------------------------------------------------------------------

/// 命令面板(Entity 形态):顶部居中浮层 + 查询行 + 命中列表;数据源 =
/// 注册表条目 + 宿主自定义;模糊搜索、最近置顶、↑↓/Enter/Esc 全套。
pub struct CommandPalette {
    entries: Vec<CommandEntry>,
    /// 最近执行(容量 [`PALETTE_RECENT_CAP`];置顶排序依据)
    recent: VecDeque<SharedString>,
    /// 查询缓冲(复用 text_field 纯核;见模块 doc"搜索框形态")
    query: TextFieldBuffer,
    /// 当前高亮(命中列表下标)
    highlighted: usize,
    /// 开合
    open: bool,
    /// 面板下滑动画(0..1,STATE 120ms OutCubic)
    panel_anim: Animated<f64>,
    /// 背板淡变动画(0..1,HOVER 80ms 线性)
    scrim_anim: Animated<f64>,
    /// 面板根焦点句柄(打开时惰性创建并聚焦)
    focus: Option<FocusHandle>,
    /// 打开者焦点句柄(关闭时恢复焦点)
    opener: Option<FocusHandle>,
    on_execute: Option<CommandExecuteFn>,
    on_close: Option<CommandCloseFn>,
    /// A11Y-02 语义槽(label 缺省"命令面板";role 缺省 List——命中列表容器)
    semantic: Semantic,
}

// A11Y-02 语义槽(label/role/semantic 三件;role 默认 List——面板以命中
// 列表为语义单元;可访问名缺省"命令面板")。
semantic_slot!(CommandPalette);

impl CommandPalette {
    /// 空面板(关闭态;条目经 [`Self::set_entries`] 注入)。
    pub fn new() -> Self {
        CommandPalette {
            entries: Vec::new(),
            recent: VecDeque::new(),
            query: TextFieldBuffer::new(""),
            highlighted: 0,
            open: false,
            panel_anim: Animated::new(0.0),
            scrim_anim: Animated::new(0.0),
            focus: None,
            opener: None,
            on_execute: None,
            on_close: None,
            semantic: Semantic::new(),
        }
    }

    /// 替换条目集(注册表批量 + 宿主自定义统一入口;高亮归零)。
    pub fn set_entries(&mut self, entries: impl IntoIterator<Item = CommandEntry>) {
        self.entries = entries.into_iter().collect();
        self.highlighted = 0;
    }

    /// 替换条目集(链式;[`Self::set_entries`] 的 builder 形态)。
    #[must_use]
    pub fn with_entries(mut self, entries: impl IntoIterator<Item = CommandEntry>) -> Self {
        self.set_entries(entries);
        self
    }

    /// 执行回执(链式;唯一分发单点)。
    #[must_use]
    pub fn with_on_execute(
        mut self,
        f: impl Fn(&CommandEntry, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_execute = Some(Rc::new(f));
        self
    }

    /// 关闭回执(链式;可选)。
    #[must_use]
    pub fn with_on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    /// 解析语义(A11Y-02):显式 `.label(...)` 优先,缺省"命令面板";role
    /// 缺省 List。
    #[must_use]
    pub fn resolved_semantic(&self) -> Semantic {
        let mut sem = Semantic::new();
        let text = self
            .semantic
            .label()
            .cloned()
            .unwrap_or_else(|| SharedString::from("命令面板"));
        sem = sem.with_label(text);
        sem.with_role(self.semantic.role().unwrap_or(SemanticRole::List))
    }

    // —— 数据面(测试/宿主调试读) ——

    /// 当前条目集。
    #[must_use]
    pub fn entries(&self) -> &[CommandEntry] {
        &self.entries
    }

    /// 最近执行 id(新近序)。
    #[must_use]
    pub fn recent(&self) -> &VecDeque<SharedString> {
        &self.recent
    }

    /// 当前查询文本。
    #[must_use]
    pub fn query(&self) -> &str {
        self.query.text()
    }

    /// 面板是否打开。
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// 当前高亮下标。
    #[must_use]
    pub fn highlighted(&self) -> usize {
        self.highlighted
    }

    /// 当前命中列表(查询 + 最近置顶的排序结果;每次现算——条目集小,
    /// 现算省一份跨帧缓存失效面)。
    #[must_use]
    pub fn visible_entries(&self) -> Vec<RankedEntry> {
        let recent: Vec<SharedString> = self.recent.iter().cloned().collect();
        rank_entries(&self.entries, self.query.text(), &recent)
    }

    /// 面板下滑动画(宿主调试读)。
    #[must_use]
    pub fn panel_anim(&self) -> &Animated<f64> {
        &self.panel_anim
    }

    /// 背板淡变动画(宿主调试读)。
    #[must_use]
    pub fn scrim_anim(&self) -> &Animated<f64> {
        &self.scrim_anim
    }

    // —— 开合(库 API;Mod+K 由宿主键位绑定后调进来) ——

    /// 打开:清查询、高亮归零、启动下滑 + 背板动画、焦点移入面板根
    /// (`opener` 供关闭时恢复)。已开则刷新(重置查询与动画,幂等语义)。
    pub fn open(
        &mut self,
        opener: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        self.open = true;
        self.opener = opener;
        self.query.set_text("");
        self.highlighted = 0;
        self.panel_anim.set(
            1.0,
            Duration::from_secs_f64(PALETTE_SLIDE_MS / 1000.0),
            Easing::OutCubic,
        );
        self.scrim_anim.set(
            1.0,
            Duration::from_secs_f64(PALETTE_SCRIM_MS / 1000.0),
            Easing::Linear,
        );
        window.focus(&focus);
        cx.notify();
    }

    /// 关闭的纯状态步进(收合动画目标回落;无窗口/回执依赖,测试可直调)。
    pub fn begin_close(&mut self) {
        self.open = false;
        self.panel_anim.set(
            0.0,
            Duration::from_secs_f64(PALETTE_SLIDE_MS / 1000.0),
            Easing::OutCubic,
        );
        self.scrim_anim.set(
            0.0,
            Duration::from_secs_f64(PALETTE_SCRIM_MS / 1000.0),
            Easing::Linear,
        );
    }

    /// 关闭:启动收合动画、焦点还给打开者、回执 [`Self::with_on_close`]。
    /// 未开则无操作。
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.begin_close();
        if let Some(opener) = self.opener.clone() {
            window.focus(&opener);
        }
        if let Some(on_close) = self.on_close.clone() {
            on_close(window, cx);
        }
        cx.notify();
    }

    /// 查询写入(宿主受控面;高亮归零)。
    pub fn set_query(&mut self, text: &str) {
        self.query.set_text(text);
        self.highlighted = 0;
    }

    /// 记录一次执行(去重置顶 + 容量截断;高亮排序随之变化)。
    pub fn note_execute(&mut self, id: &str) {
        push_recent(&mut self.recent, id, PALETTE_RECENT_CAP);
    }

    /// 执行命中列表第 `index` 项:回执 [`Self::with_on_execute`] + 记最近 +
    /// 关闭。越界/无回执仅关面板(防御,不 panic)。
    pub fn execute(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let visible = self.visible_entries();
        let Some(ranked) = visible.get(index) else {
            self.close(window, cx);
            return;
        };
        let entry = self.entries[ranked.index].clone();
        push_recent(&mut self.recent, entry.id.as_ref(), PALETTE_RECENT_CAP);
        if let Some(on_execute) = self.on_execute.clone() {
            on_execute(&entry, window, cx);
        }
        self.close(window, cx);
    }

    /// 是否需要续帧(开合动画进行中;静止零帧提交)。
    #[must_use]
    pub fn needs_frame(&self) -> bool {
        let now = Instant::now();
        self.panel_anim.is_running_at(now) || self.scrim_anim.is_running_at(now)
    }

    // —— 键盘(唯一接线:列表导航 + 编辑映射两通道) ——

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let key = keystroke.key.as_str();
        // 通道一:列表导航(up/down/home/end/enter/escape/tab 不属编辑域;
        // home/end 给列表跳端而非查询光标——面板惯例)
        match palette_nav(self.highlighted, self.visible_entries().len(), key) {
            PaletteNav::None => {}
            PaletteNav::Highlight(index) => {
                self.highlighted = index;
                cx.notify();
                return;
            }
            PaletteNav::Execute(index) => {
                self.execute(index, window, cx);
                return;
            }
            PaletteNav::Close => {
                self.close(window, cx);
                return;
            }
        }
        // 通道二:编辑映射(text_field::edit_key 单点复用;面板无 IME 组合态)
        match edit_key(
            key,
            &keystroke.modifiers,
            keystroke.key_char.as_deref(),
            false,
        ) {
            EditKey::Ignored | EditKey::CancelComposition => {}
            EditKey::Submit => self.execute(self.highlighted, window, cx),
            EditKey::TabSubmit => self.close(window, cx),
            EditKey::Cancel => self.close(window, cx),
            EditKey::Backspace => {
                if self.query.backspace() {
                    self.highlighted = 0;
                    cx.notify();
                }
            }
            EditKey::Delete => {
                if self.query.delete() {
                    cx.notify();
                }
            }
            EditKey::Insert(ch) => {
                if self.query.insert(ch.encode_utf8(&mut [0; 4])) {
                    self.highlighted = 0;
                    cx.notify();
                }
            }
            EditKey::Caret(direction, extend) => {
                self.query.move_caret(direction, extend);
                cx.notify();
            }
            EditKey::SelectAll => self.query.select_all(),
        }
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for CommandPalette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let panel_progress = f32v(self.panel_anim.value_at(now));
        let scrim_progress = f32v(self.scrim_anim.value_at(now));
        let anim_running = self.needs_frame();
        if !self.open && !anim_running {
            return div().into_any_element();
        }
        let colors = theme(cx).colors;
        let entity = cx.entity();

        // 背板:共享单点遮罩,HOVER 80ms 独立淡变
        let scrim = overlay_scrim(&colors).opacity(scrim_progress);

        // 查询行(复用 text_field 视觉规格单点;占位 = text_placeholder 令牌)
        let style = text_field_style(TextFieldSize::Default, InteractState::Idle, &colors);
        let query_text = self.query.text().to_string();
        let query_line = h_flex()
            .min_h(px(style.height))
            .w_full()
            .px(px(SpacingTokens::SM))
            .font_family(UI_FONT)
            .text_size(px(style.text.size))
            .font_weight(gpui::FontWeight(style.text.weight))
            .child(if query_text.is_empty() {
                div()
                    .text_color(style.placeholder)
                    .truncate()
                    .child("搜索命令…")
            } else {
                div().text_color(style.fg).truncate().child(query_text)
            });

        // 命中列表(↑↓ 高亮;行内高亮 = render_query_label 切段)
        let visible = self.visible_entries();
        let highlighted = self.highlighted.min(visible.len().saturating_sub(1));
        let mut rows = v_flex().min_w_full();
        for (row, ranked) in visible.iter().take(PALETTE_MAX_ROWS).enumerate() {
            let Some(entry) = self.entries.get(ranked.index) else {
                continue;
            };
            let is_highlighted = row == highlighted;
            let hit = fuzzy_match(self.query.text(), entry.label.as_ref()).unwrap_or_default();
            let mut row_div = h_flex()
                .justify_between()
                .w_full()
                .h(px(palette_row_height()))
                .px(px(SpacingTokens::SM))
                .gap(px(SpacingTokens::SM))
                .font_family(UI_FONT)
                .text_size(px(TextSize::LABEL.size))
                .font_weight(gpui::FontWeight(TextSize::LABEL.weight))
                .child(self.render_label(entry, &hit, is_highlighted, &colors))
                .child(
                    div()
                        .text_color(if is_highlighted {
                            colors.text_secondary
                        } else {
                            colors.text_tertiary
                        })
                        .truncate()
                        .child(entry.rendered_keystroke()),
                );
            if is_highlighted {
                row_div = row_div.bg(state_layer(
                    colors.surface_1,
                    InteractState::Selected,
                    colors.accent,
                ));
            } else {
                let hover_bg = state_layer(colors.surface_1, InteractState::Hover, colors.accent);
                row_div = row_div.hover(move |s| s.bg(hover_bg));
            }
            // 点击行 = 执行(捕获可见序号;实体闭包单点)
            let entity = entity.clone();
            rows = rows.child(
                row_div
                    .id(ElementId::NamedInteger(
                        "palette-row".into(),
                        u64::try_from(row).unwrap_or(0),
                    ))
                    .cursor_pointer()
                    .on_mouse_down(gpui::MouseButton::Left, move |_ev, window, cx| {
                        entity.update(cx, |palette, cx| palette.execute(row, window, cx));
                    }),
            );
        }
        let rows_area = rows
            .id("palette-rows")
            .max_h(px(palette_list_height(visible.len())))
            .overflow_y_scroll();

        // 面板壳:L4 材质(surface_1 + border_strong + LG 圆角),下滑 8px
        // 同插值(进度 0 → 上方 8px,进度 1 → 落位)
        let panel = elevated(
            4,
            v_flex()
                .w(px(PALETTE_WIDTH_PX))
                .max_w_full()
                .px(px(SpacingTokens::SM))
                .py(px(SpacingTokens::SM))
                .gap(px(SpacingTokens::XS))
                .rounded(px(RadiusTokens::LG))
                .border_1()
                .border_color(colors.border_strong)
                .bg(state_layer(
                    colors.surface_1,
                    InteractState::Idle,
                    colors.accent,
                ))
                .child(query_line)
                .child(rows_area),
        )
        .mt(px((1.0 - panel_progress) * -PALETTE_SLIDE_PX))
        .opacity(panel_progress);

        let focus = self
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let root = div()
            .absolute()
            .inset_0()
            .child(scrim)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .pt(px(PALETTE_TOP_PX))
                    .px(px(OVERLAY_MARGIN_PX as f32))
                    .child(panel),
            )
            .track_focus(&focus)
            .on_key_down(cx.listener(Self::on_key_down));

        if anim_running {
            window.request_animation_frame();
        }
        root.into_any_element()
    }
}

impl CommandPalette {
    /// 行内 label 渲染:命中段 accent、未命中段正文色(render_query_label
    /// 切段;连续段归并,分段子 div 与 TextField 分段渲染同款取舍)。
    fn render_label(
        &self,
        entry: &CommandEntry,
        hit: &FuzzyMatch,
        is_highlighted: bool,
        colors: &crate::tokens::ColorTokens,
    ) -> Div {
        let base = if is_highlighted {
            colors.text_strong
        } else {
            colors.text_primary
        };
        let mut line = h_flex().min_w_0();
        for (text, matched) in render_query_label(entry.label.as_ref(), hit) {
            line = line.child(
                div()
                    .text_color(if matched { colors.accent } else { base })
                    .child(text),
            );
        }
        line
    }
}

/// 帧泵裁决:面板是否需要续帧(与 `tooltip_host_needs_frame` 同款契约)。
#[must_use]
pub fn command_palette_needs_frame(palette: &CommandPalette) -> bool {
    palette.needs_frame()
}

// ---------------------------------------------------------------------------
// 工具(f64 → f32 收口,button.rs 同款惯例)
// ---------------------------------------------------------------------------

#[allow(clippy::cast_possible_truncation)]
fn f32v(v: f64) -> f32 {
    v as f32
}

// ---------------------------------------------------------------------------
// TC-CMP-CMD-01(模糊匹配纯函数 + 最近置顶 + 键盘状态机 + 动画相位)
// 全部纯函数断言,不经 GUI、不触全局开关(reduced 走 Animated 全局开关,
// 测试内成对置位/复位)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::text_field::TextCaretMove;
    use crate::keymap::{ActionSpec, KeymapRegistry};

    fn entries() -> Vec<CommandEntry> {
        vec![
            CommandEntry::new("undo", "撤销")
                .with_keystroke("ctrl-z")
                .with_category("编辑"),
            CommandEntry::new("redo", "重做")
                .with_keystroke("shift-ctrl-z")
                .with_category("编辑"),
            CommandEntry::new("export", "导出视频")
                .alias("dc")
                .alias("daochu"),
            CommandEntry::new("toggle-timeline", "切换时间轴").with_category("视图"),
        ]
    }

    impl CommandEntry {
        fn with_keystroke(mut self, keys: &str) -> Self {
            self.keystroke = SharedString::from(keys.to_string());
            self
        }
        fn with_category(mut self, category: &str) -> Self {
            self.category = SharedString::from(category.to_string());
            self
        }
    }

    // —— TC-CMP-CMD-01:模糊匹配(子序列 + 评分 + 高亮切段) ——

    #[test]
    fn tc_cmp_cmd_01_fuzzy_match_subsequence_and_score() {
        // 完整/部分/大小写不敏感
        assert!(fuzzy_match("导出", "导出视频").is_some());
        assert!(fuzzy_match("视频", "导出视频").is_some(), "非前缀子序列");
        assert!(fuzzy_match("UNDO", "撤销 undo").is_some(), "大小写不敏感");
        assert!(fuzzy_match("undo", "Undo").is_some());
        // 顺序破坏 = 不命中
        assert!(fuzzy_match("出导", "导出视频").is_none(), "子序列必须按序");
        // 完全不含 = 不命中
        assert!(fuzzy_match("xyz", "撤销").is_none());
        // 空 query = 命中全部、无高亮
        let empty = fuzzy_match("", "任何文本").unwrap();
        assert_eq!(empty.score, 0);
        assert!(empty.indices.is_empty());
        // 命中下标(char 序、升序,供高亮)
        let hit = fuzzy_match("ex", "导出视频 export").unwrap();
        assert_eq!(hit.indices, vec![5, 6], "'export' 的 e/x 下标");
        // 词首奖励:同两字符,命中词首得分更高
        let word_start = fuzzy_match("ex", "export").unwrap();
        let mid_word = fuzzy_match("ex", "aaexport").unwrap();
        assert!(word_start.score > mid_word.score, "词首优先");
        // 连续奖励:连续命中 > 跳跃命中(两串都含 e…x 子序列)
        let contiguous = fuzzy_match("ex", "export").unwrap();
        let skipped = fuzzy_match("ex", "eaxport").unwrap();
        assert!(contiguous.score > skipped.score, "连续段优先");
    }

    #[test]
    fn tc_cmp_cmd_01_label_highlight_segments() {
        let label = "导出视频 export";
        let hit = fuzzy_match("ex", label).unwrap();
        let segments = render_query_label(label, &hit);
        // 段落合并后还原原文
        let joined: String = segments.iter().map(|(text, _)| text.as_str()).collect();
        assert_eq!(joined, label, "切段无损");
        // 命中段恰好是 "ex"(下标 5/6)
        assert_eq!(segments.len(), 3, "未命中(5) + 命中(2) + 未命中(8)");
        assert!(!segments[0].1 && segments[1].1 && !segments[2].1);
        assert_eq!(segments[1].0, "ex");
        // 空 query:单段全否(无高亮)
        let plain = render_query_label(label, &FuzzyMatch::default());
        assert_eq!(plain, vec![(label.to_string(), false)]);
    }

    // —— TC-CMP-CMD-01:最近置顶 + 别名匹配 ——

    #[test]
    fn tc_cmp_cmd_01_recent_first_and_alias_match() {
        let list = entries();
        // 无 query、无最近:注册序
        let ranked = rank_entries(&list, "", &[]);
        let ids: Vec<&str> = ranked.iter().map(|r| list[r.index].id.as_ref()).collect();
        assert_eq!(ids, vec!["undo", "redo", "export", "toggle-timeline"]);
        // 执行 export 后置顶(去重置顶语义)
        let mut recent = VecDeque::new();
        push_recent(&mut recent, "export", PALETTE_RECENT_CAP);
        let recent_slice: Vec<SharedString> = recent.iter().cloned().collect();
        let ranked = rank_entries(&list, "", &recent_slice);
        assert_eq!(list[ranked[0].index].id, "export", "最近执行置顶");
        // 再执行 undo:undo 最新在前
        push_recent(&mut recent, "undo", PALETTE_RECENT_CAP);
        let recent_slice: Vec<SharedString> = recent.iter().cloned().collect();
        let ranked = rank_entries(&list, "", &recent_slice);
        assert_eq!(list[ranked[0].index].id, "undo");
        assert_eq!(list[ranked[1].index].id, "export");
        // 查询过滤:只留命中者
        let ranked = rank_entries(&list, "导出", &recent_slice);
        assert_eq!(ranked.len(), 1);
        assert_eq!(list[ranked[0].index].id, "export");
        // 别名命中(拼音首字母容错槽):dc → export(扣折价,但能命中)
        assert!(
            fuzzy_match("dc", "导出视频").is_none(),
            "中文 label 无拼音表不直配"
        );
        let ranked = rank_entries(&list, "dc", &[]);
        assert_eq!(ranked.len(), 1, "别名命中");
        assert_eq!(list[ranked[0].index].id, "export");
        // 折价:label 直配压过别名命中
        let with_label = vec![
            CommandEntry::new("a", "dc 直接"),
            CommandEntry::new("b", "导出").alias("dc"),
        ];
        let ranked = rank_entries(&with_label, "dc", &[]);
        assert_eq!(with_label[ranked[0].index].id, "a", "直配 > 别名");
    }

    #[test]
    fn tc_cmp_cmd_01_push_recent_dedupes_and_caps() {
        assert_eq!(PALETTE_RECENT_CAP, 8);
        let mut recent = VecDeque::new();
        for i in 0..10 {
            push_recent(&mut recent, &format!("cmd-{i}"), PALETTE_RECENT_CAP);
        }
        assert_eq!(recent.len(), PALETTE_RECENT_CAP, "容量截断");
        assert_eq!(recent.front().map(SharedString::as_ref), Some("cmd-9"));
        assert_eq!(recent.back().map(SharedString::as_ref), Some("cmd-2"));
        // 重复执行:去重置顶,不产生双份
        push_recent(&mut recent, "cmd-5", PALETTE_RECENT_CAP);
        assert_eq!(recent.len(), PALETTE_RECENT_CAP);
        assert_eq!(recent.front().map(SharedString::as_ref), Some("cmd-5"));
        assert_eq!(
            recent.iter().filter(|id| id.as_ref() == "cmd-5").count(),
            1,
            "去重"
        );
    }

    // —— TC-CMP-CMD-01:键盘状态机 ——

    #[test]
    fn tc_cmp_cmd_01_keyboard_state_machine() {
        // ↑↓ 移动高亮、端点钳制不回绕
        assert_eq!(palette_nav(0, 4, "down"), PaletteNav::Highlight(1));
        assert_eq!(
            palette_nav(3, 4, "down"),
            PaletteNav::Highlight(3),
            "末端钳制"
        );
        assert_eq!(palette_nav(2, 4, "up"), PaletteNav::Highlight(1));
        assert_eq!(
            palette_nav(0, 4, "up"),
            PaletteNav::Highlight(0),
            "顶端钳制"
        );
        // Home/End 跳端;Enter 执行高亮;Esc/Tab 关闭
        assert_eq!(palette_nav(2, 4, "home"), PaletteNav::Highlight(0));
        assert_eq!(palette_nav(2, 4, "end"), PaletteNav::Highlight(3));
        assert_eq!(palette_nav(2, 4, "enter"), PaletteNav::Execute(2));
        assert_eq!(palette_nav(2, 4, "escape"), PaletteNav::Close);
        assert_eq!(palette_nav(2, 4, "tab"), PaletteNav::Close);
        // 字符/编辑键无列表意图(走 edit_key 通道)
        assert_eq!(palette_nav(1, 4, "a"), PaletteNav::None);
        assert_eq!(palette_nav(1, 4, "backspace"), PaletteNav::None);
        // 空列表:仅 Esc/Tab 关闭,Enter 不凭空执行
        assert_eq!(palette_nav(0, 0, "escape"), PaletteNav::Close);
        assert_eq!(palette_nav(0, 0, "enter"), PaletteNav::None);
        assert_eq!(palette_nav(0, 0, "down"), PaletteNav::None);
        // 越界高亮钳入界内(宿主未及时归位的防御)
        assert_eq!(palette_nav(9, 4, "down"), PaletteNav::Highlight(3));
        assert_eq!(palette_nav(9, 4, "enter"), PaletteNav::Execute(3));
        // 单项:down 停 0,enter 执行 0
        assert_eq!(palette_nav(0, 1, "down"), PaletteNav::Highlight(0));
        assert_eq!(palette_nav(0, 1, "enter"), PaletteNav::Execute(0));
    }

    #[test]
    fn tc_cmp_cmd_01_edit_channel_reuses_text_field_keymap() {
        // 编辑通道 = text_field::edit_key 单点(键位映射不再第二份):
        // 字符落 Insert、退格/删除/光标移动同 TextField 语义
        let modifiers = gpui::Modifiers::default();
        assert_eq!(
            edit_key("a", &modifiers, Some("a"), false),
            EditKey::Insert('a')
        );
        assert_eq!(
            edit_key("backspace", &modifiers, None, false),
            EditKey::Backspace
        );
        assert_eq!(edit_key("delete", &modifiers, None, false), EditKey::Delete);
        assert_eq!(
            edit_key("left", &modifiers, None, false),
            EditKey::Caret(TextCaretMove::Left, false)
        );
        // Ctrl 组合不落字符(保留给宿主快捷键)
        let ctrl = gpui::Modifiers {
            control: true,
            ..gpui::Modifiers::default()
        };
        assert_eq!(edit_key("k", &ctrl, Some("k"), false), EditKey::Ignored);
        // 查询缓冲(TextFieldBuffer 复用)插入/回退:走一遍 CJK 词汇
        let mut palette = CommandPalette::new();
        palette.set_entries(entries());
        palette.set_query("撤");
        assert_eq!(palette.query(), "撤");
        assert!(palette.query.backspace());
        assert_eq!(palette.query(), "");
        assert!(palette.query.insert("撤"));
        assert!(palette.query.insert("销"));
        assert_eq!(palette.query(), "撤销");
        // 查询驱动命中集
        assert_eq!(palette.visible_entries().len(), 1, "'撤销' 只中 undo");
    }

    // —— TC-CMP-CMD-01:动画相位(下滑 120ms + 背板 80ms;reduced 直切) ——

    #[test]
    fn tc_cmp_cmd_01_anim_durations_slide_down_and_scrim_fade() {
        assert_eq!(
            PALETTE_SLIDE_MS,
            MotionTokens::DUR_STATE_MS,
            "下滑 = STATE 档"
        );
        assert_eq!(
            PALETTE_SCRIM_MS,
            MotionTokens::DUR_HOVER_MS,
            "背板 = HOVER 档"
        );
        assert_eq!(
            PALETTE_SLIDE_PX,
            crate::controls::select::MENU_OPEN_OFFSET_PX,
            "8px 位移单源"
        );
        let mut palette = CommandPalette::new();
        assert!(!palette.needs_frame(), "初始静止");
        assert_eq!(*palette.panel_anim.target(), 0.0);
        assert_eq!(*palette.scrim_anim.target(), 0.0);
        // open:面板目标 1(120ms OutCubic)、背板目标 1(80ms 线性)
        palette
            .panel_anim
            .set(1.0, Duration::from_secs_f64(0.12), Easing::OutCubic);
        palette
            .scrim_anim
            .set(1.0, Duration::from_secs_f64(0.08), Easing::Linear);
        assert_eq!(*palette.panel_anim.target(), 1.0);
        assert_eq!(*palette.scrim_anim.target(), 1.0);
        assert!(palette.needs_frame(), "动画进行中续帧");
        // close:双目标回落 0(begin_close 纯状态步进,无窗口依赖)
        palette.begin_close();
        assert!(!palette.is_open());
        assert_eq!(*palette.panel_anim.target(), 0.0);
        assert_eq!(*palette.scrim_anim.target(), 0.0);
    }

    // —— 数据源:注册表单源 ——

    #[test]
    fn tc_cmp_cmd_01_registry_is_single_source() {
        let registry = KeymapRegistry::new()
            .register_all(crate::keymap::SUGGESTED_CORE_ACTIONS.iter().copied())
            .register(ActionSpec::new("export", "导出", "ctrl-e", "文件"));
        let converted = CommandEntry::from_registry(&registry);
        assert_eq!(converted.len(), 5);
        assert_eq!(converted[0].id, "undo");
        assert_eq!(converted[0].label, "撤销");
        assert_eq!(converted[0].keystroke, "ctrl-z");
        assert_eq!(converted[0].category, "编辑");
        // 键位惯用展示 = chord_display 单源(与 tooltip/keymap 同一份渲染)
        assert_eq!(converted[0].rendered_keystroke(), "Ctrl+Z");
        assert_eq!(converted[4].rendered_keystroke(), "Ctrl+E");
        // 「名称 (快捷键)」行文案 = key_badge_text 单源
        let undo = &converted[0];
        assert_eq!(
            format!("{} ({})", undo.label, undo.rendered_keystroke()),
            crate::controls::tooltip::key_badge_text("撤销", Some("ctrl-z"))
        );
        // 空键位渲染为空串(宿主自标"未绑定"),行文案只名称
        let cycle = converted
            .iter()
            .find(|entry| entry.id == "cycle-tools")
            .unwrap();
        assert_eq!(cycle.rendered_keystroke(), "");
        assert_eq!(cycle.badge_text(), "循环切换工具");
        // 有键位:「名称 (快捷键)」
        assert_eq!(converted[0].badge_text(), "撤销 (Ctrl+Z)");
    }

    // —— 宿主形态与几何(纯数据面) ——

    #[test]
    fn tc_cmp_cmd_01_palette_shape_and_geometry() {
        let palette = CommandPalette::new();
        assert!(!palette.is_open());
        assert_eq!(palette.highlighted(), 0);
        assert_eq!(palette.query(), "");
        assert!(palette.entries().is_empty());
        assert_eq!(palette.resolved_semantic().role(), Some(SemanticRole::List));
        assert_eq!(
            palette.resolved_semantic().label().map(|s| s.as_ref()),
            Some("命令面板")
        );
        assert!(!command_palette_needs_frame(&palette));
        // 行高 = select 下拉行高单点;列表高度 = min(命中, MAX_ROWS) × 行高
        assert_eq!(
            palette_row_height(),
            crate::controls::select::menu_row_height()
        );
        assert_eq!(palette_list_height(0), 0.0);
        assert_eq!(palette_list_height(3), 3.0 * palette_row_height());
        assert_eq!(
            palette_list_height(20),
            PALETTE_MAX_ROWS as f32 * palette_row_height()
        );
        assert_eq!(PALETTE_MAX_ROWS, 8);
        assert_eq!(PALETTE_TOP_PX, 96.0);
        assert_eq!(PALETTE_WIDTH_PX, 560.0);
        // builder 存位
        let built = CommandPalette::new()
            .with_entries(entries())
            .with_on_execute(|_e, _w, _cx| {});
        assert_eq!(built.entries().len(), 4);
        assert!(built.on_execute.is_some());
        let named = CommandPalette::new().label("快速操作");
        assert_eq!(
            named.resolved_semantic().label().map(|s| s.as_ref()),
            Some("快速操作")
        );
    }
}
