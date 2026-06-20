// Canonical static entry for local verification/capture and production serving.
// The old Vite/TS stub is not loaded by index.html.
//
// URL params for capture control: ?pressure=off  ?paused=1  ?set=id:value
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
const READY_CONTROL_SELECTOR = "#toolbar button, #launcher button";
const CAMERA_STORAGE_KEY = "fluidlab.config.v1";
const KEYBOARD_DRAG_STEP = 28;
const KEYBOARD_ZOOM_STEP = 160;

function setShellReady(ready, canvas = null) {
  const appRoot = document.getElementById("app");
  if (appRoot) {
    appRoot.dataset.shellReady = ready ? "true" : "false";
    appRoot.setAttribute("aria-busy", ready ? "false" : "true");
  }
  for (const control of document.querySelectorAll(READY_CONTROL_SELECTOR)) {
    control.disabled = !ready;
  }
  if (canvas) {
    canvas.tabIndex = ready ? 0 : -1;
    if (!ready && document.activeElement === canvas) canvas.blur();
  }
}

function showUnsupported(detail, title = "WebGPU is not available") {
  const el = document.getElementById("unsupported");
  const appRoot = document.getElementById("app");
  setShellReady(false, document.getElementById("gpu-canvas"));
  if (appRoot) {
    appRoot.inert = true;
    appRoot.setAttribute("aria-hidden", "true");
    appRoot.setAttribute("aria-busy", "false");
  }
  el.hidden = false;
  const titleEl = document.getElementById("unsupported-title");
  if (titleEl) titleEl.textContent = title;
  document.getElementById("unsupported-detail").textContent = detail;
  window.requestAnimationFrame(() => el.focus({ preventScroll: true }));
  console.error("[fluid-lab] " + detail);
}

function gpuDeviceStatus(app) {
  return typeof app.gpu_device_status === "function" ? app.gpu_device_status() : "unknown";
}

function fatalGpuStatus(status) {
  return status === "device-lost" || status === "surface-validation-error";
}

function sizeCanvas(canvas) {
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.max(1, Math.floor(canvas.clientWidth * dpr));
  canvas.height = Math.max(1, Math.floor(canvas.clientHeight * dpr));
}

async function loadVersionLabel() {
  for (const path of ["./pkg/package.json", "./package.json"]) {
    try {
      const res = await fetch(path, { cache: "no-store" });
      if (!res.ok) continue;
      const pkg = await res.json();
      if (pkg && pkg.version) return `v${pkg.version}`;
    } catch {}
  }
  return "";
}

function finiteParam(params, name) {
  const raw = params.get(name);
  if (raw === null) return null;
  if (raw.trim() === "") return null;
  const value = Number(raw);
  return Number.isFinite(value) ? value : null;
}

function parseRegistryUrlSettings(params) {
  const entries = [];
  for (const raw of params.getAll("set")) {
    const sep = raw.indexOf(":");
    if (sep <= 0 || sep === raw.length - 1) {
      console.warn("[fluid-lab] ignored malformed set URL param:", raw);
      continue;
    }
    const id = raw.slice(0, sep);
    const value = Number(raw.slice(sep + 1));
    entries.push([id, value]);
  }
  return entries;
}

function storedCameraDistancePresent() {
  try {
    const raw = localStorage.getItem(CAMERA_STORAGE_KEY);
    if (!raw) return false;
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" && (
      "camera.distance" in parsed ||
      (parsed.settings && typeof parsed.settings === "object" && "camera.distance" in parsed.settings)
    );
  } catch {
    return false;
  }
}

function urlCameraDistancePresent(entries) {
  return entries.some(([id]) => id === "camera.distance");
}

function shouldApplyMobileInitialFit(urlSettings) {
  if (urlCameraDistancePresent(urlSettings) || storedCameraDistancePresent()) return false;
  return window.matchMedia("(max-width: 560px), (max-width: 820px) and (orientation: portrait)").matches;
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
  setShellReady(false, canvas);

  if (!("gpu" in navigator)) {
    showUnsupported("navigator.gpu is missing - WebGPU is not supported in this browser.");
    return;
  }
  let app;
  try {
    sizeCanvas(canvas);
    await init();
    app = await FluidApp.create(canvas);
  } catch (e) {
    showUnsupported("WebGPU initialization failed: " + String(e));
    return;
  }
  window.__fluid = app;
  window.__fluidGpuStatus = () => gpuDeviceStatus(app);

  if (versionEl) {
    versionEl.textContent = await loadVersionLabel();
  }

  const panelApi = initPanels(app);
  const params = new URLSearchParams(location.search);
  if (params.get("pressure") === "off") app.set_pressure_enabled(false);
  const canonicalUrlSettings = parseRegistryUrlSettings(params);
  const urlSettings = [];
  const flip = finiteParam(params, "flip");
  if (flip !== null && !canonicalUrlSettings.some(([id]) => id === "physics.flip_blend")) {
    urlSettings.push(["physics.flip_blend", flip]);
  }
  urlSettings.push(...canonicalUrlSettings);
  if (params.get("slice") === "1") app.set_slice_enabled(true);
  const sliceMode = finiteParam(params, "slicemode");
  if (sliceMode !== null) app.set_slice_mode(Math.trunc(sliceMode));

  let productMode = "autoRotate";
  let controlTarget = "camera";
  let stoppedForGpuStatus = false;
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
    if (!app.reset()) return false;
    applyProductMode(productMode);
    panelApi?.rerender();
    return true;
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
    canvas.focus({ preventScroll: true });
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
  canvas.addEventListener("keydown", (e) => {
    const keyDeltas = {
      ArrowLeft: [-KEYBOARD_DRAG_STEP, 0],
      ArrowRight: [KEYBOARD_DRAG_STEP, 0],
      ArrowUp: [0, -KEYBOARD_DRAG_STEP],
      ArrowDown: [0, KEYBOARD_DRAG_STEP],
    };
    if (e.key === "PageUp" || e.key === "+" || e.key === "=") {
      e.preventDefault();
      app.camera_zoom(-KEYBOARD_ZOOM_STEP);
      return;
    }
    if (e.key === "PageDown" || e.key === "-" || e.key === "_") {
      e.preventDefault();
      app.camera_zoom(KEYBOARD_ZOOM_STEP);
      return;
    }
    const delta = keyDeltas[e.key];
    if (!delta) return;
    e.preventDefault();
    const [dx, dy] = delta;
    if (controlTarget === "camera") {
      if (e.shiftKey) app.camera_pan(dx, dy);
      else if (e.altKey) app.camera_twist(dx, dy);
      else app.camera_orbit(dx, dy);
    } else if (controlTarget === "cube") {
      if (e.shiftKey) app.move_box(dx, dy);
      else if (e.altKey) app.rotate_box_roll(dx, dy);
      else app.rotate_box(dx, dy);
    }
  });

  setControlTarget("camera");
  applyProductMode("autoRotate");
  let urlApplyResult = null;
  if (urlSettings.length) urlApplyResult = panelApi?.applySettings(urlSettings, "url") || null;
  if (shouldApplyMobileInitialFit(urlSettings)) app.camera_zoom(320);

  const applyResize = () => {
    sizeCanvas(canvas);
    app.resize(canvas.width, canvas.height);
  };
  new ResizeObserver(applyResize).observe(canvas);
  window.addEventListener("resize", applyResize);

  let last = performance.now();
  const loop = (now) => {
    if (stoppedForGpuStatus) return;
    const target = app.fps_target();
    const minMs = target > 0 ? 1000 / target : 0;
    const dt = now - last;
    if (dt >= minMs) {
      app.frame(dt);
      last = now;
      const status = gpuDeviceStatus(app);
      if (fatalGpuStatus(status)) {
        stoppedForGpuStatus = true;
        showUnsupported(
          `GPU status: ${status}. Rendering has stopped; reload the page to request a new WebGPU device.`,
          "GPU rendering stopped",
        );
        return;
      }
    }
    requestAnimationFrame(loop);
  };
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
      return resetSimulation();
    },
    applySettings(entries, source = "shell") {
      return panelApi?.applySettings(entries, source) || null;
    },
    importConfigPayload(payload, source = "shell import") {
      return panelApi?.importConfigPayload(payload, source) || null;
    },
    exportConfig() {
      return panelApi?.exportConfig() || null;
    },
    shareUrl() {
      return panelApi?.shareUrl() || "";
    },
    setting(id) {
      return panelApi?.setting(id) || null;
    },
    setTheme(id) {
      return panelApi?.setTheme(id) || "default";
    },
    activeTheme() {
      return panelApi?.activeTheme() ?? "default";
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
        theme: panelApi?.activeTheme() ?? "default",
        paused: app.is_paused(),
        gpuDeviceStatus: gpuDeviceStatus(app),
        gpuStopped: stoppedForGpuStatus,
        urlApplyResult,
      };
    },
  };

  setShellReady(true, canvas);
  requestAnimationFrame(loop);
  console.log("[fluid-lab] shell running (static).");
}

main().catch((e) => showUnsupported("Application startup failed: " + String(e)));
