//! 壳态持久化契约(RBT-02 / §6 RB-03、RB-08):**原子写 + 坏文件回退默认 +
//! 版本迁移**,库级统一,宿主禁自写。
//!
//! # 宿主契约(硬约束)
//!
//! 壳态(布局/键位/主题/面板开关)落盘与恢复**必须经本契约**,不许宿主
//! `std::fs::write` 直写、不许静默吞错:
//!
//! - **写**:`persist_layout`(传 `&Entity<DockArea>` 一步到位)或
//!   `persist_layout_state`(纯态,无 GPUI 场景)——序列化 + 版本字段归一 +
//!   `sable_foundation::persistence::atomic_write`(tmp + fsync + rename,
//!   临时名含 pid);任何失败返回 `Err(DockError)`,**不留半截文件**;
//! - **读**:`load_layout`——文件不存在 → 默认布局;文件损坏/解析失败 →
//!   默认布局 + **显式表达"已回退"**([`LoadedLayout::Fallback`] + `tracing`
//!   告警,不许静默);版本字段缺失/偏旧 → 迁移链(`migrate_v1_to_current`
//!   桩);版本未知(比库新)→ 回退默认并告警;
//! - **键位/主题等其余壳态**:同样经 `atomic_write` 落盘、解析失败回退默认
//!   并告警(布局之外的字段迁移在各自契约里声明)。
//!
//! 违反形态(上游 VellumBench UI-12 / R-17 教训):多窗口最后写入胜、坏文件
//! 崩壳、壳态无持久化。布局 JSON 复用上游 `DockAreaState` 自带的
//! `version: Option<usize>` 字段([`LAYOUT_VERSION`] = 1,与
//! [`WorkspacePresets::build_workspace`](crate::WorkspacePresets) 传入
//! `DockArea::new(.., Some(1), ..)` 的版本一致),不另造信封结构。
//!
//! # 来源与署名
//!
//! 原子写原语:`sable-foundation::persistence`(V4.0 T7);本模块只做布局
//! 契约封装,未复制任何上游源码。

use std::path::Path;

use gpui::{App, Entity};
use gpui_component::dock::{DockArea, DockAreaState};

use crate::error::DockResult;

/// 布局 JSON 的当前格式版本(**v1**)。
///
/// 写入端([`persist_layout`] / [`persist_layout_state`])把
/// `DockAreaState::version` 归一为 `Some(LAYOUT_VERSION)`;读取端按
/// [`load_layout`] 的版本分派:等于 → 直接恢复;缺失/偏旧 → 迁移;
/// 高于 → 回退默认。
pub const LAYOUT_VERSION: usize = 1;

/// 布局持久化(宿主一步到位):`DockArea::dump` 序列化当前布局 → 版本归一 →
/// 原子写落盘。
///
/// 失败(序列化/IO)返回 `Err(DockError)`;原子写语义保证失败不留半截文件、
/// 成功则目标要么是完整旧文件要么是完整新文件。宿主可
/// `cx.subscribe(&area, ..)` 监听上游 `DockEvent::LayoutChanged` 后调用本函数
/// 自动保存(多窗口各自持久化各自的布局文件,禁止多窗口写同一路径——
/// 最后写入胜是上游教训 UI-12)。
pub fn persist_layout(path: &Path, area: &Entity<DockArea>, cx: &App) -> DockResult<()> {
    let state = area.read(cx).dump(cx);
    persist_layout_state(path, &state)
}

/// 布局持久化(纯态版):把已序列化面的 [`DockAreaState`] 原子写落盘。
///
/// 与 [`persist_layout`] 的差异仅在布局来源:本函数不要求 GPUI 上下文,
/// 供无窗口场景(测试、宿主自持状态)使用。版本字段统一归一为
/// [`LAYOUT_VERSION`](即使传入 `version: None`,落盘文件也带 `v1` 标记)。
pub fn persist_layout_state(path: &Path, state: &DockAreaState) -> DockResult<()> {
    let mut versioned = state.clone();
    versioned.version = Some(LAYOUT_VERSION);
    let json = serde_json::to_string(&versioned)?;
    sable_foundation::persistence::atomic_write(path, json.as_bytes())?;
    Ok(())
}

/// [`load_layout`] 的结果:显式区分"从文件恢复"与"已回退默认"
/// (§6 RB-08:回退不许静默)。
///
/// 只要布局、不关心来源时用 [`LoadedLayout::into_layout`];宿主**应当**检查
/// [`LoadedLayout::is_fallback`] 并向用户告警(库侧已 `tracing` 记录)。
#[derive(Debug, Clone, PartialEq)]
pub enum LoadedLayout {
    /// 从文件恢复。`migrated_from` 为迁移前版本:`None` = 文件版本即当前
    /// ([`LAYOUT_VERSION`]);`Some(from)` = 旧版本经迁移链升到当前
    /// (`Some(0)` = 旧版 `save_layout` 产物,无版本字段)。
    Restored {
        /// 恢复出的布局(已迁到当前结构)。
        layout: DockAreaState,
        /// 迁移前版本;`None` 表示无需迁移。
        migrated_from: Option<usize>,
    },
    /// 已回退到默认布局(`DockAreaState::default()`)。`reason` 说明原因,
    /// 供宿主告警/上报。
    Fallback {
        /// 回退原因。
        reason: FallbackReason,
        /// 默认布局。
        layout: DockAreaState,
    },
}

impl LoadedLayout {
    /// 布局本体(借用;不区分恢复/回退)。
    pub fn layout(&self) -> &DockAreaState {
        match self {
            LoadedLayout::Restored { layout, .. } | LoadedLayout::Fallback { layout, .. } => layout,
        }
    }

    /// 消耗自身取布局(只要布局、不关心来源时的便捷出口)。
    pub fn into_layout(self) -> DockAreaState {
        match self {
            LoadedLayout::Restored { layout, .. } | LoadedLayout::Fallback { layout, .. } => layout,
        }
    }

    /// 是否发生了回退(RB-08 的"已回退"可观测口)。
    pub fn is_fallback(&self) -> bool {
        matches!(self, LoadedLayout::Fallback { .. })
    }

    /// 回退原因(仅 [`LoadedLayout::Fallback`] 时为 `Some`)。
    pub fn fallback_reason(&self) -> Option<&FallbackReason> {
        match self {
            LoadedLayout::Restored { .. } => None,
            LoadedLayout::Fallback { reason, .. } => Some(reason),
        }
    }

    /// 回退结果构造(默认布局 + 原因)。
    fn fallback(reason: FallbackReason) -> Self {
        LoadedLayout::Fallback {
            reason,
            layout: DockAreaState::default(),
        }
    }
}

/// 回退默认布局的原因(RB-08 告警载荷)。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FallbackReason {
    /// 布局文件不存在(首次启动属正常;库侧 `info` 级记录)。
    #[error("布局文件不存在,使用默认布局")]
    FileMissing,

    /// 文件存在但 JSON 损坏/解析失败(库侧 `warn` 级告警)。
    #[error("布局文件损坏(JSON 解析失败:{detail}),已回退默认布局")]
    CorruptJson {
        /// serde 的原始错误信息(诊断用)。
        detail: String,
    },

    /// 文件版本高于当前库支持的版本,无法向前迁移(库侧 `warn` 级告警;
    /// 典型场景:新版软件写的布局被旧版软件读到)。
    #[error("布局版本 {found} 高于当前支持的版本 {supported},已回退默认布局")]
    UnknownVersion {
        /// 文件里的版本号。
        found: usize,
        /// 本库支持的最新版本([`LAYOUT_VERSION`])。
        supported: usize,
    },
}

/// 从文件恢复布局(壳态持久化契约的读半边)。
///
/// 版本分派:
///
/// | 文件形态 | 结果 |
/// |---|---|
/// | 不存在 | `Ok(Fallback { FileMissing, 默认布局 })`(首次启动,`info`) |
/// | JSON 损坏/解析失败 | `Ok(Fallback { CorruptJson, 默认布局 })` + `warn` |
/// | `version = 1`(当前) | `Ok(Restored { migrated_from: None })` |
/// | `version` 缺失/`null`/偏旧 | `Ok(Restored { migrated_from: Some(from) })`(迁移链,`info`) |
/// | `version` 高于 [`LAYOUT_VERSION`] | `Ok(Fallback { UnknownVersion, 默认布局 })` + `warn` |
/// | 读文件 IO 失败(权限/占用) | `Err(DockError::Io)`(数据不可读 ≠ 损坏,交宿主决策) |
///
/// 同目录残留的 `*.tmp` 半截文件(写中断产物)不是目标文件,本函数只读
/// 目标路径,天然不受影响(原子写契约的读端保证)。
pub fn load_layout(path: &Path) -> DockResult<LoadedLayout> {
    if !path.exists() {
        tracing::info!(
            path = %path.display(),
            "布局文件不存在,使用默认布局(首次启动属正常)"
        );
        return Ok(LoadedLayout::fallback(FallbackReason::FileMissing));
    }
    let text = std::fs::read_to_string(path)?;

    let state: DockAreaState = match serde_json::from_str(&text) {
        Ok(state) => state,
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                detail = %err,
                "布局文件损坏,已回退默认布局"
            );
            return Ok(LoadedLayout::fallback(FallbackReason::CorruptJson {
                detail: err.to_string(),
            }));
        }
    };

    match state.version {
        // 当前版本:直接恢复(迁移桩恒等,仍过一遍以为未来留结构)。
        Some(v) if v == LAYOUT_VERSION => Ok(LoadedLayout::Restored {
            layout: migrate_v1_to_current(state),
            migrated_from: None,
        }),
        // 比库还新的版本:无法向前迁移,回退默认并告警(不许静默)。
        Some(found) if found > LAYOUT_VERSION => {
            tracing::warn!(
                path = %path.display(),
                found,
                supported = LAYOUT_VERSION,
                "布局文件版本高于当前支持,已回退默认布局"
            );
            Ok(LoadedLayout::fallback(FallbackReason::UnknownVersion {
                found,
                supported: LAYOUT_VERSION,
            }))
        }
        // 旧版本(v1 之前:version 缺失 / null / 0):走迁移链升到当前。
        // 旧版 save_layout 产物无版本字段,serde 经 #[serde(default)] 读成
        // None,记为版本 0;未来出现其它 <v1 的历史版本时在此分派。
        other => {
            let from = other.unwrap_or_default();
            tracing::info!(
                path = %path.display(),
                from,
                to = LAYOUT_VERSION,
                "布局文件为旧版本,已迁移到当前版本"
            );
            Ok(LoadedLayout::Restored {
                layout: migrate_v1_to_current(state),
                migrated_from: Some(from),
            })
        }
    }
}

/// 迁移函数桩:**v1 → 当前结构**。当前恒等(v1 载荷与现结构同构)。
///
/// 结构上预留:未来 `DockAreaState` 演进时,v1 固化为快照结构(该版本文件
/// 仍须可读),本函数改为做字段搬运;若出现 v2+,在 [`load_layout`] 的版本
/// 分派里按 `v1 → v2 → … → 当前` 逐级挂链。**迁移只向前,不向后**;无法
/// 迁移的版本一律回退默认并告警。
fn migrate_v1_to_current(state: DockAreaState) -> DockAreaState {
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 迁移桩恒等性(结构预留的冒烟;契约路径断言在
    /// `tests/tc_rbt_persist_01.rs`,只用公开 API)。
    #[test]
    fn migrate_v1_is_identity() {
        let state = DockAreaState::default();
        assert_eq!(migrate_v1_to_current(state.clone()), state);
    }

    /// 版本常量与上游 `DockArea::new(.., Some(1), ..)` 的约定一致。
    #[test]
    fn layout_version_is_v1() {
        assert_eq!(LAYOUT_VERSION, 1);
    }
}
