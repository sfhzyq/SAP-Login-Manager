import { useState } from "react";
import type { AppSettings, Group } from "../types";
import { useI18n, LANGUAGE_OPTIONS } from "../i18n";
import type { Language } from "../i18n";

// 分组标题图标
const IconSecurity = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" /></svg>
);
const IconLogin = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4" /><polyline points="10 17 15 12 10 7" /><line x1="15" y1="12" x2="3" y2="12" /></svg>
);
const IconWindow = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><rect x="3" y="4" width="18" height="16" rx="2" /><line x1="3" y1="9" x2="21" y2="9" /></svg>
);
const IconLang = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="10" /><line x1="2" y1="12" x2="22" y2="12" /><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" /></svg>
);
const IconAbout = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="10" /><line x1="12" y1="16" x2="12" y2="12" /><line x1="12" y1="8" x2="12.01" y2="8" /></svg>
);
const IconList = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><line x1="8" y1="6" x2="21" y2="6" /><line x1="8" y1="12" x2="21" y2="12" /><line x1="8" y1="18" x2="21" y2="18" /><line x1="3" y1="6" x2="3.01" y2="6" /><line x1="3" y1="12" x2="3.01" y2="12" /><line x1="3" y1="18" x2="3.01" y2="18" /></svg>
);

// 通用开关行
function ToggleRow({ label, desc, checked, onChange }: { label: string; desc?: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="set-row clickable" style={{ cursor: "pointer" }}>
      <div className="set-row-main">
        <div className="set-row-label">{label}</div>
        {desc && <div className="set-row-desc">{desc}</div>}
      </div>
      <span className="switch">
        <input type="checkbox" checked={checked} onChange={e => onChange(e.target.checked)} />
        <span className="slider" />
      </span>
    </label>
  );
}

// 纯文本输入的横向数字行
const NUM_INPUT_STYLE = { paddingLeft: 12, textAlign: "center" as const };

export function SettingsModal({ settings, groups = [], onSave, onChangePassword, onClose }: { settings: AppSettings; groups?: Group[]; onSave: (s: AppSettings) => void; onChangePassword?: () => void; onClose: () => void }) {
  const { t } = useI18n();
  const [form, setForm] = useState<AppSettings>(settings);
  const set = (k: keyof AppSettings, v: string | boolean | number) => setForm(p => ({ ...p, [k]: v }));

  // 系统分组ID -> 翻译键（与 App.tsx 保持一致）
  const SYSTEM_GROUP_NAME_KEYS: Record<string, string> = {
    "group-production": "group.name.production",
    "group-test": "group.name.test",
    "group-development": "group.name.development",
    "group-configuration": "group.name.configuration",
  };
  const groupLabel = (g: Group): string => {
    if (g.is_default) return t("group.name.default");
    if (SYSTEM_GROUP_NAME_KEYS[g.id]) return t(SYSTEM_GROUP_NAME_KEYS[g.id] as any);
    return g.group_name;
  };
  // 下拉可选分组：系统分组在前，自定义分组在后（排除默认分组，"全部"已单列）
  const systemGroups = groups.filter(g => g.is_system && !g.is_default);
  const customGroups = groups.filter(g => !g.is_system && !g.is_default);

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={e => e.stopPropagation()} style={{ width: 440 }}>
        <div className="modal-header" style={{ textAlign: "center" }}>{t("settings.title")}</div>
        <div className="modal-body">
          {/* 安全 */}
          <div className="set-group">
            <div className="set-group-title"><IconSecurity />{t("settings.security")}</div>
            <div className="set-card">
              <ToggleRow label={t("settings.password_free")} desc={t("settings.password_free_hint")} checked={form.password_free} onChange={v => set("password_free", v)} />
              <div className="set-row">
                <div className="set-row-main">
                  <div className="set-row-label">{t("settings.auto_lock")}</div>
                  <div className="set-row-desc">{t("settings.auto_lock_hint")}</div>
                </div>
                <div className="set-row-control">
                  <input className="input set-input-sm" type="number" min={0} max={120} value={form.auto_lock_minutes} onChange={e => set("auto_lock_minutes", parseInt(e.target.value) || 0)} style={NUM_INPUT_STYLE} />
                </div>
              </div>
              {onChangePassword && (
                <div className="set-row clickable" onClick={onChangePassword}>
                  <div className="set-row-main">
                    <div className="set-row-label">{t("settings.change_password")}</div>
                    <div className="set-row-desc">{t("settings.change_password_hint")}</div>
                  </div>
                  <button type="button" className="set-link-btn">{t("common.edit")} ›</button>
                </div>
              )}
              <div className="set-row">
                <div className="set-row-main">
                  <div className="set-row-label">{t("settings.clipboard_clear")}</div>
                  <div className="set-row-desc">{t("settings.clipboard_clear_hint")}</div>
                </div>
                <div className="set-row-control">
                  <input className="input set-input-sm" type="number" min={0} max={300} value={form.clipboard_clear_seconds} onChange={e => set("clipboard_clear_seconds", Math.min(300, Math.max(0, parseInt(e.target.value) || 0)))} style={NUM_INPUT_STYLE} />
                </div>
              </div>
            </div>
          </div>

          {/* 登录 */}
          <div className="set-group">
            <div className="set-group-title"><IconLogin />{t("settings.login")}</div>
            <div className="set-card">
              <div className="set-row">
                <div className="set-row-main">
                  <div className="set-row-label">{t("settings.batch_interval")}</div>
                  <div className="set-row-desc">{t("settings.batch_interval_hint")}</div>
                </div>
                <div className="set-row-control">
                  <input className="input set-input-sm" type="number" min={1} max={10} value={form.batch_login_interval} onChange={e => set("batch_login_interval", Math.min(10, parseInt(e.target.value) || 2))} style={NUM_INPUT_STYLE} />
                </div>
              </div>
              <div className="set-row-stack">
                <div className="set-row-label">{t("settings.sap_logon_path")}<span style={{ fontWeight: 400, fontSize: 11, color: "var(--text-muted)", marginLeft: 4 }}>{t("settings.sap_logon_path_hint")}</span></div>
                <input className="input" type="text" value={form.sap_logon_path || ""} onChange={e => set("sap_logon_path", e.target.value)} placeholder={t("settings.sap_logon_path_placeholder")} style={{ paddingLeft: 12 }} />
              </div>
            </div>
          </div>

          {/* 列表 */}
          <div className="set-group">
            <div className="set-group-title"><IconList />{t("settings.list")}</div>
            <div className="set-card">
              <ToggleRow label={t("settings.group_by_environment")} desc={t("settings.group_by_environment_hint")} checked={form.group_by_environment} onChange={v => set("group_by_environment", v)} />
              <ToggleRow label={t("settings.compact_mode")} desc={t("settings.compact_mode_hint")} checked={form.compact_mode} onChange={v => set("compact_mode", v)} />
              <div className="set-row">
                <div className="set-row-main">
                  <div className="set-row-label">{t("settings.default_group")}</div>
                  <div className="set-row-desc">{t("settings.default_group_hint")}</div>
                </div>
                <div className="set-row-control">
                  <select className="input" style={{ width: 150, paddingLeft: 12 }} value={form.default_group || ""} onChange={e => set("default_group", e.target.value)}>
                    <option value="">{t("settings.default_group_all")}</option>
                    <option value="favorites">{t("settings.default_group_favorites")}</option>
                    {systemGroups.map(g => <option key={g.id} value={g.id}>{groupLabel(g)}</option>)}
                    {customGroups.map(g => <option key={g.id} value={g.id}>{groupLabel(g)}</option>)}
                  </select>
                </div>
              </div>
            </div>
          </div>

          {/* 窗口 */}
          <div className="set-group">
            <div className="set-group-title"><IconWindow />{t("settings.window")}</div>
            <div className="set-card">
              <ToggleRow label={t("settings.auto_start")} desc={t("settings.auto_start_hint")} checked={form.auto_start} onChange={v => set("auto_start", v)} />
              <ToggleRow label={t("settings.close_to_tray")} checked={form.close_to_tray} onChange={v => set("close_to_tray", v)} />
              <ToggleRow label={t("settings.minimize_to_tray")} checked={form.minimize_to_tray} onChange={v => set("minimize_to_tray", v)} />
            </div>
          </div>

          {/* 语言 */}
          <div className="set-group">
            <div className="set-group-title"><IconLang />{t("settings.language")}</div>
            <div className="set-card">
              <div className="set-row">
                <div className="set-row-main">
                  <div className="set-row-label">{t("settings.language")}</div>
                  <div className="set-row-desc">{t("settings.language_hint")}</div>
                </div>
                <div className="set-row-control">
                  <select className="input" style={{ width: 130, paddingLeft: 12 }} value={form.default_language || "zh"} onChange={e => set("default_language", e.target.value as Language)}>
                    {LANGUAGE_OPTIONS.map(o => <option key={o.value} value={o.value}>{o.label}</option>)}
                  </select>
                </div>
              </div>
            </div>
          </div>

          {/* 关于 */}
          <div className="set-group">
            <div className="set-group-title"><IconAbout />{t("settings.about")}</div>
            <div className="set-card">
              <div className="set-row">
                <div className="set-row-main">
                  <div className="set-row-label">{t("settings.feedback")}</div>
                  <div className="set-row-desc">{t("settings.feedback_hint")}</div>
                </div>
                <div className="set-row-control">
                  <a href="mailto:sfhzyq@qq.com" style={{ fontSize: 12, color: "var(--accent-color)", textDecoration: "none" }}>sfhzyq@qq.com</a>
                  <button type="button" className="input-action" title={t("common.copy")} onClick={() => navigator.clipboard?.writeText("sfhzyq@qq.com")}>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="9" y="9" width="13" height="13" rx="2" ry="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg>
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
        <div className="modal-footer" style={{ justifyContent: "flex-end" }}>
          <div className="modal-actions">
            <button className="btn btn-secondary" onClick={onClose}>{t("common.cancel")}</button>
            <button className="btn btn-primary" onClick={() => onSave(form)}>{t("common.save")}</button>
          </div>
        </div>
      </div>
    </div>
  );
}
