 # Nexus WorldEditor

Initial Rust workspace for the Nexus client and world editor.

## Workspace layout

```text
crates/nexus-core          Shared math and world data
crates/nexus-platform      Window and OS integration boundary
crates/nexus-render        Renderer-independent render contract
crates/nexus-render-vulkan Vulkan renderer implementation boundary
apps/world-editor          Editor application
```

## Current status

The workspace currently validates the ownership boundaries and frame lifecycle
without creating a native Vulkan device yet. `nexus-render-vulkan` is a
placeholder implementation so the application can be developed incrementally.
The reported Vulkan device information is temporary and must be replaced by
actual Vulkan physical-device enumeration before rendering real frames.

## Build and run

```bash
cargo check --workspace
cargo run -p world-editor
```

## Planned implementation order

1. Add native Vulkan instance and physical-device enumeration.
2. Add platform window and surface creation.
3. Add swapchain, command buffers, synchronization, and frame submission.
4. Add 2D orthographic rendering before 3D features.
5. Add feature tiers based on the selected physical device.
.
