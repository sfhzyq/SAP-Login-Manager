// Windows DPAPI 封装：用当前 Windows 用户身份加密/解密数据。
// 用于「免密模式」下安全保存主密码——密文仅当前 Windows 用户可解密，
// 拷贝 store.json 到其他机器/账户无法还原明文。
//
// 非 Windows 平台提供空实现（本项目仅面向 Windows，但保留跨平台编译能力）。

#[cfg(windows)]
mod imp {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
    };

    /// 用 DPAPI 加密明文字符串，返回 Base64 密文。
    pub fn encrypt(plain: &str) -> Result<String, String> {
        let mut input = plain.as_bytes().to_vec();
        let mut in_blob = CRYPT_INTEGER_BLOB {
            cbData: input.len() as u32,
            pbData: input.as_mut_ptr(),
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        // 使用 CRYPTPROTECT_UI_FORBIDDEN(0x1) 防止弹出 UI；其余参数为空。
        let ok = unsafe {
            CryptProtectData(
                &mut in_blob,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0x1,
                &mut out_blob,
            )
        };
        if ok == 0 {
            return Err("DPAPI 加密失败".to_string());
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize) };
        let encoded = STANDARD.encode(bytes);
        unsafe {
            LocalFree(out_blob.pbData as _);
        }
        Ok(encoded)
    }

    /// 解密 Base64 密文，返回明文字符串。
    pub fn decrypt(b64: &str) -> Result<String, String> {
        let mut data = STANDARD
            .decode(b64)
            .map_err(|_| "密文 Base64 解码失败".to_string())?;
        let mut in_blob = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_mut_ptr(),
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = unsafe {
            CryptUnprotectData(
                &mut in_blob,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0x1,
                &mut out_blob,
            )
        };
        if ok == 0 {
            return Err("DPAPI 解密失败（可能非本用户加密）".to_string());
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize) };
        let result = String::from_utf8(bytes.to_vec())
            .map_err(|_| "解密结果非法 UTF-8".to_string());
        unsafe {
            LocalFree(out_blob.pbData as _);
        }
        result
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn encrypt(_plain: &str) -> Result<String, String> {
        Err("DPAPI 仅在 Windows 上可用".to_string())
    }
    pub fn decrypt(_b64: &str) -> Result<String, String> {
        Err("DPAPI 仅在 Windows 上可用".to_string())
    }
}

pub use imp::{decrypt, encrypt};
