#![forbid(unsafe_code)]

use nexus_core::Vec2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderBackend {
    Vulkan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderFeature {
    Compute,
    GeometryShader,
    Tessellation,
    RayTracing,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderDeviceInfo {
    pub api_major: u32,
    pub api_minor: u32,
    pub is_integrated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RenderCommand {
    Clear {
        color: [f32; 4],
    },
    DrawRect {
        position: Vec2,
        size: Vec2,
        color: [f32; 4],
    },
}

#[derive(Debug)]
pub enum RenderError {
    InitializationFailed(String),
    FrameFailed(String),
}

pub trait Renderer {
    fn backend(&self) -> RenderBackend;
    fn device_info(&self) -> RenderDeviceInfo;
    fn begin_frame(&mut self) -> Result<(), RenderError>;
    fn submit(&mut self, command: RenderCommand) -> Result<(), RenderError>;
    fn end_frame(&mut self) -> Result<(), RenderError>;
}
