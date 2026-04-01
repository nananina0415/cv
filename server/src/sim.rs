use serde::{Deserialize, Serialize};

// ── UserIn ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TouchStartPayload {
    #[serde(rename = "targetPartIndex")]
    pub target_part_index: f32,
    #[serde(rename = "actionPoint")]
    pub action_point: Vec3,
    #[serde(rename = "fingerPoint")]
    pub finger_point: Vec3,
    pub z_direction: Vec3,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TouchingPayload {
    #[serde(rename = "fingerPoint")]
    pub finger_point: Vec3,
    pub z_direction: Vec3,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TouchEndPayload {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum UserIn {
    TouchStart(TouchStartPayload),
    Touching(TouchingPayload),
    TouchEnd(TouchEndPayload),
}

// ── SimOut ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ObjectTransform {
    pub name: String,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SimOut {
    pub timestamp: f64,
    pub objects: Vec<ObjectTransform>,
}

// ── SimThread ─────────────────────────────────────────────────────────────────

pub struct SimThread;

impl SimThread {
    pub fn new() -> SimThread {
        SimThread
    }
}
