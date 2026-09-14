#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Entity(u32);

#[derive(Debug, Default)]
pub struct World {
    next_entity: u32,
}

impl World {
    pub fn spawn(&mut self) -> Entity {
        let entity = Entity(self.next_entity);
        self.next_entity = self.next_entity.saturating_add(1);
        entity
    }

    pub fn entity_count(&self) -> u32 {
        self.next_entity
    }
}
