// 轻量 i18n：zustand store + 字典，无需引入 i18next
import { create } from "zustand";
import { zh } from "./zh";
import { en } from "./en";
import { ja } from "./ja";

export type Lang = "zh" | "en" | "ja";
export type Dict = Record<string, string>;

const LANG_KEY = "llmux-lang";

function getInitialLang(): Lang {
  const saved = localStorage.getItem(LANG_KEY);
  if (saved === "zh" || saved === "en" || saved === "ja") return saved;
  const lang = navigator.language.toLowerCase();
  if (lang.startsWith("zh")) return "zh";
  if (lang.startsWith("ja")) return "ja";
  return "en";
}

interface I18nState {
  lang: Lang;
  setLang: (lang: Lang) => void;
  /** 取文案；{key} 占位符用 params 替换；缺失回退英文，再缺失原样返回 key */
  t: (key: string, params?: Record<string, string | number>) => string;
}

export const useI18n = create<I18nState>((set, get) => ({
  lang: getInitialLang(),
  setLang: (lang) => {
    localStorage.setItem(LANG_KEY, lang);
    document.documentElement.lang = lang;
    set({ lang });
  },
  t: (key, params) => {
    const dicts: Record<Lang, Dict> = { zh, en, ja };
    const dict: Dict = dicts[get().lang] ?? en;
    let text = dict[key] ?? en[key] ?? key;
    if (params) {
      for (const [k, v] of Object.entries(params)) {
        text = text.split(`{${k}}`).join(String(v));
      }
    }
    return text;
  },
}));

/** 便捷 hook：返回 t 函数 + 当前语言 + 切换 */
export function useT() {
  const lang = useI18n((s) => s.lang);
  const setLang = useI18n((s) => s.setLang);
  const t = useI18n((s) => s.t);
  return { t, lang, setLang };
}
