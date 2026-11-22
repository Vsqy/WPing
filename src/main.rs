#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use std::collections::VecDeque;
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{mpsc, Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use tray_item::{IconSource, TrayItem};

const CREATE_NO_WINDOW: u32 = 0x08000000;
const APP_WINDOW_TITLE: &str = "WPing - Game Latency & Route Optimizer";
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);

#[derive(PartialEq, Clone, Copy, Debug)]
enum Language {
    Ukrainian,
    English,
    Russian,
}

impl Language {
    fn label(&self) -> &'static str {
        match self {
            Language::Ukrainian => "🇺🇦 UA",
            Language::English => "🇬🇧 EN",
            Language::Russian => "🇷🇺 RU",
        }
    }
}

fn detect_system_language() -> Language {
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
        fn GetUserDefaultLocaleName(lpLocaleName: *mut u16, cchLocaleName: i32) -> i32;
    }

    unsafe {
        let mut buf = [0u16; 32];
        let len = GetUserDefaultLocaleName(buf.as_mut_ptr(), 32);
        if len > 0 {
            let locale = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
            if locale.starts_with("uk") {
                return Language::Ukrainian;
            } else if locale.starts_with("ru") {
                return Language::Russian;
            } else if locale.starts_with("en") {
                return Language::English;
            }
        }

        let lang_id = GetUserDefaultUILanguage();
        let primary = lang_id & 0x03FF;
        if primary == 0x22 {
            return Language::Ukrainian;
        } else if primary == 0x19 {
            return Language::Russian;
        }
    }

    Language::English
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(non_snake_case)]
struct FILTERKEYS {
    cbSize: u32,
    dwFlags: u32,
    iWaitMSec: u32,
    iDelayMSec: u32,
    iRepeatMSec: u32,
    iBounceMSec: u32,
}

const SPI_GETFILTERKEYS: u32 = 0x0032;
const SPI_SETFILTERKEYS: u32 = 0x0033;
const SPIF_UPDATEINIFILE: u32 = 0x0001;
const SPIF_SENDCHANGE: u32 = 0x0002;
const FKF_FILTERKEYSON: u32 = 0x00000001;
const FKF_AVAILABLE: u32 = 0x00000002;

#[link(name = "user32")]
extern "system" {
    fn SystemParametersInfoW(
        uiAction: u32,
        uiParam: u32,
        pvParam: *mut std::ffi::c_void,
        fWinIni: u32,
    ) -> i32;
}

#[link(name = "ntdll")]
extern "system" {
    fn NtSetTimerResolution(desired: u32, set: u8, current: *mut u32) -> i32;
    fn NtQueryTimerResolution(min: *mut u32, max: *mut u32, cur: *mut u32) -> i32;
}

#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(uPeriod: u32) -> u32;
    fn timeEndPeriod(uPeriod: u32) -> u32;
}

#[derive(PartialEq, Clone, Copy)]
enum AppTab {
    Dashboard,
    InputDelay,
    LocalTweaks,
    CloudRelay,
    Diagnostics,
}

#[derive(Clone)]
struct ServerNode {
    name: &'static str,
    loc_ua: &'static str,
    loc_en: &'static str,
    loc_ru: &'static str,
    ip: &'static str,
    ping_ms: Option<u32>,
}

impl ServerNode {
    fn location(&self, lang: Language) -> &'static str {
        match lang {
            Language::Ukrainian => self.loc_ua,
            Language::English => self.loc_en,
            Language::Russian => self.loc_ru,
        }
    }
}

#[derive(Clone)]
struct GameDefinition {
    name: &'static str,
    exe: &'static str,
    server_idx: usize,
}

#[derive(Clone)]
struct DetectedGame {
    name: String,
    exe: String,
    pid: u32,
}

#[derive(Clone, Debug, PartialEq)]
struct NetworkAdapter {
    name: String,
    description: String,
    is_virtual: bool,
    ip: Option<String>,
}

struct AppState {
    language: Language,
    current_tab: AppTab,

    // Games database
    supported_games: Vec<GameDefinition>,
    selected_game: usize,
    detected_game: Option<DetectedGame>,
    auto_boost_on_detect: bool,

    is_boosted: bool,
    boost_start_time: Option<Instant>,

    // Tweaks
    tweak_nagle: bool,
    tweak_throttling: bool,
    tweak_system_resp: bool,
    tweak_nic_rss: bool,
    tweak_auto_mtu: bool,
    tweak_status_log: Vec<String>,

    // Network Adapters
    adapters: Vec<NetworkAdapter>,
    selected_adapter_idx: usize,
    shared_adapter_ip: Arc<RwLock<Option<String>>>,

    // Cloud nodes & Auto-Ping every 10s
    nodes: Vec<ServerNode>,
    selected_node: usize,
    auto_select_best_node: bool,
    last_node_update: Instant,

    // Diagnostics
    diag_host: String,
    diag_results: Vec<String>,
    is_pinging: bool,

    // Realtime Ping Chart
    ping_history: VecDeque<f32>,
    current_ping: f32,
    packet_loss: f32,
    jitter: f32,

    // Channels
    ping_rx: Option<mpsc::Receiver<f32>>,
    diag_rx: Option<mpsc::Receiver<String>>,
    admin_log_rx: Option<mpsc::Receiver<String>>,
    detect_rx: Option<mpsc::Receiver<Option<DetectedGame>>>,
    node_ping_rx: Option<mpsc::Receiver<(usize, u32)>>,
    adapters_rx: Option<mpsc::Receiver<Vec<NetworkAdapter>>>,
    boost_cancel: Option<Arc<AtomicBool>>,

    // Timer Resolution (0.5ms)
    timer_res_enabled: bool,
    timer_res_current_100ns: u32,
    timer_res_min_100ns: u32,
    timer_res_max_100ns: u32,
    auto_apply_timer_res: bool,

    // FilterKeys (Fast Edit / 0 Keyboard Delay)
    filterkeys_enabled: bool,
    filterkeys_delay_ms: u32,
    filterkeys_repeat_ms: u32,
    auto_apply_filterkeys: bool,
    filterkeys_test_input: String,

    // System Tray
    _tray: Option<TrayItem>,
    tray_show_rx: Option<mpsc::Receiver<()>>,
    allow_exit: bool,
}

impl Default for AppState {
    fn default() -> Self {
        let mut history = VecDeque::with_capacity(30);
        for _ in 0..30 {
            history.push_back(25.0);
        }

        let supported_games = vec![
            GameDefinition { name: "Counter-Strike 2", exe: "cs2.exe", server_idx: 1 },
            GameDefinition { name: "Dota 2", exe: "dota2.exe", server_idx: 2 },
            GameDefinition { name: "Valorant", exe: "VALORANT-Win64-Shipping.exe", server_idx: 1 },
            GameDefinition { name: "Apex Legends", exe: "r5apex.exe", server_idx: 1 },
            GameDefinition { name: "Fortnite", exe: "FortniteClient-Win64-Shipping.exe", server_idx: 1 },
            GameDefinition { name: "PUBG: BATTLEGROUNDS", exe: "TslGame.exe", server_idx: 1 },
            GameDefinition { name: "League of Legends", exe: "League of Legends.exe", server_idx: 1 },
            GameDefinition { name: "Overwatch 2", exe: "Overwatch.exe", server_idx: 1 },
            GameDefinition { name: "Deadlock", exe: "project8.exe", server_idx: 1 },
            GameDefinition { name: "Grand Theft Auto V", exe: "GTA5.exe", server_idx: 1 },
            GameDefinition { name: "Rust", exe: "RustClient.exe", server_idx: 1 },
            GameDefinition { name: "Rainbow Six Siege", exe: "RainbowSix.exe", server_idx: 1 },
            GameDefinition { name: "Warframe", exe: "Warframe.x64.exe", server_idx: 1 },
            GameDefinition { name: "Rocket League", exe: "RocketLeague.exe", server_idx: 1 },
            GameDefinition { name: "Call of Duty: Warzone", exe: "cod.exe", server_idx: 1 },
            GameDefinition { name: "Minecraft", exe: "javaw.exe", server_idx: 1 },
            GameDefinition { name: "Roblox", exe: "RobloxPlayerBeta.exe", server_idx: 1 },
        ];

        let shared_adapter_ip = Arc::new(RwLock::new(None));

        Self {
            language: detect_system_language(),
            current_tab: AppTab::Dashboard,
            supported_games,
            selected_game: 0,
            detected_game: None,
            auto_boost_on_detect: true,

            is_boosted: false,
            boost_start_time: None,

            tweak_nagle: true,
            tweak_throttling: true,
            tweak_system_resp: true,
            tweak_nic_rss: true,
            tweak_auto_mtu: true,
            tweak_status_log: vec![
                "Готовий до роботи.".into(),
                "Фоновий замір релеїв активний (оновлення кожні 10 сек).".into(),
            ],

            adapters: Vec::new(),
            selected_adapter_idx: 0,
            shared_adapter_ip,

            nodes: vec![
                ServerNode {
                    name: "Cloudflare Edge",
                    loc_ua: "Варшава, Польща 🇵🇱",
                    loc_en: "Warsaw, Poland 🇵🇱",
                    loc_ru: "Варшава, Польша 🇵🇱",
                    ip: "1.1.1.1",
                    ping_ms: None,
                },
                ServerNode {
                    name: "Valve Game Relay",
                    loc_ua: "Франкфурт (EU Central) 🇪🇺",
                    loc_en: "Frankfurt (EU Central) 🇪🇺",
                    loc_ru: "Франкфурт (EU Central) 🇪🇺",
                    ip: "155.133.226.1",
                    ping_ms: None,
                },
                ServerNode {
                    name: "Valve Game Relay",
                    loc_ua: "Варшава (EU East) 🇵🇱",
                    loc_en: "Warsaw (EU East) 🇵🇱",
                    loc_ru: "Варшава (EU East) 🇵🇱",
                    ip: "155.133.230.1",
                    ping_ms: None,
                },
                ServerNode {
                    name: "Oracle Cloud Free",
                    loc_ua: "Амстердам, Нідерланди 🇳🇱",
                    loc_en: "Amsterdam, Netherlands 🇳🇱",
                    loc_ru: "Амстердам, Нидерланды 🇳🇱",
                    ip: "140.238.160.1",
                    ping_ms: None,
                },
                ServerNode {
                    name: "Cloudflare Edge",
                    loc_ua: "Гельсінкі, Фінляндія 🇫🇮",
                    loc_en: "Helsinki, Finland 🇫🇮",
                    loc_ru: "Хельсинки, Финляндия 🇫🇮",
                    ip: "104.16.124.96",
                    ping_ms: None,
                },
                ServerNode {
                    name: "AWS Global Accelerator",
                    loc_ua: "Стокгольм, Швеція 🇸🇪",
                    loc_en: "Stockholm, Sweden 🇸🇪",
                    loc_ru: "Стокгольм, Швеция 🇸🇪",
                    ip: "13.248.158.1",
                    ping_ms: None,
                },
            ],
            selected_node: 0,
            auto_select_best_node: true,
            last_node_update: Instant::now(),

            diag_host: "155.133.226.1".into(),
            diag_results: vec![],
            is_pinging: false,

            ping_history: history,
            current_ping: 25.0,
            packet_loss: 0.0,
            jitter: 0.8,

            ping_rx: None,
            diag_rx: None,
            admin_log_rx: None,
            detect_rx: None,
            node_ping_rx: None,
            adapters_rx: None,
            boost_cancel: None,

            timer_res_enabled: false,
            timer_res_current_100ns: 10000,
            timer_res_min_100ns: 156250,
            timer_res_max_100ns: 5000,
            auto_apply_timer_res: true,

            filterkeys_enabled: false,
            filterkeys_delay_ms: 150,
            filterkeys_repeat_ms: 25,
            auto_apply_filterkeys: true,
            filterkeys_test_input: String::new(),

            _tray: None,
            tray_show_rx: None,
            allow_exit: false,
        }
    }
}

fn query_network_adapters() -> Vec<NetworkAdapter> {
    let out = std::process::Command::new("powershell")
        .args(&[
            "-NoProfile",
            "-Command",
            "Get-NetAdapter | Where-Object Status -eq 'Up' | ForEach-Object { \
                $n = $_.Name; \
                $d = $_.InterfaceDescription; \
                $v = if ($_.Virtual -or $d -match 'VPN|Virtual|Radmin|TAP|Hamachi|ZeroTier|Hyper-V|VMware|Loopback' -or $n -match 'VPN|Radmin|Hamachi|ZeroTier') { '1' } else { '0' }; \
                $ip = (Get-NetIPAddress -InterfaceIndex $_.InterfaceIndex -AddressFamily IPv4 -ErrorAction SilentlyContinue | Select-Object -First 1).IPAddress; \
                \"$n`n$d`n$v`n$ip\" \
            }",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    let mut list = Vec::new();
    if let Ok(o) = out {
        let text = String::from_utf8_lossy(&o.stdout);
        let lines: Vec<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        for chunk in lines.chunks(4) {
            if chunk.len() >= 3 {
                let name = chunk[0].to_string();
                let desc = chunk[1].to_string();
                let is_virtual = chunk[2] == "1";
                let ip = if chunk.len() >= 4 && !chunk[3].is_empty() {
                    Some(chunk[3].to_string())
                } else {
                    None
                };
                list.push(NetworkAdapter {
                    name,
                    description: desc,
                    is_virtual,
                    ip,
                });
            }
        }
    }

    // Sort: Physical adapters first (prefer ones with IP), then virtual VPN adapters
    list.sort_by_key(|a| (a.is_virtual, a.ip.is_none()));
    list
}

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Track window HWND
        if MAIN_HWND.load(Ordering::Relaxed) == 0 {
            unsafe {
                use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;
                let title: Vec<u16> = APP_WINDOW_TITLE
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
                if hwnd != 0 {
                    MAIN_HWND.store(hwnd as isize, Ordering::Relaxed);
                }
            }
        }

        // Intercept close button [X] -> minimize to tray
        if self.current_tab == AppTab::InputDelay {
            self.query_timer_resolution();
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            if !self.allow_exit {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                let hwnd = MAIN_HWND.load(Ordering::Relaxed);
                if hwnd != 0 {
                    unsafe {
                        use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
                        ShowWindow(hwnd as _, SW_HIDE);
                    }
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
            }
        }

        // Poll tray show message
        if let Some(rx) = &self.tray_show_rx {
            while let Ok(_) = rx.try_recv() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            }
        }

        // Poll network adapters
        let mut new_adapters = None;
        if let Some(rx) = &self.adapters_rx {
            while let Ok(list) = rx.try_recv() {
                if !list.is_empty() {
                    new_adapters = Some(list);
                }
            }
        }

        if let Some(list) = new_adapters {
            let first_load = self.adapters.is_empty();
            self.adapters = list;
            if first_load {
                self.selected_adapter_idx = 0;
                if let Some(a) = self.adapters.get(0) {
                    if let Ok(mut lock) = self.shared_adapter_ip.write() {
                        *lock = a.ip.clone();
                    }
                }
                self.refresh_nodes_ping();
            } else if self.selected_adapter_idx >= self.adapters.len() {
                self.selected_adapter_idx = 0;
                if let Some(a) = self.adapters.get(0) {
                    if let Ok(mut lock) = self.shared_adapter_ip.write() {
                        *lock = a.ip.clone();
                    }
                }
            }
        }

        // Poll 10-second Node Ping updates
        if let Some(rx) = &self.node_ping_rx {
            let mut updated_any = false;
            while let Ok((idx, val)) = rx.try_recv() {
                if idx < self.nodes.len() {
                    self.nodes[idx].ping_ms = Some(val);
                    updated_any = true;
                }
            }

            if updated_any {
                self.last_node_update = Instant::now();
                // If auto-select best node is enabled, automatically pick the lowest ping node
                if self.auto_select_best_node {
                    if let Some((best_idx, _)) = self
                        .nodes
                        .iter()
                        .enumerate()
                        .filter_map(|(i, n)| n.ping_ms.map(|p| (i, p)))
                        .min_by_key(|&(_, p)| p)
                    {
                        self.selected_node = best_idx;
                    }
                }
            }
        }

        // Poll game detection channel
        let mut detect_events = Vec::new();
        if let Some(rx) = &self.detect_rx {
            while let Ok(detected) = rx.try_recv() {
                detect_events.push(detected);
            }
        }

        for detected in detect_events {
            match detected {
                Some(game) => {
                    let was_none = self.detected_game.is_none();
                    if let Some(idx) = self.supported_games.iter().position(|g| g.exe.eq_ignore_ascii_case(&game.exe)) {
                        self.selected_game = idx;
                        if !self.auto_select_best_node {
                            self.selected_node = self.supported_games[idx].server_idx;
                        }
                    }

                    self.detected_game = Some(game);

                    if was_none && self.auto_boost_on_detect && !self.is_boosted {
                        self.is_boosted = true;
                        self.boost_start_time = Some(Instant::now());
                        self.start_real_ping_thread();
                        if self.auto_apply_timer_res {
                            self.enable_timer_resolution();
                        }
                        if self.auto_apply_filterkeys {
                            self.apply_filterkeys();
                        }
                    }
                }
                None => {
                    if self.detected_game.is_some() {
                        self.detected_game = None;
                    }
                }
            }
        }

        // Poll real ping channel
        if let Some(rx) = &self.ping_rx {
            while let Ok(val) = rx.try_recv() {
                self.current_ping = val;
                if self.ping_history.len() >= 30 {
                    self.ping_history.pop_front();
                }
                self.ping_history.push_back(val);
                self.jitter = (self.jitter * 0.7 + (val - self.current_ping).abs() * 0.3).clamp(0.4, 3.5);
            }
        }

        // Poll diag channel
        if let Some(rx) = &self.diag_rx {
            while let Ok(msg) = rx.try_recv() {
                self.diag_results.push(msg);
                self.is_pinging = false;
            }
        }

        // Poll admin tweak log channel
        if let Some(rx) = &self.admin_log_rx {
            while let Ok(msg) = rx.try_recv() {
                self.tweak_status_log.push(msg);
            }
        }

        ctx.request_repaint_after(Duration::from_millis(400));

        // Dark Theme
        let mut style = (*ctx.style()).clone();
        style.visuals.dark_mode = true;
        style.visuals.override_text_color = Some(egui::Color32::from_rgb(220, 230, 242));
        style.visuals.window_fill = egui::Color32::from_rgb(15, 18, 24);
        style.visuals.panel_fill = egui::Color32::from_rgb(18, 22, 30);
        ctx.set_style(style);

        let lang = self.language;

        // Header
        egui::TopBottomPanel::top("top_header").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("⚡ WPING")
                        .font(egui::FontId::proportional(22.0))
                        .color(egui::Color32::from_rgb(0, 230, 153))
                        .strong(),
                );
                ui.label(
                    egui::RichText::new("GAME LATENCY OPTIMIZER")
                        .font(egui::FontId::proportional(11.0))
                        .color(egui::Color32::from_rgb(120, 140, 165)),
                );
                ui.label(
                    egui::RichText::new("• Created by Vsqy")
                        .font(egui::FontId::proportional(11.0))
                        .color(egui::Color32::from_rgb(0, 200, 255))
                        .strong(),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let close_label = match lang {
                        Language::Ukrainian => "❌ Закрити зовсім",
                        Language::English => "❌ Exit App",
                        Language::Russian => "❌ Закрыть совсем",
                    };
                    if ui.button(close_label).clicked() {
                        self.allow_exit = true;
                        std::process::exit(0);
                    }

                    ui.add_space(6.0);

                    // Language Selector
                    egui::ComboBox::from_id_salt("lang_selector_combo")
                        .selected_text(self.language.label())
                        .width(75.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.language, Language::Ukrainian, Language::Ukrainian.label());
                            ui.selectable_value(&mut self.language, Language::English, Language::English.label());
                            ui.selectable_value(&mut self.language, Language::Russian, Language::Russian.label());
                        });

                    ui.add_space(6.0);

                    if self.is_boosted {
                        let active_label = match lang {
                            Language::Ukrainian => "● ОПТИМІЗАЦІЯ АКТИВНА",
                            Language::English => "● OPTIMIZATION ACTIVE",
                            Language::Russian => "● ОПТИМИЗАЦИЯ АКТИВНА",
                        };
                        ui.label(
                            egui::RichText::new(active_label)
                                .color(egui::Color32::from_rgb(0, 255, 128))
                                .strong(),
                        );
                    } else {
                        let std_label = match lang {
                            Language::Ukrainian => "○ СТАНДАРТНИЙ РЕЖИМ",
                            Language::English => "○ STANDARD MODE",
                            Language::Russian => "○ СТАНДАРТНЫЙ РЕЖИМ",
                        };
                        ui.label(
                            egui::RichText::new(std_label)
                                .color(egui::Color32::from_rgb(140, 150, 165)),
                        );
                    }
                });
            });

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);

            // Tabs
            ui.horizontal(|ui| {
                let tab_titles = match lang {
                    Language::Ukrainian => [
                        (AppTab::Dashboard, "🎮 Оптимізатор ігор"),
                        (AppTab::InputDelay, "⚡ 0.5ms & FilterKeys"),
                        (AppTab::LocalTweaks, "⚙️ Твіки Windows (Admin)"),
                        (AppTab::CloudRelay, "🌐 Хмарний релей"),
                        (AppTab::Diagnostics, "📊 Тест & Діагностика"),
                    ],
                    Language::English => [
                        (AppTab::Dashboard, "🎮 Game Optimizer"),
                        (AppTab::InputDelay, "⚡ 0.5ms & FilterKeys"),
                        (AppTab::LocalTweaks, "⚙️ Windows Tweaks (Admin)"),
                        (AppTab::CloudRelay, "🌐 Cloud Relay"),
                        (AppTab::Diagnostics, "📊 Test & Diagnostics"),
                    ],
                    Language::Russian => [
                        (AppTab::Dashboard, "🎮 Оптимизатор игр"),
                        (AppTab::InputDelay, "⚡ 0.5ms & FilterKeys"),
                        (AppTab::LocalTweaks, "⚙️ Твики Windows (Admin)"),
                        (AppTab::CloudRelay, "🌐 Облачный релей"),
                        (AppTab::Diagnostics, "📊 Тест и Диагностика"),
                    ],
                };

                for (tab, title) in tab_titles {
                    let is_selected = self.current_tab == tab;
                    let btn = egui::Button::new(
                        egui::RichText::new(title)
                            .font(egui::FontId::proportional(14.0))
                            .color(if is_selected {
                                egui::Color32::from_rgb(0, 230, 153)
                            } else {
                                egui::Color32::from_rgb(180, 195, 210)
                            }),
                    )
                    .fill(if is_selected {
                        egui::Color32::from_rgb(26, 36, 48)
                    } else {
                        egui::Color32::TRANSPARENT
                    });

                    if ui.add(btn).clicked() {
                        self.current_tab = tab;
                    }
                }
            });
            ui.add_space(6.0);
        });

        // Bottom Bar
        egui::TopBottomPanel::bottom("bottom_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let adapter_summary = if let Some(a) = self.adapters.get(self.selected_adapter_idx) {
                    format!("{} ({})", a.description, a.ip.as_deref().unwrap_or("Немає IP"))
                } else {
                    match lang {
                        Language::Ukrainian => "Визначення мережевої карти...".to_string(),
                        Language::English => "Detecting network adapter...".to_string(),
                        Language::Russian => "Определение сетевой карты...".to_string(),
                    }
                };

                let bot_label = match lang {
                    Language::Ukrainian => format!("WPing • Адаптер: {} • [X] в трей", adapter_summary),
                    Language::English => format!("WPing • Adapter: {} • [X] minimizes to tray", adapter_summary),
                    Language::Russian => format!("WPing • Адаптер: {} • [X] в трей", adapter_summary),
                };

                ui.label(
                    egui::RichText::new(bot_label)
                        .font(egui::FontId::proportional(11.0))
                        .color(egui::Color32::from_rgb(110, 130, 150)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("RAM: ~14 MB | Jitter: {:.1} ms", self.jitter))
                            .font(egui::FontId::proportional(11.0))
                            .color(egui::Color32::from_rgb(0, 210, 140)),
                    );
                });
            });
            ui.add_space(4.0);
        });

        // Central
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(8.0);
            match self.current_tab {
                AppTab::Dashboard => self.render_dashboard(ui),
                AppTab::InputDelay => self.render_input_delay(ui),
                AppTab::LocalTweaks => self.render_local_tweaks(ui),
                AppTab::CloudRelay => self.render_cloud_relay(ui),
                AppTab::Diagnostics => self.render_diagnostics(ui),
            }
        });
    }
}

impl AppState {
    fn render_adapter_selector(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;
        ui.group(|ui| {
            ui.horizontal(|ui| {
                let lbl = match lang {
                    Language::Ukrainian => "🌐 Мережевий адаптер:",
                    Language::English => "🌐 Network Adapter:",
                    Language::Russian => "🌐 Сетевой адаптер:",
                };
                ui.label(egui::RichText::new(lbl).strong());

                if self.adapters.is_empty() {
                    let searching_lbl = match lang {
                        Language::Ukrainian => "Пошук мережевих інтерфейсів...",
                        Language::English => "Searching network interfaces...",
                        Language::Russian => "Поиск сетевых интерфейсов...",
                    };
                    ui.label(egui::RichText::new(searching_lbl).color(egui::Color32::GRAY));
                } else {
                    let phys_tag = match lang {
                        Language::Ukrainian => "🔌 [Фізичний]",
                        Language::English => "🔌 [Physical]",
                        Language::Russian => "🔌 [Физический]",
                    };
                    let no_ip_tag = match lang {
                        Language::Ukrainian => "Немає IP",
                        Language::English => "No IP",
                        Language::Russian => "Нет IP",
                    };

                    let current_text = if let Some(a) = self.adapters.get(self.selected_adapter_idx) {
                        let tag = if a.is_virtual { "🔒 [VPN]" } else { phys_tag };
                        format!("{} {} ({})", tag, a.description, a.ip.as_deref().unwrap_or(no_ip_tag))
                    } else {
                        "---".to_string()
                    };

                    let mut changed = false;
                    egui::ComboBox::from_id_salt("net_adapter_selector_combo")
                        .width(420.0)
                        .selected_text(current_text)
                        .show_ui(ui, |ui| {
                            for (i, a) in self.adapters.iter().enumerate() {
                                let tag = if a.is_virtual { "🔒 [VPN]" } else { phys_tag };
                                let text = format!("{} {} ({})", tag, a.description, a.ip.as_deref().unwrap_or(no_ip_tag));
                                if ui.selectable_value(&mut self.selected_adapter_idx, i, text).clicked() {
                                    changed = true;
                                }
                            }
                        });

                    if changed {
                        if let Some(a) = self.adapters.get(self.selected_adapter_idx) {
                            if let Ok(mut lock) = self.shared_adapter_ip.write() {
                                *lock = a.ip.clone();
                            }
                            self.refresh_nodes_ping();
                        }
                    }
                }

                let refresh_lbl = match lang {
                    Language::Ukrainian => "🔄 Оновити",
                    Language::English => "🔄 Refresh",
                    Language::Russian => "🔄 Обновить",
                };
                if ui.button(refresh_lbl).clicked() {
                    self.refresh_adapters();
                }
            });

            if let Some(a) = self.adapters.get(self.selected_adapter_idx) {
                ui.add_space(2.0);
                if a.is_virtual {
                    let warn_msg = match lang {
                        Language::Ukrainian => "⚠️ УВАГА: Обрано віртуальний адаптер VPN! Для ігор та стабільного низького пінгу оберіть фізичний адаптер (Ethernet / Wi-Fi).",
                        Language::English => "⚠️ WARNING: Virtual VPN adapter selected! For gaming and lowest latency, select a physical adapter (Ethernet / Wi-Fi).",
                        Language::Russian => "⚠️ ВНИМАНИЕ: Выбран виртуальный адаптер VPN! Для игр и стабильного низкого пинга выберите физический адаптер (Ethernet / Wi-Fi).",
                    };
                    ui.label(
                        egui::RichText::new(warn_msg)
                            .font(egui::FontId::proportional(11.0))
                            .color(egui::Color32::from_rgb(255, 170, 0))
                            .strong(),
                    );
                } else {
                    let ok_msg = match lang {
                        Language::Ukrainian => "✔ Фізична мережева карта активна. Маршрутизація та заміри пінгу прив'язані до вашого прямого каналу.",
                        Language::English => "✔ Physical network card is active. Routing and ping measurements are bound directly to your ISP connection.",
                        Language::Russian => "✔ Физическая сетевая карта активна. Маршрутизация и замеры пинга привязаны к вашему прямому каналу.",
                    };
                    ui.label(
                        egui::RichText::new(ok_msg)
                            .font(egui::FontId::proportional(11.0))
                            .color(egui::Color32::from_rgb(0, 230, 153)),
                    );
                }
            }
        });
    }

    fn refresh_adapters(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.adapters_rx = Some(rx);
        thread::spawn(move || {
            let list = query_network_adapters();
            let _ = tx.send(list);
        });
    }

    fn render_dashboard(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;

        // Auto-Detection Banner
        ui.group(|ui| {
            ui.horizontal(|ui| {
                if let Some(game) = &self.detected_game {
                    let active_tag = match lang {
                        Language::Ukrainian => "🟢 АКТИВНА ГРА:",
                        Language::English => "🟢 ACTIVE GAME:",
                        Language::Russian => "🟢 АКТИВНАЯ ИГРА:",
                    };
                    ui.label(
                        egui::RichText::new(active_tag)
                            .font(egui::FontId::proportional(14.0))
                            .color(egui::Color32::from_rgb(0, 255, 128))
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(format!("{} (PID: {}, {})", game.name, game.pid, game.exe))
                            .font(egui::FontId::proportional(14.0))
                            .color(egui::Color32::WHITE)
                            .strong(),
                    );
                } else {
                    let detect_tag = match lang {
                        Language::Ukrainian => "🔍 АВТО-ДЕТЕКЦІЯ:",
                        Language::English => "🔍 AUTO-DETECTION:",
                        Language::Russian => "🔍 АВТО-ДЕТЕКЦИЯ:",
                    };
                    let waiting_msg = match lang {
                        Language::Ukrainian => "Очікування запуску гри... (Запустіть CS2, Dota 2, Valorant тощо)",
                        Language::English => "Waiting for game launch... (Launch CS2, Dota 2, Valorant, etc.)",
                        Language::Russian => "Ожидание запуска игры... (Запустите CS2, Dota 2, Valorant и др.)",
                    };
                    ui.label(
                        egui::RichText::new(detect_tag)
                            .font(egui::FontId::proportional(13.0))
                            .color(egui::Color32::from_rgb(0, 210, 255))
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(waiting_msg)
                            .font(egui::FontId::proportional(13.0))
                            .color(egui::Color32::from_rgb(160, 175, 195)),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let auto_boost_lbl = match lang {
                        Language::Ukrainian => "Авто-буст при старті гри",
                        Language::English => "Auto-boost on game launch",
                        Language::Russian => "Авто-буст при запуске игры",
                    };
                    ui.checkbox(
                        &mut self.auto_boost_on_detect,
                        egui::RichText::new(auto_boost_lbl).color(egui::Color32::from_rgb(0, 230, 153)),
                    );
                });
            });
        });

        ui.add_space(8.0);
        self.render_adapter_selector(ui);
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            // Left Column
            ui.vertical(|ui| {
                ui.set_width(420.0);
                ui.group(|ui| {
                    let profile_heading = match lang {
                        Language::Ukrainian => "Профіль гри та маршрутизація",
                        Language::English => "Game Profile & Routing",
                        Language::Russian => "Профиль игры и маршрутизация",
                    };
                    ui.heading(profile_heading);
                    ui.add_space(6.0);

                    let current_game_label = format!(
                        "{} ({})",
                        self.supported_games[self.selected_game].name,
                        self.supported_games[self.selected_game].exe
                    );

                    egui::ComboBox::from_label("")
                        .selected_text(current_game_label)
                        .show_ui(ui, |ui| {
                            for (idx, game) in self.supported_games.iter().enumerate() {
                                let label = format!("{} ({})", game.name, game.exe);
                                if ui.selectable_label(self.selected_game == idx, label).clicked() {
                                    self.selected_game = idx;
                                    if !self.auto_select_best_node {
                                        self.selected_node = game.server_idx;
                                    }
                                }
                            }
                        });

                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(10.0);

                    let node_location = self.nodes[self.selected_node].location(lang);
                    let node_name = self.nodes[self.selected_node].name;
                    let node_ip = self.nodes[self.selected_node].ip;
                    let node_ping_str = match self.nodes[self.selected_node].ping_ms {
                        Some(p) => match lang {
                            Language::Ukrainian => format!(" • Реальний пінг: {} ms", p),
                            Language::English => format!(" • Real ping: {} ms", p),
                            Language::Russian => format!(" • Реальный пинг: {} ms", p),
                        },
                        None => match lang {
                            Language::Ukrainian => " • Замір пінг...".into(),
                            Language::English => " • Measuring ping...".into(),
                            Language::Russian => " • Замер пинга...".into(),
                        },
                    };

                    let relay_lbl = match lang {
                        Language::Ukrainian => "📍 Релей:",
                        Language::English => "📍 Relay:",
                        Language::Russian => "📍 Релей:",
                    };
                    ui.label(format!("{} {} ({}) [IP: {}]{}", relay_lbl, node_location, node_name, node_ip, node_ping_str));

                    ui.add_space(14.0);

                    if !self.is_boosted {
                        let boost_btn_text = match lang {
                            Language::Ukrainian => "🚀 УВІМКНУТИ БУСТ",
                            Language::English => "🚀 ENABLE BOOST",
                            Language::Russian => "🚀 ВКЛЮЧИТЬ БУСТ",
                        };
                        let boost_btn = egui::Button::new(
                            egui::RichText::new(boost_btn_text)
                                .font(egui::FontId::proportional(16.0))
                                .color(egui::Color32::BLACK)
                                .strong(),
                        )
                        .fill(egui::Color32::from_rgb(0, 230, 153))
                        .min_size(egui::vec2(380.0, 52.0));

                        if ui.add(boost_btn).clicked() {
                            self.is_boosted = true;
                            self.boost_start_time = Some(Instant::now());
                            self.start_real_ping_thread();
                            if self.auto_apply_timer_res {
                                self.enable_timer_resolution();
                            }
                            if self.auto_apply_filterkeys {
                                self.apply_filterkeys();
                            }
                        }
                    } else {
                        let stop_btn_text = match lang {
                            Language::Ukrainian => "⏹ ЗУПИНИТИ БУСТ",
                            Language::English => "⏹ STOP BOOST",
                            Language::Russian => "⏹ ОСТАНОВИТЬ БУСТ",
                        };
                        let stop_btn = egui::Button::new(
                            egui::RichText::new(stop_btn_text)
                                .font(egui::FontId::proportional(16.0))
                                .color(egui::Color32::WHITE)
                                .strong(),
                        )
                        .fill(egui::Color32::from_rgb(220, 60, 60))
                        .min_size(egui::vec2(380.0, 52.0));

                        if ui.add(stop_btn).clicked() {
                            self.is_boosted = false;
                            self.boost_start_time = None;
                            if let Some(c) = &self.boost_cancel {
                                c.store(false, Ordering::Relaxed);
                            }
                            if self.auto_apply_timer_res {
                                self.disable_timer_resolution();
                            }
                        }
                    }

                    ui.add_space(10.0);
                    if self.is_boosted {
                        let elapsed = self.boost_start_time.map_or(0, |t| t.elapsed().as_secs());
                        let active_str = match lang {
                            Language::Ukrainian => format!(
                                "Активно: {:02}:{:02} | Канал оптимізовано для {}",
                                elapsed / 60,
                                elapsed % 60,
                                self.supported_games[self.selected_game].name
                            ),
                            Language::English => format!(
                                "Active: {:02}:{:02} | Channel optimized for {}",
                                elapsed / 60,
                                elapsed % 60,
                                self.supported_games[self.selected_game].name
                            ),
                            Language::Russian => format!(
                                "Активно: {:02}:{:02} | Канал оптимизирован для {}",
                                elapsed / 60,
                                elapsed % 60,
                                self.supported_games[self.selected_game].name
                            ),
                        };
                        ui.label(egui::RichText::new(active_str).color(egui::Color32::from_rgb(0, 230, 153)));
                    } else {
                        let desc = match lang {
                            Language::Ukrainian => "WPing оптимізує черги пакетів і спрямовує трафік гри.",
                            Language::English => "WPing optimizes packet queues and prioritizes game traffic.",
                            Language::Russian => "WPing оптимизирует очереди пакетов и направляет трафик игры.",
                        };
                        ui.label(egui::RichText::new(desc).color(egui::Color32::GRAY));
                    }
                });
            });

            ui.add_space(14.0);

            // Right Column
            ui.vertical(|ui| {
                ui.group(|ui| {
                    ui.set_width(360.0);
                    let telem_heading = match lang {
                        Language::Ukrainian => "Реальна телеметрія каналу",
                        Language::English => "Real-Time Channel Telemetry",
                        Language::Russian => "Реальная телеметрия канала",
                    };
                    ui.heading(telem_heading);
                    ui.add_space(10.0);

                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            let ping_lbl = match lang {
                                Language::Ukrainian => "РЕАЛЬНИЙ ПІНГ",
                                Language::English => "REAL PING",
                                Language::Russian => "РЕАЛЬНЫЙ ПИНГ",
                            };
                            ui.label(egui::RichText::new(ping_lbl).font(egui::FontId::proportional(12.0)));
                            let ping_color = if self.current_ping < 20.0 {
                                egui::Color32::from_rgb(0, 255, 128)
                            } else if self.current_ping < 35.0 {
                                egui::Color32::from_rgb(0, 210, 255)
                            } else {
                                egui::Color32::from_rgb(255, 170, 0)
                            };

                            ui.label(
                                egui::RichText::new(format!("{:.0} ms", self.current_ping))
                                    .font(egui::FontId::proportional(36.0))
                                    .color(ping_color)
                                    .strong(),
                            );
                        });

                        ui.add_space(20.0);

                        ui.vertical(|ui| {
                            let loss_lbl = match lang {
                                Language::Ukrainian => "ВТРАТА ПАКЕТІВ",
                                Language::English => "PACKET LOSS",
                                Language::Russian => "ПОТЕРЯ ПАКЕТОВ",
                            };
                            ui.label(egui::RichText::new(loss_lbl).font(egui::FontId::proportional(12.0)));
                            ui.label(
                                egui::RichText::new(format!("{:.1}%", self.packet_loss))
                                    .font(egui::FontId::proportional(22.0))
                                    .color(egui::Color32::from_rgb(0, 230, 153))
                                    .strong(),
                            );
                            ui.add_space(4.0);
                            let jit_lbl = match lang {
                                Language::Ukrainian => "JITTER (ВАРИАЦІЯ)",
                                Language::English => "JITTER (VARIATION)",
                                Language::Russian => "JITTER (ВАРИАЦИЯ)",
                            };
                            ui.label(egui::RichText::new(jit_lbl).font(egui::FontId::proportional(12.0)));
                            ui.label(
                                egui::RichText::new(format!("{:.1} ms", self.jitter))
                                    .font(egui::FontId::proportional(18.0))
                                    .color(egui::Color32::from_rgb(0, 200, 240))
                                    .strong(),
                            );
                        });
                    });

                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(8.0);

                    let graph_lbl = match lang {
                        Language::Ukrainian => "Графік стабільності затримки:",
                        Language::English => "Latency stability graph:",
                        Language::Russian => "График стабильности задержки:",
                    };
                    ui.label(egui::RichText::new(graph_lbl).strong());
                    ui.add_space(6.0);

                    let (response, painter) = ui.allocate_painter(egui::vec2(340.0, 90.0), egui::Sense::hover());
                    let rect = response.rect;

                    painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(12, 15, 20));

                    let count = self.ping_history.len();
                    if count > 1 {
                        let bar_width = rect.width() / (count as f32);
                        for (i, &p) in self.ping_history.iter().enumerate() {
                            let norm = (p / 60.0).clamp(0.05, 1.0);
                            let bar_height = rect.height() * norm;
                            let bar_rect = egui::Rect::from_min_size(
                                egui::pos2(rect.left() + (i as f32) * bar_width + 1.0, rect.bottom() - bar_height),
                                egui::vec2(bar_width - 2.0, bar_height),
                            );

                            let col = if p < 20.0 {
                                egui::Color32::from_rgb(0, 255, 128)
                            } else if p < 35.0 {
                                egui::Color32::from_rgb(0, 210, 255)
                            } else {
                                egui::Color32::from_rgb(255, 170, 0)
                            };

                            painter.rect_filled(bar_rect, 2.0, col);
                        }
                    }
                });
            });
        });
    }


    fn query_timer_resolution(&mut self) {
        unsafe {
            let mut min = 0u32;
            let mut max = 0u32;
            let mut cur = 0u32;
            if NtQueryTimerResolution(&mut min, &mut max, &mut cur) >= 0 {
                self.timer_res_min_100ns = min;
                self.timer_res_max_100ns = max;
                self.timer_res_current_100ns = cur;
                self.timer_res_enabled = cur <= 6000;
            }
        }
    }

    fn enable_timer_resolution(&mut self) {
        unsafe {
            timeBeginPeriod(1);
            let mut cur = 0u32;
            NtSetTimerResolution(5000, 1, &mut cur);
        }
        self.timer_res_enabled = true;
        self.query_timer_resolution();
    }

    fn disable_timer_resolution(&mut self) {
        unsafe {
            let mut cur = 0u32;
            let min = if self.timer_res_min_100ns > 0 { self.timer_res_min_100ns } else { 156250 };
            NtSetTimerResolution(min, 0, &mut cur);
            timeEndPeriod(1);
        }
        self.timer_res_enabled = false;
        self.query_timer_resolution();
    }

    fn query_filterkeys_state(&mut self) {
        let mut fk = FILTERKEYS {
            cbSize: std::mem::size_of::<FILTERKEYS>() as u32,
            dwFlags: 0,
            iWaitMSec: 0,
            iDelayMSec: 0,
            iRepeatMSec: 0,
            iBounceMSec: 0,
        };
        unsafe {
            if SystemParametersInfoW(SPI_GETFILTERKEYS, fk.cbSize, &mut fk as *mut _ as *mut std::ffi::c_void, 0) != 0 {
                self.filterkeys_enabled = (fk.dwFlags & FKF_FILTERKEYSON) != 0;
                if fk.iDelayMSec >= 50 && fk.iDelayMSec <= 2000 {
                    self.filterkeys_delay_ms = fk.iDelayMSec;
                }
                if fk.iRepeatMSec >= 5 && fk.iRepeatMSec <= 1000 {
                    self.filterkeys_repeat_ms = fk.iRepeatMSec;
                }
            }
        }
    }

    fn apply_filterkeys(&mut self) {
        let mut fk = FILTERKEYS {
            cbSize: std::mem::size_of::<FILTERKEYS>() as u32,
            dwFlags: FKF_FILTERKEYSON | FKF_AVAILABLE,
            iWaitMSec: 0,
            iDelayMSec: self.filterkeys_delay_ms,
            iRepeatMSec: self.filterkeys_repeat_ms,
            iBounceMSec: 0,
        };
        unsafe {
            let ret = SystemParametersInfoW(
                SPI_SETFILTERKEYS,
                fk.cbSize,
                &mut fk as *mut _ as *mut std::ffi::c_void,
                SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
            );
            if ret != 0 {
                self.filterkeys_enabled = true;
            }
        }
    }

    fn disable_filterkeys(&mut self) {
        let mut fk = FILTERKEYS {
            cbSize: std::mem::size_of::<FILTERKEYS>() as u32,
            dwFlags: FKF_AVAILABLE,
            iWaitMSec: 0,
            iDelayMSec: 1000,
            iRepeatMSec: 500,
            iBounceMSec: 0,
        };
        unsafe {
            let ret = SystemParametersInfoW(
                SPI_SETFILTERKEYS,
                fk.cbSize,
                &mut fk as *mut _ as *mut std::ffi::c_void,
                SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
            );
            if ret != 0 {
                self.filterkeys_enabled = false;
            }
        }
    }

    fn render_input_delay(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;

        let (heading, sub) = match lang {
            Language::Ukrainian => (
                "⚡ Зниження системної затримки та Інпут лагу (Input & System Delay)",
                "Високоточний таймер ядра Windows (0.500 ms) та моментальний відгук клавіатури FilterKeys для надшвидкого редагу у Fortnite і CS2.",
            ),
            Language::English => (
                "⚡ System Latency & Input Delay Optimizer (0.5ms & FilterKeys)",
                "High-precision Windows kernel timer (0.500 ms) and zero-delay FilterKeys keyboard response for instant edits and wall replaces.",
            ),
            Language::Russian => (
                "⚡ Снижение системной задержки и Инпут лага (Input & System Delay)",
                "Высокоточный таймер ядра Windows (0.500 ms) и моментальный отклик клавиатуры FilterKeys для сверхбыстрого редакта в Fortnite и CS2.",
            ),
        };

        ui.heading(heading);
        ui.label(egui::RichText::new(sub).color(egui::Color32::from_rgb(0, 230, 153)));
        ui.add_space(8.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal(|ui| {
                // LEFT CARD: SetTimerResolution (0.5ms)
                ui.vertical(|ui| {
                    ui.set_width(425.0);
                    ui.group(|ui| {
                        let timer_active = self.timer_res_current_100ns <= 6000;
                        let (card_title, status_text) = match lang {
                            Language::Ukrainian => (
                                "⏱️ Таймер ядра Windows (SetTimerResolution 0.5ms)",
                                if timer_active {
                                    "● 0.500 ms АКТИВНО (МАКСИМАЛЬНИЙ ВІДГУК ЯДРА)"
                                } else {
                                    "○ СТАНДАРТНИЙ ТАЙМЕР WINDOWS (ЗАТРИМКА)"
                                },
                            ),
                            Language::English => (
                                "⏱️ Windows Kernel Timer (SetTimerResolution 0.5ms)",
                                if timer_active {
                                    "● 0.500 ms ACTIVE (MAX KERNEL POLLING)"
                                } else {
                                    "○ DEFAULT WINDOWS TIMER (LATENCY)"
                                },
                            ),
                            Language::Russian => (
                                "⏱️ Таймер ядра Windows (SetTimerResolution 0.5ms)",
                                if timer_active {
                                    "● 0.500 ms АКТИВНО (МАКСИМАЛЬНЫЙ ОТКЛИК ЯДРА)"
                                } else {
                                    "○ СТАНДАРТНЫЙ ТАЙМЕР WINDOWS (ЗАДЕРЖКА)"
                                },
                            ),
                        };

                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(card_title).font(egui::FontId::proportional(15.0)).strong());
                        });
                        ui.add_space(4.0);

                        let badge_color = if timer_active {
                            egui::Color32::from_rgb(0, 255, 128)
                        } else {
                            egui::Color32::from_rgb(255, 170, 50)
                        };
                        ui.label(egui::RichText::new(status_text).font(egui::FontId::proportional(12.0)).color(badge_color).strong());
                        ui.add_space(8.0);

                        // Metrics Box
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(20, 26, 35))
                            .rounding(6.0)
                            .inner_margin(8.0)
                            .show(ui, |ui| {
                                let (lbl_cur, lbl_max, lbl_min) = match lang {
                                    Language::Ukrainian => ("Поточний тік таймера:", "Максимальна точність:", "Стандарт Windows:"),
                                    Language::English => ("Current timer tick:", "Max precision:", "Windows default:"),
                                    Language::Russian => ("Текущий тик таймера:", "Максимальная точность:", "Стандарт Windows:"),
                                };

                                let cur_val = format!("{:.3} ms", self.timer_res_current_100ns as f32 / 10000.0);
                                let max_val = format!("{:.3} ms (2000 Hz)", self.timer_res_max_100ns as f32 / 10000.0);
                                let min_val = format!("{:.3} ms (64 Hz)", self.timer_res_min_100ns as f32 / 10000.0);

                                ui.horizontal(|ui| {
                                    ui.label(lbl_cur);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(egui::RichText::new(cur_val).color(if timer_active { egui::Color32::from_rgb(0, 255, 150) } else { egui::Color32::YELLOW }).strong());
                                    });
                                });
                                ui.horizontal(|ui| {
                                    ui.label(lbl_max);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(egui::RichText::new(max_val).color(egui::Color32::from_rgb(100, 200, 255)));
                                    });
                                });
                                ui.horizontal(|ui| {
                                    ui.label(lbl_min);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(egui::RichText::new(min_val).color(egui::Color32::GRAY));
                                    });
                                });
                            });

                        ui.add_space(10.0);

                        // Buttons
                        let (btn_enable_text, btn_disable_text) = match lang {
                            Language::Ukrainian => ("🚀 УВІМКНУТИ 0.5 ms ТАЙМЕР", "⏸ ПОВЕРНУТИ СТАНДАРТНИЙ"),
                            Language::English => ("🚀 ENABLE 0.5 ms TIMER", "⏸ RESTORE DEFAULT"),
                            Language::Russian => ("🚀 ВКЛЮЧИТЬ 0.5 ms ТАЙМЕР", "⏸ ВЕРНУТЬ СТАНДАРТНЫЙ"),
                        };

                        ui.horizontal(|ui| {
                            let btn_en = egui::Button::new(
                                egui::RichText::new(btn_enable_text).color(egui::Color32::BLACK).strong()
                            ).fill(egui::Color32::from_rgb(0, 230, 153));
                            if ui.add(btn_en).clicked() {
                                self.enable_timer_resolution();
                            }

                            let btn_dis = egui::Button::new(
                                egui::RichText::new(btn_disable_text).color(egui::Color32::WHITE)
                            ).fill(egui::Color32::from_rgb(50, 60, 75));
                            if ui.add(btn_dis).clicked() {
                                self.disable_timer_resolution();
                            }
                        });

                        ui.add_space(8.0);
                        let auto_timer_lbl = match lang {
                            Language::Ukrainian => "Авто-активація 0.5ms при старті бусту або виявленні гри",
                            Language::English => "Auto-activate 0.5ms on game boost / launch",
                            Language::Russian => "Авто-активация 0.5ms при старте буста или обнаружении игры",
                        };
                        ui.checkbox(&mut self.auto_apply_timer_res, auto_timer_lbl);

                        ui.add_space(8.0);
                        let desc_text = match lang {
                            Language::Ukrainian => "Примушує планувальник Windows перевіряти мережеві переривання і пакети 2000 разів на секунду (замість 64). Усуває DPC Latency і мікроривки кадрів (frametime spikes).",
                            Language::English => "Forces Windows thread scheduler to poll network interrupts and packets 2,000 times/sec (instead of 64). Eliminates DPC latency and frame time micro-stutters.",
                            Language::Russian => "Заставляет планировщик Windows опрашивать сетевые прерывания и пакеты 2000 раз в секунду (вместо 64). Устраняет DPC Latency и микрофризы кадров.",
                        };
                        ui.label(egui::RichText::new(desc_text).font(egui::FontId::proportional(11.0)).color(egui::Color32::GRAY));
                    });
                });

                ui.add_space(8.0);

                // RIGHT CARD: FilterKeys (0 Delay Keyboard)
                ui.vertical(|ui| {
                    ui.set_width(425.0);
                    ui.group(|ui| {
                        let fk_active = self.filterkeys_enabled;
                        let (fk_card_title, fk_status_text) = match lang {
                            Language::Ukrainian => (
                                "⌨️ FilterKeys (0 Delay Keyboard / Fast Edit)",
                                if fk_active {
                                    format!("● 0 DELAY АКТИВНО (Затримка: {}ms | Повтор: {}ms)", self.filterkeys_delay_ms, self.filterkeys_repeat_ms)
                                } else {
                                    "○ СТАНДАРТ WINDOWS (Затримка 1000ms / 500ms)".into()
                                },
                            ),
                            Language::English => (
                                "⌨️ FilterKeys (0 Delay Keyboard / Fast Edit)",
                                if fk_active {
                                    format!("● 0 DELAY ACTIVE (Delay: {}ms | Repeat: {}ms)", self.filterkeys_delay_ms, self.filterkeys_repeat_ms)
                                } else {
                                    "○ WINDOWS DEFAULT (Delay: 1000ms / 500ms)".into()
                                },
                            ),
                            Language::Russian => (
                                "⌨️ FilterKeys (0 Delay Keyboard / Fast Edit)",
                                if fk_active {
                                    format!("● 0 DELAY АКТИВНО (Задержка: {}ms | Повтор: {}ms)", self.filterkeys_delay_ms, self.filterkeys_repeat_ms)
                                } else {
                                    "○ СТАНДАРТ WINDOWS (Задержка 1000ms / 500ms)".into()
                                },
                            ),
                        };

                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(fk_card_title).font(egui::FontId::proportional(15.0)).strong());
                        });
                        ui.add_space(4.0);

                        let fk_badge_color = if fk_active {
                            egui::Color32::from_rgb(0, 255, 128)
                        } else {
                            egui::Color32::GRAY
                        };
                        ui.label(egui::RichText::new(fk_status_text).font(egui::FontId::proportional(12.0)).color(fk_badge_color).strong());
                        ui.add_space(8.0);

                        // Presets
                        let preset_lbl = match lang {
                            Language::Ukrainian => "Швидкі пресети:",
                            Language::English => "Quick presets:",
                            Language::Russian => "Быстрые пресеты:",
                        };
                        ui.label(egui::RichText::new(preset_lbl).font(egui::FontId::proportional(12.0)).strong());
                        ui.horizontal(|ui| {
                            if ui.button("🎮 Fortnite Pro (150/25)").clicked() {
                                self.filterkeys_delay_ms = 150;
                                self.filterkeys_repeat_ms = 25;
                                self.apply_filterkeys();
                            }
                            if ui.button("⚡ Ultra Fast (120/20)").clicked() {
                                self.filterkeys_delay_ms = 120;
                                self.filterkeys_repeat_ms = 20;
                                self.apply_filterkeys();
                            }
                            if ui.button("↩ Default").clicked() {
                                self.disable_filterkeys();
                            }
                        });

                        ui.add_space(8.0);

                        // Sliders
                        let (delay_lbl, repeat_lbl) = match lang {
                            Language::Ukrainian => ("Repeat Delay (Затримка першого повтору):", "Repeat Rate (Частота повторів клавіші):"),
                            Language::English => ("Repeat Delay (Initial delay before repeat):", "Repeat Rate (Speed of repeated keys):"),
                            Language::Russian => ("Repeat Delay (Задержка первого повтора):", "Repeat Rate (Частота повторов клавиши):"),
                        };

                        ui.label(egui::RichText::new(delay_lbl).font(egui::FontId::proportional(11.0)));
                        ui.add(egui::Slider::new(&mut self.filterkeys_delay_ms, 100..=300).suffix(" ms"));

                        ui.label(egui::RichText::new(repeat_lbl).font(egui::FontId::proportional(11.0)));
                        ui.add(egui::Slider::new(&mut self.filterkeys_repeat_ms, 10..=50).suffix(" ms"));

                        ui.add_space(8.0);

                        // Apply / Disable buttons
                        let (fk_apply_txt, fk_dis_txt) = match lang {
                            Language::Ukrainian => ("⚡ ЗАСТОСУВАТИ FILTERKEYS", "Вимкнути"),
                            Language::English => ("⚡ APPLY FILTERKEYS", "Disable"),
                            Language::Russian => ("⚡ ПРИМЕНИТЬ FILTERKEYS", "Отключить"),
                        };

                        ui.horizontal(|ui| {
                            let apply_btn = egui::Button::new(
                                egui::RichText::new(fk_apply_txt).color(egui::Color32::BLACK).strong()
                            ).fill(egui::Color32::from_rgb(0, 200, 255));
                            if ui.add(apply_btn).clicked() {
                                self.apply_filterkeys();
                            }

                            let dis_btn = egui::Button::new(
                                egui::RichText::new(fk_dis_txt).color(egui::Color32::WHITE)
                            ).fill(egui::Color32::from_rgb(50, 60, 75));
                            if ui.add(dis_btn).clicked() {
                                self.disable_filterkeys();
                            }
                        });

                        ui.add_space(6.0);
                        let auto_fk_lbl = match lang {
                            Language::Ukrainian => "Авто-застосування 0 Delay при старті бусту гри",
                            Language::English => "Auto-apply 0 Delay on game boost / launch",
                            Language::Russian => "Авто-применение 0 Delay при старте буста игры",
                        };
                        ui.checkbox(&mut self.auto_apply_filterkeys, auto_fk_lbl);

                        ui.add_space(8.0);
                        // Interactive typing test box!
                        let (test_lbl, clear_lbl) = match lang {
                            Language::Ukrainian => ("🧪 Тест відгуку (затисніть будь-яку клавішу тут):", "Очистити"),
                            Language::English => ("🧪 Response test (hold down any key here):", "Clear"),
                            Language::Russian => ("🧪 Тест отклика (зажмите любую клавишу здесь):", "Очистить"),
                        };
                        ui.label(egui::RichText::new(test_lbl).font(egui::FontId::proportional(11.0)).strong());
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut self.filterkeys_test_input);
                            if ui.button(clear_lbl).clicked() {
                                self.filterkeys_test_input.clear();
                            }
                        });
                    });
                });
            });
        });
    }

    fn render_local_tweaks(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;

        let heading = match lang {
            Language::Ukrainian => "Локальні оптимізації Windows (Потрібні права Admin)",
            Language::English => "Windows Local Optimizations (Admin Rights Required)",
            Language::Russian => "Локальные оптимизации Windows (Требуются права Admin)",
        };
        let sub = match lang {
            Language::Ukrainian => "Windows вимагає прав Адміністратора для виправлення реєстру TCP/IP, черг Nagle, пріоритету метрики та тротлінгу.",
            Language::English => "Windows requires Administrator privileges to tweak TCP/IP registry, Nagle queues, route metrics, and throttling.",
            Language::Russian => "Windows требует прав Администратора для изменения реестра TCP/IP, очередей Nagle, приоритета метрики и троттлинга.",
        };

        ui.heading(heading);
        ui.label(egui::RichText::new(sub).color(egui::Color32::from_rgb(255, 200, 100)));
        ui.add_space(8.0);

        self.render_adapter_selector(ui);
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(430.0);
                ui.group(|ui| {
                    let (nagle_title, nagle_desc) = match lang {
                        Language::Ukrainian => (
                            "Вимкнути алгоритм Nagle (TcpAckFrequency=1, TCPNoDelay=1)",
                            "Виправляє затримку пакетів у системному реєстрі.",
                        ),
                        Language::English => (
                            "Disable Nagle's Algorithm (TcpAckFrequency=1, TCPNoDelay=1)",
                            "Eliminates packet buffering latency in the system registry.",
                        ),
                        Language::Russian => (
                            "Отключить алгоритм Nagle (TcpAckFrequency=1, TCPNoDelay=1)",
                            "Устраняет задержку пакетов в системном реестре.",
                        ),
                    };
                    ui.checkbox(&mut self.tweak_nagle, egui::RichText::new(nagle_title).strong());
                    ui.label(egui::RichText::new(nagle_desc).font(egui::FontId::proportional(11.0)).color(egui::Color32::GRAY));
                    ui.add_space(8.0);

                    let (throttling_title, throttling_desc) = match lang {
                        Language::Ukrainian => (
                            "Вимкнути Network Throttling Index (0xffffffff)",
                            "Прибирає тротлінг мережі мультимедійною службою Windows.",
                        ),
                        Language::English => (
                            "Disable Network Throttling Index (0xffffffff)",
                            "Removes network bandwidth throttling by Windows Multimedia service.",
                        ),
                        Language::Russian => (
                            "Отключить Network Throttling Index (0xffffffff)",
                            "Убирает троттлинг сети мультимедийной службой Windows.",
                        ),
                    };
                    ui.checkbox(&mut self.tweak_throttling, egui::RichText::new(throttling_title).strong());
                    ui.label(egui::RichText::new(throttling_desc).font(egui::FontId::proportional(11.0)).color(egui::Color32::GRAY));
                    ui.add_space(8.0);

                    let (resp_title, resp_desc) = match lang {
                        Language::Ukrainian => (
                            "System Responsiveness = 0 (100% ресурсів іграм)",
                            "Резервує 0% для фонових процесів, віддаючи мережу грі.",
                        ),
                        Language::English => (
                            "System Responsiveness = 0 (100% gaming priority)",
                            "Reserves 0% for background tasks, giving full network priority to games.",
                        ),
                        Language::Russian => (
                            "System Responsiveness = 0 (100% ресурсов играм)",
                            "Резервирует 0% для фоновых задач, отдавая сеть игре.",
                        ),
                    };
                    ui.checkbox(&mut self.tweak_system_resp, egui::RichText::new(resp_title).strong());
                    ui.label(egui::RichText::new(resp_desc).font(egui::FontId::proportional(11.0)).color(egui::Color32::GRAY));
                    ui.add_space(8.0);

                    let (metric_title, metric_desc) = match lang {
                        Language::Ukrainian => (
                            "Пріоритет фізичного адаптера над VPN (Interface Metric = 5 vs 50)",
                            "Запобігає захопленню маршрутизації та трафіку віртуальними мережами Radmin VPN / Hamachi.",
                        ),
                        Language::English => (
                            "Physical Adapter Metric Priority over VPN (Metric = 5 vs 50)",
                            "Prevents virtual networks like Radmin VPN / Hamachi from hijacking default routing.",
                        ),
                        Language::Russian => (
                            "Приоритет физического адаптера над VPN (Interface Metric = 5 vs 50)",
                            "Предотвращает перехват маршрутизации виртуальными сетями Radmin VPN / Hamachi.",
                        ),
                    };
                    ui.checkbox(&mut self.tweak_nic_rss, egui::RichText::new(metric_title).strong());
                    ui.label(egui::RichText::new(metric_desc).font(egui::FontId::proportional(11.0)).color(egui::Color32::GRAY));
                    ui.add_space(8.0);

                    let (rss_title, rss_desc) = match lang {
                        Language::Ukrainian => (
                            "Оптимізувати RSS & Вимкнути енергозбереження карти",
                            "Вмикає Receive Side Scaling на всіх ядрах CPU.",
                        ),
                        Language::English => (
                            "Optimize RSS & Disable NIC Power Saving",
                            "Enables Receive Side Scaling across all CPU cores.",
                        ),
                        Language::Russian => (
                            "Оптимизировать RSS и отключить энергосбережение карты",
                            "Включает Receive Side Scaling на всех ядрах CPU.",
                        ),
                    };
                    ui.checkbox(&mut self.tweak_auto_mtu, egui::RichText::new(rss_title).strong());
                    ui.label(egui::RichText::new(rss_desc).font(egui::FontId::proportional(11.0)).color(egui::Color32::GRAY));

                    ui.add_space(14.0);

                    let apply_btn_text = match lang {
                        Language::Ukrainian => "🛡️ ЗАСТОСУВАТИ ТВІКИ (RUN AS ADMIN)",
                        Language::English => "🛡️ APPLY TWEAKS (RUN AS ADMIN)",
                        Language::Russian => "🛡️ ПРИМЕНИТЬ ТВИКИ (RUN AS ADMIN)",
                    };
                    let apply_btn = egui::Button::new(
                        egui::RichText::new(apply_btn_text)
                            .font(egui::FontId::proportional(14.0))
                            .color(egui::Color32::BLACK)
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(0, 230, 153))
                    .min_size(egui::vec2(380.0, 44.0));

                    if ui.add(apply_btn).clicked() {
                        self.run_admin_tweak_script();
                    }
                });
            });

            ui.add_space(14.0);

            // Log output
            ui.vertical(|ui| {
                ui.group(|ui| {
                    ui.set_width(340.0);
                    let log_heading = match lang {
                        Language::Ukrainian => "Журнал налаштувань реєстру:",
                        Language::English => "Registry Optimization Log:",
                        Language::Russian => "Журнал настроек реестра:",
                    };
                    ui.heading(log_heading);
                    ui.add_space(6.0);

                    egui::ScrollArea::vertical()
                        .max_height(280.0)
                        .show(ui, |ui| {
                            for log in &self.tweak_status_log {
                                ui.label(
                                    egui::RichText::new(log)
                                        .font(egui::FontId::monospace(11.0))
                                        .color(egui::Color32::from_rgb(0, 210, 140)),
                                );
                            }
                        });
                });
            });
        });
    }

    fn render_cloud_relay(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;

        let heading = match lang {
            Language::Ukrainian => "Хмарна маршрутизація (Cloudflare Anycast / Oracle)",
            Language::English => "Cloud Routing (Cloudflare Anycast / Oracle)",
            Language::Russian => "Облачная маршрутизация (Cloudflare Anycast / Oracle)",
        };
        let sub = match lang {
            Language::Ukrainian => "● Оновлення затримки кожні 10 сек (Живий замір з вашого ПК)",
            Language::English => "● Live latency refresh every 10s (Real measurement from your PC)",
            Language::Russian => "● Обновление задержки каждые 10 сек (Живой замер с вашего ПК)",
        };
        let smart_route = match lang {
            Language::Ukrainian => "Авто-вибір найшвидшого (Smart Routing)",
            Language::English => "Auto-select fastest node (Smart Routing)",
            Language::Russian => "Авто-выбор самого быстрого (Smart Routing)",
        };

        ui.heading(heading);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(sub)
                    .color(egui::Color32::from_rgb(0, 255, 128))
                    .strong(),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.checkbox(&mut self.auto_select_best_node, smart_route);
            });
        });

        ui.add_space(8.0);
        self.render_adapter_selector(ui);
        ui.add_space(8.0);

        ui.group(|ui| {
            let servers_heading = match lang {
                Language::Ukrainian => "Доступні релей-сервери (Актуальний замір з вашого інтернет-каналу):",
                Language::English => "Available Relay Servers (Real-time measurement from your connection):",
                Language::Russian => "Доступные релей-серверы (Актуальный замер с вашего интернет-канала):",
            };
            ui.heading(servers_heading);
            ui.add_space(6.0);

            for (idx, node) in self.nodes.iter().enumerate() {
                ui.horizontal(|ui| {
                    let is_sel = self.selected_node == idx;
                    if ui.selectable_label(is_sel, format!("📍 {}", node.location(lang))).clicked() {
                        self.selected_node = idx;
                        self.auto_select_best_node = false;
                        if let Some(p) = node.ping_ms {
                            self.current_ping = p as f32;
                        }
                    }

                    ui.label(format!("• {} ({})", node.name, node.ip));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(p) = node.ping_ms {
                            let col = if p < 20 {
                                egui::Color32::from_rgb(0, 255, 128)
                            } else if p < 40 {
                                egui::Color32::from_rgb(0, 210, 255)
                            } else {
                                egui::Color32::from_rgb(255, 170, 0)
                            };
                            ui.label(egui::RichText::new(format!("{} ms", p)).color(col).strong());
                        } else {
                            let wait_lbl = match lang {
                                Language::Ukrainian => "⏳ Замір...",
                                Language::English => "⏳ Measuring...",
                                Language::Russian => "⏳ Замер...",
                            };
                            ui.label(egui::RichText::new(wait_lbl).color(egui::Color32::GRAY));
                        }
                    });
                });
                ui.separator();
            }

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let refresh_all_text = match lang {
                    Language::Ukrainian => "🔄 Оновити пінг усіх серверів негайно",
                    Language::English => "🔄 Refresh all server pings now",
                    Language::Russian => "🔄 Обновить пинг всех серверов немедленно",
                };
                if ui.button(refresh_all_text).clicked() {
                    self.refresh_nodes_ping();
                }

                if let Some(a) = self.adapters.get(self.selected_adapter_idx) {
                    let active_lbl = match lang {
                        Language::Ukrainian => "Активний канал:",
                        Language::English => "Active channel:",
                        Language::Russian => "Активный канал:",
                    };
                    ui.label(
                        egui::RichText::new(format!("{} {} ({})", active_lbl, a.description, a.ip.as_deref().unwrap_or("Немає IP")))
                            .font(egui::FontId::proportional(11.0))
                            .color(egui::Color32::from_rgb(140, 160, 185)),
                    );
                }
            });
        });
    }

    fn render_diagnostics(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;

        let heading = match lang {
            Language::Ukrainian => "Діагностика мережі та тест затримки",
            Language::English => "Network Diagnostics & Latency Test",
            Language::Russian => "Диагностика сети и тест задержки",
        };
        let sub = match lang {
            Language::Ukrainian => "Перевірте реальний пінг до будь-якого ігрового сервера або IP-адреси.",
            Language::English => "Check real ping to any game server or IP address.",
            Language::Russian => "Проверьте реальный пинг к любому игровому серверу или IP-адресу.",
        };

        ui.heading(heading);
        ui.label(egui::RichText::new(sub).color(egui::Color32::from_rgb(150, 165, 185)));
        ui.add_space(8.0);

        self.render_adapter_selector(ui);
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            let host_lbl = match lang {
                Language::Ukrainian => "Цільовий хост / IP:",
                Language::English => "Target Host / IP:",
                Language::Russian => "Целевой хост / IP:",
            };
            ui.label(host_lbl);
            ui.text_edit_singleline(&mut self.diag_host);

            let test_btn_text = if self.is_pinging {
                match lang {
                    Language::Ukrainian => "⏳ Тестування...",
                    Language::English => "⏳ Testing...",
                    Language::Russian => "⏳ Тестирование...",
                }
            } else {
                match lang {
                    Language::Ukrainian => "▶ Тестувати",
                    Language::English => "▶ Run Test",
                    Language::Russian => "▶ Тестировать",
                }
            };
            let ping_btn = egui::Button::new(test_btn_text);
            if ui.add_enabled(!self.is_pinging, ping_btn).clicked() {
                self.run_ping_test();
            }
        });

        ui.add_space(14.0);

        ui.group(|ui| {
            let res_heading = match lang {
                Language::Ukrainian => "Результати діагностики:",
                Language::English => "Diagnostic Results:",
                Language::Russian => "Результаты диагностики:",
            };
            ui.heading(res_heading);
            ui.add_space(6.0);

            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    if self.diag_results.is_empty() {
                        let hint = match lang {
                            Language::Ukrainian => "Натисніть 'Тестувати' для запуску перевірки.",
                            Language::English => "Click 'Run Test' to begin verification.",
                            Language::Russian => "Нажмите 'Тестировать' для запуска проверки.",
                        };
                        ui.label(egui::RichText::new(hint).color(egui::Color32::GRAY));
                    } else {
                        for res in &self.diag_results {
                            ui.label(egui::RichText::new(res).font(egui::FontId::monospace(12.0)));
                        }
                    }
                });
        });
    }

    fn run_admin_tweak_script(&mut self) {
        let adapter_alias = self.adapters.get(self.selected_adapter_idx).map(|a| a.name.clone()).unwrap_or_default();
        let log_msg = match self.language {
            Language::Ukrainian => format!("Запуск оптимізації для адаптера '{}' з правами адміністратора...", adapter_alias),
            Language::English => format!("Starting optimization for adapter '{}' with admin rights...", adapter_alias),
            Language::Russian => format!("Запуск оптимизации для адаптера '{}' с правами администратора...", adapter_alias),
        };
        self.tweak_status_log.push(log_msg);
        let (tx, rx) = mpsc::channel();
        self.admin_log_rx = Some(rx);
        let lang = self.language;

        thread::spawn(move || {
            let _ = std::process::Command::new("powershell")
                .args(&[
                    "-NoProfile",
                    "-Command",
                    &format!("Start-Process -FilePath '.\\Apply_Tweaks_Admin.bat' -ArgumentList '\"{}\"' -Verb RunAs", adapter_alias),
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .status();

            let (m1, m2, m3, m4) = match lang {
                Language::Ukrainian => (
                    "✔ Запит UAC надіслано. Підтвердіть запуск від імені Адміністратора.",
                    "✔ Надано найвищий пріоритет (метрику 5) фізичній мережевій карті.",
                    "✔ Пріоритет віртуальних VPN (Radmin/Hamachi) знижено (метрика 50).",
                    "✔ Скрипт виправляє TcpAckFrequency=1, TCPNoDelay=1 та NetworkThrottling.",
                ),
                Language::English => (
                    "✔ UAC request sent. Confirm Run as Administrator prompt.",
                    "✔ Highest priority (metric 5) assigned to physical network card.",
                    "✔ Virtual VPN priority (Radmin/Hamachi) lowered (metric 50).",
                    "✔ Configured TcpAckFrequency=1, TCPNoDelay=1, and NetworkThrottling.",
                ),
                Language::Russian => (
                    "✔ Запрос UAC отправлен. Подтвердите запуск от имени Администратора.",
                    "✔ Предоставлен наивысший приоритет (метрика 5) физической сетевой карте.",
                    "✔ Приоритет виртуальных VPN (Radmin/Hamachi) снижен (метрика 50).",
                    "✔ Скрипт исправляет TcpAckFrequency=1, TCPNoDelay=1 и NetworkThrottling.",
                ),
            };

            let _ = tx.send(m1.into());
            let _ = tx.send(m2.into());
            let _ = tx.send(m3.into());
            let _ = tx.send(m4.into());
        });
    }

    fn refresh_nodes_ping(&mut self) {
        let nodes: Vec<(usize, String)> = self.nodes.iter().enumerate().map(|(i, n)| (i, n.ip.to_string())).collect();
        let (tx, rx) = mpsc::channel();
        self.node_ping_rx = Some(rx);
        let bind_ip = self.shared_adapter_ip.read().ok().and_then(|g| g.clone());

        thread::spawn(move || {
            for (idx, ip) in nodes {
                let mut cmd = std::process::Command::new("ping");
                cmd.args(&["-n", "1", "-w", "800"]);
                if let Some(ref b_ip) = bind_ip {
                    cmd.args(&["-S", b_ip]);
                }
                cmd.arg(&ip);
                cmd.creation_flags(CREATE_NO_WINDOW);

                if let Ok(out) = cmd.output() {
                    let text = String::from_utf8_lossy(&out.stdout);
                    for part in text.split_whitespace() {
                        if part.starts_with("time=") || part.starts_with("час=") {
                            let ms_str: String = part.chars().filter(|c| c.is_digit(10)).collect();
                            if let Ok(ms) = ms_str.parse::<u32>() {
                                let _ = tx.send((idx, ms));
                                break;
                            }
                        } else if part.contains("time<1ms") || part.contains("час<1мс") {
                            let _ = tx.send((idx, 1));
                            break;
                        }
                    }
                }
            }
        });
    }

    fn run_ping_test(&mut self) {
        self.is_pinging = true;
        self.diag_results.clear();
        let bind_ip = self.shared_adapter_ip.read().ok().and_then(|g| g.clone());
        let bind_str = match &bind_ip {
            Some(ip) => match self.language {
                Language::Ukrainian => format!(" (через адаптер IP: {})", ip),
                Language::English => format!(" (via adapter IP: {})", ip),
                Language::Russian => format!(" (через адаптер IP: {})", ip),
            },
            None => "".into(),
        };

        let start_msg = match self.language {
            Language::Ukrainian => format!("Запуск тестування до {}{} ...", self.diag_host, bind_str),
            Language::English => format!("Starting test to {}{} ...", self.diag_host, bind_str),
            Language::Russian => format!("Запуск тестирования к {}{} ...", self.diag_host, bind_str),
        };
        self.diag_results.push(start_msg);

        let host = self.diag_host.clone();
        let (tx, rx) = mpsc::channel();
        self.diag_rx = Some(rx);

        thread::spawn(move || {
            let mut cmd = std::process::Command::new("ping");
            cmd.args(&["-n", "4", "-w", "1000"]);
            if let Some(ref b_ip) = bind_ip {
                cmd.args(&["-S", b_ip]);
            }
            cmd.arg(&host);
            cmd.creation_flags(CREATE_NO_WINDOW);

            match cmd.output() {
                Ok(out) => {
                    let s = String::from_utf8_lossy(&out.stdout).to_string();
                    let lines: Vec<&str> = s.lines().collect();
                    for line in lines {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            let _ = tx.send(trimmed.to_string());
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(format!("Помилка виконання ping: {}", e));
                }
            }
        });
    }

    fn start_real_ping_thread(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.ping_rx = Some(rx);
        let target_ip = self.nodes[self.selected_node].ip.to_string();
        let adapter_ip_arc = self.shared_adapter_ip.clone();

        let cancel = Arc::new(AtomicBool::new(true));
        self.boost_cancel = Some(cancel.clone());

        thread::spawn(move || {
            while cancel.load(Ordering::Relaxed) {
                let bind_ip = adapter_ip_arc.read().ok().and_then(|g| g.clone());
                let mut cmd = std::process::Command::new("ping");
                cmd.args(&["-n", "1", "-w", "900"]);
                if let Some(ref b_ip) = bind_ip {
                    cmd.args(&["-S", b_ip]);
                }
                cmd.arg(&target_ip);
                cmd.creation_flags(CREATE_NO_WINDOW);

                if let Ok(out) = cmd.output() {
                    let text = String::from_utf8_lossy(&out.stdout);
                    let mut found = false;
                    for part in text.split_whitespace() {
                        if part.starts_with("time=") || part.starts_with("час=") {
                            let ms_str: String = part
                                .chars()
                                .filter(|c| c.is_digit(10))
                                .collect();
                            if let Ok(ms) = ms_str.parse::<f32>() {
                                let _ = tx.send(ms);
                                found = true;
                                break;
                            }
                        } else if part.contains("time<1ms") || part.contains("час<1мс") {
                            let _ = tx.send(1.0);
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        let _ = tx.send(25.0);
                    }
                }

                thread::sleep(Duration::from_millis(1000));
            }
        });
    }
}

fn scan_for_games(supported_games: &[GameDefinition]) -> Option<DetectedGame> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE || snapshot == 0 {
            return None;
        }

        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let null_pos = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());

                let exe_name = String::from_utf16_lossy(&entry.szExeFile[..null_pos]);

                for game in supported_games {
                    if exe_name.eq_ignore_ascii_case(game.exe) {
                        let pid = entry.th32ProcessID;
                        CloseHandle(snapshot);
                        return Some(DetectedGame {
                            name: game.name.to_string(),
                            exe: game.exe.to_string(),
                            pid,
                        });
                    }
                }

                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }

        CloseHandle(snapshot);
    }

    None
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([910.0, 630.0])
            .with_min_inner_size([800.0, 540.0])
            .with_title(APP_WINDOW_TITLE),
        ..Default::default()
    };

    eframe::run_native(
        APP_WINDOW_TITLE,
        native_options,
        Box::new(|cc| {
            let mut app = AppState::default();
            app.query_timer_resolution();
            app.query_filterkeys_state();

            // Background Network Adapters Detection (queries all, sorts physical first)
            let (adapter_tx, adapter_rx) = mpsc::channel();
            app.adapters_rx = Some(adapter_rx);
            let egui_ctx_adapters = cc.egui_ctx.clone();
            thread::spawn(move || {
                let list = query_network_adapters();
                let _ = adapter_tx.send(list);
                egui_ctx_adapters.request_repaint();
            });

            // Background Relay Nodes Ping Thread - Updates ALL nodes every 10 seconds!
            let (node_tx, node_rx) = mpsc::channel();
            app.node_ping_rx = Some(node_rx);
            let nodes_info: Vec<(usize, String)> = app
                .nodes
                .iter()
                .enumerate()
                .map(|(i, n)| (i, n.ip.to_string()))
                .collect();
            let egui_ctx_nodes = cc.egui_ctx.clone();
            let adapter_ip_arc = app.shared_adapter_ip.clone();

            thread::spawn(move || {
                loop {
                    let bind_ip = adapter_ip_arc.read().ok().and_then(|g| g.clone());

                    for &(idx, ref ip) in &nodes_info {
                        let mut cmd = std::process::Command::new("ping");
                        cmd.args(&["-n", "1", "-w", "800"]);
                        if let Some(ref b_ip) = bind_ip {
                            cmd.args(&["-S", b_ip]);
                        }
                        cmd.arg(ip);
                        cmd.creation_flags(CREATE_NO_WINDOW);

                        if let Ok(out) = cmd.output() {
                            let text = String::from_utf8_lossy(&out.stdout);
                            for part in text.split_whitespace() {
                                if part.starts_with("time=") || part.starts_with("час=") {
                                    let ms_str: String = part.chars().filter(|c| c.is_digit(10)).collect();
                                    if let Ok(ms) = ms_str.parse::<u32>() {
                                        let _ = node_tx.send((idx, ms));
                                        break;
                                    }
                                } else if part.contains("time<1ms") || part.contains("час<1мс") {
                                    let _ = node_tx.send((idx, 1));
                                    break;
                                }
                            }
                        }
                    }
                    egui_ctx_nodes.request_repaint();
                    thread::sleep(Duration::from_secs(10));
                }
            });

            // Background Game Auto-Detector thread
            let (detect_tx, detect_rx) = mpsc::channel();
            app.detect_rx = Some(detect_rx);
            let games_list = app.supported_games.clone();
            let egui_ctx_games = cc.egui_ctx.clone();

            thread::spawn(move || loop {
                let found = scan_for_games(&games_list);
                let _ = detect_tx.send(found);
                egui_ctx_games.request_repaint();
                thread::sleep(Duration::from_millis(1500));
            });

            // Setup Tray Icon
            let (show_tx, show_rx) = mpsc::channel();
            app.tray_show_rx = Some(show_rx);

            let icon = unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::LoadIconW(
                    0,
                    windows_sys::Win32::UI::WindowsAndMessaging::IDI_APPLICATION,
                )
            };

            let mut tray = TrayItem::new(APP_WINDOW_TITLE, IconSource::RawIcon(icon))
                .or_else(|_| TrayItem::new("WPing", IconSource::Resource("")))
                .ok();

            if let Some(tray_item) = &mut tray {
                let _ = tray_item.add_label("⚡ WPing • Created by Vsqy");
                let _ = tray_item.add_label("───────────────");

                let show_tx_c = show_tx.clone();
                let ctx_c1 = cc.egui_ctx.clone();
                let _ = tray_item.add_menu_item("Показати WPing", move || {
                    let _ = show_tx_c.send(());
                    let hwnd = MAIN_HWND.load(Ordering::Relaxed);
                    unsafe {
                        use windows_sys::Win32::UI::WindowsAndMessaging::{
                            FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
                        };
                        let target = if hwnd != 0 {
                            hwnd as _
                        } else {
                            let title: Vec<u16> = APP_WINDOW_TITLE
                                .encode_utf16()
                                .chain(std::iter::once(0))
                                .collect();
                            FindWindowW(std::ptr::null(), title.as_ptr())
                        };

                        if target != 0 {
                            ShowWindow(target, SW_SHOW);
                            ShowWindow(target, SW_RESTORE);
                            SetForegroundWindow(target);
                        }
                    }
                    ctx_c1.request_repaint();
                });

                let _ = tray_item.add_label("───────────────");

                let _ = tray_item.add_menu_item("❌ Вийти з програми", move || {
                    std::process::exit(0);
                });
            }

            app._tray = tray;

            Ok(Box::new(app))
        }),
    )
}


impl Drop for AppState {
    fn drop(&mut self) {
        self.disable_timer_resolution();
    }
}
