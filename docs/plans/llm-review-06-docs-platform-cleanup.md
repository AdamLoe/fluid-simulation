---
status:        shipped
owner:         codex
last_updated:  2026-06-12
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/gpu-resources.md
  - architecture/rendering.md
  - architecture/settings.md
  - architecture/web-shell.md
  - decisions/platform.md
  - decisions/scope.md
---

# LLM Review 06 - Docs Platform Cleanup

## Mission

Reduce docs drift from repeated removed-feature prose and give platform gaps, especially
device loss and surface loss, a clear owner. Done means current-state docs describe what
exists, compatibility/removal facts live in one place, and platform failure behavior is
documented honestly.

## Scope

In scope:

- A single removed-feature or compatibility registry if audits confirm repeated absence
  prose across docs.
- Removing duplicate "does not exist" prose from subsystem docs where it can be linked
  instead.
- Documenting current device-loss/surface-loss behavior and the desired recovery owner.
- Adding ownership entries if a new registry doc is created.

Out of scope:

- Full device-loss recovery implementation unless it is trivial and low risk.
- Rewriting architecture docs unrelated to removed features or platform behavior.

## Approach

1. Treat `architecture/rendering.md` as the canonical owner for removed render features
   unless ownership review says otherwise.
2. Replace duplicate removed-feature absence prose in `gpu-resources.md` and
   `settings.md` with short pointers to the canonical owner.
3. Document current surface-loss handling and the missing true device-loss status path.
4. If plan 02 adds device/status fields, migrate the durable facts into
   `gpu-resources.md` and `decisions/platform.md`; otherwise record the gap without
   claiming recovery.

## Subagents

- Read-only audit: observability/docs explorer.
- Worker: docs/platform cleanup. This worker may touch docs only unless a trivial
  device-loss hook is explicitly approved by the hub.

## Audit Notes

- The app currently handles `CurrentSurfaceTexture::Lost | Outdated` by recreating
  swapchain-sized targets and continuing; validation errors return an error.
- The docs do not describe a full WebGPU device-loss recovery path.
- Removed render features are source-absent and documented consistently, but
  no-caustics/temporal/wet-wall/wall-fill notes are repeated across rendering,
  gpu-resources, and settings docs.

## Exit Gate

- Docs links resolve by inspection.
- Targeted `rg` for removed feature terms shows they live in the canonical owner or
  necessary compatibility surfaces, not repeated subsystem prose.

## Migration Notes

This docs-only pass migrated durable facts without creating a new registry doc:

- Removed render-feature ownership -> `architecture/rendering.md` as the canonical
  owner. No `_meta/ownership.json` change was needed.
- Duplicate removed-target prose in `architecture/gpu-resources.md` now points to
  `architecture/rendering.md`.
- Settings legacy-id behavior remains in `architecture/settings.md`, with a pointer
  back to `architecture/rendering.md` for the removed feature set.
- Surface-loss current state -> `architecture/gpu-resources.md`: Lost/Outdated
  recreate swapchain-sized targets and continue; Validation returns an error.
- Platform policy -> `decisions/platform.md`: true WebGPU device loss is reload-only;
  in-place device rebuild remains future work.

Source-level status pass:

- `GpuContext` now tracks `gpu_device_status` as `ok`, transient `surface-lost`,
  `device-lost`, or `validation-error`.
- `CurrentSurfaceTexture::Lost | Outdated` set `surface-lost`, recreate
  swapchain-sized targets, skip the frame, and return to `ok` after the next
  successful surface acquisition.
- `CurrentSurfaceTexture::Validation` sets `validation-error` and returns an error.
- wgpu's device-lost callback sets `device-lost`; the app does not recreate the
  adapter/device/queue.
- `FluidApp::gpu_device_status()` exposes the status to JS. The shell includes it
  in `window.__fluidShell.state().gpuDeviceStatus`, stops scheduling frames on
  fatal statuses, and shows reload guidance through the existing WebGPU overlay.
- `tools/capture.mjs` records final shell state and fails when final
  `gpuDeviceStatus` is `device-lost`/`validation-error` or console output reports
  WebGPU device loss/validation failures.

Deferred:

- Full WebGPU device recovery remains out of scope. Recovery still means page reload
  and fresh WebGPU initialization.
