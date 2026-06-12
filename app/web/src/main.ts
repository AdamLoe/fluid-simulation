// Orphaned legacy TypeScript shell. The live app uses web/main.js + panels.js.
//
// Responsibilities, kept deliberately minimal (orchestrator_rules.md Rule 9):
//   - feature-detect WebGPU and show a clean unsupported message,
//   - load the wasm module and construct the single Rust `FluidApp`,
//   - own requestAnimationFrame and call `app.frame(dt)` (frame-loop ownership:
//     JavaScript drives rAF; Rust owns all sim state),
//   - forward pause/reset/step and camera pointer input to Rust,
//   - keep the canvas sized to the device pixel ratio.

import init, { FluidApp } from "../pkg/fluid_lab.js";

function showUnsupported(detail: string): void {
  const el = document.getElementById("unsupported")!;
  el.style.display = "grid";
  document.getElementById("unsupported-detail")!.textContent = detail;
  console.error("[fluid-lab] " + detail);
}

async function main(): Promise<void> {
  const canvas = document.getElementById("gpu-canvas") as HTMLCanvasElement;

  if (!("gpu" in navigator)) {
    showUnsupported("navigator.gpu is missing — WebGPU is not supported in this browser.");
    return;
  }

  sizeCanvas(canvas);

  await init();

  let app: FluidApp;
  try {
    app = await FluidApp.create(canvas);
  } catch (e) {
    showUnsupported("WebGPU initialization failed: " + String(e));
    return;
  }

  wireControls(app);
  wireCamera(app, canvas);
  wireResize(app, canvas);

  // --- frame loop (TS owns rAF, Rust owns the frame) ---
  let last = performance.now();
  const loop = (now: number): void => {
    const dtMs = now - last;
    last = now;
    app.frame(dtMs);
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);

  console.log("[fluid-lab] shell running — see boot diagnostics + profiler logs above/below.");
}

function sizeCanvas(canvas: HTMLCanvasElement): void {
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.floor(canvas.clientWidth * dpr));
  const h = Math.max(1, Math.floor(canvas.clientHeight * dpr));
  canvas.width = w;
  canvas.height = h;
}

function wireControls(app: FluidApp): void {
  const pauseBtn = document.getElementById("btn-pause") as HTMLButtonElement;
  pauseBtn.addEventListener("click", () => {
    app.set_paused(!app.is_paused());
    pauseBtn.textContent = app.is_paused() ? "Resume" : "Pause";
  });
  document
    .getElementById("btn-step")!
    .addEventListener("click", () => app.step());
  document
    .getElementById("btn-reset")!
    .addEventListener("click", () => app.reset());
}

function wireCamera(app: FluidApp, canvas: HTMLCanvasElement): void {
  let dragging = false;
  let lastX = 0;
  let lastY = 0;

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
  canvas.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    const dx = e.clientX - lastX;
    const dy = e.clientY - lastY;
    lastX = e.clientX;
    lastY = e.clientY;
    app.camera_orbit(dx, dy);
  });
  canvas.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      app.camera_zoom(e.deltaY);
    },
    { passive: false },
  );
}

function wireResize(app: FluidApp, canvas: HTMLCanvasElement): void {
  const apply = (): void => {
    sizeCanvas(canvas);
    app.resize(canvas.width, canvas.height);
  };
  const ro = new ResizeObserver(apply);
  ro.observe(canvas);
  window.addEventListener("resize", apply);
}

main();
