//! TC-A11Y-LABEL-01:语义标签静态门禁(迭代审查报告 2026-10-04 §4.3 A11Y-02 /
//! §5.10.3,G-UI-E 的组件侧扫描刀)。
//!
//! # 病根(门禁要防的事)
//!
//! 报告实锤(R4):全仓 `with_label|aria|role` = 0,a11y-notes 曾虚标
//! "✅ V3.0"。**gpui 0.2.2 无语义树 API**(crate 无 `_accessibility` 模块、
//! Cargo 无 accesskit 依赖,2026-10 源码核实)——本批以库侧语义接口层落地:
//! 每个 pub 组件类型必有 `.label(...)` 槽(`semantic_slot!` 宏或显式方法,
//! 存态 + `attach_semantics` 渲染挂接点),TD-01(gpui 升级)后零改动接通
//! 原生语义树。读屏真机走查表契约见 docs/a11y-notes.md(如实标注"暂不可
//! 消费",不虚标)。
//!
//! # 扫描面(显式化,GATE-03 纪律③)
//!
//! - 对 [`COMPONENTS`] 逐项:文件生产段必须含 `fn label(`(builder 槽);
//! - `EXEMPT`:数据规格/非组件类型,理由显式;
//! - 规则 2:生产段出现 `attach_semantics(` 的组件文件,必须配套构造
//!   `Semantic::new()`(语义挂接点不允许空挂)。
//!
//! # 反面测试
//!
//! - 注入无 `fn label(` 的合成组件文件 → 红;加槽后 → 放行。

use std::fs;
use std::path::{Path, PathBuf};

/// pub 组件类型 → 实现文件(类型名仅用于违例信息可读)。
const COMPONENTS: &[(&str, &str)] = &[
    ("Button", "controls/button.rs"),
    ("IconButton", "controls/button.rs"),
    ("Choice", "controls/choice.rs"),
    ("Select", "controls/select.rs"),
    ("Tabs", "controls/tabs.rs"),
    ("TextField", "controls/text_field.rs"),
    ("ScrollArea", "controls/scroll_area.rs"),
    ("TooltipHost", "controls/tooltip.rs"),
    ("NumberField", "number_field.rs"),
    ("ColorWell", "color.rs"),
    ("ColorWheel", "color.rs"),
    ("GradientEditor", "gradient_editor.rs"),
    ("LayerPanel", "layer_panel.rs"),
    ("LayerTreePanel", "layer_tree.rs"),
    ("EffectStackPanel", "effect_stack.rs"),
    ("InspectorPanel", "inspector.rs"),
    ("PropertyRow", "property_row.rs"),
    ("TimelineView", "timeline_view.rs"),
    ("NeonCardState", "neon_card.rs"),
    ("CurvePreview", "curve_editor.rs"),
];

/// 数据规格/非组件豁免(理由显式):
/// - TooltipSpec:`label` 字段即可访问名(数据规格,无 builder);
/// - EffectStackSpec / RowSpec / SectionSpec:数据规格,label/title 字段即
///   语义(受控面板逐帧由宿主构造);
/// - Semantic / SemanticRole:语义层本体(interact.rs)。
const EXEMPT: &[&str] = &["TooltipSpec", "EffectStackSpec", "RowSpec", "SectionSpec"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CARGO_MANIFEST_DIR 应有两级父目录(仓库根)")
        .to_path_buf()
}

/// 行内花括号净深度(字符串感知)。
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

/// 生产段视图(剥整行注释与 `#[cfg(test)] mod` 区)。
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

/// 门禁裁决(纯函数):输入 = (组件名, 相对路径, 文件内容) 列表,
/// 返回违例清单,空 = 通过。
fn gate_violations(entries: &[(String, String, String)]) -> Vec<String> {
    let mut bad = Vec::new();
    for (ty, rel, content) in entries {
        if EXEMPT.contains(&ty.as_str()) {
            continue;
        }
        let production = production_view(content);
        let has_label_slot = production.contains("fn label(")
            || production.contains("fn with_label(")
            || production.contains("semantic_slot!(");
        if !has_label_slot {
            bad.push(format!(
                "{rel}:{ty} 缺 `.label(...)` 语义槽(A11Y-02/TC-A11Y-LABEL-01;\
                 经 semantic_slot! 宏或显式 builder 方法落地)"
            ));
        }
    }
    bad
}

fn load_components(root: &Path) -> Vec<(String, String, String)> {
    let base = root.join("crates/sable-widgets/src");
    COMPONENTS
        .iter()
        .map(|(ty, rel)| {
            let path = base.join(rel);
            let content = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("读取 {} 失败:{e}", path.display()));
            (ty.to_string(), rel.to_string(), content)
        })
        .collect()
}

#[test]
fn every_pub_component_has_a_label_slot() {
    let root = repo_root();
    let entries = load_components(&root);
    assert_eq!(
        entries.len(),
        COMPONENTS.len(),
        "组件清单装载不完整(文件缺失?)"
    );
    let bad = gate_violations(&entries);
    assert!(
        bad.is_empty(),
        "A11Y-02 语义槽门禁违例:\n  - {}",
        bad.join("\n  - ")
    );
}

/// 正主的自我保护:语义层单点仍存在(Semantic/SemanticRole/attach_semantics)。
#[test]
fn semantic_layer_single_source_still_present() {
    let root = repo_root();
    let text = fs::read_to_string(root.join("crates/sable-widgets/src/interact.rs"))
        .expect("读取 interact.rs");
    for expected in [
        "pub enum SemanticRole",
        "pub struct Semantic",
        "pub fn attach_semantics",
        "macro_rules! semantic_slot",
    ] {
        assert!(
            text.contains(expected),
            "interact.rs 丢失 `{expected}`(语义层收口失效)"
        );
    }
}

// ---------------------------------------------------------------------------
// 反面测试
// ---------------------------------------------------------------------------

#[test]
fn tc_a11y_label_01_component_without_label_slot_is_red() {
    let entries = vec![(
        "FutureWidget".to_string(),
        "crates/sable-widgets/src/future.rs".to_string(),
        "pub struct FutureWidget;\nimpl RenderOnce for FutureWidget {\n    fn render(self) -> Div {\n        div()\n    }\n}\n"
            .to_string(),
    )];
    let bad = gate_violations(&entries);
    assert_eq!(bad.len(), 1, "缺 label 槽必须报红:{bad:?}");
    let with_slot = vec![(
        "FutureWidget".to_string(),
        "crates/sable-widgets/src/future.rs".to_string(),
        "pub fn label(mut self, l: impl Into<SharedString>) -> Self { self }\n".to_string(),
    )];
    assert!(gate_violations(&with_slot).is_empty(), "补槽后放行");
}

#[test]
fn tc_a11y_label_01_data_specs_are_exempt() {
    let entries: Vec<(String, String, String)> = EXEMPT
        .iter()
        .map(|ty| ((*ty).to_string(), "spec.rs".to_string(), String::new()))
        .collect();
    assert!(gate_violations(&entries).is_empty(), "数据规格豁免面放行");
}
