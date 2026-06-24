# fluid-lab

A browser-native fluid sim in Rust, WASM, and WebGPU.

This is real 3D water simulation running in a tab: hundreds of thousands of
particles feeding a MAC grid, a pressure solve every step, and a renderer turning
that moving volume back into water you can orbit, slice, and inspect.

[![fluid-lab water simulation demo](media/demo/poster.png)](media/demo/max-density-30s-60fps-128cube.mp4)

[Watch the 30 second demo MP4](media/demo/max-density-30s-60fps-128cube.mp4).

`fluid-lab` runs a bounded-tank FLIP/PIC liquid simulation in the browser. Particles
carry the splash, a MAC grid handles pressure and velocity, and the UI can flip
between water, particle, and grid-slice views. The settings panel is live, and the
profiler reports GPU timing when the browser exposes it.

## Working In This Repo

Use the agents for code work. The README is a front door, not the project map.

Fresh agents should start at [docs/index.md](docs/index.md). It routes to the current
architecture docs, build/run workflow, testing notes, and the plan layer. The app code
root is `app/`.
