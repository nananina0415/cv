use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=pychrono/environment.yml");
    println!("cargo:rerun-if-changed=pychrono/simulator/");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let env_yml = Path::new(&manifest_dir).join("pychrono/environment.yml");
    let profile = env::var("PROFILE").unwrap_or_default();

    update_conda_env(&env_yml);

    if profile == "release" {
        let bundle_out = Path::new(&manifest_dir).join("target/release/python_env.tar.gz");
        pack_conda_env(&bundle_out);
    }
}

/// conda 환경 업데이트
/// shell이 환경을 생성한 뒤 cargo가 실행되므로, 이 시점엔 환경이 반드시 존재함
/// environment.yml 변경 시 cargo:rerun-if-changed에 의해 자동 재실행됨
fn update_conda_env(env_yml: &Path) {
    println!("cargo:warning=Updating conda environment...");
    let status = Command::new("conda")
        .args(["env", "update", "--file", env_yml.to_str().unwrap(), "--prune", "--solver=libmamba"])
        .status()
        .expect("conda not found");
    assert!(status.success(), "conda env update failed");
}

/// conda-pack으로 Python 환경 번들링
fn pack_conda_env(bundle_out: &PathBuf) {
    println!("cargo:warning=Bundling Python environment with conda-pack...");
    std::fs::create_dir_all(bundle_out.parent().unwrap()).unwrap();
    let status = Command::new("conda")
        .args([
            "run", "-n", "cadverse",
            "conda-pack", "-n", "cadverse",
            "-o", bundle_out.to_str().unwrap(),
            "--ignore-missing-files",
        ])
        .status()
        .expect("conda-pack failed");
    assert!(status.success(), "conda-pack failed");
    println!("cargo:warning=Python bundle created at {:?}", bundle_out);
}
