import { useState, useMemo, useEffect } from "react";
import type { SapConnection, Credential } from "../types";
import { useI18n } from "../i18n";

export function SapImportModal({ connections, existingCredentials, onImport, onClose }: {
  connections: SapConnection[];
  existingCredentials: Credential[];
  onImport: (selected: SapConnection[]) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  // 判断连接是否已存在（按 实例编号+应用服务器+系统ID+router 匹配）
  const dupKeys = useMemo(() => {
    const keys = new Set<string>();
    for (const c of existingCredentials) {
      // 用 system_id + app_server + system_number + saprouter 组合作为唯一键
      const key = [c.system_id, c.app_server, c.system_number, c.saprouter].join("|").toLowerCase();
      keys.add(key);
    }
    return keys;
  }, [existingCredentials]);

  const isDuplicate = (conn: SapConnection): boolean => {
    const key = [conn.system_id || "", conn.server || "", conn.system_number || "", conn.saprouter || ""].join("|").toLowerCase();
    return dupKeys.has(key);
  };

  // 可导入的连接（非重复）
  const importableConns = connections.filter(c => !isDuplicate(c));
  const dupCount = connections.length - importableConns.length;

  // 默认勾选所有可导入的（connections 变化时重新初始化）
  const [checked, setChecked] = useState<Set<number>>(() => new Set(importableConns.map((_, i) => i)));
  useEffect(() => {
    setChecked(new Set(importableConns.map((_, i) => i)));
  }, [connections]);

  const toggle = (idx: number) => setChecked(prev => { const n = new Set(prev); n.has(idx) ? n.delete(idx) : n.add(idx); return n; });
  const toggleAll = () => {
    if (checked.size === importableConns.length) setChecked(new Set());
    else setChecked(new Set(importableConns.map((_, i) => i)));
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={e => e.stopPropagation()} style={{ width: 760 }}>
        <div className="modal-header" style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
          <span>{t("import.title")}</span>
          <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
            {t("import.summary").replace("{total}", String(connections.length)).replace("{importable}", String(importableConns.length)).replace("{dup}", String(dupCount))}
          </span>
        </div>
        {connections.length === 0 ? (
          <p style={{ color: "var(--text-muted)", textAlign: "center", padding: 40 }}>{t("import.no_config")}</p>
        ) : (
          <>
            <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 20px", borderBottom: "1px solid var(--border-color)" }}>
              <label className="check-wrap"><input type="checkbox" checked={checked.size === importableConns.length && importableConns.length > 0} onChange={toggleAll} /><span className="check-box" /></label>
              <span style={{ fontSize: 11, fontWeight: 600, color: "var(--text-secondary)" }}>{t("import.select_importable")}</span>
            </div>
            <div style={{ maxHeight: 400, overflow: "auto" }}>
              <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 11 }}>
                <thead style={{ position: "sticky", top: 0, background: "var(--bg-secondary)", zIndex: 1 }}>
                  <tr style={{ borderBottom: "1px solid var(--border-color)" }}>
                    <th style={{ padding: "6px 8px", textAlign: "left", width: 28 }}></th>
                    <th style={{ padding: "6px 8px", textAlign: "left", fontWeight: 600, color: "var(--text-secondary)" }}>{t("import.workspace")}</th>
                    <th style={{ padding: "6px 8px", textAlign: "left", fontWeight: 600, color: "var(--text-secondary)" }}>{t("import.name")}</th>
                    <th style={{ padding: "6px 8px", textAlign: "left", fontWeight: 600, color: "var(--text-secondary)" }}>SID</th>
                    <th style={{ padding: "6px 8px", textAlign: "left", fontWeight: 600, color: "var(--text-secondary)" }}>{t("credential.app_server")}</th>
                    <th style={{ padding: "6px 8px", textAlign: "left", fontWeight: 600, color: "var(--text-secondary)" }}>{t("import.instance")}</th>
                    <th style={{ padding: "6px 8px", textAlign: "left", fontWeight: 600, color: "var(--text-secondary)" }}>Router</th>
                    <th style={{ padding: "6px 8px", textAlign: "left", fontWeight: 600, color: "var(--text-secondary)" }}>{t("import.type")}</th>
                    <th style={{ padding: "6px 8px", textAlign: "left", fontWeight: 600, color: "var(--text-secondary)" }}>{t("import.status")}</th>
                  </tr>
                </thead>
                <tbody>
                  {connections.map((c) => {
                    const dup = isDuplicate(c);
                    const key = `${c.workspace_name || t("import.ungrouped")}::${c.name}`;
                    const isLoadBal = c.connection_type === "load_balancing";
                    const importIdx = importableConns.indexOf(c);
                    const isChecked = checked.has(importIdx);
                    return (
                      <tr
                        key={key}
                        style={{
                          borderBottom: "1px solid var(--border-color)",
                          cursor: dup ? "not-allowed" : "pointer",
                          background: isChecked ? "var(--accent-light)" : dup ? "var(--bg-tertiary)" : "transparent",
                          opacity: dup ? 0.6 : 1,
                        }}
                        onClick={() => !dup && toggle(importIdx)}
                      >
                        <td style={{ padding: "5px 8px" }} onClick={e => e.stopPropagation()}>
                          <label className="check-wrap"><input type="checkbox" checked={isChecked} disabled={dup} onChange={() => toggle(importIdx)} /><span className="check-box" /></label>
                        </td>
                        <td style={{ padding: "5px 8px", color: "var(--text-secondary)", whiteSpace: "nowrap" }}>{c.workspace_name || t("import.ungrouped")}</td>
                        <td style={{ padding: "5px 8px", fontWeight: 600, whiteSpace: "nowrap" }}>{c.name}</td>
                        <td style={{ padding: "5px 8px" }}>{c.system_id && <span style={{ fontSize: 9, background: "var(--accent)", color: "white", padding: "1px 5px", borderRadius: 3 }}>{c.system_id}</span>}</td>
                        <td style={{ padding: "5px 8px", color: "var(--text-secondary)", fontSize: 10 }}>{c.server || c.message_server || "-"}</td>
                        <td style={{ padding: "5px 8px", color: "var(--text-secondary)", fontSize: 10 }}>{c.system_number || "-"}</td>
                        <td style={{ padding: "5px 8px", color: "var(--text-secondary)", fontSize: 10, maxWidth: 120, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{c.saprouter || "-"}</td>
                        <td style={{ padding: "5px 8px" }}>{isLoadBal ? <span style={{ fontSize: 9, color: "#8a3ffc" }}>{t("credential.connection_type_load_balance")}</span> : <span style={{ fontSize: 9, color: "var(--text-secondary)" }}>{t("credential.connection_type_direct")}</span>}</td>
                        <td style={{ padding: "5px 8px" }}>
                          {dup
                            ? <span style={{ fontSize: 9, color: "var(--danger)", fontWeight: 600 }}>{t("import.duplicate")}</span>
                            : <span style={{ fontSize: 9, color: "var(--success)" }}>{t("import.importable")}</span>}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </>
        )}
        <div className="modal-footer">
          <div className="modal-footer-info">
            <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
              {t("import.selected_info").replace("{selected}", String(checked.size)).replace("{groups}", String(new Set(importableConns.filter((_, i) => checked.has(i)).map(c => c.workspace_name).filter(w => w && w !== t("import.ungrouped"))).size))}
            </span>
          </div>
          <div className="modal-actions">
            <button className="btn btn-secondary" onClick={onClose}>{t("common.cancel")}</button>
            <button className="btn btn-primary" disabled={checked.size === 0} onClick={() => {
              const selected = importableConns.filter((_, i) => checked.has(i));
              onImport(selected);
            }}>{t("import.import_button")} ({checked.size})</button>
          </div>
        </div>
      </div>
    </div>
  );
}
