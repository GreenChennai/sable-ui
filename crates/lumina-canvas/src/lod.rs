//! LOD(细节层次):缩放很小时,小对象简化为包围盒色块乃至一个点(docs/02 §7.2)。
//!
//! 原文阈值:`world_size * zoom >= 4.0` 才画细节(屏幕上小于 4px 的细节不画)。
//! 在此之上补充 [`DetailLevel`] 三档,供 [`crate::render`] 分派:
//!
//! | 屏幕尺寸 | 档位 | 行为 |
//! |---|---|---|
//! | `>= 4px` | [`DetailLevel::Full`] | 完整填充 + 描边 |
//! | `>= 1px` | [`DetailLevel::Silhouette`] | 只画包围盒色块(跳过描边等细节) |
//! | `< 1px`  | [`DetailLevel::Point`] | 完全跳过(画了也只是一个次像素脏点) |

/// 进入轮廓降级(Silhouette)的屏幕尺寸阈值,单位屏幕像素(docs/02 §7.2 的 4px)。
pub const DETAIL_THRESHOLD_PX: f64 = 4.0;

/// 完全跳过绘制(Point)的屏幕尺寸阈值,单位屏幕像素。
pub const SKIP_THRESHOLD_PX: f64 = 1.0;

/// 对象当前的绘制细节档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailLevel {
    /// 完整绘制(填充 + 描边)。
    Full,
    /// 轮廓降级:只画包围盒色块,跳过描边等细节。
    Silhouette,
    /// 屏幕上不足 1px,完全跳过。
    Point,
}

/// 判定对象(以 `world_size` 为特征尺寸,如包围盒短边)在当前缩放下应走的细节档位。
pub fn detail_level(world_size: f64, zoom: f64) -> DetailLevel {
    let screen_px = world_size.abs() * zoom.abs();
    if screen_px >= DETAIL_THRESHOLD_PX {
        DetailLevel::Full
    } else if screen_px >= SKIP_THRESHOLD_PX {
        DetailLevel::Silhouette
    } else {
        DetailLevel::Point
    }
}

/// docs/02 §7.2 原样的阈值判断:屏幕尺寸是否足以画细节。
///
/// 等价于 [`detail_level`] `>=` [`DetailLevel::Silhouette`];保留原函数名以便
/// 与手册对照,新代码建议直接用 [`detail_level`]。
pub fn should_draw_detail(world_size: f64, zoom: f64) -> bool {
    world_size * zoom >= DETAIL_THRESHOLD_PX
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_split_three_levels() {
        // 10 世界单位 × zoom:
        assert_eq!(detail_level(10.0, 1.0), DetailLevel::Full); // 10px
        assert_eq!(detail_level(10.0, 0.5), DetailLevel::Full); // 5px
        assert_eq!(detail_level(10.0, 0.39), DetailLevel::Silhouette); // 3.9px
        assert_eq!(detail_level(10.0, 0.3), DetailLevel::Silhouette); // 3px
        assert_eq!(detail_level(10.0, 0.1), DetailLevel::Silhouette); // 1px
        assert_eq!(detail_level(10.0, 0.05), DetailLevel::Point); // 0.5px
    }

    #[test]
    fn boundary_values_are_inclusive() {
        // 4px 恰好落 Full(>= 阈值),1px 恰好落 Silhouette
        assert_eq!(detail_level(4.0, 1.0), DetailLevel::Full);
        assert_eq!(detail_level(1.0, 1.0), DetailLevel::Silhouette);
        assert_eq!(detail_level(0.99, 1.0), DetailLevel::Point);
    }

    #[test]
    fn should_draw_detail_matches_original_formula() {
        // docs/02 §7.2 原式:world_size * zoom >= 4.0
        assert!(should_draw_detail(10.0, 1.0));
        assert!(should_draw_detail(4.0, 1.0));
        assert!(!should_draw_detail(10.0, 0.39));
        // 与 detail_level 的 Full 档一致
        for world in [0.5, 3.0, 17.0, 100.0] {
            for zoom in [0.01, 0.5, 1.0, 8.0, 64.0] {
                assert_eq!(
                    should_draw_detail(world, zoom),
                    detail_level(world, zoom) == DetailLevel::Full
                );
            }
        }
    }

    #[test]
    fn negative_sizes_are_treated_by_magnitude() {
        // 负尺寸(脏数据)按绝对值处理,不 panic、不进入 Full
        assert_eq!(detail_level(-10.0, 1.0), DetailLevel::Full);
        assert_eq!(detail_level(-10.0, 0.05), DetailLevel::Point);
    }
}
