//! Windows tray UI: event loop, tray icon, menu.

use crate::devices::{self, hid};
use crate::status::{tooltip, DeviceKind, DeviceState, DeviceStatus};
use crate::worker::{self, Backend, Trigger};
use crate::{autostart, icon, notify};
use hidapi::HidApi;
use log::{error, info, LevelFilter};
use std::path::PathBuf;
use std::sync::mpsc;
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows_sys::Win32::System::Threading::CreateMutexW;

/// Must match `AppMutex` in `installer/logibat-tray.iss`.
const MUTEX_NAME: &str = "LogiBatTrayMutex";

enum UserEvent {
    Status(Vec<DeviceStatus>),
    Menu(MenuEvent),
}

struct HidBackend {
    api: Option<HidApi>,
}

impl Backend for HidBackend {
    fn refresh(&mut self) {
        match &mut self.api {
            Some(api) => hid::refresh(api),
            None => match HidApi::new() {
                Ok(api) => self.api = Some(api),
                Err(e) => error!("hidapi init failed: {e}"),
            },
        }
    }

    fn present(&self, kind: DeviceKind) -> bool {
        self.api
            .as_ref()
            .is_some_and(|api| hid::find_path(api, devices::spec(kind)).is_some())
    }

    fn read(&mut self, kind: DeviceKind) -> DeviceState {
        match &self.api {
            Some(api) => hid::read_device(api, devices::spec(kind)),
            None => DeviceState::Unreachable("hidapi not available".into()),
        }
    }
}

/// Returns false if another instance already runs.
fn single_instance() -> bool {
    let name: Vec<u16> = MUTEX_NAME.encode_utf16().chain(Some(0)).collect();
    // SAFETY: name is a valid NUL-terminated UTF-16 string. The handle is
    // never closed: it must live as long as the process.
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        !handle.is_null() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

fn log_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("logibat-tray")
}

fn init_logging() {
    let dir = log_dir();
    let _ = std::fs::create_dir_all(&dir);
    let level = if std::env::args().any(|a| a == "--debug") {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    let mut config = simplelog::ConfigBuilder::new();
    let _ = config.set_time_offset_to_local();
    if let Ok(file) = std::fs::File::create(dir.join("logibat-tray.log")) {
        let _ = simplelog::WriteLogger::init(level, config.build(), file);
    }
}

fn make_icon(statuses: &[DeviceStatus]) -> Option<Icon> {
    let rgba = icon::render(statuses, autostart::light_taskbar());
    Icon::from_rgba(rgba, icon::SIZE as u32, icon::SIZE as u32)
        .map_err(|e| error!("icon: {e}"))
        .ok()
}

pub fn run() {
    if !single_instance() {
        return;
    }
    init_logging();
    info!("logibat-tray {} started", env!("CARGO_PKG_VERSION"));

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(UserEvent::Menu(e));
    }));

    let (tx, rx) = mpsc::channel::<Trigger>();
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        worker::run(HidBackend { api: None }, rx, |s| {
            let _ = proxy.send_event(UserEvent::Status(s));
        })
    });
    let _registration = notify::register(tx.clone());

    let refresh = MenuItem::new("Refresh now", true, None);
    let autostart_item =
        CheckMenuItem::new("Start with Windows", true, autostart::is_enabled(), None);
    let open_log = MenuItem::new("Open log folder", true, None);
    let quit = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    let _ = menu.append_items(&[
        &refresh,
        &autostart_item,
        &open_log,
        &PredefinedMenuItem::separator(),
        &quit,
    ]);

    let initial: Vec<DeviceStatus> = DeviceKind::ALL
        .iter()
        .map(|&k| DeviceStatus::new(k))
        .collect();
    let mut tray: Option<TrayIcon> = None;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {
                let mut builder = TrayIconBuilder::new()
                    .with_menu(Box::new(menu.clone()))
                    .with_tooltip(tooltip(&initial, None));
                if let Some(i) = make_icon(&initial) {
                    builder = builder.with_icon(i);
                }
                match builder.build() {
                    Ok(t) => tray = Some(t),
                    Err(e) => error!("tray icon: {e}"),
                }
            }
            Event::UserEvent(UserEvent::Status(statuses)) => {
                if let Some(t) = &tray {
                    let now = chrono::Local::now().format("%H:%M").to_string();
                    let _ = t.set_tooltip(Some(tooltip(&statuses, Some(&now))));
                    if let Some(i) = make_icon(&statuses) {
                        let _ = t.set_icon(Some(i));
                    }
                }
            }
            Event::UserEvent(UserEvent::Menu(e)) => {
                if e.id == refresh.id() {
                    let _ = tx.send(Trigger::Refresh);
                } else if e.id == autostart_item.id() {
                    let wanted = autostart_item.is_checked();
                    if let Err(err) = autostart::set_enabled(wanted) {
                        error!("autostart: {err}");
                    }
                    autostart_item.set_checked(autostart::is_enabled());
                } else if e.id == open_log.id() {
                    let _ = std::process::Command::new("explorer")
                        .arg(log_dir())
                        .spawn();
                } else if e.id == quit.id() {
                    tray.take();
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    });
}
