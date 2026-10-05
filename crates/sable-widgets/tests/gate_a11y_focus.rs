//! TC-A11Y-FOCUS-01:焦点体系静态门禁(迭代审查报告 2026-10-04 §4.3 A11Y-01 /
//! §5.10.1,G-UI-D 的组件侧扫描刀)。
//!
//! # 病根(门禁要防的事)
//!
//! 报告实锤:焦点此前仅 NumberField 接入,LayersPanel/LayerTree/EffectStack/
//! Timeline 不可 Tab 进入、无可视焦点环(R4 复盘"✅ V3.0"系虚标)。本批
//! A11Y-01 落地后,全部**可交互 Entity 组件** `track_focus` + `tab_stop(true)`
//! 入 Tab 序,焦点环统一 [`interact::focus_ring`](单点,两主题 accent)。
//!
//! # 扫描面(显式化,GATE-03 纪律③)
//!
//! - 递归收集 `crates/sable-widgets/src/**/*.rs`,剥行注释与 `#[cfg(test)]`
//!   区(与 gate_no_dup_button 同款 production_view),只在生产段作数;
//! - 规则 1(实现面):文件生产段出现 `impl Render for` / `impl RenderOnce
//!   for`(即组件类型)→ 该文件生产段必须含 `track_focus(`(Entity 焦点
//!   接入),或在 [`EXEMPT`] 白名单(容器/装饰件,理由显式);
//! - 规则 2(焦点环单点):生产段出现 `focus_ring_layers(` 即红——焦点环
//!   实现已提升为 `interact::focus_ring`,禁止两份(旧 choice.rs 私有件)。
//!
//! # 反面测试(门禁要有反面用例,GATE-03 纪律②)
//!
//! - 注入含 `impl Render for` 而无 `track_focus(` 的合成生产文件 → 红;
//! - 同内容进白名单 → 放行;含 `focus_ring_layers(` → 红。

use std::fs;
use std::path::{Path, PathBuf};

/// 容器/装饰件白名单(路径含此子串即豁免规则 1;理由显式,防"豁免即遗忘"):
/// - property_row / inspector:RenderOnce 纯容器,行不可点,交互语义由
///   子控件承担(NumberField 已入 Tab 序);
/// - effect_stack:RenderOnce 行列表,行焦点需跨帧句柄(宿主壳接线 =
///   M2 契约,见 effect_stack 模块 doc);行内按钮为 Compact 内联形态,
///   焦点体系随宿主壳;
/// - tooltip:程序化浮层宿主,非交互件(不进 Tab 序);
/// - neon_card / curve_editor:装饰/只读预览件(无操作语义)。
const EXEMPT: &[&str] = &[
    "property_row.rs",
    "inspector.rs",
    "effect_stack.rs",
    "tooltip.rs",
    "neon_card.rs",
    "curve_editor.rs",
];

/// 仓库根(…/crates/sable-widgets → 上溯两级)。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CARGO_MANIFEST_DIR 应有两级父目录(仓库根)")
        .to_path_buf()
}

/// 递归收集 dir 下全部 .rs 文件(排序,保证输出确定)。
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

/// 扫描面:sable-widgets src/**/*.rs(显式化,见模块文档)。
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

/// 行内花括号净深度(字符串感知;gate_no_dup_button 同款)。
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

/// 剥掉一行内的 `//` 行注释(含 `///`/`//!`)。
fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    }
}

/// 生产段视图(剥整行注释与 `#[cfg(test)] mod` 区;gate_no_dup_button 同款)。
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

/// 门禁裁决(纯函数):返回违例清单,空 = 通过。
fn gate_violations(files: &[(String, String)]) -> Vec<String> {
    let mut bad = Vec::new();
    for (rel, content) in files {
        let production = production_view(content);
        let is_component =
            production.contains("impl Render for") || production.contains("impl RenderOnce for");
        let exempt = EXEMPT.iter().any(|name| rel.contains(name));
        if is_component && !exempt && !production.contains("track_focus(") {
            bad.push(format!(
                "{rel}:组件类型(impl Render/RenderOnce)未接入 track_focus——\
                 可交互 Entity 组件必须 track_focus + tab_stop(true) 入 Tab 序;\
                 容器/装饰件请进 EXEMPT 白名单并写明理由(A11Y-01/TC-A11Y-FOCUS-01)"
            ));
        }
        if production.contains("focus_ring_layers(") {
            bad.push(format!(
                "{rel}:生产段出现旧焦点环实现 `focus_ring_layers(`——焦点环已\
                 提升为 interact::focus_ring 单点(A11Y-01),禁止两份"
            ));
        }
    }
    bad
}

#[test]
fn no_focus_ring_duplicate_and_all_components_track_focus() {
    let root = repo_root();
    let files = scan_face_files(&root);
    assert!(
        files.len() >= 20,
        "扫描面异常:仅找到 {} 个 .rs,目录收集逻辑疑似失效",
        files.len()
    );
    let bad = gate_violations(&files);
    assert!(
        bad.is_empty(),
        "A11Y-01 焦点门禁违例:\n  - {}",
        bad.join("\n  - ")
    );
}

/// 正主的自我保护:焦点环单点仍存在且被多个组件消费。
#[test]
fn focus_ring_single_source_still_consumed() {
    let root = repo_root();
    let interact = fs::read_to_string(root.join("crates/sable-widgets/src/interact.rs"))
        .expect("读取 interact.rs");
    assert!(
        interact.contains("pub fn focus_ring("),
        "interact::focus_ring 单点丢失(焦点环收口失效)"
    );
    for consumer in [
        "controls/choice.rs",
        "controls/tabs.rs",
        "controls/select.rs",
    ] {
        let text = fs::read_to_string(root.join("crates/sable-widgets/src").join(consumer))
            .unwrap_or_else(|e| panic!("读取 {consumer} 失败:{e}"));
        assert!(
            text.contains("interact::focus_ring("),
            "{consumer} 不再消费 interact::focus_ring(焦点环单点被绕开)"
        );
    }
}

// ---------------------------------------------------------------------------
// 反面测试(注入缺陷必须红灯;纯函数裁决,不写文件)
// ---------------------------------------------------------------------------

#[test]
fn tc_a11y_focus_01_component_without_track_focus_is_red() {
    let files = vec![(
        "crates/sable-widgets/src/future_widget.rs".to_string(),
        "pub struct W;\nimpl Render for W {\n    fn render(&mut self) -> Div {\n        div()\n    }\n}\n"
            .to_string(),
    )];
    let bad = gate_violations(&files);
    assert_eq!(bad.len(), 1, "无 track_focus 的组件必须报红:{bad:?}");
    assert!(bad[0].contains("future_widget.rs"));
}

#[test]
fn tc_a11y_focus_01_exempt_container_passes() {
    let files = vec![(
        "crates/sable-widgets/src/property_row.rs".to_string(),
        "pub struct Row;\nimpl RenderOnce for Row {\n    fn render(self) -> Div {\n        div()\n    }\n}\n"
            .to_string(),
    )];
    assert!(
        gate_violations(&files).is_empty(),
        "白名单容器(纯 RenderOnce 容器)必须放行"
    );
}

#[test]
fn tc_a11y_focus_01_duplicate_focus_ring_impl_is_red() {
    let files = vec![(
        "crates/sable-widgets/src/controls/choice.rs".to_string(),
        "fn focus_ring_layers(accent: Hsla) -> [AnyElement; 2] {\n    [div().into_any_element()]\n}\n"
            .to_string(),
    )];
    let bad = gate_violations(&files);
    assert_eq!(bad.len(), 1, "旧焦点环实现复发必须报红:{bad:?}");
    assert!(bad[0].contains("focus_ring_layers"));
}
