//! 脚本宿主:Rhai `Engine` + 场景算子注册。
//!
//! # 状态出借机制(本模块的核心设计)
//!
//! rhai 的 `register_fn` 闭包必须 `Send + Sync + 'static`,无法借用
//! `ScriptHost` 自身的 `&mut` 状态;而算子又要同时改 [`History`] 与
//! [`Scene`]。解法:**注册一次**捕获 `Arc<Mutex<Option<HostState>>>` 出借槽,
//! `run` 期间把自有的 history/scene 移入槽内,算子经槽访问;eval 结束后
//! 无论成败都收回。槽在 eval 之外恒为 `None`,算子在此时被调用只会得到
//! 运行时错误(防误用,不 panic)。
//!
//! # 安全边界
//!
//! - 不注册任何文件/网络/进程算子;rhai 默认标准包本身零 IO。
//! - 算子闭包不 panic:一切可失败路径都返回 `Result`(rhai 转成脚本
//!   运行时错误);互斥锁毒化直接接管(`PoisonError::into_inner`)。
//! - 参数一律收 `Dynamic` 再手工 coerce:rhai 对原生函数参数**不做**
//!   INT→FLOAT 自动转换(1.26.1 源码 `src/func/call.rs` 的通配符分发只
//!   匹配 `Dynamic` 参数),整数字面量坐标脚本占多数,收 `Dynamic` 才能同时
//!   接受 `add_rect(0,…)` 与 `add_rect(0.5,…)`;类型不符给中文
//!   [`ScriptError::Cast`] 消息而非 rhai 的 "Function not found"。

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use kurbo::BezPath;
use rhai::{Dynamic, Engine, EvalAltResult, Position};
use sable_foundation::prelude::{
    AddNode, Command, History, Node, NodeContent, NodeId, Paint, PathNode, RemoveNode, Rgba8,
    Scene, SetFill, SetName,
};
use slotmap::{Key, KeyData};

use crate::error::{ScriptError, ScriptResult};

/// `run` 期间出借给算子闭包的可变状态。
struct HostState {
    history: History,
    scene: Scene,
}

/// 出借槽:`run` 期间为 `Some`,其余时刻为 `None`。
type SharedState = Arc<Mutex<Option<HostState>>>;

// —— 参数 coerce(rhai Dynamic → 宿主类型,错误消息面向脚本作者)——

fn coerce_f64(v: Dynamic, what: &str) -> ScriptResult<f64> {
    v.as_float()
        .or_else(|_| v.as_int().map(|i| i as f64))
        .map_err(|_| ScriptError::Cast(format!("{what} 需要数值,实际是 {}", v.type_name())))
}

/// 四个坐标(接受整数/浮点任意混合)。
fn coerce_f64x4(v: [Dynamic; 4], what: &str) -> ScriptResult<[f64; 4]> {
    let mut out = [0.0f64; 4];
    for (i, d) in v.into_iter().enumerate() {
        out[i] = coerce_f64(d, what)?;
    }
    Ok(out)
}

/// 一个颜色通道:接受数值,四舍五入后钳到 0..=255(与设计软件取色器一致,
/// 拒绝越界值对脚本作者反而难用)。
fn coerce_color(v: Dynamic, what: &str) -> ScriptResult<u8> {
    let x = coerce_f64(v, what)?;
    Ok(x.round().clamp(0.0, 255.0) as u8)
}

fn coerce_rgba8(v: [Dynamic; 4]) -> ScriptResult<Rgba8> {
    let channels = ["r", "g", "b", "a"];
    let mut out = [0u8; 4];
    for (i, d) in v.into_iter().enumerate() {
        out[i] = coerce_color(d, channels[i])?;
    }
    Ok(out)
}

/// 节点 id:脚本侧是 `i64`(由算子返回值给出),宿主侧经 slotmap ffi 表示
/// 还原为 [`NodeId`]。负数直接拒绝(合法 id 的 ffi 表示最高位不会为负)。
fn coerce_id(v: Dynamic, op: &str) -> ScriptResult<NodeId> {
    let raw = v.as_int().map_err(|_| {
        ScriptError::Cast(format!("{op}: 节点 id 需要整数,实际是 {}", v.type_name()))
    })?;
    if raw < 0 {
        return Err(ScriptError::Eval(format!(
            "{op}: 节点 id 不能为负数({raw})"
        )));
    }
    Ok(NodeId::from(KeyData::from_ffi(raw as u64)))
}

fn coerce_name(v: Dynamic, op: &str) -> ScriptResult<String> {
    let tn = v.type_name().to_string();
    v.into_string()
        .map_err(|_| ScriptError::Cast(format!("{op}: 名称需要字符串,实际是 {tn}")))
}

/// 节点 id 的稳定显示形态(slotmap key 的 ffi 表示)。
fn id_str(id: NodeId) -> u64 {
    id.data().as_ffi()
}

/// 把宿主侧错误转成 rhai 脚本运行时错误(无源码定位,定位由 rhai 调用栈补充)。
fn script_err(msg: impl Into<String>) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(
        Dynamic::from(msg.into()),
        Position::NONE,
    ))
}

/// 在出借槽上执行一个算子。eval 之外被调用(槽为空)返回运行时错误。
fn with_state<T>(
    slot: &SharedState,
    f: impl FnOnce(&mut HostState) -> ScriptResult<T>,
) -> Result<T, Box<EvalAltResult>> {
    let mut guard = lock_state(slot);
    let Some(state) = guard.as_mut() else {
        return Err(script_err("宿主状态缺失:算子只应在脚本执行期间被调用"));
    };
    f(state).map_err(|e| script_err(e.to_string()))
}

/// 加锁并接管毒化:算子闭包不 panic,锁内状态天然一致;真被外部 panic 毒死
/// 时也不该把后续脚本一并锁死。
fn lock_state(slot: &SharedState) -> MutexGuard<'_, Option<HostState>> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

impl HostState {
    /// 唯一的写入口:一切算子都组 Command 后 exec 进 History
    /// (AGENTS.md §3.1:直接改 Scene 的路径不允许存在)。
    fn exec(&mut self, cmd: impl Command) -> ScriptResult<()> {
        self.history.exec(cmd.boxed(), &mut self.scene);
        Ok(())
    }

    /// 添加节点(尾部追加)并取回新 id。`AddNode` 的 id 在 exec 内 apply 时
    /// 才发号且命令已入栈,故从场景端取回:roots 尾 / 父 children 尾。
    fn add_node(&mut self, parent: Option<NodeId>, node: Node, op: &str) -> ScriptResult<i64> {
        if let Some(p) = parent
            && self.scene.node(p).is_none()
        {
            return Err(ScriptError::Eval(format!(
                "{op}: 父节点 {} 不存在",
                id_str(p)
            )));
        }
        self.exec(AddNode::new(parent, None, node))?;
        let id = match parent {
            Some(p) => self.scene.node(p).and_then(|n| n.children.last()).copied(),
            None => self.scene.roots.last().copied(),
        };
        id.map(|id| id_str(id) as i64).ok_or_else(|| {
            ScriptError::Eval(format!("{op}: 添加成功但未能取回新节点 id(场景结构异常)"))
        })
    }

    /// `add_rect(x0, y0, x1, y1, r, g, b, a[, parent]) -> i64`
    fn add_rect(
        &mut self,
        coords: [Dynamic; 4],
        rgba: [Dynamic; 4],
        parent: Option<Dynamic>,
    ) -> ScriptResult<i64> {
        let [x0, y0, x1, y1] = coerce_f64x4(coords, "add_rect: 矩形坐标")?;
        let color = coerce_rgba8(rgba)?;
        let parent = match parent {
            Some(v) => Some(coerce_id(v, "add_rect")?),
            None => None,
        };
        // 四边矩形路径,与 UI 矩形工具/测试夹具同构;不规范化坐标顺序
        // (负向矩形按原样入路径,规范化是交互层的职责)。
        let mut path = BezPath::new();
        path.move_to((x0, y0));
        path.line_to((x1, y0));
        path.line_to((x1, y1));
        path.line_to((x0, y1));
        path.close_path();
        let content = NodeContent::Path(PathNode {
            path,
            fill: Some(Paint::Solid(color)),
            stroke: None,
        });
        self.add_node(parent, Node::new("矩形", content), "add_rect")
    }

    /// `add_group(name[, parent]) -> i64`
    fn add_group(&mut self, name: Dynamic, parent: Option<Dynamic>) -> ScriptResult<i64> {
        let name = coerce_name(name, "add_group")?;
        let parent = match parent {
            Some(v) => Some(coerce_id(v, "add_group")?),
            None => None,
        };
        self.add_node(parent, Node::new(name, NodeContent::Group), "add_group")
    }

    /// `set_fill(id, r, g, b, a)`(SetFill:同节点连续调用随 History merge
    /// 合并为一步撤销)。
    fn set_fill(&mut self, id: Dynamic, rgba: [Dynamic; 4]) -> ScriptResult<()> {
        let id = coerce_id(id, "set_fill")?;
        let color = coerce_rgba8(rgba)?;
        match self.scene.node(id) {
            None => Err(ScriptError::Eval(format!(
                "set_fill: 节点 {} 不存在",
                id_str(id)
            ))),
            Some(node) if !matches!(node.content, NodeContent::Path(_)) => Err(ScriptError::Eval(
                format!("set_fill: 节点 {} 不是路径节点,没有填充", id_str(id)),
            )),
            Some(_) => {
                // 校验先行,失败不产生空撤销步
                let old = self.scene.path(id).and_then(|p| p.fill.clone());
                self.exec(SetFill {
                    id,
                    old,
                    new: Some(Paint::Solid(color)),
                })
            }
        }
    }

    /// `set_name(id, name)`
    fn set_name(&mut self, id: Dynamic, name: Dynamic) -> ScriptResult<()> {
        let id = coerce_id(id, "set_name")?;
        let name = coerce_name(name, "set_name")?;
        let Some(node) = self.scene.node(id) else {
            return Err(ScriptError::Eval(format!(
                "set_name: 节点 {} 不存在",
                id_str(id)
            )));
        };
        let old = node.name.clone();
        self.exec(SetName { id, old, new: name })
    }

    /// `remove(id)`(RemoveNode:apply 时捕获整棵子树,撤销整树恢复,
    /// 恢复后节点换发新 id——见 command.rs 的 id 身份约定)。
    fn remove(&mut self, id: Dynamic) -> ScriptResult<()> {
        let id = coerce_id(id, "remove")?;
        if self.scene.node(id).is_none() {
            return Err(ScriptError::Eval(format!(
                "remove: 节点 {} 不存在",
                id_str(id)
            )));
        }
        self.exec(RemoveNode::new(id))
    }

    /// `undo()` / `redo()`:脚本与 UI 共用同一撤销栈。
    fn undo(&mut self) -> ScriptResult<()> {
        self.history.undo(&mut self.scene);
        Ok(())
    }

    fn redo(&mut self) -> ScriptResult<()> {
        self.history.redo(&mut self.scene);
        Ok(())
    }

    /// `undo_len() -> i64`:撤销栈深度。
    fn undo_len(&self) -> ScriptResult<i64> {
        Ok(self.history.undo_len() as i64)
    }

    /// `node_count() -> i64`:存活节点总数(含组)。
    fn node_count(&self) -> ScriptResult<i64> {
        Ok(self.scene.len() as i64)
    }

    /// `node_name(id) -> String`:只读辅助算子(控制台读场景用)。
    fn node_name(&self, id: Dynamic) -> ScriptResult<String> {
        let id = coerce_id(id, "node_name")?;
        self.scene
            .node(id)
            .map(|n| n.name.clone())
            .ok_or_else(|| ScriptError::Eval(format!("node_name: 节点 {} 不存在", id_str(id))))
    }
}

/// 脚本宿主:Rhai 沙盒 + 场景/撤销栈的所有者。
///
/// 构造后经 [`ScriptHost::run`] 执行脚本;脚本算子全部经 Command 进
/// [`History`],撤销重做与 UI 共用同一套语义。
pub struct ScriptHost {
    engine: Engine,
    slot: SharedState,
    history: History,
    scene: Scene,
}

impl Default for ScriptHost {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptHost {
    /// 构造宿主并一次性注册全部场景算子(注册的闭包只捕获出借槽)。
    pub fn new() -> Self {
        let mut engine = Engine::new();
        let slot: SharedState = Arc::new(Mutex::new(None));
        register_operators(&mut engine, &slot);
        ScriptHost {
            engine,
            slot,
            history: History::new(),
            scene: Scene::new(),
        }
    }

    /// 执行一段脚本,返回脚本末表达式的值。
    ///
    /// # 脚本语义(console 口径,对标 Blender Python console)
    ///
    /// - 算子逐条生效:错误**之前**已执行的算子留在撤销栈里,可经 `undo()`
    ///   或宿主侧撤销恢复;纯错误脚本(未执行任何算子)对场景零影响。
    /// - 每个算子一步撤销;`set_fill` 同节点连续调用随 History merge 合并。
    /// - 撤销删除后恢复的节点**换发新 id**(slotmap 约定),脚本持有的旧 id
    ///   随之失效,与 UI 撤销的既定语义一致。
    pub fn run(&mut self, script: &str) -> ScriptResult<Dynamic> {
        // 防御:上一次 run 若因 panic 中断未归还状态,先收回,避免本次覆盖丢状态
        if let Some(state) = lock_state(&self.slot).take() {
            self.history = state.history;
            self.scene = state.scene;
        }
        // 出借:history/scene 移入槽,算子闭包经 Arc<Mutex<_>> 访问
        *lock_state(&self.slot) = Some(HostState {
            history: std::mem::take(&mut self.history),
            scene: std::mem::take(&mut self.scene),
        });
        let result = self.engine.eval::<Dynamic>(script);
        // 归还(eval 无论成败都执行)
        match lock_state(&self.slot).take() {
            Some(state) => {
                self.history = state.history;
                self.scene = state.scene;
            }
            None => {
                // 理论不可达(上文刚放入);保守重建空态而非 panic(pub API 纪律)
                self.history = History::new();
                self.scene = Scene::new();
            }
        }
        result.map_err(|e| ScriptError::Eval(e.to_string()))
    }

    /// 只读访问场景。
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// 只读访问撤销栈。
    pub fn history(&self) -> &History {
        &self.history
    }

    /// 取出最终状态(丢弃 engine):主程序接管场景与历史继续编辑。
    pub fn into_parts(self) -> (Scene, History) {
        (self.scene, self.history)
    }

    /// 调整沙盒资源限制(执行步数/递归深度/表达式深度等)。
    ///
    /// 默认沿用 rhai 出厂限制(表达式深度与调用层级有界;操作数不限)。
    /// 需要更严的沙盒可在此收紧,例如:
    /// `host.engine_mut().set_max_operations(10_000)`(0 = 不限)。
    ///
    /// 注意:不要经此注册文件/网络类包,那会击穿沙盒承诺。
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }
}

/// 注册全部场景算子(13 个签名 / 12 个脚本名;可选参数 = 同名不同元数重载,
/// rhai 按名字 + 参数个数分发,1.26.1 已核实)。
fn register_operators(engine: &mut Engine, slot: &SharedState) {
    // —— 结构:添加 ——
    // add_rect(x0, y0, x1, y1, r, g, b, a) -> i64
    engine.register_fn("add_rect", {
        let slot = slot.clone();
        move |x0: Dynamic,
              y0: Dynamic,
              x1: Dynamic,
              y1: Dynamic,
              r: Dynamic,
              g: Dynamic,
              b: Dynamic,
              a: Dynamic|
              -> Result<i64, Box<EvalAltResult>> {
            with_state(&slot, |st| {
                st.add_rect([x0, y0, x1, y1], [r, g, b, a], None)
            })
        }
    });
    // add_rect(…, parent) -> i64:挂到指定组下(可选父节点重载)
    engine.register_fn("add_rect", {
        let slot = slot.clone();
        move |x0: Dynamic,
              y0: Dynamic,
              x1: Dynamic,
              y1: Dynamic,
              r: Dynamic,
              g: Dynamic,
              b: Dynamic,
              a: Dynamic,
              parent: Dynamic|
              -> Result<i64, Box<EvalAltResult>> {
            with_state(&slot, |st| {
                st.add_rect([x0, y0, x1, y1], [r, g, b, a], Some(parent))
            })
        }
    });
    // add_group(name) -> i64
    engine.register_fn("add_group", {
        let slot = slot.clone();
        move |name: Dynamic| -> Result<i64, Box<EvalAltResult>> {
            with_state(&slot, |st| st.add_group(name, None))
        }
    });
    // add_group(name, parent) -> i64:组嵌组
    engine.register_fn("add_group", {
        let slot = slot.clone();
        move |name: Dynamic, parent: Dynamic| -> Result<i64, Box<EvalAltResult>> {
            with_state(&slot, |st| st.add_group(name, Some(parent)))
        }
    });

    // —— 结构:修改/删除 ——
    // set_fill(id, r, g, b, a)
    engine.register_fn("set_fill", {
        let slot = slot.clone();
        move |id: Dynamic,
              r: Dynamic,
              g: Dynamic,
              b: Dynamic,
              a: Dynamic|
              -> Result<(), Box<EvalAltResult>> {
            with_state(&slot, |st| st.set_fill(id, [r, g, b, a]))
        }
    });
    // set_name(id, name)
    engine.register_fn("set_name", {
        let slot = slot.clone();
        move |id: Dynamic, name: Dynamic| -> Result<(), Box<EvalAltResult>> {
            with_state(&slot, |st| st.set_name(id, name))
        }
    });
    // remove(id)
    engine.register_fn("remove", {
        let slot = slot.clone();
        move |id: Dynamic| -> Result<(), Box<EvalAltResult>> {
            with_state(&slot, |st| st.remove(id))
        }
    });

    // —— 历史算子(与 UI 共用同一撤销栈)——
    engine.register_fn("undo", {
        let slot = slot.clone();
        move || -> Result<(), Box<EvalAltResult>> { with_state(&slot, HostState::undo) }
    });
    engine.register_fn("redo", {
        let slot = slot.clone();
        move || -> Result<(), Box<EvalAltResult>> { with_state(&slot, HostState::redo) }
    });
    engine.register_fn("undo_len", {
        let slot = slot.clone();
        move || -> Result<i64, Box<EvalAltResult>> { with_state(&slot, |st| st.undo_len()) }
    });

    // —— 只读查询 ——
    engine.register_fn("node_count", {
        let slot = slot.clone();
        move || -> Result<i64, Box<EvalAltResult>> { with_state(&slot, |st| st.node_count()) }
    });
    engine.register_fn("node_name", {
        let slot = slot.clone();
        move |id: Dynamic| -> Result<String, Box<EvalAltResult>> {
            with_state(&slot, |st| st.node_name(id))
        }
    });
}
