// Plain-JS entry for static serving (Python http.server) - bypasses the bundler
// for verification/capture. Mirrors src/main.ts. The Vite/TS path remains the
// canonical build; this is the no-dependency verification path.
//
// URL params for capture control: ?pressure=off  ?paused=1
// Exposes window.__fluid and window.__fluidShell so the capture harness can drive controls.

import init, { FluidApp } from "./pkg/fluid_lab.js";
import { initPanels } from "./panels.js";

const PRODUCT_MODES = {
  autoRotate: {
    autoRollEnabled: 1,
    waveEnabled: 0,
  },
  waves: {
    autoRollEnabled: 0,
    waveEnabled: 1,
  },
  manual: {
    autoRollEnabled: 0,
    waveEnabled: 0,
  },
};

const POINTER_MODE_ORDER = ["camera", "rotate", "rotateRoll", "slosh"];
const POINTER_MODE_CURSOR = {
  camera: "grab",
  rotate: "ew-resize",
  rotateRoll: "ns-resize",
  slosh: "move",
};

function showUnsupported(detail) {
  const el = document.getElementById("unsupported");
  el.style.display = "grid";
  document.getElementById("unsupported-detail").textContent = detail;
  console.error("[fluid-lab] " + detail);
}

function sizeCanvas(canvas) {
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.max(1, Math.floor(canvas.clientWidth * dpr));
  canvas.height = Math.max(1, Math.floor(canvas.clientHeight * dpr));
}

async function loadVersionLabel() {
  try {
    const res = await fetch("./package.json", { cache: "no-store" });
    if (!res.ok) return "";
    const pkg = await res.json();
    return pkg && pkg.version ? `v${pkg.version}` : "";
  } catch {
    return "";
  }
}

function setPauseButtonState(button, paused) {
  button.setAttribute("aria-label", paused ? "Resume simulation" : "Pause simulation");
  button.title = paused ? "Resume simulation" : "Pause simulation";
  button.classList.toggle("btn-active", paused);
  button.innerHTML = paused
    ? `<svg viewBox="0 0 24 24" aria-hidden="true">
         <path d="M8 5.5l10 6.5-10 6.5z" fill="currentColor"></path>
       </svg>
       <span class="visually-hidden">Resume</span>`
    : `<svg viewBox="0 0 24 24" aria-hidden="true">
         <path d="M8 5h3v14H8zM13 5h3v14h-3z" fill="currentColor"></path>
       </svg>
       <span class="visually-hidden">Pause</span>`;
}

async function main() {
  const canvas = document.getElementById("gpu-canvas");
  const pauseBtn = document.getElementById("btn-pause");
  const resetBtn = document.getElementById("btn-reset");
  const versionEl = document.getElementById("app-version");
  const manualPointerGroup = document.getElementById("manual-pointer-group");

  if (!("gpu" in navigator)) {
    showUnsupported("navigator.gpu is missing - WebGPU is not supported in this browser.");
    return;
  }
  sizeCanvas(canvas);
  await init();

  let app;
  try {
    app = await FluidApp.create(canvas);
  } catch (e) {
    showUnsupported("WebGPU initialization failed: " + String(e));
    return;
  }
  window.__fluid = app;

  if (versionEl) {
    versionEl.textContent = await loadVersionLabel();
  }

  const panelApi = initPanels(app);
  const params = new URLSearchParams(location.search);
  if (params.get("pressure") === "off") app.set_pressure_enabled(false);
  if (params.get("flip") !== null) app.set_flip_blend(parseFloat(params.get("flip")));
  if (params.get("slice") === "1") app.set_slice_enabled(true);
  if (params.get("slicemode") !== null) app.set_slice_mode(parseInt(params.get("slicemode"), 10));

  let productMode = "autoRotate";
  let manualPointerMode = "camera";
  let dragging = false;
  let lastX = 0;
  let lastY = 0;

  const productModeBtns = {
    autoRotate: document.getElementById("product-auto-rotate"),
    waves: document.getElementById("product-waves"),
    manual: document.getElementById("product-manual"),
  };
  const pointerModeBtns = {
    camera: document.getElementById("mode-camera"),
    rotate: document.getElementById("mode-rotate"),
    rotateRoll: document.getElementById("mode-rotate-roll"),
    slosh: document.getElementById("mode-slosh"),
  };

  function activePointerMode() {
    return productMode === "manual" ? manualPointerMode : "camera";
  }

  function syncPointerUi() {
    const manualVisible = productMode === "manual";
    manualPointerGroup.hidden = !manualVisible;
    const activeMode = activePointerMode();
    for (const [modeId, btn] of Object.entries(pointerModeBtns)) {
      btn.classList.toggle("mode-active", manualVisible && modeId === manualPointerMode);
      btn.setAttribute("aria-pressed", manualVisible && modeId === manualPointerMode ? "true" : "false");
    }
    canvas.style.cursor = POINTER_MODE_CURSOR[activeMode] || "default";
  }

  function setManualPointerMode(nextMode) {
    if (!pointerModeBtns[nextMode]) return;
    manualPointerMode = nextMode;
    syncPointerUi();
  }

  function applyProductMode(nextMode) {
    if (!PRODUCT_MODES[nextMode]) return;
    productMode = nextMode;
    const scheduler = PRODUCT_MODES[nextMode];
    app.set_setting("interaction.auto_roll_enabled", scheduler.autoRollEnabled);
    app.set_setting("interaction.wave_enabled", scheduler.waveEnabled);

    for (const [modeId, btn] of Object.entries(productModeBtns)) {
      const selected = modeId === nextMode;
      btn.classList.toggle("mode-active", selected);
      btn.setAttribute("aria-pressed", selected ? "true" : "false");
    }
    syncPointerUi();
    panelApi?.rerenderModes();
  }

  for (const [modeId, btn] of Object.entries(productModeBtns)) {
    btn.addEventListener("click", () => applyProductMode(modeId));
  }

  for (const [modeId, btn] of Object.entries(pointerModeBtns)) {
    btn.addEventListener("click", () => {
      if (productMode === "manual") setManualPointerMode(modeId);
    });
  }

  if (params.get("paused") === "1") {
    app.set_paused(true);
  }
  setPauseButtonState(pauseBtn, app.is_paused());

  pauseBtn.addEventListener("click", () => {
    app.set_paused(!app.is_paused());
    setPauseButtonState(pauseBtn, app.is_paused());
  });

  function resetSimulation() {
    app.reset();
    applyProductMode(productMode);
    panelApi?.rerender();
  }

  resetBtn.addEventListener("click", resetSimulation);

  window.addEventListener("keydown", (e) => {
    const t = e.target;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT")) {
      return;
    }
    if (e.key === "r" || e.key === "R") {
      resetSimulation();
      return;
    }
    if (productMode !== "manual") return;
    const idx = parseInt(e.key, 10) - 1;
    if (idx >= 0 && idx < POINTER_MODE_ORDER.length) {
      setManualPointerMode(POINTER_MODE_ORDER[idx]);
    }
  });

  canvas.addEventListener("pointerdown", (e) => {
    dragging = true;
    lastX = e.clientX;
    lastY = e.clientY;
    canvas.setPointerCapture(e.pointerId);
  });
  canvas.addEventListener("pointerup", (e) => {
    dragging = false;
    canvas.releasePointerCapture(e.pointerId);
  });
  canvas.addEventListener("pointercancel", (e) => {
    dragging = false;
    canvas.releasePointerCapture(e.pointerId);
  });
  canvas.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    const dx = e.clientX - lastX;
    const dy = e.clientY - lastY;
    const mode = activePointerMode();
    if (mode === "camera") app.camera_orbit(dx, dy);
    else if (mode === "rotate") app.rotate_box(dx, dy);
    else if (mode === "rotateRoll") app.rotate_box_roll(dx, dy);
    else if (mode === "slosh") app.slosh_box(dx, dy);
    lastX = e.clientX;
    lastY = e.clientY;
  });
  canvas.addEventListener("wheel", (e) => {
    e.preventDefault();
    app.camera_zoom(e.deltaY);
  }, { passive: false });

  setManualPointerMode("camera");
  applyProductMode("autoRotate");

  const applyResize = () => {
    sizeCanvas(canvas);
    app.resize(canvas.width, canvas.height);
  };
  new ResizeObserver(applyResize).observe(canvas);
  window.addEventListener("resize", applyResize);

  let last = performance.now();
  const loop = (now) => {
    const target = app.fps_target();
    const minMs = target > 0 ? 1000 / target : 0;
    const dt = now - last;
    if (dt >= minMs) {
      app.frame(dt);
      last = now;
    }
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);

  window.__fluidShell = {
    openWorkspace(tab = "general") {
      panelApi?.openWorkspace(tab);
    },
    closeWorkspace() {
      panelApi?.closeWorkspace();
    },
    selectWorkspaceTab(tab) {
      if (!panelApi?.isOpen()) {
        panelApi?.openWorkspace(tab);
      } else {
        panelApi?.setActiveTab(tab);
      }
    },
    selectProductMode(mode) {
      applyProductMode(mode);
    },
    selectManualPointerMode(mode) {
      if (productMode === "manual") setManualPointerMode(mode);
    },
    reset() {
      resetSimulation();
    },
    state() {
      return {
        workspaceOpen: panelApi?.isOpen() ?? false,
        workspaceTab: panelApi?.activeTab() ?? "general",
        productMode,
        manualPointerMode,
        paused: app.is_paused(),
      };
    },
  };

  console.log("[fluid-lab] shell running (static).");
}

main();
