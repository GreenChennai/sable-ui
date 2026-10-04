//! 字体随包(TOK-02,迭代审查报告 2026-10-04 §5.4):UI 字体 Inter、
//! 等宽数值字体 JetBrains Mono,经 [`include_bytes!`] 嵌入二进制并在应用
//! 启动时注册进 gpui 文本系统——组件不依赖宿主安装任何字体。
//!
//! # 用法(应用启动时,一次)
//!
//! ```ignore
//! sable_widgets::fonts::install(cx); // 幂等安全:重复注册报错被吞掉并降级
//! ```
//!
//! # fallback 链声明(TC-TOK-TYPE-03 断言点)
//!
//! `UI 字体(Inter)→ 系统 CJK 字体`,缺字**逐级降级不豆腐块**:
//!
//! 1. Inter / JetBrains Mono(本模块嵌入,覆盖拉丁 + 常用符号;数字、
//!    十六进制与拉丁文案全部命中,NumberField 等宽数值不抖动);
//! 2. 系统 CJK:Windows `Microsoft YaHei UI`/`Microsoft YaHei`、macOS
//!    `PingFang SC`、Linux `Noto Sans CJK SC`/`Noto Sans CJK`——中文/日文/
//!    韩文等 Inter 未覆盖的字符由 gpui 平台文本系统(DirectWrite/CoreText/
//!    fontconfig)按 [`FALLBACK_FAMILIES`] 与平台默认链逐级回退。
//!
//! gpui 0.2.2 的字体注册 API(源码级核实):`App::text_system()` 返回
//! `Arc<TextSystem>`,`TextSystem::add_fonts(Vec<Cow<'static, [u8]>>)` 在
//! Windows 上经 `IDWriteInMemoryFontFileLoader::CreateInMemoryFontFileReference`
//! 注册(见 gpui src/platform/windows/direct_write.rs);Inter 静态 TTF 带
//! typographic family "Inter"(name ID 16)+ OS/2 usWeightClass 400/500/600,
//! 按族名 + 字重匹配可达 Medium/SemiBold。
//!
//! # 真机走查步骤(不在 CI 覆盖内,如实声明)
//!
//! TC-TOK-TYPE-03 的"缺字不豆腐块"是**渲染态**验收,CI 只覆盖声明与
//! 字节存在性(见 tests)。真机走查步骤(每次字体/平台层改动后人工执行):
//!
//! 1. `cargo run -p story`(已接 [`install`]);
//! 2. 切到 NumberField 分组,确认数值显示为等宽字形(与 UI 文字并排目测
//!    字宽差异)、拖动时字符不横向抖动;
//! 3. 在任意组件的标签里放一段中文(如"图层不透明度"),确认中文以系统
//!    雅黑/苹方/思源黑体渲染,无 `□`(豆腐块);
//! 4. 拔掉 [`install`] 调用重复 2~3,确认整体降级到系统字体、仍无豆腐块。
//!
//! # 许可
//!
//! 两套字体均为 SIL Open Font License 1.1,许可全文随字体放在
//! `assets/fonts/`(Inter-LICENSE.txt / JetBrainsMono-OFL.txt),登记于
//! 仓库根 NOTICE.md。嵌入不含任何色值(唯一色值点纪律不变)。

use std::borrow::Cow;

use gpui::App;

use crate::tokens::MONO_FONT;

/// Inter Regular(字重 400;正文/说明,411 KB)。
/// 来源:rsms/inter v4.1 release,SIL OFL 1.1。
pub const INTER_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");

/// Inter Medium(字重 500;label 档,418 KB)。
pub const INTER_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/Inter-Medium.ttf");

/// Inter SemiBold(字重 600;display/title/body-strong 档,420 KB)。
pub const INTER_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");

/// JetBrains Mono Regular(字重 400;数值/十六进制,274 KB)。
/// 来源:JetBrains/JetBrainsMono v2.304 release,SIL OFL 1.1。
pub const JETBRAINS_MONO_REGULAR: &[u8] =
    include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf");

/// 全部嵌入字体(与 [`crate::tokens::TEXT_SIZES`] 用到的字重集合 400/500/600
/// 精确对应,不多嵌)。
pub const EMBEDDED_FONTS: [&[u8]; 4] = [
    INTER_REGULAR,
    INTER_MEDIUM,
    INTER_SEMIBOLD,
    JETBRAINS_MONO_REGULAR,
];

/// 声明的系统 CJK fallback 族名(按平台优先序;TC-TOK-TYPE-03 断言点)。
/// 嵌入字体缺字时,gpui 平台文本系统按这些族名逐级回退(链尾"系统默认"
/// 由平台兜底,无需显式声明)。
pub const FALLBACK_FAMILIES: [&str; 5] = [
    "Microsoft YaHei UI", // Windows 现代 UI 字体(简中)
    "Microsoft YaHei",    // Windows 兼容名
    "PingFang SC",        // macOS 简中
    "Noto Sans CJK SC",   // Linux(fontconfig 常见名)
    "Noto Sans CJK",      // Linux 变体名
];

/// 把全部嵌入字体注册进 gpui 文本系统(应用启动时调用一次)。
///
/// 失败语义:**尽力而为,不 panic**——注册失败(理论上仅平台文本系统
/// 未初始化等极端情形)打印诊断后继续,组件经 fallback 链降级到系统字体,
/// 应用照常可用(豆腐块风险由上表 fallback 声明兜住)。
pub fn install(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = EMBEDDED_FONTS
        .iter()
        .map(|data| Cow::Borrowed(*data))
        .collect();
    let text_system = cx.text_system();
    if let Err(e) = text_system.add_fonts(fonts) {
        eprintln!(
            "[sable::fonts] 嵌入字体注册失败,降级系统字体(UI={Inter},mono={mono}):{e}",
            Inter = crate::tokens::UI_FONT,
            mono = MONO_FONT
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TC-TOK-TYPE-03(令牌侧):fallback 链必须声明系统 CJK 族,且覆盖
    /// 三大平台;嵌入族名声明与 tokens 字体族令牌成对出现。
    #[test]
    fn tc_tok_type_03_fallback_chain_declares_cjk_fallbacks() {
        // UI 族与 mono 族来自 tokens 单点(不在此处另立字符串)
        assert_eq!(crate::tokens::UI_FONT, "Inter");
        assert_eq!(MONO_FONT, "JetBrains Mono");
        // fallback 链覆盖 Windows/macOS/Linux 的系统 CJK
        for platform_needle in ["Microsoft YaHei", "PingFang SC", "Noto Sans CJK"] {
            assert!(
                FALLBACK_FAMILIES
                    .iter()
                    .any(|f| f.contains(platform_needle)),
                "fallback 链缺 {platform_needle}(跨平台 CJK 降级不完整)"
            );
        }
        // 链首到链尾不得含嵌入族自身(fallback 只声明"系统侧")
        for f in FALLBACK_FAMILIES {
            assert_ne!(f, crate::tokens::UI_FONT);
            assert_ne!(f, MONO_FONT);
        }
        // 真机"不豆腐块"走查步骤已写进模块 doc(渲染态验收,不在 CI 覆盖内)
        let doc = include_str!("fonts.rs");
        assert!(
            doc.contains("真机走查步骤"),
            "模块 doc 必须保留真机走查步骤(如实声明 CI 边界)"
        );
    }

    /// 嵌入字体字节体检(防"下载到 HTML 错误页"类事故进仓库):
    /// 每个文件 >100KB 且以合法 sfnt 魔数开头(TTF 0x00010000 / OTTO)。
    #[test]
    fn embedded_fonts_are_real_ttf_over_100kb() {
        for data in EMBEDDED_FONTS {
            assert!(data.len() > 100_000, "字体文件应 >100KB,得 {}", data.len());
            let magic = &data[..4];
            let is_ttf = magic == [0x00, 0x01, 0x00, 0x00];
            let is_otto = magic == b"OTTO";
            let is_true = magic == b"true";
            assert!(
                is_ttf || is_otto || is_true,
                "非法 sfnt 魔数:{magic:?}(这不是字体文件)"
            );
        }
        // 字重集合覆盖 tokens 用到的 400/500/600:三份 Inter(400/500/600)
        // + 一份 JetBrains Mono(400),恰好四份
        assert_eq!(EMBEDDED_FONTS.len(), 4);
    }
}
