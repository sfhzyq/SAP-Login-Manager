// cipher 0.5：`BlockEncryptMut`/`BlockDecryptMut` 更名为 `BlockModeEncrypt`/`BlockModeDecrypt`，
// 且 `encrypt_padded_vec_mut`/`decrypt_padded_vec_mut` 去掉 `_mut` 后缀（改按值消费 self）
use aes::cipher::{block_padding::Pkcs7, BlockModeDecrypt, BlockModeEncrypt, KeyIvInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::aead::{Aead, KeyInit};
use argon2::{Algorithm, Argon2, Params, Version};
// rand 0.10：`Rng` 只剩核心 trait（rand_core），随机值扩展方法移到 `RngExt`
use rand::RngExt;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use sha2::Sha256;
// hmac 0.13 把密钥注入从 `Mac::new_from_slice` 挪到 `KeyInit`；aes-gcm 0.11 后此处
// 与 `aes_gcm::aead::KeyInit` 已是同一个 crypto-common 0.2 trait，故无需别名
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use subtle::ConstantTimeEq;
use thiserror::Error;

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;
type HmacSha256 = Hmac<Sha256>;

/// 密钥派生版本：0 = 旧版(10000轮自定义，非标准)
pub const KDF_VERSION_LEGACY: u32 = 0;
/// 密钥派生版本：1 = 标准 PBKDF2-HMAC-SHA256 (600000 轮)
pub const KDF_VERSION_PBKDF2: u32 = 1;
/// 密钥派生版本：2 = Argon2id（OWASP 2023 推荐参数，抗 GPU/ASIC 爆破）
pub const KDF_VERSION_ARGON2: u32 = 2;
/// 最新密钥派生版本（首次设密码 / 改密码 / 迁移统一使用）
pub const KDF_VERSION_LATEST: u32 = KDF_VERSION_ARGON2;

/// 标准密钥派生轮数 (OWASP 2023 推荐)
pub const KDF_ITERATIONS_V2: u32 = 600_000;
/// 旧版轮数
pub const KDF_ITERATIONS_LEGACY: u32 = 10_000;

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("加密失败: {0}")]
    EncryptError(String),
    #[error("解密失败: {0}")]
    DecryptError(String),
    #[error("密钥派生失败")]
    KeyDerivationError,
}

/// 生成随机 salt (32 字节)
pub fn generate_salt() -> String {
    let mut rng = rand::rng();
    let salt: [u8; 32] = rng.random();
    BASE64.encode(salt)
}

/// 旧版密钥派生 (10000 轮自定义 PBKDF2-like，非标准)
/// 保留用于兼容旧数据迁移
fn derive_key_legacy(password: &str, salt: &str) -> Result<[u8; 32], CryptoError> {
    let salt_bytes = BASE64.decode(salt).map_err(|e| CryptoError::EncryptError(e.to_string()))?;
    let password_bytes = password.as_bytes();

    let mut mac = <HmacSha256 as KeyInit>::new_from_slice(password_bytes)
        .map_err(|_| CryptoError::KeyDerivationError)?;

    mac.update(&salt_bytes);
    let mut u = mac.finalize().into_bytes().to_vec();
    let mut result = vec![0u8; 32];

    for _ in 0..KDF_ITERATIONS_LEGACY {
        let mut mac2 = <HmacSha256 as KeyInit>::new_from_slice(password_bytes)
            .map_err(|_| CryptoError::KeyDerivationError)?;
        mac2.update(&u);
        u = mac2.finalize().into_bytes().to_vec();
        for j in 0..32 {
            result[j] ^= u[j];
        }
    }

    let key: [u8; 32] = result.try_into().map_err(|_| CryptoError::KeyDerivationError)?;
    Ok(key)
}

/// V2 密钥派生 (标准 PBKDF2-HMAC-SHA256, 600000 轮)
fn derive_key_v2(password: &str, salt: &str) -> Result<[u8; 32], CryptoError> {
    let salt_bytes = BASE64.decode(salt).map_err(|e| CryptoError::EncryptError(e.to_string()))?;
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt_bytes, KDF_ITERATIONS_V2, &mut key);
    Ok(key)
}

/// V3 密钥派生 (Argon2id，OWASP 2023 推荐参数：m=19MiB, t=2, p=1)
fn derive_key_argon2(password: &str, salt: &str) -> Result<[u8; 32], CryptoError> {
    let salt_bytes = BASE64.decode(salt).map_err(|e| CryptoError::EncryptError(e.to_string()))?;
    // m=19456 KiB(19MiB), t=2, p=1，输出 32 字节作为 AES-256 密钥
    let params = Params::new(19_456, 2, 1, Some(32))
        .map_err(|_| CryptoError::KeyDerivationError)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon
        .hash_password_into(password.as_bytes(), &salt_bytes, &mut key)
        .map_err(|_| CryptoError::KeyDerivationError)?;
    Ok(key)
}

/// 根据版本派生密钥
/// version: 0 = 旧版, 1 = 标准 PBKDF2, 2 = Argon2id
/// 未知版本显式报错——静默落到 Argon2id 会用损坏的版本号派生出
/// 「看似可用实则错误」的密钥，把数据损坏伪装成密码错误
pub fn derive_key(password: &str, salt: &str, version: u32) -> Result<[u8; 32], CryptoError> {
    match version {
        KDF_VERSION_LEGACY => derive_key_legacy(password, salt),
        KDF_VERSION_PBKDF2 => derive_key_v2(password, salt),
        KDF_VERSION_ARGON2 => derive_key_argon2(password, salt),
        _ => Err(CryptoError::KeyDerivationError),
    }
}

/// AES-256-CBC 加密（旧版，仅用于解密旧数据）
pub fn encrypt(plaintext: &str, key: &[u8; 32]) -> Result<String, CryptoError> {
    let iv: [u8; 16] = rand::rng().random();
    let plaintext_bytes = plaintext.as_bytes();

    let ciphertext = Aes256CbcEnc::new(key.into(), &iv.into())
        .encrypt_padded_vec::<Pkcs7>(plaintext_bytes);

    let mut result = Vec::with_capacity(16 + ciphertext.len());
    result.extend_from_slice(&iv);
    result.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(&result))
}

/// AES-256-CBC 解密（旧版，仅用于解密旧数据）
pub fn decrypt(ciphertext_b64: &str, key: &[u8; 32]) -> Result<String, CryptoError> {
    let data = BASE64.decode(ciphertext_b64).map_err(|_| CryptoError::DecryptError("解密失败".to_string()))?;

    if data.len() < 16 {
        return Err(CryptoError::DecryptError("解密失败".to_string()));
    }

    let (iv, ciphertext) = data.split_at(16);
    let iv: [u8; 16] = iv.try_into().map_err(|_| CryptoError::DecryptError("解密失败".to_string()))?;

    let decrypted = Aes256CbcDec::new(key.into(), &iv.into())
        .decrypt_padded_vec::<Pkcs7>(ciphertext)
        .map_err(|_| CryptoError::DecryptError("解密失败".to_string()))?;

    String::from_utf8(decrypted).map_err(|_| CryptoError::DecryptError("解密失败".to_string()))
}

/// AES-256-GCM 加密（带认证，新版本使用）
/// 格式: base64(nonce(12) + ciphertext + tag(16))
pub fn encrypt_gcm(plaintext: &str, key: &[u8; 32]) -> Result<String, CryptoError> {
    // cipher 0.5 / hybrid-array 0.4 起 `Array::from_slice` 弃用：定长数组直接 From 转换
    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(*key));
    let nonce_bytes: [u8; 12] = rand::rng().random();
    let nonce = Nonce::from(nonce_bytes);

    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|_| CryptoError::EncryptError("加密失败".to_string()))?;

    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(&result))
}

/// AES-256-GCM 解密（带认证，新版本使用）
pub fn decrypt_gcm(ciphertext_b64: &str, key: &[u8; 32]) -> Result<String, CryptoError> {
    let data = BASE64.decode(ciphertext_b64).map_err(|_| CryptoError::DecryptError("解密失败".to_string()))?;

    if data.len() < 28 {
        return Err(CryptoError::DecryptError("解密失败".to_string()));
    }

    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce_arr: [u8; 12] = nonce_bytes
        .try_into()
        .map_err(|_| CryptoError::DecryptError("解密失败".to_string()))?;
    let nonce = Nonce::from(nonce_arr);

    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(*key));
    let plaintext = cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|_| CryptoError::DecryptError("解密失败".to_string()))?;

    String::from_utf8(plaintext).map_err(|_| CryptoError::DecryptError("解密失败".to_string()))
}

/// 生成密码哈希
pub fn hash_password(password: &str, salt: &str, version: u32) -> Result<String, CryptoError> {
    let key = derive_key(password, salt, version)?;
    Ok(BASE64.encode(key))
}

/// 恒定时间验证密码哈希（防止时序攻击）
pub fn verify_password(password: &str, salt: &str, hash: &str, version: u32) -> Result<bool, CryptoError> {
    let key = derive_key(password, salt, version)?;
    let computed = BASE64.encode(key);
    // 恒定时间比较，防止时序攻击
    Ok(bool::from(computed.as_bytes().ct_eq(hash.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argon2_derive_is_deterministic() {
        let salt = generate_salt();
        let a1 = derive_key_argon2("master-pw", &salt).unwrap();
        let a2 = derive_key_argon2("master-pw", &salt).unwrap();
        assert_eq!(a1, a2);
        let wrong = derive_key_argon2("master-pw2", &salt).unwrap();
        assert_ne!(a1, wrong);
        // 不同 KDF 对同一密码必须得到不同密钥
        let p = derive_key_v2("master-pw", &salt).unwrap();
        assert_ne!(a1, p);
    }

    #[test]
    fn gcm_roundtrip_with_argon2_key() {
        let salt = generate_salt();
        let key = derive_key_argon2("master-pw", &salt).unwrap();
        let enc = encrypt_gcm("SAP 密码123", &key).unwrap();
        assert_ne!(enc, "SAP 密码123");
        let dec = decrypt_gcm(&enc, &key).unwrap();
        assert_eq!(dec, "SAP 密码123");
        // 错误密钥必须解密失败（GCM 认证）
        let other = derive_key_argon2("wrong-pw", &salt).unwrap();
        assert!(decrypt_gcm(&enc, &other).is_err());
    }

    #[test]
    fn hash_and_verify_roundtrip_all_versions() {
        let salt = generate_salt();
        for v in [KDF_VERSION_LEGACY, KDF_VERSION_PBKDF2, KDF_VERSION_ARGON2] {
            let h = hash_password("pw", &salt, v).unwrap();
            assert!(verify_password("pw", &salt, &h, v).unwrap());
            assert!(!verify_password("pw2", &salt, &h, v).unwrap());
        }
    }
}
