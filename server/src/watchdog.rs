use std::path::PathBuf;
use std::time::Duration;
use anyhow::Result;
use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;

// ── UI 트레이트 ───────────────────────────────────────────────────────────────

pub trait WatchdogUI {
    fn request_folder_input(&self) -> PathBuf;
}

// ── 핸들 ──────────────────────────────────────────────────────────────────────

pub struct Watchdog {
    pub folder: PathBuf,
    debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

impl Watchdog {
    pub fn new<F>(folder: PathBuf, on_change: F) -> Result<Self>
    where
        F: Fn(PathBuf) + Send + 'static,
    {
        let folder_clone = folder.clone();
        let mut debouncer = new_debouncer(Duration::from_millis(500), move |events| {
            if events.is_ok() {
                on_change(folder_clone.clone());
            }
        })?;

        debouncer.watcher().watch(&folder, RecursiveMode::NonRecursive)?;

        Ok(Self { folder, debouncer })
    }

    pub fn set_folder(&mut self, folder: PathBuf) -> Result<()> {
        let _ = self.debouncer.watcher().unwatch(&self.folder);
        self.debouncer.watcher().watch(&folder, RecursiveMode::NonRecursive)?;
        self.folder = folder;
        Ok(())
    }
}

// ── 진입점 ────────────────────────────────────────────────────────────────────

pub fn run_watchdog<U, F>(ui: U, on_change: F) -> Result<Watchdog>
where
    U: WatchdogUI,
    F: Fn(PathBuf) + Send + 'static,
{
    let folder = ui.request_folder_input();
    if folder.is_dir() {
        Ok(Watchdog::new(folder, on_change)?)
    } else {
        Err(anyhow::anyhow!("입력된 경로가 폴더가 아닙니다"))
    }
}
