//! Windows COM 辅助：STA 初始化 + `IDispatch` 晚期绑定调用封装。
//!
//! SAP GUI Scripting 是标准的 IDispatch 自动化接口，所有操作都经由
//! `GetIDsOfNames` + `Invoke` 完成。这里不依赖任何强类型 COM 绑定，完全走
//! 晚期绑定，因此无需为 SAP GUI 生成 TLB/接口定义。

#![cfg(windows)]

use std::ptr::null_mut;
use std::time::Duration;

use windows::core::{IUnknown, Interface, GUID, PCWSTR};
use windows::Win32::System::Com::{
    CLSIDFromProgID, CoInitializeEx, CoUninitialize, IDispatch, COINIT_APARTMENTTHREADED,
    DISPATCH_FLAGS, DISPATCH_PROPERTYPUT, DISPPARAMS, EXCEPINFO,
};
use windows::Win32::System::Ole::GetActiveObject;
use windows::Win32::System::Variant::VARIANT;

/// DISPID 常量：属性写入（PROPERTYPUT）的命名参数必须是 DISPID_PROPERTYPUT(-3)。
const DISPID_PROPERTYPUT: i32 = -3;

/// COM 重试参数
const COM_RETRY_MAX: u32 = 3;
const COM_RETRY_INTERVAL_MS: u64 = 1000;

/// STA 生命周期守卫：初始化 COM，drop 时反初始化。
/// SAP GUI Scripting 必须在单线程单元（STA）中运行。
pub struct ComRuntime;

impl ComRuntime {
    pub fn init() -> windows::core::Result<Self> {
        unsafe {
            // SAFETY: CoInitializeEx 不接收调用方指针或缓冲区；None 为默认分配器，
            // COINIT_APARTMENTTHREADED 是有效常量。失败由 .ok()? 作为 Result 返回。
            // HRESULT::ok() 已返回 windows::core::Result，无需再 from 转换
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        }
        Ok(ComRuntime)
    }
}

impl Drop for ComRuntime {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: 与 init() 中的 CoInitializeEx 成对调用，且 ComRuntime 每个实例
            // 只 drop 一次；CoUninitialize 不接收指针，失败也不返回可用错误。
            CoUninitialize();
        }
    }
}

/// 转 UTF-16 宽字符串（带 null 结尾）。
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 通过 ProgID 取已运行的 SAPGUI 对象（对应 VBScript 的 `GetObject("SAPGUI")`）。
///
/// `GetActiveObject` 返回 `IUnknown`，需再 `cast` 成 `IDispatch` 才能调用脚本接口。
pub fn get_sapgui_object() -> windows::core::Result<IDispatch> {
    get_sapgui_object_inner()
}

/// 带重试和分类错误信息的版本（多 SAP 窗口/后台线程场景下更稳健）。
///
/// 重试最多 `COM_RETRY_MAX` 次，每次间隔 `COM_RETRY_INTERVAL_MS` ms。
/// 失败时返回可读的错误描述（区分「SAP 未启动」「scripting 未启用」「COM 竞态」等）。
pub fn get_sapgui_object_with_retry() -> Result<IDispatch, String> {
    let mut last_err = String::new();
    for attempt in 1..=COM_RETRY_MAX {
        match get_sapgui_object_inner() {
            Ok(obj) => return Ok(obj),
            Err(e) => {
                let code = e.code().0;
                let desc = classify_com_error(code);
                last_err = format!(
                    "COM 错误 (尝试 {}/{}): {} (HRESULT 0x{:08X})",
                    attempt, COM_RETRY_MAX, desc, code,
                );
                if attempt < COM_RETRY_MAX {
                    std::thread::sleep(Duration::from_millis(COM_RETRY_INTERVAL_MS));
                }
            }
        }
    }
    Err(last_err)
}

/// 内部实现：单次尝试获取 SAPGUI 对象。
fn get_sapgui_object_inner() -> windows::core::Result<IDispatch> {
    // SAFETY: wide("SAPGUI") 返回带 null 结尾的 UTF-16 Vec，其指针在本语句
    // 调用期间存活；CLSIDFromProgID 只读取宽串，不取得所有权。
    let clsid = unsafe { CLSIDFromProgID(PCWSTR::from_raw(wide("SAPGUI").as_ptr()))? };
    let mut unk: Option<IUnknown> = None;
    unsafe {
        // SAFETY: clsid 为上一调用成功返回的有效 CLSID；unk 是栈上 Option，
        // &mut unk 在调用期间有效；GetActiveObject 只写入该输出参数。
        GetActiveObject(&clsid, None, &mut unk)?;
    }
    let unk =
        unk.ok_or_else(|| windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL))?;
    unk.cast::<IDispatch>()
}

/// 将 COM HRESULT 错误码翻译为用户可读的中文描述。
pub fn classify_com_error(code: i32) -> &'static str {
    // 常见 SAP GUI Scripting 相关 COM 错误（HRESULT 高位为 1 表示错误，需用 u32 字面量）
    match code {
        x if x == (0x800401F3u32 as i32) => {
            "无效的类字符串 (CO_E_CLASSSTRING) — SAP GUI 可能未正确注册或 scripting 未启用"
        }
        x if x == (0x800401E4u32 as i32) => "无效的语法 (CO_E_SYNTAX) — ProgID 格式错误",
        x if x == (0x80020009u32 as i32) => "异常发生 (DISP_E_EXCEPTION) — SAP GUI 内部错误",
        x if x == (0x80020006u32 as i32) => "未知名称 (DISP_E_UNKNOWNNAME) — 脚本引擎接口不可用",
        x if x == (0x8001010Au32 as i32) => "消息过滤错误 (RPC_E_WRONG_THREAD) — COM 线程单元冲突",
        x if x == (0x80070005u32 as i32) => "访问被拒绝 (E_ACCESSDENIED) — 权限不足",
        x if x == (0x80040154u32 as i32) => {
            "未注册类 (REGDB_E_CLASSNOTREG) — SAP GUI COM 组件未注册"
        }
        x if x == (0x800401E3u32 as i32) => {
            "无可用对象 (MK_E_UNAVAILABLE) — SAP GUI 未运行或已关闭"
        }
        _ => "未知 COM 错误",
    }
}

/// 调用 `IDispatch` 的方法/属性。
///
/// `args` 按源码顺序传入，函数内部逆序 arranging（COM `rgvarg` 约定为右到左）。
/// `flags` 为 `DISPATCH_METHOD` / `DISPATCH_PROPERTYGET` / `DISPATCH_PROPERTYPUT`。
pub fn invoke(
    disp: &IDispatch,
    name: &str,
    flags: DISPATCH_FLAGS,
    args: &[VARIANT],
) -> windows::core::Result<VARIANT> {
    unsafe {
        // SAFETY: wname/rgvarg/params/result/excep 均在本块内保持存活且未被移动；
        // cArgs 与 rgvarg 长度一致、named 数组仅在 PROPERTYPUT 时长度 1；
        // Invoke 为同步调用，返回前不会释放这些缓冲；返回的 VARIANT 与
        // EXCEPINFO 内 BSTR 的所有权按 windows-rs 约定由调用方持有/释放。
        let wname = wide(name);
        let name_ptr = PCWSTR::from_raw(wname.as_ptr());
        let mut dispid: i32 = 0;
        disp.GetIDsOfNames(&GUID::default(), &name_ptr, 1, 0, &mut dispid)?;

        // 逆序排列参数（COM rgvarg 约定：右到左）
        let mut rgvarg: Vec<VARIANT> = args.to_vec();
        rgvarg.reverse();

        let mut named = [DISPID_PROPERTYPUT];
        let (rgdispid_named_args, c_named) = if flags == DISPATCH_PROPERTYPUT {
            (named.as_mut_ptr(), 1u32)
        } else {
            (null_mut(), 0u32)
        };

        let params = DISPPARAMS {
            rgvarg: rgvarg.as_mut_ptr(),
            rgdispidNamedArgs: rgdispid_named_args,
            cArgs: rgvarg.len() as u32,
            cNamedArgs: c_named,
        };
        let mut result = VARIANT::default();
        let mut excep = EXCEPINFO::default();
        let hr = disp.Invoke(
            dispid,
            &GUID::default(),
            0,
            flags,
            &params,
            Some(&mut result),
            Some(&mut excep),
            None,
        );
        // DISP_E_EXCEPTION：服务器经 excep 返回了可读的异常描述（如 SAP 的
        // 「无效的字符」等）。取出文本并入错误消息，同时释放 EXCEPINFO 内
        // 三个手动管理的 BSTR（ManuallyDrop，无 Drop 兜底，不取即泄漏）。
        if let Err(e) = hr {
            if e.code() == windows::Win32::Foundation::DISP_E_EXCEPTION {
                let desc = take_excepinfo_description(&mut excep);
                let msg = if desc.is_empty() { e.to_string() } else { desc };
                return Err(windows::core::Error::new(e.code(), msg));
            }
            return Err(e);
        }
        Ok(result)
    }
}

/// 提取 `EXCEPINFO.bstrDescription` 文本并释放全部三个手动管理的 BSTR。
///
/// # Safety 约定
/// 仅可在 `IDispatch::Invoke` 返回 `DISP_E_EXCEPTION` 之后调用：此时服务器
/// 已填充 `excep` 的 BSTR 字段，所有权转移给调用方；`take` 后 drop 即
/// `SysFreeString`，不会双重释放。空字符串字段无需释放，但统一 take 无害。
fn take_excepinfo_description(excep: &mut EXCEPINFO) -> String {
    unsafe {
        // SAFETY: 见函数级约定——BSTR 所有权已由失败的 Invoke 转移到本函数，
        // take 取出后立即释放，避免泄漏；此函数是该字段的唯一消费者。
        let desc = core::mem::ManuallyDrop::take(&mut excep.bstrDescription);
        let source = core::mem::ManuallyDrop::take(&mut excep.bstrSource);
        let help = core::mem::ManuallyDrop::take(&mut excep.bstrHelpFile);
        let text = desc.to_string();
        drop(desc);
        drop(source);
        drop(help);
        text
    }
}

/// 由字符串构造 VARIANT（BSTR）。
pub fn variant_string(s: &str) -> VARIANT {
    VARIANT::from(s)
}

/// 由 i32 构造 VARIANT。
pub fn variant_i32(n: i32) -> VARIANT {
    VARIANT::from(n)
}
