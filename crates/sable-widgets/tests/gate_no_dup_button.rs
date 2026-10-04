//! TC-GATE-DUP-01:私有按钮实现**收口门禁**(迭代审查报告 2026-10-04
//! CMP-01/CMP-11,§5.11 G-UI-H 的第一把刀)。
//!
//! # 病根(门禁要防的事)
//!
//! 基础控件层缺席期间,四处面板各写各的私有按钮 helper
//! (`layer_panel.rs:429` `simple_tool_button`、`effect_stack.rs:440`
//! `mini_button`、`gradient_editor.rs:259` `text_button`、`layer_tree.rs:585`
//! `tree_glyph_button`)——视觉规格四处漂移、TOK-07 禁用语义各自实现。
//! CMP-11 收口后,一切按钮统一走 `controls::button` 的 `Button`/`IconButton`
//! (`sable_widgets::controls::button`),私有实现不得复发。
//!
//! # 扫描面(显式化,GATE-03 纪律③)
//!
//! - 递归收集 `<仓库根>/crates/*/src/**/*.rs`(不含 examples/tests/docs);
//! - 剥掉行注释(`//`/`///`/`//!`)与 `#[cfg(test)] mod` 区(与
//!   gate_no_hardcoded_color 的规则 0、gate_elevation_consumed 的
//!   `production_view` 同语义),只在**真实生产段**作数;
//! - 生产段命中 `fn [a-z_]*_button(` 形态的私有按钮定义即红
//!   (`fn button_style(`/`Button::new(` 等非按钮定义形态不命中);
//! - **豁免面**:`src/controls/` 目录(button.rs 等基础控件实现是正主);
//! - 注释里提及"xxx_button 已删"不影响裁决(注释先剥)。
//!
//! # 反面测试(门禁要有反面用例,GATE-03 纪律②)
//!
//! - 注入含 `fn my_button(` 的合成生产文件 → 裁决必须报红;
//! - 同一内容放进 `src/controls/` 豁免面 → 必须放行;
//! - 同一 `fn` 写进 `#[cfg(test)] mod tests` → 剥除后必须放行;
//! - 非按钮形态(`fn button_style(`)→ 必须放行(防误伤)。

use std::fs;
use std::path::{Path, PathBuf};

/// 豁免面:路径含此子串(正斜杠归一后)不参与裁决——controls/ 是基础
/// 控件的唯一实现地(CMP-01 收口的正主)。
const EXEMPT_SUBSTR: &str = "src/controls/";

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

/// 剥掉一行内的 `//` 行注释(含 `///`/`//!` 文档行——注释提及不算实现)。
fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    }
}

/// 生产段视图(与 gate_no_hardcoded_color / gate_elevation_consumed 的规则
/// 0 同语义):剥离整行注释与 `#[cfg(test)] mod` 区,匹配只在真实生产
/// 代码里作数。
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

/// 单行匹配 `fn [a-z_]*_button(`:取 `fn ` 后的 `[a-z0-9_]` 标识符,
/// 以 `_button` 结尾且紧随 `(` 即命中(返回命中的标识符)。
fn dup_button_ident(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut from = 0usize;
    while let Some(at) = line[from..].find("fn ") {
        let start = from + at + 3;
        if start >= bytes.len() {
            break;
        }
        let ident: String = bytes[start..]
            .iter()
            .map(|&c| c as char)
            .take_while(|c| c.is_ascii_lowercase() || *c == '_' || c.is_ascii_digit())
            .collect();
        let after = start + ident.len();
        if ident.ends_with("_button") && after < bytes.len() && bytes[after] == b'(' {
            return Some(ident);
        }
        from = start;
    }
    None
}

/// 门禁裁决(纯函数):`files` 为 (相对路径, 内容) 列表,返回违例清单,
/// 空 = 通过。`src/controls/` 豁免;`#[cfg(test)]` 区剥除。
fn gate_violations(files: &[(String, String)]) -> Vec<String> {
    let mut bad = Vec::new();
    for (rel, content) in files {
        if rel.contains(EXEMPT_SUBSTR) {
            continue;
        }
        let production = production_view(content);
        for line in production.lines() {
            if let Some(ident) = dup_button_ident(line) {
                bad.push(format!(
                    "{rel}:生产段出现私有按钮定义 `fn {ident}(`——按钮统一走 \
                     controls::button 的 Button/IconButton(CMP-11/TC-GATE-DUP-01)"
                ));
            }
        }
    }
    bad
}

// ---------------------------------------------------------------------------
// 正向门禁:当前 crates 面零私有按钮实现
// ---------------------------------------------------------------------------

#[test]
fn no_private_button_definitions_in_production() {
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
        "CMP-11 私有按钮收口门禁违例:\n  - {}",
        bad.join("\n  - ")
    );
}

/// 正主的自我保护:controls/button.rs 必须仍存在且定义 Button/IconButton
/// (定义被删/改名后,豁免面就变成了"无人实现按钮"的假阴性)。
#[test]
fn controls_button_module_still_offers_the_components() {
    let root = repo_root();
    let path = root.join("crates/sable-widgets/src/controls/button.rs");
    let content =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("读取 {} 失败:{e}", path.display()));
    for expected in [
        "pub struct Button",
        "pub struct IconButton",
        "pub fn button_style(",
        "pub fn button_height(",
        "pub fn button_element(",
        "pub fn icon_button_element(",
        "pub fn solid_variant_foreground(",
    ] {
        assert!(
            content.contains(expected),
            "controls/button.rs 丢失 `{expected}`(收口正主失效,门禁地基动摇)"
        );
    }
}

// ---------------------------------------------------------------------------
// 反面测试(注入缺陷必须红灯;纯函数裁决,不写文件)
// ---------------------------------------------------------------------------

/// TC-GATE-DUP-01 反面 1:合成生产文件含 `fn my_button(` → 必须报红。
#[test]
fn tc_gate_dup_01_injected_private_button_is_red() {
    let files = vec![(
        "crates/sable-widgets/src/some_panel.rs".to_string(),
        "#[cfg(test)]\nmod tests {}\n\nfn helper() {}\n\nfn my_button(\n    label: &str,\n) -> Div {\n    div()\n}\n"
            .to_string(),
    )];
    let bad = gate_violations(&files);
    assert_eq!(bad.len(), 1, "注入私有按钮必须命中:{bad:?}");
    assert!(bad[0].contains("some_panel.rs"), "违例须带文件定位:{bad:?}");
    assert!(bad[0].contains("my_button"), "违例须带标识符:{bad:?}");
}

/// TC-GATE-DUP-01 反面 2:同一内容在 controls/ 豁免面 → 必须放行。
#[test]
fn tc_gate_dup_01_controls_module_is_exempt() {
    let files = vec![(
        "crates/sable-widgets/src/controls/button.rs".to_string(),
        "fn my_button(label: &str) -> Div {\n    div()\n}\n".to_string(),
    )];
    assert!(
        gate_violations(&files).is_empty(),
        "controls/ 是按钮实现正主,必须豁免"
    );
}

/// TC-GATE-DUP-01 反面 3:`fn *_button(` 写进 `#[cfg(test)]` 区 → 剥除后放行
/// (测试桩/固定装置不是生产实现)。
#[test]
fn tc_gate_dup_01_cfg_test_section_is_stripped() {
    let content = "fn prod() {}\n\n#[cfg(test)]\nmod tests {\n    fn fake_button() {}\n\n    #[test]\n    fn t() {\n        fake_button();\n    }\n}\n";
    let files = vec![("crates/x/src/panel.rs".to_string(), content.to_string())];
    assert!(
        gate_violations(&files).is_empty(),
        "cfg(test) 区必须剥除(生产段裁决)"
    );
    // 同内容若出现在生产段则必红(自证剥除逻辑真的在工作)
    let leaked = "fn fake_button() {}\n";
    let prod_files = vec![("crates/x/src/panel.rs".to_string(), leaked.to_string())];
    assert_eq!(gate_violations(&prod_files).len(), 1);
}

/// TC-GATE-DUP-01 反面 4:非按钮定义形态不误伤(`fn button_style(`、
/// `Button::new(`、注释提及、返回类型)。
#[test]
fn tc_gate_dup_01_non_button_shapes_are_not_flagged() {
    let content = "/// 调用 fn old_button( 的注释不算实现\n\
                   pub fn button_style(size: ButtonSize) -> ButtonStyle {\n}\n\
                   fn make_ui() -> Div {\n    let b = Button::new(\"x\", \"X\");\n    b\n}\n\
                   fn buttons_container() -> Div { div() }\n";
    let files = vec![("crates/x/src/panel.rs".to_string(), content.to_string())];
    assert!(
        gate_violations(&files).is_empty(),
        "非 `fn *_button(` 形态不得误伤:{:?}",
        gate_violations(&files)
    );
}
