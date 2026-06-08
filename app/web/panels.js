// panels.js - Right-side workspace tabs for Fluid Lab.
// Pure ES module, vanilla DOM, no dependencies, no build step.
//
// NOTE on reset/reload-class settings: calling app.set_setting() updates the stored
// value in the Rust side, but the running simulation only picks up "reset"-class
// changes after app.reset() and "reload"-class changes after a full page reload.
// This module flags those rows with a badge so the user knows action is needed.
// Live-class settings apply to the running sim immediately and need no badge.

const LS_KEY = "fluidlab.config.v1";
const HIDDEN_SETTING_IDS = new Set([
  "interaction.auto_roll_enabled",
  "interaction.wave_enabled",
]);
const TAB_ORDER = ["render", "water", "general", "physics", "modes", "profiler"];
const TAB_META = {
  render:   { label: "Render" },
  water:    { label: "Water" },
  general:  { label: "General" },
  physics:  { label: "Physics" },
  modes:    { label: "Modes" },
  profiler: { label: "Profiler" },
};
const TAB_CATEGORY_ALLOWLIST = {
  render: new Set(["Render", "Camera"]),
  water: new Set(["Water"]),
  general: new Set(["Scene", "Grid", "Particles"]),
  physics: new Set(["Physics", "Solver"]),
  modes: new Set(["Interaction"]),
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
    const map = {};
    for (const s of settings) {
      if (!HIDDEN_SETTING_IDS.has(s.id)) {
        map[s.id] = s.value;
      }
    }
    localStorage.setItem(LS_KEY, JSON.stringify(map));
  } catch (e) {
    console.warn("[panels] localStorage write failed:", e);
  }
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

const PANEL_GROUPS = [
  { id: "default", label: "Default", collapsed: false },
  { id: "advanced", label: "Advanced", collapsed: true },
  { id: "dev", label: "Dev", collapsed: true },
];
const PRESENTATION_CATEGORIES = new Set(["Render", "Camera"]);

function filteredSettingsForTab(settings, tabId) {
  if (tabId === "modes") {
    return settings.filter(
      (s) => s.category === "Interaction" && !HIDDEN_SETTING_IDS.has(s.id)
    );
  }
  const allow = TAB_CATEGORY_ALLOWLIST[tabId];
  if (!allow) return settings;
  return settings.filter((s) => allow.has(s.category));
}

function buildConfigPanel(container, app, tabId) {
  hideTip();
  container.innerHTML = "";

  const settings = filteredSettingsForTab(safeConfigJson(app), tabId);
  if (!settings.length) {
    container.innerHTML = '<p class="panel-empty">No settings returned.</p>';
    return;
  }

  if (tabId === "modes") {
    buildModesPanel(container, app, settings);
    return;
  }

  const rowEls = {};
  const byGroup = splitByPanelGroup(settings);
  const defaultSettings = byGroup.default || [];
  const defaultCore = defaultSettings.filter((s) => !PRESENTATION_CATEGORIES.has(s.category));
  const defaultPresentation = defaultSettings.filter((s) => PRESENTATION_CATEGORIES.has(s.category));

  appendCategorySections(container, defaultCore, rowEls, app);

  const expertDrawers = document.createElement("div");
  expertDrawers.className = "cfg-expert-drawers";
  for (const group of PANEL_GROUPS.filter((g) => g.collapsed)) {
    appendCollapsedGroup(expertDrawers, group, byGroup[group.id], rowEls, app);
  }
  if (expertDrawers.children.length > 0) {
    container.appendChild(expertDrawers);
  }

  appendCategorySections(container, defaultPresentation, rowEls, app);
  appendResetDefaultsAction(container, app, tabId);
}

function buildModesPanel(container, app, settings) {
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
      if (setting && setting.panel_group === "default") {
        orderedDefault.push(setting);
        const row = buildSettingRow(setting, app);
        rowEls[setting.id] = row.el;
        section.appendChild(row.el);
      }
    }

    if (section.children.length > 1) {
      container.appendChild(section);
    }
  }

  const leftover = settings.filter((s) => !orderedDefault.includes(s));
  const byGroup = splitByPanelGroup(leftover);

  const expertDrawers = document.createElement("div");
  expertDrawers.className = "cfg-expert-drawers";
  for (const group of PANEL_GROUPS.filter((g) => g.collapsed)) {
    appendCollapsedGroup(expertDrawers, group, byGroup[group.id], rowEls, app);
  }
  if (expertDrawers.children.length > 0) {
    container.appendChild(expertDrawers);
  }

  appendResetDefaultsAction(container, app, "modes");
}

function splitByPanelGroup(settings) {
  const byGroup = {};
  for (const group of PANEL_GROUPS) byGroup[group.id] = [];
  for (const s of settings) {
    const group = PANEL_GROUPS.some((g) => g.id === s.panel_group) ? s.panel_group : "default";
    byGroup[group].push(s);
  }
  return byGroup;
}

function appendResetDefaultsAction(container, app, tabId) {
  const actions = document.createElement("div");
  actions.className = "cfg-actions";

  const resetBtn = document.createElement("button");
  resetBtn.className = "panel-btn";
  resetBtn.textContent = "Reset to Defaults";
  resetBtn.title = "Restore all settings to their compiled defaults";
  resetBtn.addEventListener("click", () => {
    const current = safeConfigJson(app);
    for (const s of current) {
      app.set_setting(s.id, s.default);
    }
    localStorage.removeItem(LS_KEY);
    buildConfigPanel(container, app, tabId);
  });

  actions.appendChild(resetBtn);
  container.appendChild(actions);
}

function appendCollapsedGroup(parent, group, groupSettings, rowEls, app) {
  if (!groupSettings || !groupSettings.length) return;

  const details = document.createElement("details");
  details.className = "cfg-group cfg-group-" + group.id;

  const summary = document.createElement("summary");
  summary.className = "cfg-group-heading";

  const title = document.createElement("span");
  title.textContent = group.label;
  const count = document.createElement("span");
  count.className = "cfg-group-count";
  count.textContent = groupSettings.length + " controls";

  summary.appendChild(title);
  summary.appendChild(count);
  details.appendChild(summary);

  const body = document.createElement("div");
  body.className = "cfg-group-body";
  appendCategorySections(body, groupSettings, rowEls, app);
  details.appendChild(body);

  parent.appendChild(details);
}

function appendCategorySections(parent, settings, rowEls, app) {
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
      const row = buildSettingRow(s, app);
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

function buildSettingRow(s, app) {
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
    return buildEnumRow(s, app, row, labelWrap);
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

  function applyChange(rawVal) {
    const v = clamp(isF32 ? parseFloat(rawVal) : parseInt(rawVal, 10), s.min, s.max);
    if (isNaN(v)) return;

    const live = app.set_setting(s.id, v);
    s.value = v;
    slider.value = toSlider(v);
    numInput.value = isF32 ? v.toFixed(decimals) : v;
    persistCurrentSettings(app);

    if (!live && s.apply !== "live") {
      const bi = APPLY_BADGE[s.apply];
      if (bi) {
        badge.textContent = bi.text;
        badge.className = "cfg-badge " + bi.cls;
        badge.style.display = "inline";
      }
    } else {
      badge.style.display = "none";
    }
  }

  slider.addEventListener("input", () => applyChange(fromSlider(slider.value)));
  numInput.addEventListener("change", () => applyChange(numInput.value));
  resetBtn.addEventListener("click", () => applyChange(s.default));

  return { el: row };
}

function buildEnumRow(s, app, row, labelWrap) {
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

  const autoReset = s.id === "scene.preset";

  function applyChange(v) {
    v = clamp(parseInt(v, 10), s.min, s.max);
    if (isNaN(v)) return;
    app.set_setting(s.id, v);
    s.value = v;
    select.value = String(v);
    persistCurrentSettings(app);

    if (autoReset) {
      app.reset();
      badge.style.display = "none";
    } else if (s.apply !== "live") {
      const bi = APPLY_BADGE[s.apply];
      if (bi) {
        badge.textContent = bi.text;
        badge.className = "cfg-badge " + bi.cls;
        badge.style.display = "inline";
      }
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
  picker.style.cssText =
    "width:44px;height:26px;padding:1px 2px;border:1px solid #2a3142;" +
    "border-radius:4px;background:#0f1219;cursor:pointer;flex-shrink:0;";

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
    app.set_setting(s.id, v);
    s.value = v;
    picker.value = toHex(v);
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

  let html = `
    <div class="prof-row" style="align-items:baseline;padding-top:2px;padding-bottom:2px;">
      <span class="prof-key" style="font-size:12px;">FPS</span>
      <span class="prof-val" style="font-size:20px;font-weight:700;color:${fpsColor};line-height:1;">${fmt(fps, 0)}</span>
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
      <span class="prof-key">Substeps this frame</span>
      <span class="prof-val">${stats.substeps_this_frame ?? stats.substeps ?? "—"}</span>
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
          <div class="prof-note">CG categories are contiguous GPU passes: "reductions" is the d·q dot; the r·r dot is grouped into "updates".</div>
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
  const workspace = document.getElementById("workspace");
  const workspaceBody = document.getElementById("workspace-body");
  const tabsRoot = document.getElementById("workspace-tabs");
  const tabLabel = document.getElementById("workspace-tab-label");
  const btnConfig = document.getElementById("btn-config");
  const toolbarReset = document.getElementById("btn-reset");

  if (!workspace || !workspaceBody || !tabsRoot || !tabLabel || !btnConfig) {
    console.warn("[panels] Workspace DOM elements not found - skipping initPanels.");
    return null;
  }

  const stored = loadStoredConfig();
  if (Object.keys(stored).length > 0) {
    const current = safeConfigJson(app);
    const knownIds = new Set(current.map((s) => s.id));
    let needsRebuild = false;
    for (const [id, value] of Object.entries(stored)) {
      if (knownIds.has(id)) {
        try {
          const live = app.set_setting(id, value);
          if (!live) needsRebuild = true;
        } catch (e) {
          console.warn("[panels] failed to restore setting", id, e);
        }
      }
    }
    if (needsRebuild) app.reset();
  }

  let isOpen = false;
  let activeTab = "general";

  function renderActiveTab() {
    if (activeTab === "profiler") {
      buildProfilerPanel(workspaceBody, app);
    } else {
      buildConfigPanel(workspaceBody, app, activeTab);
    }
    tabLabel.textContent = TAB_META[activeTab].label;
    for (const btn of tabsRoot.querySelectorAll(".tab-btn")) {
      const selected = btn.dataset.tab === activeTab;
      btn.classList.toggle("tab-active", selected);
      btn.setAttribute("aria-selected", selected ? "true" : "false");
      btn.tabIndex = selected ? 0 : -1;
    }
  }

  function setWorkspaceOpen(nextOpen) {
    isOpen = nextOpen;
    workspace.hidden = !isOpen;
    btnConfig.classList.toggle("btn-active", isOpen);
    btnConfig.setAttribute("aria-expanded", isOpen ? "true" : "false");
    if (isOpen) renderActiveTab();
  }

  function openWorkspace(tab = "general") {
    activeTab = TAB_META[tab] ? tab : "general";
    setWorkspaceOpen(true);
  }

  function closeWorkspace() {
    setWorkspaceOpen(false);
  }

  function toggleWorkspace() {
    if (isOpen) {
      closeWorkspace();
    } else {
      openWorkspace("general");
    }
  }

  function setActiveTab(tab) {
    if (!TAB_META[tab]) return;
    activeTab = tab;
    if (isOpen) renderActiveTab();
  }

  for (const tabId of TAB_ORDER) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "tab-btn";
    btn.dataset.tab = tabId;
    btn.textContent = TAB_META[tabId].label;
    btn.setAttribute("role", "tab");
    btn.setAttribute("aria-controls", "workspace-body");
    btn.addEventListener("click", () => setActiveTab(tabId));
    tabsRoot.appendChild(btn);
  }

  btnConfig.addEventListener("click", toggleWorkspace);

  if (toolbarReset) {
    toolbarReset.addEventListener("click", () => {
      if (isOpen) renderActiveTab();
    });
  }

  setWorkspaceOpen(false);
  renderActiveTab();

  window.setInterval(() => {
    if (isOpen && activeTab === "profiler") {
      buildProfilerPanel(workspaceBody, app);
    }
  }, 250);

  return {
    openWorkspace,
    closeWorkspace,
    toggleWorkspace,
    setActiveTab,
    rerender() {
      if (isOpen) renderActiveTab();
    },
    rerenderModes() {
      if (isOpen && activeTab === "modes") renderActiveTab();
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
