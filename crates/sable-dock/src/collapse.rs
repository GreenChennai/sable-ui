//! 窄窗折叠与坞宽策略(迭代审查报告 §5.7.1,上游 §4.3 的库侧行为规格)。
//!
//! 全部**纯函数**,零 gpui 依赖(窗口宽度由宿主传入),单测钉死;
//! `DockArea` 级接线(折叠为图标条/拖拽钳制)随 gpui-component 升级窗口
//! 落地(TD-01/02),本模块提供可测的策略真相源与动画插值件。

/// 断点:窗口宽度低于此值 → 左右坞强制折叠为图标条(手动展开不可覆盖)。
pub const FOLD_BREAKPOINT_PX: f64 = 1200.0;

/// 折叠态图标条宽度(px)。
pub const ICON_STRIP_PX: f64 = 40.0;

/// 展开态默认宽:左坞 / 右坞(px;剪映/达芬奇习惯,左窄右宽)。
pub const DEFAULT_LEFT_W: f64 = 260.0;
pub const DEFAULT_RIGHT_W: f64 = 300.0;

/// 展开宽拖拽范围(报告 §5.7.1:240–420)。
pub const MIN_DOCK_W: f64 = 240.0;
pub const MAX_DOCK_W: f64 = 420.0;

/// 折叠策略(纯数据;宿主据此设置 DockArea 面板可见性与宽度)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CollapsePolicy {
    /// `true` = 左右坞折叠为图标条(<1200px 强制,手动展开不可覆盖)。
    pub fold_side_docks: bool,
    /// 折叠态图标条宽度。
    pub icon_strip_px: f64,
    /// 左坞默认/回落宽。
    pub default_left_w: f64,
    /// 右坞默认/回落宽。
    pub default_right_w: f64,
    /// 展开宽拖拽下限。
    pub drag_min_w: f64,
    /// 展开宽拖拽上限。
    pub drag_max_w: f64,
}

/// 窗口宽 → 折叠策略(纯函数;`window_width <= 0` 视为未知,不折叠——
/// 首帧 bounds 未回写时保守不折叠,绝不把可见面板藏掉)。
#[must_use]
pub fn collapse_policy(window_width: f64) -> CollapsePolicy {
    let fold = window_width > 0.0 && window_width < FOLD_BREAKPOINT_PX;
    CollapsePolicy {
        fold_side_docks: fold,
        icon_strip_px: ICON_STRIP_PX,
        default_left_w: DEFAULT_LEFT_W,
        default_right_w: DEFAULT_RIGHT_W,
        drag_min_w: MIN_DOCK_W,
        drag_max_w: MAX_DOCK_W,
    }
}

/// 展开宽钳制(纯函数;NaN/非有限 → 左坞默认宽)。
#[must_use]
pub fn clamp_dock_width(w: f64) -> f64 {
    if !w.is_finite() {
        return DEFAULT_LEFT_W;
    }
    w.clamp(MIN_DOCK_W, MAX_DOCK_W)
}

/// 折叠/展开动画(ANI-01 #3 面板折叠插值的可测内核)。
///
/// `from → to` 沿 [`sable_widgets::tokens::MotionTokens`] 的 PANEL 档
/// (200ms)OutCubic 插值;`reduced_motion` 直切。DockArea 级接线随
/// gpui-component 升级窗口落地,本件供宿主/未来接线复用。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoldAnimation {
    from: f64,
    to: f64,
    started_ms: f64,
}

/// PANEL 档时长(ms;与 widgets MotionTokens 同值——tokens 是唯一真相,
/// dock 不依赖 widgets 时以常量镜像并在测试中钉死一致性)。
pub const FOLD_PANEL_MS: f64 = 200.0;

impl FoldAnimation {
    /// 构造一次折叠/展开动画。
    #[must_use]
    pub fn new(from: f64, to: f64, started_ms: f64) -> Self {
        FoldAnimation {
            from,
            to,
            started_ms,
        }
    }

    /// 进度 0..=1(200ms OutCubic;`reduced_motion` 直切 0/1;
    /// 未开始/已完成钳制,负 dt 恒 0)。
    #[must_use]
    pub fn progress_at(&self, now_ms: f64, reduced_motion: bool) -> f32 {
        if reduced_motion {
            return if now_ms > self.started_ms { 1.0 } else { 0.0 };
        }
        if now_ms <= self.started_ms {
            return 0.0;
        }
        let t = ((now_ms - self.started_ms) / FOLD_PANEL_MS).min(1.0);
        let eased = 1.0 - (1.0 - t) * (1.0 - t); // OutCubic 二次缓出(与 widgets OutCubic 同族)
        eased as f32
    }

    /// 当前宽度(线性作用于 from/to;进度已含缓动)。
    #[must_use]
    pub fn width_at(&self, now_ms: f64, reduced_motion: bool) -> f64 {
        let p = f64::from(self.progress_at(now_ms, reduced_motion));
        self.from + (self.to - self.from) * p
    }

    /// 是否落定(宿主据此停帧:静止零帧提交纪律)。
    #[must_use]
    pub fn settled(&self, now_ms: f64) -> bool {
        now_ms - self.started_ms >= FOLD_PANEL_MS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_dock_collapse_policy_breakpoint_and_unknown_width() {
        // <1200 强制折叠;>=1200 默认展开;<=0(未知)保守不折叠
        assert!(collapse_policy(800.0).fold_side_docks);
        assert!(collapse_policy(1199.9).fold_side_docks);
        assert!(!collapse_policy(1200.0).fold_side_docks);
        assert!(!collapse_policy(1920.0).fold_side_docks);
        assert!(!collapse_policy(0.0).fold_side_docks, "未知宽不折叠");
        assert!(!collapse_policy(-1.0).fold_side_docks);
        // 常量与报告口径一致
        let p = collapse_policy(1920.0);
        assert_eq!(
            (
                p.icon_strip_px,
                p.default_left_w,
                p.default_right_w,
                p.drag_min_w,
                p.drag_max_w
            ),
            (40.0, 260.0, 300.0, 240.0, 420.0)
        );
    }

    #[test]
    fn tc_dock_collapse_clamp_and_nan() {
        assert_eq!(clamp_dock_width(100.0), MIN_DOCK_W);
        assert_eq!(clamp_dock_width(9999.0), MAX_DOCK_W);
        assert_eq!(clamp_dock_width(300.0), 300.0);
        assert_eq!(clamp_dock_width(f64::NAN), DEFAULT_LEFT_W);
        assert_eq!(clamp_dock_width(f64::INFINITY), DEFAULT_LEFT_W);
    }

    #[test]
    fn tc_dock_fold_anim_progress_settle_and_reduced() {
        let anim = FoldAnimation::new(260.0, 40.0, 1000.0);
        assert_eq!(anim.progress_at(1000.0, false), 0.0);
        assert_eq!(anim.progress_at(500.0, false), 0.0, "未开始恒 0");
        let mid = anim.progress_at(1100.0, false);
        assert!(
            (0.0..1.0).contains(&mid) && mid > f32::EPSILON,
            "中途单调推进"
        );
        assert_eq!(anim.progress_at(1200.0, false), 1.0, "200ms 落定精确 1");
        assert!(anim.settled(1200.0));
        assert!(!anim.settled(1199.9));
        // reduced 直切
        assert_eq!(anim.progress_at(1000.0, true), 0.0);
        assert_eq!(anim.progress_at(1000.5, true), 1.0);
        // 宽度插值端点
        assert_eq!(anim.width_at(1200.0, false), 40.0);
        assert_eq!(anim.width_at(1000.0, false), 260.0);
        // PANEL 档一致性(与 widgets MotionTokens 200ms 同值)
        assert_eq!(FOLD_PANEL_MS, 200.0);
    }
}
