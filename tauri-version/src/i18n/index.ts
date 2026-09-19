// i18n - 多语言支持（全局订阅式，切换语言时所有组件同步刷新）
import { useState, useEffect, useCallback } from "react";
import { zh, type TranslationKey } from "./zh";
import { en } from "./en";
import { ja } from "./ja";

export type Language = "zh" | "en" | "ja";

const translations: Record<Language, Record<TranslationKey, string>> = { zh, en, ja };

// 检测系统语言，非支持语言则默认英语
function detectSystemLanguage(): Language {
  const sysLang = (navigator.language || "en").toLowerCase();
  if (sysLang.startsWith("zh")) return "zh";
  if (sysLang.startsWith("ja")) return "ja";
  return "en";
}

// 全局状态 + 订阅列表
let globalLang: Language = detectSystemLanguage();
const listeners = new Set<() => void>();

export function getLanguage(): Language {
  return globalLang;
}

export function setLanguage(lang: Language) {
  if (lang === globalLang) return;
  globalLang = lang;
  listeners.forEach(fn => fn());
}

// 翻译函数（可在组件外使用）
export function t(key: TranslationKey): string {
  return translations[globalLang]?.[key] ?? translations.zh[key] ?? key;
}

// React hook —— 订阅全局语言变化，切换时自动触发重渲染
export function useI18n() {
  const [, forceUpdate] = useState({});

  useEffect(() => {
    const listener = () => forceUpdate({});
    listeners.add(listener);
    return () => { listeners.delete(listener); };
  }, []);

  const translate = useCallback((key: TranslationKey): string => {
    return translations[globalLang]?.[key] ?? translations.zh[key] ?? key;
  }, []);

  const changeLang = useCallback((newLang: Language) => {
    setLanguage(newLang);
  }, []);

  return { lang: globalLang, t: translate, changeLang };
}

// 语言选项
export const LANGUAGE_OPTIONS: { value: Language; label: string }[] = [
  { value: "zh", label: "中文" },
  { value: "en", label: "English" },
  { value: "ja", label: "日本語" },
];
