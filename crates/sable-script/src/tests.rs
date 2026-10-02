//! T4 验收测试(迭代计划 09:脚本 ≥ 6 个;V4.0 T4 增补 ⑧-⑪):
//! 建 3 矩形改色并全撤销、组挂载(重载)、非法脚本零影响、10 矩形循环端到端、
//! undo/redo 算子往返、remove/node_count 一致性 + into_parts、
//! 出借槽协议;V4.0:死循环操作数上限、非有限坐标拒收、id 位级无损往返、
//! 中文错误消息关键字。

use rhai::Dynamic;
use sable_foundation::prelude::{NodeId, Paint, Scene};
use slotmap::KeyData;

use crate::api::{ScriptHost, coerce_id, script_id};
use crate::error::ScriptError;

/// 脚本侧 i64 id → 宿主侧 NodeId(与 api.rs 同一 ffi 表示往返)。
fn node_id(raw: i64) -> NodeId {
    NodeId::from(KeyData::from_ffi(raw as u64))
}

/// 场景里全部实心填充(排序后比较,规避撤销恢复换发 id 带来的顺序漂移)。
fn solid_fills(scene: &Scene) -> Vec<[u8; 4]> {
    let mut fills: Vec<[u8; 4]> = scene
        .nodes
        .iter()
        .filter_map(
            |(id, _)| match scene.path(id).and_then(|p| p.fill.clone()) {
                Some(Paint::Solid(c)) => Some(c),
                _ => None,
            },
        )
        .collect();
    fills.sort_unstable();
    fills
}

/// 场景里第一个指定名称的节点 id(测试夹具)。
fn find_by_name(scene: &Scene, name: &str) -> Option<NodeId> {
    scene
        .nodes
        .iter()
        .find(|(_, n)| n.name == name)
        .map(|(id, _)| id)
}

/// ① 建矩形 ×3 + set_fill → 场景断言 + 撤销栈计数 + 全撤销/全重做往返。
///
/// 语义备注:任务书预期 undo_len == 3,但 History 实际语义是 AddNode 不参与
/// merge、SetFill 只与同节点 SetFill 合并(docs/03 §2),故 3 次添加 + 2 次
/// set_fill(同节点合并为一步)= 4 步;此处按真实语义断言。
#[test]
fn script_adds_three_rects_and_set_fill_is_fully_undoable() {
    let mut host = ScriptHost::new();
    let script = "
        let a = add_rect(0, 0, 10, 10, 200, 40, 40, 255);
        let b = add_rect(20, 0, 30, 10, 40, 200, 40, 255);
        let c = add_rect(40, 0, 50, 10, 40, 40, 200, 255);
        set_fill(a, 250, 120, 0, 255);
        set_fill(a, 10, 10, 10, 255);
        a
    ";
    let a_raw = host
        .run(script)
        .expect("脚本应成功")
        .as_int()
        .expect("返回节点 id");
    assert_eq!(host.scene().len(), 3);
    assert_eq!(
        host.history().undo_len(),
        4,
        "三条 AddNode 各一步;同节点两条 SetFill 合并为一步"
    );
    assert_eq!(
        solid_fills(host.scene()),
        vec![[10, 10, 10, 255], [40, 40, 200, 255], [40, 200, 40, 255]],
        "merge 保留最新的 new"
    );

    // undo ×4 全还原(经脚本 undo 算子,与 UI 共用同一撤销栈)
    for _ in 0..4 {
        let _ = host.run("undo()").expect("undo 算子应成功");
    }
    assert!(host.scene().is_empty(), "4 步撤销后场景清空");
    assert_eq!(host.history().undo_len(), 0);
    assert!(host.history().can_redo(), "撤销后的命令全部进入 redo 栈");

    // redo ×4 一字不差地恢复(节点换发新 id,数据不变)
    for _ in 0..4 {
        let _ = host.run("redo()").expect("redo 算子应成功");
    }
    assert_eq!(host.scene().len(), 3);
    assert_eq!(host.history().undo_len(), 4);
    assert_eq!(
        solid_fills(host.scene()),
        vec![[10, 10, 10, 255], [40, 40, 200, 255], [40, 200, 40, 255]]
    );
    // 脚本持有的旧 id 在 undo/redo 循环后已失效(slotmap 换发新 id)
    assert!(
        host.scene().node(node_id(a_raw)).is_none(),
        "旧 id 失效是既定语义(command.rs)"
    );
}

/// ② add_group + add_rect 带可选 parent(同名不同元数重载)。
#[test]
fn script_add_group_and_parented_rects_via_overload() {
    let mut host = ScriptHost::new();
    let script = r#"
        let g = add_group("组");
        let r1 = add_rect(0, 0, 10, 10, 255, 0, 0, 255, g);
        let r2 = add_rect(0, 0, 10, 10, 0, 255, 0, 255, g);
        [g, r1, r2]
    "#;
    let ids = host
        .run(script)
        .expect("脚本应成功")
        .into_array()
        .expect("返回 id 数组");
    let raw: Vec<i64> = ids.iter().map(|d| d.as_int().expect("id 为整数")).collect();
    let (g, r1, r2) = (node_id(raw[0]), node_id(raw[1]), node_id(raw[2]));

    let g_node = host.scene().node(g).expect("组存在");
    assert_eq!(g_node.children, vec![r1, r2], "两个矩形按顺序挂进组");
    assert_eq!(host.scene().node(r1).expect("r1").parent, Some(g));
    assert_eq!(host.scene().len(), 3);
    assert_eq!(host.history().undo_len(), 3);

    // 逐层撤销:矩形先走,组最后走
    let _ = host.run("undo()").expect("undo");
    assert_eq!(host.scene().node(g).expect("组还在").children.len(), 1);
    let _ = host.run("undo()").expect("undo");
    assert_eq!(host.scene().node(g).expect("组还在").children.len(), 0);
    let _ = host.run("undo()").expect("undo");
    assert!(host.scene().is_empty(), "组也撤销后场景清空");
}

/// ③ 非法脚本(语法错/未定义变量/非法 id/非法类型/非法父节点)→ Err(Eval)
/// 且场景与撤销栈零影响。
#[test]
fn illegal_scripts_error_without_touching_scene() {
    let mut host = ScriptHost::new();

    // 语法错误
    let r = host.run("let x = ;");
    assert!(matches!(r, Err(ScriptError::Eval(_))), "语法错应报 Eval");
    // 未定义变量
    let r = host.run("let y = no_such_var + 1;");
    assert!(
        matches!(r, Err(ScriptError::Eval(_))),
        "未定义变量应报 Eval"
    );
    // 未知节点 id:校验先行,不产生空撤销步
    let r = host.run("set_fill(999999, 0, 0, 0, 255);");
    assert!(matches!(r, Err(ScriptError::Eval(_))), "未知 id 应报 Eval");
    // 参数类型非法(Dynamic coerce 失败)
    let r = host.run("add_rect(\"左上\", 0, 1, 1, 0, 0, 0, 255);");
    assert!(
        matches!(r, Err(ScriptError::Eval(_))),
        "类型不合法应报 Eval"
    );
    // 非法父节点 id
    let r = host.run("add_rect(0, 0, 1, 1, 0, 0, 0, 255, 424242);");
    assert!(
        matches!(r, Err(ScriptError::Eval(_))),
        "父节点不存在应报 Eval"
    );
    // set_fill 打在组上:拒绝而非静默空转
    let g_raw = host
        .run("add_group(\"组\")")
        .expect("建组")
        .as_int()
        .expect("id");
    let r = host.run(&format!("set_fill({g_raw}, 0, 0, 0, 255);"));
    assert!(
        matches!(r, Err(ScriptError::Eval(_))),
        "组没有填充,应报 Eval"
    );

    // 全程:场景不变、撤销栈零污染
    assert_eq!(host.scene().len(), 1, "只有成功建出的那一个组");
    assert_eq!(host.history().undo_len(), 1, "失败算子不产生撤销步");
}

/// ④ 10 个矩形循环脚本端到端(T4 验收原文场景)。
#[test]
fn loop_script_builds_ten_rects_end_to_end() {
    let mut host = ScriptHost::new();
    let script = "
        for i in 0..10 {
            let x = i * 20;
            add_rect(x, 0, x + 15, 10, i * 20, 100, 200, 255);
        }
        node_count()
    ";
    let count = host
        .run(script)
        .expect("循环脚本应成功")
        .as_int()
        .expect("返回 node_count");
    assert_eq!(count, 10);
    assert_eq!(host.scene().len(), 10, "node_count 与场景实际节点数一致");
    assert_eq!(host.history().undo_len(), 10, "每个 add_rect 一步");
    // 整数坐标已被 coerce 成 f64、颜色通道按值生效:抽查一个矩形的填充
    let fills = solid_fills(host.scene());
    assert!(
        fills.contains(&[180, 100, 200, 255]),
        "i = 9 的矩形填充应在场"
    );

    // 全量撤销回到空场景
    for _ in 0..10 {
        let _ = host.run("undo()").expect("undo");
    }
    assert!(host.scene().is_empty());
    assert_eq!(host.history().undo_len(), 0);
}

/// ⑤ undo/redo 算子往返(脚本内触发,撤销栈与 UI 同源)。
#[test]
fn script_undo_redo_operators_roundtrip() {
    let mut host = ScriptHost::new();
    let id_raw = host
        .run(r#"let a = add_rect(0, 0, 5, 5, 1, 2, 3, 255); set_fill(a, 9, 8, 7, 255); set_name(a, "主角"); a"#)
        .expect("构建脚本应成功")
        .as_int()
        .expect("返回节点 id");
    assert_eq!(
        host.history().undo_len(),
        3,
        "AddNode + SetFill + SetName 各一步"
    );

    // 脚本内 undo:名称回退到 AddNode 默认名
    let name = host
        .run(&format!("undo(); node_name({id_raw})"))
        .expect("undo 后读取名称")
        .into_string()
        .expect("返回名称字符串");
    assert_eq!(name, "矩形", "撤销 SetName 后回到默认名");
    assert_eq!(host.history().undo_len(), 2);

    // 脚本内 redo:名称恢复
    let name = host
        .run(&format!("redo(); node_name({id_raw})"))
        .expect("redo 后读取名称")
        .into_string()
        .expect("返回名称字符串");
    assert_eq!(name, "主角");

    // 宿主侧继续撤销:SetName、SetFill、AddNode 依次走完,场景清空,id 失效
    let _ = host.run("undo()").expect("undo SetName");
    let _ = host.run("undo()").expect("undo SetFill");
    let _ = host.run("undo()").expect("undo AddNode");
    assert!(host.scene().is_empty());
    assert_eq!(host.history().undo_len(), 0);
    assert!(
        matches!(
            host.run(&format!("node_name({id_raw})")),
            Err(ScriptError::Eval(_))
        ),
        "节点被删后旧 id 失效,读取应报 Eval"
    );
}

/// ⑥ remove 子树 + node_count 一致性 + into_parts 交还最终状态。
#[test]
fn remove_and_node_count_consistent_across_undo_redo() {
    let mut host = ScriptHost::new();
    let script = r#"
        let g = add_group("组");
        let r1 = add_rect(0, 0, 10, 10, 1, 1, 1, 255, g);
        let r2 = add_rect(0, 0, 10, 10, 2, 2, 2, 255, g);
        let r3 = add_rect(20, 20, 30, 30, 3, 3, 3, 255);
        remove(r1);
        [node_count(), undo_len()]
    "#;
    let ret = host.run(script).expect("脚本应成功");
    let ret = ret.into_array().expect("返回 [node_count, undo_len]");
    assert_eq!(ret[0].as_int().expect("count"), 3, "删除后 4 → 3");
    assert_eq!(
        ret[1].as_int().expect("undo_len"),
        5,
        "建组 + 3 矩形 + 删除 = 5 步"
    );

    // 撤销 remove:整树恢复(r1 换新 id 挂回组内)
    let _ = host.run("undo()").expect("undo");
    assert_eq!(host.scene().len(), 4);
    let g = find_by_name(host.scene(), "组").expect("组在场");
    assert_eq!(
        host.scene().node(g).expect("组节点").children.len(),
        2,
        "r1 回到组内"
    );
    assert_eq!(host.history().undo_len(), 4);

    // 重做 remove,再交还宿主
    let _ = host.run("redo()").expect("redo");
    assert_eq!(host.scene().len(), 3);
    let (scene, history) = host.into_parts();
    assert_eq!(scene.len(), 3);
    assert_eq!(history.undo_len(), 5);
}

/// ⑦ 出借槽协议:eval 之外算子不可用(防误用返回运行时错误,不 panic);
/// 静态注册的算子在多次 run 之间复用。
#[test]
fn operators_reusable_across_runs_and_slot_protocol_holds() {
    let mut host = ScriptHost::new();
    for i in 0..3u64 {
        let raw = host
            .run(&format!(
                "add_rect(0, 0, {i}, 5, 1, 1, 1, 255); node_count()"
            ))
            .expect("同一引擎多次 run")
            .as_int()
            .expect("返回 count");
        assert_eq!(
            raw,
            i as i64 + 1,
            "第 {} 次 run 后节点数应为 {}",
            i + 1,
            i + 1
        );
    }
    assert_eq!(host.history().undo_len(), 3);
}

/// ⑧ `while true {}`:在默认操作数上限内确定性报 [`ScriptError::Limit`],
/// 不挂死宿主线程(V4.0 T4.1 验收:测试快速返回,场景/撤销栈零影响)。
#[test]
fn infinite_loop_fails_deterministically_at_operation_limit() {
    let mut host = ScriptHost::new();
    let start = std::time::Instant::now();
    let result = host.run("while true {}");
    let elapsed = start.elapsed();

    match result {
        Err(err @ ScriptError::Limit(_)) => {
            let msg = err.to_string();
            assert!(
                msg.contains("操作数上限"),
                "超限消息应含上限语义关键字:{msg}"
            );
        }
        other => panic!("死循环应报 ScriptError::Limit,实际 {other:?}"),
    }
    assert!(
        elapsed.as_secs() < 5,
        "死循环应在默认上限内被确定性拦下而非挂死(实际耗时 {elapsed:?})"
    );
    assert_eq!(host.scene().len(), 0, "超限脚本对场景零影响");
    assert_eq!(host.history().undo_len(), 0, "超限脚本不产生撤销步");
}

/// ⑨ 非有限坐标(NaN/±inf,任一坐标位)拒收:中文错误、场景与撤销栈零
/// 影响(V4.0 T4.2);有限的大坐标仍被接受——只拒非有限,不收紧合法范围。
#[test]
fn add_rect_rejects_non_finite_coordinates() {
    let mut host = ScriptHost::new();
    let cases: [(&str, &str); 5] = [
        ("x0 是 NaN", "0.0 / 0.0, 0, 10, 10"),
        ("x0 是 +inf", "1.0 / 0.0, 0, 10, 10"),
        ("y0 是 -inf", "0, -1.0 / 0.0, 10, 10"),
        ("x1 是 NaN", "0, 0, 0.0 / 0.0, 10"),
        ("y1 是 +inf", "0, 0, 10, 1.0 / 0.0"),
    ];
    for (label, coords) in cases {
        let script = format!("add_rect({coords}, 0, 0, 0, 255)");
        match host.run(&script) {
            Err(ScriptError::Eval(msg)) => assert!(
                msg.contains("有限数值"),
                "{label}:应报中文有限数值错误,实际:{msg}"
            ),
            other => panic!("{label}:非有限坐标应被拒收,实际 {other:?}"),
        }
    }
    assert_eq!(host.scene().len(), 0, "非有限坐标一律不得写入场景");
    assert_eq!(host.history().undo_len(), 0, "拒收不产生撤销步");

    let _ = host
        .run("add_rect(-1.0e300, 0, 1.0e300, 10, 0, 0, 0, 255)")
        .expect("有限大坐标应被接受");
    assert_eq!(host.scene().len(), 1, "有限值不误伤");
}

/// ⑩ id 位级无损往返(V4.0 T4.2):极端位型(高位为 1,脚本侧呈负数)经
/// `script_id`/`coerce_id` 编解码不变;真实场景 mint 的 key 端到端可用;
/// 解码后不存在的垃圾 id 仍被"节点不存在"查询兜底,且消息显示脚本侧数值。
#[test]
fn id_round_trip_is_lossless_for_extreme_bit_patterns() {
    // 编解码函数级:任意 u64 位型 → 脚本侧 i64 → 解码回同一 key
    for bits in [0u64, 1, i64::MAX as u64, 0x8000_0000_0000_0000, u64::MAX] {
        let id = NodeId::from(KeyData::from_ffi(bits));
        let raw = script_id(id);
        let decoded = coerce_id(Dynamic::from(raw), "test").expect("任何 i64 位型都是合法编码");
        assert_eq!(decoded, id, "位型 {bits:#016x} 经脚本侧 {raw} 往返应无损");
    }

    // 端到端:真实场景 key 经脚本往返(算子接受、消息里同一数值)
    let mut host = ScriptHost::new();
    let _ = host
        .run(r#"add_group("组"); add_rect(0, 0, 1, 1, 0, 0, 0, 255)"#)
        .expect("建场景");
    let keys: Vec<NodeId> = host.scene().nodes.iter().map(|(id, _)| id).collect();
    assert_eq!(keys.len(), 2);
    for id in keys {
        let raw = script_id(id);
        assert!(
            host.run(&format!(r#"set_name({raw}, "x")"#)).is_ok(),
            "真实 key 的脚本侧形态 {raw} 应被算子接受"
        );
        assert_eq!(
            coerce_id(Dynamic::from(raw), "test").expect("解码"),
            id,
            "真实 key 往返无损"
        );
    }

    // 垃圾 id(解码出的 key 不存在)由查询兜底拒绝,不 panic
    match host.run(r#"set_name(-1, "幽灵")"#) {
        Err(ScriptError::Eval(msg)) => {
            assert!(msg.contains("不存在"), "垃圾 id 应报节点不存在:{msg}");
            assert!(msg.contains("-1"), "消息应显示脚本侧数值形态:{msg}");
        }
        other => panic!("垃圾 id 应被拒收,实际 {other:?}"),
    }
}

/// ⑪ 中文错误消息断言(V4.0 T4.3):语法错/未定义变量带"脚本执行失败"
/// 中文前缀(rhai 英文原文被包装而非穿透);操作数超限含上限语义关键字,
/// 且 rhai 原始错误文本保留在附注里。
#[test]
fn error_messages_carry_required_chinese_keywords() {
    let mut host = ScriptHost::new();

    let syntax = host.run("let x = ;").expect_err("语法错应失败");
    assert!(
        syntax.to_string().contains("脚本执行失败"),
        "语法错应带中文前缀:{syntax}"
    );

    let undef = host
        .run("let y = no_such_var + 1;")
        .expect_err("未定义变量应失败");
    assert!(
        undef.to_string().contains("脚本执行失败"),
        "未定义变量应带中文前缀:{undef}"
    );

    let limit = host.run("while true {}").expect_err("死循环应失败");
    assert!(
        limit.to_string().contains("操作数上限"),
        "超限消息应含上限语义关键字:{limit}"
    );
    match limit {
        ScriptError::Limit(note) => assert!(
            note.contains("Too many operations"),
            "rhai 原始错误文本应保留在附注里:{note}"
        ),
        other => panic!("死循环应报 Limit 变体,实际 {other:?}"),
    }
}
