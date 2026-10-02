//! A1 编排引擎 AnimationTimeline——GSAP 式时间轴(08 迭代计划 A1)。
//!
//! 串行 [`AnimationTimeline::then`] / 并行 [`AnimationTimeline::together`] /
//! 交错 [`AnimationTimeline::stagger`] / 循环 [`AnimationTimeline::repeat`] /
//! 往返 [`AnimationTimeline::yoyo`];时间由调用方显式注入
//! ([`AnimationTimeline::value_at`] / [`AnimationTimeline::sample_at`] 收
//! `t_ms`),纯函数采样、确定性可单测。
//!
//! # 模型
//!
//! 时间轴是**有序添加的段列表**([`TimelineEntry`],段 = 起始时刻 + 时长 + 缓动);
//! 每次 `then`/`together`/`stagger` 把 [`Segment`] 定位到当前游标并推进游标
//! (游标 = 已有段的最右端点,时间轴总从 0 开始):
//!
//! - `then(seg)`:段从游标起,游标推进到段尾(串行接续);
//! - `together(segs)`:各段同时从游标起,游标推进到最长段尾(并行);
//! - `stagger(template, count, gap)`:第 i 段从 `游标 + i·gap` 起,游标推进到
//!   `(count-1)·gap + 最长段尾`(交错;`gap < 0` 按 0 处理);
//! - `repeat(n)`:`n = 0` 无限循环,`n > 0` 追加 n 遍(共 n+1 遍,GSAP 语义);
//! - `yoyo()`:奇数遍反向播放(无 repeat 时 = 去程 + 回程一遍)。
//!
//! 段只承载**进度 0→1**(`from`/`to` 由调用方对进度自行插值,时间轴不关心值
//! 类型);段未开始记 0、已结束记 1(保持终态)。多轨采样用
//! [`AnimationTimeline::sample_at`](顺序 = 添加序),单轨标量用
//! [`AnimationTimeline::value_at`](取活动段:段交界取前段终态)。
//!
//! A8:[`crate::anim::reduced_motion`] 为真时采样直通终态(有限遍取末遍终点;
//! 无限循环取首遍尾)。

use super::easing::Easing;
use super::reduced_motion;

/// 一段可采样的编排单元:绝对起始时刻 + 时长 + 缓动(进度 0→1)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelineEntry {
    start_ms: f64,
    duration_ms: f64,
    ease: Easing,
}

impl TimelineEntry {
    /// 段起始时刻(ms,时间轴坐标)。
    pub fn start_ms(&self) -> f64 {
        self.start_ms
    }

    /// 段时长(ms)。
    pub fn duration_ms(&self) -> f64 {
        self.duration_ms
    }

    /// 段缓动。
    pub fn ease(&self) -> Easing {
        self.ease
    }

    /// 在时刻 `t_ms` 的进度:未开始 0、进行中缓动插值、结束 1(保持终态)。
    fn sample(&self, t_ms: f64) -> f64 {
        let end = self.start_ms + self.duration_ms;
        if self.duration_ms <= 0.0 {
            // 零时长段:到达即完成(与 Animated 零时长立刻到位一致)
            return if t_ms >= end { 1.0 } else { 0.0 };
        }
        if t_ms <= self.start_ms {
            return 0.0;
        }
        if t_ms >= end {
            return 1.0;
        }
        self.ease.apply((t_ms - self.start_ms) / self.duration_ms)
    }
}

/// 待编排的段描述:`start_ms` 是**相对插入游标的偏移**(0 = 紧接当前游标),
/// 定位由时间轴完成——`from`/`to` 由调用方经进度自行插值,段本身不携带值。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    start_ms: f64,
    duration_ms: f64,
    ease: Easing,
}

impl Segment {
    /// 从游标起播、时长 `duration_ms` 的新段。
    pub fn new(duration_ms: f64, ease: Easing) -> Self {
        Segment {
            start_ms: 0.0,
            duration_ms,
            ease,
        }
    }

    /// 相对游标偏移 `start_ms` 起播的段(负值 = 与前段重叠)。
    pub fn with_offset(start_ms: f64, duration_ms: f64, ease: Easing) -> Self {
        Segment {
            start_ms,
            duration_ms,
            ease,
        }
    }

    /// 相对游标的起始偏移(ms)。
    pub fn offset_ms(&self) -> f64 {
        self.start_ms
    }

    /// 段时长(ms)。
    pub fn duration_ms(&self) -> f64 {
        self.duration_ms
    }

    /// 段缓动。
    pub fn ease(&self) -> Easing {
        self.ease
    }
}

/// GSAP 式编排时间轴(A1):`then`/`together`/`stagger`/`repeat`/`yoyo`,
/// 时间显式注入、确定性采样。Clone 后可派生变体(共享已编排内容)。
#[derive(Clone, Debug, Default)]
pub struct AnimationTimeline {
    entries: Vec<TimelineEntry>,
    cursor_ms: f64,
    /// `None` = 单遍(无 repeat);`Some(0)` = 无限循环;`Some(n)` = 追加 n 遍
    repeat: Option<u32>,
    yoyo: bool,
}

impl AnimationTimeline {
    /// 空时间轴。
    pub fn new() -> Self {
        AnimationTimeline {
            entries: Vec::new(),
            cursor_ms: 0.0,
            repeat: None,
            yoyo: false,
        }
    }

    /// 串行接续:段从当前游标起,游标推进到段尾。
    pub fn then(mut self, seg: Segment) -> Self {
        let anchor = self.cursor_ms;
        self.place_at(seg, anchor, 0.0);
        self
    }

    /// 并行:各段**同时**从调用时刻的游标快照起(可带各自的
    /// [`Segment::with_offset`] 微调),游标推进到最长段尾。
    pub fn together(mut self, segs: Vec<Segment>) -> Self {
        let anchor = self.cursor_ms; // 游标快照:并行各段同锚,不随前一段推进
        for seg in segs {
            self.place_at(seg, anchor, 0.0);
        }
        self
    }

    /// 交错:第 i 段 = `template(i)` 从 `游标 + i·gap_ms` 起(GSAP stagger),
    /// 游标推进到 `(count-1)·gap_ms + 最长段尾`。`count = 0` 为空操作,
    /// `gap_ms < 0` 按 0 处理。
    pub fn stagger(
        mut self,
        seg_template: impl Fn(usize) -> Segment,
        count: usize,
        gap_ms: f64,
    ) -> Self {
        let anchor = self.cursor_ms; // 游标快照:第 i 段起点相对同一锚点
        let gap = gap_ms.max(0.0);
        for i in 0..count {
            let seg = seg_template(i);
            self.place_at(seg, anchor, i as f64 * gap);
        }
        self
    }

    /// 循环:`times = 0` 无限循环;`times > 0` 追加 `times` 遍(共 times+1 遍)。
    /// 重复调用以最后一次为准。
    pub fn repeat(mut self, times: u32) -> Self {
        self.repeat = Some(times);
        self
    }

    /// 往返:奇数遍反向播放。无 [`Self::repeat`] 时 = 去程 + 回程共 2 遍;
    /// 与 repeat(n) 叠加共 n+1 遍、方向交替(F、B、F…)。
    pub fn yoyo(mut self) -> Self {
        self.yoyo = true;
        self
    }

    /// 单遍内容时长(ms;不含 repeat/yoyo 展开)。
    pub fn duration(&self) -> f64 {
        self.cursor_ms
    }

    /// 含 repeat/yoyo 的总时长(ms);无限循环返回 [`f64::INFINITY`]。
    pub fn total_duration(&self) -> f64 {
        match self.repeat {
            Some(0) => f64::INFINITY,
            Some(n) => self.duration() * (u64::from(n) + 1) as f64,
            None => {
                if self.yoyo {
                    self.duration() * 2.0
                } else {
                    self.duration()
                }
            }
        }
    }

    /// 已编排的段(按添加序)。
    pub fn entries(&self) -> &[TimelineEntry] {
        &self.entries
    }

    /// 单轨标量进度采样:**活动段**优先——添加序第一个覆盖当前时刻的段
    /// ([start, end] 闭端点,段交界取前段终态);无覆盖段时取最后一个已
    /// 开始的段(已结束保持终态、间隙保持);无一开始则 0.0。空时间轴
    /// 返回 0.0。多轨用 [`Self::sample_at`]。
    pub fn value_at(&self, t_ms: f64) -> f64 {
        let local = self.content_time_at(t_ms);
        let mut active: Option<&TimelineEntry> = None;
        for entry in &self.entries {
            if entry.start_ms <= local && local <= entry.start_ms + entry.duration_ms {
                active = Some(entry);
                break;
            }
            if entry.start_ms <= local {
                active = Some(entry); // 兜底候选:最后一个已开始的段
            }
        }
        active.map_or(0.0, |e| e.sample(local))
    }

    /// 全轨进度采样(顺序 = 添加序)。
    pub fn sample_at(&self, t_ms: f64) -> Vec<f64> {
        let local = self.content_time_at(t_ms);
        self.entries.iter().map(|e| e.sample(local)).collect()
    }

    /// 把段定位到 `anchor_ms + 段偏移 + 额外偏移` 并推进游标(游标只进不退)。
    ///
    /// `anchor_ms` 由调用方快照(`then` 用当前游标;`together`/`stagger` 用
    /// 进入时的游标快照,保证并行/交错各段同锚)——本方法会推进游标,循环中
    /// 直接复用 `self.cursor_ms` 会把并行段逐个串行化(回归教训)。
    fn place_at(&mut self, seg: Segment, anchor_ms: f64, extra_offset_ms: f64) {
        let entry = TimelineEntry {
            start_ms: anchor_ms + seg.start_ms + extra_offset_ms,
            duration_ms: seg.duration_ms.max(0.0),
            ease: seg.ease,
        };
        self.cursor_ms = self.cursor_ms.max(entry.start_ms + entry.duration_ms);
        self.entries.push(entry);
    }

    /// 把注入的 `t_ms` 映射为单遍内容时间(处理 repeat/yoyo 与钳制)。
    ///
    /// 负 t / NaN → 0(起点);超出总时长 → 末遍终点钳制(yoyo 末遍反向时
    /// 终点即 0)。A8:reduced_motion 直通终态。
    fn content_time_at(&self, t_ms: f64) -> f64 {
        let base = self.duration();
        if base <= 0.0 {
            return 0.0;
        }
        if reduced_motion() {
            return match self.repeat {
                Some(0) => base, // 无限循环无终态,取首遍尾(文档约定)
                Some(n) => {
                    if self.yoyo && n % 2 == 1 {
                        0.0 // 末遍(n+1 遍的最后一遍)反向,终点 = 起点
                    } else {
                        base
                    }
                }
                None => {
                    if self.yoyo {
                        0.0 // 去程 + 回程,终态回到起点
                    } else {
                        base
                    }
                }
            };
        }
        let t = t_ms.max(0.0); // 同时消化 NaN(f64::max 返回非 NaN 一侧)
        match self.repeat {
            Some(0) => {
                let p = (t / base).floor();
                let local = (t - p * base).clamp(0.0, base);
                if self.yoyo && (p as u64) & 1 == 1 {
                    base - local
                } else {
                    local
                }
            }
            Some(n) => {
                let passes = u64::from(n) + 1;
                let p = ((t / base).floor() as u64).min(passes - 1);
                let local = (t - p as f64 * base).clamp(0.0, base);
                if self.yoyo && p & 1 == 1 {
                    base - local
                } else {
                    local
                }
            }
            None => {
                if self.yoyo {
                    let local = t.clamp(0.0, base * 2.0);
                    if local <= base {
                        local
                    } else {
                        base * 2.0 - local
                    }
                } else {
                    t.clamp(0.0, base)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIN: Easing = Easing::Linear;

    #[test]
    fn then_chains_serially_and_holds_endpoints() {
        let tl = AnimationTimeline::new()
            .then(Segment::new(100.0, LIN))
            .then(Segment::new(100.0, LIN));
        assert_eq!(tl.duration(), 200.0);
        assert_eq!(tl.value_at(0.0), 0.0);
        assert_eq!(tl.value_at(50.0), 0.5);
        assert_eq!(tl.value_at(100.0), 1.0, "第一段结束保持终态");
        assert_eq!(tl.value_at(150.0), 0.5);
        assert_eq!(tl.value_at(999.0), 1.0, "超出总时长钳在终态");
        assert_eq!(tl.value_at(-5.0), 0.0, "负 t 钳在起点");
    }

    #[test]
    fn together_runs_parallel_and_advances_cursor_by_longest() {
        let tl = AnimationTimeline::new()
            .then(Segment::new(100.0, LIN))
            .together(vec![Segment::new(50.0, LIN), Segment::new(80.0, LIN)]);
        assert_eq!(tl.duration(), 180.0, "游标推进到最长并行段尾");
        assert_eq!(tl.entries().len(), 3);
        // t=120:第一轨已完(1.0);第二轨 20/50;第三轨 20/80
        let s = tl.sample_at(120.0);
        assert_eq!(s[0], 1.0);
        assert!((s[1] - 0.4).abs() < 1e-12);
        assert!((s[2] - 0.25).abs() < 1e-12);
    }

    #[test]
    fn stagger_three_tracks_frame_by_frame() {
        // A1 验收:3 轨道 stagger 时间轴逐帧采样断言
        let tl = AnimationTimeline::new().stagger(|_| Segment::new(100.0, LIN), 3, 30.0);
        assert_eq!(tl.duration(), 160.0, "(3-1)·30 + 100");
        let expected: [(f64, [f64; 3]); 17] = [
            (0.0, [0.0, 0.0, 0.0]),
            (10.0, [0.1, 0.0, 0.0]),
            (20.0, [0.2, 0.0, 0.0]),
            (30.0, [0.3, 0.0, 0.0]),
            (40.0, [0.4, 0.1, 0.0]),
            (50.0, [0.5, 0.2, 0.0]),
            (60.0, [0.6, 0.3, 0.0]),
            (70.0, [0.7, 0.4, 0.1]),
            (80.0, [0.8, 0.5, 0.2]),
            (90.0, [0.9, 0.6, 0.3]),
            (100.0, [1.0, 0.7, 0.4]),
            (110.0, [1.0, 0.8, 0.5]),
            (120.0, [1.0, 0.9, 0.6]),
            (130.0, [1.0, 1.0, 0.7]),
            (140.0, [1.0, 1.0, 0.8]),
            (150.0, [1.0, 1.0, 0.9]),
            (160.0, [1.0, 1.0, 1.0]),
        ];
        for (t, exp) in expected {
            let got = tl.sample_at(t);
            for (g, e) in got.iter().zip(exp.iter()) {
                assert!((g - e).abs() < 1e-12, "t={t}: 期望 {exp:?} 实际 {got:?}");
            }
        }
        // stagger 各轨起始时刻:0 / 30 / 60
        let starts: Vec<f64> = tl.entries().iter().map(|e| e.start_ms()).collect();
        assert_eq!(starts, vec![0.0, 30.0, 60.0]);
    }

    #[test]
    fn repeat_clamps_negative_and_beyond_total() {
        // repeat(2) = 共 3 遍,总 300ms
        let tl = AnimationTimeline::new()
            .then(Segment::new(100.0, LIN))
            .repeat(2);
        assert_eq!(tl.total_duration(), 300.0);
        assert_eq!(tl.value_at(-1.0), 0.0, "负 t 钳起点");
        assert_eq!(tl.value_at(250.0), 0.5, "第三遍中点");
        assert_eq!(tl.value_at(300.0), 1.0);
        assert_eq!(tl.value_at(9999.0), 1.0, "超总时长钳终态");
        // 无限循环按模映射
        let inf = AnimationTimeline::new()
            .then(Segment::new(100.0, LIN))
            .repeat(0);
        assert_eq!(inf.total_duration(), f64::INFINITY);
        assert_eq!(inf.value_at(250.0), 0.5);
        assert_eq!(inf.value_at(1000.0), 0.0, "整周期回到起点");
    }

    #[test]
    fn yoyo_reverses_odd_passes_and_returns_to_start() {
        let tl = AnimationTimeline::new()
            .then(Segment::new(100.0, LIN))
            .yoyo();
        assert_eq!(tl.total_duration(), 200.0);
        assert_eq!(tl.value_at(50.0), 0.5, "去程");
        assert_eq!(tl.value_at(150.0), 0.5, "回程中点 = 内容 50ms");
        assert_eq!(tl.value_at(200.0), 0.0, "回程终点回到起点");
        assert_eq!(tl.value_at(9999.0), 0.0, "yoyo 终态 = 起点");
        // repeat(2) + yoyo = F、B、F 三遍,终态 = 终点
        let fy = AnimationTimeline::new()
            .then(Segment::new(100.0, LIN))
            .repeat(2)
            .yoyo();
        assert_eq!(fy.total_duration(), 300.0);
        assert_eq!(fy.value_at(150.0), 0.5, "第二遍反向");
        assert_eq!(fy.value_at(250.0), 0.5);
        assert_eq!(fy.value_at(300.0), 1.0, "第三遍正向,终态 = 终点");
    }

    #[test]
    fn stagger_accepts_template_variants_and_zero_count() {
        // 模板按序号变时长;count=0 空操作
        let tl = AnimationTimeline::new().stagger(
            |i| Segment::new(100.0 + 10.0 * i as f64, LIN),
            2,
            20.0,
        );
        assert_eq!(tl.duration(), 130.0, "20 + 110");
        let empty = AnimationTimeline::new().stagger(|_| Segment::new(50.0, LIN), 0, 10.0);
        assert_eq!(empty.duration(), 0.0);
        assert!(empty.sample_at(10.0).is_empty());
        // 负 gap 按 0 处理
        let neg = AnimationTimeline::new().stagger(|_| Segment::new(50.0, LIN), 2, -5.0);
        assert_eq!(neg.duration(), 50.0);
    }

    #[test]
    fn value_at_samples_active_segment() {
        let tl = AnimationTimeline::new()
            .then(Segment::new(100.0, LIN))
            .together(vec![Segment::new(40.0, LIN)]);
        assert_eq!(tl.value_at(20.0), 0.2, "第一段活动期内取第一段");
        assert_eq!(tl.value_at(120.0), 0.5, "第一段结束后取活动段(第二轨)");
        assert_eq!(tl.value_at(100.0), 1.0, "段交界取前段终态");
        assert_eq!(tl.sample_at(20.0).len(), 2, "多轨走 sample_at");
        assert_eq!(AnimationTimeline::new().value_at(5.0), 0.0, "空时间轴 0");
    }

    #[test]
    fn reduced_motion_jumps_to_final_state() {
        super::super::set_reduced_motion(true);
        // 有限遍:正向终态
        let fwd = AnimationTimeline::new()
            .then(Segment::new(100.0, LIN))
            .repeat(2);
        assert_eq!(fwd.value_at(0.0), 1.0, "减弱动态:直通终态");
        assert_eq!(fwd.sample_at(42.0), vec![1.0]);
        // yoyo 有限遍:终态回到起点
        let yoyo = AnimationTimeline::new()
            .then(Segment::new(100.0, LIN))
            .yoyo();
        assert_eq!(yoyo.value_at(42.0), 0.0, "yoyo 终态 = 起点");
        // 无限循环:取首遍尾
        let inf = AnimationTimeline::new()
            .then(Segment::new(100.0, LIN))
            .repeat(0);
        assert_eq!(inf.value_at(42.0), 1.0);
        super::super::set_reduced_motion(false);
    }
}
