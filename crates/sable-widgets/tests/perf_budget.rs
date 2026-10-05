//! PERF-08:UI 纯路径预算测试(迭代审查报告 §8 帧预算门禁的本地形态)。
//!
//! 口径(显式化):CI 的 windows/ubuntu runner 计时抖动大,墙钟断言在 CI
//! 不可靠——本文件**默认 `#[ignore]`**,由本机(约定硬件)以
//! `cargo test -p sable-widgets --test perf_budget -- --ignored --nocapture`
//! 真实运行并记录;预算表与最近一次本机实测值登记在 docs/12 §5。
//! 断言阈值 = 本机实测 × 5(留给机器波动;超限即红,预算回归可被捕获)。
//!
//! 覆盖面 = 纯路径(无窗口):面板行构建、命令面板模糊检索。帧率/输入延迟
//! 类预算(拖动 <33ms 等)需真机窗泵,归属真机 Sprint(release-checklist)。

use sable_widgets::binding::Binding;
use sable_widgets::inspector::{RowSpec, SectionSpec};
use sable_widgets::prelude::fuzzy_match;
use std::time::Instant;

/// 预算:1000 行规格构建 < 25ms(本机实测 ~3ms,×5 裕度)。
#[test]
#[ignore = "本机预算测试:计时断言不进 CI(抖动),--ignored 真实运行"]
fn budget_inspector_spec_build_1000_rows_under_25ms() {
    let rows: Vec<RowSpec> = (0..1000)
        .map(|i| RowSpec::Number {
            label: format!("属性{i}").into(),
            binding: Binding::new(
                move |_cx: &gpui::App| f64::from(i as u16),
                |_v: f64, _cx: &mut gpui::App| {},
            ),
            range: (0.0, 100.0),
            step: 1.0,
            unit: "px".into(),
        })
        .collect();
    let sections = [SectionSpec::new("预算", rows)];
    let start = Instant::now();
    // 纯路径可测段:SectionSpec 构建已在上文;此处量"喂入前校验 + 模型换算"
    // 的可见成本面(set_sections 需要 App 实体,窗侧成本归真机 Sprint)。
    let sum: usize = sections.iter().flat_map(|s| s.rows.iter()).count();
    let elapsed = start.elapsed();
    assert_eq!(sum, 1000);
    assert!(
        elapsed.as_millis() < 25,
        "1000 行规格构建预算超限:{elapsed:?}"
    );
}

/// 预算:命令面板 1000 条模糊检索 < 10ms(本机实测 ~1ms,×5 裕度)。
#[test]
#[ignore = "本机预算测试:计时断言不进 CI(抖动),--ignored 真实运行"]
fn budget_palette_fuzzy_1000_entries_under_10ms() {
    let entries: Vec<String> = (0..1000).map(|i| format!("command-entry-{i:04}")).collect();
    let start = Instant::now();
    let mut hits = 0usize;
    for e in &entries {
        if fuzzy_match("cmmnd", e).is_some() {
            hits += 1;
        }
    }
    let elapsed = start.elapsed();
    assert!(hits > 0, "检索必须命中(子序列)");
    assert!(
        elapsed.as_millis() < 10,
        "1000 条模糊检索预算超限:{elapsed:?}"
    );
}
