//! GATE-02 行数门禁 + COUP-01 依赖图门禁(迭代审查报告 §4.8/§4.7)。
//!
//! 扫描面(显式化):
//! - 行数:`crates/*/src/**/*.rs` 生产文件(不含 tests/benches/examples);
//!   上限 = **现有最大值 2800 行**(2026-10-04 基线:command.rs 2737)。
//!   报告原案 ≤800 行需拆分 command/tool/render 三巨石,归属第 8 组后
//!   独立拆分轮(公开 API 面,须过 public-api 快照);本轮门禁先钉死
//!   "不再恶化"——超基线即红,拆分后逐文件下调上限。
//! - 依赖图:逐 crate 读 Cargo.toml 文本,断言金字塔单向
//!   (foundation 零 UI/渲染依赖;canvas 不得依赖 widgets/dock;
//!   paint 不得依赖 canvas/widgets/dock;video/script 仅 foundation;
//!   widgets 不得依赖 dock)。越界即红(COUP-R1)。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const LINE_CEILING: usize = 2800;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/sable-foundation 上两级 = 仓库根")
        .to_path_buf()
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn gate_02_line_count_ceiling_not_worsened() {
    let root = repo_root();
    let mut files = Vec::new();
    for crate_dir in fs::read_dir(root.join("crates")).expect("crates").flatten() {
        let src = crate_dir.path().join("src");
        if src.is_dir() {
            collect_rs(&src, &mut files);
        }
    }
    let mut violations = Vec::new();
    for f in &files {
        let Ok(src) = fs::read_to_string(f) else {
            continue;
        };
        let lines = src.lines().count();
        if lines > LINE_CEILING {
            violations.push(format!("{} = {lines} 行 > {LINE_CEILING}", f.display()));
        }
    }
    assert!(
        violations.is_empty(),
        "GATE-02 生产文件行数超基线(拆分前上限 {LINE_CEILING};\
         command.rs/tool.rs/render.rs 巨石拆分登记在案,拆一个降一个):\n{}",
        violations.join("\n")
    );
}

#[test]
fn coup_01_dependency_graph_stays_one_way() {
    let root = repo_root();
    let mut tomls: HashMap<String, String> = HashMap::new();
    for crate_dir in fs::read_dir(root.join("crates")).expect("crates").flatten() {
        let toml = crate_dir.path().join("Cargo.toml");
        if toml.is_file() {
            let name = crate_dir.file_name().to_string_lossy().to_string();
            tomls.insert(name, fs::read_to_string(toml).expect("读 Cargo.toml"));
        }
    }
    // 禁止边:(下游 crate, 禁止出现在其 [dependencies] 的上游 crate 名)
    const FORBIDDEN: &[(&str, &[&str])] = &[
        ("sable-foundation", &["gpui", "vello", "sable-"]),
        (
            "sable-paint",
            &["sable-canvas", "sable-widgets", "sable-dock", "gpui"],
        ),
        ("sable-canvas", &["sable-widgets", "sable-dock"]),
        (
            "sable-video",
            &[
                "sable-paint",
                "sable-canvas",
                "sable-widgets",
                "sable-dock",
                "gpui",
            ],
        ),
        (
            "sable-script",
            &[
                "sable-paint",
                "sable-canvas",
                "sable-video",
                "sable-widgets",
                "sable-dock",
                "gpui",
            ],
        ),
        ("sable-widgets", &["sable-dock"]),
    ];
    let mut violations = Vec::new();
    for (crate_name, forbidden) in FORBIDDEN {
        let Some(text) = tomls.get(*crate_name) else {
            continue;
        };
        // 只看 [dependencies] 段(dev-dependencies 允许);剥注释行,
        // 取真实依赖键(`ident = ...` / `ident.workspace = true` / 目标表头)精确比对。
        let deps = text
            .split("[dev-dependencies]")
            .next()
            .expect("split 总有首段");
        let keys: Vec<String> = deps
            .lines()
            .map(str::trim_start)
            .filter(|l| !l.starts_with('#') && !l.starts_with('['))
            .filter_map(|l| l.split('=').next())
            .map(|k| k.trim().trim_matches('"').to_string())
            .filter(|k| !k.is_empty())
            .collect();
        for f in *forbidden {
            let hit = keys
                .iter()
                .any(|k| k == f || (f.ends_with('-') && k.starts_with(f)));
            if hit {
                violations.push(format!("{crate_name} → {f}(违反金字塔单向)"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "COUP-01 依赖图越界:\n{}",
        violations.join("\n")
    );
}
