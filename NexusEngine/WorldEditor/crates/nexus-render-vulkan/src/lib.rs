#![forbid(unsafe_code)]

use nexus_render::{RenderBackend, RenderCommand, RenderDeviceInfo, RenderError, Renderer};

#[derive(Debug)]
pub struct VulkanRenderer {
    device_info: RenderDeviceInfo,
    frame_open: bool,
}

impl VulkanRenderer {
    pub fn new() -> Result<Self, RenderError> {
        Ok(Self {
            device_info: RenderDeviceInfo {
                api_major: 1,
                api_minor: 2,
                is_integrated: true,
            },
            frame_open: false,
        })
    }
}

impl Renderer for VulkanRenderer {
    fn backend(&self) -> RenderBackend {
        RenderBackend::Vulkan
    }

    fn device_info(&self) -> RenderDeviceInfo {
        self.device_info
    }

    fn begin_frame(&mut self) -> Result<(), RenderError> {
        if self.frame_open {
            return Err(RenderError::FrameFailed(String::from("frame already open")));
        }
        self.frame_open = true;
        Ok(())
    }

    fn submit(&mut self, _command: RenderCommand) -> Result<(), RenderError> {
        if !self.frame_open {
            return Err(RenderError::FrameFailed(String::from("frame is not open")));
        }
        Ok(())
    }

    fn end_frame(&mut self) -> Result<(), RenderError> {
        if !self.frame_open {
            return Err(RenderError::FrameFailed(String::from("frame is not open")));
        }
        self.frame_open = false;
        Ok(())
    }
}
