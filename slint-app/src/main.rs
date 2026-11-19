#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod models;
mod crypto;
mod storage;
mod sap_automation;
mod sap_parser;
#[cfg(windows)]
mod dpapi;
mod i18n;

slint::include_modules!();

use std::sync::{Arc, Mutex};
use chrono::Utc;
use slint::{SharedString, VecModel, Model};
use models::{Credential, StoreData};

fn main() {
    let _ = env_logger::try_init();

    // Load store
    let store_data = storage::init_store().unwrap_or_else(|e| {
        log::error!("Failed to init store: {}", e);
        storage::create_empty_store()
    });

    let store = Arc::new(Mutex::new(store_data));
    let master_pw = Arc::new(Mutex::new(String::new()));
    let session_pw: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    // Clean up any legacy remembered password
    #[cfg(windows)]
    {
        // Clear remembered_master on startup (in-process session model)
        let s = store.lock().unwrap();
        if !s.remembered_master.is_empty() {
            drop(s);
            let mut s = store.lock().unwrap();
            s.remembered_master = String::new();
            let _ = storage::save_store(&s);
        }
    }

    let app = App::new().unwrap();

    // Determine initial page state
    let is_setup = {
        let s = store.lock().unwrap();
        !storage::is_password_set(&s)
    };
    app.set_page_state(if is_setup { 0 } else { 1 });

    // Load settings and i18n
    {
        let s = store.lock().unwrap();
        let st = &s.settings;
        app.set_settings_password_free(st.password_free);
        app.set_settings_batch_interval(st.batch_login_interval as i32);
        app.set_settings_auto_lock(st.auto_lock_minutes as i32);
        app.set_settings_clipboard_clear(st.clipboard_clear_seconds as i32);
        app.set_settings_group_by_env(st.group_by_environment);
        app.set_settings_compact(st.compact_mode);
        app.set_settings_language(SharedString::from(st.default_language.as_str()));
        app.set_settings_theme(SharedString::from(st.theme.as_str()));
        app.set_settings_sap_logon_path(SharedString::from(st.sap_logon_path.as_deref().unwrap_or("")));

        // Apply initial theme (dark/light) to the Theme global
        app.global::<Theme>().set_dark(st.theme == "dark");

        let t = i18n::get_strings(&st.default_language);
        app.set_t_title(SharedString::from(t.title));
        app.set_t_unlock_prompt(SharedString::from(t.unlock_prompt));
        app.set_t_unlock_btn(SharedString::from(t.unlock_btn));
        app.set_t_setup_prompt(SharedString::from(t.setup_prompt));
        app.set_t_setup_confirm(SharedString::from(t.setup_confirm));
        app.set_t_search_placeholder(SharedString::from(t.search_placeholder));
        app.set_t_add(SharedString::from(t.add));
        app.set_t_login(SharedString::from(t.login));
        app.set_t_batch_login(SharedString::from(t.batch_login));
        app.set_t_copy_password(SharedString::from(t.copy_password));
        app.set_t_reveal_password(SharedString::from(t.reveal_password));
        app.set_t_edit(SharedString::from(t.edit));
        app.set_t_delete(SharedString::from(t.delete));
        app.set_t_favorite(SharedString::from(t.favorite));
        app.set_t_cancel_favorite(SharedString::from(t.cancel_favorite));
        app.set_t_share(SharedString::from(t.share));
        app.set_t_settings(SharedString::from(t.settings));
        app.set_t_save(SharedString::from(t.save));
        app.set_t_cancel(SharedString::from(t.cancel));
        app.set_t_confirm_delete(SharedString::from(t.confirm_delete));
        app.set_t_delete_warning(SharedString::from(t.delete_warning));
        app.set_t_group_all(SharedString::from(t.group_all));
        app.set_t_group_favorites(SharedString::from(t.group_favorites));
        app.set_t_lock(SharedString::from(t.lock));
        app.set_t_sap_logon(SharedString::from(t.sap_logon));
        app.set_t_import(SharedString::from(t.import));
        app.set_t_export(SharedString::from(t.export));
        app.set_t_theme_light(SharedString::from(t.theme_light));
        app.set_t_theme_dark(SharedString::from(t.theme_dark));
        app.set_t_password_free(SharedString::from(t.password_free));
        app.set_t_password_free_hint(SharedString::from(t.password_free_hint));
        app.set_t_batch_interval(SharedString::from(t.batch_interval));
        app.set_t_auto_lock(SharedString::from(t.auto_lock));
        app.set_t_clipboard_clear(SharedString::from(t.clipboard_clear));
        app.set_t_group_by_env(SharedString::from(t.group_by_env));
        app.set_t_compact_mode(SharedString::from(t.compact_mode));
        app.set_t_language(SharedString::from(t.language));
        app.set_t_reveal_title(SharedString::from(t.reveal_title));
        app.set_t_reveal_verify(SharedString::from(t.reveal_verify));
        app.set_t_reveal_verify_btn(SharedString::from(t.reveal_verify_btn));
        app.set_t_reveal_password_label(SharedString::from(t.reveal_password_label));
        app.set_t_reveal_copy(SharedString::from(t.reveal_copy));
        app.set_t_reveal_auto_hide(SharedString::from(t.reveal_auto_hide));
        app.set_t_connection_name(SharedString::from(t.connection_name));
        app.set_t_client(SharedString::from(t.client));
        app.set_t_username(SharedString::from(t.username));
        app.set_t_password(SharedString::from(t.password));
        app.set_t_language_field(SharedString::from(t.language_field));
        app.set_t_system_id(SharedString::from(t.system_id));
        app.set_t_environment(SharedString::from(t.environment));
        app.set_t_description(SharedString::from(t.description));
        app.set_t_snc(SharedString::from(t.snc));
        app.set_t_snc_sso(SharedString::from(t.snc_sso));
        app.set_t_empty_list(SharedString::from(t.empty_list));
        app.set_t_no_creds_selected(SharedString::from(t.no_creds_selected));
        app.set_t_group_manage(SharedString::from(t.group_manage));
        app.set_t_group_create(SharedString::from(t.group_create));
        app.set_t_group_rename(SharedString::from(t.group_rename));
        app.set_t_group_delete(SharedString::from(t.group_delete));
        app.set_t_group_delete_warn(SharedString::from(t.group_delete_warn));
        app.set_t_group_delete_move(SharedString::from(t.group_delete_move));
        app.set_t_group_delete_all(SharedString::from(t.group_delete_all));
        app.set_t_group_section_system(SharedString::from(t.group_section_system));
        app.set_t_group_section_custom(SharedString::from(t.group_section_custom));
        app.set_t_group_name_prompt(SharedString::from(t.group_name_prompt));
        app.set_t_group_empty(SharedString::from(t.group_empty));
        app.set_t_batch_move(SharedString::from(t.batch_move));
        app.set_t_batch_select(SharedString::from(t.batch_select));
        app.set_t_batch_cancel(SharedString::from(t.batch_cancel));
        app.set_t_batch_selected(SharedString::from(t.batch_selected));
    }

    // init callback - built-in, no explicit declaration needed

    // Unlock callback
    let store_c2 = store.clone();
    let master_pw_c2 = master_pw.clone();
    let session_pw_c2 = session_pw.clone();
    let app_handle2 = app.as_weak();
    app.on_unlock(move |pw| {
        let pw_str = pw.as_str().to_string();
        if let Some(a) = app_handle2.upgrade() {
            a.set_unlock_error(SharedString::from(""));
        }
        let verify_result = {
            let s = store_c2.lock().unwrap();
            storage::verify_master_password(&s, &pw_str)
        };
        match verify_result {
            Ok(true) => {
                // Try migration if needed
                {
                    let mut s = store_c2.lock().unwrap();
                    if s.key_derivation_version < 1 || s.encryption_version < 1 {
                        let _ = storage::migrate_key_derivation(&mut s, &pw_str);
                    }
                }
                *master_pw_c2.lock().unwrap() = pw_str.clone();
                {
                    let s = store_c2.lock().unwrap();
                    if s.settings.password_free {
                        *session_pw_c2.lock().unwrap() = Some(pw_str);
                    }
                }
                load_credentials_to_ui(&store_c2, &app_handle2);
                if let Some(a) = app_handle2.upgrade() { a.set_page_state(2); }
            }
            Ok(false) => {
                if let Some(a) = app_handle2.upgrade() {
                    a.set_unlock_error(SharedString::from("密码错误，请重试"));
                }
            }
            Err(e) => {
                log::error!("Unlock error: {}", e);
                if let Some(a) = app_handle2.upgrade() {
                    a.set_unlock_error(SharedString::from(format!("错误: {}", e)));
                }
            }
        }
    });

    // Setup callback
    let store_c3 = store.clone();
    let master_pw_c3 = master_pw.clone();
    let session_pw_c3 = session_pw.clone();
    let app_handle3 = app.as_weak();
    app.on_setup(move |pw| {
        let pw_str = pw.as_str().to_string();
        let set_result = {
            let mut s = store_c3.lock().unwrap();
            storage::set_master_password(&mut s, &pw_str)
        };
        match set_result {
            Ok(()) => {
                *master_pw_c3.lock().unwrap() = pw_str.clone();
                {
                    let s = store_c3.lock().unwrap();
                    if s.settings.password_free {
                        *session_pw_c3.lock().unwrap() = Some(pw_str);
                    }
                }
                load_credentials_to_ui(&store_c3, &app_handle3);
                if let Some(a) = app_handle3.upgrade() { a.set_page_state(2); }
            }
            Err(e) => {
                log::error!("Setup error: {}", e);
                if let Some(a) = app_handle3.upgrade() {
                    a.set_unlock_error(SharedString::from(format!("错误: {}", e)));
                }
            }
        }
    });

    // Login callback
    let store_c4 = store.clone();
    let master_pw_c4 = master_pw.clone();
    app.on_login(move |cred_id| {
        let id = cred_id.as_str().to_string();
        let mp = master_pw_c4.lock().unwrap().clone();
        let s = store_c4.lock().unwrap();
        let cred = s.credentials.iter().find(|c| c.id == id).cloned();
        drop(s);
        if let Some(cred) = cred {
            let password = if cred.snc_enabled && cred.snc_sso {
                String::new()
            } else {
                let s = store_c4.lock().unwrap();
                storage::decrypt_credential_password(&s, &mp, &cred.encrypted_password)
                    .unwrap_or_default()
            };
            let params = sap_automation::SapLoginParams {
                connection_id: cred.connection_id.clone(),
                client: cred.client.clone(),
                username: cred.username.clone(),
                password,
                language: cred.language.clone(),
                connection_type: cred.connection_type.clone(),
                system_id: cred.system_id.clone(),
                app_server: cred.app_server.clone(),
                system_number: cred.system_number.clone(),
                message_server: cred.message_server.clone(),
                message_server_port: cred.message_server_port.clone(),
                logon_group: cred.logon_group.clone(),
                saprouter: cred.saprouter.clone(),
                uuid: cred.uuid.clone(),
                snc_enabled: cred.snc_enabled,
                snc_name: cred.snc_name.clone(),
                snc_qop: cred.snc_qop.clone(),
                snc_sso: cred.snc_sso,
            };
            if let Err(e) = sap_automation::login_to_sap(&params) {
                log::error!("SAP login failed: {}", e);
            } else {
                let mut s = store_c4.lock().unwrap();
                if let Some(c) = s.credentials.iter_mut().find(|c| c.id == id) {
                    c.login_count += 1;
                    c.last_login_at = Some(Utc::now());
                }
                let _ = storage::save_store(&s);
            }
        }
    });

    // Batch login
    let store_c5 = store.clone();
    let master_pw_c5 = master_pw.clone();
    app.on_batch_login(move || {
        let mp = master_pw_c5.lock().unwrap().clone();
        let s = store_c5.lock().unwrap();
        let interval = s.settings.batch_login_interval as u64;
        let (creds, key, ev) = {
            let key = crate::crypto::derive_key(&mp, &s.salt, s.key_derivation_version).unwrap();
            (s.credentials.iter().cloned().collect::<Vec<_>>(), key, s.encryption_version)
        };
        drop(s);
        for cred in &creds {
            if cred.username.is_empty() { continue; }
            let password = if ev >= 1 {
                crate::crypto::decrypt_gcm(&cred.encrypted_password, &key).unwrap_or_default()
            } else {
                crate::crypto::decrypt(&cred.encrypted_password, &key).unwrap_or_default()
            };
            let params = sap_automation::SapLoginParams {
                connection_id: cred.connection_id.clone(),
                client: cred.client.clone(),
                username: cred.username.clone(),
                password,
                language: cred.language.clone(),
                connection_type: cred.connection_type.clone(),
                system_id: cred.system_id.clone(),
                app_server: cred.app_server.clone(),
                system_number: cred.system_number.clone(),
                message_server: cred.message_server.clone(),
                message_server_port: cred.message_server_port.clone(),
                logon_group: cred.logon_group.clone(),
                saprouter: cred.saprouter.clone(),
                uuid: cred.uuid.clone(),
                snc_enabled: cred.snc_enabled,
                snc_name: cred.snc_name.clone(),
                snc_qop: cred.snc_qop.clone(),
                snc_sso: cred.snc_sso,
            };
            let _ = sap_automation::login_to_sap(&params);
            if interval > 0 { std::thread::sleep(std::time::Duration::from_secs(interval)); }
        }
        let mut s = store_c5.lock().unwrap();
        let now = Utc::now();
        for cred in &creds {
            if let Some(c) = s.credentials.iter_mut().find(|c| c.id == cred.id) {
                if !c.username.is_empty() {
                    c.login_count += 1;
                    c.last_login_at = Some(now);
                }
            }
        }
        let _ = storage::save_store(&s);
    });

    // Copy password
    let store_c6 = store.clone();
    let master_pw_c6 = master_pw.clone();
    app.on_copy_password(move |cred_id| {
        let id = cred_id.as_str().to_string();
        let mp = master_pw_c6.lock().unwrap().clone();
        let s = store_c6.lock().unwrap();
        if let Ok(plain) = storage::decrypt_credential_password(&s, &mp, &{
            s.credentials.iter().find(|c| c.id == id).map(|c| c.encrypted_password.clone()).unwrap_or_default()
        }) {
            let _ = arboard::Clipboard::new().and_then(|mut cb| cb.set_text(plain));
        }
    });

    // Reveal password - set cred name and show modal
    let store_c7 = store.clone();
    let app_handle7 = app.as_weak();
    app.on_reveal_password(move |cred_id| {
        let id = cred_id.as_str().to_string();
        let s = store_c7.lock().unwrap();
        if let Some(cred) = s.credentials.iter().find(|c| c.id == id) {
            let name = cred.display_name.clone().unwrap_or_else(|| cred.connection_id.clone());
            if let Some(a) = app_handle7.upgrade() {
                a.set_reveal_cred_id(SharedString::from(id));
                a.set_reveal_cred_name(SharedString::from(name));
                a.set_reveal_verified(false);
                a.set_revealed_password(SharedString::from(""));
                a.set_show_reveal_modal(true);
            }
        } else {
            // New credential being created - no reveal needed
        }
    });

    // Verify reveal - check master password and decrypt
    let store_c8 = store.clone();
    let master_pw_c8 = master_pw.clone();
    let app_handle8 = app.as_weak();
    app.on_verify_reveal(move |pw| {
        let pw_str = pw.as_str().to_string();
        let s = store_c8.lock().unwrap();
        match storage::verify_master_password(&s, &pw_str) {
            Ok(true) => {
                let cred_id = if let Some(a) = app_handle8.upgrade() {
                    a.get_reveal_cred_id().as_str().to_string()
                } else { return; };
                let mp = master_pw_c8.lock().unwrap().clone();
                if let Some(cred) = s.credentials.iter().find(|c| c.id == cred_id) {
                    let plain = storage::decrypt_credential_password(&s, &mp, &cred.encrypted_password)
                        .unwrap_or_default();
                    drop(s);
                    if let Some(a) = app_handle8.upgrade() {
                        a.set_revealed_password(SharedString::from(plain));
                        a.set_reveal_verified(true);
                    }
                    // Auto-hide after 20 seconds
                    let ah = app_handle8.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(20));
                        if let Some(a) = ah.upgrade() {
                            a.set_show_reveal_modal(false);
                            a.set_reveal_verified(false);
                            a.set_revealed_password(SharedString::from(""));
                        }
                    });
                }
            }
            Ok(false) => {
                drop(s);
                if let Some(a) = app_handle8.upgrade() {
                    a.set_t_reveal_verify(SharedString::from("密码错误，请重试"));
                }
            }
            Err(e) => {
                drop(s);
                log::error!("Verify reveal error: {}", e);
            }
        }
    });

    // Save credential
    let store_c9 = store.clone();
    let master_pw_c9 = master_pw.clone();
    let app_handle9 = app.as_weak();

    // Edit credential - load data into editing-cred and show form
    let store_edit = store.clone();
    let app_handle_edit = app.as_weak();
    app.on_edit_credential(move |cred_id| {
        let id = cred_id.as_str().to_string();
        if id.is_empty() {
            // New credential
            if let Some(a) = app_handle_edit.upgrade() {
                a.set_show_credential_form(true);
            }
            return;
        }
        let s = store_edit.lock().unwrap();
        if let Some(cred) = s.credentials.iter().find(|c| c.id == id) {
            if let Some(a) = app_handle_edit.upgrade() {
                a.set_editing_cred(CredentialItem {
                    id: SharedString::from(cred.id.as_str()),
                    connection_id: SharedString::from(cred.connection_id.as_str()),
                    display_name: SharedString::from(cred.display_name.as_deref().unwrap_or("")),
                    client: SharedString::from(cred.client.as_str()),
                    username: SharedString::from(cred.username.as_str()),
                    language: SharedString::from(cred.language.as_str()),
                    system_id: SharedString::from(cred.system_id.as_str()),
                    environment: SharedString::from(cred.environment.as_str()),
                    color_tag: SharedString::from(""), // password left blank for security
                    is_favorite: cred.is_favorite,
                    login_count: cred.login_count as i32,
                    group_id: SharedString::from(cred.group_id.as_deref().unwrap_or("")),
                    snc_enabled: cred.snc_enabled,
                    description: SharedString::from(cred.description.as_str()),
                    is_selected: false,
                });
                a.set_show_credential_form(true);
            }
        }
    });

    app.on_save_credential(move || {
        let mp = master_pw_c9.lock().unwrap().clone();
        let editing = if let Some(a) = app_handle9.upgrade() { a.get_editing_cred() } else { return; };

        let mut cred = Credential {
            id: editing.id.as_str().to_string(),
            connection_id: editing.connection_id.as_str().to_string(),
            client: editing.client.as_str().to_string(),
            username: editing.username.as_str().to_string(),
            password: editing.color_tag.as_str().to_string(), // password temporarily stored in color_tag
            language: editing.language.as_str().to_string(),
            system_id: editing.system_id.as_str().to_string(),
            environment: editing.environment.as_str().to_string(),
            color_tag: String::new(),
            is_favorite: editing.is_favorite,
            login_count: editing.login_count as u32,
            group_id: if editing.group_id.as_str().is_empty() { None } else { Some(editing.group_id.as_str().to_string()) },
            snc_enabled: editing.snc_enabled,
            description: editing.description.as_str().to_string(),
            display_name: if editing.display_name.as_str().is_empty() { Some(editing.connection_id.as_str().to_string()) } else { Some(editing.display_name.as_str().to_string()) },
            ..Default::default()
        };

        let mut s = store_c9.lock().unwrap();
        if cred.id.is_empty() {
            // Add new
            cred.id = uuid::Uuid::new_v4().to_string();
            cred.encrypted_password = if !cred.password.is_empty() {
                storage::encrypt_credential_password(&s, &mp, &cred.password).unwrap_or_default()
            } else { String::new() };
            cred.password = String::new();
            cred.created_at = Utc::now();
            cred.updated_at = Utc::now();
            // Auto-assign to system group based on environment
            let resolved = storage::resolve_group_id(&s, &cred);
            cred.group_id = resolved;
            s.credentials.push(cred);
        } else {
            // Update existing
            if let Some(idx) = s.credentials.iter().position(|c| c.id == cred.id) {
                if !cred.password.is_empty() {
                    cred.encrypted_password = storage::encrypt_credential_password(&s, &mp, &cred.password).unwrap_or_default();
                } else {
                    cred.encrypted_password = s.credentials[idx].encrypted_password.clone();
                }
                cred.password = String::new();
                cred.login_count = s.credentials[idx].login_count;
                cred.last_login_at = s.credentials[idx].last_login_at;
                cred.updated_at = Utc::now();
                // Auto-assign group if not custom
                let resolved = storage::resolve_group_id(&s, &cred);
                cred.group_id = resolved;
                s.credentials[idx] = cred;
            }
        }
        let _ = storage::save_store(&s);
        drop(s);
        if let Some(a) = app_handle9.upgrade() {
            a.set_show_credential_form(false);
        }
        load_credentials_to_ui(&store_c9, &app_handle9);
    });

    // Delete credential
    let store_c10 = store.clone();
    let app_handle10 = app.as_weak();
    app.on_delete_credential(move |cred_id| {
        let id = cred_id.as_str().to_string();
        let mut s = store_c10.lock().unwrap();
        s.credentials.retain(|c| c.id != id);
        for g in &mut s.groups { g.entries.retain(|e| e != &id); }
        let _ = storage::save_store(&s);
        drop(s);
        if let Some(a) = app_handle10.upgrade() {
            a.set_show_credential_form(false);
            a.set_show_delete_confirm(false);
        }
        load_credentials_to_ui(&store_c10, &app_handle10);
    });

    // Toggle favorite
    let store_c11 = store.clone();
    app.on_toggle_favorite(move |cred_id| {
        let id = cred_id.as_str().to_string();
        let mut s = store_c11.lock().unwrap();
        let max_order = s.credentials.iter().filter_map(|c| c.favorite_order).max().unwrap_or(0);
        if let Some(cred) = s.credentials.iter_mut().find(|c| c.id == id) {
            cred.is_favorite = !cred.is_favorite;
            if cred.is_favorite {
                cred.favorite_order = Some(max_order + 1);
            } else {
                cred.favorite_order = None;
            }
            cred.updated_at = Utc::now();
        }
        let _ = storage::save_store(&s);
    });

    // Share credential
    let store_c12 = store.clone();
    app.on_share_credential(move |cred_id| {
        let id = cred_id.as_str().to_string();
        let s = store_c12.lock().unwrap();
        if let Some(cred) = s.credentials.iter().find(|c| c.id == id) {
            let mut lines = Vec::new();
            lines.push(format!("系统标识: {}", cred.connection_id));
            if let Some(ref dn) = cred.display_name { if !dn.is_empty() { lines.push(format!("显示名称: {}", dn)); } }
            lines.push(format!("客户端: {}", cred.client));
            lines.push(format!("用户名: {}", cred.username));
            lines.push(format!("语言: {}", cred.language));
            let text = lines.join("\n");
            let _ = arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text));
        }
    });

    // Open SAP Logon
    let store_c13 = store.clone();
    app.on_open_sap_logon(move || {
        let s = store_c13.lock().unwrap();
        let path = s.settings.sap_logon_path.as_deref();
        let _ = sap_automation::open_sap_logon(path);
    });

    // Import SAP connections
    let store_c14 = store.clone();
    let app_handle14 = app.as_weak();
    app.on_import_sap_connections(move || {
        let connections = sap_parser::get_all_sap_connections().unwrap_or_default();
        if connections.is_empty() { return; }
        let mut s = store_c14.lock().unwrap();
        let default_group = storage::get_default_group_id(&s).unwrap_or_default();
        let now = Utc::now();
        let mut imported = 0;
        for conn in connections {
            if s.credentials.iter().any(|c| c.connection_id == conn.name) { continue; }
            let cred = Credential {
                id: uuid::Uuid::new_v4().to_string(),
                connection_id: conn.name.clone(),
                system_id: conn.system_id.clone().unwrap_or_default(),
                app_server: conn.server.clone().unwrap_or_default(),
                system_number: conn.system_number.clone().unwrap_or_default(),
                language: "ZH".to_string(),
                connection_type: conn.connection_type.clone().unwrap_or_else(|| "direct".into()),
                message_server: conn.message_server.clone().unwrap_or_default(),
                message_server_port: conn.message_server_port.clone().unwrap_or_default(),
                logon_group: conn.logon_group.clone().or(conn.group.clone()).unwrap_or_default(),
                saprouter: conn.saprouter.clone().unwrap_or_default(),
                description: conn.description.clone().unwrap_or_default(),
                uuid: conn.uuid.clone().unwrap_or_default(),
                snc_enabled: conn.sncop.as_deref().map(|s| s != "-1" && !s.is_empty()).unwrap_or(false),
                snc_name: conn.snc_name.clone().unwrap_or_default(),
                snc_qop: conn.sncop.clone().filter(|s| s != "-1").unwrap_or_default(),
                display_name: Some(conn.name.clone()),
                group_id: Some(default_group.clone()),
                created_at: now,
                updated_at: now,
                ..Default::default()
            };
            let _ = storage::add_credential_to_group(&mut s, &cred.id, &default_group);
            s.credentials.push(cred);
            imported += 1;
        }
        if imported > 0 { let _ = storage::save_store(&s); }
        drop(s);
        load_credentials_to_ui(&store_c14, &app_handle14);
    });

    // Export credentials
    let store_c15 = store.clone();
    app.on_export_credentials(move || {
        let s = store_c15.lock().unwrap();
        let bundle = models::ExportBundle {
            app_version: "2.0.0".into(),
            exported_at: Utc::now(),
            group_count: s.groups.len(),
            connection_count: s.credentials.len(),
            has_landscape: false,
            source_salt: s.salt.clone(),
            credentials: s.credentials.clone(),
            groups: s.groups.clone(),
        };
        let json = serde_json::to_string_pretty(&bundle).unwrap_or_default();
        let _ = arboard::Clipboard::new().and_then(|mut cb| cb.set_text(json));
    });

    // Lock
    let master_pw_c16 = master_pw.clone();
    let session_pw_c16 = session_pw.clone();
    let app_handle16 = app.as_weak();
    app.on_lock(move || {
        *master_pw_c16.lock().unwrap() = String::new();
        *session_pw_c16.lock().unwrap() = None;
        if let Some(a) = app_handle16.upgrade() { a.set_page_state(1); }
    });

    // Save settings
    let store_c17 = store.clone();
    let session_pw_c17 = session_pw.clone();
    let master_pw_c17 = master_pw.clone();
    let app_handle17 = app.as_weak();
    app.on_save_settings(move || {
        let (pf, bi, al, cc, gbe, comp, lang, theme, path) = if let Some(a) = app_handle17.upgrade() {
            (a.get_settings_password_free(), a.get_settings_batch_interval(), a.get_settings_auto_lock(),
             a.get_settings_clipboard_clear(), a.get_settings_group_by_env(), a.get_settings_compact(),
             a.get_settings_language().as_str().to_string(), a.get_settings_theme().as_str().to_string(),
             a.get_settings_sap_logon_path().as_str().to_string())
        } else { return };

        let mut s = store_c17.lock().unwrap();
        let was_free = s.settings.password_free;
        s.settings.password_free = pf;
        s.settings.batch_login_interval = bi as u32;
        s.settings.auto_lock_minutes = al as u32;
        s.settings.clipboard_clear_seconds = cc as u32;
        s.settings.group_by_environment = gbe;
        s.settings.compact_mode = comp;
        s.settings.default_language = lang.clone();
        s.settings.theme = theme.clone();
        s.settings.sap_logon_path = if path.is_empty() { None } else { Some(path) };

        // Session password management
        if pf && !was_free {
            let mp = master_pw_c17.lock().unwrap().clone();
            if !mp.is_empty() { *session_pw_c17.lock().unwrap() = Some(mp); }
        } else if !pf && was_free {
            *session_pw_c17.lock().unwrap() = None;
        }

        let _ = storage::save_store(&s);
        drop(s);

        // Update i18n
        let t = i18n::get_strings(&lang);
        if let Some(a) = app_handle17.upgrade() {
            a.set_t_search_placeholder(SharedString::from(t.search_placeholder));
            a.set_t_add(SharedString::from(t.add));
            // ... more i18n updates can be added
        }
    });

    // Apply theme
    let app_handle_theme = app.as_weak();
    app.on_apply_theme(move |theme| {
        if let Some(a) = app_handle_theme.upgrade() {
            a.global::<Theme>().set_dark(theme.as_str() == "dark");
        }
    });

    // Apply language
    let _store_c18 = store.clone();
    let app_handle18 = app.as_weak();
    app.on_apply_language(move |lang| {
        let lang_str = lang.as_str().to_string();
        let t = i18n::get_strings(&lang_str);
        if let Some(a) = app_handle18.upgrade() {
            a.set_t_title(SharedString::from(t.title));
            a.set_t_unlock_prompt(SharedString::from(t.unlock_prompt));
            a.set_t_unlock_btn(SharedString::from(t.unlock_btn));
            a.set_t_setup_prompt(SharedString::from(t.setup_prompt));
            a.set_t_setup_confirm(SharedString::from(t.setup_confirm));
            a.set_t_search_placeholder(SharedString::from(t.search_placeholder));
            a.set_t_add(SharedString::from(t.add));
            a.set_t_login(SharedString::from(t.login));
            a.set_t_batch_login(SharedString::from(t.batch_login));
            a.set_t_copy_password(SharedString::from(t.copy_password));
            a.set_t_reveal_password(SharedString::from(t.reveal_password));
            a.set_t_edit(SharedString::from(t.edit));
            a.set_t_delete(SharedString::from(t.delete));
            a.set_t_favorite(SharedString::from(t.favorite));
            a.set_t_cancel_favorite(SharedString::from(t.cancel_favorite));
            a.set_t_share(SharedString::from(t.share));
            a.set_t_settings(SharedString::from(t.settings));
            a.set_t_save(SharedString::from(t.save));
            a.set_t_cancel(SharedString::from(t.cancel));
            a.set_t_confirm_delete(SharedString::from(t.confirm_delete));
            a.set_t_delete_warning(SharedString::from(t.delete_warning));
            a.set_t_group_all(SharedString::from(t.group_all));
            a.set_t_group_favorites(SharedString::from(t.group_favorites));
            a.set_t_lock(SharedString::from(t.lock));
            a.set_t_sap_logon(SharedString::from(t.sap_logon));
            a.set_t_import(SharedString::from(t.import));
            a.set_t_export(SharedString::from(t.export));
            a.set_t_empty_list(SharedString::from(t.empty_list));
            a.set_t_no_creds_selected(SharedString::from(t.no_creds_selected));
            a.set_t_group_manage(SharedString::from(t.group_manage));
            a.set_t_group_create(SharedString::from(t.group_create));
            a.set_t_group_rename(SharedString::from(t.group_rename));
            a.set_t_group_delete(SharedString::from(t.group_delete));
            a.set_t_group_delete_warn(SharedString::from(t.group_delete_warn));
            a.set_t_group_delete_move(SharedString::from(t.group_delete_move));
            a.set_t_group_delete_all(SharedString::from(t.group_delete_all));
            a.set_t_group_section_system(SharedString::from(t.group_section_system));
            a.set_t_group_section_custom(SharedString::from(t.group_section_custom));
            a.set_t_group_name_prompt(SharedString::from(t.group_name_prompt));
            a.set_t_group_empty(SharedString::from(t.group_empty));
            a.set_t_batch_move(SharedString::from(t.batch_move));
            a.set_t_batch_select(SharedString::from(t.batch_select));
            a.set_t_batch_cancel(SharedString::from(t.batch_cancel));
            a.set_t_batch_selected(SharedString::from(t.batch_selected));
        }
    });

    // Select credential - toggle selection in batch mode
    let store_sel = store.clone();
    let app_handle_sel = app.as_weak();
    app.on_select_credential(move |id, checked| {
        if let Some(a) = app_handle_sel.upgrade() {
            let cred_id = id.as_str().to_string();
            if !a.get_batch_mode() {
                a.set_batch_mode(true);
            }
            let mut updated: Vec<CredentialItem> = a.get_selected_creds().iter().map(|c| c.clone()).collect();
            if checked {
                if !updated.iter().any(|c| c.id.as_str() == cred_id.as_str()) {
                    let all_creds: Vec<CredentialItem> = a.get_filtered_credentials().iter().map(|c| c.clone()).collect();
                    if let Some(cred) = all_creds.iter().find(|c| c.id.as_str() == cred_id.as_str()) {
                        updated.push(cred.clone());
                    }
                }
            } else {
                updated.retain(|c| c.id.as_str() != cred_id.as_str());
            }
            let model = std::rc::Rc::new(VecModel::from(updated));
            a.set_selected_creds(slint::ModelRc::from(model));
            // Refresh filtered list to update checkbox states
            load_filtered_credentials(&store_sel, &app_handle_sel);
        }
    });

    // Search changed — re-filter credentials
    let store_search = store.clone();
    let app_handle_search = app.as_weak();
    app.on_search_changed(move |_text| {
        load_filtered_credentials(&store_search, &app_handle_search);
    });

    // Group changed — re-filter credentials
    let store_group = store.clone();
    let app_handle_group = app.as_weak();
    app.on_group_changed(move |_gid| {
        load_filtered_credentials(&store_group, &app_handle_group);
    });

    // Create group
    let store_cg = store.clone();
    let app_handle_cg = app.as_weak();
    app.on_create_group(move |name| {
        let group_name = name.as_str().to_string();
        if group_name.is_empty() { return; }
        let mut s = store_cg.lock().unwrap();
        storage::add_group(&mut s, &group_name);
        let _ = storage::save_store(&s);
        drop(s);
        load_credentials_to_ui(&store_cg, &app_handle_cg);
        if let Some(a) = app_handle_cg.upgrade() {
            a.set_show_create_group(false);
        }
    });

    // Rename group
    let store_rg = store.clone();
    let app_handle_rg = app.as_weak();
    app.on_rename_group(move |group_id, new_name| {
        let gid = group_id.as_str().to_string();
        let name = new_name.as_str().to_string();
        if name.is_empty() { return; }
        let mut s = store_rg.lock().unwrap();
        let _ = storage::rename_group(&mut s, &gid, &name);
        let _ = storage::save_store(&s);
        drop(s);
        load_credentials_to_ui(&store_rg, &app_handle_rg);
        if let Some(a) = app_handle_rg.upgrade() {
            a.set_show_rename_group(false);
        }
    });

    // Delete group
    let store_dg = store.clone();
    let app_handle_dg = app.as_weak();
    app.on_delete_group(move |group_id, mode| {
        let gid = group_id.as_str().to_string();
        let mode_str = mode.as_str().to_string();
        let mut s = store_dg.lock().unwrap();
        let _ = storage::delete_group_with_options(&mut s, &gid, &mode_str);
        let _ = storage::save_store(&s);
        drop(s);
        // Reset to "all" if the deleted group was active
        if let Some(a) = app_handle_dg.upgrade() {
            a.set_active_group(SharedString::from("all"));
        }
        load_credentials_to_ui(&store_dg, &app_handle_dg);
        if let Some(a) = app_handle_dg.upgrade() {
            a.set_show_delete_group(false);
        }
    });

    // Batch move to group
    let store_bmt = store.clone();
    let app_handle_bmt = app.as_weak();
    app.on_batch_move_to_group(move |gid| {
        let group_id = gid.as_str().to_string();
        let selected: Vec<String> = if let Some(a) = app_handle_bmt.upgrade() {
            a.get_selected_creds().iter().map(|c| c.id.as_str().to_string()).collect()
        } else { return };
        if selected.is_empty() { return; }
        let mut s = store_bmt.lock().unwrap();
        let _ = storage::batch_move_to_group(&mut s, &selected, &group_id);
        let _ = storage::save_store(&s);
        drop(s);
        load_credentials_to_ui(&store_bmt, &app_handle_bmt);
        if let Some(a) = app_handle_bmt.upgrade() {
            a.set_batch_mode(false);
            a.set_selected_creds(slint::ModelRc::from(std::rc::Rc::new(VecModel::from(Vec::new()))));
        }
    });

    // Batch favorite
    app.on_batch_favorite(move |_fav| {});

    // Change password
    let _store_c19 = store.clone();
    app.on_change_password(move |_old, _new| {
        // Will be implemented
    });

    // Restore credential
    let store_c20 = store.clone();
    let app_handle20 = app.as_weak();
    app.on_restore_credential(move |cred| {
        let mut s = store_c20.lock().unwrap();
        let cred_data = Credential {
            id: cred.id.as_str().to_string(),
            connection_id: cred.connection_id.as_str().to_string(),
            client: cred.client.as_str().to_string(),
            username: cred.username.as_str().to_string(),
            language: cred.language.as_str().to_string(),
            system_id: cred.system_id.as_str().to_string(),
            environment: cred.environment.as_str().to_string(),
            is_favorite: cred.is_favorite,
            login_count: cred.login_count as u32,
            group_id: if cred.group_id.as_str().is_empty() { None } else { Some(cred.group_id.as_str().to_string()) },
            snc_enabled: cred.snc_enabled,
            description: cred.description.as_str().to_string(),
            ..Default::default()
        };
        if !s.credentials.iter().any(|c| c.id == cred_data.id) {
            if let Some(ref gid) = cred_data.group_id {
                let _ = storage::add_credential_to_group(&mut s, &cred_data.id, gid);
            }
            s.credentials.push(cred_data);
            let _ = storage::save_store(&s);
        }
        drop(s);
        load_credentials_to_ui(&store_c20, &app_handle20);
    });

    app.run().unwrap();
}

fn load_credentials_to_ui(
    store: &Arc<Mutex<StoreData>>,
    app_handle: &slint::Weak<App>,
) {
    let s = store.lock().unwrap();
    let mut creds: Vec<Credential> = s.credentials.clone();
    creds.sort_by(|a, b| {
        b.is_favorite.cmp(&a.is_favorite)
            .then_with(|| b.login_count.cmp(&a.login_count))
            .then_with(|| b.last_login_at.cmp(&a.last_login_at))
    });

    let cred_items: Vec<CredentialItem> = creds.iter().map(|c| CredentialItem {
        id: SharedString::from(c.id.as_str()),
        connection_id: SharedString::from(c.connection_id.as_str()),
        display_name: SharedString::from(c.display_name.as_deref().unwrap_or(&c.connection_id)),
        client: SharedString::from(c.client.as_str()),
        username: SharedString::from(c.username.as_str()),
        language: SharedString::from(c.language.as_str()),
        system_id: SharedString::from(c.system_id.as_str()),
        environment: SharedString::from(c.environment.as_str()),
        color_tag: SharedString::from(c.color_tag.as_str()),
        is_favorite: c.is_favorite,
        login_count: c.login_count as i32,
        group_id: SharedString::from(c.group_id.as_deref().unwrap_or("")),
        snc_enabled: c.snc_enabled,
        description: SharedString::from(c.description.as_str()),
        is_selected: false,
    }).collect();

    let groups: Vec<GroupItem> = s.groups.iter().map(|g| {
        let count = s.credentials.iter().filter(|c| c.group_id.as_deref() == Some(&g.id)).count();
        GroupItem {
            id: SharedString::from(g.id.as_str()),
            name: SharedString::from(g.group_name.as_str()),
            count: count as i32,
            is_system: g.is_system,
            is_default: g.is_default,
        }
    }).collect();

    let all_count = s.credentials.len() as i32;
    let favorites_count = s.credentials.iter().filter(|c| c.is_favorite).count() as i32;

    drop(s);

    if let Some(a) = app_handle.upgrade() {
        let cred_model = std::rc::Rc::new(VecModel::from(cred_items));
        let group_model = std::rc::Rc::new(VecModel::from(groups));
        a.set_credentials(slint::ModelRc::from(cred_model));
        a.set_groups(slint::ModelRc::from(group_model));
        a.set_all_count(all_count);
        a.set_favorites_count(favorites_count);
        // Trigger filtered list update
        load_filtered_credentials(store, &a.as_weak());
    }
}

/// System group ID → environment string mapping
const SYSTEM_GROUP_ENVS: [(&str, &str); 4] = [
    ("group-production", "production"),
    ("group-test", "test"),
    ("group-development", "development"),
    ("group-configuration", "configuration"),
];

/// Filter credentials by active group + search text and set filtered-credentials on UI
fn load_filtered_credentials(
    store: &Arc<Mutex<StoreData>>,
    app_handle: &slint::Weak<App>,
) {
    let active_group = app_handle.upgrade()
        .map(|a| a.get_active_group().as_str().to_string())
        .unwrap_or_else(|| "all".to_string());
    let search_text = app_handle.upgrade()
        .map(|a| a.get_search_text().as_str().to_string())
        .unwrap_or_default();
    let selected_ids: Vec<String> = app_handle.upgrade()
        .map(|a| a.get_selected_creds().iter().map(|c| c.id.as_str().to_string()).collect())
        .unwrap_or_default();

    let s = store.lock().unwrap();
    let mut creds: Vec<Credential> = s.credentials.clone();

    // Filter by active group
    if active_group != "all" {
        if active_group == "favorites" {
            creds.retain(|c| c.is_favorite);
        } else if let Some(env) = SYSTEM_GROUP_ENVS.iter().find(|(gid, _)| *gid == active_group).map(|(_, e)| *e) {
            creds.retain(|c| c.environment == env);
        } else {
            creds.retain(|c| c.group_id.as_deref() == Some(&active_group));
        }
    }

    // Filter by search text
    if !search_text.is_empty() {
        let search_lower = search_text.to_lowercase();
        creds.retain(|c| {
            c.display_name.as_deref().unwrap_or("").to_lowercase().contains(&search_lower)
                || c.connection_id.to_lowercase().contains(&search_lower)
                || c.username.to_lowercase().contains(&search_lower)
                || c.system_id.to_lowercase().contains(&search_lower)
                || c.description.to_lowercase().contains(&search_lower)
        });
    }

    // Sort: favorites first, then by login count, then by last login
    creds.sort_by(|a, b| {
        b.is_favorite.cmp(&a.is_favorite)
            .then_with(|| b.login_count.cmp(&a.login_count))
            .then_with(|| b.last_login_at.cmp(&a.last_login_at))
    });

    let filtered_count = creds.len();

    // Resolve display name for the active group
    let group_display_name = if active_group == "all" {
        "全部".to_string()
    } else if active_group == "favorites" {
        "收藏".to_string()
    } else {
        s.groups.iter()
            .find(|g| g.id == active_group)
            .map(|g| g.group_name.clone())
            .unwrap_or_else(|| active_group.clone())
    };

    let filtered_items: Vec<CredentialItem> = creds.iter().map(|c| {
        let is_sel = selected_ids.iter().any(|id| id == &c.id);
        CredentialItem {
            id: SharedString::from(c.id.as_str()),
            connection_id: SharedString::from(c.connection_id.as_str()),
            display_name: SharedString::from(c.display_name.as_deref().unwrap_or(&c.connection_id)),
            client: SharedString::from(c.client.as_str()),
            username: SharedString::from(c.username.as_str()),
            language: SharedString::from(c.language.as_str()),
            system_id: SharedString::from(c.system_id.as_str()),
            environment: SharedString::from(c.environment.as_str()),
            color_tag: SharedString::from(c.color_tag.as_str()),
            is_favorite: c.is_favorite,
            login_count: c.login_count as i32,
            group_id: SharedString::from(c.group_id.as_deref().unwrap_or("")),
            snc_enabled: c.snc_enabled,
            description: SharedString::from(c.description.as_str()),
            is_selected: is_sel,
        }
    }).collect();

    drop(s);

    if let Some(a) = app_handle.upgrade() {
        let model = std::rc::Rc::new(VecModel::from(filtered_items));
        a.set_filtered_credentials(slint::ModelRc::from(model));
        a.set_current_group_count(filtered_count as i32);
        a.set_current_group_name(SharedString::from(group_display_name));
    }
}

impl Default for Credential {
    fn default() -> Self {
        Self {
            id: String::new(),
            connection_id: String::new(),
            client: String::new(),
            username: String::new(),
            password: String::new(),
            encrypted_password: String::new(),
            language: String::new(),
            connection_type: String::new(),
            system_id: String::new(),
            app_server: String::new(),
            system_number: String::new(),
            message_server: String::new(),
            message_server_port: String::new(),
            logon_group: String::new(),
            saprouter: String::new(),
            description: String::new(),
            post_login_action: None,
            post_login_action_type: None,
            group_id: None,
            environment: String::new(),
            login_count: 0,
            last_login_at: None,
            color_tag: String::new(),
            is_favorite: false,
            favorite_order: None,
            display_name: None,
            uuid: String::new(),
            snc_enabled: false,
            snc_name: String::new(),
            snc_qop: String::new(),
            snc_sso: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}
