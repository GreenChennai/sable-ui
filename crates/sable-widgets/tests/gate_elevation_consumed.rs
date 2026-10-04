//! TOK-01:ELEVATIONS 海拔令牌**消费门禁**(迭代审查报告 2026-10-04 §4.1
//! TOK-01 当场修复方案 2;TC-TOK-ELEV-01 / TC-GATE-ELEV-01)。
//!
//! # 病根(门禁要防的事)
//!
//! v1.0 引入 `tokens::ELEVATIONS[5]` 后长期零组件消费("定义即废弃"),
//! 深色主题下所有面板/浮层糊成一片。本轮接线两个真实消费点:
//!
//! - `number_field.rs`:输入凸起 = L2(`theme::elevated` 的 quad 近似阴影);
//! - `neon_card.rs`:环境辉光底 = L3(离屏 `render_shadow_rgba`,参数
//!   `ELEVATIONS[3]` 派生)。
//!
//! # 扫描面(显式化,GATE-03 纪律③)
//!
//! - 递归收集 `<仓库根>/crates/*/src/**/*.rs`(与 gate_no_hardcoded_color
//!   同扫描面;不含 examples/tests/docs),按文件统计出现任一**消费标记**
//!   的文件数(排除定义侧 `tokens.rs`/`theme.rs`):
//!   `ELEVATIONS` / `theme::shadow` / `theme::elevated` / `shadow_quads`;
//! - 消费文件数 < 2 → 门禁红(至少两档真实消费的报告要求);
//! - 定义侧自检:`tokens.rs` 必须仍定义 `pub const ELEVATIONS`、`theme.rs`
//!   必须仍定义 `pub fn shadow`(改名/删除后门禁不自转)。
//!
//! # 反面测试
//!
//! TC-GATE-ELEV-01:用内联合成文件面模拟"移除全部/仅剩一个消费点" →
//! 门禁裁决函数必须报红(纯函数,不写文件)。

use std::fs;
use std::path::{Path, PathBuf};

use sable_widgets::theme::shadow;
use sable_widgets::tokens::shadow_layer_params;

/// 消费标记:命中即认定该文件消费海拔令牌。
const CONSUMPTION_MARKERS: [&str; 4] = [
    "ELEVATIONS",
    "theme::shadow",
    "theme::elevated",
    "shadow_quads",
];

/// 定义侧文件(排除在消费统计之外,且其存在性受自检)。
const DEFINITION_FILES: [&str; 2] = [
    "crates/sable-widgets/src/tokens.rs",
    "crates/sable-widgets/src/theme.rs",
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

/// 扫描面:crates/*/src/**/*.rs(显式化,见模块文档)。
fn scan_face_files(root: &Path) -> Vec<PathBuf> {
    let crates_dir = root.join("crates");
    let mut files = Vec::new();
    let entries = match fs::read_dir(&crates_dir) {
        Ok(entries) => entries,
        Err(_) => return files,
    };
    for entry in entries.flatten() {
        let src = entry.path().join("src");
        if src.is_dir() {
            collect_rs_files(&src, &mut files);
        }
    }
    files.sort();
    files
}

/// 相对仓库根的正斜杠路径(Windows 反斜杠归一)。
fn rel_from_root(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 行内花括号净深度(字符串感知:字符串字面量内的 `{`/`}` 不计)。
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

/// 剥掉一行内的 `//` 行注释(含 `///`/`//!` 文档行——文档提及不算真实消费)。
fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    }
}

/// 生产段视图(与 gate_no_hardcoded_color 的规则 0 同语义):剥离整行
/// 注释与 `#[cfg(test)] mod` 区,消费标记只在真实生产代码里作数。
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

/// 门禁裁决(纯函数):`files` 为 (相对路径, 内容) 列表,返回违例清单,
/// 空 = 通过。消费文件数 < 2 或定义侧标记丢失即违例。
fn gate_violations(files: &[(String, String)]) -> Vec<String> {
    let mut bad = Vec::new();
    let mut consumers = Vec::new();
    for (rel, content) in files {
        let production = production_view(content);
        let is_definition = DEFINITION_FILES.contains(&rel.as_str());
        let hit = CONSUMPTION_MARKERS.iter().any(|m| production.contains(m));
        if is_definition {
            // 定义侧自检:tokens.rs 必须定义令牌表、theme.rs 必须有派生子
            // (带 "(" 防 shadow_quads 等更长名误配)
            let expected_marker = if rel.ends_with("tokens.rs") {
                "pub const ELEVATIONS"
            } else {
                "pub fn shadow("
            };
            if !production.contains(expected_marker) {
                bad.push(format!(
                    "{rel}:定义侧丢失 `{expected_marker}`(门禁地基失效)"
                ));
            }
        } else if hit {
            consumers.push(rel.clone());
        }
    }
    if consumers.len() < 2 {
        bad.push(format!(
            "ELEVATIONS 消费点不足:仅 {consumers:?}(要求 ≥ 2 个非 tokens/theme 的组件文件 \
             真实消费;杜绝\"定义即废弃\"复发,报告 §4.1 TOK-01)"
        ));
    }
    bad
}

// ---------------------------------------------------------------------------
// 正向门禁:当前 crates 面有两个真实消费点 + spec 可断言
// ---------------------------------------------------------------------------

/// TC-TOK-ELEV-01:两档海拔的 spec 纯函数可断言(L2 凸起 / L4 浮层取值、
/// shadow() 访问、quad 分层参数;离屏像素 golden 不在本轮要求——按报告
/// 验收口径"spec 断言 + 消费门禁即可",如实声明覆盖层级 = 参数/分层级,
/// 不含最终合成像素)。
#[test]
fn tc_tok_elev_01_elevation_tiers_are_spec_assertable() {
    // 消费点 1(NumberField,输入凸起 = L2):blur 8 / offset 2 / alpha 0.14
    let l2 = shadow(2);
    assert_eq!(
        (l2.blur, l2.offset_y, l2.alpha),
        (8.0, 2.0, 0.14),
        "L2 凸起档参数"
    );
    // 消费点 2(NeonCard,辉光底 = L3,同表对齐 L4 档):16/4/0.20 与 32/8/0.32
    let l3 = shadow(3);
    assert_eq!((l3.blur, l3.offset_y, l3.alpha), (16.0, 4.0, 0.20));
    let l4 = shadow(4);
    assert_eq!((l4.blur, l4.offset_y, l4.alpha), (32.0, 8.0, 0.32));
    // quad 分层参数(纯函数 spec):L2 → spread [2,4,6],alpha = 0.14×权重
    // (f32 乘积容差 1e-6)
    let l2_layers = shadow_layer_params(2).to_vec();
    let expect_l2 = [(2.0f32, 0.07f32), (4.0, 0.042), (6.0, 0.028)];
    for (got, want) in l2_layers.iter().zip(expect_l2) {
        assert!(
            (got.0 - want.0).abs() < 1e-6 && (got.1 - want.1).abs() < 1e-6,
            "L2 分层参数 {got:?} != {want:?}"
        );
    }
    // 分层元素数与权重表一致(shadow_quads 每级 3 层)
    let quads = sable_widgets::theme::shadow_quads(2);
    assert_eq!(quads.len(), 3, "quad 近似必须 3 层(内→外)");
    // L0 无影
    assert_eq!((shadow(0).blur, shadow(0).alpha), (0.0, 0.0));
}

#[test]
fn elevation_tokens_are_consumed() {
    let root = repo_root();
    let files = scan_face_files(&root);
    assert!(
        files.len() >= 20,
        "扫描面异常:仅找到 {} 个 .rs,目录收集逻辑疑似失效",
        files.len()
    );
    let mut faces = Vec::new();
    for file in &files {
        let rel = rel_from_root(&root, file);
        let content =
            fs::read_to_string(file).unwrap_or_else(|e| panic!("读取 {} 失败:{e}", file.display()));
        faces.push((rel, content));
    }
    let bad = gate_violations(&faces);
    assert!(
        bad.is_empty(),
        "TOK-01 海拔消费门禁违例:\n  - {}",
        bad.join("\n  - ")
    );
}

// ---------------------------------------------------------------------------
// 反面测试(TC-GATE-ELEV-01):移除消费点 → 门禁红
// ---------------------------------------------------------------------------

#[test]
fn tc_gate_elev_01_removing_consumers_turns_gate_red() {
    // 合成文件面工厂(每场景重建,规避所有权纠缠)
    fn face(rel: &str, body: &str) -> (String, String) {
        (rel.to_string(), body.to_string())
    }
    let tokens_def = face(
        "crates/sable-widgets/src/tokens.rs",
        "pub const ELEVATIONS: [Elevation; 5] = [ /* … */ ];",
    );
    let theme_def = face(
        "crates/sable-widgets/src/theme.rs",
        "pub fn shadow(level: usize) -> Elevation { }",
    );
    let consumer_a = face(
        "crates/sable-widgets/src/number_field.rs",
        "let root = crate::theme::elevated(2, inner);",
    );
    let consumer_b = face(
        "crates/sable-widgets/src/neon_card.rs",
        "let p = crate::tokens::ELEVATIONS[3];",
    );

    // 两个消费点在 → 绿
    let both = vec![
        tokens_def.clone(),
        theme_def.clone(),
        consumer_a.clone(),
        consumer_b.clone(),
    ];
    assert!(
        gate_violations(&both).is_empty(),
        "双消费点必须通过:{:?}",
        gate_violations(&both)
    );

    // 移除全部消费点 → 红
    let none = vec![tokens_def.clone(), theme_def.clone()];
    let bad = gate_violations(&none);
    assert!(
        bad.iter().any(|m| m.contains("消费点不足")),
        "TC-GATE-ELEV-01:全部消费点被移除必须红:{bad:?}"
    );

    // 仅剩一个消费点 → 红(报告要求"至少两档真实消费")
    let one = vec![tokens_def.clone(), theme_def.clone(), consumer_a.clone()];
    let bad = gate_violations(&one);
    assert!(
        bad.iter().any(|m| m.contains("消费点不足")),
        "仅剩单消费点必须红:{bad:?}"
    );

    // 定义侧地基被掏空(tokens.rs 不再定义 ELEVATIONS)→ 红
    let hollow_tokens = vec![
        face("crates/sable-widgets/src/tokens.rs", "// ELEVATIONS 被删除"),
        theme_def.clone(),
        consumer_a.clone(),
        consumer_b.clone(),
    ];
    let bad = gate_violations(&hollow_tokens);
    assert!(
        bad.iter().any(|m| m.contains("tokens.rs")),
        "定义侧地基丢失必须红:{bad:?}"
    );

    // theme.rs 丢派生子 → 红
    let hollow_theme = vec![
        tokens_def,
        face("crates/sable-widgets/src/theme.rs", "// shadow 被删除"),
        consumer_a,
        consumer_b,
    ];
    let bad = gate_violations(&hollow_theme);
    assert!(
        bad.iter().any(|m| m.contains("theme.rs")),
        "theme.rs 派生子丢失必须红:{bad:?}"
    );
}
