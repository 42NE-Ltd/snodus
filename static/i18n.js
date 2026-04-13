/**
 * Snodus i18n — lightweight client-side internationalisation.
 *
 * Usage:
 *   <span data-i18n="landing.hero_headline"></span>
 *   <input data-i18n-placeholder="common.search">
 *   <img data-i18n-alt="landing.hero_alt">
 *
 * The loader fetches /i18n/{lang}.json, caches it in memory, and
 * re-renders every element with a data-i18n* attribute. Language
 * preference is persisted in localStorage.
 */

const i18n = (() => {
  const STORAGE_KEY = 'snodus-lang';
  const FALLBACK = 'en';
  const SUPPORTED = [
    'en','zh','hi','es','ar','fr','bn','pt','ru','id','de','ja','ko','it'
  ];

  let locale = FALLBACK;
  let strings = {};
  const cache = {};

  function detect() {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored && SUPPORTED.includes(stored)) return stored;
    const nav = (navigator.language || '').split('-')[0];
    return SUPPORTED.includes(nav) ? nav : FALLBACK;
  }

  function resolve(key) {
    return key.split('.').reduce((o, k) => (o && o[k] !== undefined ? o[k] : null), strings);
  }

  function t(key, params) {
    let str = resolve(key);
    if (str === null) return key;
    if (params) {
      Object.keys(params).forEach(k => {
        str = str.replace(new RegExp('\\{' + k + '\\}', 'g'), params[k]);
      });
    }
    return str;
  }

  function render() {
    document.querySelectorAll('[data-i18n]').forEach(el => {
      const val = resolve(el.dataset.i18n);
      if (val !== null) el.textContent = val;
    });
    document.querySelectorAll('[data-i18n-html]').forEach(el => {
      const val = resolve(el.dataset.i18nHtml);
      if (val !== null) el.innerHTML = val;
    });
    document.querySelectorAll('[data-i18n-placeholder]').forEach(el => {
      const val = resolve(el.dataset.i18nPlaceholder);
      if (val !== null) el.placeholder = val;
    });
    document.querySelectorAll('[data-i18n-alt]').forEach(el => {
      const val = resolve(el.dataset.i18nAlt);
      if (val !== null) el.alt = val;
    });
    document.querySelectorAll('[data-i18n-title]').forEach(el => {
      const val = resolve(el.dataset.i18nTitle);
      if (val !== null) el.title = val;
    });
    // RTL for Arabic
    document.documentElement.dir = locale === 'ar' ? 'rtl' : 'ltr';
    document.documentElement.lang = locale;

    // Update language switcher if present
    const switcher = document.getElementById('lang-switcher');
    if (switcher) switcher.value = locale;
  }

  async function load(lang) {
    if (!SUPPORTED.includes(lang)) lang = FALLBACK;
    if (cache[lang]) {
      strings = cache[lang];
      locale = lang;
      localStorage.setItem(STORAGE_KEY, lang);
      render();
      return;
    }
    try {
      const res = await fetch('/i18n/' + lang + '.json');
      if (!res.ok) throw new Error(res.status);
      strings = await res.json();
      cache[lang] = strings;
      locale = lang;
      localStorage.setItem(STORAGE_KEY, lang);
      render();
    } catch (e) {
      console.warn('i18n: failed to load ' + lang + ', falling back to ' + FALLBACK);
      if (lang !== FALLBACK) return load(FALLBACK);
    }
  }

  async function init() {
    locale = detect();
    await load(locale);
  }

  return { init, load, t, render, get locale() { return locale; }, SUPPORTED };
})();

// Auto-init when DOM is ready
if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', () => i18n.init());
} else {
  i18n.init();
}
