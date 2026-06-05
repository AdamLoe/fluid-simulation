import { defineConfig } from "vite";

// Thin dev/build config. The wasm-bindgen `--target web` glue in ./pkg is loaded
// as a normal ES module; Vite serves fluid_lab_bg.wasm as an asset. No COOP/COEP
// headers needed yet (no wasm threads until/unless a later phase requires them).
export default defineConfig({
  server: {
    host: true,
    // Dedicated strict port — this machine runs other projects' dev servers on
    // 5173/5174/5175, so bind our own and fail loudly on collision.
    port: 5184,
    strictPort: true,
    fs: {
      // Allow importing the generated ../pkg sibling directory.
      allow: [".."],
    },
  },
  // Don't let Vite try to pre-bundle/inline the wasm glue.
  optimizeDeps: {
    exclude: ["fluid-lab"],
  },
});
