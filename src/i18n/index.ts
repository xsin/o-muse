import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";
import en from "./locales/en.json";
import zh from "./locales/zh.json";

// ADR-0004：中英双语，默认随系统，持久化键 ui_lang。
i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      zh: { translation: zh },
    },
    fallbackLng: "en",
    supportedLngs: ["en", "zh"],
    interpolation: { escapeValue: false },
    detection: {
      order: ["localStorage", "navigator"],
      lookupLocalStorage: "ui_lang",
      caches: ["localStorage"],
    },
  });

// 同步 <html lang>：影响屏幕阅读器发音与浏览器拼写/翻译行为（review 修复）。
function syncHtmlLang() {
  const lang = i18n.resolvedLanguage ?? "en";
  document.documentElement.lang = lang === "zh" ? "zh-CN" : lang;
}
syncHtmlLang();
i18n.on("languageChanged", syncHtmlLang);

export default i18n;
