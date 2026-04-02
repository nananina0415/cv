mod utils;
mod net;
mod sim;
mod watchdog;


const MAX_NAME_BYTES: usize = 64;

#[repr(usize)]
pub enum UIMenu {
    Exit            = 0,
    StartSimulation = 1,
    StopSimulation  = 2,
    EnterSimulation = 3,
    ShowGroupInfo   = 4,
    WrongInput,
}

impl From<usize> for UIMenu {
    fn from(v: usize) -> UIMenu {
        match v {
            0 => UIMenu::Exit,
            1 => UIMenu::StartSimulation,
            2 => UIMenu::StopSimulation,
            3 => UIMenu::EnterSimulation,
            4 => UIMenu::ShowGroupInfo,
            _ => UIMenu::WrongInput,
        }
    }
}

use net::{NetSetting, NetThread};
use utils::TripleBuffer;
use sim::{UserIn, SimOut, SimThread, SimIoBuf};
use UIMenu::*;

fn main() {
    printsh!("사용자 이름: ");
    let name = input::<String>();

    printsh!("그룹 이름: ");
    let net_id = input::<String>();

    let password = format!("{:06}", rand::Rng::gen_range(&mut rand::thread_rng(), 0..1_000_000));
    println!("그룹 비밀번호: {password}");

    let net_setting = NetSetting {
        net_id,
        password,
        name,
        peer_type: p2p_core::PeerType::MidServer,
    };

    let (userin_r, userin_w, userin_swap) = TripleBuffer::new([
        Vec::<UserIn>::with_capacity(32),
        Vec::<UserIn>::with_capacity(32),
        Vec::<UserIn>::with_capacity(32),
    ]);
    let (simout_r, simout_w, simout_swap) = TripleBuffer::new([
        SimOut::default(),
        SimOut::default(),
        SimOut::default(),
    ]);

    let mut sim_io = Some(SimIoBuf {
        userin_r,
        userin_swap,
        simout_w,
        simout_swap,
    });
    
    // 서버 스레드 실행
    let net = NetThread::new(&net_setting, userin_w, simout_r);

    let mut sim: Option<SimThread> = None;

    // 사용자 입력 루프
    loop {
        println!("1. 시뮬레이션 시작  2. 시뮬레이션 종료  3. 시뮬레이션 입장  4. 그룹 정보  0. 종료");
        printsh!("> ");
        match input::<usize>().into() {
            StartSimulation => {
                if let Some(io) = sim_io.take() {
                    setup_python();
                    printsh!("시뮬레이션 데이터가 있는 폴더: ");
                    let sim_folder = input::<std::path::PathBuf>();
                    sim = Some(SimThread::new(sim_folder.clone(), io));
                    if let Err(e) = net.notice_sim_online(sim_folder) {
                        println!("시뮬레이션 온라인 알림 실패: {e}");
                    }
                } else {
                    println!("이미 시뮬레이션이 실행 중입니다.");
                }
            }
            StopSimulation => {
                if let Some(s) = sim.take() {
                    sim_io = Some(s.stop());
                    if let Err(e) = net.notice_sim_offline() {
                        println!("시뮬레이션 오프라인 알림 실패: {e}");
                    }
                }
            }
            EnterSimulation => {
                printsh!("어떤 그룹원의 시뮬레이션에 참가하시겠습니까?: ");
                let name = input::<String>();
                if let Some(addr) = net.sim_info(&name) {
                    show_qr(format!("{addr}"));
                } else {
                    println!("그룹원이 존재하지 않거나 시뮬레이션이 실행 중이지 않습니다");
                    continue;
                }
            }
            ShowGroupInfo => {
                show_group_info(&net_setting, &net.peer_list());
            }
            Exit => {
                break;
            }
            WrongInput => {
                // 보이자마자 리프레시될텐데, 일단 그냥 둬.
                println!("잘못된 입력입니다.");
            }
        }
    }
}


/*
    Python::with_gil(|py| {
        let simulator = py.import("simulator").expect("simulator 패키지를 찾을 수 없음");
        let result: String = simulator
            .call_method0("hello")
            .expect("hello() 호출 실패")
            .extract()
            .expect("반환값 추출 실패");
        println!("{}", result);
    });
*/


fn show_qr(data: String) {
    std::thread::spawn(move || {
        let code = qrcode::QrCode::with_error_correction_level(&data, qrcode::EcLevel::M).unwrap();
        let modules: Vec<bool> = code.to_colors().iter()
            .map(|c| *c == qrcode::Color::Dark)
            .collect();
        let size = (modules.len() as f64).sqrt() as usize;
        let scale = 8usize;
        let img_size = size * scale;

        let mut pixels = vec![255u8; img_size * img_size * 4];
        for y in 0..size {
            for x in 0..size {
                if modules[y * size + x] {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let i = ((y * scale + dy) * img_size + (x * scale + dx)) * 4;
                            pixels[i] = 0; pixels[i+1] = 0; pixels[i+2] = 0;
                        }
                    }
                }
            }
        }

        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([img_size as f32 + 40.0, img_size as f32 + 60.0])
                .with_resizable(false),
            ..Default::default()
        };
        eframe::run_native("QR Code", options, Box::new(move |cc| {
            let image = egui::ColorImage::from_rgba_unmultiplied([img_size, img_size], &pixels);
            let texture = cc.egui_ctx.load_texture("qr", image, Default::default());
            Ok(Box::new(QrApp { texture, data }))
        })).unwrap();
    });
}

struct QrApp {
    texture: egui::TextureHandle,
    data: String,
}

impl eframe::App for QrApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(&self.data);
            ui.add(egui::Image::new(&self.texture));
        });
    }
}


fn show_group_info(setting: &NetSetting, peers: &[p2p_core::PeerInfo]) {
    println!("그룹명: {}  사용자명: {}  비밀번호: {}", setting.net_id, setting.name, setting.password);

    let group_peers: Vec<_> = peers.iter()
        .filter(|p| !matches!(p.peer_type, p2p_core::PeerType::ArClient { .. }))
        .collect();

    println!("=== 그룹원 목록 ({}) ===", group_peers.len());
    for p in group_peers {
        let sim = if matches!(p.peer_type, p2p_core::PeerType::SimServer) { " [시뮬 실행중]" } else { "" };
        println!("  {}{}", p.name, sim);
    }

}

pub fn setup_python() {
    #[cfg(debug_assertions)]
    {
        let conda_prefix = std::env::var("CONDA_PREFIX")
            .expect("CONDA_PREFIX not set. Activate the cadverse environment first.");
        unsafe { std::env::set_var("PYTHONHOME", conda_prefix) };
    }

    #[cfg(not(debug_assertions))]
    {
        let exe_dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let python_env = exe_dir.join("python_env");
        if !python_env.exists() {
            let bundle = exe_dir.join("python_env.tar.gz");
            assert!(bundle.exists(), "python_env.tar.gz not found next to executable");
            std::fs::create_dir_all(&python_env).unwrap();
            let status = std::process::Command::new("tar")
                .args(["-xzf", bundle.to_str().unwrap(), "-C", python_env.to_str().unwrap()])
                .status()
                .expect("tar failed");
            assert!(status.success(), "Failed to unpack python_env.tar.gz");
        }
        unsafe { std::env::set_var("PYTHONHOME", &python_env) };
    }
}