import { useState } from "react";
import type { Credential, Group, SapConnection } from "../types";
import { useI18n } from "../i18n";

// 凭据编辑弹窗
export function CredentialModal({ credential, groups, sapConnections, clipboardClearSeconds = 20, onSave, onDuplicate, onClose }: {
  credential?: Credential | Partial<Credential>; groups: Group[]; sapConnections?: SapConnection[]; clipboardClearSeconds?: number; onSave: (c: Partial<Credential>) => void; onDuplicate?: (c: Partial<Credential>) => void; onClose: () => void;
}) {
  const { t } = useI18n();
  const [form, setForm] = useState<Partial<Credential>>(credential || { connection_id: "", connection_type: "direct", app_server: "", system_number: "", system_id: "", message_server: "", logon_group: "PUBLIC", saprouter: "", description: "", client: "", username: "", password: "", language: "ZH", environment: "", group_id: "" });
  const set = (k: keyof Credential, v: string | boolean | number | null) => setForm(p => ({ ...p, [k]: v }));
  const customGroups = groups.filter(g => !g.is_system && !g.is_default);

  // 复制字段值（敏感信息按设置自动清空剪贴板）
  const copyField = (text: string) => {
    if (!text) return;
    try { navigator.clipboard?.writeText(text).catch(() => {}); } catch {}
    if (clipboardClearSeconds > 0) setTimeout(() => { try { navigator.clipboard?.writeText("").catch(() => {}); } catch {} }, clipboardClearSeconds * 1000);
  };

  const [activeTab, setActiveTab] = useState<"conn" | "cred" | "snc">("conn");
  const [showPassword, setShowPassword] = useState(false);
  const [validationErrors, setValidationErrors] = useState<Record<string, string>>({});

  const sncQopOptions = [
    { v: "9", l: `${t("snc.qop.9")} (9)` },
    { v: "8", l: `${t("snc.qop.8")} (8)` },
    { v: "3", l: `${t("snc.qop.3")} (3)` },
    { v: "2", l: `${t("snc.qop.2")} (2)` },
    { v: "1", l: `${t("snc.qop.1")} (1)` },
  ];

  const generateDisplayName = () => {
    const sid = form.system_id || "";
    const envMap: Record<string, string> = { production: t("env.production"), development: t("env.development"), test: t("env.test"), configuration: t("env.configuration"), "": t("env.other") };
    const env = envMap[form.environment || ""] || t("env.other");
    const connId = form.connection_id || "";
    const template = `${sid}-${env}-${connId}`.replace(/^-+|-+$/g, "").replace(/-+/g, "-");
    set("display_name", template);
  };

  // 选择 SAP 配置中的连接：connection_id 取连接名（-sysname），并带入可解析的服务器信息
  const applySapConnection = (name: string) => {
    if (!name) { set("connection_id", ""); return; }
    const conn = (sapConnections || []).find(c => c.name === name);
    setForm(p => {
      const next: Partial<Credential> = { ...p, connection_id: name };
      if (conn) {
        if (conn.system_id) next.system_id = conn.system_id;
        if (conn.connection_type) next.connection_type = conn.connection_type;
        if (conn.connection_type === "load_balancing") {
          if (conn.message_server) next.message_server = conn.message_server;
          if (conn.logon_group || conn.group) next.logon_group = conn.logon_group || conn.group || "";
        } else {
          if (conn.server) next.app_server = conn.server;
          if (conn.system_number) next.system_number = conn.system_number;
        }
        if (conn.saprouter) next.saprouter = conn.saprouter;
        if (conn.description && !p.description) next.description = conn.description;
      }
      return next;
    });
  };
  const sapConnNames = (sapConnections || []).map(c => c.name).filter(Boolean);

  const envOptions = [{v: "configuration", l: t("env.configuration"), c: "#8a3ffc"}, {v: "development", l: t("env.development"), c: "#198038"}, {v: "test", l: t("env.test"), c: "#f1c21b"}, {v: "production", l: t("env.production"), c: "#da1e28"}, {v: "", l: t("env.other"), c: "#6b7280"}];
  const actionOptions = [{v: "", l: t("credential.action_none")}, {v: "transaction", l: t("credential.action_transaction")}, {v: "report", l: t("credential.action_report")}, {v: "command", l: t("credential.action_command")}];

  const copyIcon = <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="9" y="9" width="13" height="13" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg>;
  const eyeIcon = <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" /><circle cx="12" cy="12" r="3" /></svg>;
  const eyeOffIcon = <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24" /><line x1="1" y1="1" x2="23" y2="23" /></svg>;

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={e => e.stopPropagation()}>
        <div className="modal-header" style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
          <span>{credential && (credential as Credential).id ? t("credential.title_edit") : t("credential.title_add")}</span>
          {credential && (credential as Credential).id && onDuplicate && (
            <button type="button" className="btn btn-secondary btn-sm" style={{ fontSize: 11, padding: "3px 8px" }} onClick={() => onDuplicate(form)} data-tooltip={t("credential.duplicate_tooltip")}>
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ marginRight: 4, verticalAlign: "middle" }}><rect x="9" y="9" width="13" height="13" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg>
              {t("common.duplicate")}
            </button>
          )}
        </div>
        <div className="modal-tabs">
          <button className={`modal-tab${activeTab === "conn" ? " active" : ""}`} onClick={() => setActiveTab("conn")}>{t("credential.tab_connection")}</button>
          <button className={`modal-tab${activeTab === "cred" ? " active" : ""}`} onClick={() => setActiveTab("cred")}>{t("credential.tab_credentials")}</button>
          <button className={`modal-tab${activeTab === "snc" ? " active" : ""}`} onClick={() => setActiveTab("snc")}>{t("credential.tab_snc")}</button>
        </div>
        <div className="modal-body">
          {activeTab === "conn" && (<div className="tab-panel" key="conn">
            <div className="form-group">
              <label className="label">{t("credential.display_name")}</label>
              <div style={{display:"flex",gap:8}}>
                <input className="input" placeholder={t("credential.display_name_placeholder")} value={form.display_name || ""} onChange={e => set("display_name", e.target.value)} />
                <button type="button" className="btn btn-secondary btn-sm" style={{whiteSpace:"nowrap"}} onClick={generateDisplayName} title={t("credential.generate_name")}>⚙ {t("credential.generate")}</button>
              </div>
            </div>
            <div className="form-group">
              <div style={{ display: "flex", gap: 8 }}>
                <div style={{ flex: 3, minWidth: 0 }}>
                  <label className="label">{t("credential.system_id")}<span style={{ color: "var(--danger)" }}>*</span></label>
                  <input className="input" placeholder={t("credential.system_id_placeholder")} value={form.system_id || ""} onChange={e => { const val = e.target.value.replace(/[^a-zA-Z0-9]/g, "").toUpperCase(); set("system_id", val); if (validationErrors.system_id) setValidationErrors(p => ({ ...p, system_id: "" })); }} style={validationErrors.system_id ? { borderColor: "var(--danger)" } : undefined} />
                  <span style={{ fontSize: 11, color: "var(--danger)", marginTop: 2, display: "block" }}>{validationErrors.system_id}</span>
                </div>
                <div style={{ flex: 7, minWidth: 0 }}>
                  <label className="label">{t("credential.connection_id")} <span style={{ fontSize: 10, color: "var(--text-muted)" }}>{t("credential.connection_id_desc")}</span></label>
                  <select className="input" value={form.connection_id || ""} onChange={e => applySapConnection(e.target.value)}>
                    {sapConnNames.map(n => <option key={n} value={n}>{n}</option>)}
                  </select>
                </div>
              </div>
            </div>
            <div className="form-group">
              <label className="label">{t("credential.environment")}{t("credential.env_suffix")}</label>
              <div className="seg-control">
                {envOptions.map(opt => (
                  <button key={opt.v} type="button" className={`seg-btn${form.environment === opt.v ? " active" : ""}`} onClick={() => { set("environment", opt.v); if (opt.v) set("group_id", ""); }}>{opt.l}</button>
                ))}
              </div>
            </div>
            <div className="form-group">
              <label className="label">{t("credential.connection_type")}</label>
              <div className="seg-control">
                <button type="button" className={`seg-btn${(form.connection_type || "direct") === "direct" ? " active" : ""}`} onClick={() => set("connection_type", "direct")}>{t("credential.connection_type_direct")}</button>
                <button type="button" className={`seg-btn${form.connection_type === "load_balancing" ? " active" : ""}`} onClick={() => set("connection_type", "load_balancing")}>{t("credential.connection_type_load_balance")}</button>
              </div>
            </div>
            {(form.connection_type || "direct") === "direct" ? (<>
              <div className="form-group">
                <label className="label">{t("credential.app_server")} <span style={{color:"var(--danger)"}}>*</span></label>
                <div className="input-wrapper">
                  <input className="input" placeholder={t("credential.app_server_placeholder")} value={form.app_server || ""} onChange={e => { set("app_server", e.target.value); if (validationErrors.app_server) setValidationErrors(p => ({ ...p, app_server: "" })); }} style={validationErrors.app_server ? { borderColor: "var(--danger)" } : undefined} />
                  {form.app_server && <button type="button" className="input-action" title={t("credential.copy")} onClick={() => copyField(form.app_server || "")}>{copyIcon}</button>}
                </div>
                <span style={{ fontSize: 11, color: "var(--danger)", marginTop: 2 }}>{validationErrors.app_server}</span>
              </div>
              <div className="form-group"><label className="label">{t("credential.system_number")} <span style={{color:"var(--danger)"}}>*</span></label><input className="input" placeholder={t("credential.system_number_placeholder")} value={form.system_number || ""} onChange={e => { set("system_number", e.target.value); if (validationErrors.system_number) setValidationErrors(p => ({ ...p, system_number: "" })); }} style={validationErrors.system_number ? { borderColor: "var(--danger)" } : undefined} /><span style={{ fontSize: 11, color: "var(--danger)", marginTop: 2 }}>{validationErrors.system_number}</span></div>
            </>) : (<>
              <div className="form-group">
                <label className="label">{t("credential.message_server")} <span style={{color:"var(--danger)"}}>*</span></label>
                <div className="input-wrapper">
                  <input className="input" placeholder={t("credential.app_server_placeholder")} value={form.message_server || ""} onChange={e => { set("message_server", e.target.value); if (validationErrors.message_server) setValidationErrors(p => ({ ...p, message_server: "" })); }} style={validationErrors.message_server ? { borderColor: "var(--danger)" } : undefined} />
                  {form.message_server && <button type="button" className="input-action" title={t("credential.copy")} onClick={() => copyField(form.message_server || "")}>{copyIcon}</button>}
                </div>
                <span style={{ fontSize: 11, color: "var(--danger)", marginTop: 2 }}>{validationErrors.message_server}</span>
              </div>
              <div className="form-group"><label className="label">{t("credential.logon_group")} <span style={{color:"var(--danger)"}}>*</span></label><input className="input" placeholder="PUBLIC" value={form.logon_group || ""} onChange={e => { set("logon_group", e.target.value); if (validationErrors.logon_group) setValidationErrors(p => ({ ...p, logon_group: "" })); }} style={validationErrors.logon_group ? { borderColor: "var(--danger)" } : undefined} /><span style={{ fontSize: 11, color: "var(--danger)", marginTop: 2 }}>{validationErrors.logon_group}</span></div>
            </>)}
            <div className="form-group">
              <label className="label">{t("credential.saprouter")}</label>
              <div className="input-wrapper">
                <input className="input" placeholder={t("credential.saprouter_placeholder")} value={form.saprouter || ""} onChange={e => set("saprouter", e.target.value)} />
                {form.saprouter && <button type="button" className="input-action" title={t("credential.copy")} onClick={() => copyField(form.saprouter || "")}>{copyIcon}</button>}
              </div>
            </div>
            <div className="form-group"><label className="label">{t("credential.description")}</label><input className="input" placeholder={t("credential.description_placeholder")} value={form.description || ""} onChange={e => set("description", e.target.value)} /></div>
          </div>)}

          {activeTab === "cred" && (<div className="tab-panel" key="cred">
            <div className="form-group"><label className="label">{t("credential.client")}<span style={{ color: "var(--danger)" }}>*</span></label><input className="input" placeholder={t("credential.client_placeholder")} value={form.client} onChange={e => { set("client", e.target.value); if (validationErrors.client) setValidationErrors(p => ({ ...p, client: "" })); }} style={validationErrors.client ? { borderColor: "var(--danger)" } : undefined} /><span style={{ fontSize: 11, color: "var(--danger)", marginTop: 2 }}>{validationErrors.client}</span></div>
            <div className="form-group">
              <label className="label">{t("credential.username")}</label>
              <div className="input-wrapper">
                <input className="input" placeholder={t("credential.username_placeholder")} value={form.username || ""} onChange={e => { set("username", e.target.value); if (validationErrors.username) setValidationErrors(p => ({ ...p, username: "" })); }} style={validationErrors.username ? { borderColor: "var(--danger)" } : undefined} />
                {form.username && <button type="button" className="input-action" title={t("credential.copy")} onClick={() => copyField(form.username || "")}>{copyIcon}</button>}
              </div>
              {validationErrors.username && <span style={{ fontSize: 11, color: "var(--danger)", marginTop: 2 }}>{validationErrors.username}</span>}
            </div>
            <div className="form-group">
              <label className="label">{t("credential.password")}</label>
              <div className="input-wrapper">
                <input className="input" type={showPassword ? "text" : "password"} placeholder={credential ? t("credential.password_keep") : t("credential.password_placeholder")} value={form.password || ""} onChange={e => set("password", e.target.value)} />
                <button type="button" className="input-action" style={{right: form.password ? 34 : 6}} title={showPassword ? t("credential.hide_password") : t("credential.show_password")} onClick={() => setShowPassword(!showPassword)}>{showPassword ? eyeOffIcon : eyeIcon}</button>
                {form.password && <button type="button" className="input-action" title={t("credential.copy")} onClick={() => copyField(form.password || "")}>{copyIcon}</button>}
              </div>
            </div>
            <div className="form-group">
              <label className="label">{t("credential.language")} <span style={{color:"var(--danger)"}}>*</span></label>
              <select className="input" value={form.language} onChange={e => set("language", e.target.value)}>
                <option value="ZH">{t("credential.lang_zh")}</option><option value="EN">{t("credential.lang_en")}</option><option value="JA">{t("credential.lang_ja")}</option>
              </select>
            </div>
            <div className="form-group">
              <label className="label">{t("credential.post_login_action")}</label>
              <div className="seg-control" style={{marginBottom:8}}>
                {actionOptions.map(opt => (
                  <button key={opt.v} type="button" className={`seg-btn${(form.post_login_action_type || "") === opt.v ? " active" : ""}`} onClick={() => { set("post_login_action_type", opt.v); if (!opt.v) set("post_login_action", ""); }}>{opt.l}</button>
                ))}
              </div>
              {form.post_login_action_type && (
                <input className="input" placeholder={form.post_login_action_type === "transaction" ? t("credential.action_placeholder_transaction") : form.post_login_action_type === "report" ? t("credential.action_placeholder_report") : t("credential.action_placeholder_command")} value={form.post_login_action || ""} onChange={e => set("post_login_action", e.target.value)} />
              )}
            </div>
          </div>)}

          {activeTab === "snc" && (<div className="tab-panel" key="snc">
            <div className="form-group">
              <label className="label" style={{ display: "flex", alignItems: "center", gap: 6, cursor: "pointer" }}>
                <label className="check-wrap"><input type="checkbox" checked={!!form.snc_enabled} onChange={e => set("snc_enabled", e.target.checked)} /><span className="check-box" /></label>
                {t("credential.snc_enabled")}{t("credential.snc_security_note")}
              </label>
            </div>

            {form.snc_enabled && (<>
              <div className="form-group">
                <label className="label">{t("credential.snc_name")} <span style={{ color: "var(--danger)", fontSize: 10 }}>*{t("credential.required")}</span></label>
                <div className="input-wrapper">
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--text-muted)" strokeWidth="2" style={{ position: "absolute", left: 10, top: "50%", transform: "translateY(-50%)" }}><rect x="3" y="11" width="18" height="11" rx="2" /><path d="M7 11V7a5 5 0 0 1 10 0v4" /></svg>
                  <input className="input" placeholder={t("credential.snc_name_placeholder")} value={form.snc_name || ""} onChange={e => set("snc_name", e.target.value)} style={{ paddingLeft: 32 }} />
                </div>
                {validationErrors.snc_name && <span style={{ color: "var(--danger)", fontSize: 10 }}>{validationErrors.snc_name}</span>}
              </div>

              <div className="form-group">
                <label className="label">{t("credential.snc_account")}
                  <span style={{ fontSize: 10, color: "var(--text-muted)", fontWeight: 400 }}>{t("credential.snc_account_hint")}</span>
                </label>
                <div className="input-wrapper">
                  <input className="input" value={form.username || ""} readOnly disabled placeholder={t("credential.snc_account_empty")} style={{ opacity: 0.85, cursor: "not-allowed" }} />
                </div>
                {form.snc_enabled && !form.username?.trim() && <span style={{ color: "var(--danger)", fontSize: 10 }}>{t("credential.snc_account_need_username")}</span>}
              </div>

              <div className="form-group">
                <label className="label">{t("credential.snc_qop")}{t("credential.qop_suffix")}</label>
                <div className="seg-control" style={{ marginBottom: 0 }}>
                  {sncQopOptions.map(opt => (
                    <button key={opt.v} type="button" className={`seg-btn${(form.snc_qop || "9") === opt.v ? " active" : ""}`} onClick={() => set("snc_qop", opt.v)}>{opt.l}</button>
                  ))}
                </div>
              </div>

              <div className="form-group">
                <label className="label" style={{ display: "flex", alignItems: "center", gap: 6, cursor: "pointer" }}>
                  <label className="check-wrap"><input type="checkbox" checked={!!form.snc_sso} onChange={e => set("snc_sso", e.target.checked)} /><span className="check-box" /></label>
                  {t("credential.snc_sso")}
                </label>
                <span style={{ fontSize: 10, color: "var(--text-muted)" }}>
                  {form.snc_sso ? t("credential.snc_sso_on") : t("credential.snc_sso_off")}
                </span>
              </div>
            </>)}

            {!form.snc_enabled && (
              <p style={{ color: "var(--text-muted)", textAlign: "center", padding: 20, fontSize: 12 }}>
                {t("credential.snc_hint")}
              </p>
            )}
          </div>)}

        </div>
        <div className="modal-footer">
          <div className="modal-footer-info">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" /></svg>
            <select className="input" style={{ width: "auto", minWidth: 100, padding: "2px 20px 2px 6px", fontSize: 11, height: 24 }} value={form.group_id || ""} onChange={e => set("group_id", e.target.value)}>
              <option value="">{t("credential.default_group")}</option>
              {customGroups.map(g => <option key={g.id} value={g.id}>{g.group_name}</option>)}
            </select>
          </div>
          <div className="modal-actions">
            <button className="btn btn-secondary" onClick={onClose}>{t("common.cancel")}</button>
            <button className="btn btn-primary" onClick={() => {
              const errors: Record<string, string> = {};
              if (!form.system_id?.trim()) errors.system_id = t("validation.system_id");
              if (!form.client?.trim()) errors.client = t("validation.client");
              if (!form.language?.trim()) errors.language = t("validation.language");
              if ((form.connection_type || "direct") === "direct") {
                if (!form.app_server?.trim()) errors.app_server = t("validation.app_server");
                if (!form.system_number?.trim()) errors.system_number = t("validation.system_number");
              } else {
                if (!form.message_server?.trim()) errors.message_server = t("validation.message_server");
                if (!form.logon_group?.trim()) errors.logon_group = t("validation.logon_group");
              }
              if (form.snc_enabled && !form.snc_name?.trim()) errors.snc_name = t("validation.snc_name");
              if (form.snc_enabled && !form.username?.trim()) errors.username = t("validation.snc_account");
              setValidationErrors(errors);
              if (Object.keys(errors).length > 0) {
                // 自动跳转到第一个有错误的页签
                if (errors.snc_name) {
                  setActiveTab("snc");
                } else if (errors.client || errors.language || errors.username) {
                  setActiveTab("cred");
                } else {
                  setActiveTab("conn");
                }
                return;
              }
              onSave(form);
            }}>{t("common.save")}</button>
          </div>
        </div>
      </div>
    </div>
  );
}
