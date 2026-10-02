# 对标记录:luminaui.in "Neon Card" → Sable 组件补齐(V4.1 组件波次 · 1/1)

> 背景:V4.0"接线与诚实收口"收口后,以 Web 组件库 luminaui.in(恰为本库弃用旧名
> Lumina UI 的同名站点)的 Neon Card 页为靶子,检验本库"能否做 / 方便做 / 好看"。
> 结论先行:**八项视觉交互全部可做(一项等效档),此前"不方便"——没有现成组件,
> 本轮补齐 `sable-widgets::neon_card`,story 第 9 分组实机演示。**

---

## 1. 能力对照(对标源码逐项 → Sable 落点)

| # | Neon Card 效果(源码数值) | Sable 能力落点 | 判定 |
|---|---|---|---|
| 1 | 2px 流动渐变描边:linear-gradient(90deg,#FF0080→#7928CA→#FF0080),2s 无限循环 | `Paint::LinearGradient` + 端点锚定卡心、窗口每周期平移一卡宽(shimmer 纯函数) | ✅ 可做,已做 |
| 2 | 环境辉光:box-shadow 0 0 20px 2px @15% | `render_shadow_rgba`(V4.0 效果管线同源)+ 预乘域着色 | ✅ 可做,已做 |
| 3 | 光标跟随外辉光:1000px 径向,主色 15%→透明 40%,0.75→1 淡入 | `Paint::RadialGradient` + hover 进度插值;鼠标局部坐标由元素层捕获 | ✅ 可做,已做 |
| 4 | 卡体:rounded-2xl 玻璃拟态,slate-900/85,hover /70 | 半透明 `surface_2` × glass_alpha(hover 降透)+ 1px `border_strong` 内环。**真 backdrop-blur(GPU 后处理)仍是路线图项,等效档如实注明** | 🟡 等效档 |
| 5 | 光标跟随内高光:800px 白 8%→透明 40% | 同 #3,卡体路径作裁切 | ✅ 可做,已做 |
| 6 | hover 缩放 1.02(500ms)+ 入场 opacity/y 位移 | `Easing::OutCubic` 纯函数进度 → 缓冲变换 Affine;reduced_motion 直切 | ✅ 可做,已做 |
| 7 | hover 漂移粒子:10 颗 6-10px,3s 循环,0.2s 错峰,各带小辉光 | `particle_frame` 纯函数(种子派生,同种子逐位同帧)+ 径向渐变小辉光;动画引擎无需真开 10 timeline | ✅ 可做,已做 |
| 8 | 标题渐变文字(bg-clip-text) | story 演示用 token 纯色降级;字形轮廓渐变填充待文本管线接线 | 🟡 降级注明 |

## 2. "是否方便做"的答案:补齐前不方便,补齐后一行组件

对标前,八项效果的**底座**全在(渐变/效果管线/动画引擎/token),但没有任何组合件——
每个效果都要应用层自己拼。本轮 `crates/sable-widgets/src/neon_card.rs`(~1150 行,
10 测试)提供:

- `NeonCardStyle`(全参数默认值 = 对标数值;换配色只改 `gradient_stops` + `glow_color`)
- `NeonCardState` 纯函数状态机(hover/入场/粒子相位;`tick` 回报动画活动,帧泵
  空闲零帧提交——对比标组件的无限 CSS 循环是刻意的性能取舍,doc 注明)
- `render_neon_card`(纯函数像素渲染,不触 gpui/全局,可独立单测)
- `neon_card`(gpui 元素封装:`canvas` paint 阶段 `paint_image` 上屏 + hover/鼠标
  事件接线;位图缓存按 `frame_cache_key`,键稳定零重渲)

## 3. "是否好看"的保障

- 全部颜色走 `ColorTokens`(主题语义)或组件 style 参数(光效载荷),无散落魔法值;
- 像素级断言锁视觉:描边取色(相位 0.5 = 首停色)、卡体 α=0.85×255、辉光区非零且
  主色系、shimmer 半周期换色、光标象限亮度、缩放外接尺寸、粒子 hover 出现/空闲消失、
  入场 opacity 单调、reduced_motion 逐位直切(10 测试,158 全绿);
- 深浅两主题:卡体/内环走 surface_2/border_strong token 自适配,光效载荷色不随主题翻转;
- story 实机冒烟通过(启动 12s 无 panic;三卡:粉紫对标默认/青蓝/金橙)。

## 4. 诚实边界与后续

| 项 | 现状 | 后续 |
|---|---|---|
| 真 backdrop-blur | 等效档(半透明底+高光内边) | GPU 后处理管线(Dual Kawase),真机 Sprint |
| 渐变文字 | story 演示用纯色降级 | 字形轮廓 + 渐变 fill 接线 |
| shimmer 空闲态 | 帧泵停则相位冻结(缓存键稳定) | 若要"永远流动",宿主保持帧泵即可(性能自决) |
| widgets→paint 直连 | 新增内部依赖边(向下,合法) | 手册 §2.1 依赖图补记 |

*执行:组件代码中断续作(子 Agent 半成品),主会话修复五处编译 + 双重平移定位
bug(card_geometry 已返回缓冲坐标,to_buffer 不得再平移 PAD)+ shimmer 锚定卡心 +
入场测试期望对齐 OutCubic 曲线;story 演示页主会话补齐。门禁:fmt/clippy -D warnings/
cargo test(sable-widgets 158 全绿)/cargo check -p story 全过。*
