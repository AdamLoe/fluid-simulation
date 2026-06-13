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
const HIDDEN_SETTING_IDS = new Set([
  "interaction.auto_roll_enabled",
  "interaction.wave_enabled",
]);
const DEFAULT_TAB = "scenario";
const PROFILER_TAB = { id: "profiler", label: "Profiler", order: 1000, profiler: true };
const TAB_ALIASES = {
  general: "scenario",
  render: "camera-view",
  water: "water-surface",
  physics: "simulation",
};
const MODE_SECTION_ORDER = [
  {
    label: "Auto Rotate",
    note: "Controls the scheduled tank rocking used by Auto Rotate.",
    ids: ["interaction.auto_roll_strength", "interaction.auto_roll_cadence"],
  },
  {
    label: "Waves",
    note: "Controls the scheduled wave-maker impulses used by Waves.",
    ids: ["interaction.wave_strength", "interaction.wave_frequency"],
  },
];

function deriveTabs(settings) {
  const byId = new Map();
  for (const s of settings) {
    if (HIDDEN_SETTING_IDS.has(s.id)) continue;
    if (!s.tab || !s.tab_label) continue;
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
  return [...byId.values()].sort((a, b) => a.order - b.order).concat(PROFILER_TAB);
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
    "background:#0f1219;color:#cdd6e4;border:1px solid #2a3142;border-radius:6px;" +
    "font-size:11px;line-height:1.45;box-shadow:0 4px 14px rgba(0,0,0,0.5);" +
    "pointer-events:none;visibility:hidden;opacity:0;white-space:normal;" +
    "transition:opacity 0.06s ease;";
  document.body.appendChild(_tipEl);
  return _tipEl;
}

function styleTip(tip, kind) {
  if (kind === "technical") {
    tip.style.background = "#151427";
    tip.style.color = "#ded8ff";
    tip.style.borderColor = "#6d5dfc";
  } else {
    tip.style.background = "#0f1219";
    tip.style.color = "#cdd6e4";
    tip.style.borderColor = "#2a3142";
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

function formatImportSummary(result) {
  const parts = [`${result.applied} applied`];
  if (result.clamped) parts.push(`${result.clamped} clamped`);
  const unknown = result.results.filter((r) => r.status === "unknown_id").length;
  if (unknown) parts.push(`${unknown} unknown`);
  if (result.rejected && result.rejected !== unknown) parts.push(`${result.rejected} rejected`);
  if (result.resetApplied) parts.push("reset applied");
  if (result.resetRejected) parts.push("reset failed");
  if (result.needsReload) parts.push("reload needed");
  return parts.join(", ");
}

const APPLY_DOT = {
  live:   { color: "#4ade80", title: "Live - takes effect immediately" },
  reset:  { color: "#fbbf24", title: "Reset - takes effect after Reset" },
  reload: { color: "#f87171", title: "Reload - takes effect after page reload" },
};

const APPLY_BADGE = {
  reset:  { text: "reset to apply", cls: "badge-reset" },
  reload: { text: "reload to apply", cls: "badge-reload" },
};

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

  if (tabId === "modes") {
    buildModesPanel(container, app, settings, pendingApplyIds);
    return;
  }

  if (tabId === "scenario") {
    appendScenarioSummary(container, app);
  }

  const rowEls = {};
  appendCategorySections(container, settings, rowEls, app, pendingApplyIds);
  appendResetDefaultsAction(container, app, tabId, settings);
}

// A small read-only readout at the top of the Scenario tab. It shows the grid
// resolution and the EFFECTIVE particle count the current density + grid + scenario
// resolve to, so the user can see what the "Particle density (/cell)" control yields
// without opening the Profiler. Values come from stats_json (the live resolved
// count); count is Reset-class, so it reflects the last applied reset.
function appendScenarioSummary(container, app) {
  const stats = safeStatsJson(app);
  if (!stats) return;

  const gridRes = stats.grid_res ?? stats.grid_n;
  const totalCells = stats.total_cells;
  const requested = stats.requested_particles;
  const actual = stats.particles;

  const box = document.createElement("div");
  box.className = "cfg-scenario-summary";
  box.style.cssText =
    "margin:0 0 10px;padding:8px 10px;background:#11151f;border:1px solid #2a3142;" +
    "border-radius:6px;font-size:11px;line-height:1.5;color:#aeb8cc;";

  const fmtN = (n) => (typeof n === "number" ? n.toLocaleString() : "—");
  box.innerHTML =
    `<div style="font-weight:600;color:#cdd6e4;margin-bottom:3px;">Effective scenario</div>` +
    `<div>Grid: <span style="color:#cdd6e4;">${gridRes ?? "—"}</span>` +
    ` &nbsp;·&nbsp; Total cells: <span style="color:#cdd6e4;">${fmtN(totalCells)}</span></div>` +
    `<div>Particles (resolved): <span style="color:#7dd3fc;">${fmtN(requested)}</span>` +
    (typeof actual === "number" && actual !== requested
      ? ` &nbsp;·&nbsp; seeded: <span style="color:#cdd6e4;">${fmtN(actual)}</span>`
      : "") +
    `</div>` +
    `<div style="color:#6b7689;margin-top:2px;">Density &times; grid &times; scenario fill. Changes apply on Reset.</div>`;

  container.appendChild(box);
}

function buildModesPanel(container, app, settings, pendingApplyIds = new Set()) {
  const rowEls = {};
  const settingsById = new Map(settings.map((s) => [s.id, s]));
  const orderedDefault = [];

  for (const sectionMeta of MODE_SECTION_ORDER) {
    const note = document.createElement("div");
    note.className = "mode-section-note";
    note.textContent = sectionMeta.note;
    container.appendChild(note);

    const section = document.createElement("div");
    section.className = "cfg-section";
    const heading = document.createElement("div");
    heading.className = "cfg-section-heading";
    heading.textContent = sectionMeta.label;
    section.appendChild(heading);

    for (const id of sectionMeta.ids) {
      const setting = settingsById.get(id);
      if (setting) {
        orderedDefault.push(setting);
        const row = buildSettingRow(setting, app, pendingApplyIds.has(setting.id));
        rowEls[setting.id] = row.el;
        section.appendChild(row.el);
      }
    }

    if (section.children.length > 1) {
      container.appendChild(section);
    }
  }

  const leftover = settings.filter((s) => !orderedDefault.includes(s));
  if (leftover.length) {
    appendCategorySections(container, leftover, rowEls, app, pendingApplyIds);
  }

  appendResetDefaultsAction(container, app, "modes", settings);
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
  appendShareImportActions(container, app);
}

function appendShareImportActions(container, app) {
  const actions = document.createElement("div");
  actions.className = "cfg-actions cfg-share-actions";

  const status = document.createElement("div");
  status.className = "cfg-share-status";
  status.setAttribute("aria-live", "polite");

  const copyBtn = document.createElement("button");
  copyBtn.className = "panel-btn";
  copyBtn.type = "button";
  copyBtn.textContent = "Copy Share URL";
  copyBtn.title = "Copy a URL with visible non-default settings";

  const exportBtn = document.createElement("button");
  exportBtn.className = "panel-btn";
  exportBtn.type = "button";
  exportBtn.textContent = "Export JSON";
  exportBtn.title = "Download visible non-default settings as JSON";

  const importBtn = document.createElement("button");
  importBtn.className = "panel-btn";
  importBtn.type = "button";
  importBtn.textContent = "Import JSON";
  importBtn.title = "Import a settings JSON file";

  const importInput = document.createElement("input");
  importInput.type = "file";
  importInput.accept = "application/json,.json";
  importInput.hidden = true;

  function setStatus(text, isError = false) {
    status.textContent = text;
    status.classList.toggle("cfg-share-error", isError);
  }

  copyBtn.addEventListener("click", async () => {
    const url = buildShareUrl(app);
    try {
      await navigator.clipboard.writeText(url);
      setStatus("Share URL copied.");
    } catch (e) {
      console.warn("[panels] clipboard write failed:", e);
      setStatus("Copy failed; share URL is in the console.", true);
    }
    console.info("[panels] share URL", url);
  });

  exportBtn.addEventListener("click", () => {
    const payload = exportConfigPayload(app);
    const blob = new Blob([JSON.stringify(payload, null, 2) + "\n"], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = "fluidlab-config.json";
    document.body.appendChild(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
    setStatus(`Exported ${Object.keys(payload.settings).length} settings.`);
    console.info("[panels] export config", payload);
  });

  importBtn.addEventListener("click", () => importInput.click());
  importInput.addEventListener("change", async () => {
    const file = importInput.files && importInput.files[0];
    importInput.value = "";
    if (!file) return;
    try {
      const payload = JSON.parse(await file.text());
      const result = applySettingEntries(app, entriesFromImportPayload(payload), "file import");
      persistCurrentSettings(app);
      setStatus(formatImportSummary(result), result.rejected > 0);
    } catch (e) {
      console.warn("[panels] import failed:", e);
      setStatus("Import failed; see console.", true);
    }
  });

  actions.appendChild(copyBtn);
  actions.appendChild(exportBtn);
  actions.appendChild(importBtn);
  actions.appendChild(importInput);
  container.appendChild(actions);
  container.appendChild(status);
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
  const hasTechnical = typeof s.technical_tooltip === "string" && s.technical_tooltip.length > 0;
  if (!hasFunctional && !hasTechnical) return;

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

  if (hasTechnical) {
    const tech = document.createElement("span");
    tech.className = "cfg-info cfg-info-technical";
    tech.textContent = "T";
    tech.tabIndex = 0;
    tech.setAttribute("role", "button");
    tech.setAttribute("aria-label", "Technical setting help");
    attachTip(tech, s.technical_tooltip, "technical");
    help.appendChild(tech);
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
  const timingColor = timing === "gpu-timestamp" ? "#4ade80" : "#fbbf24";
  const scaleOk = !stats.scale_status || stats.scale_status === "ok";
  const scaleColor = scaleOk ? "#4ade80" : "#f87171";
  const fps = stats.fps;
  const fpsColor = fps == null ? "#6b7689" : fps >= 55 ? "#4ade80" : fps >= 30 ? "#fbbf24" : "#f87171";
  const rtf = stats.real_time_factor;
  const rtfColor = rtf == null ? "#6b7689" : rtf >= 0.95 ? "#4ade80" : rtf >= 0.5 ? "#fbbf24" : "#f87171";
  const capColor = stats.substep_cap_hit ? "#f87171" : "#94a3b8";
  const substepText = `${stats.substeps_this_frame ?? stats.substeps ?? "—"} / ${stats.natural_substeps ?? "—"}`;
  const maxSubstepsText = stats.max_substeps != null ? `cap ${stats.max_substeps}` : "cap —";

  let html = `
    <div class="prof-row" style="align-items:baseline;padding-top:2px;padding-bottom:2px;">
      <span class="prof-key" style="font-size:12px;">FPS</span>
      <span class="prof-val" style="font-size:20px;font-weight:700;color:${fpsColor};line-height:1;">${fmt(fps, 0)}</span>
    </div>
    <div class="prof-row">
      <span class="prof-key">Sim / real-time</span>
      <span class="prof-val" style="color:${rtfColor}">${fmt(rtf, 2)}x &nbsp;<span class="prof-fps">(${fmt(stats.sim_advanced_ms, 2)} / ${fmt(stats.wall_raf_ms, 2)} ms)</span></span>
    </div>
    <div class="prof-divider"></div>
    <div class="prof-row prof-header-row">
      <span class="prof-key">Timing</span>
      <span class="prof-val" style="color:${timingColor}">${timing} (${stats.frame_samples ?? "—"} frames)</span>
    </div>`;
  const liquidCells = stats.gpu && stats.gpu.liquid_cells != null ? stats.gpu.liquid_cells : null;
  const dispatchShape = stats.particle_dispatch_groups_x != null && stats.particle_dispatch_groups_y != null
    ? `${stats.particle_dispatch_groups_x.toLocaleString()} x ${stats.particle_dispatch_groups_y.toLocaleString()} x 1`
    : "—";
  html += `
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

    if (g.diffuse) {
      const d = g.diffuse;
      html += `
        <div class="prof-row">
          <span class="prof-key">Foam particles${d.clamped > 0 ? ' <span class="prof-dominant">(capped)</span>' : ""}</span>
          <span class="prof-val">${(d.alive ?? 0).toLocaleString()}</span>
        </div>
        <div class="prof-row">
          <span class="prof-key">&nbsp;&nbsp;emitted / clamped</span>
          <span class="prof-val">${(d.emitted ?? 0).toLocaleString()} / ${(d.clamped ?? 0).toLocaleString()}</span>
        </div>
      `;
    }

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
  const tabsRoot = document.getElementById("settings-tabs");
  const btnConfig = document.getElementById("btn-config");
  const toolbarReset = document.getElementById("btn-reset");

  if (!settingsPanel || !settingsBody || !tabsRoot || !btnConfig) {
    console.warn("[panels] Settings DOM elements not found - skipping initPanels.");
    return null;
  }

  const stored = loadStoredConfig();
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
    if (activeTab === "profiler") {
      buildProfilerPanel(settingsBody, app);
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

  let lastGroup = "";
  for (const tab of tabs) {
    if (tab.group !== lastGroup) {
      lastGroup = tab.group;
      const group = document.createElement("div");
      group.className = "tab-group-label";
      group.textContent = tab.group;
      tabsRoot.appendChild(group);
    }
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
      if (isOpen && activeTab === "modes") renderActiveTab();
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
    isOpen() {
      return isOpen;
    },
    activeTab() {
      return activeTab;
    },
    applyStoredConfigDone: true,
  };
}
