//! GATE-04:零硬编码色纪律静态门禁(迭代审查报告 §4.8 GATE-04;§5.11 G-UI-B)。
//!
//! # 纪律(宣称即证据的代码化)
//!
//! `crates/sable-widgets/src/tokens.rs` 是全 workspace 唯一允许**定义**颜色
//! 字面量的文件(令牌单一真相源,见其头部纪律注释);`crates/sable-widgets/
//! src/theme.rs` 集中定义画布语义色表(深浅两套 CanvasTheme)。其余组件与
//! 渲染代码一律经 `sable_widgets::tokens` / `sable_widgets::theme::theme(cx)`
//! 取语义色,不得直接写字面量颜色。
//!
//! # 扫描面(显式化,GATE-03 纪律③)
//!
//! - 递归扫描 `<仓库根>/crates/*/src/**/*.rs`(以 `CARGO_MANIFEST_DIR` 定位
//!   仓库根)。**不含** examples/、tests/、benches/、docs/——扫描面是生产段。
//! - 豁免:本目录 `gate_no_hardcoded_color.allowlist`,每行
//!   `<仓库根相对路径> # 原因`,文件级整文件豁免;新增豁免必须写明原因。
//!
//! # 判定规则(逐行,先剥离 `//` 行注释)
//!
//! 0. **只看生产段**:`#[cfg(test)] mod` 区内的命中不计(对齐 GATE-01
//!    panic 门禁的"生产段"语义;测试构造端点色/断言 SVG 序列化语法是正当
//!    用法)。跨文件 `#[cfg(test)] mod tests;` 引入的整文件测试模块不受此
//!    规则覆盖,出现命中时走豁免清单。
//! 1. `rgba(`/`hsla(`/`rgb(` 调用且第一个实参以**数字/`.`/`+`/`-` 起始**
//!    (即字面量实参形态)。前缀要求词边界,排除 `draw_rgba`/`lerp_hsla`
//!    等标识符;首实参为变量/表达式的动态构造(如 `hsla(hue_i, ..)`)不算
//!    硬编码——色轮按定义逐色相构造颜色属于组件功能本体,不是主题装饰色。
//! 2. `#` 后接 3/6/8 位十六进制(`#RGB`/`#RRGGBB`/`#RRGGBBAA`)。前导字符
//!    为 `{`/`:`/`#` 时视为格式串占位(如 `{bits:#016x}`)跳过。
//!
//! # 已知局限(逐行近似,宁红勿漏)
//!
//! - 只剥离 `//` 行注释:`/* .. */` 跨行块注释、跨行原始字符串内的命中会
//!   误报(出现时人工裁决:改代码或写豁免)。
//! - `'"'` 字符字面量会翻转行内字符串状态(本仓无此用法)。
//! - 画布侧 `sable-canvas/src/render.rs` 的 `CanvasColors` 以 `[u8; 4]`
//!   字节流形态定义集中语义色,不在上述规则覆盖内——已记录于
//!   docs/12-门禁与质量纪律.md 的门禁清单,后续并入 G-UI-A 令牌对齐。

use std::fs;
use std::path::{Path, PathBuf};

/// 规则 1 的关键字(rgba 先于 rgb,避免 "rgba" 被 "rgb" 抢先截断)。
const COLOR_FUNCS: [&str; 3] = ["rgba", "hsla", "rgb"];

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// 判定单行代码(已剥离 `//` 注释)是否含颜色字面量。纯函数。
fn line_has_color_literal(code: &str) -> bool {
    let bytes = code.as_bytes();

    // 规则 1:rgba(/hsla(/rgb( 且首实参以字面量字符起始
    for kw in COLOR_FUNCS {
        let mut start = 0;
        while let Some(rel) = code[start..].find(kw) {
            let at = start + rel;
            start = at + kw.len();
            // 词边界:前一字符是标识符字符 → 是更长标识符的一部分,跳过
            if at > 0 && is_ident_byte(bytes[at - 1]) {
                continue;
            }
            // 关键字后(允许空白)必须紧跟 "("
            let mut i = start;
            while i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
            if i >= bytes.len() || bytes[i] != b'(' {
                continue;
            }
            // "(" 后跳过空白,首实参以数字/./+/- 起始 → 字面量实参
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j < bytes.len()
                && (bytes[j].is_ascii_digit()
                    || bytes[j] == b'.'
                    || bytes[j] == b'+'
                    || bytes[j] == b'-')
            {
                return true;
            }
        }
    }

    // 规则 2:#RGB / #RRGGBB / #RRGGBBAA
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            // 格式串占位({:#016x} 的 '#' 前导为 ':'/'{'/'#')→ 跳过
            let prev = if i == 0 { 0 } else { bytes[i - 1] };
            if !matches!(prev, b'{' | b':' | b'#') {
                let mut run = 0usize;
                while i + 1 + run < bytes.len() && bytes[i + 1 + run].is_ascii_hexdigit() {
                    run += 1;
                }
                if run == 3 || run == 6 || run == 8 {
                    return true;
                }
                i += 1 + run;
                continue;
            }
        }
        i += 1;
    }
    false
}

/// 剥离一行内的 `//` 行注释(跟踪字符串字面量状态;`\"` 转义识别)。
fn strip_line_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '/' if !in_string && line[idx..].starts_with("//") => {
                return &line[..idx];
            }
            _ => {}
        }
    }
    line
}

/// 行内花括号净深度(字符串感知:字符串字面量内的 `{`/`}` 不计;
/// `{{`/`}}` 转义自然抵消)。
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

/// 扫描一段源码,返回命中的 (行号, 原始行)。
///
/// 带一个简单的 `#[cfg(test)] mod` 区域跟踪(brace 深度,字符串感知):
/// 区域内的命中不计——门禁只看生产段(规则 0)。
fn scan_str(content: &str) -> Vec<(usize, String)> {
    let mut hits = Vec::new();
    let mut depth = 0usize;
    let mut test_pending = false; // 刚见过 depth==0 的 #[cfg(test)] 属性
    let mut in_test = false; // 位于 #[cfg(test)] mod 区内
    let mut test_base_depth = 0usize; // 进入测试区时的外层深度

    for (i, raw) in content.lines().enumerate() {
        let code = strip_line_comment(raw);
        let has_test_attr = code.contains("#[cfg(test)]");
        let opens_mod = code.contains("mod ") && code.contains('{');
        let depth_before = depth;

        if !in_test && line_has_color_literal(code) {
            hits.push((i + 1, raw.trim_end().to_string()));
        }

        depth = brace_depth(code, depth);

        if has_test_attr && depth_before == 0 {
            if opens_mod && depth > depth_before {
                // `#[cfg(test)] mod tests {` 同行写法
                in_test = true;
                test_base_depth = depth_before;
            } else {
                test_pending = true;
            }
        } else if test_pending {
            if opens_mod && depth > depth_before {
                in_test = true;
                test_base_depth = depth_before;
            }
            // 属性只作用于紧随项:无论是否 mod,待定态就此失效
            test_pending = false;
        }

        if in_test && depth <= test_base_depth {
            in_test = false;
        }
    }
    hits
}

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

/// 豁免清单(相对仓库根、正斜杠路径集合)。
fn allowlist() -> Vec<String> {
    let file =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/gate_no_hardcoded_color.allowlist");
    let content = fs::read_to_string(&file).unwrap_or_else(|e| {
        panic!(
            "豁免清单 {} 读取失败:{e}(门禁扫描面必须有显式豁免文件)",
            file.display()
        )
    });
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            line.split_once('#')
                .map(|(path, _reason)| path.trim())
                .unwrap_or(line)
                .to_string()
        })
        .map(|path| path.replace('\\', "/"))
        .collect()
}

/// 相对仓库根的正斜杠路径(Windows 反斜杠归一)。
fn rel_from_root(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

// ---------------------------------------------------------------------------
// 反面单测(GATE-03 纪律②:门禁要有反面测试——注入缺陷必须红)
// ---------------------------------------------------------------------------

#[test]
fn negative_numeric_arg_color_calls_are_flagged() {
    for code in [
        "let c = rgba(1,2,3,1);",
        "let c = hsla(0.5, 1.0, 0.5, 1.0);",
        "let c = rgb(0xFF0000FF);",
        "let c = rgba( 1, 2, 3, 1 );",
        "paint.fill = rgba(-1, -2, -3, 1);",
    ] {
        assert!(!scan_str(code).is_empty(), "注入缺陷必须命中:{code}");
    }
}

#[test]
fn negative_hex_literals_are_flagged() {
    for code in [
        "let s = \"#abc\";",
        "let s = \"#aabbcc\";",
        "let s = \"#aabbccdd\";",
        "theme.fallback = \"#4F9FFF\";",
    ] {
        assert!(!scan_str(code).is_empty(), "注入缺陷必须命中:{code}");
    }
}

#[test]
fn positive_clean_code_passes() {
    let clean = concat!(
        "let x = sink.draw_rgba(&buf, w, h, dx, dy);\n",
        "let y = lerp_hsla(a, b, t);\n",
        "let s = \"rgba({},{},{},{})\"; // SVG 序列化格式串\n",
        "let f = format!(\"{bits:#016x}\");\n",
        "#[derive(Debug)]\n",
        "struct S;\n",
        "let z = supports_draw_rgba();\n",
        "let hue_ring = gpui::hsla(hue_i, 1.0, 0.5, 1.0);\n",
    );
    assert!(
        scan_str(clean).is_empty(),
        "干净代码不得命中:{:?}",
        scan_str(clean)
    );
}

#[test]
fn positive_line_comments_are_stripped() {
    assert!(scan_str("let ok = true; // rgba(1,2,3,1)").is_empty());
    assert!(scan_str("/// 文档里的参考色 #00c8ff 不算命中").is_empty());
    assert!(scan_str("// #ffffff").is_empty());
}

#[test]
fn positive_cfg_test_regions_are_skipped() {
    let code = concat!(
        "fn prod() { let theme = theme(cx); }\n",
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    #[test]\n",
        "    fn t() {\n",
        "        let a = rgba(1, 2, 3, 1);\n",
        "        assert_eq!(format!(\"{a:?}\"), \"#aabbcc\");\n",
        "    }\n",
        "}\n",
    );
    assert!(
        scan_str(code).is_empty(),
        "#[cfg(test)] 区内命中必须跳过(规则 0):{:?}",
        scan_str(code)
    );
}

#[test]
fn negative_test_region_does_not_leak_after_close() {
    // 测试区闭合之后,生产段命中必须重新变红(区域跟踪不得失灵)
    let code = concat!(
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    fn t() { let a = rgba(1,2,3,1); }\n",
        "}\n",
        "fn prod() { let b = rgba(1,2,3,1); }\n",
    );
    let hits = scan_str(code);
    assert_eq!(hits.len(), 1, "仅生产段那一行应命中:{hits:?}");
    assert_eq!(hits[0].0, 5, "命中的应是第 5 行(生产段)");
}

// ---------------------------------------------------------------------------
// 正向门禁:当前 crates 面 = 0 违例(TC-GATE-COLOR-01 的守卫侧)
// ---------------------------------------------------------------------------

#[test]
fn crates_face_has_zero_hardcoded_colors() {
    let root = repo_root();
    let files = scan_face_files(&root);
    assert!(
        files.len() >= 20,
        "扫描面异常:仅找到 {} 个 .rs,目录收集逻辑疑似失效",
        files.len()
    );
    let allow = allowlist();
    let mut violations = Vec::new();
    for file in &files {
        let rel = rel_from_root(&root, file);
        if allow.iter().any(|a| a == &rel) {
            continue;
        }
        let content =
            fs::read_to_string(file).unwrap_or_else(|e| panic!("读取 {} 失败:{e}", file.display()));
        for (line_no, line) in scan_str(&content) {
            violations.push(format!("{rel}:{line_no}: {line}"));
        }
    }
    assert!(
        violations.is_empty(),
        "硬编码色纪律违例 {} 处(组件一律经 tokens/theme 取色;确需豁免请附原因写入 \
         crates/sable-widgets/tests/gate_no_hardcoded_color.allowlist):\n{}",
        violations.len(),
        violations.join("\n")
    );
}

#[test]
fn allowlist_entries_exist_and_cover_the_contract() {
    let root = repo_root();
    let allow = allowlist();
    assert!(
        allow.len() >= 2,
        "豁免清单至少应含 tokens.rs 与 theme.rs(纪律合同),实际 {allow:?}"
    );
    for entry in &allow {
        let path = root.join(entry.replace('/', "\\"));
        assert!(
            path.is_file(),
            "豁免条目 {entry} 指向的文件不存在(改名后请同步清理豁免)"
        );
    }
}
