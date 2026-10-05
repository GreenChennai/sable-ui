//! 时间轴数据模型(docs/04 §8):轨道/clip/关键帧、不变量维护、吸附对齐。
//!
//! # 不变量(所有修改路径必须维持,`is_valid` 可随时校验)
//!
//! 1. 每轨 `clips` 按 `start_ms` **严格升序**(duration 恒 > 0,故起点不会并列);
//! 2. 任意两 clip 的区间 `[start, start+duration)` 互不重叠(相邻贴合允许);
//! 3. `duration_ms` = 全轨 max(clip end),是派生缓存,由每次修改后刷新。
//!
//! # 守卫(RBT-07/RBT-08,迭代审查报告 §6 RB-13)
//!
//! - `Clip::validate` 是 **speed / 出入点 / duration 的唯一守卫谓词**,
//!   反序列化(手写 `Deserialize`,加载即校验)与 `Timeline::set_speed`
//!   两条入口共用,坏值一律结构化 [`VideoError::InvalidClip`],不静默钳制;
//! - 所有结构修改在收尾处做**发布期同样生效**的不变量校验,破坏即
//!   [`VideoError::InvariantViolated`];成功路径上 debug 构建另有
//!   `debug_assert!` 额外校验(见 `Timeline::finish_mutation`)。
//!
//! # id 身份(架构决策,详见 crate 文档)
//!
//! [`ClipId`] 稳定自增 u64、**永不复用**(连跳号都不回收),因此撤销历史里的
//! 失效引用只会"目标已删除",绝不会误指新 clip——这是本 crate 撤销机制远比
//! sable-foundation(slotmap 换发 id + 重映射)简单的根本原因。

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{VideoError, VideoResult};

/// clip 的稳定身份:自增 u64,永不复用。由 [`Timeline`] 内部铸造,外部不可构造。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ClipId(u64);

impl ClipId {
    /// 内部数值(UI 层画历史面板/日志用)。
    pub fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for ClipId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 轨道类型(docs/04 §8:视频/音频/贴纸/字幕)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrackKind {
    Video,
    Audio,
    Sticker,
    Subtitle,
}

/// 素材引用:v0.1 只存路径 + 内容哈希(解码与缩略图在 v0.2 进程隔离补齐)。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetRef {
    pub path: String,
    pub hash: u64,
}

impl AssetRef {
    pub fn new(path: impl Into<String>, hash: u64) -> Self {
        AssetRef {
            path: path.into(),
            hash,
        }
    }
}

/// 预设缓动(docs/04 §9,与分册六 §4.2 同名;求值实现见 [`crate::curve`])。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Easing {
    Linear,
    InCubic,
    OutCubic,
    InOutCubic,
    Spring,
}

/// 单值关键帧(v0.1)。`t_ms` 为 **clip 本地时间**(clip 起点 = 0),
/// 移动 clip 关键帧随之移动,分割 clip 时按本地分割点切分。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub t_ms: u64,
    pub value: f64,
    pub easing: Easing,
}

/// 时间轴上的一段素材(docs/04 §8)。
///
/// `in_ms`/`out_ms` 是素材内出入点(源域),`start_ms`/`duration_ms` 是时间轴
/// 位置(时间轴域);变速时 `duration_ms = round((out_ms - in_ms) / speed)`。
/// 两域换算存在 ±1ms 取整误差,取舍以时间轴域连续性优先(docs/04 §8)。
///
/// # 反序列化守卫(RBT-07/RB-13)
///
/// `Deserialize` 为手写实现:字段照常提取后立即过 [`Clip::validate`],
/// speed 非有限正数 / 出点不大于入点 / duration 为 0 一律报错——坏值无法经
/// 任何 serde 格式(.sable/MessagePack/JSON)绕过守卫进入内存。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Clip {
    pub id: ClipId,
    pub asset: AssetRef,
    /// 在时间轴上的起点
    pub start_ms: u64,
    pub duration_ms: u64,
    /// 素材内入点(裁剪)
    pub in_ms: u64,
    /// 素材内出点(裁剪,必须 > in_ms)
    pub out_ms: u64,
    /// 播放速率(> 0)
    pub speed: f64,
    /// §9 曲线编辑器的数据(单值关键帧,t_ms 为 clip 本地时间)
    pub keyframes: Vec<Keyframe>,
}

impl Clip {
    /// 时间轴域终点(不含)。
    pub fn end_ms(&self) -> u64 {
        self.start_ms.saturating_add(self.duration_ms)
    }

    /// 素材域长度(出点 - 入点)。
    pub fn source_span_ms(&self) -> u64 {
        self.out_ms.saturating_sub(self.in_ms)
    }

    /// 结构守卫(RBT-07/RB-13):speed 有限且 > 0、出点严格大于入点、
    /// duration > 0。反序列化与 `Timeline::set_speed` 两条入口共用同一
    /// 谓词,守卫策略恒一致;坏值返回结构化 [`VideoError::InvalidClip`]。
    pub fn validate(&self) -> VideoResult<()> {
        ensure_valid_speed(self.speed)?;
        if self.out_ms <= self.in_ms {
            return Err(VideoError::InvalidClip("出点必须大于入点(out > in)"));
        }
        if self.duration_ms == 0 {
            return Err(VideoError::InvalidClip("duration_ms 必须大于 0"));
        }
        Ok(())
    }
}

/// speed 守卫谓词(RBT-07):`set_speed` 与反序列化两条入口共用,策略恒一致。
/// `!(speed > 0.0)` 同时命中 0/负数/NaN(0 > 0 为假、负数同理、`!(NaN > 0)`
/// 恰为 true——改写为 `speed <= 0.0` 即行为变更),`is_finite` 再拦 ±inf。
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn ensure_valid_speed(speed: f64) -> VideoResult<()> {
    if !(speed > 0.0) || !speed.is_finite() {
        return Err(VideoError::InvalidClip(
            "speed 必须为有限正数(speed > 0 且有限)",
        ));
    }
    Ok(())
}

impl<'de> Deserialize<'de> for Clip {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        /// 字段镜像(与 `Clip` 一一对应);守卫在字段提取后立即执行,
        /// 错误以 [`VideoError`] 的 Display 文本进入反序列化器的结构化错误。
        #[derive(Deserialize)]
        struct RawClip {
            id: ClipId,
            asset: AssetRef,
            start_ms: u64,
            duration_ms: u64,
            in_ms: u64,
            out_ms: u64,
            speed: f64,
            keyframes: Vec<Keyframe>,
        }
        let raw = RawClip::deserialize(deserializer)?;
        let clip = Clip {
            id: raw.id,
            asset: raw.asset,
            start_ms: raw.start_ms,
            duration_ms: raw.duration_ms,
            in_ms: raw.in_ms,
            out_ms: raw.out_ms,
            speed: raw.speed,
            keyframes: raw.keyframes,
        };
        clip.validate().map_err(serde::de::Error::custom)?;
        Ok(clip)
    }
}

/// 一条轨道。`clips` 按 start_ms 严格升序、互不重叠(不变量,见模块文档)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub kind: TrackKind,
    pub clips: Vec<Clip>,
    pub muted: bool,
    pub locked: bool,
}

impl Track {
    /// 命中测试:返回覆盖 `ms` 的 clip(start <= ms < end)。
    pub fn clip_at(&self, ms: u64) -> Option<&Clip> {
        let idx = self.clips.partition_point(|c| c.end_ms() <= ms);
        let clip = self.clips.get(idx)?;
        (clip.start_ms <= ms).then_some(clip)
    }
}

/// 时间轴整体(docs/04 §8)。全部类型 serde,未来直接进 .sable/.cutforge 工程文件。
///
/// `PartialEq` 为手写实现:`next_clip_id` 计数器**刻意不参与相等**——撤销到放置
/// 之前时计数器不回退(id 永不复用),撤销等价性必须无视它。
///
/// # 反序列化守卫(RBT-07)
///
/// `Deserialize` 为手写实现:反序列化完成后立即过 [`Timeline::validate`]
/// (每个 clip 的数据守卫 + 结构不变量),坏文件在加载边界即被结构化拒收,
/// 不带病进内存。
#[derive(Clone, Debug, Serialize)]
pub struct Timeline {
    pub tracks: Vec<Track>,
    /// 时间轴缩放(像素/秒),时间↔像素换算见 [`Timeline::ms_to_px`]。
    pub px_per_second: f64,
    pub snap_enabled: bool,
    /// 派生缓存:全轨 max(clip end);每次修改后刷新,勿手改。
    #[serde(default)]
    pub duration_ms: u64,
    /// ClipId 发号器(私有;serde 持久化,避免重开工程后复用 id)。
    #[serde(default)]
    next_clip_id: u64,
}

impl Default for Timeline {
    fn default() -> Self {
        Timeline {
            tracks: Vec::new(),
            px_per_second: 100.0,
            snap_enabled: true,
            duration_ms: 0,
            next_clip_id: 0,
        }
    }
}

impl PartialEq for Timeline {
    fn eq(&self, other: &Self) -> bool {
        // next_clip_id 刻意排除:id 计数器只进不退(永不复用),不影响文档内容等价
        self.tracks == other.tracks
            && self.px_per_second == other.px_per_second
            && self.snap_enabled == other.snap_enabled
            && self.duration_ms == other.duration_ms
    }
}

impl<'de> Deserialize<'de> for Timeline {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        /// 字段镜像(含两个 `#[serde(default)]` 字段,与 `Timeline` 一致)。
        #[derive(Deserialize)]
        struct RawTimeline {
            tracks: Vec<Track>,
            px_per_second: f64,
            snap_enabled: bool,
            #[serde(default)]
            duration_ms: u64,
            #[serde(default)]
            next_clip_id: u64,
        }
        let raw = RawTimeline::deserialize(deserializer)?;
        let timeline = Timeline {
            tracks: raw.tracks,
            px_per_second: raw.px_per_second,
            snap_enabled: raw.snap_enabled,
            duration_ms: raw.duration_ms,
            next_clip_id: raw.next_clip_id,
        };
        // 加载入口统一 validate(RBT-07):坏 clip 数据与结构不变量破坏
        // (乱序/重叠/缓存不一致)一律在此结构化报错。
        timeline.validate().map_err(serde::de::Error::custom)?;
        Ok(timeline)
    }
}

/// 就近吸附(纯函数,单测重点):返回与 `raw_ms` 距离最近且不超过容差的目标,
/// 无候选/全部超容差则原样返回。等距并列取切片中先出现者(`snap_targets`
/// 升序给出,即较小者)。
pub fn snap_time(raw_ms: u64, tolerance_ms: u64, targets: &[u64]) -> u64 {
    targets
        .iter()
        .copied()
        .min_by_key(|t| t.abs_diff(raw_ms))
        .filter(|t| t.abs_diff(raw_ms) <= tolerance_ms)
        .unwrap_or(raw_ms)
}

impl Timeline {
    pub fn new() -> Self {
        Self::default()
    }

    // —— 轨道管理(直接修改,不走撤销;v0.1 轨道结构变化不进历史)——

    /// 追加一条空轨道,返回其下标。
    pub fn add_track(&mut self, kind: TrackKind) -> usize {
        self.tracks.push(Track {
            kind,
            clips: Vec::new(),
            muted: false,
            locked: false,
        });
        self.tracks.len() - 1
    }

    /// 删除轨道并返回它。拒绝删除最后一条轨道(保证永远有可放置的轨道)。
    pub fn remove_track(&mut self, track: usize) -> VideoResult<Track> {
        if track >= self.tracks.len() {
            return Err(VideoError::TrackNotFound(track));
        }
        if self.tracks.len() <= 1 {
            return Err(VideoError::EmptyTimeline);
        }
        let removed = self.tracks.remove(track);
        self.finish_mutation("remove_track")?;
        debug_assert!(self.is_valid());
        Ok(removed)
    }

    /// 第一条指定类型轨道的下标(如"拖入音频素材时找音频轨")。
    pub fn track_of_kind(&self, kind: TrackKind) -> VideoResult<usize> {
        self.tracks
            .iter()
            .position(|t| t.kind == kind)
            .ok_or(VideoError::NoActiveTrack(kind))
    }

    // —— 查询 ——

    /// 定位 clip 所在 (轨道下标, 轨内下标)。
    pub fn locate(&self, id: ClipId) -> Option<(usize, usize)> {
        for (ti, track) in self.tracks.iter().enumerate() {
            for (ci, clip) in track.clips.iter().enumerate() {
                if clip.id == id {
                    return Some((ti, ci));
                }
            }
        }
        None
    }

    pub fn clip(&self, id: ClipId) -> Option<&Clip> {
        let (t, i) = self.locate(id)?;
        self.tracks.get(t)?.clips.get(i)
    }

    /// 命中测试:某轨道上覆盖 `ms` 的 clip id(start <= ms < end)。
    pub fn clip_at(&self, track: usize, ms: u64) -> Option<ClipId> {
        self.tracks.get(track)?.clip_at(ms).map(|c| c.id)
    }

    /// 时间轴总长(派生缓存)。
    pub fn duration(&self) -> u64 {
        self.duration_ms
    }

    // —— 结构修改(全部维持不变量;撤销包装见 crate::command)——

    /// 放置 clip:入点 0、速度 1.0、无关键帧;放不下返回 [`VideoError::Overlap`]。
    ///
    /// id 即使放置失败也会消耗一个号(自增计数器只进不退,永不复用——跳号无害)。
    pub fn place_clip(
        &mut self,
        track: usize,
        asset: AssetRef,
        start_ms: u64,
        duration_ms: u64,
    ) -> VideoResult<ClipId> {
        let id = self.mint_id();
        self.place_clip_with_id(track, asset, start_ms, duration_ms, id)?;
        Ok(id)
    }

    /// 移动 clip 到新起点(吸附在外部做,这里只管碰撞):撞邻返回
    /// [`VideoError::Overlap`],时间轴保持原状。
    pub fn move_clip(&mut self, id: ClipId, new_start_ms: u64) -> VideoResult<()> {
        let Some((t, i)) = self.locate(id) else {
            return Err(VideoError::ClipNotFound(id.value()));
        };
        if self.tracks[t].clips[i].start_ms == new_start_ms {
            return Ok(());
        }
        let mut clip = self.tracks[t].clips.remove(i);
        let old_start = clip.start_ms;
        clip.start_ms = new_start_ms;
        match self.insert_clip_raw(t, clip) {
            Ok(()) => {
                self.finish_mutation("move_clip")?;
                debug_assert!(self.is_valid());
                Ok(())
            }
            Err(err) => {
                // 失败原位放回,时间轴一字不差
                let (mut clip, e) = *err;
                clip.start_ms = old_start;
                self.tracks[t].clips.insert(i, clip);
                Err(e)
            }
        }
    }

    /// 裁剪素材出入点(out 必须严格大于 in;duration 按 (out-in)/speed 同步重算)。
    ///
    /// 起点不动;若新 duration 变长撞到右邻,返回 [`VideoError::Overlap`]。
    /// 关键帧不随裁剪调整(v0.1:超出新 duration 的关键帧求值时被钳制,保留以简化撤销)。
    pub fn trim_clip(&mut self, id: ClipId, new_in_ms: u64, new_out_ms: u64) -> VideoResult<()> {
        if new_out_ms <= new_in_ms {
            return Err(VideoError::InvalidRange("出点必须大于入点(out > in)"));
        }
        let Some((t, i)) = self.locate(id) else {
            return Err(VideoError::ClipNotFound(id.value()));
        };
        // duration 按 (out-in)/speed 换算:speed 被直改破坏时在此按统一谓词
        // 拒收,防 inf/NaN duration 进入时间轴(RBT-07/RB-13)
        let speed = self.tracks[t].clips[i].speed;
        ensure_valid_speed(speed)?;
        let start = self.tracks[t].clips[i].start_ms;
        let new_duration = (((new_out_ms - new_in_ms) as f64 / speed).round() as u64).max(1);
        let new_end = start.saturating_add(new_duration);
        if let Some(next) = self.tracks[t].clips.get(i + 1)
            && next.start_ms < new_end
        {
            return Err(VideoError::Overlap);
        }
        let clip = &mut self.tracks[t].clips[i];
        clip.in_ms = new_in_ms;
        clip.out_ms = new_out_ms;
        clip.duration_ms = new_duration;
        self.finish_mutation("trim_clip")?;
        debug_assert!(self.is_valid());
        Ok(())
    }

    /// 在 `ms` 处分割:分割点必须**严格**落在某 clip 内部(start < ms < end),
    /// 恰在端点/clip 外/空时间轴分别报 InvalidRange / InvalidRange / EmptyTimeline。
    ///
    /// 左半保留原 id,右半铸造新 id;两半时间轴域严格贴合(端到端连续,总长不变);
    /// 关键帧按本地分割点切分(恰在分割点的关键帧归右半)。
    pub fn split_at(&mut self, track: usize, ms: u64) -> VideoResult<(ClipId, ClipId)> {
        let right_id = self.mint_id();
        self.split_at_with_ids(track, ms, right_id)
    }

    /// 删除 clip,返回被删者。
    pub fn remove_clip(&mut self, id: ClipId) -> VideoResult<Clip> {
        let Some((t, i)) = self.locate(id) else {
            return Err(VideoError::ClipNotFound(id.value()));
        };
        let clip = self.tracks[t].clips.remove(i);
        self.finish_mutation("remove_clip")?;
        debug_assert!(self.is_valid());
        Ok(clip)
    }

    /// 波纹删除:删掉 clip 后,同轨后续 clip 前移补位(无空洞,总长收缩)。
    pub fn ripple_remove(&mut self, id: ClipId) -> VideoResult<Clip> {
        let Some((t, i)) = self.locate(id) else {
            return Err(VideoError::ClipNotFound(id.value()));
        };
        // 前移下溢预检(RBT-08):release 无溢出检查,`-=` 回绕会静默破坏
        // 时间轴。正常不变量下后续起点 >= 被删终点 > 被删时长,不会下溢;
        // 只有不变量已被绕过 API 的直改破坏才会触发,显式拒收、原状保留。
        let removed_duration = self.tracks[t].clips[i].duration_ms;
        if self.tracks[t].clips[i + 1..]
            .iter()
            .any(|c| c.start_ms < removed_duration)
        {
            return Err(VideoError::InvariantViolated(
                "ripple_remove:后续 clip 起点小于被删时长,前移将下溢",
            ));
        }
        let clip = self.tracks[t].clips.remove(i);
        for c in &mut self.tracks[t].clips[i..] {
            c.start_ms -= clip.duration_ms; // 上方已预检,不会下溢
        }
        self.finish_mutation("ripple_remove")?;
        debug_assert!(self.is_valid());
        Ok(clip)
    }

    /// 变速(speed 必须为有限正数);duration 按素材时长换算重算:duration = (out-in)/speed。
    /// 变慢(duration 变长)撞到右邻时返回 [`VideoError::Overlap`]。
    pub fn set_speed(&mut self, id: ClipId, speed: f64) -> VideoResult<()> {
        // 与反序列化入口共用同一守卫谓词、同一错误变体(RBT-07:两条入口
        // 同一策略):0/负数/NaN 经 `!(speed > 0.0)` 拦截,±inf 由 is_finite 拦截。
        ensure_valid_speed(speed)?;
        let Some((t, i)) = self.locate(id) else {
            return Err(VideoError::ClipNotFound(id.value()));
        };
        let clip = &self.tracks[t].clips[i];
        let (in_ms, out_ms, start) = (clip.in_ms, clip.out_ms, clip.start_ms);
        let new_duration = (((out_ms - in_ms) as f64 / speed).round() as u64).max(1);
        let new_end = start.saturating_add(new_duration);
        if let Some(next) = self.tracks[t].clips.get(i + 1)
            && next.start_ms < new_end
        {
            return Err(VideoError::Overlap);
        }
        let clip = &mut self.tracks[t].clips[i];
        clip.speed = speed;
        clip.duration_ms = new_duration;
        self.finish_mutation("set_speed")?;
        debug_assert!(self.is_valid());
        Ok(())
    }

    // —— 吸附与换算(docs/04 §8:候选 = 其他 clip 边缘 + 0)——

    /// 吸附候选:所有 clip 起止点(可排除一个 clip,拖动它自身时排除)+ 0,升序去重。
    pub fn snap_targets(&self, exclude_id: Option<ClipId>) -> Vec<u64> {
        let mut out = vec![0u64];
        for track in &self.tracks {
            for clip in &track.clips {
                if Some(clip.id) == exclude_id {
                    continue;
                }
                out.push(clip.start_ms);
                out.push(clip.end_ms());
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// 便捷入口:尊重 `snap_enabled`,拖动 clip 时传入 `Some(id)` 排除自身。
    pub fn snap(&self, raw_ms: u64, tolerance_ms: u64, exclude_id: Option<ClipId>) -> u64 {
        if !self.snap_enabled {
            return raw_ms;
        }
        snap_time(raw_ms, tolerance_ms, &self.snap_targets(exclude_id))
    }

    /// 毫秒 → 像素(时间轴视图横向换算)。
    pub fn ms_to_px(&self, ms: u64) -> f64 {
        ms as f64 / 1000.0 * self.px_per_second
    }

    /// 像素 → 毫秒(向下取整,负像素钳为 0)。
    pub fn px_to_ms(&self, px: f64) -> u64 {
        (px / self.px_per_second * 1000.0).max(0.0) as u64
    }

    // —— 不变量自检(测试与 debug_assert 用)——

    /// 全量校验(RBT-07 加载入口与测试用):每个 clip 过 [`Clip::validate`],
    /// 再校验结构不变量([`Self::is_valid`]:升序不重叠、duration>0、派生缓存
    /// 一致)。任一失败返回结构化错误,不静默钳制。
    pub fn validate(&self) -> VideoResult<()> {
        for track in &self.tracks {
            for clip in &track.clips {
                clip.validate()?;
            }
        }
        if !self.is_valid() {
            return Err(VideoError::InvariantViolated(
                "结构不变量(升序/不重叠/duration>0/缓存一致)",
            ));
        }
        Ok(())
    }

    /// 校验三条不变量(见模块文档):升序不重叠、duration>0、派生缓存正确。
    pub fn is_valid(&self) -> bool {
        let mut max_end = 0u64;
        for track in &self.tracks {
            let mut prev_end: Option<u64> = None;
            for clip in &track.clips {
                if clip.duration_ms == 0 {
                    return false;
                }
                if let Some(pe) = prev_end
                    && clip.start_ms < pe
                {
                    return false; // 乱序或重叠
                }
                prev_end = Some(clip.end_ms());
                max_end = max_end.max(clip.end_ms());
            }
        }
        self.duration_ms == max_end
    }

    // —— 内部零件(crate 内命令系统复用)——

    /// 修改收尾(RBT-08):刷新派生缓存后做**发布期同样生效**的不变量校验,
    /// 失败显式返回 [`VideoError::InvariantViolated`] 而非静默放行;成功路径
    /// 上 debug 构建再由调用方叠加 `debug_assert!` 额外校验。输入校验齐全时
    /// 本校验不可达,出现即内部缺陷或不变量已被绕过 API 的直改破坏。
    fn finish_mutation(&mut self, op: &'static str) -> VideoResult<()> {
        self.refresh_duration();
        if !self.is_valid() {
            return Err(VideoError::InvariantViolated(op));
        }
        Ok(())
    }

    /// 铸造新 id:自增,永不复用;u64 溢出按回绕处理(现实中不可达)。
    fn mint_id(&mut self) -> ClipId {
        let id = ClipId(self.next_clip_id);
        self.next_clip_id = self.next_clip_id.wrapping_add(1);
        id
    }

    /// 以指定 id 放置(命令 redo 复用 id 保证 ClipId 稳定;见 crate::command::PlaceClip)。
    pub(crate) fn place_clip_with_id(
        &mut self,
        track: usize,
        asset: AssetRef,
        start_ms: u64,
        duration_ms: u64,
        id: ClipId,
    ) -> VideoResult<()> {
        if duration_ms == 0 {
            return Err(VideoError::InvalidRange("duration_ms 必须大于 0"));
        }
        let clip = Clip {
            id,
            asset,
            start_ms,
            duration_ms,
            in_ms: 0,
            out_ms: duration_ms,
            speed: 1.0,
            keyframes: Vec::new(),
        };
        self.insert_clip_raw(track, clip).map_err(|err| err.1)?;
        self.finish_mutation("place_clip")?;
        debug_assert!(self.is_valid());
        Ok(())
    }

    /// 保持有序 + 碰撞检查的插入(失败时不消费 clip,以 `Box<(Clip, VideoError)>`
    /// 原样退回供调用方还原;Box 装 Err 是因 Clip 体积大,避免大 Err 直接内联)。
    pub(crate) fn insert_clip_raw(
        &mut self,
        track: usize,
        clip: Clip,
    ) -> Result<(), Box<(Clip, VideoError)>> {
        let Some(t) = self.tracks.get_mut(track) else {
            return Err(Box::new((clip, VideoError::TrackNotFound(track))));
        };
        let start = clip.start_ms;
        let end = start.saturating_add(clip.duration_ms);
        let pos = t.clips.partition_point(|c| c.start_ms < start);
        if pos > 0
            && let Some(prev) = t.clips.get(pos - 1)
            && prev.end_ms() > start
        {
            return Err(Box::new((clip, VideoError::Overlap)));
        }
        if let Some(next) = t.clips.get(pos)
            && next.start_ms < end
        {
            return Err(Box::new((clip, VideoError::Overlap)));
        }
        t.clips.insert(pos, clip);
        self.refresh_duration();
        Ok(())
    }

    /// 带 id 的分割(命令 redo 确定性重放;见 crate::command::SplitClip)。
    pub(crate) fn split_at_with_ids(
        &mut self,
        track: usize,
        ms: u64,
        right_id: ClipId,
    ) -> VideoResult<(ClipId, ClipId)> {
        // "空时间轴" = 没有任何 clip 可分割(有轨无 clip 也算空;无轨自然包含)
        if self.tracks.iter().all(|t| t.clips.is_empty()) {
            return Err(VideoError::EmptyTimeline);
        }
        let Some(t) = self.tracks.get_mut(track) else {
            return Err(VideoError::TrackNotFound(track));
        };
        let idx = t.clips.partition_point(|c| c.end_ms() <= ms);
        let Some(clip) = t.clips.get(idx) else {
            return Err(VideoError::InvalidRange("分割点不在任何 clip 内部"));
        };
        if clip.start_ms >= ms {
            return Err(VideoError::InvalidRange(
                "分割点必须严格落在 clip 内部(start < ms < end)",
            ));
        }
        // clip 数据守卫(RBT-07):speed 非有限正数 / 出入点倒挂会让下面的
        // 换算产生 inf/NaN,先按统一谓词拒收
        clip.validate()?;
        let left_id = clip.id;
        let local = ms - clip.start_ms;
        let (speed, in_ms, out_ms, duration) =
            (clip.speed, clip.in_ms, clip.out_ms, clip.duration_ms);
        // 时间轴域 → 素材域取整;时间轴域连续性优先(两半端到端贴合,总长不变)
        let split_source = in_ms.saturating_add((local as f64 * speed).round() as u64);
        let left_duration = ((split_source - in_ms) as f64 / speed).round() as u64;
        if split_source <= in_ms
            || split_source >= out_ms
            || left_duration == 0
            || left_duration >= duration
        {
            return Err(VideoError::InvalidRange(
                "分割点换算到素材域后过于贴近边缘,拒绝退化分割",
            ));
        }
        // 右半起点溢出预检(RBT-08):release 无溢出检查,回绕会静默打乱升序
        // 不变量;显式拒收,时间轴保持原状
        let right_start =
            clip.start_ms
                .checked_add(left_duration)
                .ok_or(VideoError::InvariantViolated(
                    "split_at:右半起点溢出,拒绝分割",
                ))?;
        let mut right = clip.clone();
        right.id = right_id;
        right.start_ms = right_start;
        right.duration_ms = duration - left_duration;
        right.in_ms = split_source;
        right.keyframes = clip
            .keyframes
            .iter()
            .filter(|k| k.t_ms >= local)
            .map(|k| Keyframe {
                t_ms: k.t_ms - local,
                value: k.value,
                easing: k.easing,
            })
            .collect();
        {
            let left = &mut t.clips[idx];
            left.duration_ms = left_duration;
            left.out_ms = split_source;
            left.keyframes.retain(|k| k.t_ms < local);
        }
        t.clips.insert(idx + 1, right);
        self.finish_mutation("split_at")?;
        debug_assert!(self.is_valid());
        Ok((left_id, right_id))
    }

    /// 波纹还原(命令 revert 专用):右侧 clip 让位后放回被删 clip。
    pub(crate) fn ripple_restore(&mut self, track: usize, clip: Clip) {
        let Some(t) = self.tracks.get_mut(track) else {
            tracing::warn!(track, "ripple_restore:轨道不存在,放弃还原");
            return;
        };
        for c in &mut t.clips {
            if c.start_ms >= clip.start_ms {
                c.start_ms = c.start_ms.saturating_add(clip.duration_ms);
            }
        }
        let pos = t.clips.partition_point(|c| c.start_ms < clip.start_ms);
        t.clips.insert(pos, clip);
        self.refresh_duration();
    }

    /// 刷新派生缓存 duration_ms = 全轨 max(clip end)。
    fn refresh_duration(&mut self) {
        self.duration_ms = self
            .tracks
            .iter()
            .map(|t| t.clips.last().map(Clip::end_ms).unwrap_or(0))
            .max()
            .unwrap_or(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn asset(name: &str) -> AssetRef {
        AssetRef::new(format!("/assets/{name}.mp4"), 42)
    }

    /// 两轨:0=Video、1=Audio;轨0 已放 [0,500) 与 [500,1000),轨1 放 [200,600)。
    fn demo_timeline() -> (Timeline, Vec<ClipId>) {
        let mut tl = Timeline::new();
        tl.add_track(TrackKind::Video);
        tl.add_track(TrackKind::Audio);
        let a = tl.place_clip(0, asset("v1"), 0, 500).expect("place v1");
        let b = tl.place_clip(0, asset("v2"), 500, 500).expect("place v2");
        let c = tl.place_clip(1, asset("a1"), 200, 400).expect("place a1");
        (tl, vec![a, b, c])
    }

    #[test]
    fn place_clip_inserts_sorted_and_reports_id() {
        let mut tl = Timeline::new();
        let t = tl.add_track(TrackKind::Video);
        let id1 = tl.place_clip(t, asset("a"), 1000, 300).expect("place 1");
        let id2 = tl.place_clip(t, asset("b"), 0, 200).expect("place 2");
        let id3 = tl.place_clip(t, asset("c"), 400, 100).expect("place 3");
        let starts: Vec<u64> = tl.tracks[t].clips.iter().map(|c| c.start_ms).collect();
        assert_eq!(starts, vec![0, 400, 1000], "乱序插入后仍按 start_ms 升序");
        assert!(tl.is_valid());
        assert_eq!(tl.duration(), 1300);
        // 新 clip 出入点/速度默认值
        let clip = tl.clip(id3).expect("clip 3");
        assert_eq!(clip.in_ms, 0);
        assert_eq!(clip.out_ms, 100);
        assert_eq!(clip.speed, 1.0);
        // id 互不相同且递增
        assert!(id1.value() < id2.value() && id2.value() < id3.value());
    }

    #[test]
    fn place_clip_rejects_overlap_and_bad_input() {
        let (mut tl, _) = demo_timeline();
        // 与 [0,500) 重叠
        assert_eq!(
            tl.place_clip(0, asset("x"), 400, 200),
            Err(VideoError::Overlap)
        );
        // 与相邻两段同时重叠
        assert_eq!(
            tl.place_clip(0, asset("x"), 450, 100),
            Err(VideoError::Overlap)
        );
        // 贴合边界允许:轨0 [1000,1200) 为空档
        assert!(tl.place_clip(0, asset("gap"), 1000, 200).is_ok());
        // duration 0
        assert!(matches!(
            tl.place_clip(0, asset("x"), 2000, 0),
            Err(VideoError::InvalidRange(_))
        ));
        // 轨道越界
        assert_eq!(
            tl.place_clip(9, asset("x"), 0, 100),
            Err(VideoError::TrackNotFound(9))
        );
        assert!(tl.is_valid());
    }

    #[test]
    fn move_clip_to_free_slot_and_reject_collision() {
        let (mut tl, ids) = demo_timeline();
        let (a, b) = (ids[0], ids[1]);
        // 自由槽位:[1000, 1500)
        assert_eq!(tl.move_clip(a, 1000), Ok(()));
        assert_eq!(tl.clip(a).expect("a").start_ms, 1000);
        assert!(tl.is_valid());
        let snapshot = tl.clone();
        // 撞 b([500,1000))
        assert_eq!(tl.move_clip(a, 700), Err(VideoError::Overlap));
        assert_eq!(tl, snapshot, "失败的移动不得改变时间轴");
        // 撞 a(挪 b 到 900 会与 [1000,1500) 重叠)
        assert_eq!(tl.move_clip(b, 900), Err(VideoError::Overlap));
        assert_eq!(tl, snapshot);
        // 不存在的 clip
        assert_eq!(
            tl.move_clip(ClipId(9999), 0),
            Err(VideoError::ClipNotFound(9999))
        );
    }

    #[test]
    fn trim_clip_syncs_duration_and_validates() {
        let (mut tl, ids) = demo_timeline();
        let b = ids[1]; // [500,1000) in=0 out=500
        assert_eq!(tl.trim_clip(b, 100, 400), Ok(()));
        let clip = tl.clip(b).expect("b");
        assert_eq!(clip.in_ms, 100);
        assert_eq!(clip.out_ms, 400);
        assert_eq!(clip.duration_ms, 300, "duration 按 (out-in)/speed 同步");
        assert_eq!(clip.start_ms, 500, "起点不动");
        assert_eq!(clip.end_ms(), 800);
        assert!(tl.is_valid());
        // out <= in 拒绝
        assert!(matches!(
            tl.trim_clip(b, 300, 300),
            Err(VideoError::InvalidRange(_))
        ));
        assert!(matches!(
            tl.trim_clip(b, 350, 300),
            Err(VideoError::InvalidRange(_))
        ));
        // 不存在的 clip
        let snapshot = tl.clone();
        assert_eq!(
            tl.trim_clip(ClipId(777), 0, 100),
            Err(VideoError::ClipNotFound(777))
        );
        assert_eq!(tl, snapshot);
    }

    #[test]
    fn trim_growth_collision_rejected() {
        let (mut tl, ids) = demo_timeline();
        let a = ids[0]; // [0,500) 右邻 b 起点在 500
        assert_eq!(
            tl.trim_clip(a, 0, 600),
            Err(VideoError::Overlap),
            "变长撞右邻"
        );
        assert_eq!(tl.trim_clip(a, 0, 500), Ok(()), "恰好贴合允许");
        assert!(tl.is_valid());
    }

    #[test]
    fn split_at_produces_contiguous_halves() {
        let (mut tl, ids) = demo_timeline();
        let b = ids[1]; // [500,1000) 素材内 [0,500)
        let (left, right) = tl.split_at(0, 700).expect("split");
        assert_eq!(left, b, "左半保留原 id");
        assert_ne!(right, left);
        let l = tl.clip(left).expect("left").clone();
        let r = tl.clip(right).expect("right").clone();
        assert_eq!(l.start_ms, 500);
        assert_eq!(l.duration_ms, 200);
        assert_eq!(l.out_ms, 200);
        assert_eq!(r.start_ms, 700, "右半起点 = 分割点");
        assert_eq!(r.duration_ms, 300);
        assert_eq!(r.in_ms, 200, "右半素材入点 = 左半出点");
        assert_eq!(r.out_ms, 500);
        assert_eq!(l.end_ms(), r.start_ms, "两半严格贴合");
        assert_eq!(tl.duration(), 1000, "总长不变");
        assert!(tl.is_valid());
        // 两半各自仍可再分割
        assert!(tl.split_at(0, 600).is_ok());
        assert!(tl.is_valid());
    }

    #[test]
    fn split_at_rejects_boundaries_and_empty() {
        let (mut tl, _ids) = demo_timeline();
        // 恰在端点
        assert!(matches!(
            tl.split_at(0, 500),
            Err(VideoError::InvalidRange(_))
        ));
        assert!(matches!(
            tl.split_at(0, 1000),
            Err(VideoError::InvalidRange(_))
        ));
        // clip 外(空档:轨1 [0,200) 无 clip)
        assert!(matches!(
            tl.split_at(1, 100),
            Err(VideoError::InvalidRange(_))
        ));
        // 轨道越界
        assert_eq!(tl.split_at(9, 700), Err(VideoError::TrackNotFound(9)));
        // 空时间轴(有轨无 clip)
        let mut empty = Timeline::new();
        empty.add_track(TrackKind::Video);
        assert_eq!(empty.split_at(0, 10), Err(VideoError::EmptyTimeline));
    }

    #[test]
    fn split_at_with_speed_keeps_invariants() {
        let mut tl = Timeline::new();
        tl.add_track(TrackKind::Video);
        let id = tl.place_clip(0, asset("v"), 100, 300).expect("place");
        tl.set_speed(id, 2.0).expect("2x");
        // 现在 [100,250),素材内 [0,300)
        let (l, r) = tl.split_at(0, 180).expect("split at 180");
        let lc = tl.clip(l).expect("l").clone();
        let rc = tl.clip(r).expect("r").clone();
        assert_eq!(lc.end_ms(), rc.start_ms, "变速分割仍端到端贴合");
        assert_eq!(
            lc.start_ms + lc.duration_ms + rc.duration_ms,
            250,
            "总长不变"
        );
        assert_eq!(rc.start_ms, 180, "整数倍速下分割点无取整误差");
        assert_eq!(rc.in_ms, 160);
        assert_eq!(rc.out_ms, 300);
        assert!(tl.is_valid());
    }

    #[test]
    fn split_keyframes_follow_clips() {
        let (mut tl, ids) = demo_timeline();
        let b = ids[1];
        // 给 b 加关键帧:本地 0/100/200/400(时间轴 500/600/700/900)
        let (t, i) = tl.locate(b).expect("b in track");
        tl.tracks[t].clips[i].keyframes = vec![
            Keyframe {
                t_ms: 0,
                value: 0.0,
                easing: Easing::Linear,
            },
            Keyframe {
                t_ms: 100,
                value: 1.0,
                easing: Easing::Linear,
            },
            Keyframe {
                t_ms: 200,
                value: 2.0,
                easing: Easing::Linear,
            },
            Keyframe {
                t_ms: 400,
                value: 4.0,
                easing: Easing::Linear,
            },
        ];
        let (l, r) = tl.split_at(0, 700).expect("split at local 200");
        let lks = tl.clip(l).expect("l").keyframes.clone();
        let rks = tl.clip(r).expect("r").keyframes.clone();
        assert_eq!(lks.iter().map(|k| k.t_ms).collect::<Vec<_>>(), vec![0, 100]);
        assert_eq!(
            rks.iter().map(|k| k.t_ms).collect::<Vec<_>>(),
            vec![0, 200],
            "恰在分割点的关键帧归右半并把本地时间归零"
        );
        assert_eq!(rks[0].value, 2.0);
        assert_eq!(rks[1].value, 4.0);
    }

    #[test]
    fn remove_clip_and_locate() {
        let (mut tl, ids) = demo_timeline();
        assert_eq!(tl.locate(ids[1]), Some((0, 1)));
        let removed = tl.remove_clip(ids[1]).expect("remove");
        assert_eq!(removed.id, ids[1]);
        assert_eq!(tl.locate(ids[1]), None);
        assert_eq!(tl.clip(ids[1]), None);
        assert_eq!(tl.duration(), 600, "轨0 只剩 [0,500),轨1 到 600");
        assert!(tl.is_valid());
        assert_eq!(
            tl.remove_clip(ids[1]),
            Err(VideoError::ClipNotFound(ids[1].value()))
        );
    }

    #[test]
    fn ripple_remove_closes_gap_and_shrinks() {
        let mut tl = Timeline::new();
        tl.add_track(TrackKind::Video);
        let a = tl.place_clip(0, asset("a"), 0, 100).expect("a");
        let b = tl.place_clip(0, asset("b"), 100, 200).expect("b");
        let c = tl.place_clip(0, asset("c"), 300, 150).expect("c");
        let removed = tl.ripple_remove(b).expect("ripple");
        assert_eq!(removed.id, b);
        let starts: Vec<u64> = tl.tracks[0].clips.iter().map(|x| x.start_ms).collect();
        assert_eq!(starts, vec![0, 100], "c 前移 200ms 补位,无空洞");
        assert_eq!(tl.clip(c).expect("c").start_ms, 100);
        assert_eq!(tl.duration(), 250, "总长收缩被删时长");
        assert!(tl.is_valid());
        // 波纹删除首段:后续整体前移
        assert!(tl.ripple_remove(a).is_ok());
        assert_eq!(tl.tracks[0].clips[0].start_ms, 0);
        assert_eq!(tl.duration(), 150);
        assert!(tl.is_valid());
    }

    #[test]
    fn set_speed_recomputes_duration_and_validates() {
        let (mut tl, ids) = demo_timeline();
        let a = ids[0]; // [0,500) 素材 [0,500)
        assert_eq!(tl.set_speed(a, 2.0), Ok(()));
        let clip = tl.clip(a).expect("a");
        assert_eq!(clip.speed, 2.0);
        assert_eq!(clip.duration_ms, 250, "duration = (out-in)/speed = 500/2");
        assert_eq!(clip.end_ms(), 250);
        assert!(tl.is_valid());
        // 变速回去,duration 复原(素材域 out-in 是稳定量,无漂移)
        assert_eq!(tl.set_speed(a, 1.0), Ok(()));
        assert_eq!(tl.clip(a).expect("a").duration_ms, 500);
        // 非法速度(与反序列化共用同一谓词、同一错误变体:InvalidClip)
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                matches!(tl.set_speed(a, bad), Err(VideoError::InvalidClip(_))),
                "speed = {bad} 必须被拒收为 InvalidClip"
            );
        }
        // 变慢撞右邻:speed 0.5 → duration 1000,b 起点在 500
        assert_eq!(tl.set_speed(a, 0.5), Err(VideoError::Overlap));
        // 不存在的 clip
        assert!(matches!(
            tl.set_speed(ClipId(4242), 1.0),
            Err(VideoError::ClipNotFound(_))
        ));
    }

    #[test]
    fn duration_tracks_max_end_across_tracks() {
        let (tl, _) = demo_timeline();
        assert_eq!(tl.duration(), 1000, "轨0 到 1000 > 轨1 到 600");
    }

    #[test]
    fn snap_time_within_tolerance_snaps_to_nearest() {
        let targets = [0, 500, 1000];
        assert_eq!(snap_time(520, 100, &targets), 500);
        assert_eq!(snap_time(940, 100, &targets), 1000);
        assert_eq!(snap_time(0, 100, &targets), 0);
    }

    #[test]
    fn snap_time_outside_tolerance_returns_raw() {
        let targets = [0, 500, 1000];
        assert_eq!(snap_time(250, 100, &targets), 250, "距两侧都是 250 > 100");
        assert_eq!(snap_time(123, 5, &targets), 123);
    }

    #[test]
    fn snap_time_multi_candidates_picks_nearest_and_tie_prefers_smaller() {
        let targets = [100, 140, 900];
        assert_eq!(snap_time(130, 50, &targets), 140);
        // 等距并列:取切片中先出现者(snap_targets 升序 → 较小者)
        assert_eq!(snap_time(120, 50, &targets), 100);
        // 空候选表
        assert_eq!(snap_time(42, 100, &[]), 42);
    }

    #[test]
    fn snap_targets_excludes_clip_and_includes_zero() {
        let (tl, ids) = demo_timeline();
        // 全部边缘:轨0 {0,500}∪{500,1000} + 轨1 {200,600} + 0
        assert_eq!(tl.snap_targets(None), vec![0, 200, 500, 600, 1000]);
        // 排除 b([500,1000)):其两个边缘消失
        assert_eq!(tl.snap_targets(Some(ids[1])), vec![0, 200, 500, 600]);
    }

    #[test]
    fn timeline_snap_respects_snap_enabled() {
        let (mut tl, ids) = demo_timeline();
        // 排除自身 b 后,495 最近候选是 a 的终点 500
        assert_eq!(tl.snap(495, 10, Some(ids[1])), 500);
        tl.snap_enabled = false;
        assert_eq!(tl.snap(495, 10, None), 495, "吸附关闭:原值返回");
    }

    #[test]
    fn ms_px_conversions_round_trip() {
        let tl = Timeline {
            px_per_second: 50.0,
            ..Timeline::new()
        };
        assert_eq!(tl.ms_to_px(1000), 50.0);
        assert_eq!(tl.px_to_ms(50.0), 1000);
        assert_eq!(tl.px_to_ms(-3.0), 0, "负像素钳 0");
        assert_eq!(tl.px_to_ms(tl.ms_to_px(1000)), 1000);
    }

    #[test]
    fn track_management_and_errors() {
        let (mut tl, _) = demo_timeline();
        assert_eq!(
            tl.track_of_kind(TrackKind::Sticker),
            Err(VideoError::NoActiveTrack(TrackKind::Sticker))
        );
        assert_eq!(tl.track_of_kind(TrackKind::Audio), Ok(1));
        let removed = tl.remove_track(1).expect("remove track 1");
        assert_eq!(removed.kind, TrackKind::Audio);
        assert_eq!(tl.tracks.len(), 1);
        assert_eq!(tl.duration(), 1000, "缓存随之收缩");
        // 拒绝删空 / 越界
        assert_eq!(tl.remove_track(0), Err(VideoError::EmptyTimeline));
        assert_eq!(tl.remove_track(5), Err(VideoError::TrackNotFound(5)));
    }

    #[test]
    fn clip_at_hit_test() {
        let (tl, ids) = demo_timeline();
        assert_eq!(tl.clip_at(0, 250), Some(ids[0]));
        assert_eq!(
            tl.clip_at(0, 500),
            Some(ids[1]),
            "端点归后一段 [start, end)"
        );
        assert_eq!(tl.clip_at(0, 999), Some(ids[1]));
        assert_eq!(tl.clip_at(0, 1000), None, "终点之外无命中");
        assert_eq!(tl.clip_at(1, 100), None, "空档无命中");
        assert_eq!(tl.tracks[1].clip_at(400).map(|c| c.id), Some(ids[2]));
    }

    #[test]
    fn serde_roundtrip_preserves_timeline() {
        let (mut tl, _) = demo_timeline();
        let b = tl.tracks[0].clips[1].id;
        tl.set_speed(b, 1.5).expect("speed");
        tl.snap_enabled = false;
        let json = serde_json::to_string(&tl).expect("serialize");
        let mut back: Timeline = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, tl);
        // 反序列化后再放置,id 从持久化的发号器继续(不复用旧 id)
        let new_id = back
            .place_clip(0, asset("post"), 2000, 100)
            .expect("place after load");
        assert_eq!(new_id.value(), 3, "demo 放了 3 个 clip,发号器持久化为 3");
    }

    #[test]
    fn partial_eq_ignores_id_counter() {
        let (mut tl, _) = demo_timeline();
        let snapshot = tl.clone();
        let victim = tl.tracks[0].clips[0].id;
        tl.remove_clip(victim).expect("remove");
        assert_ne!(tl, snapshot);
        tl.insert_clip_raw(0, snapshot.tracks[0].clips[0].clone())
            .expect("reinsert");
        assert_eq!(tl, snapshot, "删除又放回后内容等价(尽管 id 计数器已前进)");
    }

    // —— TC-RBT-VIDEO-01/02、TC-RBT-TL-01(迭代审查报告 RBT-07/RBT-08/§6 RB-13)——

    /// 构造最小 Timeline JSON(.sable 同构数据),注入指定 speed 字面量。
    fn timeline_json_with_speed(speed_json: &str) -> String {
        format!(
            r#"{{"tracks":[{{"kind":"Video","clips":[{{"id":0,"asset":{{"path":"/assets/v.mp4","hash":1}},"start_ms":0,"duration_ms":100,"in_ms":0,"out_ms":100,"speed":{speed_json},"keyframes":[]}}],"muted":false,"locked":false}}],"px_per_second":100.0,"snap_enabled":true,"duration_ms":100,"next_clip_id":1}}"#
        )
    }

    fn plain_clip(id: u64, speed: f64) -> Clip {
        Clip {
            id: ClipId(id),
            asset: AssetRef::new("/assets/v.mp4", 1),
            start_ms: 0,
            duration_ms: 100,
            in_ms: 0,
            out_ms: 100,
            speed,
            keyframes: Vec::new(),
        }
    }

    /// TC-RBT-VIDEO-01:经 .sable 同构序列化数据注入 speed=0/-1/NaN/inf,
    /// 反序列化必须返回结构化错误(不静默钳制、不 panic);坏出入点与
    /// 结构破坏(同轨重叠)同样在加载边界被拒收;合法文件照常加载。
    #[test]
    fn tc_rbt_video_01_deserialization_rejects_bad_values() {
        // speed = 0 / 负数:JSON 合法,必须由统一守卫谓词(而非解析器)拒绝
        for bad in ["0", "-1", "0.0", "-0.5"] {
            let err = serde_json::from_str::<Timeline>(&timeline_json_with_speed(bad))
                .expect_err("坏 speed 必须反序列化失败");
            assert!(
                err.to_string()
                    .contains("非法 clip 数据: speed 必须为有限正数"),
                "错误信息必须来自 InvalidClip 守卫: {err}"
            );
        }
        // ±inf / NaN 无法用 JSON 数字字面量表达,经同一守卫谓词(Clip::validate,
        // 即反序列化内部调用的同一函数)验证拒收
        for speed in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            let err = plain_clip(0, speed)
                .validate()
                .expect_err("非有限 speed 必须被守卫拒收");
            assert!(matches!(err, VideoError::InvalidClip(msg) if msg.contains("speed")));
        }
        // 1e999 溢出 f64:无论解析层还是守卫层拦截,都不得静默放行
        assert!(serde_json::from_str::<Timeline>(&timeline_json_with_speed("1e999")).is_err());
        // 出入点倒挂(out <= in)同样被守卫拒收
        let bad_range = timeline_json_with_speed("1.0")
            .replace("\"in_ms\":0,\"out_ms\":100", "\"in_ms\":200,\"out_ms\":100");
        let err = serde_json::from_str::<Timeline>(&bad_range).expect_err("出点必须大于入点");
        assert!(
            err.to_string().contains("出点必须大于入点"),
            "错误信息必须来自 InvalidClip 守卫: {err}"
        );
        // 结构破坏(同轨重叠)在加载入口被 Timeline::validate 拒收
        let overlapping = r#"{"tracks":[{"kind":"Video","clips":[
            {"id":0,"asset":{"path":"/a.mp4","hash":1},"start_ms":0,"duration_ms":100,"in_ms":0,"out_ms":100,"speed":1.0,"keyframes":[]},
            {"id":1,"asset":{"path":"/b.mp4","hash":1},"start_ms":50,"duration_ms":100,"in_ms":0,"out_ms":100,"speed":1.0,"keyframes":[]}],
            "muted":false,"locked":false}],
            "px_per_second":100.0,"snap_enabled":true,"duration_ms":150,"next_clip_id":2}"#;
        let err = serde_json::from_str::<Timeline>(overlapping).expect_err("重叠结构必须被拒收");
        assert!(
            err.to_string().contains("时间轴不变量被破坏"),
            "错误信息必须来自 InvariantViolated: {err}"
        );
        // 合法 speed 正常通过反序列化(守卫不误伤)
        let tl: Timeline =
            serde_json::from_str(&timeline_json_with_speed("1.5")).expect("合法文件必须可加载");
        assert!(tl.is_valid());
        assert_eq!(tl.tracks[0].clips[0].speed, 1.5);
    }

    /// TC-RBT-VIDEO-02:`set_speed` 与反序列化(Clip::validate)两条入口
    /// 对同一坏值拒收策略一致、错误变体一致;合法 speed 两条入口一致放行。
    #[test]
    fn tc_rbt_video_02_speed_guard_policy_identical() {
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            // 入口一:set_speed 拒收,且不改动时间轴
            let (mut tl, ids) = demo_timeline();
            let snapshot = tl.clone();
            assert!(
                matches!(tl.set_speed(ids[0], bad), Err(VideoError::InvalidClip(_))),
                "set_speed({bad}) 必须返回 InvalidClip"
            );
            assert_eq!(tl, snapshot, "被拒收的变速不得改动时间轴");
            // 入口二:同一坏值经反序列化守卫也是同一变体
            assert!(
                matches!(
                    plain_clip(0, bad).validate(),
                    Err(VideoError::InvalidClip(_))
                ),
                "同一坏值经反序列化守卫也必须返回 InvalidClip"
            );
        }
        // 反序列化错误信息与 InvalidClip 的 Display 同源(错误类型/策略一致)
        let err = serde_json::from_str::<Timeline>(&timeline_json_with_speed("0"))
            .expect_err("speed=0 必须被拒收");
        let guard_err = plain_clip(0, 0.0)
            .validate()
            .expect_err("speed=0 必须被守卫拒收");
        let VideoError::InvalidClip(msg) = guard_err else {
            panic!("测试前置失败:守卫必须返回 InvalidClip,实际 {guard_err:?}");
        };
        assert!(
            err.to_string()
                .contains(&VideoError::InvalidClip(msg).to_string()),
            "反序列化错误必须携带同一 InvalidClip 文本: {err}"
        );
        // 合法 speed:两条入口一致放行(单 clip 无右邻,变速不撞)
        for good in [0.1, 0.5, 1.0, 2.0, 100.0] {
            let mut tl = Timeline::new();
            tl.add_track(TrackKind::Video);
            let id = tl.place_clip(0, asset("v"), 0, 1000).expect("place");
            assert_eq!(tl.set_speed(id, good), Ok(()), "set_speed({good}) 必须放行");
            let clip = tl.clip(id).expect("clip");
            assert_eq!(clip.validate(), Ok(()), "合法 speed 加载守卫必须放行");
            assert!(tl.is_valid());
        }
    }

    /// TC-RBT-TL-01:非法修改必须返回 Err 而非静默通过(普通 cargo test
    /// 即验证,不依赖 debug_assert):变速 +inf 旧实现 release 下被放行
    /// (duration 归 1);波纹前移下溢在 release 回绕、debug 崩溃;两者
    /// 现在均为类型化 Err 且时间轴原状;常规非法修改依旧类型化拒收。
    #[test]
    fn tc_rbt_tl_01_illegal_mutations_rejected_not_silent() {
        // 1) 变速 +inf:显式拒收,时间轴原状
        let (mut tl, ids) = demo_timeline();
        let snapshot = tl.clone();
        assert!(matches!(
            tl.set_speed(ids[0], f64::INFINITY),
            Err(VideoError::InvalidClip(_))
        ));
        assert_eq!(tl, snapshot, "非法修改不得生效");

        // 2) 波纹删除前移下溢:不变量已被直改破坏时显式拒收(模拟历史脏数据;
        //    API 自身无法构造此状态),拒收后原状保留
        let mut tl = Timeline::new();
        tl.add_track(TrackKind::Video);
        let a = tl.place_clip(0, asset("a"), 0, 100).expect("a");
        let b = tl.place_clip(0, asset("b"), 100, 200).expect("b");
        tl.tracks[0].clips[1].start_ms = 50; // 绕过 API 直改制造不变量破坏
        assert!(matches!(
            tl.ripple_remove(a),
            Err(VideoError::InvariantViolated(_))
        ));
        assert_eq!(tl.clip(a).map(|c| c.start_ms), Some(0), "拒收后原状");
        assert_eq!(tl.clip(b).map(|c| c.start_ms), Some(50), "拒收后原状");

        // 3) 常规非法修改依旧走类型化 Result 拒收(回归:通道未被弱化)
        let (mut tl, ids) = demo_timeline();
        assert_eq!(
            tl.place_clip(0, asset("x"), 100, 100),
            Err(VideoError::Overlap)
        );
        assert_eq!(tl.move_clip(ids[1], 100), Err(VideoError::Overlap));
        assert!(matches!(
            tl.trim_clip(ids[0], 300, 300),
            Err(VideoError::InvalidRange(_))
        ));
        assert!(tl.is_valid(), "全部拒收后时间轴不变量必须成立");
    }

    // —— proptest:随机直接修改序列始终维持不变量 ——

    #[derive(Debug, Clone)]
    enum Op {
        Place { track: usize, start: u64, dur: u64 },
        Move { pick: usize, to: u64 },
        Trim { pick: usize, new_in: u64, len: u64 },
        Speed { pick: usize, tenth: u32 },
    }

    fn any_op() -> impl Strategy<Value = Op> {
        prop_oneof![
            3 => (0usize..2, 0u64..20, 1u64..6)
                .prop_map(|(t, s, d)| Op::Place { track: t, start: s * 100, dur: d * 100 }),
            3 => (0usize..8, 0u64..20).prop_map(|(p, s)| Op::Move { pick: p, to: s * 100 }),
            2 => (0usize..8, 0u64..8, 1u64..8)
                .prop_map(|(p, i, l)| Op::Trim { pick: p, new_in: i * 100, len: l * 100 }),
            1 => (0usize..8, 1u32..30).prop_map(|(p, s)| Op::Speed { pick: p, tenth: s }),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn random_mutations_keep_invariants(ops in proptest::collection::vec(any_op(), 1..40)) {
            let (mut tl, ids) = demo_timeline();
            let mut registry = ids;
            for op in ops {
                match op {
                    Op::Place { track, start, dur } => {
                        if let Ok(id) = tl.place_clip(track, asset("gen"), start, dur) {
                            registry.push(id);
                        }
                    }
                    Op::Move { pick, to } => {
                        if let Some(id) = registry.get(pick % registry.len().max(1)).copied() {
                            let _ = tl.move_clip(id, to);
                        }
                    }
                    Op::Trim { pick, new_in, len } => {
                        if let Some(id) = registry.get(pick % registry.len().max(1)).copied() {
                            let _ = tl.trim_clip(id, new_in, new_in + len);
                        }
                    }
                    Op::Speed { pick, tenth } => {
                        if let Some(id) = registry.get(pick % registry.len().max(1)).copied() {
                            let _ = tl.set_speed(id, f64::from(tenth) / 10.0);
                        }
                    }
                }
                prop_assert!(tl.is_valid(), "每次修改后不变量必须成立");
            }
        }
    }
}
