#![forbid(unsafe_code)]

#[derive(Clone, Debug)]
pub struct WindowConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: String::from("Nexus WorldEditor"),
            width: 1280,
            height: 720,
        }
    }
}
