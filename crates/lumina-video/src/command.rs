//! 时间轴命令系统:撤销重做的唯一正解(docs/04 §8,范式同 docs/03 §2)。
//!
//! 铁律(与 AGENTS.md §3.1 同源):**一切时间轴修改必须走 [`TimelineCommand`]。**
//!
//! # 与 lumina-core Command/History 的关系(架构决策,详见 crate 文档)
//!
//! 同构而独立:core 的 History 为 slotmap 场景图内置 id 治愈(`remap_ids`/
//! `IdRemap` 广播)。时间轴 [`crate::model::ClipId`] 稳定自增**永不复用**,因此:
//!
//! - `revert` 无返回值——撤销不需要任何 id 重映射;
//! - 历史条目的失效引用 = 目标已删除 = 静默跳过(core 同款 LIFO 约定),
//!   且**永不误伤别的 clip**(id 不复用保证);
//! - 涉及新 id 的命令(PlaceClip/SplitClip)在 apply 时缓存铸造结果,redo
//!   复用同一 id。
//!
//! `merge` 合并范式保留 docs/03 §2 语义:同 clip 连续移动合并为一步撤销,
//! 保留最早的 old、最新的 new。有状态命令一律 **apply 时捕获旧值**(而非
//! 构造时),构造只表达意图。

use std::any::Any;
use std::borrow::Cow;
use std::fmt;

use crate::model::{AssetRef, Clip, ClipId, Timeline};

/// 命令 = 一次可逆的时间轴修改。
pub trait TimelineCommand: Send + 'static {
    /// 正向执行。目标不存在/非法时静默跳过(LIFO 约定下不应发生;发生即漂移异常)。
    fn apply(&mut self, timeline: &mut Timeline);

    /// 逆向执行(撤销)。ClipId 永不复用,无需返回 id 重映射。
    fn revert(&mut self, timeline: &mut Timeline);

    /// 命令合并(连续拖动 → 合并为一步撤销;保留最早的 old、最新的 new)。
    fn merge(&self, next: &dyn TimelineCommand) -> Option<Box<dyn TimelineCommand>> {
        let _ = next;
        None
    }

    /// `merge` 里 downcast 用,必填。
    fn as_any(&self) -> &dyn Any;

    /// 命令名(历史面板/日志用)。
    fn name(&self) -> Cow<'static, str>;

    /// 装箱为 trait object。
    fn boxed(self) -> Box<dyn TimelineCommand>
    where
        Self: Sized,
    {
        Box::new(self)
    }
}

/// 批量事务命令:一次 `begin_transaction`..`end_transaction` 之间执行的全部
/// 命令,撤销/重做各算一步。
pub struct BatchCommand(pub Vec<Box<dyn TimelineCommand>>);

impl fmt::Debug for BatchCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BatchCommand")
            .field("steps", &self.0.len())
            .finish()
    }
}

impl TimelineCommand for BatchCommand {
    fn apply(&mut self, timeline: &mut Timeline) {
        for cmd in &mut self.0 {
            cmd.apply(timeline);
        }
    }

    fn revert(&mut self, timeline: &mut Timeline) {
        for cmd in self.0.iter_mut().rev() {
            cmd.revert(timeline);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("批量操作")
    }
}

/// 历史栈:时间轴撤销/重做的唯一入口。
#[derive(Default)]
pub struct TimelineHistory {
    undo: Vec<Box<dyn TimelineCommand>>,
    redo: Vec<Box<dyn TimelineCommand>>,
    /// 批量事务:如"对齐 5 个 clip" = 一步撤销
    transaction: Option<Vec<Box<dyn TimelineCommand>>>,
}

impl fmt::Debug for TimelineHistory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TimelineHistory")
            .field("undo_len", &self.undo.len())
            .field("redo_len", &self.redo.len())
            .field("in_transaction", &self.transaction.is_some())
            .finish()
    }
}

impl TimelineHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// 执行一条命令:先 apply;事务内只收集,否则尝试与栈顶合并,并清空 redo。
    pub fn exec(&mut self, mut cmd: Box<dyn TimelineCommand>, timeline: &mut Timeline) {
        cmd.apply(timeline);
        match &mut self.transaction {
            Some(batch) => batch.push(cmd),
            None => {
                // 尝试与栈顶合并(连续拖动场景)
                let merged = self.undo.last().and_then(|top| top.merge(&*cmd));
                if let Some(merged) = merged {
                    self.undo.pop();
                    self.undo.push(merged);
                } else {
                    self.undo.push(cmd);
                }
                self.redo.clear();
            }
        }
    }

    /// 撤销一步。
    pub fn undo(&mut self, timeline: &mut Timeline) {
        if let Some(mut cmd) = self.undo.pop() {
            cmd.revert(timeline);
            self.redo.push(cmd);
        }
    }

    /// 重做一步(按撤销的逆序逐条重放)。
    pub fn redo(&mut self, timeline: &mut Timeline) {
        if let Some(mut cmd) = self.redo.pop() {
            cmd.apply(timeline);
            self.undo.push(cmd);
        }
    }

    /// 开始批量事务(嵌套调用被忽略,不丢已收集的命令)。
    pub fn begin_transaction(&mut self) {
        if self.transaction.is_none() {
            self.transaction = Some(Vec::new());
        }
    }

    /// 结束批量事务;空事务不产生撤销步。
    pub fn end_transaction(&mut self) {
        if let Some(batch) = self.transaction.take() {
            if !batch.is_empty() {
                self.undo.push(Box::new(BatchCommand(batch)));
                self.redo.clear();
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// 撤销栈深度(自动保存计数用)。
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// 清空全部历史(如"打开新工程")。
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.transaction = None;
    }
}

// —— 内置命令集 ——

/// 放置 clip(入点 0、速度 1)。apply 时铸造 id 并缓存;redo 复用同一 id
/// (ClipId 永不复用,撤销/重做循环后身份不变)。
#[derive(Debug)]
pub struct PlaceClip {
    pub track: usize,
    pub asset: AssetRef,
    pub start_ms: u64,
    pub duration_ms: u64,
    /// apply 后为实际生效的 clip id(放置失败时保持 None,撤销为空操作)。
    pub id: Option<ClipId>,
}

impl TimelineCommand for PlaceClip {
    fn apply(&mut self, timeline: &mut Timeline) {
        match self.id {
            None => match timeline.place_clip(
                self.track,
                self.asset.clone(),
                self.start_ms,
                self.duration_ms,
            ) {
                Ok(id) => self.id = Some(id),
                Err(e) => {
                    tracing::warn!(error = %e, track = self.track, "PlaceClip apply 失败,跳过")
                }
            },
            Some(id) => {
                // redo:clip 已被 revert 摘除,用缓存的 id 确定性放回
                if let Err(e) = timeline.place_clip_with_id(
                    self.track,
                    self.asset.clone(),
                    self.start_ms,
                    self.duration_ms,
                    id,
                ) {
                    tracing::warn!(error = %e, "PlaceClip redo 失败,跳过");
                }
            }
        }
    }

    fn revert(&mut self, timeline: &mut Timeline) {
        if let Some(id) = self.id {
            if let Err(e) = timeline.remove_clip(id) {
                tracing::warn!(error = %e, clip = id.value(), "PlaceClip revert 失败,跳过");
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("放置 clip")
    }
}

/// 删除 clip(摘除但不补位;补位版见 [`RemoveClipRipple`])。
#[derive(Debug)]
pub struct RemoveClip {
    pub id: ClipId,
    /// apply 时捕获的轨道下标(revert 放回用)。
    pub track: Option<usize>,
    /// apply 时捕获的完整 clip(undo 逐字段恢复)。
    pub clip: Option<Clip>,
}

impl TimelineCommand for RemoveClip {
    fn apply(&mut self, timeline: &mut Timeline) {
        if self.clip.is_some() {
            // redo:clip 已由 revert 放回,再次摘除
            if let Err(e) = timeline.remove_clip(self.id) {
                tracing::warn!(error = %e, "RemoveClip redo 失败,跳过");
            }
        } else if let Some((track, _)) = timeline.locate(self.id) {
            match timeline.remove_clip(self.id) {
                Ok(clip) => {
                    self.track = Some(track);
                    self.clip = Some(clip);
                }
                Err(e) => tracing::warn!(error = %e, "RemoveClip apply 失败,跳过"),
            }
        } else {
            tracing::warn!(clip = self.id.value(), "RemoveClip:目标不存在,跳过");
        }
    }

    fn revert(&mut self, timeline: &mut Timeline) {
        if let (Some(track), Some(clip)) = (self.track, self.clip.clone()) {
            if let Err(err) = timeline.insert_clip_raw(track, clip) {
                let (_, e) = *err;
                tracing::warn!(error = %e, "RemoveClip revert 失败,跳过");
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("删除 clip")
    }
}

/// 移动 clip 起点(吸附在外部做,命令只管碰撞)。
/// merge:同 clip 连续移动合并为一步,保留最早的 from、最新的 to。
#[derive(Debug)]
pub struct MoveClip {
    pub id: ClipId,
    /// apply 时捕获的原始起点。
    pub from_ms: Option<u64>,
    pub to_ms: u64,
}

impl TimelineCommand for MoveClip {
    fn apply(&mut self, timeline: &mut Timeline) {
        if self.from_ms.is_none() {
            self.from_ms = timeline.clip(self.id).map(|c| c.start_ms);
        }
        if let Err(e) = timeline.move_clip(self.id, self.to_ms) {
            tracing::warn!(error = %e, clip = self.id.value(), "MoveClip apply 失败,跳过");
        }
    }

    fn revert(&mut self, timeline: &mut Timeline) {
        if let Some(from) = self.from_ms {
            if let Err(e) = timeline.move_clip(self.id, from) {
                tracing::warn!(error = %e, clip = self.id.value(), "MoveClip revert 失败,跳过");
            }
        }
    }

    fn merge(&self, next: &dyn TimelineCommand) -> Option<Box<dyn TimelineCommand>> {
        let next = next.as_any().downcast_ref::<MoveClip>()?;
        if next.id != self.id {
            return None;
        }
        Some(Box::new(MoveClip {
            id: self.id,
            from_ms: self.from_ms,
            to_ms: next.to_ms,
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("移动 clip")
    }
}

/// 裁剪素材出入点(duration 按 (out-in)/speed 同步)。
#[derive(Debug)]
pub struct TrimClip {
    pub id: ClipId,
    pub new_in_ms: u64,
    pub new_out_ms: u64,
    /// apply 时捕获的旧 (in, out)。
    pub old: Option<(u64, u64)>,
}

impl TimelineCommand for TrimClip {
    fn apply(&mut self, timeline: &mut Timeline) {
        if self.old.is_none() {
            self.old = timeline.clip(self.id).map(|c| (c.in_ms, c.out_ms));
        }
        if let Err(e) = timeline.trim_clip(self.id, self.new_in_ms, self.new_out_ms) {
            tracing::warn!(error = %e, clip = self.id.value(), "TrimClip apply 失败,跳过");
        }
    }

    fn revert(&mut self, timeline: &mut Timeline) {
        if let Some((in_ms, out_ms)) = self.old {
            if let Err(e) = timeline.trim_clip(self.id, in_ms, out_ms) {
                tracing::warn!(error = %e, clip = self.id.value(), "TrimClip revert 失败,跳过");
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("裁剪 clip")
    }
}

/// 分割 clip:左半保留原 id,右半铸造新 id(apply 时缓存,redo 复用)。
#[derive(Debug)]
pub struct SplitClip {
    pub track: usize,
    pub ms: u64,
    /// apply 时捕获的分割前原 clip(撤销时摘除两半、放回原 clip)。
    pub original: Option<Clip>,
    /// apply 时捕获的右半新 id。
    pub right_id: Option<ClipId>,
}

impl TimelineCommand for SplitClip {
    fn apply(&mut self, timeline: &mut Timeline) {
        match self.right_id {
            None => {
                // 首次:先捕获分割点所在的 clip(分割会原地修改它),再分割
                if let Some(id) = timeline.clip_at(self.track, self.ms) {
                    let before = timeline.clip(id).cloned();
                    if let Ok((_left, right)) = timeline.split_at(self.track, self.ms) {
                        self.original = before;
                        self.right_id = Some(right);
                    }
                } else {
                    tracing::warn!(
                        track = self.track,
                        ms = self.ms,
                        "SplitClip:分割点无 clip,跳过"
                    );
                }
            }
            Some(right_id) => {
                // redo:右半 id 已定,确定性重放
                if let Err(e) = timeline.split_at_with_ids(self.track, self.ms, right_id) {
                    tracing::warn!(error = %e, "SplitClip redo 失败,跳过");
                }
            }
        }
    }

    fn revert(&mut self, timeline: &mut Timeline) {
        let (Some(original), Some(right_id)) = (self.original.clone(), self.right_id) else {
            return; // apply 从未成功:空操作
        };
        if let Err(e) = timeline.remove_clip(original.id) {
            tracing::warn!(error = %e, "SplitClip revert 摘左半失败,跳过");
        }
        if let Err(e) = timeline.remove_clip(right_id) {
            tracing::warn!(error = %e, "SplitClip revert 摘右半失败,跳过");
        }
        if let Err(err) = timeline.insert_clip_raw(self.track, original) {
            let (_, e) = *err;
            tracing::warn!(error = %e, "SplitClip revert 放回原 clip 失败,跳过");
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("分割 clip")
    }
}

/// 波纹删除:删除 clip 且同轨后续 clip 前移补位。
#[derive(Debug)]
pub struct RemoveClipRipple {
    pub id: ClipId,
    /// apply 时捕获的轨道下标。
    pub track: Option<usize>,
    /// apply 时捕获的完整 clip。
    pub removed: Option<Clip>,
}

impl TimelineCommand for RemoveClipRipple {
    fn apply(&mut self, timeline: &mut Timeline) {
        if self.removed.is_some() {
            // redo:波纹量(被删时长)不变,结果确定性一致
            if let Err(e) = timeline.ripple_remove(self.id) {
                tracing::warn!(error = %e, "RemoveClipRipple redo 失败,跳过");
            }
        } else if let Some((track, _)) = timeline.locate(self.id) {
            match timeline.ripple_remove(self.id) {
                Ok(clip) => {
                    self.track = Some(track);
                    self.removed = Some(clip);
                }
                Err(e) => tracing::warn!(error = %e, "RemoveClipRipple apply 失败,跳过"),
            }
        } else {
            tracing::warn!(clip = self.id.value(), "RemoveClipRipple:目标不存在,跳过");
        }
    }

    fn revert(&mut self, timeline: &mut Timeline) {
        if let (Some(track), Some(clip)) = (self.track, self.removed.clone()) {
            timeline.ripple_restore(track, clip);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("波纹删除 clip")
    }
}

/// 变速(duration 按 (out-in)/speed 重算)。
#[derive(Debug)]
pub struct SetSpeed {
    pub id: ClipId,
    pub new_speed: f64,
    /// apply 时捕获的旧速度。
    pub old_speed: Option<f64>,
}

impl TimelineCommand for SetSpeed {
    fn apply(&mut self, timeline: &mut Timeline) {
        if self.old_speed.is_none() {
            self.old_speed = timeline.clip(self.id).map(|c| c.speed);
        }
        if let Err(e) = timeline.set_speed(self.id, self.new_speed) {
            tracing::warn!(error = %e, clip = self.id.value(), "SetSpeed apply 失败,跳过");
        }
    }

    fn revert(&mut self, timeline: &mut Timeline) {
        if let Some(old) = self.old_speed {
            if let Err(e) = timeline.set_speed(self.id, old) {
                tracing::warn!(error = %e, clip = self.id.value(), "SetSpeed revert 失败,跳过");
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("变速")
    }
}

/// 轨道静音开关。注意:v0.1 轨道以**下标**寻址且轨道增删不走撤销,
/// 历史里的轨道下标在用户直接增删轨道后可能失效(失效即静默跳过,不会误伤)。
#[derive(Debug)]
pub struct SetMuted {
    pub track: usize,
    pub muted: bool,
    /// apply 时捕获的旧值。
    pub old: Option<bool>,
}

impl TimelineCommand for SetMuted {
    fn apply(&mut self, timeline: &mut Timeline) {
        let Some(t) = timeline.tracks.get_mut(self.track) else {
            tracing::warn!(track = self.track, "SetMuted:轨道不存在,跳过");
            return;
        };
        if self.old.is_none() {
            self.old = Some(t.muted);
        }
        t.muted = self.muted;
    }

    fn revert(&mut self, timeline: &mut Timeline) {
        if let (Some(old), Some(t)) = (self.old, timeline.tracks.get_mut(self.track)) {
            t.muted = old;
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("轨道静音")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Easing, Keyframe, TrackKind};
    use proptest::prelude::*;

    fn asset(name: &str) -> AssetRef {
        AssetRef::new(format!("/assets/{name}.mp4"), 42)
    }

    /// 两轨:0=Video、1=Audio;轨0 [0,500)+[500,1000),轨1 [200,600)。
    fn demo_timeline() -> (Timeline, Vec<ClipId>) {
        let mut tl = Timeline::new();
        tl.add_track(TrackKind::Video);
        tl.add_track(TrackKind::Audio);
        let a = tl.place_clip(0, asset("v1"), 0, 500).expect("place v1");
        let b = tl.place_clip(0, asset("v2"), 500, 500).expect("place v2");
        let c = tl.place_clip(1, asset("a1"), 200, 400).expect("place a1");
        (tl, vec![a, b, c])
    }

    fn ids_of(tl: &Timeline) -> Vec<ClipId> {
        tl.tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.id))
            .collect()
    }

    /// 铸造一个 demo_timeline(占 0..3)里不存在的 clip id(ClipId 外部不可构造)。
    fn foreign_id() -> ClipId {
        let mut tl = Timeline::new();
        tl.add_track(TrackKind::Video);
        for i in 0..4u64 {
            tl.place_clip(0, asset("x"), i * 1000, 100).expect("place");
        }
        tl.tracks[0].clips[3].id
    }

    #[test]
    fn place_clip_command_roundtrip() {
        let (mut tl, _) = demo_timeline();
        let before = tl.clone();
        let before_ids = ids_of(&tl);
        let mut h = TimelineHistory::new();

        h.exec(
            PlaceClip {
                track: 0,
                asset: asset("new"),
                start_ms: 1500,
                duration_ms: 300,
                id: None,
            }
            .boxed(),
            &mut tl,
        );
        assert_eq!(h.undo_len(), 1);
        assert_eq!(tl.duration(), 1800);
        let added = ids_of(&tl)
            .into_iter()
            .find(|id| !before_ids.contains(id))
            .expect("新增 clip");

        h.undo(&mut tl);
        assert_eq!(tl, before, "撤销后回到放置前");
        assert_eq!(tl.clip(added), None);

        h.redo(&mut tl);
        let clip = tl.clip(added).expect("redo 后同一 id 回归");
        assert_eq!(clip.start_ms, 1500, "ClipId 永不复用:redo 保持身份");

        h.undo(&mut tl);
        assert_eq!(tl, before);
    }

    #[test]
    fn remove_clip_command_roundtrip() {
        let (mut tl, ids) = demo_timeline();
        let before = tl.clone();
        let b = ids[1];
        let mut h = TimelineHistory::new();

        h.exec(
            RemoveClip {
                id: b,
                track: None,
                clip: None,
            }
            .boxed(),
            &mut tl,
        );
        assert_eq!(tl.clip(b), None);
        assert_eq!(tl.duration(), 600);

        h.undo(&mut tl);
        assert_eq!(tl, before, "摘除的 clip 逐字段恢复");

        h.redo(&mut tl);
        assert_eq!(tl.clip(b), None);

        h.undo(&mut tl);
        assert_eq!(tl, before);
    }

    #[test]
    fn move_clip_command_roundtrip() {
        let (mut tl, ids) = demo_timeline();
        let before = tl.clone();
        let b = ids[1]; // [500,1000)
        let mut h = TimelineHistory::new();

        h.exec(
            MoveClip {
                id: b,
                from_ms: None,
                to_ms: 1500,
            }
            .boxed(),
            &mut tl,
        );
        assert_eq!(tl.clip(b).expect("b").start_ms, 1500);
        assert_eq!(tl.duration(), 2000);

        h.undo(&mut tl);
        assert_eq!(tl, before);

        h.redo(&mut tl);
        assert_eq!(tl.clip(b).expect("b").start_ms, 1500);

        h.undo(&mut tl);
        assert_eq!(tl, before);
    }

    #[test]
    fn move_clip_merge_collapses_to_one_step() {
        let (mut tl, ids) = demo_timeline();
        let b = ids[1];
        let mut h = TimelineHistory::new();

        h.exec(
            MoveClip {
                id: b,
                from_ms: None,
                to_ms: 1200,
            }
            .boxed(),
            &mut tl,
        );
        h.exec(
            MoveClip {
                id: b,
                from_ms: None,
                to_ms: 1600,
            }
            .boxed(),
            &mut tl,
        );
        assert_eq!(h.undo_len(), 1, "同 clip 连续移动合并为一步");
        assert_eq!(tl.clip(b).expect("b").start_ms, 1600);

        h.undo(&mut tl);
        assert_eq!(
            tl.clip(b).expect("b").start_ms,
            500,
            "保留最早的 from:一步回到最初"
        );

        h.redo(&mut tl);
        assert_eq!(tl.clip(b).expect("b").start_ms, 1600, "保留最新的 to");

        // 不同 clip 不合并
        let a = ids[0];
        h.exec(
            MoveClip {
                id: a,
                from_ms: None,
                to_ms: 2500,
            }
            .boxed(),
            &mut tl,
        );
        assert_eq!(h.undo_len(), 2);
    }

    #[test]
    fn trim_clip_command_roundtrip() {
        let (mut tl, ids) = demo_timeline();
        let before = tl.clone();
        let b = ids[1]; // in=0 out=500
        let mut h = TimelineHistory::new();

        h.exec(
            TrimClip {
                id: b,
                new_in_ms: 100,
                new_out_ms: 400,
                old: None,
            }
            .boxed(),
            &mut tl,
        );
        let clip = tl.clip(b).expect("b");
        assert_eq!((clip.in_ms, clip.out_ms, clip.duration_ms), (100, 400, 300));

        h.undo(&mut tl);
        assert_eq!(tl, before);

        h.redo(&mut tl);
        assert_eq!(tl.clip(b).expect("b").duration_ms, 300);

        h.undo(&mut tl);
        assert_eq!(tl, before);
    }

    #[test]
    fn split_clip_command_roundtrip() {
        let (mut tl, ids) = demo_timeline();
        let b = ids[1];
        // 给 b 加关键帧,验证 undo 后逐字段(含关键帧)一致
        let (t, i) = tl.locate(b).expect("b in track");
        tl.tracks[t].clips[i].keyframes = vec![
            Keyframe {
                t_ms: 0,
                value: 1.0,
                easing: Easing::Linear,
            },
            Keyframe {
                t_ms: 400,
                value: 2.0,
                easing: Easing::InOutCubic,
            },
        ];
        let before = tl.clone();
        let before_ids = ids_of(&tl);
        let mut h = TimelineHistory::new();

        h.exec(
            SplitClip {
                track: 0,
                ms: 700,
                original: None,
                right_id: None,
            }
            .boxed(),
            &mut tl,
        );
        assert_eq!(h.undo_len(), 1);
        assert_eq!(tl.tracks[0].clips.len(), 3, "一分为二");
        let right = ids_of(&tl)
            .into_iter()
            .find(|id| !before_ids.contains(id))
            .expect("右半新 id");

        h.undo(&mut tl);
        assert_eq!(tl, before, "两半合并回原 clip,含关键帧逐字段一致");
        assert_eq!(tl.tracks[0].clips.len(), 2);

        h.redo(&mut tl);
        assert_eq!(tl.tracks[0].clips.len(), 3);
        let r = tl.clip(right).expect("redo 后右半保持同一 id");
        assert_eq!(r.in_ms, 200);

        h.undo(&mut tl);
        assert_eq!(tl, before);
    }

    #[test]
    fn ripple_remove_command_roundtrip() {
        let (mut tl, ids) = demo_timeline();
        let before = tl.clone();
        let (a, b) = (ids[0], ids[1]);
        let mut h = TimelineHistory::new();

        h.exec(
            RemoveClipRipple {
                id: a,
                track: None,
                removed: None,
            }
            .boxed(),
            &mut tl,
        );
        assert_eq!(tl.clip(a), None);
        assert_eq!(tl.clip(b).expect("b").start_ms, 0, "后续前移补位,无空洞");
        assert_eq!(tl.duration(), 600);

        h.undo(&mut tl);
        assert_eq!(tl, before, "波纹还原:a 回到 0,b 回到 500");

        h.redo(&mut tl);
        assert_eq!(tl.clip(b).expect("b").start_ms, 0);

        h.undo(&mut tl);
        assert_eq!(tl, before);
    }

    #[test]
    fn set_speed_command_roundtrip() {
        let (mut tl, ids) = demo_timeline();
        let before = tl.clone();
        let b = ids[1];
        let mut h = TimelineHistory::new();

        h.exec(
            SetSpeed {
                id: b,
                new_speed: 2.0,
                old_speed: None,
            }
            .boxed(),
            &mut tl,
        );
        assert_eq!(tl.clip(b).expect("b").speed, 2.0);
        assert_eq!(tl.clip(b).expect("b").duration_ms, 250);

        h.undo(&mut tl);
        assert_eq!(tl, before);

        h.redo(&mut tl);
        assert_eq!(tl.clip(b).expect("b").duration_ms, 250);

        h.undo(&mut tl);
        assert_eq!(tl, before);
    }

    #[test]
    fn set_muted_command_roundtrip() {
        let (mut tl, _) = demo_timeline();
        let before = tl.clone();
        let mut h = TimelineHistory::new();

        h.exec(
            SetMuted {
                track: 1,
                muted: true,
                old: None,
            }
            .boxed(),
            &mut tl,
        );
        assert!(tl.tracks[1].muted);

        h.undo(&mut tl);
        assert_eq!(tl, before);
        assert!(!tl.tracks[1].muted);

        h.redo(&mut tl);
        assert!(tl.tracks[1].muted);

        h.undo(&mut tl);
        assert_eq!(tl, before);
    }

    #[test]
    fn failed_apply_is_silent_noop_and_undo_safe() {
        let (mut tl, ids) = demo_timeline();
        let before = tl.clone();
        let mut h = TimelineHistory::new();

        // 放不下(撞 a)
        h.exec(
            PlaceClip {
                track: 0,
                asset: asset("x"),
                start_ms: 100,
                duration_ms: 100,
                id: None,
            }
            .boxed(),
            &mut tl,
        );
        // 目标不存在
        h.exec(
            MoveClip {
                id: foreign_id(),
                from_ms: None,
                to_ms: 50,
            }
            .boxed(),
            &mut tl,
        );
        // out <= in
        h.exec(
            TrimClip {
                id: ids[1],
                new_in_ms: 400,
                new_out_ms: 100,
                old: None,
            }
            .boxed(),
            &mut tl,
        );
        // 分割点在端点
        h.exec(
            SplitClip {
                track: 0,
                ms: 500,
                original: None,
                right_id: None,
            }
            .boxed(),
            &mut tl,
        );

        assert_eq!(tl, before, "全部失败命令 apply 后时间轴原状");
        while h.can_undo() {
            h.undo(&mut tl);
        }
        assert_eq!(tl, before, "失败命令的撤销链全部为无害空操作");
    }

    #[test]
    fn transaction_is_one_undo_step() {
        let (mut tl, _) = demo_timeline();
        let before = tl.clone();
        let mut h = TimelineHistory::new();

        h.begin_transaction();
        h.exec(
            SetMuted {
                track: 0,
                muted: true,
                old: None,
            }
            .boxed(),
            &mut tl,
        );
        h.exec(
            PlaceClip {
                track: 0,
                asset: asset("t"),
                start_ms: 1500,
                duration_ms: 200,
                id: None,
            }
            .boxed(),
            &mut tl,
        );
        h.end_transaction();

        assert_eq!(h.undo_len(), 1, "两个命令合并为一步撤销");
        h.undo(&mut tl);
        assert_eq!(tl, before, "批量一步撤销");
        h.redo(&mut tl);
        assert!(tl.tracks[0].muted);
        assert_eq!(tl.duration(), 1700);
        h.undo(&mut tl);
        assert_eq!(tl, before);
    }

    #[test]
    fn empty_transaction_creates_no_step() {
        let (mut tl, _) = demo_timeline();
        let mut h = TimelineHistory::new();
        h.begin_transaction();
        h.end_transaction();
        assert_eq!(h.undo_len(), 0);
        assert!(!h.can_undo());

        // 嵌套 begin 被忽略,不丢已收集命令
        h.begin_transaction();
        h.exec(
            SetMuted {
                track: 1,
                muted: true,
                old: None,
            }
            .boxed(),
            &mut tl,
        );
        h.begin_transaction();
        h.end_transaction();
        assert_eq!(h.undo_len(), 1);
    }

    #[test]
    fn exec_clears_redo_stack() {
        let (mut tl, ids) = demo_timeline();
        let mut h = TimelineHistory::new();
        h.exec(
            SetMuted {
                track: 0,
                muted: true,
                old: None,
            }
            .boxed(),
            &mut tl,
        );
        h.undo(&mut tl);
        assert!(h.can_redo());
        h.exec(
            SetMuted {
                track: 1,
                muted: true,
                old: None,
            }
            .boxed(),
            &mut tl,
        );
        assert!(!h.can_redo(), "新命令清空 redo");
        let _ = ids;
    }

    #[test]
    fn clear_drops_history() {
        let (mut tl, _) = demo_timeline();
        let mut h = TimelineHistory::new();
        h.exec(
            SetMuted {
                track: 0,
                muted: true,
                old: None,
            }
            .boxed(),
            &mut tl,
        );
        assert!(h.can_undo());
        h.clear();
        assert!(!h.can_undo() && !h.can_redo());
        assert_eq!(h.undo_len(), 0);
    }

    #[test]
    fn command_names_are_reported() {
        let (_, ids) = demo_timeline();
        let cases: Vec<Box<dyn TimelineCommand>> = vec![
            Box::new(PlaceClip {
                track: 0,
                asset: asset("x"),
                start_ms: 0,
                duration_ms: 1,
                id: None,
            }),
            Box::new(RemoveClip {
                id: ids[0],
                track: None,
                clip: None,
            }),
            Box::new(MoveClip {
                id: ids[0],
                from_ms: None,
                to_ms: 0,
            }),
            Box::new(TrimClip {
                id: ids[0],
                new_in_ms: 0,
                new_out_ms: 1,
                old: None,
            }),
            Box::new(SplitClip {
                track: 0,
                ms: 1,
                original: None,
                right_id: None,
            }),
            Box::new(RemoveClipRipple {
                id: ids[0],
                track: None,
                removed: None,
            }),
            Box::new(SetSpeed {
                id: ids[0],
                new_speed: 1.0,
                old_speed: None,
            }),
            Box::new(SetMuted {
                track: 0,
                muted: false,
                old: None,
            }),
            Box::new(BatchCommand(Vec::new())),
        ];
        let names: Vec<String> = cases.iter().map(|c| c.name().into_owned()).collect();
        assert_eq!(
            names,
            vec![
                "放置 clip",
                "删除 clip",
                "移动 clip",
                "裁剪 clip",
                "分割 clip",
                "波纹删除 clip",
                "变速",
                "轨道静音",
                "批量操作",
            ]
        );
    }

    // —— proptest:随机合法命令序列 1..40 步,全 undo 后 Timeline == 快照 ——

    #[derive(Debug, Clone)]
    enum Op {
        Place { track: usize, start: u64, dur: u64 },
        Move { pick: usize, to: u64 },
        Trim { pick: usize, new_in: u64, len: u64 },
        Split { track: usize, ms: u64 },
        Remove { pick: usize },
        Ripple { pick: usize },
        Speed { pick: usize, tenth: u32 },
        Mute { track: usize, muted: bool },
    }

    fn any_op() -> impl Strategy<Value = Op> {
        prop_oneof![
            3 => (0usize..2, 0u64..20, 1u64..6)
                .prop_map(|(t, s, d)| Op::Place { track: t, start: s * 100, dur: d * 100 }),
            3 => (0usize..8, 0u64..20).prop_map(|(p, s)| Op::Move { pick: p, to: s * 100 }),
            2 => (0usize..8, 0u64..8, 1u64..8)
                .prop_map(|(p, i, l)| Op::Trim { pick: p, new_in: i * 100, len: l * 100 }),
            2 => (0usize..2, 0u64..20).prop_map(|(t, m)| Op::Split { track: t, ms: m * 100 }),
            2 => (0usize..8).prop_map(|p| Op::Remove { pick: p }),
            1 => (0usize..8).prop_map(|p| Op::Ripple { pick: p }),
            1 => (0usize..8, 1u32..30).prop_map(|(p, s)| Op::Speed { pick: p, tenth: s }),
            1 => (0usize..2, any::<bool>()).prop_map(|(t, m)| Op::Mute { track: t, muted: m }),
        ]
    }

    /// 从登记表取一个"当前存活"的 clip id(取模避免越界;已删除的 id 自动跳过)。
    fn pick_id(tl: &Timeline, registry: &[ClipId], pick: usize) -> Option<ClipId> {
        if registry.is_empty() {
            return None;
        }
        let id = registry[pick % registry.len()];
        tl.clip(id).is_some().then_some(id)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn random_command_sequence_full_undo_restores_snapshot(
            ops in proptest::collection::vec(any_op(), 1..40),
        ) {
            let (mut tl, ids) = demo_timeline();
            let snapshot = tl.clone();
            let mut h = TimelineHistory::new();
            let mut registry = ids;

            for op in ops {
                match op {
                    Op::Place { track, start, dur } => {
                        let before_ids = ids_of(&tl);
                        h.exec(
                            PlaceClip {
                                track,
                                asset: asset("gen"),
                                start_ms: start,
                                duration_ms: dur,
                                id: None,
                            }
                            .boxed(),
                            &mut tl,
                        );
                        registry.extend(ids_of(&tl).into_iter().filter(|id| !before_ids.contains(id)));
                    }
                    Op::Move { pick, to } => {
                        if let Some(id) = pick_id(&tl, &registry, pick) {
                            h.exec(MoveClip { id, from_ms: None, to_ms: to }.boxed(), &mut tl);
                        }
                    }
                    Op::Trim { pick, new_in, len } => {
                        if let Some(id) = pick_id(&tl, &registry, pick) {
                            h.exec(
                                TrimClip { id, new_in_ms: new_in, new_out_ms: new_in + len, old: None }
                                    .boxed(),
                                &mut tl,
                            );
                        }
                    }
                    Op::Split { track, ms } => {
                        h.exec(SplitClip { track, ms, original: None, right_id: None }.boxed(), &mut tl);
                    }
                    Op::Remove { pick } => {
                        if let Some(id) = pick_id(&tl, &registry, pick) {
                            h.exec(RemoveClip { id, track: None, clip: None }.boxed(), &mut tl);
                        }
                    }
                    Op::Ripple { pick } => {
                        if let Some(id) = pick_id(&tl, &registry, pick) {
                            h.exec(RemoveClipRipple { id, track: None, removed: None }.boxed(), &mut tl);
                        }
                    }
                    Op::Speed { pick, tenth } => {
                        if let Some(id) = pick_id(&tl, &registry, pick) {
                            h.exec(
                                SetSpeed { id, new_speed: f64::from(tenth) / 10.0, old_speed: None }
                                    .boxed(),
                                &mut tl,
                            );
                        }
                    }
                    Op::Mute { track, muted } => {
                        h.exec(SetMuted { track, muted, old: None }.boxed(), &mut tl);
                    }
                }
                prop_assert!(tl.is_valid(), "每条命令(或其静默跳过)后不变量必须成立");
            }

            while h.can_undo() {
                h.undo(&mut tl);
            }
            prop_assert_eq!(tl, snapshot, "任意命令序列全部撤销后必须回到初始快照");
        }
    }
}
