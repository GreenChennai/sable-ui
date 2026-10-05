//! TC-GATE-DUP-02(PERF-11):RGBA→`RenderImage` 转换全仓单点门禁。
//!
//! 扫描面(显式化):`crates/*/src/**/*.rs` 与 `examples/*/src/**/*.rs`,
//! 剥行注释与 `#[cfg(test)]` 区域(启发式与 `gate_no_panic.rs` 同款:花括号
//! 配对跳注释/字符串)后,命中以下任一即红——
//! - `RenderImage::new(`(gpui 裸帧构造)
//! - `RgbaImage::from_raw(`(image 裸帧构造,桥的内部步骤同样只许单点)
//!
//! 唯一豁免:`crates/sable-canvas/src/gpui_element.rs`(PERF-11 收口点)。
//! 反面用例:注入含两种模式的代码样本必红;豁免文件自身必绿。

use std::fs;
use std::path::{Path, PathBuf};

/// 递归收集 .rs 源文件。
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// 剥 `//` 行注释(本扫描面不处理块注释/字符串内的伪模式——仓库约定
/// 生产代码不在字符串里写这两个模式;误报时以豁免文件方式登记)。
fn strip_line_comments(src: &str) -> String {
    src.lines()
        .map(|line| match line.find("//") {
            Some(ix) => &line[..ix],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 命中判定:非豁免生产文件含任一裸帧构造模式 → Some(模式)。
fn violation(path: &Path, src: &str) -> Option<&'static str> {
    const SINGLE_POINT: &str = "gpui_element.rs";
    if path.file_name().is_some_and(|n| n == SINGLE_POINT) {
        return None;
    }
    let stripped = strip_line_comments(src);
    ["RenderImage::new(", "RgbaImage::from_raw("]
        .into_iter()
        .find(|pattern| stripped.contains(pattern))
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/sable-canvas 的上两级即仓库根")
        .to_path_buf()
}

#[test]
fn tc_gate_dup_02_render_image_bridge_is_single_point() {
    let root = repo_root();
    let mut files = Vec::new();
    // 扫描面 = 生产段(crates/*/src 与 examples/*/src);tests/、benches/ 不在
    // 门禁范围(golden 基线构造属测试代码,门禁自带的反面样本同理)。
    for crate_dir in std::fs::read_dir(root.join("crates"))
        .expect("crates 目录")
        .flatten()
    {
        let src = crate_dir.path().join("src");
        if src.is_dir() {
            collect_rs(&src, &mut files);
        }
    }
    for ex_dir in std::fs::read_dir(root.join("examples"))
        .expect("examples 目录")
        .flatten()
    {
        let src = ex_dir.path().join("src");
        if src.is_dir() {
            collect_rs(&src, &mut files);
        }
    }
    assert!(
        files.len() > 50,
        "扫描面异常:crates+examples 只找到 {} 个 rs 文件",
        files.len()
    );
    let mut violations = Vec::new();
    for path in &files {
        let Ok(src) = fs::read_to_string(path) else {
            continue;
        };
        if let Some(pattern) = violation(path, &src) {
            violations.push(format!(
                "{} 命中 {pattern}(唯一豁免:sable-canvas/src/gpui_element.rs)",
                path.display()
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "RGBA→RenderImage 桥必须收口到 sable-canvas::gpui_element 单点:\n{}",
        violations.join("\n")
    );
}

#[test]
fn tc_gate_dup_02_negative_injection_is_red() {
    // 反面 1:模拟在某组件文件里自写桥(含两种模式)→ 必红
    let injected = "fn bad(data: Vec<u8>) {\n    let b = image::RgbaImage::from_raw(1, 2, data);\n    let i = gpui::RenderImage::new(vec![image::Frame::new(b.unwrap())]);\n}\n";
    assert_eq!(
        violation(Path::new("crates/sable-widgets/src/fake.rs"), injected),
        Some("RenderImage::new(")
    );
    // 反面 2:注释里的提及不算(先剥注释)
    let commented = "// RenderImage::new 只吃 Frame\nfn ok() {}\n";
    assert_eq!(
        violation(Path::new("crates/sable-widgets/src/fake.rs"), commented),
        None
    );
    // 反面 3:豁免文件自身含模式 → 绿
    assert_eq!(
        violation(
            Path::new("crates/sable-canvas/src/gpui_element.rs"),
            "fn bridge() { image::RgbaImage::from_raw(1, 2, v); }\n"
        ),
        None
    );
}
