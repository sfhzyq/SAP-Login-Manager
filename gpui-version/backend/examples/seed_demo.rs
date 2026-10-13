//! 演示数据种子工具（仅用于 README 截图，不属于应用功能）
//!
//! 复用 sap-backend 的 crypto/models，生成与真实数据格式完全一致的加密 store.json。
//! 用法：cargo run -p sap-backend --example seed_demo -- <输出根目录>
//! 生成 <输出根目录>/data/store.json，测试主密码：Demo@2026
//!
//! 结束前做完整自校验：重新读文件 → 验证主密码 → 逐条解密比对明文。

use chrono::{Duration, Utc};
use sap_backend::crypto::{self, KDF_VERSION_ARGON2};
use sap_backend::models::{AppSettings, Credential, Group, StoreData};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

const MASTER: &str = "Demo@2026";
const ENC_VERSION_GCM: u32 = 1;

fn hours_ago(h: i64) -> chrono::DateTime<chrono::Utc> {
    Utc::now() - Duration::hours(h)
}

fn main() {
    let out_root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "demo-data".to_string());
    let data_dir = PathBuf::from(&out_root).join("data");
    fs::create_dir_all(&data_dir).expect("创建输出目录失败");
    let store_path = data_dir.join("store.json");

    // ---- 密钥材料 ----
    let salt = crypto::generate_salt();
    let key = crypto::derive_key(MASTER, &salt, KDF_VERSION_ARGON2).expect("派生失败");
    let hash = crypto::hash_password(MASTER, &salt, KDF_VERSION_ARGON2).expect("哈希失败");

    // ---- 测试凭据（(id, 显示名, 明文密码)；SSO 两条密码为空） ----
    let plain: &[(&str, &str, &str)] = &[
        ("cred-s4p", "S/4HANA 生产系统", "S4p#Prd@2026"),
        ("cred-prd", "ECC 生产系统", "Fico!PRD#0918"),
        ("cred-qas", "ECC 质量测试", "Qas@Client200"),
        ("cred-s4d", "S/4 开发沙箱", "Dev$Sandbox42"),
        ("cred-bwp", "BW 报表生产", "Bw#Prod777!"),
        ("cred-apd", "APO 开发", "Apo@Dev#3311"),
        ("cred-slm", "Solution Manager", "Slm!Basis2026"),
        ("cred-srq", "SRM 供应商门户", "Srm@Test#0300"),
        ("cred-gw1", "Gateway 集成网关", ""),
        ("cred-ides", "IDES 练习系统", "Ides@Train800"),
        ("cred-cfg", "配置客户端", "Cfg$Config066"),
        ("cred-e5p", "ECC5 遗留系统", ""),
    ];
    let pw = |id: &str| plain.iter().find(|(i, _, _)| *i == id).unwrap().2;

    let creds = vec![
        Credential {
            id: "cred-s4p".into(),
            connection_id: "S4P".into(),
            display_name: Some("S/4HANA 生产系统".into()),
            client: "100".into(),
            username: "CONSULTANT".into(),
            encrypted_password: crypto::encrypt_gcm(pw("cred-s4p"), &key).unwrap(),
            language: "ZH".into(),
            connection_type: "direct".into(),
            system_id: "S4P".into(),
            app_server: "10.8.12.21".into(),
            system_number: "00".into(),
            description: "S/4HANA 2023 正式环境 · FICO 模块".into(),
            environment: "production".into(),
            group_id: Some("grp-fico".into()),
            login_count: 128,
            last_login_at: Some(hours_ago(1)),
            color_tag: "red".into(),
            is_favorite: true,
            favorite_order: Some(0),
            uuid: Uuid::new_v4().to_string(),
            created_at: hours_ago(24 * 56),
            updated_at: hours_ago(30),
            ..Default::default()
        },
        Credential {
            id: "cred-prd".into(),
            connection_id: "PRD".into(),
            display_name: Some("ECC 生产系统".into()),
            client: "100".into(),
            username: "FI_CONSULT".into(),
            encrypted_password: crypto::encrypt_gcm(pw("cred-prd"), &key).unwrap(),
            language: "ZH".into(),
            connection_type: "direct".into(),
            system_id: "PRD".into(),
            app_server: "10.8.10.35".into(),
            system_number: "01".into(),
            description: "ECC 6.0 EHP8 · 总账与应收应付".into(),
            environment: "production".into(),
            group_id: Some("grp-fico".into()),
            login_count: 86,
            last_login_at: Some(hours_ago(3)),
            color_tag: "orange".into(),
            is_favorite: true,
            favorite_order: Some(1),
            uuid: Uuid::new_v4().to_string(),
            created_at: hours_ago(24 * 52),
            updated_at: hours_ago(24 * 6),
            ..Default::default()
        },
        Credential {
            id: "cred-qas".into(),
            connection_id: "QAS".into(),
            display_name: Some("ECC 质量测试".into()),
            client: "200".into(),
            username: "CONSULTANT".into(),
            encrypted_password: crypto::encrypt_gcm(pw("cred-qas"), &key).unwrap(),
            language: "ZH".into(),
            connection_type: "direct".into(),
            system_id: "QAS".into(),
            app_server: "10.8.20.35".into(),
            system_number: "00".into(),
            description: "传输前验证环境".into(),
            environment: "test".into(),
            group_id: Some("grp-fico".into()),
            login_count: 45,
            last_login_at: Some(hours_ago(26)),
            color_tag: "green".into(),
            is_favorite: true,
            favorite_order: Some(2),
            uuid: Uuid::new_v4().to_string(),
            created_at: hours_ago(24 * 47),
            updated_at: hours_ago(24 * 9),
            ..Default::default()
        },
        Credential {
            id: "cred-s4d".into(),
            connection_id: "S4D".into(),
            display_name: Some("S/4 开发沙箱".into()),
            client: "100".into(),
            username: "DEV_USER".into(),
            encrypted_password: crypto::encrypt_gcm(pw("cred-s4d"), &key).unwrap(),
            language: "EN".into(),
            connection_type: "direct".into(),
            system_id: "S4D".into(),
            app_server: "10.8.22.14".into(),
            system_number: "00".into(),
            description: "ABAP 开发与键值调整".into(),
            environment: "development".into(),
            login_count: 12,
            last_login_at: Some(hours_ago(24 * 3)),
            color_tag: "blue".into(),
            uuid: Uuid::new_v4().to_string(),
            created_at: hours_ago(24 * 40),
            updated_at: hours_ago(24 * 12),
            ..Default::default()
        },
        Credential {
            id: "cred-bwp".into(),
            connection_id: "BWP".into(),
            display_name: Some("BW 报表生产".into()),
            client: "001".into(),
            username: "BW_ADMIN".into(),
            encrypted_password: crypto::encrypt_gcm(pw("cred-bwp"), &key).unwrap(),
            language: "ZH".into(),
            connection_type: "load_balancing".into(),
            system_id: "BWP".into(),
            message_server: "bwprd.corp.local".into(),
            message_server_port: "3601".into(),
            logon_group: "SPACE".into(),
            description: "BW/4HANA · 管理员 · 消息服务器负载均衡".into(),
            environment: "production".into(),
            group_id: Some("grp-basis".into()),
            login_count: 33,
            last_login_at: Some(hours_ago(24 * 2)),
            color_tag: "blue".into(),
            uuid: Uuid::new_v4().to_string(),
            created_at: hours_ago(24 * 38),
            updated_at: hours_ago(24 * 4),
            ..Default::default()
        },
        Credential {
            id: "cred-apd".into(),
            connection_id: "APD".into(),
            display_name: Some("APO 开发".into()),
            client: "100".into(),
            username: "DEV_USER".into(),
            encrypted_password: crypto::encrypt_gcm(pw("cred-apd"), &key).unwrap(),
            language: "EN".into(),
            connection_type: "direct".into(),
            system_id: "APD".into(),
            app_server: "10.8.22.51".into(),
            system_number: "00".into(),
            description: "供应链计划沙箱".into(),
            environment: "development".into(),
            login_count: 8,
            last_login_at: Some(hours_ago(24 * 7)),
            uuid: Uuid::new_v4().to_string(),
            created_at: hours_ago(24 * 35),
            updated_at: hours_ago(24 * 15),
            ..Default::default()
        },
        Credential {
            id: "cred-slm".into(),
            connection_id: "SLM".into(),
            display_name: Some("Solution Manager".into()),
            client: "001".into(),
            username: "BASIS_ADMIN".into(),
            encrypted_password: crypto::encrypt_gcm(pw("cred-slm"), &key).unwrap(),
            language: "EN".into(),
            connection_type: "direct".into(),
            system_id: "SLM".into(),
            app_server: "10.8.5.30".into(),
            system_number: "00".into(),
            saprouter: "/H/saprouter.corp.cn/H/".into(),
            description: "SolMan 7.2 · 经 SAP Router 接入 · 运维告警".into(),
            environment: "test".into(),
            group_id: Some("grp-basis".into()),
            login_count: 21,
            last_login_at: Some(hours_ago(20)),
            color_tag: "purple".into(),
            uuid: Uuid::new_v4().to_string(),
            created_at: hours_ago(24 * 33),
            updated_at: hours_ago(24 * 8),
            ..Default::default()
        },
        Credential {
            id: "cred-srq".into(),
            connection_id: "SRQ".into(),
            display_name: Some("SRM 供应商门户".into()),
            client: "300".into(),
            username: "MM_CONSULT".into(),
            encrypted_password: crypto::encrypt_gcm(pw("cred-srq"), &key).unwrap(),
            language: "ZH".into(),
            connection_type: "direct".into(),
            system_id: "SRQ".into(),
            app_server: "10.8.20.88".into(),
            system_number: "00".into(),
            description: "SRM 7.0 采购测试".into(),
            environment: "test".into(),
            group_id: Some("grp-mm".into()),
            login_count: 15,
            last_login_at: Some(hours_ago(24 * 4)),
            color_tag: "yellow".into(),
            uuid: Uuid::new_v4().to_string(),
            created_at: hours_ago(24 * 30),
            updated_at: hours_ago(24 * 11),
            ..Default::default()
        },
        Credential {
            id: "cred-gw1".into(),
            connection_id: "GW1".into(),
            display_name: Some("Gateway 集成网关".into()),
            client: "002".into(),
            username: "INTEGRATION".into(),
            language: "EN".into(),
            connection_type: "load_balancing".into(),
            system_id: "GW1".into(),
            message_server: "gwprd.corp.local".into(),
            message_server_port: "3602".into(),
            logon_group: "SPACE".into(),
            description: "SAP Gateway · SNC 单点登录（免密）".into(),
            environment: "production".into(),
            group_id: Some("grp-basis".into()),
            login_count: 52,
            last_login_at: Some(hours_ago(5)),
            color_tag: "green".into(),
            is_favorite: true,
            favorite_order: Some(3),
            snc_enabled: true,
            snc_name: "p:CN=GW1, O=Corp, C=CN".into(),
            snc_qop: "9".into(),
            snc_sso: true,
            uuid: Uuid::new_v4().to_string(),
            created_at: hours_ago(24 * 28),
            updated_at: hours_ago(24 * 2),
            ..Default::default()
        },
        Credential {
            id: "cred-ides".into(),
            connection_id: "IDES".into(),
            display_name: Some("IDES 练习系统".into()),
            client: "800".into(),
            username: "TRAINEE".into(),
            encrypted_password: crypto::encrypt_gcm(pw("cred-ides"), &key).unwrap(),
            language: "EN".into(),
            connection_type: "direct".into(),
            system_id: "IDES".into(),
            app_server: "10.9.1.7".into(),
            system_number: "00".into(),
            description: "SAP 官方演示教学数据集".into(),
            environment: String::new(),
            login_count: 5,
            last_login_at: Some(hours_ago(24 * 6)),
            uuid: Uuid::new_v4().to_string(),
            created_at: hours_ago(24 * 25),
            updated_at: hours_ago(24 * 18),
            ..Default::default()
        },
        Credential {
            id: "cred-cfg".into(),
            connection_id: "CFG".into(),
            display_name: Some("配置客户端".into()),
            client: "066".into(),
            username: "CONSULTANT".into(),
            encrypted_password: crypto::encrypt_gcm(pw("cred-cfg"), &key).unwrap(),
            language: "ZH".into(),
            connection_type: "direct".into(),
            system_id: "CFG".into(),
            app_server: "10.8.30.6".into(),
            system_number: "02".into(),
            description: "全局配置与早期验证".into(),
            environment: "configuration".into(),
            group_id: Some("grp-mm".into()),
            login_count: 9,
            last_login_at: Some(hours_ago(24 * 5)),
            color_tag: "orange".into(),
            uuid: Uuid::new_v4().to_string(),
            created_at: hours_ago(24 * 22),
            updated_at: hours_ago(24 * 14),
            ..Default::default()
        },
        Credential {
            id: "cred-e5p".into(),
            connection_id: "E5P".into(),
            display_name: Some("ECC5 遗留系统".into()),
            client: "100".into(),
            username: "LEGACY_USER".into(),
            language: "ZH".into(),
            connection_type: "direct".into(),
            system_id: "E5P".into(),
            app_server: "10.8.2.9".into(),
            system_number: "00".into(),
            description: "历史归档查询 · SNC 单点登录（免密）".into(),
            environment: "production".into(),
            group_id: Some("grp-basis".into()),
            login_count: 3,
            last_login_at: Some(hours_ago(24 * 28)),
            color_tag: "purple".into(),
            snc_enabled: true,
            snc_name: "p:CN=E5P, O=Corp, C=CN".into(),
            snc_qop: "8".into(),
            snc_sso: true,
            uuid: Uuid::new_v4().to_string(),
            created_at: hours_ago(24 * 20),
            updated_at: hours_ago(24 * 16),
            ..Default::default()
        },
    ];

    let now = Utc::now();
    let groups = vec![
        Group {
            id: "group-default".into(),
            group_name: "默认分组".into(),
            entries: vec![],
            is_system: false,
            is_default: true,
            created_at: now,
            updated_at: now,
        },
        Group {
            id: "group-production".into(),
            group_name: "生产环境".into(),
            entries: vec![],
            is_system: true,
            is_default: false,
            created_at: now,
            updated_at: now,
        },
        Group {
            id: "group-test".into(),
            group_name: "测试环境".into(),
            entries: vec![],
            is_system: true,
            is_default: false,
            created_at: now,
            updated_at: now,
        },
        Group {
            id: "group-development".into(),
            group_name: "开发环境".into(),
            entries: vec![],
            is_system: true,
            is_default: false,
            created_at: now,
            updated_at: now,
        },
        Group {
            id: "group-configuration".into(),
            group_name: "配置环境".into(),
            entries: vec![],
            is_system: true,
            is_default: false,
            created_at: now,
            updated_at: now,
        },
        Group {
            id: "grp-fico".into(),
            group_name: "FICO 项目".into(),
            entries: vec![
                "cred-s4p".into(),
                "cred-prd".into(),
                "cred-qas".into(),
            ],
            is_system: false,
            is_default: false,
            created_at: hours_ago(24 * 45),
            updated_at: hours_ago(24 * 5),
        },
        Group {
            id: "grp-basis".into(),
            group_name: "Basis 运维".into(),
            entries: vec![
                "cred-bwp".into(),
                "cred-slm".into(),
                "cred-gw1".into(),
                "cred-e5p".into(),
            ],
            is_system: false,
            is_default: false,
            created_at: hours_ago(24 * 36),
            updated_at: hours_ago(24 * 3),
        },
        Group {
            id: "grp-mm".into(),
            group_name: "物流项目".into(),
            entries: vec!["cred-srq".into(), "cred-cfg".into()],
            is_system: false,
            is_default: false,
            created_at: hours_ago(24 * 29),
            updated_at: hours_ago(24 * 10),
        },
    ];

    let settings = AppSettings {
        pinned_credential_ids: vec!["cred-s4p".into(), "cred-gw1".into()],
        ..Default::default()
    };

    let store = StoreData {
        salt: salt.clone(),
        master_password_hash: hash,
        credentials: creds,
        groups,
        settings,
        created_at: hours_ago(24 * 60),
        updated_at: now,
        key_derivation_version: KDF_VERSION_ARGON2,
        encryption_version: ENC_VERSION_GCM,
        remembered_master: String::new(),
    };

    let json = serde_json::to_string_pretty(&store).expect("序列化失败");
    fs::write(&store_path, &json).expect("写文件失败");
    println!("[seed] 写出 {}", store_path.display());

    // ---- 自校验：重读 → 验主密码 → 逐条解密比对 ----
    let raw = fs::read_to_string(&store_path).expect("回读失败");
    let loaded: StoreData = serde_json::from_str(&raw).expect("反序列化失败");
    assert!(crypto::verify_password(
        MASTER,
        &loaded.salt,
        &loaded.master_password_hash,
        loaded.key_derivation_version
    )
    .unwrap());
    let vkey =
        crypto::derive_key(MASTER, &loaded.salt, loaded.key_derivation_version).unwrap();
    let mut ok = 0usize;
    let mut sso = 0usize;
    for c in &loaded.credentials {
        if c.encrypted_password.is_empty() {
            sso += 1;
            continue;
        }
        let got = crypto::decrypt_gcm(&c.encrypted_password, &vkey).expect("解密失败");
        assert_eq!(got, pw(&c.id), "凭据 {} 明文不匹配", c.id);
        ok += 1;
    }
    println!(
        "[self-check] PASSED · 主密码验证 ✓ · 加密凭据解密比对 {ok}/{} ✓ · SSO 免密 {sso} 条 ✓ · 分组 {} 个",
        plain.len() - sso,
        loaded.groups.len()
    );
    println!("[seed] 测试主密码：{MASTER}");
}
