//! A11 关键帧桥(08 迭代计划 A11):sable-video 的 Easing/关键帧求值 →
//! 本库动画语义的互通层。
//!
//! # 为什么需要桥(分册四 §9 / 08-A11 验收:预览 = 导出)
//!
//! 曲线编辑器/时间轴的关键帧数据模型住在 sable-video(`model::Keyframe`,
//! `t_ms`/`value`/`easing`),其缓动枚举是**预设五值**;本库动画引擎用自有
//! [`Easing`](crate::anim::Easing)(CSS cubic-bezier 全参数)。桥把两侧钉在
//! 一起:UI 预览动画(本库引擎采样)= 真实导出结果(video 播放器
//! `curve::evaluate` 采样)。
//!
//! # 映射表
//!
//! | video `model::Easing` | 本库 [`Easing`](crate::anim::Easing) | 一致性 |
//! |---|---|---|
//! | `Linear` | `Linear` | 逐点一致 |
//! | `InCubic` / `OutCubic` / `InOutCubic` | 同名 | 逐点一致(公式逐字相同) |
//! | `Spring` | `CubicBezier`(ease-back 0.34,1.56,0.64,1.0) | **观感近似**:
//!   video 侧是解析欠阻尼弹簧 `1-e^(-6u)·cos(10u)`(带多次衰减回摆),本库
//!   cubic-bezier 只能表达单次回摆——端点 0/1 精确,中段曲线不逐点相等;
//!   见模块测试的容差断言 |
//!
//! # 门控
//!
//! 模块在 `timeline` feature 下编译(依赖可选的 sable-video);anim 引擎
//! 本身仍零依赖 video(基础编辑场景不背这份依赖)。
//!
//! 任务书草稿写的 `sable_video::curve::Easing` 实际住在
//! `sable_video::model`(curve.rs 从 model 导入),本桥按真实路径引用。

use crate::anim::Easing;
use sable_video::model::{Easing as VideoEasing, Keyframe};

/// video 预设缓动 → 本库 Easing(预设一一映射;Spring → ease-back 观感近似,
/// 见模块 doc 映射表)。
pub fn bridge_easing(e: VideoEasing) -> Easing {
    match e {
        VideoEasing::Linear => Easing::Linear,
        VideoEasing::InCubic => Easing::InCubic,
        VideoEasing::OutCubic => Easing::OutCubic,
        VideoEasing::InOutCubic => Easing::InOutCubic,
        VideoEasing::Spring => Easing::CubicBezier {
            x1: 0.34,
            y1: 1.56,
            x2: 0.64,
            y2: 1.0,
        },
    }
}

/// 用本库 Easing 求值 video 关键帧表(与 `sable_video::curve::evaluate` 同语义):
///
/// - 空表 → `None`;
/// - `t_ms` 在首帧之前/末帧之后 → 钳制为首/末帧的值;
/// - 单关键帧 → 恒返回该值;
/// - 区间内 → 左关键帧的缓动(经 [`bridge_easing`])作用于区间进度 u,再对
///   两端值线性插值;
/// - 零宽段(重复时间戳)按跳变处理;约定 `keys` 按 `t_ms` 升序。
pub fn eval_keyframes(keys: &[Keyframe], t_ms: u64) -> Option<f64> {
    let first = keys.first()?;
    let last = keys.last()?;
    if t_ms <= first.t_ms {
        return Some(first.value);
    }
    if t_ms >= last.t_ms {
        return Some(last.value);
    }
    // 上式保证 first.t < t < last.t,故 split ∈ [1, len-1]
    let split = keys.partition_point(|k| k.t_ms <= t_ms);
    let k0 = keys.get(split.wrapping_sub(1));
    let k1 = keys.get(split);
    let (Some(k0), Some(k1)) = (k0, k1) else {
        return Some(last.value); // 理论不可达,防御性钳制(与 video evaluate 同)
    };
    if k1.t_ms <= k0.t_ms {
        return Some(k1.value); // 零宽段:跳变
    }
    let u = (t_ms - k0.t_ms) as f64 / (k1.t_ms - k0.t_ms) as f64;
    let eased = bridge_easing(k0.easing).apply(u.clamp(0.0, 1.0));
    Some(k0.value + (k1.value - k0.value) * eased)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sable_video::curve;

    fn key(t_ms: u64, value: f64, easing: VideoEasing) -> Keyframe {
        Keyframe {
            t_ms,
            value,
            easing,
        }
    }

    #[test]
    fn bridge_easing_endpoints_are_zero_and_one() {
        for v in [
            VideoEasing::Linear,
            VideoEasing::InCubic,
            VideoEasing::OutCubic,
            VideoEasing::InOutCubic,
            VideoEasing::Spring,
        ] {
            let e = bridge_easing(v);
            assert_eq!(e.apply(0.0), 0.0, "{v:?}: 端点 0");
            assert_eq!(e.apply(1.0), 1.0, "{v:?}: 端点 1");
        }
    }

    #[test]
    fn eval_matches_video_evaluate_pointwise_on_cubic_presets() {
        // 同一表同一 t,双实现对照:cubic 预设经桥后逐点一致(预览 = 导出)
        let keys = [
            key(0, 0.0, VideoEasing::Linear),
            key(100, 10.0, VideoEasing::InCubic),
            key(200, -10.0, VideoEasing::OutCubic),
            key(300, 5.0, VideoEasing::InOutCubic),
            key(400, 42.0, VideoEasing::Linear),
        ];
        for t in (0..=400_u64).step_by(7) {
            let ours = eval_keyframes(&keys, t);
            let theirs = curve::evaluate(&keys, t);
            assert_eq!(ours, theirs, "t={t}: 桥求值必须与 video evaluate 一致");
        }
        // 钳制区(首帧前/末帧后)同样一致
        for t in [0, 500, 100_000] {
            assert_eq!(eval_keyframes(&keys, t), curve::evaluate(&keys, t));
        }
    }

    #[test]
    fn eval_edge_cases_match_video() {
        assert_eq!(eval_keyframes(&[], 0), None, "空表 None");
        let single = [key(500, 7.5, VideoEasing::InCubic)];
        for t in [0, 500, 100_000] {
            assert_eq!(eval_keyframes(&single, t), Some(7.5), "单关键帧恒值");
        }
        // 零宽段跳变
        let jump = [
            key(0, 1.0, VideoEasing::Linear),
            key(50, 9.0, VideoEasing::Linear),
            key(50, 9.0, VideoEasing::Linear),
            key(100, 2.0, VideoEasing::Linear),
        ];
        for t in [25, 50, 75] {
            assert_eq!(eval_keyframes(&jump, t), curve::evaluate(&jump, t));
        }
    }

    #[test]
    fn spring_bridge_is_an_approximation_with_exact_endpoints() {
        // 映射表契约:Spring 是观感近似(单次回摆),中段不逐点相等;区间
        // 端点行为一致(区间起止取关键帧值,由区间逻辑保证,与缓动无关)
        let keys = [
            key(0, 0.0, VideoEasing::Spring),
            key(100, 5.0, VideoEasing::Linear),
        ];
        assert_eq!(eval_keyframes(&keys, 0), Some(0.0));
        assert_eq!(eval_keyframes(&keys, 100), Some(5.0));
        let ours = eval_keyframes(&keys, 50).expect("区间内必有值");
        let theirs = curve::evaluate(&keys, 50).expect("区间内必有值");
        assert!(
            (ours - theirs).abs() < 0.75,
            "近似桥与解析弹簧同量级:ours={ours} theirs={theirs}"
        );
        // 且确实带"过冲"观感(ease-back y 可越界 1;解析弹簧在 u=0.5 处尚未过冲)
        assert!(ours > 5.0, "ease-back 近似有过冲:ours={ours}");
    }
}
