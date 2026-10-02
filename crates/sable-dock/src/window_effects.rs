//! E3 窗口系统材质(Mica/Acrylic/Tabbed;08 迭代计划 S2-E3,分册六 §2)。
//!
//! # 机制(rio terminal 同款降级模式)
//!
//! Windows 11 22621+ 走 DWM 系统背板:`DwmSetWindowAttribute` 写
//! `DWMWA_SYSTEMBACKDROP_TYPE(38)`(2=Mica 主窗 / 3=Acrylic 弹窗 / 4=Tabbed),
//! 并以 `DWMWA_USE_IMMERSIVE_DARK_MODE(20)` 跟随主题明暗。**Build < 22621
//! 或 API 失败 → 返回 `Err`(调用方日志 + 纯色降级)**——本模块不猜系统
//! 版本,以属性调用的真实结果为准(旧系统对该属性返回错误,天然构成版本
//! 检测);GPU 开销为零(材质由 DWM 合成器绘制)。
//!
//! # 职责边界(v0.2)
//!
//! - 只提供"应用一次"的 [`apply_backdrop`];`WM_THEMECHANGED` 监听与主题
//!   切换后的重应用是**应用层职责**(v0.2 不监听);
//! - 非 Windows 平台:同签名 stub 恒 `Err`(调用方纯色降级);
//! - 本机开发环境为 Win10 19045(无法视觉验收 Mica),真机验证按计划 S3
//!   (Win11)执行。
//!
//! # 关于 unsafe 的偏差说明(与任务书"windows 绑定是 safe"的假设不符)
//!
//! windows-0.62.2 registry 源码核实:`DwmSetWindowAttribute` 是
//! `pub unsafe fn`(收裸指针)。本 crate 原为 `#![forbid(unsafe_code)]`,
//! 已放宽为 workspace 纪律的 `#![deny(unsafe_code)]` + 本文件调用点精确
//! `#[allow]`(AGENTS.md §3 的 deny 全 workspace 口径不变);除唯一的
//! FFI 调用点(`set_attr`)外全文件零 unsafe。

/// 窗口系统材质(E3)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    /// 交由系统按窗口类型决定(DWMSBT_AUTO)。
    Auto,
    /// Mica 主窗材质(DWMSBT_MAINWINDOW)。
    Mica,
    /// Acrylic 弹窗材质(DWMSBT_TRANSIENTWINDOW)。
    Acrylic,
    /// Tabbed 窗口材质(DWMSBT_TABBEDWINDOW)。
    Tabbed,
    /// 关闭系统背板(DWMSBT_NONE)。
    None,
}

/// 给窗口应用系统材质背板:`hwnd` 为 Win32 窗口句柄(gpui 侧经平台扩展取
/// `HWND as isize`),`dark` 跟随主题明暗。
///
/// - 暗色标题栏(`DWMWA_USE_IMMERSIVE_DARK_MODE`)先行设置,失败直接
///   `Err`(极旧系统连它都不支持,背板必也不支持,一次失败即降级);
/// - 背板(`DWMWA_SYSTEMBACKDROP_TYPE`)失败(含 Build < 22621 的
///   E_INVALIDARG)→ `Err`,调用方应日志 + 纯色降级;
/// - 重复调用安全(属性覆盖写,主题切换后重应用即再调一次)。
#[cfg(windows)]
pub fn apply_backdrop(hwnd: isize, backdrop: Backdrop, dark: bool) -> Result<(), String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE,
        DwmSetWindowAttribute,
    };

    // 写一个 4 字节(i32)窗口属性(此即全文件仅有的 unsafe 收口)
    fn set_attr(hwnd: HWND, attr: DWMWINDOWATTRIBUTE, value: i32) -> windows::core::Result<()> {
        #[allow(unsafe_code)]
        // windows-0.62.2 的 DwmSetWindowAttribute 是 unsafe fn(见模块 doc 偏差说明)
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                attr,
                std::ptr::from_ref(&value).cast(),
                std::mem::size_of::<i32>() as u32,
            )
        }
    }

    let hwnd = HWND(hwnd as *mut core::ffi::c_void);

    // 1) 主题明暗跟随(immersive dark mode;BOOL 为 4 字节 i32)
    set_attr(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, i32::from(dark)).map_err(|e| {
        format!(
            "E3: DWMWA_USE_IMMERSIVE_DARK_MODE({dark}) 失败: {e}(HRESULT 0x{:08X});调用方应降级纯色",
            e.code().0
        )
    })?;

    // 2) 系统背板(2=Mica / 3=Acrylic / 4=Tabbed;Build < 22621 在此报错)
    let value = backdrop_value(backdrop);
    set_attr(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, value.0).map_err(|e| {
        format!(
            "E3: DWMWA_SYSTEMBACKDROP_TYPE({backdrop:?}={}) 失败: {e}(HRESULT 0x{:08X});Build < 22621 或 API 失败,调用方应降级纯色",
            value.0,
            e.code().0
        )
    })?;
    Ok(())
}

/// `Backdrop` → DWM 属性值(纯函数,映射表见 [`Backdrop`])。
#[cfg(windows)]
fn backdrop_value(backdrop: Backdrop) -> windows::Win32::Graphics::Dwm::DWM_SYSTEMBACKDROP_TYPE {
    use windows::Win32::Graphics::Dwm::{
        DWMSBT_AUTO, DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMSBT_TABBEDWINDOW, DWMSBT_TRANSIENTWINDOW,
    };
    match backdrop {
        Backdrop::Auto => DWMSBT_AUTO,
        Backdrop::Mica => DWMSBT_MAINWINDOW,
        Backdrop::Acrylic => DWMSBT_TRANSIENTWINDOW,
        Backdrop::Tabbed => DWMSBT_TABBEDWINDOW,
        Backdrop::None => DWMSBT_NONE,
    }
}

/// 非 Windows 平台 stub(同签名恒 `Err`):无 DWM,调用方纯色降级。
#[cfg(not(windows))]
pub fn apply_backdrop(_hwnd: isize, _backdrop: Backdrop, _dark: bool) -> Result<(), String> {
    Err("非 Windows 平台:无 DWM 系统背板,调用方应降级纯色".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn backdrop_values_match_dwm_constants() {
        use windows::Win32::Graphics::Dwm::{
            DWMSBT_AUTO, DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMSBT_TABBEDWINDOW,
            DWMSBT_TRANSIENTWINDOW,
        };
        // DWMWA_SYSTEMBACKDROP_TYPE 的语义值:0=Auto / 1=None / 2=Mica /
        // 3=Acrylic(Transient) / 4=Tabbed(08-E3 参数表)
        assert_eq!(backdrop_value(Backdrop::Auto), DWMSBT_AUTO);
        assert_eq!(backdrop_value(Backdrop::None), DWMSBT_NONE);
        assert_eq!(backdrop_value(Backdrop::Mica).0, DWMSBT_MAINWINDOW.0);
        assert_eq!(backdrop_value(Backdrop::Mica).0, 2);
        assert_eq!(
            backdrop_value(Backdrop::Acrylic).0,
            DWMSBT_TRANSIENTWINDOW.0
        );
        assert_eq!(backdrop_value(Backdrop::Acrylic).0, 3);
        assert_eq!(backdrop_value(Backdrop::Tabbed).0, DWMSBT_TABBEDWINDOW.0);
        assert_eq!(backdrop_value(Backdrop::Tabbed).0, 4);
    }

    #[cfg(not(windows))]
    #[test]
    fn stub_returns_err_on_non_windows() {
        // Windows 上 stub 不编译,本测试随 cfg 一起缺席
        for backdrop in [
            Backdrop::Auto,
            Backdrop::Mica,
            Backdrop::Acrylic,
            Backdrop::Tabbed,
            Backdrop::None,
        ] {
            let err = apply_backdrop(0, backdrop, true).expect_err("stub 恒 Err");
            assert!(err.contains("非 Windows 平台"));
            assert!(apply_backdrop(-1, backdrop, false).is_err());
        }
    }
}
