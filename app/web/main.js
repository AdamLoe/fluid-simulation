// Plain-JS entry for static serving (Python http.server) — bypasses the bundler
// for verification/capture. Mirrors src/main.ts. The Vite/TS path remains the
// canonical build; this is the no-dependency verification path.
//
// URL params for capture control: ?pressure=off  ?paused=1  ?panels=off
// Exposes window.__fluid so the capture harness can drive controls.

import init, { FluidApp } from "./pkg/fluid_lab.js";
import { initPanels } from "./panels.js";

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

async function main() {
  const canvas = document.getElementById("gpu-canvas");
  if (!("gpu" in navigator)) {
    showUnsupported("navigator.gpu is missing — WebGPU is not supported in this browser.");
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

  // ── Observability panels (phase 1.2) ──
  initPanels(app);

  const params = new URLSearchParams(location.search);
  if (params.get("pressure") === "off") app.set_pressure_enabled(false);
  if (params.get("flip") !== null) app.set_flip_blend(parseFloat(params.get("flip")));
  if (params.get("slice") === "1") app.set_slice_enabled(true);
  if (params.get("slicemode") !== null) app.set_slice_mode(parseInt(params.get("slicemode"), 10));
  if (params.get("paused") === "1") {
    app.set_paused(true);
    document.getElementById("btn-pause").textContent = "Resume";
  }

  // Mesh toggle (URL param ?mesh=1)
  let meshEnabled = false;
  const meshBtn = document.getElementById("btn-mesh");
  function setMeshEnabled(on) {
    meshEnabled = on;
    app.set_mesh_enabled(on);
    meshBtn.classList.toggle("btn-active", on);
  }
  meshBtn.addEventListener("click", () => setMeshEnabled(!meshEnabled));
  if (params.get("mesh") === "1") setMeshEnabled(true);

  // controls
  const pauseBtn = document.getElementById("btn-pause");
  pauseBtn.addEventListener("click", () => {
    app.set_paused(!app.is_paused());
    pauseBtn.textContent = app.is_paused() ? "Resume" : "Pause";
  });
  document.getElementById("btn-reset").addEventListener("click", () => app.reset());

  // Interaction modes — number keys 1..4 select them, in mode-bar order.
  const modeOrder = ["camera", "rotate", "rotateRoll", "slosh"];
  let mode = "camera";
  const modeBtns = {
    camera:     document.getElementById("mode-camera"),
    rotate:     document.getElementById("mode-rotate"),
    rotateRoll: document.getElementById("mode-rotate-roll"),
    slosh:      document.getElementById("mode-slosh"),
  };
  const modeCursor = { camera: "grab", rotate: "ew-resize", rotateRoll: "ns-resize", slosh: "move" };
  function setMode(m) {
    if (!modeBtns[m]) return;
    mode = m;
    for (const k in modeBtns) modeBtns[k].classList.toggle("mode-active", k === m);
    canvas.style.cursor = modeCursor[m] || "default";
  }
  for (const k in modeBtns) modeBtns[k].addEventListener("click", () => setMode(k));

  // Number keys 1..N select modes; r resets the sim (ignored while typing in a config field).
  window.addEventListener("keydown", (e) => {
    const t = e.target;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA")) return;
    if (e.key === "r" || e.key === "R") { app.reset(); return; }
    const idx = parseInt(e.key, 10) - 1;
    if (idx >= 0 && idx < modeOrder.length) setMode(modeOrder[idx]);
  });

  let dragging = false, lastX = 0, lastY = 0;
  canvas.addEventListener("pointerdown", (e) => {
    dragging = true; lastX = e.clientX; lastY = e.clientY; canvas.setPointerCapture(e.pointerId);
  });
  canvas.addEventListener("pointerup", (e) => {
    dragging = false; canvas.releasePointerCapture(e.pointerId);
  });
  canvas.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    const dx = e.clientX - lastX, dy = e.clientY - lastY;
    if (mode === "camera") app.camera_orbit(dx, dy);
    else if (mode === "rotate") app.rotate_box(dx, dy);
    else if (mode === "rotateRoll") app.rotate_box_roll(dx, dy);
    else if (mode === "slosh") app.slosh_box(dx, dy);
    lastX = e.clientX; lastY = e.clientY;
  });
  canvas.addEventListener("wheel", (e) => { e.preventDefault(); app.camera_zoom(e.deltaY); }, { passive: false });

  setMode("camera");

  // resize
  const applyResize = () => { sizeCanvas(canvas); app.resize(canvas.width, canvas.height); };
  new ResizeObserver(applyResize).observe(canvas);
  window.addEventListener("resize", applyResize);

  // frame loop (TS/JS owns rAF; Rust owns the frame)
  // Throttled by render.fps_target (0 = uncapped).
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

  console.log("[fluid-lab] shell running (static).");
}

main();
