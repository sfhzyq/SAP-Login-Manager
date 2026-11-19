import { useState, useEffect, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { save } from "@tauri-apps/plugin-dialog";
import "./styles/global.css";
import type { Credential, Group, SapConnection, AppSettings } from "./types";
import { COLOR_TAGS, ENV_COLORS, ENV_LABEL_KEYS } from "./types";
import { Toast, SetupPage, UnlockPage, CreateGroupModal, RenameGroupModal, GroupManagerModal, RevealPasswordModal } from "./components/common";
import { SapImportModal } from "./components/SapImportModal";
import { SettingsModal } from "./components/SettingsModal";
import { CredentialModal } from "./components/CredentialModal";
import { useI18n, setLanguage } from "./i18n";
import type { Language } from "./i18n";

export default function App() {
  const { t } = useI18n();
  const [masterPassword, setMasterPassword] = useState<string | null>(null);
  const [isSetup, setIsSetup] = useState<boolean | null>(null);
  const [credentials, setCredentials] = useState<Credential[]>([]);
  const [groups, setGroups] = useState<Group[]>([]);
  const [pinnedGroups, setPinnedGroups] = useState<Set<string>>(() => {
    try { return new Set(JSON.parse(localStorage.getItem("pinnedGroups") || "[]")); } catch { return new Set<string>(); }
  });
  const [pinnedCreds, setPinnedCreds] = useState<Set<string>>(() => {
    try { return new Set(JSON.parse(localStorage.getItem("pinnedCreds") || "[]")); } catch { return new Set<string>(); }
  });
  const customGroups = groups.filter(g => !g.is_system && !g.is_default).sort((a, b) => {
    const aP = pinnedGroups.has(a.id), bP = pinnedGroups.has(b.id);
    if (aP !== bP) return aP ? -1 : 1;
    return 0;
  });
  const [sapConnections, setSapConnections] = useState<SapConnection[]>([]);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [search, setSearch] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [selectedGroup, setSelectedGroup] = useState<string | null>(null);
  const [editingCred, setEditingCred] = useState<Credential | undefined>(undefined);
  const [duplicateCred, setDuplicateCred] = useState<Partial<Credential> | undefined>(undefined);
  const [showAddModal, setShowAddModal] = useState(false);
  const [showBatchDeleteConfirm, setShowBatchDeleteConfirm] = useState(false);
  const [showSapImport, setShowSapImport] = useState(false);
  const [showCreateGroup, setShowCreateGroup] = useState(false);
  const [showGroupManager, setShowGroupManager] = useState(false);
  const [showGroupDropdown, setShowGroupDropdown] = useState(false);
  const [collapsedSections, setCollapsedSections] = useState<{ system: boolean; custom: boolean }>(() => {
    try { return { system: false, custom: false, ...JSON.parse(localStorage.getItem("gdCollapsed") || "{}") }; } catch { return { system: false, custom: false }; }
  });
  const toggleSection = (key: "system" | "custom") => {
    setCollapsedSections(prev => { const next = { ...prev, [key]: !prev[key] }; localStorage.setItem("gdCollapsed", JSON.stringify(next)); return next; });
  };
  const [defaultApplied, setDefaultApplied] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [deleteGroupTarget, setDeleteGroupTarget] = useState<Group | null>(null);
  const [deleteGroupMode, setDeleteGroupMode] = useState<"move" | "delete">("move");
  const [renameGroupTarget, setRenameGroupTarget] = useState<Group | null>(null);
  const [groupMenuId, setGroupMenuId] = useState<string | null>(null);
  const [selectedCreds, setSelectedCreds] = useState<Set<string>>(new Set());
  const [lastSelectedIdx, setLastSelectedIdx] = useState<number>(-1);
  const [showMoreMenu, setShowMoreMenu] = useState(false);
  const [isPinned, setIsPinned] = useState(false);
  const [toasts, setToasts] = useState<{ id: number; message: string; type: "success" | "error" | "info"; actionLabel?: string; onAction?: () => void; duration?: number }[]>([]);
  const [showExportVerify, setShowExportVerify] = useState(false);
  const [exportSavePath, setExportSavePath] = useState<string | null>(null);
  const [exportPw, setExportPw] = useState("");
  const [exportErr, setExportErr] = useState("");
  const [showChangePassword, setShowChangePassword] = useState(false);
  const [cpOld, setCpOld] = useState("");
  const [cpNew, setCpNew] = useState("");
  const [cpConfirm, setCpConfirm] = useState("");
  const [cpErr, setCpErr] = useState("");
  const [showBatchMove, setShowBatchMove] = useState(false);
  // 按环境折叠分区的状态（持久化记忆）
  const [envCollapsed, setEnvCollapsed] = useState<Record<string, boolean>>(() => {
    try { return JSON.parse(localStorage.getItem("envCollapsed") || "{}"); } catch { return {}; }
  });
  // 键盘导航焦点索引（-1 = 无焦点）
  const [focusIdx, setFocusIdx] = useState(-1);
  // 右键菜单 {x, y, cred}
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; cred: Credential } | null>(null);
  // 查看密码弹窗（需二次校验解锁密码）
  const [revealCred, setRevealCred] = useState<Credential | null>(null);
  // 环境色图例（仅首次显示）
  const [showEnvLegend, setShowEnvLegend] = useState(() => { try { return !localStorage.getItem("envLegendShown"); } catch { return true; } });
  // 锁定前确认（有未保存编辑时）
  const [showLockConfirm, setShowLockConfirm] = useState(false);

  const addToast = (m: string, ty: "success" | "error" | "info", opts?: { actionLabel?: string; onAction?: () => void; duration?: number }) => setToasts(p => [...p, { id: Date.now() + Math.random(), message: m, type: ty, ...opts }]);
  const removeToast = (id: number) => setToasts(p => p.filter(t => t.id !== id));

  // 复制到剪贴板（敏感信息按设置自动清空）
  const copyText = async (text: string) => {
    try { await navigator.clipboard.writeText(text); } catch { return false; }
    const secs = settings?.clipboard_clear_seconds ?? 20;
    if (secs > 0) setTimeout(() => { navigator.clipboard.writeText("").catch(() => {}); }, secs * 1000);
    return true;
  };

  // 搜索关键词高亮（命中片段用 <mark> 包裹）
  const highlightText = (text: string, q: string) => {
    if (!q) return text;
    const idx = text.toLowerCase().indexOf(q.toLowerCase());
    if (idx < 0) return text;
    return <>{text.slice(0, idx)}<mark>{text.slice(idx, idx + q.length)}</mark>{text.slice(idx + q.length)}</>;
  };

  useEffect(() => { (async () => { try { setIsSetup(await invoke<boolean>("is_password_set")); } catch { setIsSetup(false); } })(); }, []);

  // 免密模式（进程内会话免密）：主密码仅保存在内存 sessionPwRef，退出应用即失效。
  // 每次冷启动一律要求输入主密码；本次运行内锁定后可用会话密码一键快速解锁。
  const sessionPwRef = useRef<string | null>(null);
  // 安全清理：清除历史版本可能落盘的 DPAPI 记忆密码（改为纯内存方案后不再持久化）
  useEffect(() => { invoke("clear_remembered_password").catch(() => {}); }, []);

  // 启动时提前读取配置中的主题并应用（get_settings 无需主密码），使登录/解锁页也遵循暗黑模式
  useEffect(() => {
    (async () => {
      try {
        const sett = await invoke<AppSettings>("get_settings");
        if (sett) setSettings(sett); // 预加载设置，使解锁页也能读取 password_free 等
        if (sett?.theme) {
          document.documentElement.setAttribute("data-theme", sett.theme);
          try { localStorage.setItem("app-theme", sett.theme); } catch {}
        }
      } catch {}
    })();
  }, []);

  // 应用主题（登录后设置变更时）+ 缓存供下次首帧使用
  useEffect(() => {
    if (settings?.theme) {
      document.documentElement.setAttribute("data-theme", settings.theme);
      try { localStorage.setItem("app-theme", settings.theme); } catch {}
    }
  }, [settings?.theme]);

  // 应用窗口置顶设置
  useEffect(() => {
    const v = settings?.always_on_top ?? false;
    setIsPinned(v);
    getCurrentWindow().setAlwaysOnTop(v).catch(() => {});
  }, [settings?.always_on_top]);

  const loadData = async () => {
    try {
      const [creds, grps, conns, sett] = await Promise.all([
        invoke<Credential[]>("get_credentials"), invoke<Group[]>("get_groups"), invoke<SapConnection[]>("get_sap_connections").catch(() => []), invoke<AppSettings>("get_settings").catch(() => null),
      ]);
      setCredentials(creds); setGroups(grps); setSapConnections(conns);
      if (sett) { setSettings(sett); if (sett.default_language) setLanguage(sett.default_language as Language); }
    } catch (e) { addToast(String(e), "error"); }
  };

  useEffect(() => { if (masterPassword) loadData(); }, [masterPassword]);

  // 默认视图：首次加载后按「设置-默认打开的分组」定位（仅生效一次，不覆盖用户后续切换）
  // 规则：""=全部；"favorites"=收藏；否则为分组ID（若该分组已删除则回退到全部）
  useEffect(() => {
    if (!defaultApplied && masterPassword && settings && credentials.length > 0) {
      const dg = settings.default_group || "";
      if (dg === "") {
        setSelectedGroup(null);
      } else if (dg === "favorites") {
        setSelectedGroup("favorites");
      } else if (groups.some(g => g.id === dg)) {
        setSelectedGroup(dg);
      } else {
        setSelectedGroup(null); // 分组已删除，回退到全部
      }
      setDefaultApplied(true);
    }
  }, [credentials, masterPassword, defaultApplied, settings, groups]);

  // 搜索防抖（300ms）
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedSearch(search), 300);
    return () => clearTimeout(timer);
  }, [search]);

  const handleLogin = async (id: string) => { try { await invoke("login_to_sap", { masterPassword, credentialId: id }); addToast(t("toast.login_start"), "info"); } catch (e) { addToast(t("toast.login_failed") + ": " + String(e), "error"); } };
  // 供键盘导航 effect 使用最新引用（避免闭包旧值）
  const handleLoginRef = useRef(handleLogin);
  handleLoginRef.current = handleLogin;
  const searchInputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const handleSaveCred = async (form: Partial<Credential>) => {
    try {
      if (editingCred) { await invoke("update_credential", { masterPassword, credential: { ...editingCred, ...form } }); addToast(t("toast.credential_updated"), "success"); }
      else { await invoke("add_credential", { masterPassword, credential: form }); addToast(t("toast.credential_added"), "success"); }
      await loadData(); setEditingCred(undefined); setShowAddModal(false);
    } catch (e) { addToast(t("toast.save_failed") + ": " + String(e), "error"); }
  };
  const handleDuplicateCred = (form: Partial<Credential>) => {
    // 复制配置：connection_id（指向 SAP 配置的连接名 -sysname）保持不变，仅改 display_name
    const cloned = { ...form, id: undefined, connection_id: form.connection_id, display_name: (form.display_name || form.system_id || "") + t("credential.copy_suffix"), username: "", password: "", login_count: 0, last_login_at: undefined };
    setEditingCred(undefined); setShowAddModal(false);
    setTimeout(() => { setDuplicateCred(cloned); setShowAddModal(true); }, 100);
  };
  const handleExport = async () => {
    try {
      const path = await save({ filters: [{ name: "JSON", extensions: ["json"] }], defaultPath: "sap-credentials-export.json" });
      if (!path) return;
      setExportSavePath(path);
      setExportPw("");
      setExportErr("");
      setShowExportVerify(true);
    } catch (e) { addToast(t("toast.export_failed") + ": " + String(e), "error"); }
  };

  const confirmExport = async () => {
    setExportErr("");
    try {
      await invoke("export_credentials", { masterPassword: exportPw, savePath: exportSavePath });
      setShowExportVerify(false); setExportSavePath(null); setExportPw(""); setExportErr("");
      addToast(t("toast.export_success"), "success");
    } catch (e) { setExportErr(t("export_verify.wrong_password")); }
  };

  const handleChangePassword = async () => {
    setCpErr("");
    if (cpNew.length < 4) return setCpErr(t("setup.password_min"));
    if (cpNew !== cpConfirm) return setCpErr(t("changepw.password_mismatch"));
    try {
      await invoke("change_master_password", { oldPassword: cpOld, newPassword: cpNew });
      setMasterPassword(cpNew);
      // 免密（内存会话）：同步更新本次会话记住的新密码
      if (settings?.password_free) sessionPwRef.current = cpNew;
      setShowChangePassword(false); setCpOld(""); setCpNew(""); setCpConfirm(""); setCpErr("");
      addToast(t("changepw.success"), "success");
    } catch (e) { setCpErr(t("changepw.failed")); }
  };

  const handleSapImport = async (selected: SapConnection[]) => {
    try {
      const count = await invoke<number>("import_sap_connections", { connections: selected });
      await loadData(); setShowSapImport(false);
      addToast(t("toast.import_success_count").replace("{count}", String(count)), "success");
    } catch (e) { addToast(t("toast.import_failed") + ": " + String(e), "error"); }
  };

  const handleCreateGroup = async (name: string) => {
    try { await invoke("add_group", { group: { id: "", group_name: name, entries: [], is_system: false, is_default: false } }); await loadData(); setShowCreateGroup(false); addToast(t("toast.group_created"), "success"); }
    catch (e) { addToast(t("toast.create_failed") + ": " + String(e), "error"); }
  };

  const handleDeleteGroup = (group: Group) => { setGroupMenuId(null); setDeleteGroupTarget(group); };

  const handleRenameGroup = async (group: Group, newName: string) => {
    try {
      await invoke("rename_group", { groupId: group.id, newName });
      setGroups(prev => prev.map(g => g.id === group.id ? { ...g, group_name: newName } : g));
      addToast(t("toast.rename_success"), "success");
    } catch (e) { addToast(e instanceof Error ? e.message : t("toast.rename_failed"), "error"); }
  };

  const togglePinGroup = (groupId: string) => {
    setPinnedGroups(prev => {
      const next = new Set(prev);
      if (next.has(groupId)) { next.delete(groupId); }
      else { next.add(groupId); }
      localStorage.setItem("pinnedGroups", JSON.stringify([...next]));
      return next;
    });
  };

  const togglePinCred = (credId: string) => {
    setPinnedCreds(prev => {
      const next = new Set(prev);
      if (next.has(credId)) { next.delete(credId); }
      else { next.add(credId); }
      localStorage.setItem("pinnedCreds", JSON.stringify([...next]));
      return next;
    });
  };

  const handleConfirmDeleteGroup = async (mode: "move" | "delete") => {
    if (!deleteGroupTarget) return;
    const { id, entries } = deleteGroupTarget;
    try {
      if (entries.length > 0) { await invoke("delete_group_with_options", { id, mode }); }
      else { await invoke("delete_group", { id }); }
      await loadData();
      if (selectedGroup === id) setSelectedGroup(null);
      setDeleteGroupTarget(null);
      addToast(mode === "move" ? t("toast.group_deleted_moved").replace("{count}", String(entries.length)) : t("toast.group_deleted_all").replace("{count}", String(entries.length)), "success");
    } catch (e) { addToast(String(e), "error"); }
  };

  const handleToggleFavorite = async (id: string) => {
    try { await invoke("toggle_favorite", { credentialId: id }); await loadData(); } catch (e) { addToast(String(e), "error"); }
  };

  const handleSaveSettings = async (s: AppSettings) => {
    try {
      await invoke("save_settings", { settings: s });
      // 免密改为进程内会话：开启/保持免密时记住当前会话密码（仅内存，不落盘）；关闭时清除
      const wasFree = settings?.password_free ?? false;
      if (s.password_free && masterPassword) {
        sessionPwRef.current = masterPassword;
      } else if (!s.password_free && wasFree) {
        sessionPwRef.current = null;
      }
      setSettings(s); setShowSettings(false); if (s.default_language) setLanguage(s.default_language as Language); addToast(t("toast.settings_saved"), "success");
    } catch (e) { addToast(t("toast.save_failed") + ": " + String(e), "error"); }
  };

  // 多选逻辑
  const handleCardClick = (e: React.MouseEvent, credId: string, idx: number) => {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      setSelectedCreds(prev => { const n = new Set(prev); n.has(credId) ? n.delete(credId) : n.add(credId); return n; });
      setLastSelectedIdx(idx);
    } else if (e.shiftKey && lastSelectedIdx >= 0) {
      e.preventDefault();
      const start = Math.min(lastSelectedIdx, idx);
      const end = Math.max(lastSelectedIdx, idx);
      // 使用函数式更新，避免闭包陷阱；基于可见顺序（visibleCreds）保证索引与折叠视图一致
      setSelectedCreds(prev => {
        const newSet = new Set(prev);
        visibleCreds.slice(start, end + 1).forEach(c => newSet.add(c.id));
        return newSet;
      });
    }
  };

  const clearSelection = () => { setSelectedCreds(new Set()); setLastSelectedIdx(-1); };

  // 批量登录：逐条执行并汇总成功/失败明细（分区一键登录也复用）
  const handleBatchLogin = async (idsOverride?: string[]) => {
    const ids = (idsOverride ?? Array.from(selectedCreds)).filter(Boolean);
    if (ids.length === 0) return;
    const interval = settings?.batch_login_interval || 2;
    addToast(t("toast.batch_login_start_count").replace("{count}", String(ids.length)), "info");
    let ok = 0;
    const failNames: string[] = [];
    for (const id of ids) {
      try { await invoke("login_to_sap", { masterPassword, credentialId: id }); ok++; }
      catch { const c = credentials.find(x => x.id === id); if (c) failNames.push(c.display_name || c.connection_id); }
      if (ids.length > 1) await new Promise(r => setTimeout(r, interval * 1000));
    }
    await loadData(); clearSelection();
    const fail = failNames.length;
    if (fail === 0) addToast(t("toast.batch_login_success_count").replace("{count}", String(ok)), "success");
    else if (ok === 0) addToast(t("toast.batch_login_failed"), "error");
    else addToast(t("toast.batch_login_summary").replace("{ok}", String(ok)).replace("{fail}", String(fail)) + "：" + failNames.join("、"), "error");
  };

  const handleBatchDelete = async () => {
    const ids = Array.from(selectedCreds);
    if (ids.length === 0) return;
    setShowBatchDeleteConfirm(true);
  };
  // 删除凭据（含撤销）：snapshot 保存原样数据，撤销时原样恢复
  const deleteCredsWithUndo = async (ids: string[]) => {
    if (ids.length === 0) return;
    const snapshot = credentials.filter(c => ids.includes(c.id));
    let ok = 0;
    for (const id of ids) {
      try { await invoke("delete_credential", { id }); ok++; } catch (e) { addToast(t("toast.delete_failed") + ": " + String(e), "error"); }
    }
    await loadData();
    clearSelection();
    if (ok === 0) return;
    addToast(t("toast.batch_delete_success_count").replace("{count}", String(ok)), "success", {
      duration: 8000,
      actionLabel: t("common.undo"),
      onAction: async () => {
        try {
          const n = await invoke<number>("restore_credentials", { credentials: snapshot });
          await loadData();
          addToast(t("toast.undo_success").replace("{count}", String(n)), "success");
        } catch (e) { addToast(t("toast.undo_failed") + ": " + String(e), "error"); }
      },
    });
  };
  const confirmBatchDelete = async () => {
    const ids = Array.from(selectedCreds);
    setShowBatchDeleteConfirm(false);
    await deleteCredsWithUndo(ids);
  };

  const handleBatchShare = async () => {
    const ids = Array.from(selectedCreds);
    if (ids.length === 0) return;
    let ok = 0;
    for (const id of ids) {
      try { await invoke<string>("share_credential", { credentialId: id }); ok++; } catch (e) { addToast(t("toast.share_failed") + ": " + String(e), "error"); }
    }
    addToast(t("toast.batch_share_success_count").replace("{count}", String(ok)), "success");
  };

  // 批量收藏（true=收藏，false=取消收藏）
  const handleBatchFavorite = async (favorite: boolean) => {
    const ids = Array.from(selectedCreds);
    if (ids.length === 0) return;
    try {
      const count = await invoke<number>("batch_favorite", { credentialIds: ids, favorite });
      await loadData();
      addToast((favorite ? t("toast.batch_favorite_success") : t("toast.batch_unfavorite_success")).replace("{count}", String(count)), "success");
    } catch (e) { addToast(t("toast.save_failed") + ": " + String(e), "error"); }
  };

  // 批量移动分组
  const handleBatchMove = async (group_id: string) => {
    const ids = Array.from(selectedCreds);
    if (ids.length === 0) { setShowBatchMove(false); return; }
    try {
      const count = await invoke<number>("batch_move_to_group", { credentialIds: ids, groupId: group_id });
      await loadData();
      setShowBatchMove(false);
      addToast(t("toast.batch_move_success").replace("{count}", String(count)), "success");
      // 移动后若当前视图是旧分组，刷新时自动调整（selectedGroup 保持，列表重新过滤）
    } catch (e) { addToast(t("toast.batch_move_failed"), "error"); }
  };

  // 全选/取消全选（基于当前显示列表）
  const handleSelectAll = () => {
    // 全选基于可见顺序（折叠分区内的卡片不参与）
    if (selectedCreds.size === visibleCreds.length && visibleCreds.length > 0) {
      clearSelection();
    } else {
      setSelectedCreds(new Set(visibleCreds.map(c => c.id)));
      setLastSelectedIdx(visibleCreds.length - 1);
    }
  };

  // 自动锁定：无操作 N 分钟后锁定
  const autoLockTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const autoLockedRef = useRef(false);
  useEffect(() => {
    const resetAutoLock = () => {
      // 已锁定后不再重置定时器，避免累积多个 toast
      if (autoLockedRef.current) return;
      if (autoLockTimer.current) clearTimeout(autoLockTimer.current);
      const minutes = settings?.auto_lock_minutes ?? 3;
      if (minutes > 0) {
        autoLockTimer.current = setTimeout(() => {
          autoLockedRef.current = true;
          clearSelection();
          setMasterPassword(null);
          addToast(t("toast.auto_locked"), "info");
        }, minutes * 60 * 1000);
      }
    };
    // 解锁时重置锁定标记
    if (masterPassword) autoLockedRef.current = false;
    resetAutoLock();
    window.addEventListener("mousemove", resetAutoLock);
    window.addEventListener("keydown", resetAutoLock);
    window.addEventListener("click", resetAutoLock);
    return () => {
      if (autoLockTimer.current) clearTimeout(autoLockTimer.current);
      window.removeEventListener("mousemove", resetAutoLock);
      window.removeEventListener("keydown", resetAutoLock);
      window.removeEventListener("click", resetAutoLock);
    };
  }, [settings?.auto_lock_minutes, masterPassword]);

  // 全局快捷键: Ctrl+B 批量登录
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "b" && !e.shiftKey && !e.altKey) {
        const target = e.target as HTMLElement;
        if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return;
        e.preventDefault();
        if (selectedCreds.size > 0) handleBatchLogin();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [selectedCreds]);

  // 系统分组ID到环境属性的映射（在条件 return 之前定义，避免 Hook 顺序问题）
  const SYSTEM_GROUP_ENV: Record<string, string> = {
    "group-production": "production",
    "group-test": "test",
    "group-development": "development",
    "group-configuration": "configuration",
  };

  // 系统分组ID到翻译键的映射
  const SYSTEM_GROUP_NAME_KEYS: Record<string, string> = {
    "group-production": "group.name.production",
    "group-test": "group.name.test",
    "group-development": "group.name.development",
    "group-configuration": "group.name.configuration",
  };

  // 获取分组显示名称（系统/默认分组按 ID 翻译，自建分组用原名）
  const getGroupDisplayName = (g: Group): string => {
    if (g.is_default) return t("group.name.default");
    if (SYSTEM_GROUP_NAME_KEYS[g.id]) return t(SYSTEM_GROUP_NAME_KEYS[g.id] as any);
    return g.group_name;
  };

  // 分组凭据计数（分组筛选 chips 用）
  const getGroupCount = (g: Group): number => {
    if (g.is_default) return credentials.length;
    if (SYSTEM_GROUP_ENV[g.id]) return credentials.filter(c => c.environment === SYSTEM_GROUP_ENV[g.id]).length;
    return g.entries.length;
  };

  // 所有 useMemo 必须在条件 return 之前调用，保证 Hook 顺序一致
  const filtered = useMemo(() => credentials.filter(c => {
    const q = debouncedSearch.toLowerCase();
    let ms = !q;
    if (q) {
      const grp = c.group_id ? groups.find(g => g.id === c.group_id) : undefined;
      const fields = [
        c.display_name, c.connection_id, c.system_id, c.username,
        c.app_server, c.message_server, c.system_number, c.client,
        c.description, c.saprouter, c.logon_group, c.snc_name,
        c.environment, grp?.group_name,
      ];
      ms = fields.some(f => !!f && f.toLowerCase().includes(q));
    }
    if (!selectedGroup || selectedGroup === "favorites") return ms;
    const defaultGroup = groups.find(g => g.is_default);
    if (defaultGroup && selectedGroup === defaultGroup.id) return ms;
    if (SYSTEM_GROUP_ENV[selectedGroup]) return ms && c.environment === SYSTEM_GROUP_ENV[selectedGroup];
    return ms && c.group_id === selectedGroup;
  }), [credentials, debouncedSearch, selectedGroup, groups]);

  const displayCreds = useMemo(() => (selectedGroup === "favorites" ? filtered.filter(c => c.is_favorite) : filtered).slice().sort((a, b) => {
    const aP = pinnedCreds.has(a.id), bP = pinnedCreds.has(b.id);
    if (aP !== bP) return aP ? -1 : 1;
    return (b.login_count || 0) - (a.login_count || 0);
  }), [filtered, selectedGroup, pinnedCreds]);

  const favoriteCreds = useMemo(() => credentials.filter(c => c.is_favorite), [credentials]);

  // === 按系统分组（环境）折叠分区 ===
  // 规则：设置开启且分区数 ≥ 2 时按环境分区折叠；仅一个分区时不展示分组信息（平铺）
  const ENV_ORDER = ["production", "test", "development", "configuration", ""];
  const sections = useMemo(() => {
    if (!(settings?.group_by_environment ?? true)) return null;
    const map = new Map<string, Credential[]>();
    for (const c of displayCreds) {
      const key = c.environment || "";
      if (!map.has(key)) map.set(key, []);
      map.get(key)!.push(c);
    }
    const secs = ENV_ORDER.filter(k => map.has(k)).map(k => ({ key: k || "none", env: k, creds: map.get(k)! }));
    // 单一分区 → 降级平铺（不展示分组信息）
    if (secs.length <= 1) return null;
    return secs;
  }, [displayCreds, settings?.group_by_environment]);

  // 键盘导航可见卡片（折叠分区内的卡片不占索引）
  const visibleCreds = useMemo(() => {
    if (!sections) return displayCreds;
    return sections.filter(s => !envCollapsed[s.key]).flatMap(s => s.creds);
  }, [sections, envCollapsed, displayCreds]);

  // 最近使用区：仅平铺模式 + "全部"视图 + 无搜索时显示（最近登录 3 条）
  const recentCreds = useMemo(() => {
    if (selectedGroup !== null || debouncedSearch || sections) return [];
    return credentials.filter(c => c.last_login_at)
      .slice().sort((a, b) => (b.last_login_at! > a.last_login_at! ? 1 : -1))
      .slice(0, 3);
  }, [credentials, selectedGroup, debouncedSearch, sections]);

  const toggleEnvSection = (key: string) => {
    setEnvCollapsed(prev => {
      const next = { ...prev, [key]: !prev[key] };
      try { localStorage.setItem("envCollapsed", JSON.stringify(next)); } catch {}
      return next;
    });
  };

  // 分区头操作：全选本组 / 取消全选本组
  const toggleSelectSection = (creds: Credential[]) => {
    const ids = creds.map(c => c.id);
    const allSelected = ids.length > 0 && ids.every(id => selectedCreds.has(id));
    setSelectedCreds(prev => {
      const next = new Set(prev);
      if (allSelected) ids.forEach(id => next.delete(id));
      else ids.forEach(id => next.add(id));
      return next;
    });
  };

  // 分区头操作：一键登录本组（仅可登录的凭据：账号/客户端/语言齐备，SNC 凭据无需密码）
  const handleSectionLogin = (creds: Credential[]) => {
    const ids = creds.filter(c => c.client && c.language && c.username && (c.snc_enabled || c.encrypted_password)).map(c => c.id);
    if (ids.length === 0) return;
    handleBatchLogin(ids);
  };

  // 键盘导航：/ 聚焦搜索，↑↓ 移动焦点，Enter 登录，Esc 取消焦点/关闭菜单
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") { setFocusIdx(-1); setCtxMenu(null); return; }
      const target = e.target as HTMLElement;
      const inInput = target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.tagName === "SELECT" || target.isContentEditable;
      const anyModal = showAddModal || showSettings || showSapImport || showBatchMove || showGroupManager || !!deleteGroupTarget || !!renameGroupTarget;
      if (anyModal) return;
      if (e.key === "/" && !inInput && !e.ctrlKey && !e.altKey) { e.preventDefault(); searchInputRef.current?.focus(); searchInputRef.current?.select(); return; }
      if (inInput) return;
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setFocusIdx(prev => {
          const n = visibleCreds.length;
          if (n === 0) return -1;
          if (prev < 0) return 0;
          return (prev + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
        });
      } else if (e.key === "Enter" && focusIdx >= 0 && focusIdx < visibleCreds.length) {
        const c = visibleCreds[focusIdx];
        if (c) { e.preventDefault(); handleLoginRef.current(c.id); }
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [visibleCreds, focusIdx, showAddModal, showSettings, showSapImport, showBatchMove, showGroupManager, deleteGroupTarget, renameGroupTarget]);

  // 键盘焦点变化时滚动到可见
  useEffect(() => {
    if (focusIdx < 0) return;
    listRef.current?.querySelector(`[data-card-idx="${focusIdx}"]`)?.scrollIntoView({ block: "nearest" });
  }, [focusIdx]);

  // 列表数据变化时修正越界的焦点索引
  useEffect(() => {
    setFocusIdx(prev => prev >= visibleCreds.length ? -1 : prev);
  }, [visibleCreds.length]);

  // 可见索引映射（键盘导航 / Shift 多选基于可见顺序）
  // 注意：必须位于早退分支之前，否则锁定/解锁切换时 Hook 数量不一致导致 React #310 崩溃
  const visibleIdxMap = useMemo(() => {
    const m = new Map<string, number>();
    visibleCreds.forEach((c, i) => m.set(c.id, i));
    return m;
  }, [visibleCreds]);

  if (!masterPassword) {
    if (isSetup === null) return null;
    if (!isSetup) return <SetupPage onSetup={p => { setMasterPassword(p); setIsSetup(true); if (settings?.password_free) sessionPwRef.current = p; }} />;
    return <UnlockPage onUnlock={p => { setMasterPassword(p); if (settings?.password_free) sessionPwRef.current = p; }} quickUnlockPw={settings?.password_free ? sessionPwRef.current : null} />;
  }

  const hasSelection = selectedCreds.size > 0;

  // 当前选中分组的完整名称
  const currentGroupName = selectedGroup === null ? t("common.all") : selectedGroup === "favorites" ? t("common.favorites") : (() => { const g = groups.find(g => g.id === selectedGroup); return g ? getGroupDisplayName(g) : ""; })();

  // 凭据卡片渲染（平铺与分区视图共用）
  const renderCard = (c: Credential) => {
    const idx = visibleIdxMap.get(c.id) ?? -1;
    const cTag = c.color_tag || "";
    const tagColor = COLOR_TAGS.find(t => t.value === cTag)?.color;
    const envColor = ENV_COLORS[c.environment || ""] || "#6b7280";
    const isSelected = selectedCreds.has(c.id);
    const isCardPinned = pinnedCreds.has(c.id);
    const isFocused = focusIdx === idx && idx >= 0;
    const displayName = c.display_name || c.connection_id;
    const borderColor = tagColor || envColor;
    const connTypeLabel = c.connection_type === "load_balancing" ? t("credential.connection_type_load_balance") : "";
    const avatarText = (c.system_id || displayName || "?").slice(0, 3).toUpperCase();
    // 可用性分级：关键信息缺失（红）vs 仅缺密码（黄）
    const canSso = !!(c.snc_enabled && c.snc_sso);
    const criticalMissing: string[] = [];
    if (!c.client) criticalMissing.push(t("credential.client"));
    if (!c.language) criticalMissing.push(t("credential.language"));
    if (!c.username) criticalMissing.push(c.snc_enabled ? t("credential.snc_account") : t("credential.username"));
    const pwMissing = criticalMissing.length === 0 && !canSso && !!c.username && !c.encrypted_password;
    const loginDisabled = criticalMissing.length > 0 || pwMissing;
    const missingList = [...criticalMissing, ...(pwMissing ? [t("card.missing_password")] : [])];
    const missingHint = loginDisabled ? t("card.missing_prefix") + missingList.join("、") : "";
    const loginCount = c.login_count || 0;
    return (
      <div key={c.id} className={`card cred-card${isSelected ? " selected" : ""}${isCardPinned ? " pinned" : ""}${loginDisabled ? " unavailable" : ""}${isFocused ? " focused" : ""}`} style={{ borderLeft: `3px solid ${borderColor}` }} data-card-idx={idx} onClick={(e) => handleCardClick(e, c.id, idx)} onDoubleClick={() => { if (!loginDisabled) handleLogin(c.id); }} onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); setCtxMenu({ x: e.clientX, y: e.clientY, cred: c }); }} data-tooltip={loginDisabled ? missingHint : t("card.dblclick_login")} data-tooltip-pos="top">
        {isCardPinned && <span className="pin-corner" title={t("card.pin")}><svg width="9" height="9" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2 4 10h5v12h6V10h5z" /></svg></span>}
        <div style={{ display: "flex", alignItems: "center", gap: 8, flex: 1, minWidth: 0 }}>
          <label className="check-wrap" onClick={e => { e.stopPropagation(); setSelectedCreds(prev => { const n = new Set(prev); n.has(c.id) ? n.delete(c.id) : n.add(c.id); return n; }); setLastSelectedIdx(idx); }}>
            <input type="checkbox" checked={isSelected} onChange={() => {}} />
            <span className="check-box" />
          </label>
          {/* 头像承载环境色 */}
          <div className="cred-avatar" style={{ background: envColor }} title={`${c.system_id || ""} · ${ENV_LABEL_KEYS[c.environment || ""] ? t(ENV_LABEL_KEYS[c.environment || ""] as any) : t("card.unclassified")}`}>{avatarText}</div>
          <div style={{ minWidth: 0, flex: 1 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 5, minWidth: 0 }}>
              <span style={{ fontFamily: "var(--font-display)", fontWeight: 650, fontSize: 13, letterSpacing: -0.2, color: "var(--text-primary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{highlightText(displayName, debouncedSearch)}</span>
              {c.snc_enabled && <span className="cred-badge" style={{ background: "var(--accent)", color: "white" }}>SNC</span>}
              {/* 登录频次标记（≥5 次显示火苗徽章） */}
              {loginCount >= 5 && (
                <span className="freq-badge" data-tooltip={t("card.login_times").replace("{count}", String(loginCount))} data-tooltip-pos="top">
                  <svg width="9" height="9" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2s4 4.5 4 9a4 4 0 0 1-8 0c0-1.5.5-2.5 1-3.5C7.5 9 6 10.5 6 13a6 6 0 0 0 12 0c0-5.5-6-11-6-11z" /></svg>
                  {loginCount >= 100 ? "99+" : loginCount}
                </span>
              )}
              {criticalMissing.length > 0 && <span className="cred-badge cred-badge-warn" title={missingHint}>!</span>}
              {pwMissing && <span className="cred-badge cred-badge-warn cred-badge-soft" title={missingHint}>!</span>}
            </div>
            {/* 副标题：客户端 · 用户名 · 消息服务器(负载均衡) · 连接方式(非直连) · 语言 */}
            <div style={{ fontSize: 10.5, color: "var(--text-muted)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", marginTop: 3, fontFamily: "var(--font-mono)", letterSpacing: -0.1, display: "flex", alignItems: "center", gap: 4 }}>
              {c.client && <span style={{ color: "var(--accent)", fontWeight: 700, flexShrink: 0 }}>{highlightText(c.client, debouncedSearch)}</span>}
              {c.client && c.username && <span style={{ opacity: 0.4 }}>·</span>}
              {c.username && <span style={{ color: "var(--text-secondary)", fontWeight: 600, flexShrink: 0 }}>{highlightText(c.username, debouncedSearch)}</span>}
              {c.connection_type === "load_balancing" && c.message_server && (c.client || c.username) && <span style={{ opacity: 0.4 }}>·</span>}
              {c.connection_type === "load_balancing" && c.message_server && <span style={{ overflow: "hidden", textOverflow: "ellipsis" }} title={c.message_server}>{highlightText(c.message_server, debouncedSearch)}</span>}
              {connTypeLabel && <span style={{ opacity: 0.4 }}>·</span>}
              {connTypeLabel && <span style={{ flexShrink: 0 }}>{connTypeLabel}</span>}
              {c.language && <span style={{ flexShrink: 0 }}>·</span>}
              {c.language && <span className="lang-chip">{c.language}</span>}
            </div>
          </div>
        </div>
        <div className="cred-actions" onClick={e => e.stopPropagation()}>
          <button className="btn btn-primary btn-sm cred-login" style={{ padding: "4px 8px", display: "flex", alignItems: "center", gap: 3 }} disabled={loginDisabled} onClick={() => handleLogin(c.id)} data-tooltip={loginDisabled ? missingHint : undefined} data-tooltip-pos="top">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4" /><polyline points="10 17 15 12 10 7" /><line x1="15" y1="12" x2="3" y2="12" /></svg>
            <span style={{ fontSize: 10 }}>{t("card.login")}</span>
          </button>
          <span className="cred-more-actions">
            <button className="btn btn-ghost btn-sm" style={{ padding: "4px", display: "flex", alignItems: "center", color: isCardPinned ? "var(--accent)" : undefined }} onClick={(e) => { e.stopPropagation(); togglePinCred(c.id); }} data-tooltip={isCardPinned ? t("card.unpin") : t("card.pin")} data-tooltip-pos="top">
              <svg width="13" height="13" viewBox="0 0 24 24" fill={isCardPinned ? "currentColor" : "none"} stroke="currentColor" strokeWidth="2"><line x1="12" y1="17" x2="12" y2="22" /><path d="M5 17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1v4.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V17z" /></svg>
            </button>
            <button className="btn btn-ghost btn-sm" style={{ padding: "4px", display: "flex", alignItems: "center" }} onClick={(e) => { e.stopPropagation(); handleToggleFavorite(c.id); }} data-tooltip={c.is_favorite ? t("card.unfavorite") : t("card.favorite")} data-tooltip-pos="top">
              {c.is_favorite
                ? <svg width="13" height="13" viewBox="0 0 24 24" fill="#f1c21b" stroke="#f1c21b" strokeWidth="2"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" /></svg>
                : <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" /></svg>
              }
            </button>
            <button className="btn btn-ghost btn-sm" style={{ padding: "4px", display: "flex", alignItems: "center" }} onClick={() => { setEditingCred(c); setDuplicateCred(undefined); setShowAddModal(true); }} data-tooltip={t("card.edit")} data-tooltip-pos="top">
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" /><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" /></svg>
            </button>
            <button className="btn btn-ghost btn-sm" style={{ padding: "4px", display: "flex", alignItems: "center" }} onClick={() => handleDuplicateCred(c)} data-tooltip={t("card.duplicate")} data-tooltip-pos="top">
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="9" y="9" width="13" height="13" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg>
            </button>
          </span>
        </div>
      </div>
    );
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100vh", position: "relative" }} onClick={(e) => { if (groupMenuId) setGroupMenuId(null); if (showMoreMenu) setShowMoreMenu(false); if (showGroupDropdown) setShowGroupDropdown(false); if (ctxMenu) setCtxMenu(null); if ((e.target as HTMLElement).classList.contains("empty-state") || e.target === e.currentTarget) clearSelection(); }} onContextMenu={(e) => { if (ctxMenu && !(e.target as HTMLElement).closest(".ctx-menu")) setCtxMenu(null); }}>
      <header data-tauri-drag-region className="app-header" style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "8px 8px 8px 14px", background: "var(--bg-elevated)", borderBottom: "1px solid var(--border-color)", boxShadow: "var(--shadow-sm)", zIndex: 10, flexShrink: 0 }}>
        <h1 style={{ fontFamily: "var(--font-display)", fontSize: 14, fontWeight: 700, letterSpacing: -0.5, color: "var(--text-primary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{currentGroupName}</h1>
        <div style={{ display: "flex", gap: 2, alignItems: "center", flexShrink: 0 }} onClick={e => e.stopPropagation()}>
          {/* 新增凭据 */}
          <button className="btn btn-ghost btn-sm" style={{ padding: "6px", display: "flex", alignItems: "center" }} onClick={() => { setEditingCred(undefined); setDuplicateCred(undefined); setShowAddModal(true); }} data-tooltip={t("tooltip.add_credential")}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" /></svg>
          </button>
          {/* 启动 SAP Logon */}
          <button className="btn btn-ghost btn-sm" style={{ padding: "6px", display: "flex", alignItems: "center" }} onClick={async () => { try { await invoke("open_sap_logon"); addToast(t("toast.sap_logon_started"), "success"); } catch (e) { addToast(t("toast.sap_logon_failed") + ": " + String(e), "error"); } }} data-tooltip={t("tooltip.start_sap_logon")}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><polyline points="15 3 21 3 21 9" /><line x1="10" y1="14" x2="21" y2="3" /></svg>
          </button>
          {/* 切换亮暗主题 */}
          <button className="btn btn-ghost btn-sm" style={{ padding: "6px", display: "flex", alignItems: "center" }} onClick={async () => { const cur = settings?.theme || "light"; const isDark = cur.startsWith("dark"); const newTheme = isDark ? (cur === "dark" ? "light" : cur.replace("dark-", "light-")) : (cur === "light" ? "dark" : cur.replace("light-", "dark-")); const updated = { ...settings!, theme: newTheme }; setSettings(updated); document.documentElement.setAttribute("data-theme", newTheme); try { await invoke("save_settings", { settings: updated }); } catch {} }} data-tooltip={t("tooltip.theme")}>
            {(settings?.theme || "light").startsWith("dark")
              ? <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="5" /><line x1="12" y1="1" x2="12" y2="3" /><line x1="12" y1="21" x2="12" y2="23" /><line x1="4.2" y1="4.2" x2="5.6" y2="5.6" /><line x1="18.4" y1="18.4" x2="19.8" y2="19.8" /><line x1="1" y1="12" x2="3" y2="12" /><line x1="21" y1="12" x2="23" y2="12" /><line x1="4.2" y1="19.8" x2="5.6" y2="18.4" /><line x1="18.4" y1="5.6" x2="19.8" y2="4.2" /></svg>
              : <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" /></svg>
            }
          </button>
          {/* 窗口置顶切换 */}
          <button className="btn btn-ghost btn-sm" style={{ padding: "6px", display: "flex", alignItems: "center" }} onClick={async () => { try { const w = getCurrentWindow(); const next = !isPinned; await w.setAlwaysOnTop(next); setIsPinned(next); if (settings) { const updated = { ...settings, always_on_top: next }; setSettings(updated); try { await invoke("save_settings", { settings: updated }); } catch {} } } catch {} }} data-tooltip={isPinned ? t("tooltip.unpin") : t("tooltip.pin")}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill={isPinned ? "currentColor" : "none"} stroke="currentColor" strokeWidth="2"><path d="M12 17v5" /><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z" /></svg>
          </button>
          {/* 更多菜单：收纳低频功能（导入 / 导出 / 设置） */}
          <div style={{ position: "relative" }}>
            <button className="btn btn-ghost btn-sm" style={{ padding: "6px", display: "flex", alignItems: "center" }} onClick={(e) => { e.stopPropagation(); setShowMoreMenu(p => !p); }} data-tooltip={t("tooltip.more")}>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><circle cx="5" cy="12" r="2" /><circle cx="12" cy="12" r="2" /><circle cx="19" cy="12" r="2" /></svg>
            </button>
            {showMoreMenu && (
              <div style={{ position: "absolute", right: 0, top: "calc(100% + 4px)", background: "var(--bg-secondary)", border: "1px solid var(--border-color)", borderRadius: 10, boxShadow: "var(--shadow-lg)", overflow: "hidden", minWidth: 190, zIndex: 100 }} onClick={e => e.stopPropagation()}>
                <div className="menu-item" onClick={() => { setShowMoreMenu(false); setShowSapImport(true); }}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="7 10 12 15 17 10" /><line x1="12" y1="15" x2="12" y2="3" /></svg>
                  {t("tooltip.import_sap")}
                </div>
                <div className="menu-item" onClick={() => { setShowMoreMenu(false); handleExport(); }}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="17 8 12 3 7 8" /><line x1="12" y1="3" x2="12" y2="15" /></svg>
                  {t("tooltip.export")}
                </div>
                <div className="menu-item" onClick={() => { setShowMoreMenu(false); setShowGroupManager(true); }}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" /></svg>
                  {t("group.manage")}
                </div>
                <div className="menu-item" onClick={() => { setShowMoreMenu(false); setShowSettings(true); }}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" /></svg>
                  {t("tooltip.settings")}
                </div>
              </div>
            )}
          </div>
          <button className="btn btn-ghost btn-sm" style={{ padding: "6px", display: "flex", alignItems: "center" }} onClick={() => { if (showAddModal || editingCred) { setShowLockConfirm(true); return; } clearSelection(); setMasterPassword(null); }} data-tooltip={t("tooltip.lock")}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="11" width="18" height="11" rx="2" /><path d="M7 11V7a5 5 0 0 1 10 0v4" /></svg>
          </button>
          <button className="btn btn-ghost btn-sm" style={{ padding: "5px", display: "flex", alignItems: "center" }} onClick={async () => { try { await getCurrentWindow().minimize(); } catch {} }} data-tooltip={t("tooltip.minimize")}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="5" y1="12" x2="19" y2="12" /></svg>
          </button>
          <button className="btn btn-ghost btn-sm" style={{ padding: "5px", display: "flex", alignItems: "center" }} onClick={async () => { try { await getCurrentWindow().close(); } catch {} }} data-tooltip={t("tooltip.close")}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
          </button>
        </div>
      </header>

      <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
        {/* 搜索栏 */}
        <div style={{ padding: "10px 12px 8px", flexShrink: 0 }}>
          <div style={{ position: "relative" }}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--text-muted)" strokeWidth="2" style={{ position: "absolute", left: 10, top: "50%", transform: "translateY(-50%)" }}><circle cx="11" cy="11" r="8" /><line x1="21" y1="21" x2="16.65" y2="16.65" /></svg>
            <input ref={searchInputRef} className="input" placeholder={t("main.search_placeholder")} value={search} onChange={e => setSearch(e.target.value)} onKeyDown={e => { if (e.key === "Escape") { setSearch(""); e.currentTarget.blur(); } }} style={{ width: "100%", fontSize: 12, paddingLeft: 32, paddingRight: search ? 28 : undefined, boxShadow: "var(--shadow-xs)" }} />
            {search && (
              <button type="button" onClick={() => setSearch("")} style={{ position: "absolute", right: 6, top: "50%", transform: "translateY(-50%)", background: "none", border: "none", cursor: "pointer", padding: 2, display: "flex", alignItems: "center", color: "var(--text-muted)" }} data-tooltip={t("card.clear_search")}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="10" /><line x1="15" y1="9" x2="9" y2="15" /><line x1="9" y1="9" x2="15" y2="15" /></svg>
              </button>
            )}
          </div>
        </div>
        {/* 分组选择器：下拉替代横向滚动 chip，分组再多也不溢出 */}
        <div style={{ padding: "8px 12px", display: "flex", alignItems: "center", gap: 6 }}>
          <div style={{ position: "relative", flex: 1, minWidth: 0 }} onClick={e => e.stopPropagation()}>
            {/* 下拉触发按钮：显示当前分组 + 计数 */}
            <button
              className="group-trigger"
              onClick={() => setShowGroupDropdown(p => !p)}
              data-active={selectedGroup !== null}
            >
              {selectedGroup === "favorites" ? (
                <svg width="13" height="13" viewBox="0 0 24 24" fill="#f1c21b" stroke="#f1c21b" strokeWidth="2" style={{ flexShrink: 0 }}><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" /></svg>
              ) : (
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ flexShrink: 0, opacity: 0.7 }}><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" /></svg>
              )}
              <span style={{ flex: 1, textAlign: "left", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontWeight: 700 }}>{currentGroupName}</span>
              <span className="gd-count">{selectedGroup === "favorites" ? favoriteCreds.length : displayCreds.length}</span>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ flexShrink: 0, opacity: 0.6, transform: showGroupDropdown ? "rotate(180deg)" : "none", transition: "transform .15s" }}><polyline points="6 9 12 15 18 9" /></svg>
            </button>

            {showGroupDropdown && (
              <div className="group-dropdown" onClick={e => e.stopPropagation()}>
                {/* 固定项：全部 / 收藏 */}
                <div className="gd-item" data-selected={selectedGroup === null} onClick={() => { setSelectedGroup(null); setShowGroupDropdown(false); }}>
                  <span className="gd-ico"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="3" y1="6" x2="21" y2="6" /><line x1="3" y1="12" x2="21" y2="12" /><line x1="3" y1="18" x2="21" y2="18" /></svg></span>
                  <span className="gd-name">{t("common.all")}</span>
                  <span className="gd-count">{credentials.length}</span>
                </div>
                {favoriteCreds.length > 0 && (
                  <div className="gd-item" data-selected={selectedGroup === "favorites"} onClick={() => { setSelectedGroup("favorites"); setShowGroupDropdown(false); }}>
                    <span className="gd-ico"><svg width="14" height="14" viewBox="0 0 24 24" fill="#f1c21b" stroke="#f1c21b" strokeWidth="2"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" /></svg></span>
                    <span className="gd-name">{t("common.favorites")}</span>
                    <span className="gd-count">{favoriteCreds.length}</span>
                  </div>
                )}
                {/* 系统 / 默认分组（可折叠） */}
                {groups.filter(g => g.is_system || g.is_default).length > 0 && (
                  <>
                    <div className="gd-section gd-section-fold" onClick={() => toggleSection("system")}>
                      <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                        <svg className="gd-fold-arrow" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" style={{ transform: collapsedSections.system ? "rotate(-90deg)" : "none" }}><polyline points="6 9 12 15 18 9" /></svg>
                        {t("group.section_system")}
                      </span>
                    </div>
                    {!collapsedSections.system && groups.filter(g => g.is_system || g.is_default).map(g => (
                      <div key={g.id} className="gd-item" data-selected={selectedGroup === g.id} onClick={() => { setSelectedGroup(g.id); setShowGroupDropdown(false); }}>
                        <span className="gd-ico"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" /></svg></span>
                        <span className="gd-name">{getGroupDisplayName(g)}</span>
                        <span className="gd-count">{getGroupCount(g)}</span>
                      </div>
                    ))}
                  </>
                )}
                {/* 自定义分组（可就地新建/删除，可折叠） */}
                <div className="gd-section gd-section-fold" onClick={() => toggleSection("custom")}>
                  <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                    <svg className="gd-fold-arrow" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" style={{ transform: collapsedSections.custom ? "rotate(-90deg)" : "none" }}><polyline points="6 9 12 15 18 9" /></svg>
                    {t("group.section_custom")}
                  </span>
                  <span className="gd-section-manage" onClick={(e) => { e.stopPropagation(); setShowGroupDropdown(false); setShowGroupManager(true); }}>{t("group.manage")}</span>
                </div>
                {!collapsedSections.custom && customGroups.map(g => {
                  const gPinned = pinnedGroups.has(g.id);
                  return (
                    <div key={g.id} className="gd-item" data-selected={selectedGroup === g.id}>
                      <span style={{ display: "flex", alignItems: "center", gap: 9, flex: 1, minWidth: 0 }} onClick={() => { setSelectedGroup(g.id); setShowGroupDropdown(false); }}>
                        <span className="gd-ico">{gPinned
                          ? <svg width="13" height="13" viewBox="0 0 24 24" fill="var(--accent)" stroke="var(--accent)" strokeWidth="2"><path d="M12 17v5" /><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z" /></svg>
                          : <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="4" /></svg>}</span>
                        <span className="gd-name">{getGroupDisplayName(g)}</span>
                        <span className="gd-count">{g.entries.length}</span>
                      </span>
                      <span className="gd-actions">
                        <span className="gd-act" title={gPinned ? t("card.unpin") : t("card.pin")} onClick={(e) => { e.stopPropagation(); togglePinGroup(g.id); }}>
                          <svg width="13" height="13" viewBox="0 0 24 24" fill={gPinned ? "var(--accent)" : "none"} stroke={gPinned ? "var(--accent)" : "currentColor"} strokeWidth="2"><line x1="12" y1="17" x2="12" y2="22" /><path d="M5 17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1v4.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V17z" /></svg>
                        </span>
                        <span className="gd-act" title={t("card.rename")} onClick={(e) => { e.stopPropagation(); setShowGroupDropdown(false); setRenameGroupTarget(g); }}>
                          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" /><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" /></svg>
                        </span>
                        <span className="gd-act gd-act-danger" title={t("card.delete")} onClick={(e) => { e.stopPropagation(); setShowGroupDropdown(false); handleDeleteGroup(g); }}>
                          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="3 6 5 6 21 6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /></svg>
                        </span>
                      </span>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
          {/* 选中数量提示 */}
          {hasSelection && (
            <span style={{ color: "var(--accent)", fontFamily: "var(--font-mono)", fontSize: 10, fontWeight: 700, flexShrink: 0 }}>{t("main.selected_count").replace("{count}", String(selectedCreds.size))}</span>
          )}
        </div>
          <div ref={listRef} className={settings?.compact_mode ? "compact-list" : undefined} style={{ flex: 1, overflowY: "auto", padding: "0 12px 14px" }} onClick={(e) => { if (e.target === e.currentTarget) clearSelection(); }}>
            {displayCreds.length === 0 ? (
              <div className="empty-state">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" style={{ width: 64, height: 64, marginBottom: 16, opacity: 0.5 }}><path d="M15.75 5.25a3 3 0 013 3m3 0a6 6 0 01-7.029 5.912c-.563-.097-1.159.026-1.563.43L10.5 17.25H8.25v2.25H6v2.25H2.25v-2.818c0-.597.237-1.17.659-1.591l6.499-6.499c.404-.404.527-1 .43-1.563A6 6 0 1121.75 8.25z" /></svg>
                {search ? (
                  <>
                    <p>{t("main.empty_no_match")}</p>
                    <button className="btn btn-ghost btn-sm" style={{ marginTop: 12 }} onClick={() => setSearch("")}>{t("main.empty_clear_search")}</button>
                  </>
                ) : credentials.length === 0 ? (
                  <>
                    <p>{t("main.no_credentials_hint")}</p>
                    <button className="btn btn-primary btn-sm" style={{ marginTop: 12, display: "inline-flex", alignItems: "center", gap: 5 }} onClick={() => { setEditingCred(undefined); setDuplicateCred(undefined); setShowAddModal(true); }}>
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" /></svg>
                      {t("main.empty_add")}
                    </button>
                  </>
                ) : (
                  <>
                    {/* 空分组引导：从 SAP 导入到此环境 */}
                    <p>{t("main.no_credentials")}</p>
                    <button className="btn btn-ghost btn-sm" style={{ marginTop: 12, display: "inline-flex", alignItems: "center", gap: 5 }} onClick={() => setShowSapImport(true)}>
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="7 10 12 15 17 10" /><line x1="12" y1="15" x2="12" y2="3" /></svg>
                      {t("main.empty_import")}
                    </button>
                  </>
                )}
              </div>
            ) : (
              <>
                {/* 环境色图例（首次使用折叠视图时显示，可关闭） */}
                {showEnvLegend && sections && sections.length > 1 && (
                  <div className="env-legend">
                    <span style={{ fontSize: 10.5, color: "var(--text-muted)", flexShrink: 0 }}>{t("main.env_legend")}</span>
                    <span className="legend-items">
                      {sections.filter(s => s.env).map(s => (
                        <span key={s.key} className="legend-item"><span className="es-dot" style={{ background: ENV_COLORS[s.env] || "#6b7280" }} />{t(ENV_LABEL_KEYS[s.env] as any)}</span>
                      ))}
                    </span>
                    <button type="button" className="legend-close" onClick={() => { setShowEnvLegend(false); try { localStorage.setItem("envLegendShown", "1"); } catch {} }} title={t("common.close")}>
                      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
                    </button>
                  </div>
                )}
                {/* 最近使用区（仅平铺模式 + 全部视图 + 无搜索时显示） */}
                {recentCreds.length > 0 && (
                  <div className="recent-section">
                    <div className="recent-title">
                      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="10" /><polyline points="12 6 12 12 16 14" /></svg>
                      {t("main.section_recent")}
                    </div>
                    <div className="recent-row">
                      {recentCreds.map(c => (
                        <button key={c.id} type="button" className="recent-chip" onClick={() => handleLogin(c.id)} data-tooltip={`${t("card.dblclick_login")} · ${c.client || ""} / ${c.username || ""}`} data-tooltip-pos="bottom">
                          <span className="es-dot" style={{ background: ENV_COLORS[c.environment || ""] || "#6b7280" }} />
                          <span style={{ fontWeight: 650, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{c.display_name || c.connection_id}</span>
                          {c.client && <span style={{ fontFamily: "var(--font-mono)", fontSize: 9, color: "var(--accent)", fontWeight: 700 }}>{c.client}</span>}
                        </button>
                      ))}
                    </div>
                  </div>
                )}
                {sections ? (
                  /* 分组折叠视图：按系统分组（环境）分区；分区头含计数/全选/一键登录 */
                  sections.map(sec => {
                    const isCollapsed = !!envCollapsed[sec.key];
                    const secLabel = sec.env ? t(ENV_LABEL_KEYS[sec.env] as any) : t("card.unclassified");
                    const secColor = ENV_COLORS[sec.env] || "#6b7280";
                    const allSelected = sec.creds.length > 0 && sec.creds.every(c => selectedCreds.has(c.id));
                    return (
                      <div key={sec.key} className="env-section">
                        <div className="env-section-header">
                          <button type="button" className="env-section-title" onClick={() => toggleEnvSection(sec.key)}>
                            <svg className="es-arrow" style={{ transform: isCollapsed ? "rotate(-90deg)" : "none" }} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"><polyline points="6 9 12 15 18 9" /></svg>
                            <span className="es-dot" style={{ background: secColor }} />
                            <span className="es-name">{secLabel}</span>
                            <span className="es-count">{sec.creds.length}</span>
                          </button>
                          <div className="es-actions" onClick={e => e.stopPropagation()}>
                            <button type="button" className="es-act" onClick={() => toggleSelectSection(sec.creds)} data-tooltip={allSelected ? t("main.select_section_cancel") : t("main.select_section")} data-tooltip-pos="top">
                              {allSelected
                                ? <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="3" width="18" height="18" rx="2" /><polyline points="8 12 11 15 16 9" /></svg>
                                : <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="3" width="18" height="18" rx="2" /><polyline points="9 12 15 12" /></svg>
                              }
                            </button>
                            <button type="button" className="es-act es-act-login" onClick={() => handleSectionLogin(sec.creds)} data-tooltip={t("main.login_section")} data-tooltip-pos="top">
                              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4" /><polyline points="10 17 15 12 10 7" /><line x1="15" y1="12" x2="3" y2="12" /></svg>
                            </button>
                          </div>
                        </div>
                        {!isCollapsed && (
                          <div style={{ display: "flex", flexDirection: "column", gap: 7, marginTop: 2 }}>
                            {sec.creds.map(c => renderCard(c))}
                          </div>
                        )}
                      </div>
                    );
                  })
                ) : (
                  /* 平铺视图（单分组降级或关闭折叠设置时） */
                  <div style={{ display: "flex", flexDirection: "column", gap: 7 }}>
                    {displayCreds.map(c => renderCard(c))}
                  </div>
                )}
              </>
            )}
          </div>
        </div>
        {/* 批量操作：底部浮动胶囊工具栏 */}
        {hasSelection && (
          <div className="batch-toolbar">
            <span className="batch-count">{t("main.selected_count").replace("{count}", String(selectedCreds.size))}</span>
            <span className="batch-divider" />
            <button className="batch-btn" onClick={handleSelectAll} data-tooltip={t("main.select_all")} data-tooltip-pos="top">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="3" width="18" height="18" rx="2" /><polyline points="8 12 11 15 16 9" /></svg>
            </button>
            <button className="batch-btn batch-btn-primary" onClick={() => handleBatchLogin()} data-tooltip={t("main.batch_login_shortcut")} data-tooltip-pos="top">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4" /><polyline points="10 17 15 12 10 7" /><line x1="15" y1="12" x2="3" y2="12" /></svg>
            </button>
            <button className="batch-btn" onClick={() => handleBatchFavorite(true)} data-tooltip={t("main.batch_favorite")} data-tooltip-pos="top">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" /></svg>
            </button>
            <button className="batch-btn" onClick={() => setShowBatchMove(true)} data-tooltip={t("main.batch_move")} data-tooltip-pos="top">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" /></svg>
            </button>
            <button className="batch-btn" onClick={handleBatchShare} data-tooltip={t("main.batch_share")} data-tooltip-pos="top">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8" /><polyline points="16 6 12 2 8 6" /><line x1="12" y1="2" x2="12" y2="15" /></svg>
            </button>
            <button className="batch-btn batch-btn-danger" onClick={handleBatchDelete} data-tooltip={t("main.batch_delete")} data-tooltip-pos="top">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="3 6 5 6 21 6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /></svg>
            </button>
            <span className="batch-divider" />
            <button className="batch-btn" onClick={clearSelection} data-tooltip={t("main.clear_selection")} data-tooltip-pos="top">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
            </button>
          </div>
        )}

        {/* 批量移动分组弹窗 */}
        {showBatchMove && (
          <div className="modal-overlay" onClick={() => setShowBatchMove(false)}>
            <div className="modal" style={{ maxWidth: 360 }} onClick={e => e.stopPropagation()}>
              <div className="modal-header" style={{ textAlign: "center" }}>{t("main.batch_move")}</div>
              <div className="modal-body">
                <p style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 12, textAlign: "center" }}>{t("main.move_to")}</p>
                <div style={{ display: "flex", flexDirection: "column", gap: 6, maxHeight: 300, overflowY: "auto" }}>
                  <button className="batch-move-item" onClick={() => handleBatchMove("")}>
                    <span style={{ flex: 1, textAlign: "left" }}>{t("credential.default_group")}</span>
                  </button>
                  {customGroups.map(g => (
                    <button key={g.id} className="batch-move-item" onClick={() => handleBatchMove(g.id)}>
                      <span style={{ flex: 1, textAlign: "left" }}>{getGroupDisplayName(g)}</span>
                      <span style={{ fontSize: 10, color: "var(--text-muted)" }}>{g.entries.length}</span>
                    </button>
                  ))}
                </div>
              </div>
              <div className="modal-footer" style={{ justifyContent: "flex-end" }}>
                <button className="btn btn-secondary" onClick={() => setShowBatchMove(false)}>{t("common.cancel")}</button>
              </div>
            </div>
          </div>
        )}

      {(showAddModal || editingCred) && <CredentialModal credential={editingCred || duplicateCred} groups={groups} sapConnections={sapConnections} clipboardClearSeconds={settings?.clipboard_clear_seconds ?? 20} onSave={handleSaveCred} onDuplicate={editingCred ? handleDuplicateCred : undefined} onClose={() => { setShowAddModal(false); setEditingCred(undefined); setDuplicateCred(undefined); }} />}
      {showSapImport && <SapImportModal connections={sapConnections} existingCredentials={credentials} onImport={handleSapImport} onClose={() => setShowSapImport(false)} />}
      {showCreateGroup && <CreateGroupModal onCreate={handleCreateGroup} onClose={() => setShowCreateGroup(false)} />}
      {showGroupManager && (
        <GroupManagerModal
          groups={customGroups}
          counts={Object.fromEntries(customGroups.map(g => [g.id, getGroupCount(g)]))}
          pinned={pinnedGroups}
          onCreate={async (name) => { await handleCreateGroup(name); }}
          onRename={(g, name) => { handleRenameGroup(g, name); }}
          onDelete={(g) => { setShowGroupManager(false); handleDeleteGroup(g); }}
          onTogglePin={(id) => togglePinGroup(id)}
          onClose={() => setShowGroupManager(false)}
        />
      )}
      {renameGroupTarget && <RenameGroupModal group={renameGroupTarget} onRename={(name) => { handleRenameGroup(renameGroupTarget, name); setRenameGroupTarget(null); }} onClose={() => setRenameGroupTarget(null)} />}
      {showSettings && settings && <SettingsModal settings={settings} groups={groups} onSave={handleSaveSettings} onChangePassword={() => { setShowSettings(false); setShowChangePassword(true); }} onClose={() => setShowSettings(false)} />}

      {revealCred && <RevealPasswordModal cred={revealCred} clipboardClearSeconds={settings?.clipboard_clear_seconds ?? 20} onToast={addToast} onClose={() => setRevealCred(null)} />}

      {showBatchDeleteConfirm && (
        <div className="modal-overlay" onClick={() => setShowBatchDeleteConfirm(false)}>
          <div className="modal" onClick={e => e.stopPropagation()}>
            <div className="modal-header">{t("common.confirm_delete")}</div>
            <div className="modal-body" style={{ minHeight: 0 }}>
              <p style={{ fontSize: 13, color: "var(--text-secondary)", lineHeight: 1.5 }}>
                {t("confirm.batch_delete").replace("{count}", String(selectedCreds.size))}
              </p>
            </div>
            <div className="modal-footer">
              <button className="btn btn-secondary btn-sm" onClick={() => setShowBatchDeleteConfirm(false)}>{t("common.cancel")}</button>
              <button className="btn btn-danger btn-sm" onClick={confirmBatchDelete}>{t("common.confirm_delete")}</button>
            </div>
          </div>
        </div>
      )}

      {showExportVerify && (
        <div className="modal-overlay" onClick={() => { setShowExportVerify(false); setExportSavePath(null); setExportPw(""); setExportErr(""); }}>
          <div className="modal" onClick={e => e.stopPropagation()}>
            <div className="modal-header">{t("export_verify.title")}</div>
            <div className="modal-body" style={{ minHeight: 0 }}>
              <p style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 12, lineHeight: 1.5 }}>{t("export_verify.hint")}</p>
              <div className="form-group" style={{ marginBottom: 0 }}>
                <input className="input" type="password" placeholder={t("unlock.password")} value={exportPw} onChange={e => setExportPw(e.target.value)} onKeyDown={e => e.key === "Enter" && confirmExport()} autoFocus style={{ textAlign: "center", padding: "10px 14px" }} />
              </div>
              {exportErr && <p style={{ color: "var(--danger)", fontSize: 12, marginBottom: 0, marginTop: 10, fontFamily: "var(--font-body)", fontWeight: 600 }}>{exportErr}</p>}
            </div>
            <div className="modal-footer">
              <button className="btn btn-secondary btn-sm" onClick={() => { setShowExportVerify(false); setExportSavePath(null); setExportPw(""); setExportErr(""); }}>{t("common.cancel")}</button>
              <button className="btn btn-primary btn-sm" onClick={confirmExport}>{t("common.confirm")}</button>
            </div>
          </div>
        </div>
      )}

      {showChangePassword && (
        <div className="modal-overlay" onClick={() => { setShowChangePassword(false); setCpOld(""); setCpNew(""); setCpConfirm(""); setCpErr(""); }}>
          <div className="modal" onClick={e => e.stopPropagation()} style={{ width: 380 }}>
            <div className="modal-header" style={{ textAlign: "center" }}>{t("changepw.title")}</div>
            <div className="modal-body">
              <div className="form-group">
                <label className="label">{t("changepw.old_password")}</label>
                <input className="input" type="password" value={cpOld} onChange={e => setCpOld(e.target.value)} autoFocus style={{ padding: "10px 14px" }} />
              </div>
              <div className="form-group">
                <label className="label">{t("changepw.new_password")}</label>
                <input className="input" type="password" value={cpNew} onChange={e => setCpNew(e.target.value)} style={{ padding: "10px 14px" }} />
              </div>
              <div className="form-group">
                <label className="label">{t("changepw.confirm_password")}</label>
                <input className="input" type="password" value={cpConfirm} onChange={e => setCpConfirm(e.target.value)} onKeyDown={e => e.key === "Enter" && handleChangePassword()} style={{ padding: "10px 14px" }} />
              </div>
              {cpErr && <p style={{ color: "var(--danger)", fontSize: 12, marginBottom: 12, fontFamily: "var(--font-body)", fontWeight: 600 }}>{cpErr}</p>}
            </div>
            <div className="modal-footer" style={{ justifyContent: "flex-end" }}>
              <div className="modal-actions">
                <button className="btn btn-secondary" onClick={() => { setShowChangePassword(false); setCpOld(""); setCpNew(""); setCpConfirm(""); setCpErr(""); }}>{t("common.cancel")}</button>
                <button className="btn btn-primary" onClick={handleChangePassword}>{t("common.confirm")}</button>
              </div>
            </div>
          </div>
        </div>
      )}

      {deleteGroupTarget && (
        <div className="modal-overlay" onClick={() => setDeleteGroupTarget(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()} style={{ width: 380 }}>
            <div className="modal-header" style={{ textAlign: "center" }}>{t("group.delete")}</div>
            <div className="modal-body">
              <div style={{ textAlign: "center", marginBottom: 16 }}>
                <div style={{ width: 48, height: 48, margin: "0 auto 12px", borderRadius: "50%", background: "var(--danger)", display: "flex", alignItems: "center", justifyContent: "center" }}>
                  <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="2"><path d="M3 6h18" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /><line x1="10" y1="11" x2="10" y2="17" /><line x1="14" y1="11" x2="14" y2="17" /></svg>
                </div>
                <p style={{ fontSize: 13, color: "var(--text-secondary)" }}>
                  {t("group.delete_confirm_name").replace("{name}", getGroupDisplayName(deleteGroupTarget))}
                </p>
                {deleteGroupTarget.entries.length > 0 && (
                  <p style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 4 }}>
                    {t("group.contains_count").replace("{count}", String(deleteGroupTarget.entries.length))}
                  </p>
                )}
              </div>
              {deleteGroupTarget.entries.length > 0 && (
                <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                  <label style={{ display: "flex", alignItems: "flex-start", gap: 8, padding: 10, borderRadius: 8, border: "1px solid var(--border-color)", cursor: "pointer", transition: "all .15s" }} onMouseEnter={e => e.currentTarget.style.borderColor = "var(--accent)"} onMouseLeave={e => e.currentTarget.style.borderColor = "var(--border-color)"}>
                    <input type="radio" name="deleteMode" defaultChecked onChange={() => setDeleteGroupMode("move")} style={{ marginTop: 2 }} />
                    <div>
                      <div style={{ fontSize: 12, fontWeight: 600, color: "var(--text-primary)" }}>{t("group.move_to_default")}</div>
                      <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 2 }}>{t("group.move_to_default_hint")}</div>
                    </div>
                  </label>
                  <label style={{ display: "flex", alignItems: "flex-start", gap: 8, padding: 10, borderRadius: 8, border: "1px solid var(--border-color)", cursor: "pointer", transition: "all .15s" }} onMouseEnter={e => e.currentTarget.style.borderColor = "var(--danger)"} onMouseLeave={e => e.currentTarget.style.borderColor = "var(--border-color)"}>
                    <input type="radio" name="deleteMode" onChange={() => setDeleteGroupMode("delete")} style={{ marginTop: 2 }} />
                    <div>
                      <div style={{ fontSize: 12, fontWeight: 600, color: "var(--danger)" }}>{t("group.delete_all_creds")}</div>
                      <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 2 }}>{t("group.delete_all_creds_hint").replace("{count}", String(deleteGroupTarget.entries.length))}</div>
                    </div>
                  </label>
                </div>
              )}
            </div>
            <div className="modal-footer" style={{ justifyContent: "flex-end" }}>
              <div className="modal-actions">
                <button className="btn btn-secondary" onClick={() => setDeleteGroupTarget(null)}>{t("common.cancel")}</button>
                <button className="btn btn-danger" onClick={() => handleConfirmDeleteGroup(deleteGroupMode)}>{t("common.confirm_delete")}</button>
              </div>
            </div>
          </div>
        </div>
      )}

      <div className="toast-container">{toasts.map(t => <Toast key={t.id} message={t.message} type={t.type} actionLabel={t.actionLabel} onAction={t.onAction} duration={t.duration} onClose={() => removeToast(t.id)} />)}</div>

      {/* 右键菜单：卡片快捷操作 */}
      {ctxMenu && (
        <>
          <div className="ctx-backdrop" onClick={e => { e.stopPropagation(); setCtxMenu(null); }} onContextMenu={e => { e.preventDefault(); e.stopPropagation(); setCtxMenu(null); }} />
          <div className="ctx-menu" style={{ left: Math.min(ctxMenu.x, (typeof window !== "undefined" ? window.innerWidth : 440) - 200), top: Math.min(ctxMenu.y, (typeof window !== "undefined" ? window.innerHeight : 956) - 340) }} onClick={e => e.stopPropagation()}>
            <button className="ctx-item" onClick={() => { const cred = ctxMenu.cred; setCtxMenu(null); handleLogin(cred.id); }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4" /><polyline points="10 17 15 12 10 7" /><line x1="15" y1="12" x2="3" y2="12" /></svg>
              {t("card.login")}
            </button>
            <button className="ctx-item" onClick={() => { const cred = ctxMenu.cred; setCtxMenu(null); setEditingCred(cred); setDuplicateCred(undefined); setShowAddModal(true); }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" /><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" /></svg>
              {t("card.edit")}
            </button>
            <button className="ctx-item" onClick={async () => { const cred = ctxMenu.cred; setCtxMenu(null); if (!cred.username) { addToast(t("card.missing_prefix") + t("credential.username"), "info"); return; } const ok = await copyText(cred.username); if (ok) addToast(t("toast.username_copied"), "success"); }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="9" y="9" width="13" height="13" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg>
              {t("card.copy_username")}
            </button>
            <button className="ctx-item" onClick={async () => { const cred = ctxMenu.cred; setCtxMenu(null); try { const pw = await invoke<string>("get_decrypted_password", { masterPassword, credentialId: cred.id }); if (!pw) { addToast(t("toast.no_password"), "info"); return; } const ok = await copyText(pw); if (ok) addToast(t("toast.password_copied"), "success"); } catch (e) { addToast(t("toast.copy_failed") + ": " + String(e), "error"); } }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4" /></svg>
              {t("card.copy_password")}
            </button>
            <button className="ctx-item" onClick={() => { const cred = ctxMenu.cred; setCtxMenu(null); setRevealCred(cred); }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M1 12s4-7 11-7 11 7 11 7-4 7-11 7-11-7-11-7z" /><circle cx="12" cy="12" r="3" /></svg>
              {t("card.reveal_password")}
            </button>
            <button className="ctx-item" onClick={() => { const cred = ctxMenu.cred; setCtxMenu(null); handleToggleFavorite(cred.id); }}>
              {ctxMenu.cred.is_favorite
                ? <svg width="14" height="14" viewBox="0 0 24 24" fill="#f1c21b" stroke="#f1c21b" strokeWidth="2"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" /></svg>
                : <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" /></svg>
              }
              {ctxMenu.cred.is_favorite ? t("card.unfavorite") : t("card.favorite")}
            </button>
            <button className="ctx-item" onClick={() => { const cred = ctxMenu.cred; setCtxMenu(null); togglePinCred(cred.id); }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="12" y1="17" x2="12" y2="22" /><path d="M5 17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1v4.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V17z" /></svg>
              {pinnedCreds.has(ctxMenu.cred.id) ? t("card.unpin") : t("card.pin")}
            </button>
            <button className="ctx-item" onClick={() => { const cred = ctxMenu.cred; setCtxMenu(null); handleDuplicateCred(cred); }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="9" y="9" width="13" height="13" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg>
              {t("card.duplicate")}
            </button>
            <div className="ctx-divider" />
            <button className="ctx-item ctx-item-danger" onClick={() => { const cred = ctxMenu.cred; setCtxMenu(null); deleteCredsWithUndo([cred.id]); }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="3 6 5 6 21 6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /></svg>
              {t("card.delete")}
            </button>
          </div>
        </>
      )}

      {/* 锁定确认：有未保存编辑时 */}
      {showLockConfirm && (
        <div className="modal-overlay" onClick={() => setShowLockConfirm(false)}>
          <div className="modal" onClick={e => e.stopPropagation()}>
            <div className="modal-header">{t("confirm.lock_title")}</div>
            <div className="modal-body" style={{ minHeight: 0 }}>
              <p style={{ fontSize: 13, color: "var(--text-secondary)", lineHeight: 1.5 }}>{t("confirm.lock_editing")}</p>
            </div>
            <div className="modal-footer">
              <button className="btn btn-secondary btn-sm" onClick={() => setShowLockConfirm(false)}>{t("common.cancel")}</button>
              <button className="btn btn-danger btn-sm" onClick={() => { setShowLockConfirm(false); clearSelection(); setMasterPassword(null); }}>{t("confirm.lock_confirm")}</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
