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

const CONTROL_TARGET_ORDER = ["camera", "cube"];
const CONTROL_TARGET_CURSOR = {
  camera: "grab",
  cube: "move",
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
  let controlTarget = "camera";
  let dragging = false;
  let dragButton = 0;
  let lastX = 0;
  let lastY = 0;

  const productModeBtns = {
    autoRotate: document.getElementById("product-auto-rotate"),
    waves: document.getElementById("product-waves"),
    manual: document.getElementById("product-manual"),
  };
  const controlTargetBtns = {
    camera: document.getElementById("control-camera"),
    cube: document.getElementById("control-cube"),
  };

  function syncControlUi() {
    for (const [targetId, btn] of Object.entries(controlTargetBtns)) {
      const selected = targetId === controlTarget;
      btn.classList.toggle("mode-active", selected);
      btn.setAttribute("aria-pressed", selected ? "true" : "false");
    }
    canvas.style.cursor = CONTROL_TARGET_CURSOR[controlTarget] || "default";
  }

  function setControlTarget(nextTarget) {
    if (!controlTargetBtns[nextTarget]) return;
    controlTarget = nextTarget;
    syncControlUi();
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
    panelApi?.rerenderModes();
  }

  for (const [modeId, btn] of Object.entries(productModeBtns)) {
    btn.addEventListener("click", () => applyProductMode(modeId));
  }

  for (const [targetId, btn] of Object.entries(controlTargetBtns)) {
    btn.addEventListener("click", () => setControlTarget(targetId));
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
    const idx = parseInt(e.key, 10) - 1;
    if (idx >= 0 && idx < CONTROL_TARGET_ORDER.length) {
      setControlTarget(CONTROL_TARGET_ORDER[idx]);
    }
  });

  canvas.addEventListener("contextmenu", (e) => e.preventDefault());
  canvas.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    dragging = true;
    dragButton = e.button;
    lastX = e.clientX;
    lastY = e.clientY;
    canvas.setPointerCapture(e.pointerId);
  });
  canvas.addEventListener("pointerup", (e) => {
    if (e.buttons === 0) {
      dragging = false;
      try {
        canvas.releasePointerCapture(e.pointerId);
      } catch {}
    }
  });
  canvas.addEventListener("pointercancel", (e) => {
    dragging = false;
    try {
      canvas.releasePointerCapture(e.pointerId);
    } catch {}
  });
  canvas.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    e.preventDefault();
    const dx = e.clientX - lastX;
    const dy = e.clientY - lastY;
    if (controlTarget === "camera") {
      if (dragButton === 1) app.camera_pan(dx, dy);
      else if (dragButton === 2) app.camera_twist(dx, dy);
      else app.camera_orbit(dx, dy);
    } else if (controlTarget === "cube") {
      if (dragButton === 1) app.move_box(dx, dy);
      else if (dragButton === 2) app.rotate_box_roll(dx, dy);
      else app.rotate_box(dx, dy);
    }
    lastX = e.clientX;
    lastY = e.clientY;
  });
  canvas.addEventListener("wheel", (e) => {
    e.preventDefault();
    app.camera_zoom(e.deltaY);
  }, { passive: false });

  setControlTarget("camera");
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
    openSettings(tab = "scenario") {
      panelApi?.openSettings(tab);
    },
    closeSettings() {
      panelApi?.closeSettings();
    },
    selectSettingsTab(tab) {
      if (!panelApi?.isOpen()) {
        panelApi?.openSettings(tab);
      } else {
        panelApi?.setActiveTab(tab);
      }
    },
    openWorkspace(tab = "scenario") {
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
    selectControlTarget(target) {
      setControlTarget(target);
    },
    selectManualPointerMode(mode) {
      const alias = mode === "camera" ? "camera" : "cube";
      setControlTarget(alias);
    },
    reset() {
      resetSimulation();
    },
    state() {
      return {
        settingsOpen: panelApi?.isOpen() ?? false,
        settingsTab: panelApi?.activeTab() ?? "scenario",
        workspaceOpen: panelApi?.isOpen() ?? false,
        workspaceTab: panelApi?.activeTab() ?? "scenario",
        productMode,
        controlTarget,
        manualPointerMode: controlTarget,
        paused: app.is_paused(),
      };
    },
  };

  console.log("[fluid-lab] shell running (static).");
}

main();
