// Windows DPAPI 封装：用当前 Windows 用户身份加密/解密数据。
// 用于「免密模式」下安全保存主密码——密文仅当前 Windows 用户可解密，
// 拷贝 store.json 到其他机器/账户无法还原明文。
//
// 非 Windows 平台提供空实现（本项目仅面向 Windows，但保留跨平台编译能力）。

/// DPAPI 封装的结构化错误（库公共 API，调用方可按类别处理）
#[derive(Debug, thiserror::Error)]
pub enum DpapiError {
    /// 加密内容超过 u32 上限（本项目场景为密码字符串，实际不可达）
    #[error("加密内容超长")]
    InputTooLong,
    /// 密文 Base64 解码失败
    #[error("密文 Base64 解码失败")]
    InvalidBase64(#[source] base64::DecodeError),
    /// 密文超过 u32 上限
    #[error("密文超长")]
    CipherTooLong,
    /// DPAPI 加密系统调用失败
    #[error("DPAPI 加密失败")]
    EncryptFailed,
    /// DPAPI 解密系统调用失败（可能非当前 Windows 用户加密）
    #[error("DPAPI 解密失败（可能非本用户加密）")]
    DecryptFailed,
    /// 解密结果不是合法 UTF-8
    #[error("解密结果非法 UTF-8")]
    InvalidUtf8(#[source] std::string::FromUtf8Error),
    /// DPAPI 仅在 Windows 上可用
    #[error("DPAPI 仅在 Windows 上可用")]
    UnsupportedPlatform,
}

#[cfg(windows)]
mod imp {
    use super::DpapiError;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
    };
    use zeroize::Zeroizing;

    /// cbData 超过 u32 上限视为异常输入（本项目场景为密码字符串，远达不到）
    const MAX_BLOB_LEN: usize = u32::MAX as usize;

    /// 把 DPAPI 输出 blob 转成字节切片并立即释放系统分配的内存。
    ///
    /// # Safety（调用约定）
    /// - 成功返回后 `out.pbData` 指向 DPAPI 分配的 `cbData` 字节，必须 LocalFree
    /// - `pbData` 可能为 null（cbData==0 时），`from_raw_parts` 对空指针
    ///   即使长度为 0 也是 UB——统一护栏后按空切片处理
    fn take_blob(out: &CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if out.cbData == 0 || out.pbData.is_null() {
            return Vec::new();
        }
        // SAFETY: cbData > 0 且 pbData 非 null，DPAPI 保证该缓冲区在
        // LocalFree 前有效且可读 cbData 字节
        let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) };
        let copied = bytes.to_vec();
        // SAFETY: pbData 由 Crypt*Data 成功调用分配（HLOCAL 语义），
        // 恰好释放一次；上方已完成拷贝
        unsafe { LocalFree(out.pbData as _) };
        copied
    }

    /// 用 DPAPI 加密明文字符串，返回 Base64 密文。
    pub fn encrypt(plain: &str) -> Result<String, DpapiError> {
        if plain.len() > MAX_BLOB_LEN {
            return Err(DpapiError::InputTooLong);
        }
        // Zeroizing：CryptProtectData 失败早退路径同样保证明文缓冲析构清零
        let mut input = Zeroizing::new(plain.as_bytes().to_vec());
        // windows-sys 0.61 起 pDataIn 声明为 *const（Win32 元数据 [in]），无需可变借用
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: input.len() as u32,
            pbData: input.as_mut_ptr(),
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        // 使用 CRYPTPROTECT_UI_FORBIDDEN(0x1) 防止弹出 UI；其余参数为空。
        // SAFETY: in_blob 指向有效的本函数栈上缓冲区，out_blob 接收系统
        // 分配的输出；描述串/可选熵/hwnd 均未使用（NULL），符合 API 契约
        let ok = unsafe {
            CryptProtectData(
                &in_blob,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0x1,
                &mut out_blob,
            )
        };
        if ok == 0 {
            return Err(DpapiError::EncryptFailed);
        }
        let b64 = STANDARD.encode(take_blob(&out_blob));
        Ok(b64) // input 析构自动清零
    }

    /// 解密 Base64 密文，返回明文字符串。
    pub fn decrypt(b64: &str) -> Result<String, DpapiError> {
        let mut data = STANDARD.decode(b64).map_err(DpapiError::InvalidBase64)?;
        if data.len() > MAX_BLOB_LEN {
            return Err(DpapiError::CipherTooLong);
        }
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_mut_ptr(),
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        // SAFETY: in_blob 指向有效输入缓冲区，out_blob 接收系统分配的输出；
        // 描述串/附加熵/prompt 结构均未使用（NULL），符合 API 契约
        let ok = unsafe {
            CryptUnprotectData(
                &in_blob,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0x1,
                &mut out_blob,
            )
        };
        if ok == 0 {
            return Err(DpapiError::DecryptFailed);
        }
        let bytes = take_blob(&out_blob);
        String::from_utf8(bytes).map_err(DpapiError::InvalidUtf8)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::DpapiError;
    pub fn encrypt(_plain: &str) -> Result<String, DpapiError> {
        Err(DpapiError::UnsupportedPlatform)
    }
    pub fn decrypt(_b64: &str) -> Result<String, DpapiError> {
        Err(DpapiError::UnsupportedPlatform)
    }
}

pub use imp::{decrypt, encrypt};
