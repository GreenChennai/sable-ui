//! G-UI-A / TOK-08:令牌 JSON 真相源 ↔ `tokens.rs` 逐值同步门禁
//! (迭代审查报告 2026-10-04 §5.3 原则 + §5.11 G-UI-A;TC-GATE-TOKENS-01 /
//! TC-ANI-SPRING-01)。
//!
//! # 真相与方向
//!
//! - **代码真相**:`crates/sable-widgets/src/tokens.rs`(运行时取值);
//! - **设计真相**:`<仓库根>/docs/design/sable-tokens.json`(W3C
//!   design-tokens 格式,本测试内置零依赖解析器读取);
//! - 比对**以代码侧装配期望、JSON 侧逐值对拍**:任一侧单改一个值而不同步,
//!   本门禁即红。反面测试用**内联字符串变换**证明比较器会红(不写临时文件)。
//!
//! # 扫描面(显式化,GATE-03 纪律③)
//!
//! - 仅比对 JSON 中的 `color`(深/浅全量 19 键)、`spacing`(6)、`radius`(4)、
//!   `elevation`(L0~L4:blur/offsetY/alpha/color)、`state-layer`(深/浅 × 5)、
//!   `motion.duration`(4 档)、`motion.spring`(3 档 × 3 参数)、`typography`
//!   (font-family 2 + text-size 7 × {size,lineHeight,weight},TOK-02);
//! - 顶层键集合也受检:出现七表之外未落地的表 → 红;
//! - 颜色以 `#RRGGBBAA` 字符串入 JSON,经 gpui 的 `rgba → Hsla` 同一转换
//!   与代码字面量逐位相等比对(与 tokens.rs 的构造路径一致,零浮点误差)。
//!
//! # 已知边界
//!
//! - JSON 数值按 f64 解析,与代码 f32 比对容差 1e-6(数量级 ≤ 600 的令牌
//!   值下远小于任何可感知差异;弹簧/时长为 f64 对 f64,精确相等);
//! - 解析器只支持本文件用到的 JSON 子集(对象/字符串/数字,不含数组、
//!   null、布尔),解析失败本身即红。

use std::fs;
use std::path::{Path, PathBuf};

use gpui::Hsla;
use sable_widgets::tokens::{
    ColorTokens, ELEVATION_SHADOW_TINT, ELEVATIONS, Elevation, MONO_FONT, MotionTokens,
    RadiusTokens, SPRING_BOUNCY, SPRING_SNAPPY, SPRING_SOFT, SpacingTokens, StateLayerTokens,
    TEXT_SIZES, UI_FONT, rgba8_from_hsla,
};

// ---------------------------------------------------------------------------
// 零依赖 JSON 子集解析器(对象/字符串/数字)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Json {
    Obj(Vec<(String, Json)>),
    Str(String),
    Num(f64),
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn ws(&mut self) {
        while self.b.get(self.i).is_some_and(u8::is_ascii_whitespace) {
            self.i += 1;
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        match self.b.get(self.i) {
            Some(b'{') => self.object(),
            Some(b'"') => self.string().map(Json::Str),
            Some(c) if *c == b'-' || c.is_ascii_digit() => self.number().map(Json::Num),
            Some(c) => Err(format!("偏移 {}:意外字符 '{}'", self.i, *c as char)),
            None => Err("意外结束(缺值)".to_string()),
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.i += 1; // '{'
        let mut pairs = Vec::new();
        self.ws();
        if self.b.get(self.i) == Some(&b'}') {
            self.i += 1;
            return Ok(Json::Obj(pairs));
        }
        loop {
            self.ws();
            if self.b.get(self.i) != Some(&b'"') {
                return Err(format!("偏移 {}:对象键必须是字符串", self.i));
            }
            let key = self.string()?;
            self.ws();
            if self.b.get(self.i) != Some(&b':') {
                return Err(format!("偏移 {}:缺少 ':'", self.i));
            }
            self.i += 1;
            let val = self.value()?;
            pairs.push((key, val));
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Json::Obj(pairs));
                }
                _ => return Err(format!("偏移 {}:对象缺少 ',' 或 '}}'", self.i)),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1; // 开引号
        let mut s = String::new();
        loop {
            match self.b.get(self.i) {
                Some(b'"') => {
                    self.i += 1;
                    return Ok(s);
                }
                Some(b'\\') => {
                    self.i += 1;
                    match self.b.get(self.i) {
                        Some(b'"') => s.push('"'),
                        Some(b'\\') => s.push('\\'),
                        Some(b'/') => s.push('/'),
                        Some(b'n') => s.push('\n'),
                        Some(b't') => s.push('\t'),
                        Some(b'u') => {
                            let end = self.i + 5;
                            if end > self.b.len() {
                                return Err("非法 \\u 转义".to_string());
                            }
                            let hex = std::str::from_utf8(&self.b[self.i + 1..end])
                                .map_err(|e| format!("非法 \\u 转义:{e}"))?;
                            let cp = u32::from_str_radix(hex, 16)
                                .map_err(|e| format!("非法 \\u 码点:{e}"))?;
                            s.push(char::from_u32(cp).ok_or("非法码点")?);
                            self.i += 4;
                        }
                        _ => return Err(format!("偏移 {}:非法转义", self.i)),
                    }
                    self.i += 1;
                }
                Some(_) => {
                    let rest = std::str::from_utf8(&self.b[self.i..])
                        .map_err(|e| format!("非法 UTF-8:{e}"))?;
                    let ch = rest.chars().next().ok_or("空字符串流")?;
                    s.push(ch);
                    self.i += ch.len_utf8();
                }
                None => return Err("字符串未闭合".to_string()),
            }
        }
    }

    fn number(&mut self) -> Result<f64, String> {
        let start = self.i;
        if self.b.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        while self.i < self.b.len()
            && (self.b[self.i].is_ascii_digit()
                || matches!(self.b[self.i], b'.' | b'e' | b'E' | b'+' | b'-'))
        {
            self.i += 1;
        }
        std::str::from_utf8(&self.b[start..self.i])
            .map_err(|e| format!("非法数字:{e}"))?
            .parse::<f64>()
            .map_err(|e| format!("偏移 {start}:非法数字:{e}"))
    }
}

fn parse_json(text: &str) -> Result<Json, String> {
    let mut p = Parser {
        b: text.as_bytes(),
        i: 0,
    };
    let v = p.value()?;
    p.ws();
    if p.i != p.b.len() {
        return Err(format!("偏移 {}:存在尾随内容", p.i));
    }
    Ok(v)
}

/// 取对象键(不存在即 Err,消息即门禁输出)。
fn get<'a>(j: &'a Json, key: &str) -> Result<&'a Json, String> {
    match j {
        Json::Obj(pairs) => pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
            .ok_or_else(|| format!("缺少键 `{key}`")),
        _ => Err(format!("取键 `{key}` 失败:不是对象")),
    }
}

/// 叶子取值:W3C 形态 `{ "$value": X }` 下钻到 X;裸值原样返回。
fn leaf(j: &Json) -> Result<&Json, String> {
    match j {
        Json::Obj(pairs) if pairs.iter().any(|(k, _)| k == "$value") => get(j, "$value"),
        other => Ok(other),
    }
}

fn as_num(j: &Json) -> Result<f64, String> {
    match j {
        Json::Num(n) => Ok(*n),
        _ => Err(format!("应为数字,得到 {j:?}")),
    }
}

fn as_str(j: &Json) -> Result<&str, String> {
    match j {
        Json::Str(s) => Ok(s),
        _ => Err(format!("应为字符串,得到 {j:?}")),
    }
}

/// W3C 颜色叶 → Hsla:与 tokens.rs 字面量同一构造路径(gpui rgba→Hsla),
/// 因此逐位相等比较成立。
fn hsla_of(j: &Json) -> Result<Hsla, String> {
    let s = as_str(j)?;
    let hex = s
        .strip_prefix('#')
        .ok_or_else(|| format!("颜色缺 '#':{s}"))?;
    if hex.len() != 8 {
        return Err(format!("颜色须为 #RRGGBBAA,得到 {s}"));
    }
    let v = u32::from_str_radix(hex, 16).map_err(|e| format!("非法颜色 {s}:{e}"))?;
    Ok(gpui::rgba(v).into())
}

// ---------------------------------------------------------------------------
// 期望侧:从代码真相(tokens.rs)装配
// ---------------------------------------------------------------------------

/// 代码真相快照(与 tokens.rs 字段一一对应;反面测试单独改字段模拟
/// "改了 tokens.rs 未同步 JSON")。
struct Expected {
    dark: Vec<(&'static str, Hsla)>,
    light: Vec<(&'static str, Hsla)>,
    spacing: Vec<(&'static str, f32)>,
    radius: Vec<(&'static str, f32)>,
    elevations: Vec<(&'static str, Elevation)>,
    shadow_hex: String,
    state_dark: Vec<(&'static str, f32)>,
    state_light: Vec<(&'static str, f32)>,
    durations: Vec<(&'static str, f64)>,
    springs: Vec<(&'static str, [f64; 3])>,
    fonts: Vec<(&'static str, &'static str)>,
    text_sizes: Vec<(&'static str, [f64; 3])>,
}

impl Expected {
    fn from_code() -> Self {
        fn color_pairs(t: &ColorTokens) -> Vec<(&'static str, Hsla)> {
            vec![
                ("surface-0", t.surface_0),
                ("surface-1", t.surface_1),
                ("surface-2", t.surface_2),
                ("surface-3", t.surface_3),
                ("surface-4", t.surface_4),
                ("border-subtle", t.border_subtle),
                ("border-strong", t.border_strong),
                ("text-strong", t.text_strong),
                ("text-primary", t.text_primary),
                ("text-secondary", t.text_secondary),
                ("text-tertiary", t.text_tertiary),
                ("text-disabled", t.text_disabled),
                ("text-placeholder", t.text_placeholder),
                ("accent", t.accent),
                ("accent-muted", t.accent_muted),
                ("danger", t.danger),
                ("warning", t.warning),
                ("success", t.success),
                ("info", t.info),
            ]
        }
        fn state_pairs(s: StateLayerTokens) -> Vec<(&'static str, f32)> {
            vec![
                ("hover", s.hover),
                ("press", s.press),
                ("selected", s.selected),
                ("focus-ring", s.focus_ring),
                ("disabled", s.disabled),
            ]
        }
        let shadow = rgba8_from_hsla(ELEVATION_SHADOW_TINT);
        Expected {
            dark: color_pairs(&ColorTokens::dark()),
            light: color_pairs(&ColorTokens::light()),
            spacing: vec![
                ("xs", SpacingTokens::XS),
                ("sm", SpacingTokens::SM),
                ("md", SpacingTokens::MD),
                ("lg", SpacingTokens::LG),
                ("xl", SpacingTokens::XL),
                ("xxl", SpacingTokens::XXL),
            ],
            radius: vec![
                ("sm", RadiusTokens::SM),
                ("md", RadiusTokens::MD),
                ("lg", RadiusTokens::LG),
                ("xl", RadiusTokens::XL),
            ],
            elevations: vec![
                ("l0", ELEVATIONS[0]),
                ("l1", ELEVATIONS[1]),
                ("l2", ELEVATIONS[2]),
                ("l3", ELEVATIONS[3]),
                ("l4", ELEVATIONS[4]),
            ],
            shadow_hex: format!(
                "#{:02X}{:02X}{:02X}{:02X}",
                shadow[0], shadow[1], shadow[2], shadow[3]
            ),
            state_dark: state_pairs(StateLayerTokens::dark()),
            state_light: state_pairs(StateLayerTokens::light()),
            durations: vec![
                ("instant", MotionTokens::DUR_INSTANT_MS),
                ("hover", MotionTokens::DUR_HOVER_MS),
                ("state", MotionTokens::DUR_STATE_MS),
                ("panel", MotionTokens::DUR_PANEL_MS),
            ],
            springs: vec![
                (
                    "snappy",
                    [
                        SPRING_SNAPPY.stiffness,
                        SPRING_SNAPPY.damping,
                        SPRING_SNAPPY.mass,
                    ],
                ),
                (
                    "soft",
                    [SPRING_SOFT.stiffness, SPRING_SOFT.damping, SPRING_SOFT.mass],
                ),
                (
                    "bouncy",
                    [
                        SPRING_BOUNCY.stiffness,
                        SPRING_BOUNCY.damping,
                        SPRING_BOUNCY.mass,
                    ],
                ),
            ],
            fonts: vec![("ui", UI_FONT), ("mono", MONO_FONT)],
            text_sizes: TEXT_SIZES
                .iter()
                .map(|(name, ts)| {
                    (
                        *name,
                        [
                            f64::from(ts.size),
                            f64::from(ts.line_height),
                            f64::from(ts.weight),
                        ],
                    )
                })
                .collect(),
        }
    }
}

/// 全量比对(纯函数):返回违例清单,空 = 同步。
fn compare_expected(exp: &Expected, json_text: &str) -> Vec<String> {
    let mut bad = Vec::new();
    let json = match parse_json(json_text) {
        Ok(j) => j,
        Err(e) => {
            bad.push(format!("JSON 解析失败:{e}"));
            return bad;
        }
    };

    // 顶层键集合:七表 + $ 元键;未落地的表出现即红
    if let Json::Obj(pairs) = &json {
        let allowed = [
            "color",
            "spacing",
            "radius",
            "elevation",
            "state-layer",
            "motion",
            "typography",
        ];
        for (k, _) in pairs {
            if k.starts_with('$') {
                continue;
            }
            if !allowed.contains(&k.as_str()) {
                bad.push(format!(
                    "顶层出现未落地表 `{k}`(TOK-08/TOK-02 本轮共七表;新表须连门禁一起落)"
                ));
            }
        }
    }

    // color:深/浅全量,键集合必须完全一致(多键/缺键都红)
    for (theme, pairs) in [("dark", &exp.dark), ("light", &exp.light)] {
        let path = format!("color.{theme}");
        let group = match get(&json, "color").and_then(|g| get(g, theme)) {
            Ok(g) => g,
            Err(e) => {
                bad.push(format!("{path}:{e}"));
                continue;
            }
        };
        if let Json::Obj(json_pairs) = group {
            let mut json_keys: Vec<&str> = json_pairs
                .iter()
                .map(|(k, _)| k.as_str())
                .filter(|k| !k.starts_with('$'))
                .collect();
            json_keys.sort_unstable();
            let mut want: Vec<&str> = pairs.iter().map(|(k, _)| *k).collect();
            want.sort_unstable();
            if json_keys != want {
                bad.push(format!(
                    "{path}:键集合不一致\n  JSON 有而代码无:{:?}\n  代码有而 JSON 无:{:?}",
                    json_keys
                        .iter()
                        .filter(|k| !want.contains(k))
                        .collect::<Vec<_>>(),
                    want.iter()
                        .filter(|k| !json_keys.contains(k))
                        .collect::<Vec<_>>(),
                ));
            }
        }
        for (key, code_color) in pairs {
            let at = format!("{path}.{key}");
            match get(group, key).and_then(leaf).and_then(hsla_of) {
                Ok(json_color) => {
                    if json_color != *code_color {
                        bad.push(format!(
                            "{at}:JSON {:?} != 代码 {:?}",
                            json_color, code_color
                        ));
                    }
                }
                Err(e) => bad.push(format!("{at}:{e}")),
            }
        }
    }

    // spacing / radius
    for (table, pairs) in [("spacing", &exp.spacing), ("radius", &exp.radius)] {
        let group = match get(&json, table) {
            Ok(g) => g,
            Err(e) => {
                bad.push(format!("{table}:{e}"));
                continue;
            }
        };
        for (key, code_v) in pairs {
            let at = format!("{table}.{key}");
            match get(group, key).and_then(leaf).and_then(as_num) {
                Ok(v) => {
                    if (v - f64::from(*code_v)).abs() > 1e-6 {
                        bad.push(format!("{at}:JSON {v} != 代码 {}", *code_v));
                    }
                }
                Err(e) => bad.push(format!("{at}:{e}")),
            }
        }
    }

    // elevation:L0~L4 × {blur, offsetY, alpha, color}
    if let Ok(elevation) = get(&json, "elevation") {
        for (key, e) in &exp.elevations {
            let at = format!("elevation.{key}");
            let val = match get(elevation, key).and_then(leaf) {
                Ok(v) => v,
                Err(err) => {
                    bad.push(format!("{at}:{err}"));
                    continue;
                }
            };
            for (field, code_v) in [
                ("blur", f64::from(e.blur)),
                ("offsetY", f64::from(e.offset_y)),
                ("alpha", f64::from(e.alpha)),
            ] {
                match get(val, field).and_then(as_num) {
                    Ok(v) => {
                        if (v - code_v).abs() > 1e-6 {
                            bad.push(format!("{at}.{field}:JSON {v} != 代码 {code_v}"));
                        }
                    }
                    Err(err) => bad.push(format!("{at}.{field}:{err}")),
                }
            }
            match get(val, "color").and_then(as_str) {
                Ok(c) => {
                    if c != exp.shadow_hex {
                        bad.push(format!(
                            "{at}.color:JSON {c} != 代码 {shadow}",
                            shadow = exp.shadow_hex
                        ));
                    }
                }
                Err(err) => bad.push(format!("{at}.color:{err}")),
            }
        }
    }

    // state-layer:深/浅 × 5
    for (theme, pairs) in [("dark", &exp.state_dark), ("light", &exp.state_light)] {
        let path = format!("state-layer.{theme}");
        let group = match get(&json, "state-layer").and_then(|g| get(g, theme)) {
            Ok(g) => g,
            Err(e) => {
                bad.push(format!("{path}:{e}"));
                continue;
            }
        };
        for (key, code_v) in pairs {
            let at = format!("{path}.{key}");
            match get(group, key).and_then(leaf).and_then(as_num) {
                Ok(v) => {
                    if (v - f64::from(*code_v)).abs() > 1e-6 {
                        bad.push(format!("{at}:JSON {v} != 代码 {}", *code_v));
                    }
                }
                Err(e) => bad.push(format!("{at}:{e}")),
            }
        }
    }

    // motion:duration 四档 + spring 三档 × 3 参数
    let motion = match get(&json, "motion") {
        Ok(m) => m,
        Err(e) => {
            bad.push(format!("motion:{e}"));
            return bad;
        }
    };
    match get(motion, "duration") {
        Ok(duration) => {
            for (key, code_v) in &exp.durations {
                let at = format!("motion.duration.{key}");
                match get(duration, key).and_then(leaf).and_then(as_num) {
                    Ok(v) => {
                        if (v - code_v).abs() > f64::EPSILON {
                            bad.push(format!("{at}:JSON {v} != 代码 {code_v}"));
                        }
                    }
                    Err(e) => bad.push(format!("{at}:{e}")),
                }
            }
        }
        Err(e) => bad.push(format!("motion.duration:{e}")),
    }
    match get(motion, "spring") {
        Ok(spring) => {
            for (key, params) in &exp.springs {
                let at = format!("motion.spring.{key}");
                let val = match get(spring, key).and_then(leaf) {
                    Ok(v) => v,
                    Err(e) => {
                        bad.push(format!("{at}:{e}"));
                        continue;
                    }
                };
                for (field, code_v) in [
                    ("stiffness", params[0]),
                    ("damping", params[1]),
                    ("mass", params[2]),
                ] {
                    match get(val, field).and_then(as_num) {
                        Ok(v) => {
                            if (v - code_v).abs() > f64::EPSILON {
                                bad.push(format!("{at}.{field}:JSON {v} != 代码 {code_v}"));
                            }
                        }
                        Err(e) => bad.push(format!("{at}.{field}:{e}")),
                    }
                }
            }
        }
        Err(e) => bad.push(format!("motion.spring:{e}")),
    }

    // typography(TOK-02):font-family 2 键 + text-size 7 档 × 3 值;
    // 键集合双向完全一致(缺键/多键都红)
    let typography = match get(&json, "typography") {
        Ok(t) => t,
        Err(e) => {
            bad.push(format!("typography:{e}"));
            return bad;
        }
    };
    match get(typography, "font-family") {
        Ok(fonts) => {
            if let Json::Obj(json_pairs) = fonts {
                let mut json_keys: Vec<&str> = json_pairs
                    .iter()
                    .map(|(k, _)| k.as_str())
                    .filter(|k| !k.starts_with('$'))
                    .collect();
                json_keys.sort_unstable();
                let mut want: Vec<&str> = exp.fonts.iter().map(|(k, _)| *k).collect();
                want.sort_unstable();
                if json_keys != want {
                    bad.push(format!(
                        "typography.font-family:键集合不一致(JSON {json_keys:?} != 代码 {want:?})"
                    ));
                }
            }
            for (key, code_v) in &exp.fonts {
                let at = format!("typography.font-family.{key}");
                match get(fonts, key).and_then(leaf).and_then(as_str) {
                    Ok(v) => {
                        if v != *code_v {
                            bad.push(format!("{at}:JSON {v:?} != 代码 {code_v:?}"));
                        }
                    }
                    Err(e) => bad.push(format!("{at}:{e}")),
                }
            }
        }
        Err(e) => bad.push(format!("typography.font-family:{e}")),
    }
    match get(typography, "text-size") {
        Ok(sizes) => {
            if let Json::Obj(json_pairs) = sizes {
                let mut json_keys: Vec<&str> = json_pairs
                    .iter()
                    .map(|(k, _)| k.as_str())
                    .filter(|k| !k.starts_with('$'))
                    .collect();
                json_keys.sort_unstable();
                let mut want: Vec<&str> = exp.text_sizes.iter().map(|(k, _)| *k).collect();
                want.sort_unstable();
                if json_keys != want {
                    bad.push(format!(
                        "typography.text-size:键集合不一致(JSON {json_keys:?} != 代码 {want:?})"
                    ));
                }
            }
            for (key, code_v) in &exp.text_sizes {
                let at = format!("typography.text-size.{key}");
                let val = match get(sizes, key).and_then(leaf) {
                    Ok(v) => v,
                    Err(e) => {
                        bad.push(format!("{at}:{e}"));
                        continue;
                    }
                };
                for (field, code_n) in [
                    ("size", code_v[0]),
                    ("lineHeight", code_v[1]),
                    ("weight", code_v[2]),
                ] {
                    match get(val, field).and_then(as_num) {
                        Ok(v) => {
                            if (v - code_n).abs() > 1e-6 {
                                bad.push(format!("{at}.{field}:JSON {v} != 代码 {code_n}"));
                            }
                        }
                        Err(e) => bad.push(format!("{at}.{field}:{e}")),
                    }
                }
            }
        }
        Err(e) => bad.push(format!("typography.text-size:{e}")),
    }

    bad
}

fn compare(json_text: &str) -> Vec<String> {
    compare_expected(&Expected::from_code(), json_text)
}

/// JSON 真相源路径(<仓库根>/docs/design/sable-tokens.json)。
fn json_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CARGO_MANIFEST_DIR 应有两级父目录(仓库根)")
        .join("docs")
        .join("design")
        .join("sable-tokens.json")
}

/// 读真实 JSON(门禁失效自检:读不到 = 红,防空转)。
fn real_json() -> String {
    let path = json_path();
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("读取令牌真相 {} 失败:{e}", path.display()))
}

/// 内联替换第一处 `from` → `to`(找不到即 panic:反面用例自身失效)。
fn replace_once(text: &str, from: &str, to: &str) -> String {
    let at = text
        .find(from)
        .unwrap_or_else(|| panic!("反面用例锚点不存在(JSON 与测试脱锚,请同步锚点):{from}"));
    let mut out = String::with_capacity(text.len() + to.len());
    out.push_str(&text[..at]);
    out.push_str(to);
    out.push_str(&text[at + from.len()..]);
    out
}

// ---------------------------------------------------------------------------
// 正向门禁:真实 JSON 与代码逐值一致
// ---------------------------------------------------------------------------

#[test]
fn tokens_json_matches_tokens_rs_value_by_value() {
    let json = real_json();
    let bad = compare(&json);
    assert!(
        bad.is_empty(),
        "tokens.rs 与 sable-tokens.json 漂移 {} 处(改任一侧必须同轮同步另一侧):\n  - {}",
        bad.len(),
        bad.join("\n  - ")
    );
}

#[test]
fn tokens_json_typography_table_landed_no_future_tables() {
    // TOK-02 已落 typography 表:必须存在且含 font-family/text-size 两键;
    // 仍属未来的表出现即红(防"提前造假"复发)
    let json = real_json();
    let root = parse_json(&json).expect("真实 JSON 必须可解析");
    let typography = get(&root, "typography")
        .unwrap_or_else(|e| panic!("TOK-02 后 JSON 必有 typography 表:{e}"));
    for key in ["font-family", "text-size"] {
        assert!(
            get(typography, key).is_ok(),
            "typography.{key} 必须存在(TOK-02)"
        );
    }
    for banned in ["icon", "icons", "density", "breakpoint", "component"] {
        assert!(
            !json.contains(&format!("\"{banned}\"")),
            "JSON 出现未落地表 \"{banned}\":新表必须连代码与门禁一起落地"
        );
    }
}

// ---------------------------------------------------------------------------
// 反面测试(TC-GATE-TOKENS-01):任一侧单改一个值必须红(内联字符串,零文件)
// ---------------------------------------------------------------------------

#[test]
fn tc_gate_tokens_01_mutating_json_value_is_red() {
    let original = real_json();
    assert!(compare(&original).is_empty(), "前置:原版必须全绿");
    let cases: [(&str, &str, &str); 10] = [
        ("color.dark.surface-2", "\"#313136FF\"", "\"#313137FF\""),
        (
            "color.light.text-secondary",
            "\"#646464FF\"",
            "\"#646465FF\"",
        ),
        (
            "spacing.sm",
            "\"sm\": { \"$type\": \"dimension\", \"$value\": 8 }",
            "\"sm\": { \"$type\": \"dimension\", \"$value\": 9 }",
        ),
        ("elevation.l3.blur", "\"blur\": 16", "\"blur\": 15"),
        (
            "state-layer.dark.hover",
            "\"hover\": { \"$type\": \"number\", \"$value\": 0.06 }",
            "\"hover\": { \"$type\": \"number\", \"$value\": 0.05 }",
        ),
        (
            "motion.duration.state",
            "\"state\": { \"$type\": \"duration\", \"$value\": 120 }",
            "\"state\": { \"$type\": \"duration\", \"$value\": 121 }",
        ),
        (
            "motion.spring.snappy.damping",
            "\"damping\": 28.0",
            "\"damping\": 27.0",
        ),
        // typography(TOK-02)侧:字号值与字体族名
        (
            "typography.text-size.caption.size",
            "\"size\": 11, \"lineHeight\": 16, \"weight\": 400",
            "\"size\": 12, \"lineHeight\": 16, \"weight\": 400",
        ),
        (
            "typography.text-size.title.lineHeight",
            "\"size\": 15, \"lineHeight\": 22, \"weight\": 600",
            "\"size\": 15, \"lineHeight\": 23, \"weight\": 600",
        ),
        (
            "typography.font-family.mono",
            "\"JetBrains Mono\"",
            "\"JetBrains Mono NL\"",
        ),
    ];
    for (path, from, to) in cases {
        let mutated = replace_once(&original, from, to);
        assert_ne!(mutated, original, "{path}:替换未生效(用例失效)");
        let bad = compare(&mutated);
        assert!(
            !bad.is_empty(),
            "TC-GATE-TOKENS-01:改 JSON 的 {path} 而不同步代码,门禁必须红"
        );
        assert!(
            bad.iter().any(|m| m.contains(path)),
            "违例消息应指认 {path}:见 {:?}",
            bad
        );
    }
}

#[test]
fn tc_gate_tokens_01_code_side_drift_is_red() {
    // "改了 tokens.rs 一个色值、JSON 未同步":期望侧手动偏移一个值,
    // 同一比较器必须红(证明门禁以代码真相驱动,不是对 JSON 的自说自话)
    let mut exp = Expected::from_code();
    let at = exp
        .dark
        .iter()
        .position(|(k, _)| *k == "surface-2")
        .expect("期望侧必有 surface-2");
    let (_, orig) = exp.dark[at];
    exp.dark[at].1 = gpui::Hsla {
        l: orig.l + 0.01,
        ..orig
    };
    // typography 侧同理:偏移 display 档字号,证明该表也是双向对拍
    let ts_at = exp
        .text_sizes
        .iter()
        .position(|(k, _)| *k == "display")
        .expect("期望侧必有 display 档");
    exp.text_sizes[ts_at].1[0] += 1.0;
    let bad = compare_expected(&exp, &real_json());
    assert!(
        bad.iter().any(|m| m.contains("color.dark.surface-2")),
        "代码侧单侧漂移必须红:{bad:?}"
    );
    assert!(
        bad.iter()
            .any(|m| m.contains("typography.text-size.display.size")),
        "typography 代码侧单侧漂移必须红:{bad:?}"
    );
}

// ---------------------------------------------------------------------------
// TC-ANI-SPRING-01:弹簧档位与 JSON 逐值一致(代码三层单点:
// tokens::SPRING_* == anim::Spring 常量 == JSON motion.spring)
// ---------------------------------------------------------------------------

#[test]
fn tc_ani_spring_01_spring_tiers_match_json() {
    let json = real_json();
    let spring_table = parse_json(&json)
        .ok()
        .and_then(|j| get(&j, "motion").ok().cloned())
        .and_then(|m| get(&m, "spring").ok().cloned())
        .expect("JSON 必有 motion.spring");
    for (name, preset, tier) in [
        (
            "snappy",
            &SPRING_SNAPPY,
            sable_widgets::anim::Spring::SNAPPY,
        ),
        ("soft", &SPRING_SOFT, sable_widgets::anim::Spring::SOFT),
        (
            "bouncy",
            &SPRING_BOUNCY,
            sable_widgets::anim::Spring::BOUNCY,
        ),
    ] {
        // 层 1:tokens 常量 == anim::Spring 常量(重指向生效)
        assert_eq!(
            (tier.stiffness, tier.damping, tier.mass),
            (preset.stiffness, preset.damping, preset.mass),
            "anim::Spring::{name} 未与 tokens::SPRING_* 单点对齐"
        );
        // 层 2:tokens 常量 == JSON 逐值
        let entry = match get(&spring_table, name).and_then(leaf) {
            Ok(v) => v,
            Err(e) => panic!("JSON motion.spring.{name}:{e}"),
        };
        for (field, code_v) in [
            ("stiffness", preset.stiffness),
            ("damping", preset.damping),
            ("mass", preset.mass),
        ] {
            let v = get(entry, field)
                .and_then(as_num)
                .unwrap_or_else(|e| panic!("{name}.{field}:{e}"));
            assert!(
                (v - code_v).abs() <= f64::EPSILON,
                "{name}.{field}:JSON {v} != 代码 {code_v}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// TC-TOK-TYPE-01:七档排版逐值断言(TOK-02,报告 §5.4 表)
// 代码 truth(tokens.rs TextSize)== 报告规格 == JSON typography 表
// ---------------------------------------------------------------------------

#[test]
fn tc_tok_type_01_seven_text_sizes_match_spec_and_json() {
    // 层 1:代码常量 == 报告 §5.4 规格(20/28/600 …)
    let spec: [(&str, [f64; 3]); 7] = [
        ("display", [20.0, 28.0, 600.0]),
        ("title", [15.0, 22.0, 600.0]),
        ("body-strong", [13.0, 20.0, 600.0]),
        ("body", [13.0, 20.0, 400.0]),
        ("label", [12.0, 18.0, 500.0]),
        ("caption", [11.0, 16.0, 400.0]),
        ("mono", [12.0, 18.0, 400.0]),
    ];
    assert_eq!(TEXT_SIZES.len(), spec.len(), "必须恰七档");
    for ((name, ts), (want, values)) in TEXT_SIZES.iter().zip(spec) {
        assert_eq!(*name, want, "档名与序 = 报告表");
        assert_eq!(
            (
                f64::from(ts.size),
                f64::from(ts.line_height),
                f64::from(ts.weight)
            ),
            (values[0], values[1], values[2]),
            "{name} 与报告 §5.4 三值不符"
        );
        assert!(ts.line_height >= ts.size, "{name} 行高 ≥ 字号(CJK 安全)");
        assert!(
            (400.0..=600.0).contains(&ts.weight),
            "{name} 字重必须在随包字重集合 400/500/600 内"
        );
    }
    // 层 2:代码常量 == JSON typography.text-size(经全量比对器,数值路径)
    let exp = Expected::from_code();
    let bad = compare_expected(&exp, &real_json());
    assert!(bad.is_empty(), "typography 代码↔JSON 漂移:{bad:?}");
    // 层 3:字体族令牌与 JSON font-family 一致(ui=Inter,mono=JetBrains Mono)
    assert_eq!(UI_FONT, "Inter");
    assert_eq!(MONO_FONT, "JetBrains Mono");
}

// ---------------------------------------------------------------------------
// 解析器自检(门禁地基;解析失败本身即红,故须证明会拒收坏输入)
// ---------------------------------------------------------------------------

#[test]
fn parser_accepts_the_token_json_subset() {
    let j = parse_json(
        "{\"a\": {\"$value\": 1.5}, \"b\": \"#0E0E10FF\", \"c\": -3, \"d\": {\"e\": {}}}",
    )
    .expect("子集 JSON 必须可解析");
    assert_eq!(get(&j, "a").and_then(leaf).and_then(as_num), Ok(1.5));
    assert_eq!(get(&j, "b").and_then(as_str), Ok("#0E0E10FF"));
    assert!(matches!(get(&j, "c"), Ok(Json::Num(n)) if *n == -3.0));
}

#[test]
fn parser_rejects_malformed_input() {
    for bad in [
        "",
        "{",
        "{\"a\"}",
        "{\"a\":}",
        "{\"a\": 1,}",
        "[1,2]",
        "1 2",
        "{} trailing",
        "\"unterminated",
        "{\"a\": tru}",
    ] {
        assert!(parse_json(bad).is_err(), "必须拒收:{bad}");
    }
}
