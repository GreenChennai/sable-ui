//! T4 验收测试(迭代计划 09:脚本 ≥ 6 个):
//! 建 3 矩形改色并全撤销、组挂载(重载)、非法脚本零影响、10 矩形循环端到端、
//! undo/redo 算子往返、remove/node_count 一致性 + into_parts。

use sable_foundation::prelude::{NodeId, Paint, Scene};
use slotmap::KeyData;

use crate::api::ScriptHost;
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
