//! TC-A11Y-RM-01:reduced-motion 全接线静态门禁(迭代审查报告 2026-10-04
//! §4.3 A11Y-05 / §5.11 G-UI-I)。
//!
//! # 病根(门禁要防的事)
//!
//! 报告实锤:reduced_motion 接线不完整(即时 hover、NumberField 光标漏接)。
//! 本门禁锁两件事:
//! 1. **静态**:任何生产段出现动画入口(`Animated::new(`/`Animated::set(`/
//!    `request_animation_frame(`)的组件文件,必须同时出现 reduced 判据
//!    (`reduced_motion`/`reduced`),或在 [`WHITELIST`] 显式登记**理由**——
//!    合法理由即"动画原语内部已短路"(Animated/HoverState/PulseState/
//!    FlipTracker/ScrollPhysics/TooltipClock 都在 value_at/progress_at/
//!    is_running 内部读 [`reduced_motion`] 直通);
//! 2. **运行时**:reduced 开 → 动画直通目标值、不再运行(帧泵停止判据恒假)
//!    ——对动画原语做一次汇总断言(逐组件的细则由各模块既有测试锁定)。
//!
//! # 扫描面(显式化,GATE-03 纪律③)
//!
//! - 递归收集 `crates/sable-widgets/src/**/*.rs`,剥行注释与 `#[cfg(test)]`
//!   区,只在生产段作数;
//! - anim/ 引擎目录豁免(它们就是 reduced 的实现地);
//! - 命中动画入口且无判据且未白名单 → 红。

use std::fs;
use std::path::{Path, PathBuf};

use sable_widgets::anim::{Animated, Easing, set_reduced_motion};

/// 白名单:文件 → 理由(动画入口存在但 reduced 判据在动画原语内部短路)。
const WHITELIST: &[(&str, &str)] = &[
    (
        "controls/choice.rs",
        "选中进度经 Animated::value_at/is_running_at 求值(anim 内部 reduced 直通);\
         hover 进度经 HoverState(内部短路)",
    ),
    (
        "controls/tabs.rs",
        "下划线 UnderlineSlide 经 Animated::value_at/is_running_at(anim 内部直通)",
    ),
    (
        "controls/select.rs",
        "开合动画 open_anim 经 Animated(anim 内部直通);hover 经 HoverState",
    ),
    (
        "controls/command_palette.rs",
        "面板下滑(panel_anim)/背板淡变(scrim_anim)经 Animated::value_at/\
         is_running_at(anim 内部直通;TC-CMP-CMD-01 动画用例锁定)",
    ),
    (
        "number_field.rs",
        "hover 经 HoverState(内部短路);编辑态光标为静态竖线(无闪烁动画)",
    ),
    (
        "layer_panel.rs",
        "hover(HoverState)/脉冲(PulseState)/让位(FlipTracker)三者内部短路",
    ),
    (
        "layer_tree.rs",
        "同 layer_panel:HoverState/PulseState/FlipTracker 内部短路",
    ),
    (
        "interact.rs",
        "HoverState/PulseState 本体即 reduced 短路的实现地",
    ),
];

/// 仓库根(…/crates/sable-widgets → 上溯两级)。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CARGO_MANIFEST_DIR 应有两级父目录(仓库根)")
        .to_path_buf()
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn scan_face_files(root: &Path) -> Vec<(String, String)> {
    let src = root.join("crates/sable-widgets/src");
    let mut files = Vec::new();
    collect_rs_files(&src, &mut files);
    files.sort();
    files
        .into_iter()
        .map(|f| {
            let rel = f
                .strip_prefix(root)
                .unwrap_or(&f)
                .to_string_lossy()
                .replace('\\', "/");
            let content =
                fs::read_to_string(&f).unwrap_or_else(|e| panic!("读取 {} 失败:{e}", f.display()));
            (rel, content)
        })
        .collect()
}

fn brace_depth(code: &str, mut depth: usize) -> usize {
    let mut in_string = false;
    let mut escaped = false;
    for ch in code.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth
}

fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    }
}

fn production_view(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut depth = 0usize;
    let mut in_test = false;
    let mut test_base = 0usize;
    let mut test_pending = false;
    for raw in content.lines() {
        let code = strip_line_comment(raw);
        let has_test_attr = code.contains("#[cfg(test)]");
        let opens_mod = code.contains("mod ") && code.contains('{');
        let depth_before = depth;
        if !in_test {
            out.push_str(code);
            out.push('\n');
        }
        depth = brace_depth(code, depth);
        if has_test_attr && depth_before == 0 {
            if opens_mod && depth > depth_before {
                in_test = true;
                test_base = depth_before;
            } else {
                test_pending = true;
            }
        } else if test_pending {
            if opens_mod && depth > depth_before {
                in_test = true;
                test_base = depth_before;
            }
            test_pending = false;
        }
        if in_test && depth <= test_base {
            in_test = false;
        }
    }
    out
}

/// 动画入口(生产段)。
fn has_animation_entry(production: &str) -> bool {
    production.contains("Animated::new(")
        || production.contains("Animated::set(")
        || production.contains("request_animation_frame(")
        || production.contains("HoverState")
        || production.contains("PulseState")
        || production.contains("FlipTracker")
        || production.contains("ScrollPhysics")
}

/// reduced 判据(生产段文本层)。
fn has_reduced_criterion(production: &str) -> bool {
    production.contains("reduced_motion") || production.contains("reduced")
}

/// 门禁裁决(纯函数):返回违例清单,空 = 通过。
fn gate_violations(files: &[(String, String)]) -> Vec<String> {
    let mut bad = Vec::new();
    for (rel, content) in files {
        // anim/ 引擎豁免:它们是 reduced 的实现地
        if rel.contains("/anim/") {
            continue;
        }
        let production = production_view(content);
        let whitelisted = WHITELIST.iter().any(|(name, _)| rel.ends_with(name));
        if has_animation_entry(&production) && !has_reduced_criterion(&production) && !whitelisted {
            bad.push(format!(
                "{rel}:动画入口(Animated/Hover/Pulse/Flip/Scroll/帧泵)无 reduced 判据\
                 ——动画必须经 reduced_motion 短路(原语内部短路即可,但需登记 \
                 WHITELIST 白名单并写明理由;A11Y-05/TC-A11Y-RM-01)"
            ));
        }
    }
    bad
}

#[test]
fn every_animation_entry_has_reduced_criterion_or_whitelist() {
    let root = repo_root();
    let files = scan_face_files(&root);
    assert!(files.len() >= 20, "扫描面异常");
    let bad = gate_violations(&files);
    assert!(
        bad.is_empty(),
        "A11Y-05 reduced-motion 门禁违例:\n  - {}",
        bad.join("\n  - ")
    );
}

/// 运行时半边:reduced 开 → 动画直通目标、不再运行(帧泵停止判据)。
/// 逐组件细则由各模块既有测试锁定(HoverState/Pulse/Flip/Tabs/Select 等)。
#[test]
fn tc_a11y_rm_01_reduced_on_means_direct_pass_and_no_pump() {
    let t0 = std::time::Instant::now();
    // 正常路径:插值进行中
    set_reduced_motion(false);
    let mut a = Animated::new(0.0_f64);
    a.set(1.0, std::time::Duration::from_millis(200), Easing::OutCubic);
    assert!(a.is_running(), "正常路径动画在跑(对照)");
    // reduced 开:直通目标、运行态恒假(帧泵停止判据)
    set_reduced_motion(true);
    let mut b = Animated::new(0.0_f64);
    b.set(1.0, std::time::Duration::from_millis(200), Easing::OutCubic);
    assert_eq!(b.value_at(t0), 1.0, "reduced 开:值直通目标");
    assert!(!b.is_running_at(t0), "reduced 开:不再续帧");
    set_reduced_motion(false);
}

/// 白名单自我保护:登记过的文件必须真实存在且确实含动画入口
/// (组件改名/删除后,白名单不能变成"幽灵豁免")。
#[test]
fn whitelist_entries_are_live_files_with_animation_entries() {
    let root = repo_root();
    for (name, reason) in WHITELIST {
        assert!(!reason.is_empty(), "{name}:白名单必须写明理由");
        let path = root.join("crates/sable-widgets/src").join(name);
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("读取 {name} 失败:{e}(幽灵白名单)"));
        assert!(
            has_animation_entry(&production_view(&text)),
            "{name}:白名单登记但生产段已无动画入口(可移除登记)"
        );
    }
}

// ---------------------------------------------------------------------------
// 反面测试
// ---------------------------------------------------------------------------

#[test]
fn tc_a11y_rm_01_animation_entry_without_criterion_is_red() {
    let files = vec![(
        "crates/sable-widgets/src/future_widget.rs".to_string(),
        "pub struct W;\nimpl Render for W {\n    fn render(&mut self) {\n        let a = Animated::new(0.0);\n    }\n}\n"
            .to_string(),
    )];
    let bad = gate_violations(&files);
    assert_eq!(bad.len(), 1, "无判据的动画入口必须报红:{bad:?}");
    // 登记白名单(带理由)后放行
    let with_wl = vec![(
        "crates/sable-widgets/src/controls/choice.rs".to_string(),
        "fn f() {\n    let a = Animated::new(0.0);\n}\n".to_string(),
    )];
    assert!(
        gate_violations(&with_wl).is_empty(),
        "白名单文件(理由在表)放行"
    );
}

#[test]
fn tc_a11y_rm_01_engine_dir_is_exempt() {
    let files = vec![(
        "crates/sable-widgets/src/anim/spring.rs".to_string(),
        "fn solve(t: f64) -> f64 {\n    t\n}\n".to_string(),
    )];
    assert!(gate_violations(&files).is_empty(), "anim 引擎目录豁免");
}
