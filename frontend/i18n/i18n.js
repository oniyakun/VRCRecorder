// VRCRecorder — i18n engine
// Must be loaded AFTER en.js, zh-CN.js, ja.js and BEFORE main.js
(function() {
    // Assemble all loaded language packs
    window.__I18N__ = {
        'en': window.__I18N_EN__ || {},
        'zh-CN': window.__I18N_ZH_CN__ || {},
        'ja': window.__I18N_JA__ || {}
    };

    // Resolve dotted key like "sidebar.source" into the translation object
    function resolve(obj, path) {
        var parts = path.split('.');
        for (var i = 0; obj && i < parts.length; i++) {
            obj = obj[parts[i]];
        }
        return obj;
    }

    // Public translation function
    // t('sidebar.source') -> "Source" (in English)
    // t('status.connected', {name: 'Spout', width: 1920, height: 1080}) -> "Connected: Spout 1920x1080"
    window.t = function(key, vars) {
        var lang = window.__CURRENT_LANG__ || 'en';
        var dict = window.__I18N__[lang] || window.__I18N__['en'];
        var str = resolve(dict, key);
        if (str === undefined || str === null) {
            // Fallback to English
            str = resolve(window.__I18N__['en'], key) || key;
        }
        if (typeof str === 'string' && vars) {
            for (var k in vars) {
                if (Object.prototype.hasOwnProperty.call(vars, k)) {
                    str = str.replace('{' + k + '}', vars[k]);
                }
            }
        }
        return str;
    };

    // Apply data-i18n bindings to the entire DOM or a container
    // Scans for: [data-i18n] (textContent), [data-i18n-placeholder] (placeholder),
    //            [data-i18n-title] (title attribute)
    window.applyI18n = function(root) {
        root = root || document;

        // textContent
        var els = root.querySelectorAll('[data-i18n]');
        for (var i = 0; i < els.length; i++) {
            var key = els[i].getAttribute('data-i18n');
            els[i].textContent = window.t(key);
        }

        // placeholder
        var ph = root.querySelectorAll('[data-i18n-placeholder]');
        for (var j = 0; j < ph.length; j++) {
            var pk = ph[j].getAttribute('data-i18n-placeholder');
            ph[j].placeholder = window.t(pk);
        }

        // title attribute
        var tl = root.querySelectorAll('[data-i18n-title]');
        for (var k = 0; k < tl.length; k++) {
            var tk = tl[k].getAttribute('data-i18n-title');
            tl[k].title = window.t(tk);
        }
    };

    // Detect system language from browser/webview
    // Returns 'en', 'zh-CN', or 'ja'
    window.detectLanguage = function() {
        var lang = (navigator.language || navigator.userLanguage || 'en').toLowerCase();
        if (lang.startsWith('zh')) return 'zh-CN';
        if (lang.startsWith('ja')) return 'ja';
        return 'en';
    };

    // Set active language and re-apply all translations
    window.setLanguage = function(lang) {
        window.__CURRENT_LANG__ = lang;
        document.documentElement.lang = lang;
        document.title = window.t('app.title');
        window.applyI18n(document);
    };
})();
