import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Group, Credential } from "../types";
import { useI18n } from "../i18n";

/** 窗口控制按钮（最小化、关闭），用于无边框窗口的锁定/设置界面 */
function WindowControls() {
  const { t } = useI18n();
  return (
    <div style={{ position: "fixed", top: 0, right: 0, display: "flex", zIndex: 9999 }}>
      <button className="btn btn-ghost btn-sm" style={{ padding: "8px", display: "flex", alignItems: "center", borderRadius: 0 }} onClick={async () => { try { await getCurrentWindow().minimize(); } catch {} }} title={t("tooltip.minimize")}>
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="5" y1="12" x2="19" y2="12" /></svg>
      </button>
      <button className="btn btn-ghost btn-sm" style={{ padding: "8px", display: "flex", alignItems: "center", borderRadius: 0 }} onClick={async () => { try { await getCurrentWindow().close(); } catch {} }} title={t("tooltip.close")}>
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
      </button>
    </div>
  );
}

export function Toast({ message, type, actionLabel, onAction, onClose, duration = 3000 }: { message: string; type: "success" | "error" | "info"; actionLabel?: string; onAction?: () => void; onClose: () => void; duration?: number }) {
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  useEffect(() => { const t = setTimeout(() => onCloseRef.current(), duration); return () => clearTimeout(t); }, [duration]);
  return (
    <div className={`toast toast-${type}`} style={actionLabel ? { display: "flex", alignItems: "center", justifyContent: "center", gap: 10, paddingLeft: 14, paddingRight: 8 } : undefined}>
      <span style={{ flex: 1, minWidth: 0 }}>{message}</span>
      {actionLabel && onAction && (
        <button
          type="button"
          className="toast-action"
          onClick={(e) => { e.stopPropagation(); onAction(); onCloseRef.current(); }}
        >
          {actionLabel}
        </button>
      )}
    </div>
  );
}

export function SetupPage({ onSetup }: { onSetup: (p: string) => void }) {
  const { t } = useI18n();
  const [pw, setPw] = useState(""); const [cf, setCf] = useState(""); const [err, setErr] = useState("");
  const submit = async () => {
    setErr("");
    if (pw.length < 4) return setErr(t("setup.password_min"));
    if (pw !== cf) return setErr(t("setup.password_mismatch"));
    try { await invoke("set_master_password", { password: pw }); onSetup(pw); } catch (e) { setErr(String(e)); }
  };
  return (
    <div data-tauri-drag-region style={{ display: "flex", alignItems: "center", justifyContent: "center", height: "100vh", background: "var(--bg-primary)" }}>
      <WindowControls />
      <div className="card" style={{ width: 420, textAlign: "center", padding: 36 }}>
        <div style={{ width: 56, height: 56, margin: "0 auto 20px", borderRadius: 16, background: "var(--accent)", display: "flex", alignItems: "center", justifyContent: "center", boxShadow: "var(--shadow-accent)" }}>
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="2"><rect x="3" y="11" width="18" height="11" rx="2" /><path d="M7 11V7a5 5 0 0 1 10 0v4" /></svg>
        </div>
        <h2 style={{ fontFamily: "var(--font-display)", marginBottom: 6, fontSize: 22, fontWeight: 700, letterSpacing: -0.5 }}>SAP Login Manager</h2>
        <p style={{ color: "var(--text-secondary)", marginBottom: 28, fontSize: 13, fontFamily: "var(--font-body)" }}>{t("setup.subtitle")}</p>
        <div className="form-group"><input className="input" type="password" placeholder={t("setup.password")} value={pw} onChange={e => setPw(e.target.value)} onKeyDown={e => e.key === "Enter" && submit()} style={{ textAlign: "center", padding: "10px 14px" }} /></div>
        <div className="form-group"><input className="input" type="password" placeholder={t("setup.confirm_password")} value={cf} onChange={e => setCf(e.target.value)} onKeyDown={e => e.key === "Enter" && submit()} style={{ textAlign: "center", padding: "10px 14px" }} /></div>
        {err && <p style={{ color: "var(--danger)", fontSize: 12, marginBottom: 12, fontFamily: "var(--font-body)", fontWeight: 600 }}>{err}</p>}
        <button className="btn btn-primary" style={{ width: "100%", padding: "11px", fontSize: 13 }} onClick={submit}>{t("setup.create")}</button>
      </div>
    </div>
  );
}

/** 独立分组管理面板：集中新增、重命名、删除、置顶自定义分组 */
export function GroupManagerModal({
  groups,
  counts,
  pinned,
  onCreate,
  onRename,
  onDelete,
  onTogglePin,
  onClose,
}: {
  groups: Group[];
  counts: Record<string, number>;
  pinned: Set<string>;
  onCreate: (name: string) => void;
  onRename: (group: Group, name: string) => void;
  onDelete: (group: Group) => void;
  onTogglePin: (groupId: string) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [newName, setNewName] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editName, setEditName] = useState("");

  const startEdit = (g: Group) => { setEditingId(g.id); setEditName(g.group_name); };
  const commitEdit = (g: Group) => {
    const n = editName.trim();
    if (n && n !== g.group_name) onRename(g, n);
    setEditingId(null);
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={e => e.stopPropagation()} style={{ width: 380, maxHeight: "82vh", display: "flex", flexDirection: "column" }}>
        <div className="modal-header" style={{ textAlign: "center" }}>{t("group.manage")}</div>
        <div className="modal-body" style={{ overflowY: "auto", flex: 1 }}>
          {/* 新建分组 */}
          <div style={{ display: "flex", gap: 6, marginBottom: 12 }}>
            <input
              className="input"
              placeholder={t("group.name_placeholder")}
              maxLength={40}
              value={newName}
              onChange={e => setNewName(e.target.value)}
              onKeyDown={e => { if (e.key === "Enter" && newName.trim()) { onCreate(newName.trim()); setNewName(""); } }}
              style={{ flex: 1 }}
            />
            <button className="btn btn-primary" disabled={!newName.trim()} onClick={() => { onCreate(newName.trim()); setNewName(""); }} style={{ whiteSpace: "nowrap", flexShrink: 0 }}>
              {t("group.create")}
            </button>
          </div>

          {groups.length === 0 ? (
            <div style={{ textAlign: "center", padding: "24px 0", color: "var(--text-muted)", fontSize: 12 }}>{t("group.empty")}</div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              {groups.map(g => {
                const isPinned = pinned.has(g.id);
                const isEditing = editingId === g.id;
                return (
                  <div key={g.id} style={{ display: "flex", alignItems: "center", gap: 6, padding: "7px 10px", background: "var(--bg-secondary)", border: "1px solid var(--border-color)", borderRadius: 8 }}>
                    {isEditing ? (
                      <input
                        className="input"
                        maxLength={40}
                        value={editName}
                        autoFocus
                        onChange={e => setEditName(e.target.value)}
                        onKeyDown={e => { if (e.key === "Enter") commitEdit(g); if (e.key === "Escape") setEditingId(null); }}
                        onBlur={() => commitEdit(g)}
                        style={{ flex: 1, padding: "4px 8px", fontSize: 13 }}
                      />
                    ) : (
                      <>
                        <span style={{ flex: 1, fontSize: 13, fontWeight: 600, color: "var(--text-primary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} onDoubleClick={() => startEdit(g)}>
                          {g.group_name}
                        </span>
                        <span style={{ fontSize: 11, color: "var(--text-muted)", fontFamily: "var(--font-mono)" }}>{counts[g.id] ?? g.entries.length}</span>
                        <button className="btn btn-ghost btn-sm" style={{ padding: 5 }} onClick={() => onTogglePin(g.id)} data-tooltip={isPinned ? t("card.unpin") : t("card.pin")}>
                          <svg width="14" height="14" viewBox="0 0 24 24" fill={isPinned ? "var(--accent)" : "none"} stroke={isPinned ? "var(--accent)" : "currentColor"} strokeWidth="2"><line x1="12" y1="17" x2="12" y2="22" /><path d="M5 17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1v4.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V17z" /></svg>
                        </button>
                        <button className="btn btn-ghost btn-sm" style={{ padding: 5 }} onClick={() => startEdit(g)} data-tooltip={t("card.rename")}>
                          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" /><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" /></svg>
                        </button>
                        <button className="btn btn-ghost btn-sm" style={{ padding: 5, color: "var(--danger)" }} onClick={() => onDelete(g)} data-tooltip={t("card.delete")}>
                          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="3 6 5 6 21 6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /></svg>
                        </button>
                      </>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
        <div className="modal-footer" style={{ justifyContent: "flex-end" }}>
          <div className="modal-actions">
            <button className="btn btn-secondary" onClick={onClose}>{t("common.close")}</button>
          </div>
        </div>
      </div>
    </div>
  );
}

export function UnlockPage({ onUnlock, quickUnlockPw }: { onUnlock: (p: string) => void; quickUnlockPw?: string | null }) {
  const { t } = useI18n();
  const [pw, setPw] = useState(""); const [err, setErr] = useState("");
  const submit = async () => {
    setErr("");
    try { const ok = await invoke<boolean>("verify_master_password", { password: pw }); if (ok) onUnlock(pw); else setErr(t("unlock.wrong_password")); } catch (e) { setErr(String(e)); }
  };
  const quickUnlock = async () => {
    if (!quickUnlockPw) return;
    setErr("");
    try { const ok = await invoke<boolean>("verify_master_password", { password: quickUnlockPw }); if (ok) onUnlock(quickUnlockPw); else setErr(t("unlock.wrong_password")); } catch (e) { setErr(String(e)); }
  };
  return (
    <div data-tauri-drag-region style={{ display: "flex", alignItems: "center", justifyContent: "center", height: "100vh", background: "var(--bg-primary)" }}>
      <WindowControls />
      <div className="card" style={{ width: 420, textAlign: "center", padding: 36 }}>
        <div style={{ width: 56, height: 56, margin: "0 auto 20px", borderRadius: 16, background: "var(--accent)", display: "flex", alignItems: "center", justifyContent: "center", boxShadow: "var(--shadow-accent)" }}>
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="2"><rect x="3" y="11" width="18" height="11" rx="2" /><path d="M7 11V7a5 5 0 0 1 10 0v4" /></svg>
        </div>
        <h2 style={{ fontFamily: "var(--font-display)", marginBottom: 6, fontSize: 22, fontWeight: 700, letterSpacing: -0.5 }}>SAP Login Manager</h2>
        <p style={{ color: "var(--text-secondary)", marginBottom: 28, fontSize: 13, fontFamily: "var(--font-body)" }}>{t("unlock.subtitle")}</p>
        <div className="form-group"><input className="input" type="password" placeholder={t("unlock.password")} value={pw} onChange={e => setPw(e.target.value)} onKeyDown={e => e.key === "Enter" && submit()} style={{ textAlign: "center", padding: "10px 14px" }} autoFocus /></div>
        {err && <p style={{ color: "var(--danger)", fontSize: 12, marginBottom: 12, fontFamily: "var(--font-body)", fontWeight: 600 }}>{err}</p>}
        <button className="btn btn-primary" style={{ width: "100%", padding: "11px", fontSize: 13 }} onClick={submit}>{t("unlock.unlock")}</button>
        {quickUnlockPw && <button className="btn btn-secondary" style={{ width: "100%", padding: "11px", fontSize: 13, marginTop: 10 }} onClick={quickUnlock}>{t("unlock.quick_unlock")}</button>}
      </div>
    </div>
  );
}

/**
 * 查看连接密码弹窗（最高安全等级）：
 * 阶段1 verify —— 必须二次输入解锁密码并通过 verify_master_password 校验
 * 阶段2 shown  —— 用校验通过的密码调用 get_decrypted_password 解密，默认掩码，眼睛切换明文
 *                  倒计时自动隐藏并关闭；关闭/卸载即清除内存中的明文
 */
export function RevealPasswordModal({ cred, clipboardClearSeconds = 20, onClose, onToast }: { cred: Credential; clipboardClearSeconds?: number; onClose: () => void; onToast?: (msg: string, type: "success" | "error" | "info") => void }) {
  const { t } = useI18n();
  const [stage, setStage] = useState<"verify" | "shown">("verify");
  const [pw, setPw] = useState("");
  const [err, setErr] = useState("");
  const [plain, setPlain] = useState("");
  const [visible, setVisible] = useState(false);
  const [countdown, setCountdown] = useState(0);
  const plainRef = useRef("");
  plainRef.current = plain;

  const AUTO_HIDE = 20; // 明文展示后自动关闭秒数

  // 关闭时彻底清除内存明文
  const close = () => { setPlain(""); plainRef.current = ""; setPw(""); onClose(); };
  useEffect(() => () => { plainRef.current = ""; }, []);

  const verify = async () => {
    if (!pw) return;
    setErr("");
    try {
      const ok = await invoke<boolean>("verify_master_password", { password: pw });
      if (!ok) { setErr(t("unlock.wrong_password")); return; }
      const plainPw = await invoke<string>("get_decrypted_password", { masterPassword: pw, credentialId: cred.id });
      setPlain(plainPw);
      setPw("");
      setStage("shown");
      setCountdown(AUTO_HIDE);
    } catch (e) { setErr(String(e)); }
  };

  // 展示阶段倒计时自动关闭
  useEffect(() => {
    if (stage !== "shown") return;
    if (countdown <= 0) { close(); return; }
    const timer = setTimeout(() => setCountdown(c => c - 1), 1000);
    return () => clearTimeout(timer);
  }, [stage, countdown]);

  const copyPlain = async () => {
    try {
      await navigator.clipboard.writeText(plainRef.current);
      if (clipboardClearSeconds > 0) setTimeout(() => { navigator.clipboard.writeText("").catch(() => {}); }, clipboardClearSeconds * 1000);
      onToast?.(t("reveal.copied"), "success");
    } catch { onToast?.(t("reveal.copy_failed"), "error"); }
  };

  return (
    <div className="modal-overlay" onClick={close}>
      <div className="modal" onClick={e => e.stopPropagation()} style={{ width: 400 }}>
        <div className="modal-header" style={{ textAlign: "center" }}>{t("reveal.title")}</div>
        <div className="modal-body">
          <p style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 14, lineHeight: 1.6 }}>
            {cred.display_name || cred.connection_id || cred.username}
          </p>
          {stage === "verify" ? (
            <>
              <p style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 12 }}>{t("reveal.verify_hint")}</p>
              <div className="form-group">
                <input className="input" type="password" placeholder={t("unlock.password")} value={pw} onChange={e => setPw(e.target.value)} onKeyDown={e => e.key === "Enter" && verify()} autoFocus />
              </div>
              {err && <p style={{ color: "var(--danger)", fontSize: 12, marginBottom: 4, fontWeight: 600 }}>{err}</p>}
            </>
          ) : (
            <>
              <div className="form-group" style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <input className="input" readOnly type={visible ? "text" : "password"} value={plain} style={{ flex: 1, fontFamily: "var(--font-mono, monospace)", letterSpacing: 1 }} onFocus={e => e.target.select()} />
                <button className="btn btn-secondary btn-sm" onClick={() => setVisible(v => !v)} title={visible ? t("reveal.hide") : t("reveal.show")}>
                  {visible ? t("reveal.hide") : t("reveal.show")}
                </button>
              </div>
              <p style={{ color: "var(--text-tertiary, var(--text-secondary))", fontSize: 11, marginTop: 4 }}>{t("reveal.countdown").replace("{s}", String(countdown))}</p>
            </>
          )}
        </div>
        <div className="modal-footer" style={{ justifyContent: "flex-end" }}>
          <div className="modal-actions">
            <button className="btn btn-secondary" onClick={close}>{t("common.cancel")}</button>
            {stage === "verify"
              ? <button className="btn btn-primary" disabled={!pw} onClick={verify}>{t("reveal.confirm")}</button>
              : <button className="btn btn-primary" onClick={copyPlain}>{t("card.copy_password")}</button>}
          </div>
        </div>
      </div>
    </div>
  );
}

export function CreateGroupModal({ onCreate, onClose }: { onCreate: (name: string) => void; onClose: () => void }) {
  const { t } = useI18n();
  const [name, setName] = useState("");
  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={e => e.stopPropagation()} style={{ width: 360 }}>
        <div className="modal-header" style={{ textAlign: "center" }}>{t("group.create")}</div>
        <div className="modal-body">
          <div className="form-group">
            <label className="label">{t("group.name_label")}</label>
            <input className="input" placeholder={t("group.name_placeholder")} maxLength={40} value={name} onChange={e => setName(e.target.value)} autoFocus onKeyDown={e => e.key === "Enter" && name.trim() && onCreate(name.trim())} />
          </div>
        </div>
        <div className="modal-footer" style={{ justifyContent: "flex-end" }}>
          <div className="modal-actions">
            <button className="btn btn-secondary" onClick={onClose}>{t("common.cancel")}</button>
            <button className="btn btn-primary" disabled={!name.trim()} onClick={() => onCreate(name.trim())}>{t("setup.create")}</button>
          </div>
        </div>
      </div>
    </div>
  );
}

export function RenameGroupModal({ group, onRename, onClose }: { group: Group; onRename: (name: string) => void; onClose: () => void }) {
  const { t } = useI18n();
  const [name, setName] = useState(group.group_name);
  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={e => e.stopPropagation()} style={{ width: 360 }}>
        <div className="modal-header" style={{ textAlign: "center" }}>{t("group.rename")}</div>
        <div className="modal-body">
          <div className="form-group">
            <label className="label">{t("group.name_label")}</label>
            <input className="input" maxLength={40} value={name} onChange={e => setName(e.target.value)} autoFocus onKeyDown={e => e.key === "Enter" && name.trim() && name.trim() !== group.group_name && onRename(name.trim())} />
          </div>
        </div>
        <div className="modal-footer" style={{ justifyContent: "flex-end" }}>
          <div className="modal-actions">
            <button className="btn btn-secondary" onClick={onClose}>{t("common.cancel")}</button>
            <button className="btn btn-primary" disabled={!name.trim() || name.trim() === group.group_name} onClick={() => onRename(name.trim())}>{t("common.save")}</button>
          </div>
        </div>
      </div>
    </div>
  );
}
