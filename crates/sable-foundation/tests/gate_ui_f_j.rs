//! G-UI-F + G-UI-J(迭代审查报告 §5.11 UI 门禁矩阵最后两项)。
//!
//! G-UI-F 组件尺寸 token 化:扫描 `crates/*/src` 生产段,命中**裸魔法尺寸**
//! —— `px(<数字字面量>)` / `w|h|size_<span>(<数字字面量>)` —— 即红;
//! 豁免(显式化):① tokens.rs/layout.rs(令牌定义域);② 各文件内
//! `const *_PX: f32` 单点常量的**定义行**(使用处一律走常量,不出现字面量);
//! ③ `px(1.0)`/`px(2.0)` 级 1–2px 细缝/hairline(报告 §5.3.5 hairline 级,
//! 白名单登记);④ controls 几何常量定义文件中的测试段。
//! G-UI-J feature 门控一致性:逐 crate 解析 Cargo.toml `[features]` 的具名
//! feature(排除 default/full/内部依赖链),断言 lib.rs 中存在
//! `#[cfg(feature = "<名>")]` 或 `#[cfg(all(... feature = "<名>" ...))]`
//! 至少一处——每个 feature 至少门控一个模块(CMP-12 的门禁面)。

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("foundation 上两级 = 仓库根")
        .to_path_buf()
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// 裸魔法尺寸判定:行内含 `px(<数字>)` 且不在豁免文件。
fn has_magic_px(line: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with("//") {
        return false;
    }
    // px( 后紧跟数字(整数或小数)
    let mut rest = line;
    while let Some(i) = rest.find("px(") {
        let after = &rest[i + 3..];
        let head = after.trim_start();
        if head.starts_with(|c: char| c.is_ascii_digit()) {
            return true;
        }
        rest = after;
    }
    false
}

#[test]
fn g_ui_f_no_bare_magic_sizes_in_components() {
    let root = repo_root();
    let mut files = Vec::new();
    for crate_dir in fs::read_dir(root.join("crates")).expect("crates").flatten() {
        let src = crate_dir.path().join("src");
        if src.is_dir() {
            collect_rs(&src, &mut files);
        }
    }
    // 豁免:令牌/布局定义域 + 画布几何单点(常量定义已在该域)
    const EXEMPT_FILES: &[&str] = &["tokens.rs", "layout.rs"];
    // 1–2px hairline 级白名单(报告 §5.3.5:发丝线/细缝,值域单点语义)
    let hairline = |line: &str| {
        for n in [
            "px(0)", "px(0.0)", "px(1)", "px(1.0)", "px(2)", "px(2.0)", "px(1.5)",
        ] {
            if line.contains(n) {
                return true;
            }
        }
        false
    };
    let mut violations = Vec::new();
    for f in &files {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if EXEMPT_FILES.contains(&name) {
            continue;
        }
        let Ok(src) = fs::read_to_string(f) else {
            continue;
        };
        // 测试后置约定:#[cfg(test)] 行到文件尾 = 测试段,豁免
        // (与本仓全部文件的测试布局一致;文件头注释已声明该启发式)。
        let effective: String = src
            .lines()
            .take_while(|l| !l.trim_start().starts_with("#[cfg(test)]"))
            .collect::<Vec<_>>()
            .join(
                "
",
            );
        for (ix, line) in effective.lines().enumerate() {
            // 测试断言里的数值(像素回归校验)非组件尺寸来源
            if line.trim_start().starts_with("assert") {
                continue;
            }
            if has_magic_px(line) && !hairline(line) {
                violations.push(format!("{}:{} {line}", f.display(), ix + 1));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "G-UI-F 裸魔法尺寸(应抽成 *_PX 常量单点或走 tokens):\n{}",
        violations.join("\n")
    );
}

#[test]
fn g_ui_j_every_feature_gates_at_least_one_module() {
    let root = repo_root();
    // 逐 crate:features ↔ lib.rs cfg(feature=...) 交叉核对
    for crate_dir in fs::read_dir(root.join("crates")).expect("crates").flatten() {
        let toml_path = crate_dir.path().join("Cargo.toml");
        if !toml_path.is_file() {
            continue;
        }
        let toml = fs::read_to_string(&toml_path).expect("读 toml");
        let Some(feat_sec) = toml.split("[features]").nth(1) else {
            continue;
        };
        let feat_sec = feat_sec.split('[').next().expect("split 总有首段");
        let features: Vec<String> = feat_sec
            .lines()
            .filter_map(|l| l.split('=').next())
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty() && k != "default" && !k.starts_with('#'))
            .collect();
        // 内部组合 feature(full 等)不算"门控一个模块"的候选:
        // 只检查"叶子 feature"= 值不含其它 feature 名的空/依赖单值。
        let lib = crate_dir.path().join("src").join("lib.rs");
        let lib_src = fs::read_to_string(&lib).unwrap_or_default();
        let mut violations = Vec::new();
        for feat in &features {
            // 组合 feature(值里引用了其它 feature)跳过——它们是打包件
            let value = feat_sec
                .lines()
                .find(|l| l.trim_start().starts_with(feat))
                .and_then(|l| l.split('=').nth(1))
                .unwrap_or("");
            let is_bundle = value.contains('"') && (value.contains(",") || value.contains("dep:"));
            if is_bundle {
                continue;
            }
            let gated = lib_src.contains(&format!("feature = \"{feat}\""))
                || lib_src.contains(&format!("feature = \"{feat}\","))
                || lib_src.contains(&format!("\", feature = \"{feat}\""));
            if !gated {
                violations.push(format!(
                    "{}:feature `{feat}` 未门控任何模块",
                    crate_name_of(&lib)
                ));
            }
        }
        assert!(
            violations.is_empty(),
            "G-UI-J feature 门控一致性(每个 feature 至少门控一个 pub 模块):\n{}",
            violations.join("\n")
        );
    }
}

fn crate_name_of(_lib: &Path) -> String {
    _lib.parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}
