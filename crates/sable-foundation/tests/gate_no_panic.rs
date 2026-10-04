//! TC-GATE-PANIC-01(迭代审查报告 RBT-05 / §6 RB-01)静态门禁:
//! `crates/*/src/**/*.rs` 的**生产段**不得出现 `unwrap(`/`expect(`/`panic!`/
//! `unreachable!`/`unimplemented!`/`todo!`,命中即红。
//! 豁免走同目录 `gate_no_panic_allowlist.txt`,每行
//! `路径:内容前缀 # 理由`(路径为仓库相对路径,`/` 分隔;内容前缀与
//! 违规行 trim 后做前缀匹配;前缀内不得含 ` # `)。
//!
//! ## 启发式说明(实现约定;误伤走 allowlist,漏报靠 code review 兜底)
//! 1. **`#[cfg(test)]` 区域剥离**:从属性处起,其后的内联项
//!    (`#[cfg(test)] mod tests { … }`)做花括号配对到区域闭合,整段视为
//!    测试代码剥离。配对会跳过行注释/块注释/字符串字面量(含原始字符串
//!    `r#"…"#`、字符字面量 `'{'`),避免字面量里的花括号干扰配对;被剥离
//!    字符替换为空格、换行保留,因此违规行号与源文件行号一致。
//! 2. **文件级测试模块**:任一文件出现 `#[cfg(test)] mod 名字;` 时,该子模块
//!    对应的 `名字.rs` / `名字/mod.rs`(存在 `名字/` 目录时整个子树)整体
//!    视为测试代码跳过(例:sable-script 的 `src/tests.rs`)。
//! 3. **按行扫描**:命中判定要求违禁子串的前一个字符不是标识符字符
//!    (字母/数字/`_`),因此 `unwrap_or(`/`unwrap_or_else(`/`expected(`
//!    不误报;注释里的示例代码**不豁免**——生产注释不示范 panic 用法。
//!    已知取舍:`expect_err`/`unwrap_err`(反向 expect,Ok 时 panic)不在
//!    本轮违禁表内,与报告 TC-GATE-PANIC-01 验收口径保持一致。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// 违禁调用(与报告 TC-GATE-PANIC-01 验收口径一致)。
const NEEDLES: &[&str] = &[
    "unwrap(",
    "expect(",
    "panic!",
    "unreachable!",
    "unimplemented!",
    "todo!",
];

const CFG_TEST_ATTR: &str = "#[cfg(test)]";
/// allowlist 与本测试同目录。
const ALLOWLIST_FILE: &str = "gate_no_panic_allowlist.txt";

// ---------------------------------------------------------------------------
// 词法状态(区域剥离与花括号配对共用同一套规则)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lex {
    Code,
    LineComment,
    BlockComment,
    Str,
}

/// `chars[i..]` 是否以 `s` 开头。
fn starts_with(chars: &[char], i: usize, s: &str) -> bool {
    s.chars()
        .enumerate()
        .all(|(k, c)| chars.get(i + k) == Some(&c))
}

/// `chars[i]` 是 `r`:是否为原始字符串字面量起点(`r"` / `r#"…"#`),返回井号数。
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
/// 否则视为生命周期(`'a` / `'static`)返回 `None`。
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

/// `chars[i]` 是否是字符串(原始/普通)在 `hashes` 口味下的收尾引号。
fn is_str_close(chars: &[char], i: usize, hashes: usize) -> bool {
    if chars.get(i) != Some(&'"') {
        return false;
    }
    (1..=hashes).all(|k| chars.get(i + k) == Some(&'#'))
}

/// `chars[open]` 是 Code 态下的 `{`:返回配对 `}` 的下标。
/// 与主剥离循环共用同一套注释/字符串状态规则,避免字面量里的花括号干扰配对。
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

// ---------------------------------------------------------------------------
// 区域剥离
// ---------------------------------------------------------------------------

/// 剥离 `#[cfg(test)]` 门控区域:被剥离字符替换为空格、换行保留,
/// 因此返回文本的行号与源文件一致。
/// 返回 (生产段源码, 文件级测试模块名列表)。
fn strip_cfg_test_regions(src: &str) -> (String, Vec<String>) {
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut gated: Vec<String> = Vec::new();
    // 不变量:主循环每消耗一个输入字符,恰向 out 推一个字符(原文或占位)。
    let mut i = 0usize;
    let mut state = Lex::Code;
    let mut block_depth = 0usize;
    let mut raw_hashes = 0usize;

    while i < chars.len() {
        let c = chars[i];
        match state {
            Lex::Code => {
                if starts_with(&chars, i, CFG_TEST_ATTR) {
                    // 1) 属性本身置空(无换行)
                    out.resize(out.len() + CFG_TEST_ATTR.len(), ' ');
                    i += CFG_TEST_ATTR.len();
                    // 2) 原样输出属性与项之间的空白/注释
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
                    // 3) 判断属性后的形态
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
                            // `#[cfg(test)] mod 名字 { … }`:整段区域置空
                            if let Some(close) = find_matching_brace(&chars, k) {
                                blank_range(&chars, &mut out, i, close);
                                i = close + 1;
                                continue;
                            }
                        } else if !name.is_empty() && chars.get(k) == Some(&';') {
                            // `#[cfg(test)] mod 名字;`:文件级测试模块声明
                            gated.push(name);
                        }
                    } else if chars.get(i) == Some(&'{') {
                        // `#[cfg(test)] { … }`(罕见形态):整段区域置空
                        if let Some(close) = find_matching_brace(&chars, i) {
                            blank_range(&chars, &mut out, i, close);
                            i = close + 1;
                            continue;
                        }
                    }
                    // 其余形态(use 语句等)原样放行,交回主循环逐字符处理
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
                                // 生命周期:原样放行单个 '
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
                    i += 2;
                } else if c == '*' && chars.get(i + 1) == Some(&'/') {
                    block_depth = block_depth.saturating_sub(1);
                    out.push('/');
                    i += 2;
                    if block_depth == 0 {
                        state = Lex::Code;
                    }
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

/// 把 `chars[from..=to]` 追加到 out(换行保留,其余置空),维持行号对齐。
fn blank_range(chars: &[char], out: &mut Vec<char>, from: usize, to: usize) {
    for ch in &chars[from..=to] {
        out.push(if *ch == '\n' { '\n' } else { ' ' });
    }
}

/// 收集文件级 `#[cfg(test)] mod 名字;` 声明(启发式:字符串搜索属性字面量)。
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
            let after_ident = &after_mod[ident.len()..];
            if !ident.is_empty() && after_ident.trim_start().starts_with(';') {
                mods.push(ident);
            }
        }
        rest = after_attr;
    }
    mods
}

// ---------------------------------------------------------------------------
// 违禁调用扫描 + allowlist
// ---------------------------------------------------------------------------

/// 纯函数:单行(不 trim)是否命中任一违禁子串。
/// 前一字符为标识符字符(字母/数字/`_`)时不命中,故 `unwrap_or(` 不误报。
fn line_hits(line: &str) -> bool {
    NEEDLES.iter().any(|needle| hits_needle(line, needle))
}

fn hits_needle(line: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = line[from..].find(needle) {
        let at = from + rel;
        let boundary = line[..at]
            .chars()
            .next_back()
            .is_none_or(|ch| !(ch.is_alphanumeric() || ch == '_'));
        if boundary {
            return true;
        }
        from = at + 1;
    }
    false
}

/// 纯函数:生产段文本 → (行号, trim 后的行内容) 违规列表。
fn violations_in_text(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line_hits(line))
        .map(|(idx, line)| (idx + 1, line.trim().to_string()))
        .collect()
}

/// allowlist 条目:(仓库相对路径, 内容前缀, 理由)。
fn parse_allowlist(raw: &str) -> Vec<(String, String, String)> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let (left, reason) = line.rsplit_once(" # ")?;
            let (file, prefix) = left.split_once(':')?;
            Some((
                file.trim().to_string(),
                prefix.trim().to_string(),
                reason.trim().to_string(),
            ))
        })
        .collect()
}

/// 违规是否被 allowlist 豁免:路径相等且违规行(trim 后)以条目前缀开头。
fn exempted<'a>(
    rel_path: &str,
    line: &str,
    allowlist: &'a [(String, String, String)],
) -> Option<&'a str> {
    allowlist
        .iter()
        .find(|(file, prefix, _)| file == rel_path && line.starts_with(prefix.as_str()))
        .map(|(_, _, reason)| reason.as_str())
}

/// 路径 → 仓库相对路径(`/` 分隔)。
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

#[test]
fn tc_gate_panic_01_production_sources_contain_no_panic_calls() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .join("..")
        .join("..")
        .canonicalize()
        .expect("仓库根目录(D:/Github/sable-ui)必须存在");
    let crates_dir = repo_root.join("crates");

    let allowlist_raw = fs::read_to_string(manifest_dir.join("tests").join(ALLOWLIST_FILE))
        .unwrap_or_else(|_| {
            // 测试基建缺文件视为配置错误,直接给红
            panic!("缺少 allowlist 文件:crates/sable-foundation/tests/{ALLOWLIST_FILE}");
        });
    let allowlist = parse_allowlist(&allowlist_raw);

    let mut crate_dirs: Vec<PathBuf> = fs::read_dir(&crates_dir)
        .expect("crates/ 目录必须存在")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("src").is_dir())
        .collect();
    crate_dirs.sort();

    let mut violations: Vec<String> = Vec::new();
    let mut scanned_files = 0usize;

    for crate_dir in &crate_dirs {
        let src_dir = crate_dir.join("src");
        let mut files = Vec::new();
        collect_rs_files(&src_dir, &mut files);

        // Pass A:文件级测试模块 → 计算整体跳过集合
        let mut skip_files: BTreeSet<PathBuf> = BTreeSet::new();
        let mut skip_dirs: BTreeSet<PathBuf> = BTreeSet::new();
        for f in &files {
            let Ok(text) = fs::read_to_string(f) else {
                continue;
            };
            for name in file_level_test_mods(&text) {
                let dir = f.parent().unwrap_or(&src_dir).to_path_buf();
                let subtree = dir.join(&name);
                if subtree.is_dir() {
                    skip_dirs.insert(subtree);
                } else {
                    let single = dir.join(format!("{name}.rs"));
                    if single.exists() {
                        skip_files.insert(single);
                    }
                }
            }
        }

        // Pass B:逐文件剥离 #[cfg(test)] 区域后扫描
        for f in &files {
            if skip_files.contains(f) || skip_dirs.iter().any(|d| f.starts_with(d)) {
                continue;
            }
            let Ok(text) = fs::read_to_string(f) else {
                continue;
            };
            scanned_files += 1;
            let (production, _gated) = strip_cfg_test_regions(&text);
            let rel = to_rel(&repo_root, f);
            for (line_no, line) in violations_in_text(&production) {
                match exempted(&rel, &line, &allowlist) {
                    Some(reason) => {
                        println!("  豁免 {rel}:{line_no}(理由:{reason})");
                    }
                    None => violations.push(format!("{rel}:{line_no}: {line}")),
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "TC-GATE-PANIC-01 红线:crates/*/src 生产段出现违禁调用 \
         (unwrap/expect/panic!/unreachable!/unimplemented!/todo!)共 {} 处:\n{}\n\
         清偿方式:改为结构化错误传播;确需豁免的,在 tests/{ALLOWLIST_FILE} \
         写明 `路径:内容前缀 # 理由`(须对应审查报告条目)。",
        violations.len(),
        violations.join("\n"),
    );
    println!("TC-GATE-PANIC-01 通过:扫描 {scanned_files} 个生产文件,生产区违规 0 处。");
}

// ---------------------------------------------------------------------------
// 扫描器反面单测(纯函数,不碰文件系统)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod scanner_tests {
    use super::*;

    /// 反面样本(报告硬性要求):含 `let x: Option<u8> = None; x.expect("boom")`
    /// 的字符串必须命中。
    #[test]
    fn negative_sample_expect_must_hit() {
        let src =
            "fn main() {\n    let x: Option<u8> = None;\n    let _ = x.expect(\"boom\");\n}\n";
        let (production, gated) = strip_cfg_test_regions(src);
        assert!(gated.is_empty(), "无文件级测试模块声明");
        let hits = violations_in_text(&production);
        assert_eq!(hits.len(), 1, "反面样本:expect 必须命中,实际 {hits:?}");
        assert!(hits[0].1.starts_with("let _ = x.expect("));
        assert_eq!(hits[0].0, 3, "行号必须与源文件一致");
    }

    /// 干净代码必须通过(unwrap_or 系与注释里的 `expected(` 不误报)。
    #[test]
    fn clean_code_must_pass() {
        let src = concat!(
            "pub fn f(v: Option<u8>, s: Result<u8, ()>) -> u8 {\n",
            "    let a = v.unwrap_or(0);\n",
            "    let b = s.unwrap_or_else(|_| 1);\n",
            "    // 注释:expected( 一类派生词不在违禁表,不误报\n",
            "    let err = s.err().unwrap_or(Some(2).unwrap_or(3));\n",
            "    a + b + err\n",
            "}\n",
        );
        let (production, gated) = strip_cfg_test_regions(src);
        assert!(gated.is_empty());
        let hits = violations_in_text(&production);
        assert!(hits.is_empty(), "干净代码不许命中,实际 {hits:?}");
    }

    /// `#[cfg(test)]` 内联区域内的 expect 必须被正确豁免;
    /// 且区域关闭后的生产代码仍要被扫到(证明花括号配对准确)。
    #[test]
    fn cfg_test_region_is_exempted_but_code_after_it_is_scanned() {
        let src = concat!(
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    #[test]\n",
            "    fn t() {\n",
            "        let x: Option<u8> = None;\n",
            "        x.expect(\"in test\"); // 测试区,豁免\n",
            "    }\n",
            "}\n",
            "\n",
            "pub fn g() {\n",
            "    let y: Option<u8> = None;\n",
            "    y.expect(\"in prod\"); // 生产区,必须命中\n",
            "}\n",
        );
        let (production, gated) = strip_cfg_test_regions(src);
        assert!(gated.is_empty());
        assert!(!production.contains("in test"), "测试区应被剥离");
        assert!(production.contains("in prod"), "生产区必须保留");
        let hits = violations_in_text(&production);
        assert_eq!(hits.len(), 1, "只许命中生产区那一处,实际 {hits:?}");
        assert!(hits[0].1.contains("in prod"));
        assert_eq!(hits[0].0, 12, "行号须对齐源文件");
    }

    /// 文件级声明 `#[cfg(test)] mod tests;` 必须被识别。
    #[test]
    fn file_level_test_mod_declaration_detected() {
        let src = "#![deny(unsafe_code)]\n\n#[cfg(test)]\nmod tests;\n\npub fn f() {}\n";
        assert_eq!(file_level_test_mods(src), vec!["tests".to_string()]);
        let src_inline = "#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n";
        assert!(
            file_level_test_mods(src_inline).is_empty(),
            "内联形态不算文件级"
        );
    }

    /// 字符串/注释/字符字面量里的花括号不得干扰区域配对。
    #[test]
    fn braces_in_literals_do_not_confuse_region_matching() {
        let src = concat!(
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    fn t() {\n",
            "        let s = \"}\",{ \n",
            "        let c = '{'; /* } */\n",
            "        let r = r#\"}\"#;\n",
            "    }\n",
            "}\n",
            "pub fn g() { let z: Option<u8> = None; z.expect(\"prod\"); }\n",
        );
        let (production, _gated) = strip_cfg_test_regions(src);
        assert!(
            production.contains("pub fn g()"),
            "区域必须在上面的花括号陷阱之后正确闭合\n--- 生产段 ---\n{production}"
        );
        assert_eq!(violations_in_text(&production).len(), 1);
    }

    /// panic!/unreachable!/unimplemented!/todo! 同样必须命中。
    #[test]
    fn panic_family_macros_are_detected() {
        let src = concat!(
            "fn a() { panic!(\"boom\"); }\n",
            "fn b() { unreachable!(); }\n",
            "fn c() { unimplemented!(); }\n",
            "fn d() { todo!(\"later\"); }\n",
            "fn e() { my_panic!(); } // 前缀是标识符 → 不误报\n",
        );
        let (production, _) = strip_cfg_test_regions(src);
        let hits = violations_in_text(&production);
        assert_eq!(hits.len(), 4, "四个宏必须全中、误报须为零,实际 {hits:?}");
    }

    /// allowlist:解析 + 前缀豁免 + 理由提取。
    #[test]
    fn allowlist_parses_and_exempts_by_prefix() {
        let raw = concat!(
            "# 注释行\n",
            "\n",
            "crates/sable-widgets/src/theme.rs:.expect(\"SableTheme # COUP-09 计划项,本轮豁免\n",
        );
        let entries = parse_allowlist(raw);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "crates/sable-widgets/src/theme.rs");
        assert_eq!(entries[0].2, "COUP-09 计划项,本轮豁免");
        let line = ".expect(\"SableTheme 未初始化:必须先 init\")";
        assert!(
            exempted("crates/sable-widgets/src/theme.rs", line, &entries).is_some(),
            "前缀匹配须豁免"
        );
        assert!(
            exempted(
                "crates/sable-widgets/src/theme.rs",
                ".unwrap_other(",
                &entries
            )
            .is_none(),
            "前缀不匹配不得豁免"
        );
        assert!(
            exempted("crates/other/src/x.rs", line, &entries).is_none(),
            "路径不同不得豁免"
        );
    }

    /// 生命周期 `&'a str` 不得被当成字符字面量吞掉后续花括号。
    #[test]
    fn lifetimes_do_not_break_lexing() {
        let src = concat!(
            "fn f<'a>(s: &'a str) -> &'a str { s }\n",
            "#[cfg(test)]\n",
            "mod tests { fn t() { panic!(\"x\"); } }\n",
            "pub fn g() {}\n",
        );
        let (production, gated) = strip_cfg_test_regions(src);
        assert!(gated.is_empty(), "内联 mod 不产生文件级声明");
        assert!(
            !production.contains("panic!"),
            "生命周期之后,测试区域仍须被正确剥离:\n{production}"
        );
        assert!(
            production.contains("pub fn g()"),
            "生产段必须保留:\n{production}"
        );
        assert_eq!(violations_in_text(&production).len(), 0);
    }
}
