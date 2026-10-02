//! 脏矩形:增量重绘的记账本(源自 docs/02 §7.1,代码照抄 + `Default`/`is_dirty`)。
//!
//! GPUI 层只在 `cx.notify()` 标记的区域重绘;画布内部用本结构聚合"本帧哪些
//! 世界区域变了"。本帧无脏区 → 直接复用上一帧纹理,GPU 零工作。

use kurbo::Rect;

/// 世界坐标脏区追踪:mark 收集,take 合并成一个包围盒。
#[derive(Debug, Clone, Default)]
pub struct DamageTracker {
    /// 世界坐标脏区
    dirty_world: Vec<Rect>,
}

impl DamageTracker {
    /// 标记一个世界坐标脏区(通常是节点改动前后的两个包围盒都 mark)。
    pub fn mark(&mut self, node_world_bbox: Rect) {
        self.dirty_world.push(node_world_bbox);
    }

    /// 取走本帧全部脏区,合并为一个包围盒;为空则返回 `None`(本帧不渲染)。
    pub fn take(&mut self) -> Option<Rect> {
        let mut merged: Option<Rect> = None;
        for rect in self.dirty_world.drain(..) {
            merged = Some(match merged {
                Some(acc) => acc.union(rect),
                None => rect,
            });
        }
        merged
    }

    /// 是否有待处理的脏区(不清空)。
    pub fn is_dirty(&self) -> bool {
        !self.dirty_world.is_empty()
    }

    /// 丢弃全部脏区(如视口整体重建后由全量重绘接管)。
    pub fn clear(&mut self) {
        self.dirty_world.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_merges_into_single_bbox_and_clears() {
        let mut tracker = DamageTracker::default();
        assert!(!tracker.is_dirty());
        assert_eq!(tracker.take(), None, "无脏区时 take 返回 None");

        tracker.mark(Rect::new(0.0, 0.0, 10.0, 10.0));
        tracker.mark(Rect::new(20.0, 5.0, 30.0, 40.0));
        assert!(tracker.is_dirty());

        let merged = tracker.take().expect("有脏区");
        assert_eq!(merged, Rect::new(0.0, 0.0, 30.0, 40.0), "合并为总包围盒");
        assert!(!tracker.is_dirty(), "take 之后清空");
        assert_eq!(tracker.take(), None, "再次 take 返回 None");
    }

    #[test]
    fn single_rect_passes_through_unchanged() {
        let mut tracker = DamageTracker::default();
        let rect = Rect::new(-5.0, -7.0, 3.0, 12.0);
        tracker.mark(rect);
        assert_eq!(tracker.take(), Some(rect));
    }

    #[test]
    fn clear_drops_pending_damage() {
        let mut tracker = DamageTracker::default();
        tracker.mark(Rect::new(0.0, 0.0, 1.0, 1.0));
        tracker.clear();
        assert!(!tracker.is_dirty());
        assert_eq!(tracker.take(), None);
    }
}
