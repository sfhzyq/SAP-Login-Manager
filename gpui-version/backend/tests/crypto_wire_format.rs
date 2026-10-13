//! 加密线格式回归测试（Known Answer Tests）
//!
//! 目的：**锁定加密实现的线格式**。密码管理器里已加密的库文件必须跨版本可解，
//! 因此 KDF 参数、CBC 填充、GCM 格式、HMAC 循环一旦变更，此测试会立刻失败。
//!
//! 基线来源：RustCrypto 栈升级（aes 0.8→0.9 / aes-gcm 0.10→0.11 / argon2 0.5→0.6 /
//! pbkdf2 0.12→0.13 / hmac 0.12→0.13 / rand 0.8→0.10）**之前**由旧实现产出，
//! 见 v1.5.0 依赖升级批次。旧库兼容性依赖这些向量保持不变。

use sap_backend::crypto::*;

/// 固定 salt（32 字节 "0123456789abcdef0123456789abcdef" 的 base64）
const SALT_B64: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";
const PW: &str = "master-pw-测试-2026";
const PLAIN: &str = "SAP 密码/凭证 p@ss-1234 · 中文";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn kdf_keys_stable_for_all_versions() {
    // 三个 KDF 版本的派生密钥必须逐字节稳定
    let cases = [
        (
            KDF_VERSION_LEGACY,
            "337b9f5bb808a09c4753939801df8e9b7870618acc8c1b027085ac653b700d1e",
            "M3ufW7gIoJxHU5OYAd+Om3hwYYrMjBsCcIWsZTtwDR4=",
        ),
        (
            KDF_VERSION_PBKDF2,
            "f361dd7a5fda3d6e848b51f67c6cd7ac4d5238083c7844de30df0b431bf36701",
            "82Hdel/aPW6Ei1H2fGzXrE1SOAg8eETeMN8LQxvzZwE=",
        ),
        (
            KDF_VERSION_ARGON2,
            "903bd140016569ec60286e7678d694af279bf232886f789f29bbc57e41a55ab4",
            "kDvRQAFlaexgKG52eNaUryeb8jKIb3ifKbvFfkGlWrQ=",
        ),
    ];
    for (version, expected_key_hex, expected_hash_b64) in cases {
        let key = derive_key(PW, SALT_B64, version).unwrap();
        assert_eq!(hex(&key), expected_key_hex, "KDF v{version} 派生密钥漂移");
        let hash = hash_password(PW, SALT_B64, version).unwrap();
        assert_eq!(hash, expected_hash_b64, "KDF v{version} 密码哈希漂移");
        assert!(
            verify_password(PW, SALT_B64, &hash, version).unwrap(),
            "KDF v{version} 校验失败"
        );
        assert!(
            !verify_password("wrong-pw", SALT_B64, &hash, version).unwrap(),
            "KDF v{version} 错误密码被接受"
        );
    }
}

#[test]
fn legacy_cbc_ciphertext_still_decrypts() {
    let key = derive_key(PW, SALT_B64, KDF_VERSION_ARGON2).unwrap();
    // 旧实现产出的真实密文（IV 已内嵌），必须仍可解密
    let recorded = "FRVParpV6RshcIY9HYFIIYFO41wdcpaH3nvUy6Zz7WPVQ9SUBzX2UEtljZIbpxwd5gOBSd+1Qo28vlktnJND0w==";
    assert_eq!(decrypt(recorded, &key).unwrap(), PLAIN);
}

#[test]
fn gcm_ciphertext_still_decrypts() {
    let key = derive_key(PW, SALT_B64, KDF_VERSION_ARGON2).unwrap();
    // 旧实现产出的真实密文（nonce 已内嵌），必须仍可解密
    let recorded = "YKBX93m++5D1iQ2jXjfbmVAD7tl42bgALIglo/u3uzk3uv0XurD1VWyIre0iLiNbbfaW9l/QQ/KRWM/ZNypaVwY=";
    assert_eq!(decrypt_gcm(recorded, &key).unwrap(), PLAIN);
}

#[test]
fn new_ciphertexts_roundtrip_and_keep_wire_layout() {
    // 新产出密文：长度布局不变（CBC = 16 IV + 16 倍数；GCM = 12 nonce + 密文 + 16 tag）
    let key = derive_key(PW, SALT_B64, KDF_VERSION_ARGON2).unwrap();
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};

    let cbc = encrypt(PLAIN, &key).unwrap();
    let cbc_raw = B64.decode(&cbc).unwrap();
    assert_eq!(
        cbc_raw.len() % 16,
        0,
        "CBC 密文不是 16 字节块的整数倍"
    );
    assert_eq!(decrypt(&cbc, &key).unwrap(), PLAIN);

    let gcm = encrypt_gcm(PLAIN, &key).unwrap();
    let gcm_raw = B64.decode(&gcm).unwrap();
    assert_eq!(
        gcm_raw.len(),
        12 + PLAIN.len() + 16,
        "GCM 线布局应为 nonce(12) + 密文 + tag(16)"
    );
    assert_eq!(decrypt_gcm(&gcm, &key).unwrap(), PLAIN);
}
