use nexus_core::World;
use nexus_platform::WindowConfig;
use nexus_render::{RenderCommand, Renderer};
use nexus_render_vulkan::VulkanRenderer;

fn main() -> Result<(), String> {
    let window = WindowConfig::default();
    let mut world = World::default();
    world.spawn();

    let mut renderer =
        VulkanRenderer::new().map_err(|error| format!("renderer init failed: {error:?}"))?;
    renderer
        .begin_frame()
        .map_err(|error| format!("begin frame failed: {error:?}"))?;
    renderer
        .submit(RenderCommand::Clear {
            color: [0.04, 0.05, 0.07, 1.0],
        })
        .map_err(|error| format!("submit failed: {error:?}"))?;
    renderer
        .end_frame()
        .map_err(|error| format!("end frame failed: {error:?}"))?;

    println!(
        "{} {}x{} | {:?} {}.{} | entities: {}",
        window.title,
        window.width,
        window.height,
        renderer.backend(),
        renderer.device_info().api_major,
        renderer.device_info().api_minor,
        world.entity_count(),
    );

    Ok(())
}
