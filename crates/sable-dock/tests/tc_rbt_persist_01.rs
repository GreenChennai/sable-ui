//! TC-RBT-PERSIST-01(迭代审查报告 RBT-02 / §6 RB-03、RB-08 验收):
//! sable-dock 壳态持久化契约——
//! ① 坏 JSON → 回退默认不 panic 且"已回退"显式可观测;
//! ② 模拟中断写(tmp 半截、未 rename)→ 旧文件完好、load 不崩;
//! ③ 版本字段缺失/未知 → 迁移/回退路径断言;
//! ④ 连续两次 persist 后文件均完整可读(原子性)。
//!
//! 只用公开 API:`sable_dock` crate 根再导出 + `sable_dock::gpui_component`
//! 公开再导出的上游 serde 面(`DockAreaState`/`PanelState`/`PanelInfo`)。

use std::fs;
use std::path::PathBuf;

use sable_dock::gpui_component::dock::{DockAreaState, PanelInfo, PanelState};
use sable_dock::{FallbackReason, LAYOUT_VERSION, LoadedLayout, load_layout, persist_layout_state};

/// 每个测试独占一个临时目录(TEMP/TMP 已指向 D:\Temp,不写 C 盘)。
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sable-dock-tc-rbt-persist-{}-{tag}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

/// 可区分的布局状态:center 面板名不同 → JSON 不同 → PartialEq 可辨。
/// `version` 故意留 `None`:落盘归一是 persist 端契约,由测试断言。
fn state_named(name: &str) -> DockAreaState {
    DockAreaState {
        version: None,
        center: PanelState {
            panel_name: name.to_string(),
            children: Vec::new(),
            info: PanelInfo::Tabs { active_index: 0 },
        },
        left_dock: None,
        right_dock: None,
        bottom_dock: None,
    }
}

/// persist 归一化后的期望态(版本戳 v1)。
fn versioned(state: &DockAreaState) -> DockAreaState {
    let mut expected = state.clone();
    expected.version = Some(LAYOUT_VERSION);
    expected
}

/// 目录里没有任何 *.tmp 残留(临时名含 pid,不硬编码具体名)。
fn assert_no_tmp_residue(dir: &std::path::Path) {
    let residue: Vec<PathBuf> = fs::read_dir(dir)
        .expect("读目录")
        .map(|e| e.expect("目录项").path())
        .filter(|p| p.extension().is_some_and(|x| x == "tmp"))
        .collect();
    assert!(residue.is_empty(), "不应残留临时文件: {residue:?}");
}

// ---------------------------------------------------------------------------
// 契约读端 · 文件不存在 → 默认布局
// ---------------------------------------------------------------------------

#[test]
fn tc_rbt_persist_01_missing_file_returns_default_layout() {
    let dir = temp_dir("missing");
    let path = dir.join("layout.json");

    let loaded = load_layout(&path).expect("缺文件回退默认,不是 Err");
    assert!(loaded.is_fallback(), "缺文件必须显式表达为回退");
    assert_eq!(
        loaded.fallback_reason(),
        Some(&FallbackReason::FileMissing),
        "回退原因 = 文件不存在"
    );
    assert_eq!(
        loaded.layout(),
        &DockAreaState::default(),
        "回退布局 = 默认布局"
    );
    assert!(matches!(loaded, LoadedLayout::Fallback { .. }));
}

// ---------------------------------------------------------------------------
// ① 坏 JSON → 回退默认,不 panic,"已回退"可观测
// ---------------------------------------------------------------------------

#[test]
fn tc_rbt_persist_01_corrupt_json_falls_back_to_default_observably() {
    let dir = temp_dir("corrupt");
    // 各种坏形态:截断 JSON、非法 token、类型不符、缺必填字段、空对象
    let corrupt_samples = [
        "{\"center\": {\"panel_na", // 写到一半断电的截断体
        "not json at all",          // 纯垃圾
        "[]",                       // 类型不符(数组)
        "null",                     // 类型不符(null)
        "{}",                       // 缺必填字段 center
        "{\"version\": \"oops\"}",  // 版本字段类型错误
    ];
    for (i, sample) in corrupt_samples.iter().enumerate() {
        let path = dir.join(format!("corrupt-{i}.json"));
        fs::write(&path, sample).expect("写坏文件");

        let loaded = load_layout(&path).expect("坏文件必须回退默认而非 Err(不许崩)");
        assert!(loaded.is_fallback(), "样本 {sample:?}:必须显式表达已回退");
        match loaded.fallback_reason() {
            Some(FallbackReason::CorruptJson { detail }) => {
                assert!(!detail.is_empty(), "CorruptJson 携带诊断 detail");
            }
            other => panic!("回退原因必须是 CorruptJson,实际 {other:?}(样本 {sample:?})"),
        }
        assert_eq!(
            loaded.layout(),
            &DockAreaState::default(),
            "坏文件回退到默认布局(样本 {sample:?})"
        );
    }
}

// ---------------------------------------------------------------------------
// ② 模拟中断写(tmp 半截、未 rename)→ 旧文件完好、load 不崩
// ---------------------------------------------------------------------------

#[test]
fn tc_rbt_persist_01_interrupted_write_leaves_old_file_intact_and_load_ok() {
    let dir = temp_dir("interrupted");
    let path = dir.join("layout.json");

    // 先落一份完整旧布局
    let old = state_named("center-v1");
    persist_layout_state(&path, &old).expect("首次持久化");
    let old_bytes = fs::read(&path).expect("读旧文件");

    // 模拟中断写:tmp 半截 JSON 已落盘,rename 未发生。
    // (foundation 原子写临时名形如 {stem}.{pid}.tmp;这里手工植入等价残留)
    let tmp = dir.join(format!("layout.json.{}.tmp", std::process::id()));
    fs::write(&tmp, "{\"center\": {\"panel_na").expect("写半截 tmp");

    // 读端契约:只认目标文件,残留 tmp 既不被误读也不让 load 出错
    let loaded = load_layout(&path).expect("残留 tmp 不得让 load 崩溃/出错");
    assert!(!loaded.is_fallback(), "目标文件完好时应正常恢复");
    assert_eq!(
        loaded.layout(),
        &versioned(&old),
        "恢复出的就是旧布局(v1 版本戳)"
    );
    assert_eq!(
        fs::read(&path).expect("读目标"),
        old_bytes,
        "中断写后旧文件字节级完好"
    );

    // 变体:首次 rename 前就崩溃(目录里只有 tmp,目标文件从未存在)
    let fresh = temp_dir("interrupted-fresh");
    let fresh_path = fresh.join("layout.json");
    fs::write(fresh.join("layout.json.424242.tmp"), "{\"center\"…半截").expect("写孤儿 tmp");
    let loaded_fresh = load_layout(&fresh_path).expect("孤儿 tmp 不得让 load 崩溃");
    assert_eq!(
        loaded_fresh.fallback_reason(),
        Some(&FallbackReason::FileMissing),
        "目标缺失按缺文件回退,tmp 不被误读为布局"
    );
}

// ---------------------------------------------------------------------------
// ③ 版本字段缺失/未知 → 迁移/回退路径
// ---------------------------------------------------------------------------

#[test]
fn tc_rbt_persist_01_missing_version_field_migrates_observably() {
    let dir = temp_dir("legacy-version");
    // 旧版 save_layout 产物:version 显式 null / 字段整体缺失,两种形态
    let legacy_samples = [
        (
            "version-null",
            r#"{"version":null,"center":{"panel_name":"legacy-a","children":[],"info":{"tabs":{"active_index":0}}}}"#,
        ),
        (
            "version-absent",
            r#"{"center":{"panel_name":"legacy-b","children":[],"info":{"tabs":{"active_index":0}}}}"#,
        ),
    ];
    for (tag, json) in legacy_samples {
        let path = dir.join(format!("{tag}.json"));
        fs::write(&path, json).expect("写旧版布局");

        let loaded = load_layout(&path).expect("旧版布局可读");
        assert!(!loaded.is_fallback(), "旧版布局走迁移,不回退({tag})");
        match loaded {
            LoadedLayout::Restored {
                layout,
                migrated_from,
            } => {
                assert_eq!(
                    migrated_from,
                    Some(0),
                    "缺失版本字段按版本 0(旧版 save_layout 产物)迁移({tag})"
                );
                assert_eq!(
                    layout.version, None,
                    "迁移桩恒等,不伪造版本戳;落盘时才归一({tag})"
                );
                assert!(
                    !layout.center.panel_name.is_empty(),
                    "布局载荷原样保留({tag})"
                );
            }
            other => panic!("必须是 Restored,实际 {other:?}({tag})"),
        }
    }
}

#[test]
fn tc_rbt_persist_01_unknown_version_falls_back_observably() {
    let dir = temp_dir("future-version");
    let path = dir.join("layout.json");

    // 未来版本写出的布局:version 高于当前库支持
    let future_version = LAYOUT_VERSION + 1;
    let mut json = serde_json::to_value(state_named("from-the-future")).expect("序列化");
    json["version"] = serde_json::json!(future_version);
    fs::write(&path, serde_json::to_string(&json).expect("回序列化")).expect("写未来版布局");

    let loaded = load_layout(&path).expect("未知版本回退默认,不是 Err");
    assert!(loaded.is_fallback(), "未知版本必须显式表达已回退");
    assert_eq!(
        loaded.fallback_reason(),
        Some(&FallbackReason::UnknownVersion {
            found: future_version,
            supported: LAYOUT_VERSION,
        }),
        "回退原因携带 found/supported 供宿主告警"
    );
    assert_eq!(loaded.layout(), &DockAreaState::default());
}

#[test]
fn tc_rbt_persist_01_current_version_restores_without_migration() {
    let dir = temp_dir("current-version");
    let path = dir.join("layout.json");
    let state = state_named("current");

    persist_layout_state(&path, &state).expect("持久化");
    let loaded = load_layout(&path).expect("当前版本布局可读");
    match loaded {
        LoadedLayout::Restored {
            layout,
            migrated_from,
        } => {
            assert_eq!(migrated_from, None, "当前版本无需迁移");
            assert_eq!(layout, versioned(&state), "载荷往返一致");
        }
        other => panic!("必须是 Restored,实际 {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// ④ 连续两次 persist 后文件均完整可读(原子性)
// ---------------------------------------------------------------------------

#[test]
fn tc_rbt_persist_01_double_persist_atomic_and_complete() {
    let dir = temp_dir("atomic");
    let path = dir.join("layout.json");
    let first = state_named("first-layout");
    let second = state_named("second-layout");

    persist_layout_state(&path, &first).expect("第一次持久化");
    let text_first = fs::read_to_string(&path).expect("第一次产物可读");
    let parsed_first: DockAreaState =
        serde_json::from_str(&text_first).expect("第一次产物是完整 JSON");
    assert_eq!(parsed_first, versioned(&first), "第一次内容完整且版本归一");

    persist_layout_state(&path, &second).expect("第二次持久化(覆盖写)");
    let text_second = fs::read_to_string(&path).expect("第二次产物可读");
    let parsed_second: DockAreaState =
        serde_json::from_str(&text_second).expect("第二次产物是完整 JSON");
    assert_eq!(parsed_second, versioned(&second), "第二次完整覆盖第一次");
    assert_ne!(text_first, text_second, "两次写入内容确实更替");

    // 原子性旁证:tmp+fsync+rename 全部收口,目录无 *.tmp 残留
    assert_no_tmp_residue(&dir);

    // 失败路径:目标父级是普通文件 → Err,且不留半截、不损伤既有文件
    let blocker = dir.join("blocker.txt");
    fs::write(&blocker, "do-not-touch").expect("写占位文件");
    let bad_path = blocker.join("layout.json");
    assert!(
        persist_layout_state(&bad_path, &first).is_err(),
        "失败必须返回 Err(失败不留半截文件)"
    );
    assert_eq!(
        fs::read_to_string(&blocker).expect("读占位文件"),
        "do-not-touch",
        "失败不损伤既有文件"
    );
    assert_no_tmp_residue(&dir);
}
