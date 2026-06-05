// panels.js — Config + Profiler side panels for Fluid Lab (phase 1.2 observability)
// Pure ES module, vanilla DOM, no dependencies, no build step.
//
// NOTE on reset/reload-class settings: calling app.set_setting() updates the stored
// value in the Rust side, but the running simulation only picks up "reset"-class
// changes after app.reset() and "reload"-class changes after a full page reload.
// This module flags those rows with a badge so the user knows action is needed.
// Live-class settings apply to the running sim immediately and need no badge.

const LS_KEY = "fluidlab.config.v1";

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

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

// ─────────────────────────────────────────────────────────────────────────────
// Instant hover tooltip
//
// The native `title` attribute only surfaces after a ~1s browser delay, which
// feels broken on the ⓘ affordance. This is a single shared bubble parented on
// <body> (position:fixed) so it appears immediately on hover and is never
// clipped by the panel-body's overflow.
// ─────────────────────────────────────────────────────────────────────────────

let _tipEl = null;
function getTipEl() {
  if (_tipEl) return _tipEl;
  _tipEl = document.createElement("div");
  _tipEl.className = "cfg-tip";
  _tipEl.style.cssText =
    "position:fixed;z-index:1000;max-width:260px;padding:7px 9px;" +
    "background:#0f1219;color:#cdd6e4;border:1px solid #2a3142;border-radius:6px;" +
    "font-size:11px;line-height:1.45;box-shadow:0 4px 14px rgba(0,0,0,0.5);" +
    "pointer-events:none;visibility:hidden;opacity:0;white-space:normal;" +
    "transition:opacity 0.06s ease;";
  document.body.appendChild(_tipEl);
  return _tipEl;
}

function showTip(text, anchor) {
  const tip = getTipEl();
  tip.textContent = text;
  // Make it laid-out-but-invisible so we can measure before positioning.
  tip.style.visibility = "hidden";
  tip.style.left = "0px";
  tip.style.top = "0px";
  const r = anchor.getBoundingClientRect();
  const tw = tip.offsetWidth;
  const th = tip.offsetHeight;
  let left = clamp(r.left, 8, Math.max(8, window.innerWidth - tw - 8));
  let top = r.bottom + 6;
  if (top + th > window.innerHeight - 8) top = r.top - th - 6; // flip above if needed
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

// Attach instant-tooltip behaviour to an element (hover + keyboard focus).
function attachTip(el, text) {
  el.addEventListener("mouseenter", () => showTip(text, el));
  el.addEventListener("mouseleave", hideTip);
  el.addEventListener("focus", () => showTip(text, el));
  el.addEventListener("blur", hideTip);
}

// ─────────────────────────────────────────────────────────────────────────────
// localStorage helpers
// ─────────────────────────────────────────────────────────────────────────────

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
    for (const s of settings) map[s.id] = s.value;
    localStorage.setItem(LS_KEY, JSON.stringify(map));
  } catch (e) {
    console.warn("[panels] localStorage write failed:", e);
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Config panel
// ─────────────────────────────────────────────────────────────────────────────

const APPLY_DOT = {
  live:   { color: "#4ade80", title: "Live — takes effect immediately" },
  reset:  { color: "#fbbf24", title: "Reset — takes effect after Reset" },
  reload: { color: "#f87171", title: "Reload — takes effect after page reload" },
};

const APPLY_BADGE = {
  reset:  { text: "⟳ reset to apply",  cls: "badge-reset" },
  reload: { text: "⤓ reload to apply", cls: "badge-reload" },
};

function buildConfigPanel(container, app) {
  hideTip();
  container.innerHTML = "";

  const settings = safeConfigJson(app);
  if (!settings.length) {
    container.innerHTML = '<p class="panel-empty">No settings returned.</p>';
    return;
  }

  // Group by category, preserving first-seen order
  const categories = [];
  const byCategory = {};
  for (const s of settings) {
    if (!byCategory[s.category]) {
      byCategory[s.category] = [];
      categories.push(s.category);
    }
    byCategory[s.category].push(s);
  }

  // Track row elements by id for badge/value updates
  const rowEls = {};

  for (const cat of categories) {
    const section = document.createElement("div");
    section.className = "cfg-section";

    const heading = document.createElement("div");
    heading.className = "cfg-section-heading";
    heading.textContent = cat;
    section.appendChild(heading);

    for (const s of byCategory[cat]) {
      const row = buildSettingRow(s, app, settings);
      rowEls[s.id] = row.el;
      section.appendChild(row.el);
    }

    container.appendChild(section);
  }

  // Bottom action buttons
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
    // Re-render config panel with fresh values
    buildConfigPanel(container, app);
  });

  const copyBtn = document.createElement("button");
  copyBtn.className = "panel-btn";
  copyBtn.textContent = "Copy Config JSON";
  copyBtn.title = "Copy current config to clipboard as JSON";
  copyBtn.addEventListener("click", () => {
    const json = app.config_json();
    navigator.clipboard.writeText(json).then(() => {
      copyBtn.textContent = "Copied!";
      setTimeout(() => { copyBtn.textContent = "Copy Config JSON"; }, 1500);
    }).catch(e => {
      console.warn("[panels] clipboard write failed:", e);
      copyBtn.textContent = "Failed";
      setTimeout(() => { copyBtn.textContent = "Copy Config JSON"; }, 1500);
    });
  });

  actions.appendChild(resetBtn);
  actions.appendChild(copyBtn);
  container.appendChild(actions);
}

function buildSettingRow(s, app, allSettings) {
  const isF32 = s.type === "f32";
  const step = isF32 ? (s.max - s.min) / 200 : 1;
  const decimals = isF32 ? 3 : 0;

  const row = document.createElement("div");
  row.className = "cfg-row";

  // Label + apply-class dot
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

  // Visible info affordance: a dimmed ⓘ that surfaces the tooltip on hover.
  // Uses the instant custom tooltip (no native-title delay) and is keyboard
  // focusable so the description is reachable without a mouse.
  const info = document.createElement("span");
  info.className = "cfg-info";
  info.textContent = "ⓘ";
  info.tabIndex = 0;
  info.style.cssText =
    "flex-shrink:0;cursor:help;color:#5a6a98;font-size:11px;margin-left:2px;";
  attachTip(info, s.tooltip || s.label);

  labelWrap.appendChild(dot);
  labelWrap.appendChild(label);
  labelWrap.appendChild(info);

  // Enum-valued settings (carry `options`) render as a dropdown instead of a
  // slider. The stored value is the selected option's index.
  if (Array.isArray(s.options) && s.options.length) {
    return buildEnumRow(s, app, row, labelWrap);
  }

  // Controls: slider + number input
  const controls = document.createElement("div");
  controls.className = "cfg-controls";

  const slider = document.createElement("input");
  slider.type = "range";
  slider.min = s.min;
  slider.max = s.max;
  slider.step = step;
  slider.value = s.value;
  slider.className = "cfg-slider";

  const numInput = document.createElement("input");
  numInput.type = "number";
  numInput.min = s.min;
  numInput.max = s.max;
  numInput.step = step;
  numInput.value = isF32 ? parseFloat(s.value).toFixed(decimals) : s.value;
  numInput.className = "cfg-number";

  // Badge for non-live settings (shown when value has been changed)
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
    s.value = v; // update local mirror

    // Sync both controls without triggering each other
    slider.value = v;
    numInput.value = isF32 ? v.toFixed(decimals) : v;

    // Persist to localStorage
    saveStoredConfig(safeConfigJson(app));

    // Show/clear badge
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

  slider.addEventListener("input", () => applyChange(slider.value));
  numInput.addEventListener("change", () => applyChange(numInput.value));
  // Per-setting reset reuses applyChange, so it goes through the same
  // set_setting + control-sync + reset-class badge path as a manual edit.
  resetBtn.addEventListener("click", () => applyChange(s.default));

  return { el: row };
}

// Build a dropdown row for an enum-valued setting (e.g. the scenario selector).
// The `scene.preset` selector auto-applies by calling app.reset() so picking a
// scenario rebuilds the sim immediately (no manual Reset needed).
function buildEnumRow(s, app, row, labelWrap) {
  const controls = document.createElement("div");
  controls.className = "cfg-controls";

  const select = document.createElement("select");
  select.className = "cfg-select";
  s.options.forEach((label, i) => {
    const opt = document.createElement("option");
    opt.value = String(i);
    opt.textContent = label;
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
    saveStoredConfig(safeConfigJson(app));

    if (autoReset) {
      // Rebuild the sim now so the new scenario takes effect immediately.
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
  // Per-setting reset reuses applyChange, so it goes through the same
  // set_setting + select-sync + auto-reset/badge path as a manual change.
  resetBtn.addEventListener("click", () => applyChange(s.default));

  return { el: row };
}

// ─────────────────────────────────────────────────────────────────────────────
// Profiler panel
// ─────────────────────────────────────────────────────────────────────────────

function buildProfilerPanel(container, app) {
  // Wipe and re-render on each poll tick
  const stats = safeStatsJson(app);

  if (!stats) {
    container.innerHTML = '<p class="panel-empty">Stats unavailable.</p>';
    return;
  }

  const timing = stats.timing || "unknown";
  const timingColor = timing === "gpu-timestamp" ? "#4ade80" : "#fbbf24";

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
      <span class="prof-val" style="color:${timingColor}">${timing}</span>
    </div>` ;
  const liquidCells = stats.gpu && stats.gpu.liquid_cells != null ? stats.gpu.liquid_cells : null;
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
      <span class="prof-key">Particles</span>
      <span class="prof-val">${stats.particles != null ? stats.particles.toLocaleString() : "—"}</span>
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
  `;

  if (stats.gpu) {
    const g = stats.gpu;
    const simMs = g.sim_ms ?? 0;
    const pressurePct = simMs > 0 ? (g.pressure_ms / simMs) * 100 : 0;

    html += `
      <div class="prof-divider"></div>
      <div class="prof-section-label">GPU Pass Breakdown</div>

      <div class="prof-row">
        <span class="prof-key">Prep</span>
        <span class="prof-val">${fmt(g.prep_ms, 2)} ms</span>
      </div>
      <div class="prof-row prof-pressure-row">
        <span class="prof-key">Pressure <span class="prof-dominant">(dominant)</span></span>
        <span class="prof-val">${fmt(g.pressure_ms, 2)} ms</span>
      </div>
      <div class="prof-pressure-bar-wrap">
        <div class="prof-pressure-bar" style="width:${pressurePct.toFixed(1)}%"></div>
        <span class="prof-pressure-pct">${pressurePct.toFixed(0)}% of sim</span>
      </div>
      <div class="prof-row">
        <span class="prof-key">Finish</span>
        <span class="prof-val">${fmt(g.finish_ms, 2)} ms</span>
      </div>
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

    // Detailed (fine) per-section breakdown — only when the dev toggle is on and
    // real timestamps produced a `sections` object. All values are ms/frame
    // (summed over the substeps that ran this frame).
    if (g.sections) {
      html += `
        <div class="prof-divider"></div>
        <div class="prof-section-label">Detailed sections (ms/frame)</div>
      `;
      for (const [name, ms] of Object.entries(g.sections)) {
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

// ─────────────────────────────────────────────────────────────────────────────
// Public entry point
// ─────────────────────────────────────────────────────────────────────────────

export function initPanels(app) {
  const configPanel  = document.getElementById("config-panel");
  const profPanel    = document.getElementById("profiler-panel");
  const btnConfig    = document.getElementById("btn-config");
  const btnProfiler  = document.getElementById("btn-profiler");

  if (!configPanel || !profPanel || !btnConfig || !btnProfiler) {
    console.warn("[panels] Panel DOM elements not found — skipping initPanels.");
    return;
  }

  const configBody  = configPanel.querySelector(".panel-body");
  const profBody    = profPanel.querySelector(".panel-body");

  // ── Apply stored config ──────────────────────────────────────────────────
  const stored = loadStoredConfig();
  if (Object.keys(stored).length > 0) {
    const current = safeConfigJson(app);
    const knownIds = new Set(current.map(s => s.id));
    let needsRebuild = false;
    for (const [id, value] of Object.entries(stored)) {
      if (knownIds.has(id)) {
        try {
          // set_setting returns true for Live (already applied); false for
          // Reset/Reload — those only take effect after a fluid rebuild.
          const live = app.set_setting(id, value);
          if (!live) needsRebuild = true;
        } catch (e) {
          console.warn("[panels] failed to restore setting", id, e);
        }
      }
      // Unknown ids are silently ignored; missing ids fall back to Rust defaults.
    }
    // Rebuild the sim once so restored Reset-class values actually take effect.
    if (needsRebuild) app.reset();
  }

  // ── Render config panel ──────────────────────────────────────────────────
  buildConfigPanel(configBody, app);

  // Pressing the toolbar Reset applies pending Reset-class changes (Rust rebuilds
  // the fluid from settings), so clear the "reset to apply" badges by re-rendering.
  const toolbarReset = document.getElementById("btn-reset");
  if (toolbarReset) {
    toolbarReset.addEventListener("click", () => buildConfigPanel(configBody, app));
  }

  // ── Profiler polling loop (4×/sec) ───────────────────────────────────────
  buildProfilerPanel(profBody, app); // immediate first render
  setInterval(() => {
    buildProfilerPanel(profBody, app);
  }, 250);

  // ── Toggle visibility (each panel independently) ──────────────────────────
  let configVisible = true;
  let profVisible   = true;

  function setConfigVisible(v) {
    configVisible = v;
    configPanel.style.display = v ? "" : "none";
    btnConfig.classList.toggle("btn-active", v);
  }
  function setProfVisible(v) {
    profVisible = v;
    profPanel.style.display = v ? "" : "none";
    btnProfiler.classList.toggle("btn-active", v);
  }

  // Respect ?panels=off URL param (hides both)
  const params = new URLSearchParams(location.search);
  const startVisible = params.get("panels") !== "off";
  setConfigVisible(startVisible);
  setProfVisible(startVisible);

  btnConfig.addEventListener("click", () => setConfigVisible(!configVisible));
  btnProfiler.addEventListener("click", () => setProfVisible(!profVisible));
}
