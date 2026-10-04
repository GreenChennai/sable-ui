//! TC-GATE-GPU-01(迭代审查报告 RBT-01)静态门禁:
//! `render_with_recovery` 必须至少有一个**生产调用点**——调用所在文件不是定义文件
//! (`crates/sable-paint/src/gpu.rs`),且不在 `#[cfg(test)]` 区域内;命中即红。
//!
//! 背景:RBT-01 的根因正是"`render_with_recovery` 全仓仅命中定义与 re-export,
//! 零生产调用方"——抽象在、恢复路径从未被走过。本门禁保证接线一旦拆除即刻显红。
//!
//! ## 启发式说明(与 `crates/sable-foundation/tests/gate_no_panic.rs` 同一套规则)
//! 1. **`#[cfg(test)] mod X { … }` 内联区域**:花括号配对剥离(配对跳过行注释/
//!    块注释/字符串字面量,含原始字符串),被剥离字符替换为空格、换行保留;
//! 2. **文件级 `#[cfg(test)] mod X;`**:对应 `X.rs`/`X/mod.rs` 子树整体跳过;
//! 3. **调用语法针**:`render_with_recovery(`(带括号)。文档/注释里不带括号的
//!    提及不算调用;定义形如 `fn render_with_recovery<`/`render_with_recovery<`
//!    (泛型尖括号在括号前)不会被误判,但定义文件整体按路径排除。
//!
//! 已知取舍:漏报(如字符串字面量伪造调用)靠 code review 兜底,误报走门禁本体修正。

use std::fs;
use std::path::{Path, PathBuf};

/// 状态机与 `GpuGuard::render_with_recovery` 方法的定义文件(仓库相对路径)。
const DEFINITION_FILE: &str = "crates/sable-paint/src/gpu.rs";

/// 调用语法针:函数/方法调用必有紧跟的左括号。
const NEEDLE: &str = "render_with_recovery(";

const CFG_TEST_ATTR: &str = "#[cfg(test)]";

// ---------------------------------------------------------------------------
// 词法与区域剥离(规则对齐 gate_no_panic.rs,按本门禁需求裁剪)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Lex {
    Code,
    LineComment,
    BlockComment,
    Str,
}

fn starts_with(chars: &[char], i: usize, s: &str) -> bool {
    s.chars()
        .enumerate()
        .all(|(k, c)| chars.get(i + k) == Some(&c))
}

/// `chars[i]` 是 `r`:是否为原始字符串起点(`r"` / `r#"…"#`),返回井号数。
fn raw_string_hashes(chars: &[char], i: usize) -> Option<usize> {
    if chars.get(i) != Some(&'r') {
        return None;
    }
    let mut j = i + 1;
    let mut hashes = 0;
    while chars.get(j) == Some(&'#') {
        hashes += 1;
        j += 1;
    }
    if chars.get(j) == Some(&'"') {
        Some(hashes)
    } else {
        None
    }
}

/// `chars[i]` 是 `'`:若为字符字面量(含 `\` 转义)返回结束引号下标;
/// 否则视为生命周期返回 `None`。
fn char_literal_end(chars: &[char], i: usize) -> Option<usize> {
    let mut j = i + 1;
    if chars.get(j) == Some(&'\\') {
        j += 1;
        while j < chars.len() {
            match chars[j] {
                '\'' => return Some(j),
                '\n' => return None,
                _ => j += 1,
            }
        }
        return None;
    }
    if chars.get(j + 1) == Some(&'\'') {
        Some(j + 1)
    } else {
        None
    }
}

fn is_str_close(chars: &[char], i: usize, hashes: usize) -> bool {
    chars.get(i) == Some(&'"') && (1..=hashes).all(|k| chars.get(i + k) == Some(&'#'))
}

/// `chars[open]` 是 Code 态的 `{`:返回配对 `}` 下标(注释/字符串不参与配对)。
fn find_matching_brace(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut block_depth = 0usize;
    let mut raw_hashes = 0usize;
    let mut state = Lex::Code;
    let mut i = open;
    while i < chars.len() {
        let c = chars[i];
        match state {
            Lex::Code => match c {
                '/' if chars.get(i + 1) == Some(&'/') => {
                    state = Lex::LineComment;
                    i += 2;
                }
                '/' if chars.get(i + 1) == Some(&'*') => {
                    state = Lex::BlockComment;
                    block_depth = 1;
                    i += 2;
                }
                '"' => {
                    raw_hashes = 0;
                    state = Lex::Str;
                    i += 1;
                }
                'r' if raw_string_hashes(chars, i).is_some() => {
                    raw_hashes = raw_string_hashes(chars, i).unwrap_or(0);
                    state = Lex::Str;
                    i += 1 + raw_hashes + 1;
                }
                '\'' => match char_literal_end(chars, i) {
                    Some(end) => i = end + 1,
                    None => i += 1,
                },
                '{' => {
                    depth += 1;
                    i += 1;
                }
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(i);
                    }
                    i += 1;
                }
                _ => i += 1,
            },
            Lex::LineComment => {
                if c == '\n' {
                    state = Lex::Code;
                }
                i += 1;
            }
            Lex::BlockComment => {
                if c == '/' && chars.get(i + 1) == Some(&'*') {
                    block_depth += 1;
                    i += 2;
                } else if c == '*' && chars.get(i + 1) == Some(&'/') {
                    block_depth -= 1;
                    i += 2;
                    if block_depth == 0 {
                        state = Lex::Code;
                    }
                } else {
                    i += 1;
                }
            }
            Lex::Str => {
                if raw_hashes == 0 && c == '\\' {
                    i += 2;
                } else if is_str_close(chars, i, raw_hashes) {
                    i += 1 + raw_hashes + 1;
                    state = Lex::Code;
                } else {
                    i += 1;
                }
            }
        }
    }
    None
}

fn blank_range(chars: &[char], out: &mut Vec<char>, from: usize, to: usize) {
    for ch in &chars[from..=to] {
        out.push(if *ch == '\n' { '\n' } else { ' ' });
    }
}

/// 剥离 `#[cfg(test)]` 门控区域(字符置空、换行保留 → 行号与源文件一致)。
/// 返回 (生产段文本, 文件级测试模块名列表)。
fn strip_cfg_test_regions(src: &str) -> (String, Vec<String>) {
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut gated: Vec<String> = Vec::new();
    let mut i = 0usize;
    let mut state = Lex::Code;
    let mut block_depth = 0usize;
    let mut raw_hashes = 0usize;

    while i < chars.len() {
        let c = chars[i];
        match state {
            Lex::Code => {
                if starts_with(&chars, i, CFG_TEST_ATTR) {
                    out.resize(out.len() + CFG_TEST_ATTR.len(), ' ');
                    i += CFG_TEST_ATTR.len();
                    // 属性与项之间的空白/注释原样输出
                    loop {
                        match chars.get(i) {
                            Some(cc) if cc.is_whitespace() => {
                                out.push(*cc);
                                i += 1;
                            }
                            Some('/') if chars.get(i + 1) == Some(&'/') => {
                                while i < chars.len() {
                                    let nl = chars[i] == '\n';
                                    out.push(chars[i]);
                                    i += 1;
                                    if nl {
                                        break;
                                    }
                                }
                            }
                            Some('/') if chars.get(i + 1) == Some(&'*') => {
                                out.push('/');
                                out.push('*');
                                i += 2;
                                while i < chars.len() {
                                    out.push(chars[i]);
                                    let closed = chars[i] == '*' && chars.get(i + 1) == Some(&'/');
                                    i += if closed { 2 } else { 1 };
                                    if closed {
                                        break;
                                    }
                                }
                            }
                            _ => break,
                        }
                    }
                    if starts_with(&chars, i, "mod")
                        && !chars
                            .get(i + 3)
                            .is_some_and(|ch| ch.is_alphanumeric() || *ch == '_')
                    {
                        let mut j = i + 3;
                        while chars.get(j).is_some_and(|ch| ch.is_whitespace()) {
                            j += 1;
                        }
                        let name_start = j;
                        while chars
                            .get(j)
                            .is_some_and(|ch| ch.is_alphanumeric() || *ch == '_')
                        {
                            j += 1;
                        }
                        let name: String = chars[name_start..j].iter().collect();
                        let mut k = j;
                        while chars.get(k).is_some_and(|ch| ch.is_whitespace()) {
                            k += 1;
                        }
                        if !name.is_empty() && chars.get(k) == Some(&'{') {
                            if let Some(close) = find_matching_brace(&chars, k) {
                                blank_range(&chars, &mut out, i, close);
                                i = close + 1;
                                continue;
                            }
                        } else if !name.is_empty() && chars.get(k) == Some(&';') {
                            gated.push(name);
                        }
                    }
                    // 其余形态(use 等)原样放行
                } else {
                    match c {
                        '/' if chars.get(i + 1) == Some(&'/') => {
                            state = Lex::LineComment;
                            out.push(c);
                            out.push('/');
                            i += 2;
                        }
                        '/' if chars.get(i + 1) == Some(&'*') => {
                            state = Lex::BlockComment;
                            block_depth = 1;
                            out.push(c);
                            out.push('*');
                            i += 2;
                        }
                        '"' => {
                            raw_hashes = 0;
                            state = Lex::Str;
                            out.push(c);
                            i += 1;
                        }
                        'r' if raw_string_hashes(&chars, i).is_some() => {
                            raw_hashes = raw_string_hashes(&chars, i).unwrap_or(0);
                            state = Lex::Str;
                            out.push(c);
                            out.resize(out.len() + raw_hashes, '#');
                            out.push('"');
                            i += 1 + raw_hashes + 1;
                        }
                        '\'' => match char_literal_end(&chars, i) {
                            Some(end) => {
                                while i <= end {
                                    out.push(chars[i]);
                                    i += 1;
                                }
                            }
                            None => {
                                out.push(c);
                                i += 1;
                            }
                        },
                        _ => {
                            out.push(c);
                            i += 1;
                        }
                    }
                }
            }
            Lex::LineComment => {
                out.push(c);
                if c == '\n' {
                    state = Lex::Code;
                }
                i += 1;
            }
            Lex::BlockComment => {
                out.push(c);
                if c == '/' && chars.get(i + 1) == Some(&'*') {
                    block_depth += 1;
                    out.push('*');
                    out.push('/');
                    i += 2;
                } else if c == '*' && chars.get(i + 1) == Some(&'/') {
                    block_depth = block_depth.saturating_sub(1);
                    out.push('*');
                    out.push('/');
                    if block_depth == 0 {
                        state = Lex::Code;
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Lex::Str => {
                out.push(c);
                if raw_hashes == 0 && c == '\\' {
                    if let Some(next) = chars.get(i + 1) {
                        out.push(*next);
                    }
                    i += 2;
                } else if is_str_close(&chars, i, raw_hashes) {
                    out.resize(out.len() + raw_hashes, '#');
                    out.push('"');
                    i += 1 + raw_hashes + 1;
                    state = Lex::Code;
                } else {
                    i += 1;
                }
            }
        }
    }
    (out.into_iter().collect(), gated)
}

/// 收集文件级 `#[cfg(test)] mod 名字;` 声明。
fn file_level_test_mods(src: &str) -> Vec<String> {
    let mut mods = Vec::new();
    let mut rest = src;
    while let Some(pos) = rest.find(CFG_TEST_ATTR) {
        let after_attr = &rest[pos + CFG_TEST_ATTR.len()..];
        let trimmed = after_attr.trim_start();
        if let Some(after_mod) = trimmed.strip_prefix("mod ") {
            let ident: String = after_mod
                .chars()
                .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
                .collect();
            if !ident.is_empty() && after_mod[ident.len()..].trim_start().starts_with(';') {
                mods.push(ident);
            }
        }
        rest = after_attr;
    }
    mods
}

/// 生产段文本中的调用命中(行号, trim 后行内容)。
fn call_sites_in(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.contains(NEEDLE))
        .map(|(idx, line)| (idx + 1, line.trim().to_string()))
        .collect()
}

fn to_rel(repo_root: &Path, p: &Path) -> String {
    p.strip_prefix(repo_root)
        .unwrap_or(p)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().replace('\\', "/"))
        .collect::<Vec<_>>()
        .join("/")
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            collect_rs_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

// ---------------------------------------------------------------------------
// 门禁主测试
// ---------------------------------------------------------------------------

/// TC-GATE-GPU-01:`render_with_recovery` 至少一个生产调用点(非定义文件、非测试区)。
#[test]
fn tc_gate_gpu_01_render_with_recovery_has_production_call_site() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .join("..")
        .join("..")
        .canonicalize()
        .expect("仓库根目录必须存在");
    let crates_dir = repo_root.join("crates");

    let mut crate_dirs: Vec<PathBuf> = fs::read_dir(&crates_dir)
        .expect("crates/ 目录必须存在")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("src").is_dir())
        .collect();
    crate_dirs.sort();

    let mut definition_hits: Vec<String> = Vec::new();
    let mut production_callers: Vec<String> = Vec::new();
    let mut scanned_files = 0usize;

    for crate_dir in &crate_dirs {
        let src_dir = crate_dir.join("src");
        let mut files = Vec::new();
        collect_rs_files(&src_dir, &mut files);

        // Pass A:文件级测试模块 → 整体跳过集合
        let mut skip_files: Vec<PathBuf> = Vec::new();
        for f in &files {
            let Ok(text) = fs::read_to_string(f) else {
                continue;
            };
            for name in file_level_test_mods(&text) {
                let single = f.parent().unwrap_or(&src_dir).join(format!("{name}.rs"));
                if single.exists() {
                    skip_files.push(single);
                }
            }
        }

        // Pass B:剥离内联 #[cfg(test)] 区域后按调用语法针扫描
        for f in &files {
            if skip_files.iter().any(|s| s == f) {
                continue;
            }
            let Ok(text) = fs::read_to_string(f) else {
                continue;
            };
            scanned_files += 1;
            let (production, _gated) = strip_cfg_test_regions(&text);
            let rel = to_rel(&repo_root, f);
            let hits: Vec<(usize, String)> = call_sites_in(&production);
            if hits.is_empty() {
                continue;
            }
            let rendered: Vec<String> = hits
                .iter()
                .map(|(line, text)| format!("  {rel}:{line}: {text}"))
                .collect();
            if rel == DEFINITION_FILE {
                definition_hits.extend(rendered);
            } else {
                production_callers.extend(rendered);
            }
        }
    }

    // 扫描器有效性:定义必须存在于定义文件,否则门禁自身失真(防空转)。
    assert!(
        !definition_hits.is_empty(),
        "TC-GATE-GPU-01 门禁自检失败:{DEFINITION_FILE} 内未见 `{NEEDLE}` \
         ——状态机定义被移走?请同步更新本门禁的 DEFINITION_FILE。"
    );
    // 门禁本体:定义文件之外必须有生产调用点。
    assert!(
        !production_callers.is_empty(),
        "TC-GATE-GPU-01 红线:扫过 {scanned_files} 个生产文件,`{NEEDLE}` 除定义文件 \
         ({DEFINITION_FILE})外零调用点——RBT-01 的接线被拆除了。\n\
         定义文件命中(仅供核对):\n{}\n\
         修复:恢复生产调用方(如 GpuFrameRenderer::render_frame 经 \
         GpuGuard::render_with_recovery 包装帧闭包)。",
        definition_hits.join("\n"),
    );
    println!(
        "TC-GATE-GPU-01 通过:扫过 {scanned_files} 个生产文件,生产调用点 {} 处:\n{}",
        production_callers.len(),
        production_callers.join("\n"),
    );
}

// ---------------------------------------------------------------------------
// 扫描器反面单测(纯函数,不碰文件系统)
// ---------------------------------------------------------------------------

#[test]
fn scanner_docs_mention_without_paren_do_not_count() {
    // 文档/注释提及(无括号)不算调用。
    let src = "//! 每帧入口经 render_with_recovery 包装\npub fn f() {}\n";
    let (production, _) = strip_cfg_test_regions(src);
    assert!(call_sites_in(&production).is_empty());
}

#[test]
fn scanner_call_inside_cfg_test_region_does_not_count() {
    let src = concat!(
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    fn t() { guard.render_with_recovery(&adapter, |_| Ok(())); }\n",
        "}\n",
        "pub fn g() {}\n",
    );
    let (production, _) = strip_cfg_test_regions(src);
    assert!(
        !production.contains("render_with_recovery"),
        "测试区必须被剥离"
    );
    assert!(call_sites_in(&production).is_empty());
}

#[test]
fn scanner_production_call_counts_and_line_numbers_align() {
    let src = concat!(
        "fn helper() {}\n",
        "fn frame() {\n",
        "    guard.render_with_recovery(&adapter, build);\n",
        "}\n",
    );
    let (production, _) = strip_cfg_test_regions(src);
    let hits = call_sites_in(&production);
    assert_eq!(hits.len(), 1, "生产调用必须命中,实际 {hits:?}");
    assert_eq!(hits[0].0, 3, "行号必须与源文件一致");
    assert!(hits[0].1.contains("render_with_recovery("));
}

#[test]
fn scanner_file_level_test_mod_file_is_skipped() {
    let src = "#![deny(unsafe_code)]\n\n#[cfg(test)]\nmod tests;\n\npub fn f() {}\n";
    assert_eq!(file_level_test_mods(src), vec!["tests".to_string()]);
    let src_inline = "#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n";
    assert!(
        file_level_test_mods(src_inline).is_empty(),
        "内联形态不算文件级"
    );
}

#[test]
fn scanner_braces_in_literals_do_not_confuse_region_matching() {
    // 所有花括号只出现在字面量里(字符串/字符/块注释/原始字符串);
    // 区域配对必须穿过它们,在真正的收尾花括号处闭合。
    let src = concat!(
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    fn t() {\n",
        "        let open = \"{\", close = \"}\";\n",
        "        let c = '{'; /* } */\n",
        "        let r = r#\"}\"#;\n",
        "        guard.render_with_recovery(&a, |_, _| Ok(()));\n",
        "    }\n",
        "}\n",
        "pub fn g() { guard.render_with_recovery(&a, build); }\n",
    );
    let (production, _) = strip_cfg_test_regions(src);
    assert!(
        production.contains("pub fn g()"),
        "区域必须正确闭合:\n{production}"
    );
    let hits = call_sites_in(&production);
    assert_eq!(hits.len(), 1, "只许命中生产区那一处,实际 {hits:?}");
    assert!(hits[0].1.contains("guard.render_with_recovery("));
}
