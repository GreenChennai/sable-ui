# 02 · VellumBench 上游 UI 分析(vb_ui 设计令牌/组件 → lumina-widgets 迁移)

> 分析基线:2026-10-02,`D:\Github\VellumBench` 工作区快照(v0.14.0,Cargo.toml workspace version)。
> 结论先行:**VellumBench 是"文档格式 = 标准 HTML/CSS"的桌面矢量绘台,UI 用 egui 0.35(e frame wgpu)自研组件层 vb_ui(12 文件 5378 行)。它对 Lumina 的价值是三样:①一套经过 WCAG AA 门禁与 CI 双真相校验的**设计令牌值**;②一组面向创作工具的**组件交互规格**(scrubby 数值框/自绘渐变条/toast/表达式求值);③一整套"去业余感"的**工程纪律**(唯一色值点/4 基数间距断言/动效总开关)。这些是语义与事实,可对齐;**其源码受 ACL-1.0 自定义许可保护,一行都不能复制进 Lumina**。**

---

## 0. 许可红线(必须最先读)

- VellumBench 全仓适用 **ACL-1.0(Artboard 社区开源协议)** 自定义许可(`LICENSE`,workspace `license-file = "LICENSE"`,Cargo.toml:26;非 SPDX 标准标识符)。
- 其第四条"传染条款"(`LICENSE` 四.1–四.4):**再分发本软件整体或其实质部分(无论是否修改、源码或打包形态),必须整体以 ACL-1.0 授权且随附完整源码**;合并进另一产品再分发的,含衍生内容的部分必须按 ACL-1.0 开源;**以网络服务方式提供其功能亦视为再分发**。
- 对 Lumina 的约束(MIT OR Apache-2.0 双许可):
  1. **禁止复制 vb_ui(或 VellumBench 任何 crate)源码、注释、组织结构到 Lumina**——逐行"翻译"(egui API → GPUI API 改名)同样构成衍生作品,不允许;
  2. **允许对齐的是语义层**:设计参数的具体数值(色值/间距/圆角——客观事实)、交互行为规格(scrubby 修饰键倍率、toast 存活时长——功能性事实)、工程纪律思想(单一真相点/门禁测试——方法);
  3. 落地方式:**优先读设计文档与令牌 JSON**(docs/design/14-下一阶段迭代方案-UI重制与AI心智对齐.md、docs/design/assets/vb-ui-tokens.json——这两份是设计侧"事实"的集中表达),把 .rs 实现仅当作行为验证参考,不对照抄写;
  4. 每次对齐在 `NOTICE.md` 登记(Lumina `NOTICE.md` 已有该条目:"仅设计令牌语义与组件交互规范的思想对齐……未复制源码")。
- 另注:VellumBench 自己在 docs/design/14 §5.2 也确立了同款纪律("抄事实不抄表达"),可引为双方共识。

## 1. VellumBench 是什么

- **产品**:桌面矢量设计工具,文档格式就是 HTML/CSS 项目目录(index.html + styles/ + assets/),导出九种格式(PNG/JPG/GIF/MP4/SVG/PDF/EPS/Ai/PPTX),面向"Agent 出稿 → 人精修 → 存回同一份 HTML"循环(README.md:24-34)。
- **技术栈**:16 crates workspace(vb_common/css/html/doc/layout/render/tools/export/kiln/browser/agent/plugin/platform/ui/app/web);宿主 `eframe 0.35.0 (wgpu)`(crates/vb_app/Cargo.toml),UI 层 egui 0.35.0(crates/vb_ui/Cargo.toml:8)。
- **渲染**:画布自研(ADR-0002 wgpu+vello;ADR-0015 eframe host canvas texture),导出走"浏览器车道 B 优先 + 自研车道 K 兜底"双车道(ADR-0020)。
- **vb_ui 定位**:"设计令牌、字体、图标与通用组件",依赖方向干净:**vb_ui 不依赖 vb_app/vb_doc,可被 vb_app 与 vb_agent 复用**(crates/vb_ui/src/lib.rs:17-18)——这正是 lumina-widgets 追求的同款分层纪律。

## 2. vb_ui 模块地图(12 文件,5378 行)

| 文件 | 行数 | 职责(lib.rs:20-37 导出面) |
|---|---|---|
| theme.rs | 862 | 深/浅双主题设计令牌 + egui 全局样式注入 + 行高派生 + 动效总开关 + 9 组门禁测试 |
| components.rs | 1510 | ToolButton / NumField / ColorField / SectionHeader / PanelTabs / field_row / dialog_footer(+btn3)/ icon_button / 文本助手 |
| fonts.rs | 411 | Inter→MiSans→系统 CJK 五族字体 fallback 链,每字重独立 FontFamily |
| icons.rs | 502 | Lucide 图标语义名枚举(iconflow 1.0 pack-lucide),代码禁裸字形/emoji |
| gradient.rs | 966 | 渐变结构化模型(Gradient/Stop/GradKind)+ CSS 解析/序列化 + 自绘色标条控件 |
| dock.rs | 86 | 面板坞折叠规则与宽度钳制(纯函数) |
| toast.rs | 298 | 可堆叠通知(逻辑/绘制分离,错误可复制) |
| expr.rs | 389 | 数值框数学表达式解析器(纯函数,零 egui 依赖) |
| motion.rs | 152 | 一次性入场动效(对话框/Tab),与总开关联动 |
| cursor.rs | 141 | 工具/手柄 → 系统光标映射表 |
| lib.rs | 37 | 模块组织与 re-export |
| motion_probe_include.rs | 24 | motion 探针(测试辅助) |

## 3. 设计令牌体系(具体值,全部有据)

### 3.1 颜色(theme.rs:57-114;设计侧真相 docs/design/assets/vb-ui-tokens.json)

17 个 UI 令牌 × 深/浅两套,主题间共用品牌色:

| 令牌 | 深色 | 浅色 | 语义(egui 落点) |
|---|---|---|---|
| bg-canvas | #1C1C1E | #F5F5F5 | 画布底(visuals.extreme_bg_color) |
| bg-panel | #2C2C2E | #FFFFFF | 面板底(panel_fill) |
| bg-raised | #3A3A3C | #FFFFFF | 弹出层/下拉(window_fill) |
| bg-input | #38383A | #F0F0F0 | 输入底(widgets.inactive.weak_bg_fill) |
| bg-hover | #48484A | #E8E8E8 | 悬停底 |
| bg-active | #545457 | #DEDEDE | 按下底 |
| border | #3F3F42 | #E6E6E6 | 分隔线/控件描边 |
| border-strong | #545457 | #C9C9C9 | 焦点环/面板边界 |
| text | #FFFFFF | #1E1E1E | 主文字 |
| text-2 | #B8B8BD | #6B6B6B | 次文字/标签 |
| text-3 | #96969B | #767676 | 禁用/占位(经 WCAG 走查两轮加深,theme.rs:70-72/99-101) |
| accent | #0D99FF | #0D99FF(共用) | 选中/激活/焦点 |
| accent-hover | #3AAEFF | #3AAEFF | accent 悬停 |
| accent-dim | rgba(13,153,255,.16) | rgba(13,153,255,.12) | 选中行底 |
| danger | #F24822 | #D93025 | 删除/溢出 |
| warn | #FFC700 | #8F6700 | 冻结块警告 |
| success | #14AE5C | #0E7C42 | 成功 |

**跨主题语义色**(theme.rs:157-256,JSON `color.semantic.*`,`immutable: true` 者不可改,禁止当装饰色):智能参考线品红 #FF00FF(浅色 #E000E0,AI 肌肉记忆)、选中框 #0D99FF(与 accent 同值是设计意图,theme.rs:758-761)、标尺青 #00A5FF、悬停轮廓 rgba(#0D99FF,0.6)、框选填充 alpha 24、网格 #3A3A3A/#DADADA、冻结块占位/描边 #B09A7A、溢出红点 #FF453A/#D70015、diff 删行 #FF7864/加行 #82DC82。

### 3.2 间距(theme.rs:262-284,4 基数,"禁止 5/7/10/15/20")

S1=2 / S2=4 / S3=6 / S4=8 / S5=12 / S6=16 / S8=24 / S9=32;结构常量:**行高 ROW_HEIGHT=24、状态栏 28、浮动工具条 40、控制面板条 40、右坞宽 DOCK_WIDTH=280、窄窗折叠阈值 COLLAPSE_BELOW=1200**。`spacing_scale_is_base_4` 测试硬约束(theme.rs:617-632)。

### 3.3 圆角 / 描边(theme.rs:289-321)

圆角 SM=4(输入框/小按钮)/ MD=6(按钮/Tab/下拉,全局默认)/ LG=8(卡片/分组/菜单)/ XL=12(工具条/浮层/窗口);egui 默认 2px 被判定为"业余感来源"必须覆盖(theme.rs:641)。**与 Lumina AGENTS.md §3.4 的圆角档 4/6/8/12 完全一致,可直接沿用。** 描边:hairline=1.0、focus=1.5。

### 3.4 控件高度(theme.rs:541-581——"行高从字号派生"制度)

- 固定档只有下限 24;**实际行高 = 当前正文字体实测排一行「字Ag0」的 galley 高 + 8,向上取整,下限 24**(`row_height()`,theme.rs:567-581)。动机:CJK 字形 galley 高 ≈ 字号×1.4~1.5,写死 18pt 导致文字压叠(theme.rs:543-551 缺陷锚点)。
- 文字承载控件(NumField/面板输入/工具按钮)一律取 `row_height()`,禁止写死小行高(theme.rs:566-567)。
- ⚠️ **与 Lumina 现行约定冲突**:Lumina AGENTS.md §3.4 定死"控件高 22/26/32"固定档;VellumBench 实证结论是"固定档会随字号/DPI/界面缩放翻车,应派生"。**建议 Lumina 决策:保留 22/26/32 作为名义档,但为文字控件提供派生函数并设下限(24 起)——此为两上游经验合并点。**
- 字号体系(tokens.json font/size):caption 11/400、label 12/500、body 13/400、heading 15/600、mono 12;字重通过独立 FontFamily 实现(fonts.rs:11-20);文本助手规格:strong 13/600、label 12/500、caption 11/400、mono 12 等宽(components.rs:52-74)。
- 字体链(fonts.rs):Regular/Medium/Semibold/Mono/Icon 五族;Inter(SIL OFL)→ MiSans(免费商用)→ 系统 CJK fallback,"缺字逐级降级不出现豆腐块",字体全缺也能跑(fonts.rs:24-26);安装结果返回 `FontReport` 可体检(fonts.rs:80)。

### 3.5 动效(theme.rs:323-368;motion.rs)

四档时长:**INSTANT=0(画布缩放/平移/拖动不走动画)/ HOVER=0.08s / STATE=0.12s / PANEL=0.20s**;明确"绝不做的动效":画布元素入场动画、按钮弹跳、粒子(theme.rs:323-324)。**动效总开关**(H-1):显式设置项持久化,关闭后 `animation_time` 归零 + 所有 `animate_bool_with_time` 经 `anim_time()` 归零(theme.rs:351-368);默认开。egui/winit 不暴露系统"减少动态效果"设置是改用显式开关的原因(theme.rs:338-341)。

### 3.6 双真相与门禁(theme.rs 测试组,583-862)

- **tokens_sync**:`docs/design/assets/vb-ui-tokens.json`(W3C design-tokens 格式)与 theme.rs 逐值比对,进 CI(theme.rs:663-698)——Lumina tokens.rs 应复制这一**机制**(JSON 设计真相 + 编译期 Rust 常量 + CI 同步测试)。
- 其余门禁:双主题字段齐全+品牌色一致(:591-598)、"浅色不是反相"亮度分层断言(:604-613)、4 基数间距(:617-632)、圆角编译期单调断言(const{},:636-642)、语义色不可被主题改写(:645-658)、语义色与主题色防撞色(:763-777)、**WCAG AA 对比度实测门禁**(正文/次级/禁用/语义色文字 ≥4.5:1,:805-845)、行高派生公式(:702-730)、动效开关归零规则(:849-861)。
- theme.rs 自身是**全仓唯一允许颜色字面量的界面文件**(theme.rs:11-14,`check_no_hardcoded_color.ps1` 把关)——对应 Lumina AGENTS.md"组件内禁止硬编码颜色"的执行版。
- 命名空间纪律:UI 主题令牌(`--vb-ui-*`)与用户文档令牌(`--vb-brand-*`)严格分离禁止互引(theme.rs:17-20)——对应 lumina 的"库层 token"与"用户文档色"分离诉求。

## 4. 组件清单与 lumina-widgets 映射(逐个)

### 4.1 theme → lumina-widgets `tokens.rs` + theme 模块
令牌结构(Tokens 结构体 17 色 + space/radius/stroke/motion 四个常量模块)与注入函数(apply 把令牌映射进宿主全局样式)两层结构可直接语义对齐;egui `all_styles_mut` 对应 GPUI 的 Theme/Style 体系。

### 4.2 components.rs 各组件

| vb_ui 组件 | 规格要点(证据) | lumina 映射(docs/04 计划组件) |
|---|---|---|
| **ToolButton**(components.rs:178-336) | 图标+可选文字;tooltip 强制「名称 (快捷键)」零成本学习;激活态 accent_dim 底+accent 描边;悬停 80ms 变底色;按下图标缩 92% 触感(:259-266);尺寸默认=行高,浮动工具条变体图标上文下 | 工具栏/工具箱按钮 → lumina-widgets ToolbarButton |
| **NumField**(:397-635) | 拖标签 scrubby:`dx×speed×修饰键倍率,Alt=×0.1、Shift=×10(Alt 优先)`(:345-354);↑/↓ 步进,Shift ×10;**数学表达式** `320/2`、`50%`(基准由调用方给)经 expr 模块;**提交会话信号** scrub_started/ended/focus_lost → 宿主合并为一次 undo(:366-384);整数无小数点、小数最多 2 位去尾零(:357-364) | **NumberField**(docs/04 核心组件)——scrubby 公式、表达式语法、undo 会话信号三件套建议整体对齐 |
| **ColorField**(:660-1097) | 自绘色块+等宽 hex 回显;点击弹紧凑取色器浮窗(预览+HEX+CSS 变量),Alt+点击弹完整取色器(RGB/HSL 输入+文档令牌色板);令牌数据由调用方注入 `(名称,值)` 对,组件层不依赖文档模型(:656-660);预乘字节陷阱用 to_srgba_unmultiplied 还原(:1087-1089) | **ColorField + 色轮/取色器**(docs/04)——"组件不依赖文档模型,令牌注入"的解耦模式值得照搬 |
| **SectionHeader**(:1106-1190) | 面板分组标题;可折叠版自绘箭头+展开动效,不用宿主默认 collapsing(三角/缩进与 24px 行高、8px 圆角冲突,:1103-1105) | 属性检查器分组头 |
| **PanelTabs**(:1211-1310) | 坞内 Tab 条,accent 下划线指示器;切换 200ms(PANEL 档);reorderable 模式右键菜单「左移/右移」发请求由调用方落顺序表(:1209-1211) | lumina-dock 面板 Tab |
| **field_row**(:81-96) | 统一表单行:标签宽固定 56px 右对齐贴控件列,**多字段左缘自动成线**(AI 属性条对齐方式);行高从 row_height 派生 | 属性检查器行布局规格 |
| **dialog_footer / btn3**(:116-169) | 分隔线上主按钮右下、取消居左;三钮变体次按钮传空串不画;Enter/Esc 接线留给调用方 | 对话框底栏规格 |
| **icon_button**(:1317) | 面板内小图标按钮:无激活态、更小、用于"动作"而非"模式"(与 ToolButton 的分工) | 面板动作按钮 |
| **key_badge_text**(:102-108) | 「名称 (键位)」唯一写法,tooltip/菜单/命令面板共用 | 键位徽章文案单源 |
| 文本助手 strong/label/caption/mono(:52-74) | 11/12/13px × 400/500/600 三档 | tokens.rs 字体档 |

### 4.3 其余模块

| 模块 | 规格 | lumina 映射 |
|---|---|---|
| **dock.rs** | 折叠规则表:窗口 <1200 **强制折叠为 40px 图标条**(手动展开不可覆盖);≥1200 由用户偏好定;展开宽可拖 240–420,默认 280;规则为**纯函数**可完整单测(dock.rs:7-16,32-44) | **lumina-dock**:gpui-component DockArea 更强(自由 dock/浮动),但"窄窗强制折叠 + 宽度钳制 + 规则纯函数化"三层语义应作为 lumina-dock 的行为规格;图标条 40px 与 CONTROL_BAR/FLOATING_TOOLBAR 同高 |
| **toast.rs** | 右下角堆叠、新的在上;TTL 分级:信息/成功 2.5s、告警 5s、**错误 10s**(要留够"读+复制"时间);错误/告警文本可选中+一键复制;入队/到期修剪纯逻辑与绘制分离可单测(toast.rs:7-12) | **lumina-widgets Toast**——TTL 分级与"错误可复制"直接对齐 |
| **expr.rs** | 递归下降四则+括号+一元正负;`%` 语义:给基准 `50%=base×0.5`,未给=0.5;`(30+20)%` 合法;中文输入法容错只收 `×÷` 与全角括号,报错指名字符;纯函数零 egui(expr.rs:5-24) | 建议 Lumina 收进 lumina-core 或 widgets 作 `eval_expr`(纯函数,配 NumberField) |
| **gradient.rs** | 结构化 `Gradient{kind,angle,angle_explicit,stops,hints,head}`:**angle_explicit=false 时回写不得补出 180deg**,否则改写用户源文件(:81-88);色标 `Stop{pos,color 字符串}` 保真透传;解析-序列化 `build(parse(x))==x` 字节幂等;自绘色标条(egui 生态无可用控件,ADR-0003:7 裁定自绘):拖动色标/位置输入/中点/不透明度,几何与命中判定全纯函数(stop_x/pos_at_x/hit_stop/hit_hint/sample_color)(:11-13) | 渐变模型与序列化纪律(幂等/保真)适用于 lumina-paint 渐变原语;色标条控件 → lumina-widgets(视频侧转场/调色渐变也用得上) |
| **motion.rs** | 一次性事件动效(非状态趋近):记起始时刻按流逝时间算缓动,>0.5s 视为重新打开;总开关关闭直通(motion.rs:1-14);**两条 egui 0.35 实测坑**:①`Painter::multiply_opacity` 整窗淡入会让窗口内容在后续所有 pass 永久空白;②Window 子块必须用 scope_builder 否则窗口缩成标题栏(:16-26) | lumina-widgets 动画引擎的"一次性动效"类别;两条坑是 egui 特有,但"入场动效只做布局位移、透明度慎用全局乘法"的经验在 GPUI 侧同样要实测 |
| **cursor.rs** | 工具/手柄→光标映射表常量化;**egui 不支持自定义光标图片**,旋转光标退化为内置最接近图标+视觉兜底,限制记入 tokens.json known_egui_constraints(cursor.rs:8-12) | lumina-canvas 光标体系;**GPUI/winit 支持自定义光标图,可超越该限制**——迁移后应补回"AI 弯曲双箭头"旋转光标 |
| **fonts.rs / icons.rs** | 见 §3.4;图标经 iconflow(字体字节,不绑 egui 版本)集成 Lucide(ISC,1666 图标);枚举引用禁裸字形,all_icons_resolve 测试保证可解析(icons.rs:15-20) | Lumina 用 GPUI 文本栈可取同类字体(Inter/MiSans);Lucide 在 GPUI 侧更自然的是 SVG 渲染,"语义名枚举+可解析测试"的纪律照搬 |

## 5. egui 依赖面(迁移工作量评估)

- **vb_ui 内部**:10 个文件的公开 API 全部以 egui 类型为接口(`Color32/Stroke/Ui/Response/Context/FontId/Margin/TextStyle/Vec2/CornerRadius`,theme.rs:22、components.rs:25 等);组件为 immediate-mode 签名 `fn ui(self, ui: &mut Ui) -> Response`。**这部分无法移植,只能按规格重写**——GPUI 是 Entity/Render 模型。
- **vb_app 侧**:`grep -rl "egui::" crates/vb_app/src | wc -l` = **63 个文件**直接使用 egui;`vb_ui::` 引用 **224 处**。即 vb_ui 只是冰山露出面,VellumBench 若整体迁移 GPUI,主工程量在 vb_app 面板层(29+ 面板:属性/图层/画板/令牌/导出/启动主页/多窗口/时间轴 ADR-0043 等)。
- **vb_ui 自身依赖仅 4 项**(vb_ui/Cargo.toml):egui 0.35、iconflow 1.0(pack-lucide)、vb_common(颜色解析复用)、vb_css(canonical_value 规范化,保证渐变回写字节幂等)——依赖面非常干净,是"UI 层可独立替换"的好范本。

## 6. 迁移到 Lumina 的建议步骤(语义对齐,零源码复制)

1. **定 token 事实(→ tokens.rs)**:把 §3.1–3.5 的数值表(17 色×2 主题、语义色、4 基数间距、圆角 4/6/8/12、动效四档、字号五档、字体链)重表达为 lumina-widgets tokens.rs;同步建 `lumina-tokens.json` 设计真相 + `tokens_sync` CI 测试(机制对齐,值是客观事实);控件高度按 §3.4 的建议合并两上游约定(先在 Lumina docs/06 立决策)。
2. **定行为规格(→ docs/04 组件验收)**:把 NumField scrubby 公式/表达式语法/undo 会话信号、Toast TTL 分级、dock 折叠规则表、PanelTabs 换序协议、取色器双档交互写进 Lumina 组件验收标准——规格是文档,不是代码。
3. **按 GPUI 惯用法实现**:接口形态走 GPUI Entity/事件回调,不模仿 `fn ui(self, ui:&mut Ui)`;立即模式的"每帧重算"语义在 GPUI 用状态+重绘请求表达;egui 两条动效坑(motion.rs:16-26)不适用,但等价风险(全局透明度乘法、子块布局回写)需在 GPUI 侧重新实测并记录进 Lumina docs/06 技术债。
4. **门禁思想移植**:唯一色值定义点(文件级白名单)、4 基数断言、WCAG AA 对比度测试(公式是 WCAG 公共规范)、语义色防撞色、动效总开关、图标可解析测试——以 Lumina 自己的测试代码实现。
5. **登记与复核**:NOTICE.md 已有条目保持更新;若某处实现"不可避免地接近"vb_ui 结构(如同一规则表),改为引用其文档条款而非复述代码组织。

## 7. 风险清单

1. **ACL-1.0 传染(最高优先)**:见 §0。任何"对照 vb_ui 源码逐行改写"的捷径都会把 Lumina 变成 ACL-1.0 衍生品,摧毁 MIT/Apache 双许可。操作纪律:实现者以本文档(语义层转述)与 vb-ui-tokens.json/design/14 篇为输入,不打开 .rs 对照写码。
2. **控件高度体系冲突**:VB"行高派生制(下限 24)"vs Lumina AGENTS"固定档 22/26/32"——不先裁决就动工会让 lumina-widgets 内部两套高度逻辑打架(见 §3.4)。
3. **宿主差异反噬**:VB 的很多"业余感"修补(去 expansion、去斑马纹、按钮默认无框、IME legacy_visuals 关闭,theme.rs:451-531)是 egui 特有Defaults;GPUI 有自己的默认观感问题(如文本度量为假、圆角抗锯齿),**逐条修补经验不可平移,需要一轮 GPUI 侧的同等走查**,否则 token 迁过去观感依然业余。
4. **文档令牌与 UI 令牌的分离**:VB 因"文档就是 CSS"必须严格分离 `--vb-ui-*` 与 `--vb-brand-*`(theme.rs:17-20);Lumina 的 CutForge 桌面壳同样有"界面主题色 vs 用户素材色(转场色块/字幕颜色)"两套色域,若 lumina tokens.rs 不预留这个命名空间隔离,后期会污染用户工程数据。
5. **图标/字体资产许可**:Inter(SIL OFL)、MiSans(免费商用)、JetBrains Mono(OFL)、Lucide(ISC)均可再分发但各有条件(tokens.json font/family/*.license);打包进 Lumina 二进制前按上游同款清单在 Lumina docs/deps(或 NOTICE)登记,且**不要嵌入 Windows 系统字体**(design/14 §5.2 第 3 条的前车之鉴)。
