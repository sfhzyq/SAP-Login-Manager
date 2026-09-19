use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, BlockDecryptMut, KeyIvInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::aead::{Aead, KeyInit};
use rand::Rng;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use sha2::Sha256;
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use subtle::ConstantTimeEq;
use thiserror::Error;

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;
type HmacSha256 = Hmac<Sha256>;

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
    let mut rng = rand::thread_rng();
    let salt: [u8; 32] = rng.gen();
    BASE64.encode(salt)
}

/// 旧版密钥派生 (10000 轮自定义 PBKDF2-like，非标准)
/// 保留用于兼容旧数据迁移
fn derive_key_legacy(password: &str, salt: &str) -> Result<[u8; 32], CryptoError> {
    let salt_bytes = BASE64.decode(salt).map_err(|e| CryptoError::EncryptError(e.to_string()))?;
    let password_bytes = password.as_bytes();

    let mut mac = <HmacSha256 as Mac>::new_from_slice(password_bytes)
        .map_err(|_| CryptoError::KeyDerivationError)?;

    mac.update(&salt_bytes);
    let mut u = mac.finalize().into_bytes().to_vec();
    let mut result = vec![0u8; 32];

    for _ in 0..KDF_ITERATIONS_LEGACY {
        let mut mac2 = <HmacSha256 as Mac>::new_from_slice(password_bytes)
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

/// 根据版本派生密钥
/// version: 0 = 旧版, >=1 = 标准 PBKDF2
pub fn derive_key(password: &str, salt: &str, version: u32) -> Result<[u8; 32], CryptoError> {
    match version {
        0 => derive_key_legacy(password, salt),
        _ => derive_key_v2(password, salt),
    }
}

/// AES-256-CBC 加密（旧版，仅用于解密旧数据）
pub fn encrypt(plaintext: &str, key: &[u8; 32]) -> Result<String, CryptoError> {
    let iv: [u8; 16] = rand::thread_rng().gen();
    let plaintext_bytes = plaintext.as_bytes();

    let ciphertext = Aes256CbcEnc::new(key.into(), &iv.into())
        .encrypt_padded_vec_mut::<Pkcs7>(plaintext_bytes);

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
        .decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
        .map_err(|_| CryptoError::DecryptError("解密失败".to_string()))?;

    String::from_utf8(decrypted).map_err(|_| CryptoError::DecryptError("解密失败".to_string()))
}

/// AES-256-GCM 加密（带认证，新版本使用）
/// 格式: base64(nonce(12) + ciphertext + tag(16))
pub fn encrypt_gcm(plaintext: &str, key: &[u8; 32]) -> Result<String, CryptoError> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce_bytes: [u8; 12] = rand::thread_rng().gen();
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
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
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| CryptoError::DecryptError("解密失败".to_string()))?;

    String::from_utf8(plaintext).map_err(|_| CryptoError::DecryptError("解密失败".to_string()))
}

/// 生成密码哈希
pub fn hash_password(password: &str, salt: &str, version: u32) -> Result<String, CryptoError> {
    let key = derive_key(password, salt, version)?;
    Ok(BASE64.encode(&key))
}

/// 恒定时间验证密码哈希（防止时序攻击）
pub fn verify_password(password: &str, salt: &str, hash: &str, version: u32) -> Result<bool, CryptoError> {
    let key = derive_key(password, salt, version)?;
    let computed = BASE64.encode(&key);
    // 恒定时间比较，防止时序攻击
    Ok(bool::from(computed.as_bytes().ct_eq(hash.as_bytes())))
}
