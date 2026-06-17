// panels.js - Right-side settings tabs for Fluid Lab.
// Pure ES module, vanilla DOM, no dependencies, no build step.
//
// NOTE on reset/reload-class settings: calling app.set_setting() updates the stored
// value in the Rust side, but the running simulation only picks up "reset"-class
// changes after a successful app.reset() and "reload"-class changes after a full
// page reload.
// This module flags those rows with a badge so the user knows action is needed.
// Live-class settings apply to the running sim immediately and need no badge.

const LS_KEY = "fluidlab.config.v1";
const THEME_STORAGE_KEY = "fluidlab.theme.v1";
const HIDDEN_SETTING_IDS = new Set([
  "interaction.auto_roll_enabled",
  "interaction.wave_enabled",
]);
const DEV_ONLY_TABS = new Set(["environment", "theme"]);
const DEFAULT_TAB = "scenario";
const PROFILER_TAB = { id: "profiler", label: "Profiler", order: 1000, profiler: true };
const THEME_TAB = { id: "theme", label: "Theme", order: 1100, shell: true };
const THEME_SWATCH_TOKENS = [
  ["--app-bg", "Page"],
  ["--text-body", "Text"],
  ["--accent", "Accent"],
  ["--button-bg", "Action"],
  ["--control-bg", "Control"],
  ["--panel-border", "Border"],
];
const THEME_TOKENS = {
  default: {
    "--app-bg": "#0a0c10",
    "--text-body": "#cdd6e4",
    "--accent": "#7dd3fc",
    "--button-bg": "#1b2130",
    "--control-bg": "#0f1219",
    "--panel-border": "#2a3142",
  },
  harbor: {
    "--app-bg": "#081013",
    "--text-body": "#c8dde2",
    "--accent": "#5eead4",
    "--button-bg": "#0f3740",
    "--control-bg": "#0b1a1f",
    "--panel-border": "#24545c",
  },
  signal: {
    "--app-bg": "#101014",
    "--text-body": "#e5dde1",
    "--accent": "#22d3ee",
    "--button-bg": "#b45309",
    "--control-bg": "#17171d",
    "--panel-border": "#4a3c30",
  },
  void: {
    "--app-bg": "#000000",
    "--text-body": "#d6dae2",
    "--accent": "#ffffff",
    "--button-bg": "#111111",
    "--control-bg": "#050505",
    "--panel-border": "#303030",
  },
  basalt: {
    "--app-bg": "#121212",
    "--text-body": "#d7d7d7",
    "--accent": "#e5e7eb",
    "--button-bg": "#2a2a2a",
    "--control-bg": "#191919",
    "--panel-border": "#3a3a3a",
  },
  ember: {
    "--app-bg": "#130b0c",
    "--text-body": "#f0d9cf",
    "--accent": "#fb923c",
    "--button-bg": "#7c2d12",
    "--control-bg": "#221312",
    "--panel-border": "#7f3f2f",
  },
  orchid: {
    "--app-bg": "#120d1f",
    "--text-body": "#eadcff",
    "--accent": "#f0abfc",
    "--button-bg": "#5b21b6",
    "--control-bg": "#1f1730",
    "--panel-border": "#6d4a9c",
  },
  circuit: {
    "--app-bg": "#03120d",
    "--text-body": "#c8f7df",
    "--accent": "#22c55e",
    "--button-bg": "#064e3b",
    "--control-bg": "#071b13",
    "--panel-border": "#1f7a4d",
  },
  glacier: {
    "--app-bg": "#dceaf3",
    "--text-body": "#1f3342",
    "--accent": "#0f6f9f",
    "--button-bg": "#c9dde8",
    "--control-bg": "#edf5f9",
    "--panel-border": "#88a9bb",
  },
  lagoon: {
    "--app-bg": "#07161b",
    "--text-body": "#d7f3f0",
    "--accent": "#2dd4bf",
    "--button-bg": "#14532d",
    "--control-bg": "#0d2428",
    "--panel-border": "#2b6d72",
  },
  eclipse: {
    "--app-bg": "#0f1022",
    "--text-body": "#e3e7ff",
    "--accent": "#a78bfa",
    "--button-bg": "#312e81",
    "--control-bg": "#181935",
    "--panel-border": "#4f46e5",
  },
  coral: {
    "--app-bg": "#171016",
    "--text-body": "#ffe1dc",
    "--accent": "#fb7185",
    "--button-bg": "#9f1239",
    "--control-bg": "#26151b",
    "--panel-border": "#be4560",
  },
};
const THEMES = [
  { id: "default", label: "Default", tokens: THEME_TOKENS.default },
  { id: "harbor", label: "Harbor", tokens: THEME_TOKENS.harbor },
  { id: "signal", label: "Signal", tokens: THEME_TOKENS.signal },
  { id: "void", label: "Void", tokens: THEME_TOKENS.void },
  { id: "basalt", label: "Basalt", tokens: THEME_TOKENS.basalt },
  { id: "ember", label: "Ember", tokens: THEME_TOKENS.ember },
  { id: "orchid", label: "Orchid", tokens: THEME_TOKENS.orchid },
  { id: "circuit", label: "Circuit", tokens: THEME_TOKENS.circuit },
  { id: "glacier", label: "Glacier", tokens: THEME_TOKENS.glacier },
  { id: "lagoon", label: "Lagoon", tokens: THEME_TOKENS.lagoon },
  { id: "eclipse", label: "Eclipse", tokens: THEME_TOKENS.eclipse },
  { id: "coral", label: "Coral", tokens: THEME_TOKENS.coral },
];
const TAB_ALIASES = {
  general: "scenario",
  modes: "scenario",
  render: "camera",
  "camera-view": "camera",
  water: "surface",
  "water-surface": "surface",
  "water-color": "color",
  "sun-reflection": "reflection",
  physics: "simulation",
};
function isDevMode() {
  return new URLSearchParams(window.location.search).get("dev") === "true";
}

function normalizeThemeId(id) {
  return THEMES.some((theme) => theme.id === id) ? id : "default";
}

function applyTheme(id, persist = false) {
  const themeId = normalizeThemeId(id);
  if (themeId === "default") {
    document.documentElement.removeAttribute("data-theme");
  } else {
    document.documentElement.dataset.theme = themeId;
  }
  if (persist) {
    try {
      localStorage.setItem(THEME_STORAGE_KEY, themeId);
    } catch (e) {
      console.warn("[panels] theme localStorage write failed:", e);
    }
  }
  return themeId;
}

function loadStoredTheme() {
  try {
    return normalizeThemeId(localStorage.getItem(THEME_STORAGE_KEY) || "default");
  } catch {
    return "default";
  }
}

function activeThemeId() {
  return normalizeThemeId(document.documentElement.dataset.theme || "default");
}

function deriveTabs(settings) {
  const byId = new Map();
  const devMode = isDevMode();
  for (const s of settings) {
    if (HIDDEN_SETTING_IDS.has(s.id)) continue;
    if (!s.tab || !s.tab_label) continue;
    if (DEV_ONLY_TABS.has(s.tab) && !devMode) continue;
    if (!byId.has(s.tab)) {
      byId.set(s.tab, {
        id: s.tab,
        label: s.tab_label,
        order: typeof s.tab_order === "number" ? s.tab_order : 500,
        group: s.tab_group || "Settings",
        variant: s.tab_variant || "normal",
      });
    }
  }
  const tabs = [...byId.values()].sort((a, b) => a.order - b.order).concat(PROFILER_TAB);
  if (devMode) tabs.push(THEME_TAB);
  return tabs;
}

function safeConfigJson(app) {
  try {
    return JSON.parse(app.config_json());
  } catch (e) {
    console.warn("[panels] config_json parse error:", e);
    return [];
  }
}

function safeStatsJson(app) {
  try {
    return JSON.parse(app.stats_json());
  } catch (e) {
    console.warn("[panels] stats_json parse error:", e);
    return null;
  }
}

function setSetting(app, id, value) {
  if (typeof app.set_setting_result_json === "function") {
    try {
      return JSON.parse(app.set_setting_result_json(id, value));
    } catch (e) {
      console.warn("[panels] set_setting_result_json parse error:", id, e);
      return { ok: false, status: "bridge_result_parse_error", id, requested_id: id };
    }
  }

  const live = app.set_setting(id, value);
  return {
    ok: live,
    status: live ? "applied" : "stored_or_rejected",
    id,
    requested_id: id,
    requested_value: value,
    stored_value: value,
    clamped: false,
    apply: live ? "live" : null,
    applied_live: live,
    needs_reset: false,
    needs_reload: false,
  };
}

function applySettingEntries(app, entries, source = "settings") {
  let needsReset = false;
  let needsReload = false;
  let applied = 0;
  let rejected = 0;
  let clamped = 0;
  let resetApplied = false;
  let resetRejected = false;
  const results = [];

  for (const [id, value] of entries) {
    const numericValue = typeof value === "number" ? value : Number(value);
    const result = setSetting(app, id, numericValue);
    results.push(result);
    if (result.ok) {
      applied += 1;
      needsReset = needsReset || !!result.needs_reset;
      needsReload = needsReload || !!result.needs_reload;
      clamped += result.clamped ? 1 : 0;
    } else {
      rejected += 1;
      console.warn(`[panels] ${source} setting rejected`, id, result.status);
    }
  }

  if (needsReset) {
    resetApplied = !!app.reset();
    resetRejected = !resetApplied;
    if (resetRejected) {
      console.warn(`[panels] ${source} reset-class settings were stored, but reset was rejected`);
    } else {
      console.info(`[panels] ${source} reset-class settings applied via reset`);
    }
  }
  if (needsReload) {
    console.warn(`[panels] ${source} reload-class settings were stored; reload required`);
  }

  const summary = {
    source,
    applied,
    rejected,
    clamped,
    needsReset,
    needsReload,
    resetApplied,
    resetRejected,
    results,
  };
  console.info(`[panels] ${source} settings import summary`, summary);
  return summary;
}

function fmt(n, decimals) {
  return typeof n === "number" ? n.toFixed(decimals) : "—";
}

function clamp(v, min, max) {
  return Math.min(max, Math.max(min, v));
}

let _tipEl = null;
function getTipEl() {
  if (_tipEl) return _tipEl;
  _tipEl = document.createElement("div");
  _tipEl.className = "cfg-tip";
  _tipEl.style.cssText =
    "position:fixed;z-index:1000;max-width:300px;padding:7px 9px;" +
    "background:var(--tip-bg);color:var(--tip-text);border:1px solid var(--tip-border);border-radius:var(--radius-md);" +
    "font-size:11px;line-height:1.45;box-shadow:var(--shadow-tip);" +
    "pointer-events:none;visibility:hidden;opacity:0;white-space:normal;" +
    "transition:opacity 0.06s ease;";
  document.body.appendChild(_tipEl);
  return _tipEl;
}

function styleTip(tip, kind) {
  if (kind === "technical") {
    tip.style.background = "var(--tip-technical-bg)";
    tip.style.color = "var(--tip-technical-text)";
    tip.style.borderColor = "var(--tip-technical-border)";
  } else {
    tip.style.background = "var(--tip-bg)";
    tip.style.color = "var(--tip-text)";
    tip.style.borderColor = "var(--tip-border)";
  }
}

function showTip(text, anchor, kind = "functional") {
  const tip = getTipEl();
  tip.textContent = text;
  styleTip(tip, kind);
  tip.style.visibility = "hidden";
  tip.style.left = "0px";
  tip.style.top = "0px";
  const r = anchor.getBoundingClientRect();
  const tw = tip.offsetWidth;
  const th = tip.offsetHeight;
  let left = clamp(r.left, 8, Math.max(8, window.innerWidth - tw - 8));
  let top = r.bottom + 6;
  if (top + th > window.innerHeight - 8) top = r.top - th - 6;
  tip.style.left = left + "px";
  tip.style.top = Math.max(8, top) + "px";
  tip.style.visibility = "visible";
  tip.style.opacity = "1";
}

function hideTip() {
  if (_tipEl) {
    _tipEl.style.opacity = "0";
    _tipEl.style.visibility = "hidden";
  }
}

function attachTip(el, text, kind = "functional") {
  el.addEventListener("mouseenter", () => showTip(text, el, kind));
  el.addEventListener("mouseleave", hideTip);
  el.addEventListener("focus", () => showTip(text, el, kind));
  el.addEventListener("blur", hideTip);
}

function loadStoredConfig() {
  try {
    const raw = localStorage.getItem(LS_KEY);
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

function saveStoredConfig(settings) {
  try {
    const map = nonDefaultSettingsMap(settings);
    localStorage.setItem(LS_KEY, JSON.stringify(map));
  } catch (e) {
    console.warn("[panels] localStorage write failed:", e);
  }
}

function isDefaultValue(s) {
  if (typeof s.value !== "number" || typeof s.default !== "number") {
    return s.value === s.default;
  }
  return Math.abs(s.value - s.default) <= 1.0e-6;
}

function nonDefaultSettingsMap(settings) {
  const map = {};
  for (const s of settings) {
    if (!HIDDEN_SETTING_IDS.has(s.id) && !isDefaultValue(s)) {
      map[s.id] = s.value;
    }
  }
  return map;
}

function exportConfigPayload(app) {
  return {
    schema: LS_KEY,
    settings: nonDefaultSettingsMap(safeConfigJson(app)),
  };
}

function entriesFromImportPayload(payload) {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    throw new Error("Config import must be a JSON object.");
  }

  if ("schema" in payload) {
    if (payload.schema !== LS_KEY) {
      throw new Error(`Unsupported config schema: ${payload.schema}`);
    }
    if (!payload.settings || typeof payload.settings !== "object" || Array.isArray(payload.settings)) {
      throw new Error("Config payload is missing a settings object.");
    }
    return Object.entries(payload.settings);
  }

  const settings = payload;
  return Object.entries(settings);
}

function buildShareUrl(app) {
  const url = new URL(window.location.href);
  url.searchParams.delete("set");
  for (const [id, value] of Object.entries(exportConfigPayload(app).settings)) {
    url.searchParams.append("set", `${id}:${value}`);
  }
  return url.toString();
}

const APPLY_DOT = {
  live:   { color: "var(--success)", title: "Live - takes effect immediately" },
  reset:  { color: "var(--warning)", title: "Reset - takes effect after Reset" },
  reload: { color: "var(--danger)", title: "Reload - takes effect after page reload" },
};

const APPLY_BADGE = {
  reset:  { text: "reset to apply", cls: "badge-reset" },
  reload: { text: "reload to apply", cls: "badge-reload" },
};

function applyLegendItem(apply) {
  const info = APPLY_DOT[apply];
  if (!info) return "";
  return `
    <span class="settings-legend-item" title="${info.title}">
      <span class="settings-legend-dot" style="background:${info.color}"></span>
      ${apply[0].toUpperCase()}${apply.slice(1)}
    </span>
  `;
}

function renderSettingsHeader(header, tab, settingsCount) {
  if (!header) return;
  const countText = tab.profiler
    ? "live stats"
    : tab.shell
      ? "shell"
      : `${settingsCount} ${settingsCount === 1 ? "setting" : "settings"}`;

  header.innerHTML = `
    <div class="settings-title-row">
      <div class="settings-title">${tab.label}</div>
      <div class="settings-count">${countText}</div>
    </div>
    <div class="settings-legend" aria-label="Apply status legend">
      ${applyLegendItem("live")}
      ${applyLegendItem("reset")}
      ${applyLegendItem("reload")}
    </div>
  `;
}

function showApplyBadge(badge, apply) {
  const bi = APPLY_BADGE[apply];
  if (!bi) {
    badge.style.display = "none";
    return;
  }
  badge.textContent = bi.text;
  badge.className = "cfg-badge " + bi.cls;
  badge.style.display = "inline";
}

function filteredSettingsForTab(settings, tabId) {
  return settings.filter((s) => s.tab === tabId && !HIDDEN_SETTING_IDS.has(s.id));
}

function buildConfigPanel(container, app, tabId, allSettings = safeConfigJson(app), pendingApplyIds = new Set()) {
  hideTip();
  container.innerHTML = "";

  const settings = filteredSettingsForTab(allSettings, tabId);
  if (!settings.length) {
    container.innerHTML = '<p class="panel-empty">No settings returned.</p>';
    return;
  }

  const rowEls = {};
  appendCategorySections(container, settings, rowEls, app, pendingApplyIds);
  appendResetDefaultsAction(container, app, tabId, settings);
}

function buildThemePanel(container) {
  hideTip();
  container.innerHTML = "";
  const grid = document.createElement("div");
  grid.className = "theme-grid";
  const current = activeThemeId();

  for (const theme of THEMES) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "theme-option";
    button.classList.toggle("theme-active", theme.id === current);
    button.setAttribute("aria-pressed", theme.id === current ? "true" : "false");
    button.dataset.theme = theme.id;

    const name = document.createElement("span");
    name.className = "theme-name";
    name.textContent = theme.label;

    const swatches = document.createElement("span");
    swatches.className = "theme-swatches";
    for (const [token, label] of THEME_SWATCH_TOKENS) {
      const swatch = document.createElement("span");
      swatch.className = "theme-swatch";
      const color = theme.tokens[token];
      swatch.style.background = color;
      swatch.title = `${label}: ${token} ${color}`;
      swatches.appendChild(swatch);
    }

    button.appendChild(name);
    button.appendChild(swatches);
    button.addEventListener("click", () => {
      applyTheme(theme.id, true);
      buildThemePanel(container);
    });
    grid.appendChild(button);
  }

  container.appendChild(grid);
}

function appendResetDefaultsAction(container, app, tabId, tabSettings) {
  const actions = document.createElement("div");
  actions.className = "cfg-actions";

  const resetBtn = document.createElement("button");
  resetBtn.className = "panel-btn";
  resetBtn.textContent = "Reset to Defaults";
  resetBtn.title = "Restore this tab to compiled defaults";
  resetBtn.addEventListener("click", () => {
    const pendingApplyIds = new Set();
    for (const s of tabSettings) {
      const result = setSetting(app, s.id, s.default);
      if (result.needs_reset || result.needs_reload) pendingApplyIds.add(s.id);
    }
    persistCurrentSettings(app);
    buildConfigPanel(container, app, tabId, safeConfigJson(app), pendingApplyIds);
  });

  actions.appendChild(resetBtn);
  container.appendChild(actions);
}

function appendCategorySections(parent, settings, rowEls, app, pendingApplyIds = new Set()) {
  if (!settings.length) return;

  const categories = [];
  const byCategory = {};
  for (const s of settings) {
    if (!byCategory[s.category]) {
      byCategory[s.category] = [];
      categories.push(s.category);
    }
    byCategory[s.category].push(s);
  }

  for (const cat of categories) {
    const section = document.createElement("div");
    section.className = "cfg-section";

    const heading = document.createElement("div");
    heading.className = "cfg-section-heading";
    heading.textContent = cat;
    section.appendChild(heading);

    for (const s of byCategory[cat]) {
      const row = buildSettingRow(s, app, pendingApplyIds.has(s.id));
      rowEls[s.id] = row.el;
      section.appendChild(row.el);
    }

    parent.appendChild(section);
  }
}

function appendHelpIcons(labelWrap, s) {
  const hasFunctional = typeof s.tooltip === "string" && s.tooltip.length > 0;
  if (!hasFunctional) return;

  const help = document.createElement("span");
  help.className = "cfg-help";

  if (hasFunctional) {
    const info = document.createElement("span");
    info.className = "cfg-info cfg-info-functional";
    info.textContent = "?";
    info.tabIndex = 0;
    info.setAttribute("role", "button");
    info.setAttribute("aria-label", "Setting help");
    attachTip(info, s.tooltip, "functional");
    help.appendChild(info);
  }

  labelWrap.appendChild(help);
}

function persistCurrentSettings(app) {
  saveStoredConfig(safeConfigJson(app));
}

function buildSettingRow(s, app, showPending = false) {
  const isF32 = s.type === "f32";
  const step = isF32 ? (s.max - s.min) / 200 : 1;
  const decimals = isF32 ? 3 : 0;

  const row = document.createElement("div");
  row.className = "cfg-row";

  const labelWrap = document.createElement("div");
  labelWrap.className = "cfg-label-wrap";

  const dot = document.createElement("span");
  dot.className = "cfg-dot";
  const dotInfo = APPLY_DOT[s.apply] || APPLY_DOT.live;
  dot.style.background = dotInfo.color;
  dot.title = dotInfo.title;

  const label = document.createElement("span");
  label.className = "cfg-label";
  label.textContent = s.label;

  labelWrap.appendChild(dot);
  labelWrap.appendChild(label);
  appendHelpIcons(labelWrap, s);

  if (Array.isArray(s.options) && s.options.length) {
    return buildEnumRow(s, app, row, labelWrap, showPending);
  }

  if (s.slider_scale === "color") {
    return buildColorRow(s, app, row, labelWrap);
  }

  const controls = document.createElement("div");
  controls.className = "cfg-controls";

  const isLog2 = s.slider_scale === "log2";
  const toSlider = (v) => isLog2 ? Math.round(Math.log2(clamp(v, s.min, s.max))) : v;
  const fromSlider = (p) =>
    isLog2 ? Math.pow(2, parseInt(p, 10)) : (isF32 ? parseFloat(p) : parseInt(p, 10));

  const slider = document.createElement("input");
  slider.type = "range";
  slider.min = toSlider(s.min);
  slider.max = toSlider(s.max);
  slider.step = isLog2 ? 1 : step;
  slider.value = toSlider(s.value);
  slider.className = "cfg-slider";

  const numInput = document.createElement("input");
  numInput.type = "number";
  numInput.min = s.min;
  numInput.max = s.max;
  numInput.step = step;
  numInput.value = isF32 ? parseFloat(s.value).toFixed(decimals) : s.value;
  numInput.className = "cfg-number";

  const badge = document.createElement("span");
  badge.className = "cfg-badge";
  badge.style.display = "none";

  const resetBtn = document.createElement("button");
  resetBtn.type = "button";
  resetBtn.className = "cfg-reset-btn";
  resetBtn.textContent = "⟲";
  resetBtn.title = "Reset to default (" + (isF32 ? parseFloat(s.default).toFixed(decimals) : s.default) + ")";

  controls.appendChild(slider);
  controls.appendChild(numInput);
  controls.appendChild(resetBtn);
  row.appendChild(labelWrap);
  row.appendChild(controls);
  row.appendChild(badge);
  if (showPending) showApplyBadge(badge, s.apply);

  function applyChange(rawVal) {
    const v = clamp(isF32 ? parseFloat(rawVal) : parseInt(rawVal, 10), s.min, s.max);
    if (isNaN(v)) return;

    const result = setSetting(app, s.id, v);
    if (!result.ok) return;
    const stored = typeof result.stored_value === "number" ? result.stored_value : v;
    s.value = stored;
    slider.value = toSlider(stored);
    numInput.value = isF32 ? stored.toFixed(decimals) : stored;
    persistCurrentSettings(app);

    if (result.needs_reset || result.needs_reload) {
      showApplyBadge(badge, result.apply || s.apply);
    } else {
      badge.style.display = "none";
    }
  }

  slider.addEventListener("input", () => applyChange(fromSlider(slider.value)));
  numInput.addEventListener("change", () => applyChange(numInput.value));
  resetBtn.addEventListener("click", () => applyChange(s.default));

  return { el: row };
}

function buildEnumRow(s, app, row, labelWrap, showPending = false) {
  const controls = document.createElement("div");
  controls.className = "cfg-controls";

  const select = document.createElement("select");
  select.className = "cfg-select";
  s.options.forEach((optionLabel, i) => {
    const opt = document.createElement("option");
    opt.value = String(i);
    opt.textContent = optionLabel;
    if (i === Math.round(s.value)) opt.selected = true;
    select.appendChild(opt);
  });

  const resetBtn = document.createElement("button");
  resetBtn.type = "button";
  resetBtn.className = "cfg-reset-btn";
  resetBtn.textContent = "⟲";
  const defLabel = s.options[Math.round(s.default)] ?? s.default;
  resetBtn.title = "Reset to default (" + defLabel + ")";

  const badge = document.createElement("span");
  badge.className = "cfg-badge";
  badge.style.display = "none";

  controls.appendChild(select);
  controls.appendChild(resetBtn);
  row.appendChild(labelWrap);
  row.appendChild(controls);
  row.appendChild(badge);
  if (showPending) showApplyBadge(badge, s.apply);

  const autoReset = s.id === "scene.preset";

  function applyChange(v) {
    v = clamp(parseInt(v, 10), s.min, s.max);
    if (isNaN(v)) return;
    const result = setSetting(app, s.id, v);
    if (!result.ok) return;
    const stored = typeof result.stored_value === "number" ? result.stored_value : v;
    s.value = stored;
    select.value = String(stored);
    persistCurrentSettings(app);

    if (autoReset) {
      if (app.reset()) {
        badge.style.display = "none";
      } else {
        showApplyBadge(badge, result.apply || s.apply);
      }
    } else if (result.needs_reset || result.needs_reload) {
      showApplyBadge(badge, result.apply || s.apply);
    }
  }

  select.addEventListener("change", () => applyChange(select.value));
  resetBtn.addEventListener("click", () => applyChange(s.default));

  return { el: row };
}

function buildColorRow(s, app, row, labelWrap) {
  function toHex(v) {
    return "#" + Math.round(v).toString(16).padStart(6, "0");
  }
  function fromHex(h) {
    return parseInt(h.slice(1), 16);
  }

  const controls = document.createElement("div");
  controls.className = "cfg-controls";

  const picker = document.createElement("input");
  picker.type = "color";
  picker.value = toHex(s.value);
  picker.className = "cfg-color";

  const resetBtn = document.createElement("button");
  resetBtn.type = "button";
  resetBtn.className = "cfg-reset-btn";
  resetBtn.textContent = "⟲";
  resetBtn.title = "Reset to default (" + toHex(s.default) + ")";

  const badge = document.createElement("span");
  badge.className = "cfg-badge";
  badge.style.display = "none";

  controls.appendChild(picker);
  controls.appendChild(resetBtn);
  row.appendChild(labelWrap);
  row.appendChild(controls);
  row.appendChild(badge);

  function applyChange(v) {
    const result = setSetting(app, s.id, v);
    if (!result.ok) return;
    const stored = typeof result.stored_value === "number" ? result.stored_value : v;
    s.value = stored;
    picker.value = toHex(stored);
    persistCurrentSettings(app);
    badge.style.display = "none";
  }

  picker.addEventListener("input", () => applyChange(fromHex(picker.value)));
  resetBtn.addEventListener("click", () => applyChange(s.default));

  return { el: row };
}

function buildProfilerPanel(container, app) {
  const stats = safeStatsJson(app);
  if (!stats) {
    container.innerHTML = '<p class="panel-empty">Stats unavailable.</p>';
    return;
  }

  const timing = stats.timing || "unknown";
  const timingColor = timing === "gpu-timestamp" ? "var(--success)" : "var(--warning)";
  const scaleOk = !stats.scale_status || stats.scale_status === "ok";
  const scaleColor = scaleOk ? "var(--success)" : "var(--danger)";
  const fps = stats.fps;
  const fpsColor = fps == null ? "var(--text-faint)" : fps >= 55 ? "var(--success)" : fps >= 30 ? "var(--warning)" : "var(--danger)";
  const rtf = stats.real_time_factor;
  const rtfColor = rtf == null ? "var(--text-faint)" : rtf >= 0.95 ? "var(--success)" : rtf >= 0.5 ? "var(--warning)" : "var(--danger)";
  const capColor = stats.substep_cap_hit ? "var(--danger)" : "var(--prof-muted)";
  const substepText = `${stats.substeps_this_frame ?? stats.substeps ?? "—"} / ${stats.natural_substeps ?? "—"}`;
  const maxSubstepsText = stats.max_substeps != null ? `cap ${stats.max_substeps}` : "cap —";

  let html = `
    <div class="prof-summary">
      <div class="prof-summary-item">
        <span class="prof-summary-label">FPS</span>
        <span class="prof-summary-value" style="color:${fpsColor}">${fmt(fps, 0)}</span>
        <span class="prof-summary-detail">${fmt(stats.frame_avg_ms, 2)} ms avg</span>
      </div>
      <div class="prof-summary-item">
        <span class="prof-summary-label">Real-time</span>
        <span class="prof-summary-value" style="color:${rtfColor}">${fmt(rtf, 2)}x</span>
        <span class="prof-summary-detail">${fmt(stats.sim_advanced_ms, 2)} / ${fmt(stats.wall_raf_ms, 2)} ms</span>
      </div>
      <div class="prof-summary-item">
        <span class="prof-summary-label">Timing</span>
        <span class="prof-summary-value" style="color:${timingColor}">${timing}</span>
        <span class="prof-summary-detail">${stats.frame_samples ?? "—"} frames</span>
      </div>
      <div class="prof-summary-item">
        <span class="prof-summary-label">Scale</span>
        <span class="prof-summary-value" style="color:${scaleColor}">${stats.scale_status ?? "—"}</span>
        <span class="prof-summary-detail">${stats.particles != null ? stats.particles.toLocaleString() : "—"} particles</span>
      </div>
    </div>`;
  const liquidCells = stats.gpu && stats.gpu.liquid_cells != null ? stats.gpu.liquid_cells : null;
  const dispatchShape = stats.particle_dispatch_groups_x != null && stats.particle_dispatch_groups_y != null
    ? `${stats.particle_dispatch_groups_x.toLocaleString()} x ${stats.particle_dispatch_groups_y.toLocaleString()} x 1`
    : "—";
  html += `
    <div class="prof-section-label">Scale and device</div>
    <div class="prof-row">
      <span class="prof-key">Grid res</span>
      <span class="prof-val">${stats.grid_res ?? stats.grid_n ?? "—"}</span>
    </div>
    <div class="prof-row">
      <span class="prof-key">Total / liquid cells</span>
      <span class="prof-val">${stats.total_cells != null ? stats.total_cells.toLocaleString() : "—"} / ${liquidCells != null ? liquidCells.toLocaleString() : "—"}</span>
    </div>
    <div class="prof-row">
      <span class="prof-key">Particles actual / requested</span>
      <span class="prof-val">${stats.particles != null ? stats.particles.toLocaleString() : "—"} / ${stats.requested_particles != null ? stats.requested_particles.toLocaleString() : "—"}</span>
    </div>
    <div class="prof-row">
      <span class="prof-key">Scale preflight</span>
      <span class="prof-val" style="color:${scaleColor}">${stats.scale_status ?? "—"}</span>
    </div>
    <div class="prof-row">
      <span class="prof-key">Seeded / dispatch cap</span>
      <span class="prof-val">${stats.estimated_particles != null ? stats.estimated_particles.toLocaleString() : "—"} / ${stats.max_particle_dispatch_count != null ? stats.max_particle_dispatch_count.toLocaleString() : "—"}</span>
    </div>
    <div class="prof-row">
      <span class="prof-key">Particle dispatch</span>
      <span class="prof-val">${dispatchShape} (${stats.particle_dispatch_capacity != null ? stats.particle_dispatch_capacity.toLocaleString() : "—"} slots)</span>
    </div>
    <div class="prof-row">
      <span class="prof-key">Workgroups / dimension</span>
      <span class="prof-val">${stats.max_compute_workgroups_per_dimension != null ? stats.max_compute_workgroups_per_dimension.toLocaleString() : "—"}</span>
    </div>
    <div class="prof-row">
      <span class="prof-key">GPU buffer mem</span>
      <span class="prof-val">${stats.gpu_buffer_mb != null ? fmt(stats.gpu_buffer_mb, 1) + " MB" : "—"}</span>
    </div>
    <div class="prof-divider"></div>
    <div class="prof-section-label">Frame and simulation</div>
    <div class="prof-row">
      <span class="prof-key">Frame avg</span>
      <span class="prof-val">${fmt(stats.frame_avg_ms, 2)} ms &nbsp;<span class="prof-fps">(${fmt(stats.fps, 1)} fps)</span></span>
    </div>
    <div class="prof-row">
      <span class="prof-key">p50 / p95 / p99</span>
      <span class="prof-val">${fmt(stats.p50, 2)} / ${fmt(stats.p95, 2)} / ${fmt(stats.p99, 2)} ms</span>
    </div>
    <div class="prof-row">
      <span class="prof-key">Substeps exec / natural</span>
      <span class="prof-val">${substepText} &nbsp;<span class="prof-fps" style="color:${capColor}">(${maxSubstepsText}${stats.substep_cap_hit ? ", hit" : ""})</span></span>
    </div>
    <div class="prof-row">
      <span class="prof-key">Timestep policy</span>
      <span class="prof-val">${stats.timestep_policy ?? "—"} &nbsp;<span class="prof-fps">(fixed ${fmt(stats.fixed_dt_ms, 3)} ms)</span></span>
    </div>
    <div class="prof-row">
      <span class="prof-key">Dropped sim time</span>
      <span class="prof-val">${fmt(stats.dropped_sim_time_ms, 2)} ms &nbsp;<span class="prof-fps">(total ${fmt(stats.total_dropped_sim_time_ms, 1)} ms)</span></span>
    </div>
    <div class="prof-row">
      <span class="prof-key">Dispatches / frame</span>
      <span class="prof-val">${stats.dispatches_this_frame != null ? stats.dispatches_this_frame.toLocaleString() : "—"} &nbsp;<span class="prof-fps">(${stats.dispatches_per_substep ?? "—"}/substep)</span></span>
    </div>
    <div class="prof-row">
      <span class="prof-key">Pressure iters / mode</span>
      <span class="prof-val">${stats.pressure_iterations ?? "—"} / ${stats.render_mode ?? "—"}</span>
    </div>
  `;

  if (stats.gpu) {
    const g = stats.gpu;
    const costs = [
      ["Prep", g.prep_ms],
      ["Pressure", g.pressure_ms],
      ["Finish", g.finish_ms],
      ["Render", g.render_ms],
    ].sort((a, b) => (b[1] ?? 0) - (a[1] ?? 0));

    html += `
      <div class="prof-divider"></div>
      <div class="prof-section-label">GPU Costs (sorted, ms/frame)</div>
      ${costs.map(([name, ms], i) => `
        <div class="prof-row${i === 0 ? " prof-total-row" : ""}">
          <span class="prof-key">${name}${i === 0 ? ' <span class="prof-dominant">(top)</span>' : ""}</span>
          <span class="prof-val">${fmt(ms, 2)} ms</span>
        </div>
      `).join("")}
      <div class="prof-row prof-total-row">
        <span class="prof-key">Sim total</span>
        <span class="prof-val">${fmt(g.sim_ms, 2)} ms</span>
      </div>
      <div class="prof-row">
        <span class="prof-key">Render</span>
        <span class="prof-val">${fmt(g.render_ms, 2)} ms</span>
      </div>
      <div class="prof-row">
        <span class="prof-key">Liquid cells</span>
        <span class="prof-val">${g.liquid_cells != null ? g.liquid_cells.toLocaleString() : "—"}</span>
      </div>
    `;

    if (g.sections) {
      html += `
        <div class="prof-divider"></div>
        <div class="prof-section-label">Detailed sections (ms/frame)</div>
      `;
      const sections = Object.entries(g.sections).sort((a, b) => b[1] - a[1]);
      for (const [name, ms] of sections) {
        html += `
          <div class="prof-row">
            <span class="prof-key">${name}</span>
            <span class="prof-val">${fmt(ms, 3)} ms</span>
          </div>
        `;
      }
      if (g.cg) {
        const cg = g.cg;
        html += `
          <div class="prof-section-label">CG (${cg.iters ?? "—"} iters)</div>
          <div class="prof-row prof-total-row">
            <span class="prof-key">CG total</span>
            <span class="prof-val">${fmt(cg.total_ms, 3)} ms</span>
          </div>
          <div class="prof-row">
            <span class="prof-key">avg / iter</span>
            <span class="prof-val">${fmt(cg.avg_ms_per_iter, 3)} ms</span>
          </div>
          <div class="prof-row">
            <span class="prof-key">SpMV total</span>
            <span class="prof-val">${fmt(cg.spmv_ms, 3)} ms</span>
          </div>
          <div class="prof-row">
            <span class="prof-key">Reductions total</span>
            <span class="prof-val">${fmt(cg.reductions_ms, 3)} ms</span>
          </div>
          <div class="prof-row">
            <span class="prof-key">Updates total</span>
            <span class="prof-val">${fmt(cg.updates_ms, 3)} ms</span>
          </div>
          <div class="prof-note">CG categories are contiguous GPU passes: "reductions" includes d·q and r·r dot products; "updates" is the vector update pass.</div>
        `;
      }
    }
  } else {
    html += `
      <div class="prof-divider"></div>
      <div class="prof-note">Per-pass GPU timing unavailable on this adapter (CPU fallback).</div>
    `;
  }

  container.innerHTML = html;
}

export function initPanels(app) {
  const settingsPanel = document.getElementById("settings-panel");
  const settingsBody = document.getElementById("settings-body");
  const settingsHeader = document.getElementById("settings-header");
  const tabsRoot = document.getElementById("settings-tabs");
  const btnConfig = document.getElementById("btn-config");
  const toolbarReset = document.getElementById("btn-reset");

  if (!settingsPanel || !settingsBody || !settingsHeader || !tabsRoot || !btnConfig) {
    console.warn("[panels] Settings DOM elements not found - skipping initPanels.");
    return null;
  }

  const stored = loadStoredConfig();
  applyTheme(loadStoredTheme(), false);
  if (Object.keys(stored).length > 0) {
    applySettingEntries(app, Object.entries(stored), "localStorage");
  }

  const tabs = deriveTabs(safeConfigJson(app));
  const tabMeta = new Map(tabs.map((tab) => [tab.id, tab]));
  let isOpen = false;
  let activeTab = tabMeta.has(DEFAULT_TAB) ? DEFAULT_TAB : tabs[0]?.id ?? "profiler";

  function normalizeTab(tab) {
    const normalized = TAB_ALIASES[tab] || tab;
    return tabMeta.has(normalized) ? normalized : activeTab;
  }

  function renderActiveTab() {
    const currentSettings = safeConfigJson(app);
    const tab = tabMeta.get(activeTab) || tabs[0] || PROFILER_TAB;
    const settingsCount = tab.profiler || tab.shell
      ? 0
      : filteredSettingsForTab(currentSettings, activeTab).length;
    renderSettingsHeader(settingsHeader, tab, settingsCount);
    if (activeTab === "profiler") {
      buildProfilerPanel(settingsBody, app);
    } else if (activeTab === "theme") {
      buildThemePanel(settingsBody);
    } else {
      buildConfigPanel(settingsBody, app, activeTab, currentSettings);
    }
    for (const btn of tabsRoot.querySelectorAll(".tab-btn")) {
      const selected = btn.dataset.tab === activeTab;
      btn.classList.toggle("tab-active", selected);
      btn.setAttribute("aria-selected", selected ? "true" : "false");
      btn.tabIndex = selected ? 0 : -1;
    }
  }

  function setSettingsOpen(nextOpen) {
    isOpen = nextOpen;
    settingsPanel.hidden = !isOpen;
    btnConfig.classList.toggle("btn-active", isOpen);
    btnConfig.setAttribute("aria-expanded", isOpen ? "true" : "false");
    if (isOpen) renderActiveTab();
  }

  function openSettings(tab = DEFAULT_TAB) {
    activeTab = normalizeTab(tab);
    setSettingsOpen(true);
  }

  function closeSettings() {
    setSettingsOpen(false);
  }

  function toggleSettings() {
    if (isOpen) {
      closeSettings();
    } else {
      openSettings(DEFAULT_TAB);
    }
  }

  function setActiveTab(tab) {
    activeTab = normalizeTab(tab);
    if (isOpen) renderActiveTab();
  }

  for (const tab of tabs) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "tab-btn";
    if (tab.variant === "experimental") btn.classList.add("tab-experimental");
    btn.dataset.tab = tab.id;
    btn.textContent = tab.label;
    btn.title = tab.label;
    btn.setAttribute("role", "tab");
    btn.setAttribute("aria-controls", "settings-body");
    btn.addEventListener("click", () => setActiveTab(tab.id));
    tabsRoot.appendChild(btn);
  }

  btnConfig.addEventListener("click", toggleSettings);

  if (toolbarReset) {
    toolbarReset.addEventListener("click", () => {
      if (isOpen) renderActiveTab();
    });
  }

  setSettingsOpen(false);
  renderActiveTab();

  window.setInterval(() => {
    if (isOpen && activeTab === "profiler") {
      buildProfilerPanel(settingsBody, app);
    }
  }, 250);

  return {
    openSettings,
    closeSettings,
    toggleSettings,
    openWorkspace: openSettings,
    closeWorkspace: closeSettings,
    toggleWorkspace: toggleSettings,
    setActiveTab,
    rerender() {
      if (isOpen) renderActiveTab();
    },
    rerenderModes() {
      if (isOpen && activeTab === "scenario") renderActiveTab();
    },
    applySettings(entries, source = "import") {
      const result = applySettingEntries(app, entries, source);
      if (isOpen) renderActiveTab();
      return result;
    },
    importConfigPayload(payload, source = "import") {
      const result = applySettingEntries(app, entriesFromImportPayload(payload), source);
      persistCurrentSettings(app);
      if (isOpen) renderActiveTab();
      return result;
    },
    exportConfig() {
      return exportConfigPayload(app);
    },
    shareUrl() {
      return buildShareUrl(app);
    },
    setting(id) {
      return safeConfigJson(app).find((s) => s.id === id) || null;
    },
    setTheme(id) {
      const themeId = applyTheme(id, true);
      if (isOpen && activeTab === "theme") renderActiveTab();
      return themeId;
    },
    activeTheme() {
      return activeThemeId();
    },
    isOpen() {
      return isOpen;
    },
    activeTab() {
      return activeTab;
    },
    applyStoredConfigDone: true,
  };
}
