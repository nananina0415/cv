use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, atomic::{AtomicU8, Ordering}};
use crate::utils::{TripleBufReader, TripleBufWriter, TripleBufSwapper};

// ── SimIoBuf ──────────────────────────────────────────────────────────────────

pub struct SimIoBuf {
    pub userin_r:    TripleBufReader<Vec<UserIn>>,
    pub userin_swap: TripleBufSwapper<Vec<UserIn>>,
    pub simout_w:    TripleBufWriter<SimOut>,
    pub simout_swap: TripleBufSwapper<SimOut>,
}

impl SimIoBuf {
    pub fn clear_and_init(&mut self, init: SimOut) {
        self.userin_swap.swap_and_clear();
        *self.simout_w.write() = init;
        self.simout_swap.swap_and_clear();
    }
}

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

impl crate::utils::Clearable for SimOut {
    fn clear(&mut self) { self.objects.clear(); }
}

// ── Simulator ─────────────────────────────────────────────────────────────────

pub struct SimModel {
    // TODO: 폴더에서 로드한 기구 목록, 조인트 등
}

pub struct Simulator {
    // TODO: pyo3 Python simulator 래퍼
}

impl Simulator {
    pub fn new() -> Self { todo!() }
    pub fn model(&mut self, _model: SimModel) -> &mut Self { todo!() }
    pub fn fps(&mut self, _fps: u32) -> &mut Self { todo!() }
    pub fn step(&mut self, _inputs: &[UserIn]) -> Vec<ObjectTransform> { todo!() }
}

fn load_model_from_folder(_folder: &PathBuf) -> (SimModel, SimOut) {
    todo!("폴더에서 시뮬 모델 로드")
}

// ── SimThread ─────────────────────────────────────────────────────────────────

const FLAG_RUN:   u8 = 0;
const FLAG_PAUSE: u8 = 1;
const FLAG_HALT:  u8 = 2;

pub struct SimThread {
    thread_handle: std::thread::JoinHandle<SimIoBuf>,
    _watchdog: crate::watchdog::Watchdog<(SimModel, SimOut)>,
    flag: Arc<AtomicU8>,
    cond: Arc<(Mutex<()>, Condvar)>,
}

impl SimThread {
    pub fn new(folder: PathBuf, mut sim_io_buf: SimIoBuf) -> SimThread {
        let (model, init) = load_model_from_folder(&folder);
        sim_io_buf.clear_and_init(init);

        let flag = Arc::new(AtomicU8::new(FLAG_RUN));
        let cond = Arc::new((Mutex::new(()), Condvar::new()));

        let watchdog = {
            let flag = flag.clone();
            let cond = cond.clone();
            crate::watchdog::Watchdog::new(folder, move |new_folder, data| {
                let (m, init) = load_model_from_folder(&new_folder);
                *data.lock().expect("watchdog data mutex poisoned") = Some((m, init));
                flag.store(FLAG_PAUSE, Ordering::Relaxed);
                cond.1.notify_one();
            }).expect("watchdog 생성 실패")
        };

        let thread_handle = std::thread::spawn({
            let flag = flag.clone();
            let cond = cond.clone();
            let reload_data = watchdog.data.clone();
            move || {
                let mut simulator = Simulator::new();
                simulator.model(model).fps(60);
                loop {
                    match flag.load(Ordering::Relaxed) {
                        FLAG_RUN => {
                            // 1. UserIn 읽기
                            sim_io_buf.userin_swap.swap_and_clear();
                            let inputs = sim_io_buf.userin_r.read();

                            // 2. 시뮬 스텝 실행 → SimOut 쓰기
                            let objects = simulator.step(inputs);
                            let out = sim_io_buf.simout_w.write();
                            out.objects = objects;
                            sim_io_buf.simout_swap.swap_and_clear();
                        }
                        FLAG_PAUSE => {
                            let (lock, cv) = &*cond;
                            let guard = lock.lock().expect("cond mutex poisoned");
                            let _ = cv.wait(guard).expect("condvar wait 실패");
                            if let Some((m, init)) = reload_data.lock().expect("reload_data mutex poisoned").take() {
                                simulator.model(m);
                                sim_io_buf.clear_and_init(init);
                            }
                        }
                        _ => break, // FLAG_HALT
                    }
                }
                sim_io_buf
            }
        });

        SimThread {
            thread_handle,
            _watchdog: watchdog,
            flag,
            cond,
        }
    }

    pub fn stop(self) -> SimIoBuf {
        self.flag.store(FLAG_HALT, Ordering::Relaxed);
        self.cond.1.notify_one();
        self.thread_handle.join().expect("시뮬 스레드 join 실패")
    }
}
