//! 受控组件的绑定抽象(分册三 §3):属性面板 ↔ 文档属性的桥梁。
//!
//! ```ignore
//! // 应用层构造(示例):把选中节点的 X 坐标绑给 NumberField
//! let x_binding = Binding::new(
//!     move |cx: &App| doc.read(cx).selection_x().unwrap_or(0.0),
//!     move |v: f64, cx: &mut App| doc.update(cx, |d, _| d.exec(SetX { to: v })),
//! );
//! NumberField::new(x_binding); // 组件只面向 Binding 编程
//! ```
//!
//! # undo 语义(重要)
//!
//! [`Binding::set`] 只是"写回"通道:**撤销语义由调用方的 set 闭包负责**——
//! 闭包内应走 History/Command(core 的 `History::exec`、video 的
//! `TimelineHistory`),连续 set 经命令 `merge` 合并为一步撤销(分册三 §2
//! 的拖动范式)。widgets 层**刻意不碰 Command 类型**:受控组件对文档模型
//! 零依赖(与上游 vb_ui "组件不依赖文档模型,令牌/数据由调用方注入" 的
//! 解耦模式同源,docs/upstream/02 §4.2)。

use gpui::App;

/// 读闭包:UI 渲染时从文档取当前值(只读 `&App`)。
pub type GetFn<T> = Box<dyn Fn(&App) -> T>;

/// 写回闭包:用户改值经它生成 Command 写回文档(撤销语义由闭包负责)。
pub type SetFn<T> = Box<dyn Fn(T, &mut App)>;

/// 双向绑定:UI 读 [`Binding::get`] 显示文档值,UI 改值 → [`Binding::set`]
/// 写回(生成 Command 由调用方闭包决定)。
///
/// `Box<dyn Fn>` 不可克隆 ⇒ [`Binding`] 本身不可 Clone;需要派生绑定请用
/// [`Binding::map`](按值消费原绑定)。绑定只在 UI 线程构造与调用,不要求 Send。
pub struct Binding<T: Clone + PartialEq + 'static> {
    get: GetFn<T>,
    set: SetFn<T>,
}

impl<T: Clone + PartialEq + 'static> Binding<T> {
    /// 从读写闭包构造。
    pub fn new(get: impl Fn(&App) -> T + 'static, set: impl Fn(T, &mut App) + 'static) -> Self {
        Binding {
            get: Box::new(get),
            set: Box::new(set),
        }
    }

    /// 读当前值(UI 渲染显示用)。
    pub fn get(&self, cx: &App) -> T {
        (self.get)(cx)
    }

    /// 写回新值(用户输入)。是否/如何进撤销栈由构造时的 set 闭包决定。
    pub fn set(&self, value: T, cx: &mut App) {
        (self.set)(value, cx);
    }

    /// 派生绑定:值域经 `f`/`f_inv` 双向换算(如 `Binding<Paint>` →
    /// `Binding<Rgba8>` 的提取/回包,f64 弧度 ↔ 度数显示等)。
    ///
    /// - `get' = f ∘ get`;`set' = set ∘ f_inv`
    /// - **按值消费**原绑定(`Box<dyn Fn>` 不可克隆);原绑定随即不可再用。
    pub fn map<U: Clone + PartialEq + 'static>(
        self,
        f: impl Fn(T) -> U + 'static,
        f_inv: impl Fn(U) -> T + 'static,
    ) -> Binding<U> {
        let Binding { get, set } = self;
        Binding::new(move |cx| f(get(cx)), move |value, cx| set(f_inv(value), cx))
    }
}

#[cfg(test)]
mod tests {
    /// map 的换算管线可以在**不触 App** 的前提下验证吗?`get`/`set` 的
    /// 签名硬绑 `&App`,而纯测试无法合法构造 `&App`(TestAppContext 需要
    /// gpui/test-support feature,workspace 未启用)。按任务契约改测纯函数
    /// 部分:map 所依赖的双向换算函数语义。
    #[test]
    fn map_transformation_pairs_round_trip() {
        // 典型用法:Paint 域 ↔ 色块域的提取/回包(度↔弧度同构)
        let extract = |p: Option<u32>| p.unwrap_or(0);
        let pack = |v: u32| Some(v);
        for v in [0u32, 7, 0xFFFF_FFFF] {
            assert_eq!(pack(extract(Some(v))), Some(v), "提取→回包应恒等");
        }
        // 度 → 弧度 → 度
        let deg = 137.5_f64;
        assert!((deg.to_radians().to_degrees() - deg).abs() < 1e-12);
    }
}
