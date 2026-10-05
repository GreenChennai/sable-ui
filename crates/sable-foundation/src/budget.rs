//! RBT-13:分域内存预算器(§6 RB-05:循环/缓存有容量与时长上限的
//! 全局账本)。各缓存(阴影/缩略图/解码)注册域预算,超限按插入序
//! (FIFO)驱逐并计数——与 ShadowCache 的容量上限互补:容量管"条数",
//! 预算管"字节"。

use std::collections::HashMap;
use std::collections::VecDeque;

/// 单域预算:FIFO 驱逐(条目字节量在插入时登记)。
struct Domain {
    budget_bytes: u64,
    used_bytes: u64,
    order: VecDeque<u64>,
    /// 每键字节量(覆盖插入时先扣旧值)。
    sizes: HashMap<u64, u64>,
    evicted: u64,
}

/// 分域内存预算。
#[derive(Default)]
pub struct MemoryBudget {
    domains: HashMap<&'static str, Domain>,
}

impl MemoryBudget {
    /// 注册/重置一个域(重复注册 = 重置该域)。
    pub fn register(&mut self, domain: &'static str, budget_bytes: u64) {
        self.domains.insert(
            domain,
            Domain {
                budget_bytes,
                used_bytes: 0,
                order: VecDeque::new(),
                sizes: HashMap::new(),
                evicted: 0,
            },
        );
    }

    /// 登记条目占用(插入或更新);超预算按 FIFO 驱逐本域旧键直至达标。
    /// 返回被驱逐键列表(宿主可据此清自己的映射)。
    pub fn charge(&mut self, domain: &'static str, key: u64, bytes: u64) -> Vec<u64> {
        let Some(d) = self.domains.get_mut(domain) else {
            return Vec::new();
        };
        if let Some(old) = d.sizes.get(&key) {
            d.used_bytes -= *old;
            d.order.retain(|k| *k != key);
        }
        d.used_bytes += bytes;
        d.sizes.insert(key, bytes);
        d.order.push_back(key);
        let mut evicted = Vec::new();
        while d.used_bytes > d.budget_bytes {
            let Some(oldest) = d.order.pop_front() else {
                break;
            };
            if let Some(sz) = d.sizes.remove(&oldest) {
                d.used_bytes = d.used_bytes.saturating_sub(sz);
            }
            d.evicted += 1;
            evicted.push(oldest);
        }
        evicted
    }

    /// 域当前占用字节。
    pub fn used(&self, domain: &str) -> u64 {
        self.domains.get(domain).map_or(0, |d| d.used_bytes)
    }

    /// 域累计驱逐数。
    pub fn evicted(&self, domain: &str) -> u64 {
        self.domains.get(domain).map_or(0, |d| d.evicted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_rbt_mem_01_domain_budget_fifo_evicts_under_pressure() {
        let mut budget = MemoryBudget::default();
        budget.register("thumb", 1000);
        // 三笔 400B:第三笔触发驱逐(最旧先出),占用回到预算内
        assert!(budget.charge("thumb", 1, 400).is_empty());
        assert!(budget.charge("thumb", 2, 400).is_empty());
        let evicted = budget.charge("thumb", 3, 400);
        assert_eq!(evicted, vec![1], "FIFO 驱逐最旧");
        assert_eq!(budget.used("thumb"), 800);
        assert_eq!(budget.evicted("thumb"), 1);
        // 覆盖同键:先扣旧值
        budget.charge("thumb", 2, 100);
        assert_eq!(budget.used("thumb"), 500);
        // 未注册域:无操作不崩
        assert!(budget.charge("nope", 1, 10).is_empty());
        assert_eq!(budget.used("nope"), 0);
    }
}
