//! `.sable` 工程文件存取闭环(迭代计划 08 S3 #3.4;分册五 §5.1)。
//!
//! # 格式:单文件 MessagePack 信封
//!
//! ```text
//! .sable = b"SABL"(4 字节魔数)+ u32 LE 版本号 + rmp-serde(ProjectData)
//! ```
//!
//! 分册五 §5.1 原文规划的是 **zip 容器**(manifest / document / timeline /
//! assets 分文件)。v0.1 采用单流 MessagePack 信封,理由:M2 只有单场景 +
//! 撤销栈,没有多素材与时间轴,zip 的按需加载与素材 CAS 寻址尚无用武之地,
//! 且 core 的依赖白名单(分册六 §2.1)不为容器多背一个 zip 依赖。多素材
//! (M4)引入 zip 时,本信封整体迁作 document 流,魔数 + 版本头的校验
//! 逻辑原样保留。
//!
//! # 版本前向兼容(分册五 §5.1、分册六 TD-05)
//!
//! 头部版本高于 [`SABLE_VERSION`] 直接拒开([`CoreError::UnsupportedVersion`],
//! 提示用户升级程序);同版本内**加新字段一律 `#[serde(default)]`**——老
//! 文件缺字段走默认值,写侧无需迁移脚本。`ProjectData.version` 与头部版本
//! 一致,供未来迁移逻辑取用。
//!
//! # 历史栈的存取边界
//!
//! - 保存点 = **已完成命令**:事务中(in-flight)的命令尚未进入撤销栈,
//!   不保存(见 [`History::to_serialized`]);
//! - 第三方命令(非内置 `Command`)跳过不阻塞保存,重开后撤销栈**部分
//!   恢复**(见 [`crate::command::SerializedCommand`])。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::command::{History, SerializedHistory};
use crate::error::{CoreError, CoreResult};
use crate::persistence::atomic_write;
use crate::scene::Scene;

/// 魔数:`.sable` 文件头 4 字节。
pub const SABLE_MAGIC: &[u8; 4] = b"SABL";

/// 当前格式版本(v0.1 = 1)。
pub const SABLE_VERSION: u32 = 1;

/// 魔数字节长度。
pub const SABLE_MAGIC_LEN: usize = 4;

/// 文件头总长 = 魔数(4)+ LE u32 版本(4)。
const HEADER_LEN: usize = SABLE_MAGIC_LEN + 4;

/// `.sable` 工程数据根容器:版本 + 场景 + 撤销/重做栈。
///
/// 未来加字段一律 `#[serde(default)]`(见模块 doc"版本前向兼容")。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectData {
    pub version: u32,
    pub scene: Scene,
    pub history: SerializedHistory,
}

/// 保存工程:场景 + 已完成历史原子写入 `.sable`,返回写入总字节数
/// (头部 + MessagePack 数据体)。
///
/// 原子写([`atomic_write`]):先写临时文件 fsync 再 rename,写一半断电也
/// 不会得到半个文件。第三方命令的跳过数在此忽略(不阻塞保存;需要精确
/// 计数时直接调 [`History::to_serialized`])。
pub fn save_project(path: &Path, scene: &Scene, history: &History) -> CoreResult<usize> {
    let (history, _skipped) = history.to_serialized();
    let data = ProjectData {
        version: SABLE_VERSION,
        scene: scene.clone(),
        history,
    };
    let payload = rmp_serde::to_vec(&data)
        .map_err(|err| CoreError::Deserialization(format!("ProjectData 编码失败: {err}")))?;
    let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
    bytes.extend_from_slice(SABLE_MAGIC);
    bytes.extend_from_slice(&SABLE_VERSION.to_le_bytes());
    bytes.extend_from_slice(&payload);
    atomic_write(path, &bytes)?;
    Ok(bytes.len())
}

/// 读取工程:校验魔数与版本后反序列化出 [`ProjectData`]。
///
/// 防御次序(错误各归其位):
/// 1. 文件不足一个头 → [`CoreError::Deserialization`](截断文件);
/// 2. 魔数不符 → [`CoreError::InvalidMagic`](不是 `.sable`);
/// 3. 头部版本过新 → [`CoreError::UnsupportedVersion`];
/// 4. 数据体 MessagePack 损坏 → [`CoreError::Deserialization`];
/// 5. 数据体版本过新 → 同 3(头部损坏但数据体完好时的兜底)。
pub fn load_project(path: &Path) -> CoreResult<ProjectData> {
    let bytes = fs::read(path)?;
    if bytes.len() < HEADER_LEN {
        return Err(CoreError::Deserialization(format!(
            "文件只有 {} 字节,不足 .sable 头部 {HEADER_LEN} 字节",
            bytes.len()
        )));
    }
    if bytes[..SABLE_MAGIC_LEN] != *SABLE_MAGIC {
        return Err(CoreError::InvalidMagic);
    }
    // 上文已校验 bytes.len() >= HEADER_LEN = SABLE_MAGIC_LEN + 4,此切片
    // 恒为 4 字节;仍按 RB-01 走结构化出口,不设"必然成功"假设。
    let header_bytes: [u8; 4] = bytes[SABLE_MAGIC_LEN..HEADER_LEN]
        .try_into()
        .map_err(|_| CoreError::Deserialization("头部版本字段不是 4 字节".into()))?;
    let header_version = u32::from_le_bytes(header_bytes);
    if header_version > SABLE_VERSION {
        return Err(CoreError::UnsupportedVersion {
            found: header_version,
            supported: SABLE_VERSION,
        });
    }
    let data: ProjectData = rmp_serde::from_slice(&bytes[HEADER_LEN..])
        .map_err(|err| CoreError::Deserialization(format!("数据体解码失败: {err}")))?;
    if data.version > SABLE_VERSION {
        return Err(CoreError::UnsupportedVersion {
            found: data.version,
            supported: SABLE_VERSION,
        });
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::command::{
        AddEffect, AddNode, Command, MoveEffect, RemoveNode, SerializedCommand, SetEffectEnabled,
        SetEffectSpec, SetFill, SetName, SetOpacity, SetVisibility,
    };
    use crate::effects::{EffectEntry, EffectSpec};
    use crate::error::CoreError;
    use crate::scene::{Node, NodeContent, NodeId, Paint, PathNode};
    use kurbo::BezPath;

    /// 每个测试独占临时目录:进程 id + 自增计数命名,零随机零 sleep。
    fn temp_dir(tag: &str) -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "sable-foundation-project-{}-{tag}-{serial}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    fn rect_content(x0: f64, y0: f64, x1: f64, y1: f64) -> NodeContent {
        let mut path = BezPath::new();
        path.move_to((x0, y0));
        path.line_to((x1, y0));
        path.line_to((x1, y1));
        path.line_to((x0, y1));
        path.close_path();
        NodeContent::Path(PathNode {
            path,
            fill: Some(Paint::Solid([200, 40, 40, 255])),
            stroke: None,
        })
    }

    /// 根组 → (矩形A, 子组 → 矩形B),外加根级矩形C(与 command.rs 场景同构)。
    fn demo_scene() -> (Scene, NodeId, NodeId, NodeId) {
        let mut scene = Scene::new();
        let root = scene
            .add_node(None, "根组", NodeContent::Group)
            .expect("add root");
        let a = scene
            .add_node(Some(root), "矩形A", rect_content(0.0, 0.0, 10.0, 10.0))
            .expect("add a");
        let sub = scene
            .add_node(Some(root), "子组", NodeContent::Group)
            .expect("add sub");
        scene
            .add_node(Some(sub), "矩形B", rect_content(20.0, 20.0, 30.0, 30.0))
            .expect("add b");
        let c = scene
            .add_node(None, "矩形C", rect_content(-5.0, -5.0, 5.0, 5.0))
            .expect("add c");
        (scene, a, sub, c)
    }

    fn write_bytes(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, bytes).expect("写测试文件");
        path
    }

    /// 第三方命令(非内置):验证 to_serialized 的跳过计数。
    struct ThirdPartyCommand {
        applied: bool,
    }

    impl Command for ThirdPartyCommand {
        fn apply(&mut self, _scene: &mut Scene) {
            self.applied = true;
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn name(&self) -> Cow<'static, str> {
            Cow::Borrowed("第三方命令")
        }
    }

    /// 3.4 验收金句:保存 → 重开 → **撤销栈仍可用**——全部撤销回到执行前
    /// 快照,redo 亦可用;覆盖 merge 合并、删子树、批量事务三种形态。
    #[test]
    fn save_load_roundtrip_full_undo_restores_snapshot() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("工程.sable");

        let (mut scene, a, sub, c) = demo_scene();
        let snapshot_before = scene.clone();
        let a_fill = scene.path(a).and_then(|p| p.fill.clone());

        // 混合命令:同节点连续 SetFill(merge 成一步)+ 删子树 + 批量事务一步
        let mut history = History::new();
        history.exec(
            SetFill {
                id: a,
                old: a_fill,
                new: Some(Paint::Solid([10, 200, 10, 255])),
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            SetFill {
                id: a,
                old: Some(Paint::Solid([10, 200, 10, 255])),
                new: Some(Paint::Solid([1, 2, 3, 255])),
            }
            .boxed(),
            &mut scene,
        );
        assert_eq!(history.undo_len(), 1, "同节点连续 SetFill 合并为一步");
        history.exec(RemoveNode::new(sub).boxed(), &mut scene);
        history.begin_transaction();
        history.exec(
            SetOpacity {
                id: c,
                old: 1.0,
                new: 0.5,
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            AddNode::new(None, None, Node::new("事务新增", NodeContent::Group)).boxed(),
            &mut scene,
        );
        history.end_transaction();
        assert_eq!(history.undo_len(), 3, "merge(1) + remove(1) + 事务(1)");
        let fully_edited = scene.clone();

        // 撤销一步(事务)再保存:顺带验证 redo 栈跨保存存活
        history.undo(&mut scene);
        assert_eq!(history.undo_len(), 2);
        let scene_at_save = scene.clone();

        let bytes = save_project(&path, &scene, &history).expect("保存");
        assert!(bytes > HEADER_LEN, "写入应含头部与数据体");
        // 原子写不残留临时文件(临时名含 pid,按扩展名扫描)
        let tmp_residue: Vec<PathBuf> = fs::read_dir(&dir)
            .expect("读目录")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "tmp"))
            .collect();
        assert!(
            tmp_residue.is_empty(),
            "原子写不残留临时文件: {tmp_residue:?}"
        );

        let data = load_project(&path).expect("读回");
        assert_eq!(data.version, SABLE_VERSION);
        assert_eq!(
            data.scene, scene_at_save,
            "场景结构化相等(slotmap 保键往返)"
        );

        let mut reopened = History::from_serialized(data.history);
        assert_eq!(reopened.undo_len(), 2);
        assert!(reopened.can_redo(), "redo 栈一并保存");

        // redo 一步 → 回到完整编辑态(AddNode 重放会换发新 id,结构化相等不受影响)
        reopened.redo(&mut scene);
        assert_eq!(scene, fully_edited, "redo 栈跨保存可用");

        // 金句:全部撤销 → 回到执行前快照
        while reopened.can_undo() {
            reopened.undo(&mut scene);
        }
        assert_eq!(
            scene, snapshot_before,
            "重开后撤销栈可用:全部撤销回到编辑前快照"
        );

        // 再全部重做/撤销一遍(覆盖 RemoveNode 恢复重映射在重开历史中的正确性)
        while reopened.can_redo() {
            reopened.redo(&mut scene);
        }
        assert_eq!(scene, fully_edited);
        while reopened.can_undo() {
            reopened.undo(&mut scene);
        }
        assert_eq!(scene, snapshot_before);
    }

    /// 效果命令跨保存存活(S4 #4.1 镜像验收):AddEffect/MoveEffect/
    /// SetEffectEnabled/SetEffectSpec(拖动 merge)保存 → 重开 → 全部撤销
    /// 回到编辑前快照、全部重做到编辑后状态(与 3.4 金句同标准)。
    #[test]
    fn effect_commands_survive_save_load() {
        let dir = temp_dir("effects");
        let path = dir.join("效果.sable");

        let (mut scene, a, _sub, _c) = demo_scene();
        let snapshot_before = scene.clone();
        let shadow = EffectEntry {
            spec: EffectSpec::DropShadow {
                blur: 4.0,
                offset: [0.0, 2.0],
                color: [0, 0, 0, 128],
            },
            enabled: true,
        };
        let blur = |radius: f64| EffectEntry {
            spec: EffectSpec::GaussianBlur { radius },
            enabled: true,
        };

        let mut history = History::new();
        history.exec(
            AddEffect {
                id: a,
                index: 0,
                entry: shadow,
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            AddEffect {
                id: a,
                index: 1,
                entry: blur(2.0),
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            MoveEffect {
                id: a,
                from: 1,
                to: 0,
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            SetEffectEnabled {
                id: a,
                index: 0,
                old: true,
                new: false,
            }
            .boxed(),
            &mut scene,
        );
        // 参数拖动:同节点同下标两条 SetEffectSpec 合并为一步
        history.exec(
            SetEffectSpec {
                id: a,
                index: 1,
                old: blur(2.0).spec,
                new: blur(4.0).spec,
            }
            .boxed(),
            &mut scene,
        );
        history.exec(
            SetEffectSpec {
                id: a,
                index: 1,
                old: blur(4.0).spec,
                new: blur(8.0).spec,
            }
            .boxed(),
            &mut scene,
        );
        assert_eq!(
            history.undo_len(),
            5,
            "add×2 + move + enable + merge(spec×2)"
        );
        let fully_edited = scene.clone();

        // 撤销一步再保存:顺带验证效果命令的 redo 栈跨保存存活
        history.undo(&mut scene);
        let scene_at_save = scene.clone();

        let bytes = save_project(&path, &scene, &history).expect("保存");
        assert!(bytes > HEADER_LEN);
        let data = load_project(&path).expect("读回");
        assert_eq!(data.scene, scene_at_save, "场景(含效果栈)无损往返");
        // 被撤销的效果命令进 redo 栈且以镜像形态存在
        assert!(
            matches!(&data.history.redo[0], SerializedCommand::SetEffectSpec(_)),
            "SetEffectSpec 必须以专属镜像变体保存"
        );

        let mut reopened = History::from_serialized(data.history);
        assert_eq!(reopened.undo_len(), 4);
        assert!(reopened.can_redo(), "redo 栈一并保存");

        reopened.redo(&mut scene);
        assert_eq!(scene, fully_edited, "redo 栈跨保存可用(效果参数恢复)");

        // 金句:全部撤销 → 回到编辑前快照
        while reopened.can_undo() {
            reopened.undo(&mut scene);
        }
        assert_eq!(scene, snapshot_before, "重开后撤销栈可用:效果命令逐条回退");
        assert!(scene.node(a).expect("a 在").effects.is_empty());

        // 再全部重做/撤销一遍
        while reopened.can_redo() {
            reopened.redo(&mut scene);
        }
        assert_eq!(scene, fully_edited);
        while reopened.can_undo() {
            reopened.undo(&mut scene);
        }
        assert_eq!(scene, snapshot_before);
    }

    /// 未知命令不阻塞保存:跳过并计数;内置命令与事务内嵌命令照常镜像。
    #[test]
    fn to_serialized_skips_unknown_commands_and_counts() {
        let (mut scene, a, _sub, _c) = demo_scene();
        let a_name = scene.node(a).expect("a 在").name.clone();
        let mut history = History::new();
        history.exec(
            SetVisibility {
                id: a,
                old: true,
                new: false,
            }
            .boxed(),
            &mut scene,
        );
        history.exec(Box::new(ThirdPartyCommand { applied: false }), &mut scene);
        history.exec(
            SetName {
                id: a,
                old: a_name,
                new: "改名A".into(),
            }
            .boxed(),
            &mut scene,
        );
        history.begin_transaction();
        history.exec(Box::new(ThirdPartyCommand { applied: false }), &mut scene);
        history.exec(
            SetOpacity {
                id: a,
                old: 1.0,
                new: 0.5,
            }
            .boxed(),
            &mut scene,
        );
        history.end_transaction();

        let (serialized, skipped) = history.to_serialized();
        assert_eq!(skipped, 2, "栈上 1 个 + 事务内 1 个第三方命令都跳过并计数");
        assert_eq!(serialized.undo.len(), 3, "SetVisibility + SetName + 事务");
        assert!(
            matches!(&serialized.undo[2], SerializedCommand::Batch(inner) if inner.len() == 1),
            "事务内被跳过后只余 SetOpacity"
        );

        // 反序列化重建后,剩余命令照常工作(第三方那两步不可恢复)
        let mut reopened = History::from_serialized(serialized);
        assert_eq!(reopened.undo_len(), 3);
        while reopened.can_undo() {
            reopened.undo(&mut scene);
        }
        assert_eq!(scene.node(a).expect("a 在").name, "矩形A", "SetName 已撤销");
        assert!(scene.node(a).expect("a 在").visible, "SetVisibility 已撤销");
    }

    #[test]
    fn bad_magic_is_rejected() {
        let dir = temp_dir("magic");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"NOPE");
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[0x80, 0x01]);
        let path = write_bytes(&dir, "bad.sable", &bytes);
        match load_project(&path) {
            Err(CoreError::InvalidMagic) => {}
            other => panic!("应 InvalidMagic,实际 {other:?}"),
        }
    }

    #[test]
    fn future_version_is_rejected() {
        let dir = temp_dir("version");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(SABLE_MAGIC);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&[0x80, 0x01]);
        let path = write_bytes(&dir, "future.sable", &bytes);
        match load_project(&path) {
            Err(CoreError::UnsupportedVersion { found, supported }) => {
                assert_eq!(found, u32::MAX);
                assert_eq!(supported, SABLE_VERSION);
            }
            other => panic!("应 UnsupportedVersion,实际 {other:?}"),
        }
    }

    #[test]
    fn truncated_files_are_rejected() {
        let dir = temp_dir("truncated");
        // 1) 连头都不全
        let short = write_bytes(&dir, "short.sable", b"LUM");
        match load_project(&short) {
            Err(CoreError::Deserialization(_)) => {}
            other => panic!("过短文件应 Deserialization,实际 {other:?}"),
        }

        // 2) 头部完好、数据体被截断
        let (mut scene, _a, _sub, _c) = demo_scene();
        let mut history = History::new();
        history.exec(
            AddNode::new(None, None, Node::new("新节点", NodeContent::Group)).boxed(),
            &mut scene,
        );
        let good = dir.join("good.sable");
        save_project(&good, &scene, &history).expect("存");
        let mut bytes = fs::read(&good).expect("读回");
        assert!(bytes.len() > HEADER_LEN + 4);
        bytes.truncate(bytes.len() - 4);
        let cut = write_bytes(&dir, "cut.sable", &bytes);
        match load_project(&cut) {
            Err(CoreError::Deserialization(_)) => {}
            other => panic!("截断数据体应 Deserialization,实际 {other:?}"),
        }
    }
}
