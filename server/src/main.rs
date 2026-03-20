use pyo3::prelude::*;

fn setup_python_home() {
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

fn main() {
    setup_python_home();

    Python::with_gil(|py| {
        let simulator = py.import("simulator").expect("simulator 패키지를 찾을 수 없음");
        let result: String = simulator
            .call_method0("hello")
            .expect("hello() 호출 실패")
            .extract()
            .expect("반환값 추출 실패");
        println!("{}", result);
    });
}
