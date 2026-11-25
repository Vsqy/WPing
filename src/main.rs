#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod net_tweaks;
mod sys_native;

use eframe::egui;
use net_tweaks::{
    apply_dns_server, apply_mtu_to_adapter, apply_selected_tweaks, default_dns_presets,
    query_network_adapters, reset_dns_dhcp, rollback_default_tweaks, DnsPreset, NetworkAdapter,
    TweakOptions,
};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{mpsc, Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use sys_native::{
    activate_power_plan, activate_ultimate_or_high_perf, apply_filterkeys,
    balance_processes_for_gaming, clean_ram_and_standby, disable_05ms_timer, disable_filterkeys,
    discover_optimal_mtu, enable_05ms_timer, get_active_power_plan, is_mouse_accel_enabled,
    is_user_admin, native_icmp_ping, query_filterkeys, query_memory_stats,
    query_timer_resolution, resolve_ipv4, restart_as_admin, restore_balanced_processes,
    scan_for_games, set_mouse_accel, DetectedGame, GameDefinition, IcmpProber, MemoryStats,
};
use tray_item::{IconSource, TrayItem};

const APP_WINDOW_TITLE: &str = "WPing v2.0 - Ultimate Latency, Ping & FPS Optimizer";
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

#[derive(PartialEq, Clone, Copy)]
enum AppTab {
    Dashboard,
    InputFps,
    NetworkDns,
    LocalTweaks,
    Diagnostics,
}

#[derive(Clone)]
struct ServerNode {
    name: &'static str,
    loc_ua: &'static str,
    loc_en: &'static str,
    loc_ru: &'static str,
    host: &'static str,
    ping_ms: Option<f32>,
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

#[derive(Clone, Copy)]
struct PingTelemetrySample {
    rtt_ms: Option<f32>,
}

struct AppState {
    language: Language,
    current_tab: AppTab,
    is_admin: bool,

    // Games database & detection
    supported_games: Vec<GameDefinition>,
    selected_game: usize,
    detected_game: Option<DetectedGame>,
    auto_boost_on_detect: bool,

    // Boost Master State
    is_boosted: bool,
    boost_start_time: Option<Instant>,
    auto_apply_timer_res: bool,
    auto_apply_filterkeys: bool,
    auto_apply_power_plan: bool,
    auto_clean_ram: bool,
    auto_throttle_bg_procs: bool,
    saved_power_plan_guid: Option<String>,
    active_power_plan_name: String,
    adjusted_bg_pids: Vec<(u32, u32)>,
    last_ram_clean: Option<Instant>,

    // Tweaks Options & Logs
    tweak_opts: TweakOptions,
    tweak_status_log: Vec<String>,
    tweaks_busy: bool,
    tweaks_rx: Option<mpsc::Receiver<Vec<String>>>,

    // Network Adapters & DNS & MTU
    adapters: Vec<NetworkAdapter>,
    selected_adapter_idx: usize,
    shared_adapter_ip: Arc<RwLock<Option<String>>>,
    dns_presets: Vec<DnsPreset>,
    dns_bench_rx: Option<mpsc::Receiver<(usize, Option<f32>)>>,
    is_benching_dns: bool,
    mtu_result: Option<u16>,
    mtu_status_msg: String,
    is_probing_mtu: bool,
    mtu_rx: Option<mpsc::Receiver<(Option<u16>, String)>>,

    // Cloud Relays (8 nodes probed natively every 10s)
    nodes: Vec<ServerNode>,
    selected_node: usize,
    auto_select_best_node: bool,
    last_node_update: Instant,
    node_ping_rx: Option<mpsc::Receiver<(usize, Option<f32>)>>,

    // Diagnostics
    diag_host: String,
    diag_results: Vec<String>,
    is_pinging: bool,
    diag_rx: Option<mpsc::Receiver<(String, bool)>>,

    // Real-time Telemetry Chart & Exact Stats
    ping_history: VecDeque<f32>,
    recent_outcomes: VecDeque<bool>,
    current_ping: f32,
    packet_loss: f32,
    jitter: f32,
    ping_rx: Option<mpsc::Receiver<PingTelemetrySample>>,
    boost_cancel: Option<Arc<AtomicBool>>,

    // Timer Resolution (0.500ms)
    timer_res_enabled: bool,
    timer_res_current_100ns: u32,
    timer_res_min_100ns: u32,
    timer_res_max_100ns: u32,

    // FilterKeys & Raw Mouse
    filterkeys_enabled: bool,
    filterkeys_delay_ms: u32,
    filterkeys_repeat_ms: u32,
    filterkeys_test_input: String,
    mouse_accel_enabled: bool,

    // Live Memory Stats
    mem_stats: MemoryStats,
    ram_clean_status: String,
    last_metrics_poll: Instant,

    // Async Receivers
    detect_rx: Option<mpsc::Receiver<Option<DetectedGame>>>,
    adapters_rx: Option<mpsc::Receiver<Vec<NetworkAdapter>>>,

    // System Tray
    _tray: Option<TrayItem>,
    tray_show_rx: Option<mpsc::Receiver<()>>,
    allow_exit: bool,
}

impl Default for AppState {
    fn default() -> Self {
        let lang = detect_system_language();
        let mut history = VecDeque::with_capacity(40);
        for _ in 0..40 {
            history.push_back(15.0);
        }

        let supported_games = vec![
            GameDefinition { name: "Counter-Strike 2", exe: "cs2.exe", server_idx: 0 },
            GameDefinition { name: "Fortnite", exe: "FortniteClient-Win64-Shipping.exe", server_idx: 3 },
            GameDefinition { name: "Valorant", exe: "VALORANT-Win64-Shipping.exe", server_idx: 1 },
            GameDefinition { name: "Dota 2", exe: "dota2.exe", server_idx: 2 },
            GameDefinition { name: "Apex Legends", exe: "r5apex.exe", server_idx: 1 },
            GameDefinition { name: "PUBG: BATTLEGROUNDS", exe: "TslGame.exe", server_idx: 1 },
            GameDefinition { name: "League of Legends", exe: "League of Legends.exe", server_idx: 1 },
            GameDefinition { name: "Overwatch 2", exe: "Overwatch.exe", server_idx: 1 },
            GameDefinition { name: "Deadlock", exe: "project8.exe", server_idx: 1 },
            GameDefinition { name: "Escape from Tarkov", exe: "EscapeFromTarkov.exe", server_idx: 0 },
            GameDefinition { name: "Rust", exe: "RustClient.exe", server_idx: 1 },
            GameDefinition { name: "Rainbow Six Siege", exe: "RainbowSix.exe", server_idx: 1 },
            GameDefinition { name: "Call of Duty: Warzone", exe: "cod.exe", server_idx: 1 },
            GameDefinition { name: "Grand Theft Auto V", exe: "GTA5.exe", server_idx: 1 },
            GameDefinition { name: "Rocket League", exe: "RocketLeague.exe", server_idx: 3 },
            GameDefinition { name: "Marvel Rivals", exe: "Marvel-Win64-Shipping.exe", server_idx: 1 },
            GameDefinition { name: "The Finals", exe: "Discovery.exe", server_idx: 1 },
            GameDefinition { name: "Minecraft", exe: "javaw.exe", server_idx: 5 },
            GameDefinition { name: "Roblox", exe: "RobloxPlayerBeta.exe", server_idx: 5 },
        ];

        let initial_logs = match lang {
            Language::Ukrainian => vec![
                "WPing v2.0 готовий до роботи.".into(),
                "Нативний ICMP двигун активовано (0 навантаження на CPU).".into(),
            ],
            Language::English => vec![
                "WPing v2.0 ready.".into(),
                "Native ICMP kernel telemetry active (0% CPU overhead).".into(),
            ],
            Language::Russian => vec![
                "WPing v2.0 готов к работе.".into(),
                "Нативный ICMP движок активирован (0 нагрузки на CPU).".into(),
            ],
        };

        let active_plan_name = get_active_power_plan()
            .map(|p| p.name)
            .unwrap_or_else(|| "Windows Default".to_string());

        Self {
            language: lang,
            current_tab: AppTab::Dashboard,
            is_admin: is_user_admin(),

            supported_games,
            selected_game: 0,
            detected_game: None,
            auto_boost_on_detect: true,

            is_boosted: false,
            boost_start_time: None,
            auto_apply_timer_res: true,
            auto_apply_filterkeys: true,
            auto_apply_power_plan: true,
            auto_clean_ram: true,
            auto_throttle_bg_procs: true,
            saved_power_plan_guid: None,
            active_power_plan_name: active_plan_name,
            adjusted_bg_pids: Vec::new(),
            last_ram_clean: None,

            tweak_opts: TweakOptions::default(),
            tweak_status_log: initial_logs,
            tweaks_busy: false,
            tweaks_rx: None,

            adapters: Vec::new(),
            selected_adapter_idx: 0,
            shared_adapter_ip: Arc::new(RwLock::new(None)),
            dns_presets: default_dns_presets(),
            dns_bench_rx: None,
            is_benching_dns: false,
            mtu_result: None,
            mtu_status_msg: String::new(),
            is_probing_mtu: false,
            mtu_rx: None,

            nodes: vec![
                ServerNode {
                    name: "Valve Game Relay",
                    loc_ua: "Варшава (EU East) 🇵🇱",
                    loc_en: "Warsaw (EU East) 🇵🇱",
                    loc_ru: "Варшава (EU East) 🇵🇱",
                    host: "155.133.230.1",
                    ping_ms: None,
                },
                ServerNode {
                    name: "Valve Game Relay",
                    loc_ua: "Франкфурт (EU Central) 🇩🇪",
                    loc_en: "Frankfurt (EU Central) 🇩🇪",
                    loc_ru: "Франкфурт (EU Central) 🇩🇪",
                    host: "155.133.226.1",
                    ping_ms: None,
                },
                ServerNode {
                    name: "Valve Game Relay",
                    loc_ua: "Відень (Austria) 🇦🇹",
                    loc_en: "Vienna (Austria) 🇦🇹",
                    loc_ru: "Вена (Austria) 🇦🇹",
                    host: "155.133.248.1",
                    ping_ms: None,
                },
                ServerNode {
                    name: "Fortnite / AWS EU",
                    loc_ua: "Франкфурт (AWS Central) 🇩🇪",
                    loc_en: "Frankfurt (AWS Central) 🇩🇪",
                    loc_ru: "Франкфурт (AWS Central) 🇩🇪",
                    host: "dynamodb.eu-central-1.amazonaws.com",
                    ping_ms: None,
                },
                ServerNode {
                    name: "Fortnite / AWS EU",
                    loc_ua: "Лондон (AWS West) 🇬🇧",
                    loc_en: "London (AWS West) 🇬🇧",
                    loc_ru: "Лондон (AWS West) 🇬🇧",
                    host: "dynamodb.eu-west-2.amazonaws.com",
                    ping_ms: None,
                },
                ServerNode {
                    name: "Cloudflare Anycast",
                    loc_ua: "Найближчий Edge (Anycast) ⚡",
                    loc_en: "Nearest Edge (Anycast) ⚡",
                    loc_ru: "Ближайший Edge (Anycast) ⚡",
                    host: "1.1.1.1",
                    ping_ms: None,
                },
                ServerNode {
                    name: "AWS Global Accelerator",
                    loc_ua: "Європа Global Gaming 🌐",
                    loc_en: "Europe Global Gaming 🌐",
                    loc_ru: "Европа Global Gaming 🌐",
                    host: "13.248.158.1",
                    ping_ms: None,
                },
                ServerNode {
                    name: "Valve Game Relay",
                    loc_ua: "Стокгольм (EU North) 🇸🇪",
                    loc_en: "Stockholm (EU North) 🇸🇪",
                    loc_ru: "Стокгольм (EU North) 🇸🇪",
                    host: "162.254.197.1",
                    ping_ms: None,
                },
            ],
            selected_node: 0,
            auto_select_best_node: true,
            last_node_update: Instant::now(),
            node_ping_rx: None,

            diag_host: "155.133.226.1".into(),
            diag_results: vec![],
            is_pinging: false,
            diag_rx: None,

            ping_history: history,
            recent_outcomes: VecDeque::with_capacity(20),
            current_ping: 15.0,
            packet_loss: 0.0,
            jitter: 0.5,
            ping_rx: None,
            boost_cancel: None,

            timer_res_enabled: false,
            timer_res_current_100ns: 10000,
            timer_res_min_100ns: 156250,
            timer_res_max_100ns: 5000,

            filterkeys_enabled: false,
            filterkeys_delay_ms: 150,
            filterkeys_repeat_ms: 25,
            filterkeys_test_input: String::new(),
            mouse_accel_enabled: false,

            mem_stats: query_memory_stats(),
            ram_clean_status: String::new(),
            last_metrics_poll: Instant::now(),

            detect_rx: None,
            adapters_rx: None,

            _tray: None,
            tray_show_rx: None,
            allow_exit: false,
        }
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        self.stop_boost_session();
    }
}

impl AppState {
    fn sync_system_metrics(&mut self) {
        let (min, max, cur) = query_timer_resolution();
        self.timer_res_min_100ns = min;
        self.timer_res_max_100ns = max;
        self.timer_res_current_100ns = cur;
        self.timer_res_enabled = cur <= 6000;

        let (fk_on, delay, repeat) = query_filterkeys();
        self.filterkeys_enabled = fk_on;
        if fk_on {
            self.filterkeys_delay_ms = delay;
            self.filterkeys_repeat_ms = repeat;
        }
        self.mouse_accel_enabled = is_mouse_accel_enabled();
        self.mem_stats = query_memory_stats();
        if let Some(p) = get_active_power_plan() {
            self.active_power_plan_name = p.name;
        }
    }

    fn enable_timer_resolution(&mut self) {
        enable_05ms_timer();
        let (min, max, cur) = query_timer_resolution();
        self.timer_res_min_100ns = min;
        self.timer_res_max_100ns = max;
        self.timer_res_current_100ns = cur;
        self.timer_res_enabled = cur <= 6000;
    }

    fn disable_timer_resolution(&mut self) {
        disable_05ms_timer(self.timer_res_min_100ns);
        let (min, max, cur) = query_timer_resolution();
        self.timer_res_min_100ns = min;
        self.timer_res_max_100ns = max;
        self.timer_res_current_100ns = cur;
        self.timer_res_enabled = cur <= 6000;
    }

    fn apply_fk(&mut self) {
        if apply_filterkeys(self.filterkeys_delay_ms, self.filterkeys_repeat_ms) {
            self.filterkeys_enabled = true;
        }
    }

    fn disable_fk(&mut self) {
        if disable_filterkeys() {
            self.filterkeys_enabled = false;
        }
    }

    fn trigger_ram_clean(&mut self) {
        let active_pid = self.detected_game.as_ref().map(|g| g.pid);
        let (freed_mb, standby_ok) = clean_ram_and_standby(self.is_admin, active_pid);
        self.mem_stats = query_memory_stats();
        self.last_ram_clean = Some(Instant::now());
        self.ram_clean_status = match self.language {
            Language::Ukrainian => {
                if standby_ok {
                    format!("✔ Очищено Standby + кеш фонових програм: звільнено {} MB RAM", freed_mb)
                } else {
                    format!("✔ Очищено кеш фонових програм: +{} MB (Для Standby запустіть як Admin)", freed_mb)
                }
            }
            Language::English => {
                if standby_ok {
                    format!("✔ Purged Standby List & trimmed bloat apps: freed {} MB RAM", freed_mb)
                } else {
                    format!("✔ Trimmed background bloat: +{} MB (Run as Admin to purge Standby)", freed_mb)
                }
            }
            Language::Russian => {
                if standby_ok {
                    format!("✔ Очищен Standby + кэш фоновых программ: освобождено {} MB RAM", freed_mb)
                } else {
                    format!("✔ Очищен кэш фоновых программ: +{} MB (Для Standby запустите от Admin)", freed_mb)
                }
            }
        };
    }

    fn start_boost_session(&mut self) {
        if self.is_boosted {
            return;
        }
        self.is_boosted = true;
        self.boost_start_time = Some(Instant::now());
        self.recent_outcomes.clear();
        self.packet_loss = 0.0;
        self.start_real_ping_thread();

        if self.auto_apply_timer_res {
            self.enable_timer_resolution();
        }
        if self.auto_apply_filterkeys {
            self.apply_fk();
        }
        if self.auto_apply_power_plan {
            if let Some(prev) = get_active_power_plan() {
                if !prev.name.to_lowercase().contains("ultimate")
                    && !prev.name.to_lowercase().contains("максимальн")
                {
                    self.saved_power_plan_guid = Some(prev.guid);
                }
            }
            let (ok, name) = activate_ultimate_or_high_perf();
            if ok {
                self.active_power_plan_name = name;
            }
        }
        if self.auto_clean_ram {
            self.trigger_ram_clean();
        }
        if self.auto_throttle_bg_procs {
            let active_pid = self.detected_game.as_ref().map(|g| g.pid);
            self.adjusted_bg_pids = balance_processes_for_gaming(active_pid);
        }
    }

    fn stop_boost_session(&mut self) {
        if !self.is_boosted && self.adjusted_bg_pids.is_empty() && self.saved_power_plan_guid.is_none() {
            return;
        }
        self.is_boosted = false;
        self.boost_start_time = None;
        if let Some(c) = &self.boost_cancel {
            c.store(false, Ordering::Relaxed);
        }
        if self.auto_apply_timer_res {
            self.disable_timer_resolution();
        }
        if !self.adjusted_bg_pids.is_empty() {
            restore_balanced_processes(&self.adjusted_bg_pids);
            self.adjusted_bg_pids.clear();
        }
        if let Some(prev_guid) = self.saved_power_plan_guid.take() {
            if activate_power_plan(&prev_guid) {
                if let Some(p) = get_active_power_plan() {
                    self.active_power_plan_name = p.name;
                }
            }
        }
    }

    fn refresh_adapters(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.adapters_rx = Some(rx);
        thread::spawn(move || {
            let list = query_network_adapters();
            let _ = tx.send(list);
        });
    }

    fn refresh_nodes_ping(&mut self) {
        let nodes: Vec<(usize, String)> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (i, n.host.to_string()))
            .collect();
        let (tx, rx) = mpsc::channel();
        self.node_ping_rx = Some(rx);
        let bind_ip = self.shared_adapter_ip.read().ok().and_then(|g| g.clone());

        thread::spawn(move || {
            for (idx, host) in nodes {
                let ping = native_icmp_ping(&host, bind_ip.as_deref(), 32, false, 800);
                let _ = tx.send((idx, ping));
            }
        });
    }

    fn run_dns_benchmark(&mut self) {
        self.is_benching_dns = true;
        let targets: Vec<(usize, &'static str)> = self
            .dns_presets
            .iter()
            .enumerate()
            .map(|(i, d)| (i, d.primary))
            .collect();
        let bind_ip = self.shared_adapter_ip.read().ok().and_then(|g| g.clone());
        let (tx, rx) = mpsc::channel();
        self.dns_bench_rx = Some(rx);

        thread::spawn(move || {
            for (idx, ip) in targets {
                let mut sum = 0.0f32;
                let mut cnt = 0;
                for _ in 0..3 {
                    if let Some(ms) = native_icmp_ping(ip, bind_ip.as_deref(), 32, false, 700) {
                        sum += ms;
                        cnt += 1;
                    }
                    thread::sleep(Duration::from_millis(40));
                }
                let avg = if cnt > 0 { Some(sum / (cnt as f32)) } else { None };
                let _ = tx.send((idx, avg));
            }
        });
    }

    fn run_mtu_discovery(&mut self) {
        self.is_probing_mtu = true;
        let target = self.nodes[self.selected_node].host.to_string();
        let bind_ip = self.shared_adapter_ip.read().ok().and_then(|g| g.clone());
        let lang = self.language;
        let (tx, rx) = mpsc::channel();
        self.mtu_rx = Some(rx);

        thread::spawn(move || {
            let mtu = discover_optimal_mtu("1.1.1.1", bind_ip.as_deref())
                .or_else(|| discover_optimal_mtu(&target, bind_ip.as_deref()));

            let msg = match mtu {
                Some(m) => match lang {
                    Language::Ukrainian => format!("✔ Знайдено оптимальний Path MTU без фрагментації: {} байт", m),
                    Language::English => format!("✔ Optimal non-fragmenting Path MTU found: {} bytes", m),
                    Language::Russian => format!("✔ Найден оптимальный Path MTU без фрагментации: {} байт", m),
                },
                None => match lang {
                    Language::Ukrainian => "⚠ Не вдалося визначити MTU (ICMP заблоковано)".into(),
                    Language::English => "⚠ Failed to probe Path MTU (ICMP blocked)".into(),
                    Language::Russian => "⚠ Не удалось определить MTU (ICMP заблокирован)".into(),
                },
            };
            let _ = tx.send((mtu, msg));
        });
    }

    fn start_real_ping_thread(&mut self) {
        if let Some(old_c) = &self.boost_cancel {
            old_c.store(false, Ordering::Relaxed);
        }

        let (tx, rx) = mpsc::channel();
        self.ping_rx = Some(rx);
        let target_host = self.nodes[self.selected_node].host.to_string();
        let adapter_ip_arc = self.shared_adapter_ip.clone();

        let cancel = Arc::new(AtomicBool::new(true));
        self.boost_cancel = Some(cancel.clone());

        thread::spawn(move || {
            let prober = IcmpProber::new();
            let dest_ip = resolve_ipv4(&target_host);

            while cancel.load(Ordering::Relaxed) {
                let bind_ip_str = adapter_ip_arc.read().ok().and_then(|g| g.clone());
                let src_ip = bind_ip_str.and_then(|s| s.trim().parse().ok());

                let rtt = match (&prober, dest_ip) {
                    (Some(p), Some(dst)) => p.ping_ipv4(dst, src_ip, 32, false, 850),
                    _ => native_icmp_ping(&target_host, None, 32, false, 850),
                };

                let _ = tx.send(PingTelemetrySample { rtt_ms: rtt });
                thread::sleep(Duration::from_millis(850));
            }
        });
    }

    fn run_diag_test(&mut self, burst_mode: bool) {
        self.is_pinging = true;
        self.diag_results.clear();
        let host = self.diag_host.trim().to_string();
        let bind_ip = self.shared_adapter_ip.read().ok().and_then(|g| g.clone());
        let lang = self.language;

        let count = if burst_mode { 30 } else { 10 };
        let interval_ms = if burst_mode { 50 } else { 150 };

        let start_msg = match lang {
            Language::Ukrainian => format!(
                "▶ {} ({} пакетів, інтервал {}ms) до '{}'...",
                if burst_mode { "Тест Bufferbloat / Стрибків" } else { "Нативний ICMP тест" },
                count,
                interval_ms,
                host
            ),
            Language::English => format!(
                "▶ {} ({} packets, {}ms interval) to '{}'...",
                if burst_mode { "Bufferbloat / Jitter Burst Test" } else { "Native ICMP Diagnostics" },
                count,
                interval_ms,
                host
            ),
            Language::Russian => format!(
                "▶ {} ({} пакетов, интервал {}ms) к '{}'...",
                if burst_mode { "Тест Bufferbloat / Скачков" } else { "Нативный ICMP тест" },
                count,
                interval_ms,
                host
            ),
        };
        self.diag_results.push(start_msg);

        let (tx, rx) = mpsc::channel();
        self.diag_rx = Some(rx);

        thread::spawn(move || {
            let resolved = resolve_ipv4(&host);
            let Some(dst_ip) = resolved else {
                let err = match lang {
                    Language::Ukrainian => format!("✖ Не вдалося розпізнати адресу '{}'", host),
                    Language::English => format!("✖ Failed to resolve host '{}'", host),
                    Language::Russian => format!("✖ Не удалось распознать адрес '{}'", host),
                };
                let _ = tx.send((err, true));
                return;
            };

            let src_ip = bind_ip.and_then(|s| s.trim().parse().ok());
            let Some(prober) = IcmpProber::new() else {
                let _ = tx.send(("✖ Failed to open Windows IcmpHandle.".into(), true));
                return;
            };

            let _ = tx.send((format!("Resolved IPv4: {}", dst_ip), false));

            let mut rtts: Vec<f32> = Vec::with_capacity(count);
            let mut lost = 0usize;

            for i in 1..=count {
                if let Some(ms) = prober.ping_ipv4(dst_ip, src_ip, 32, false, 900) {
                    rtts.push(ms);
                    if !burst_mode || i % 3 == 0 || ms > 60.0 {
                        let _ = tx.send((format!("[{:02}/{}] Reply from {}: time={:.2} ms", i, count, dst_ip, ms), false));
                    }
                } else {
                    lost += 1;
                    let _ = tx.send((format!("[{:02}/{}] Request timed out (PACKET LOSS!)", i, count), false));
                }
                thread::sleep(Duration::from_millis(interval_ms));
            }

            if !rtts.is_empty() {
                let min = rtts.iter().cloned().fold(f32::INFINITY, f32::min);
                let max = rtts.iter().cloned().fold(0.0f32, f32::max);
                let avg = rtts.iter().sum::<f32>() / (rtts.len() as f32);
                let jitter = if rtts.len() > 1 {
                    let sum_diff: f32 = rtts.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
                    sum_diff / ((rtts.len() - 1) as f32)
                } else {
                    0.0
                };
                let loss_pct = (lost as f32 / count as f32) * 100.0;

                let _ = tx.send(("─────────────────────────────────────────────".into(), false));
                let summary = match lang {
                    Language::Ukrainian => format!(
                        "✔ Min: {:.1}ms | Avg: {:.1}ms | Max: {:.1}ms | Jitter: {:.2}ms | Loss: {:.1}%",
                        min, avg, max, jitter, loss_pct
                    ),
                    Language::English => format!(
                        "✔ Min: {:.1}ms | Avg: {:.1}ms | Max: {:.1}ms | Jitter: {:.2}ms | Loss: {:.1}%",
                        min, avg, max, jitter, loss_pct
                    ),
                    Language::Russian => format!(
                        "✔ Min: {:.1}ms | Avg: {:.1}ms | Max: {:.1}ms | Jitter: {:.2}ms | Loss: {:.1}%",
                        min, avg, max, jitter, loss_pct
                    ),
                };
                let _ = tx.send((summary, true));
            } else {
                let _ = tx.send(("✖ 100% Packet Loss — target unreachable or blocks ICMP.".into(), true));
            }
        });
    }
}

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

        // Poll timer & RAM periodically
        if self.last_metrics_poll.elapsed() >= Duration::from_secs(1) {
            self.last_metrics_poll = Instant::now();
            self.sync_system_metrics();
        }

        // Auto Standby RAM clean every 3 minutes while boosted if enabled
        if self.is_boosted && self.auto_clean_ram {
            let should_clean = self
                .last_ram_clean
                .map_or(false, |t| t.elapsed() >= Duration::from_secs(180));
            if should_clean && self.mem_stats.load_percent >= 70 {
                self.trigger_ram_clean();
            }
        }

        // Intercept close button [X] -> minimize to tray
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
            if first_load || self.selected_adapter_idx >= self.adapters.len() {
                self.selected_adapter_idx = 0;
                if let Some(a) = self.adapters.get(0) {
                    if let Ok(mut lock) = self.shared_adapter_ip.write() {
                        *lock = a.ip.clone();
                    }
                }
                if first_load {
                    self.refresh_nodes_ping();
                }
            }
        }

        // Poll 10-second Node Ping updates
        if let Some(rx) = &self.node_ping_rx {
            let mut updated_any = false;
            while let Ok((idx, val)) = rx.try_recv() {
                if idx < self.nodes.len() {
                    self.nodes[idx].ping_ms = val;
                    updated_any = true;
                }
            }
            if updated_any {
                self.last_node_update = Instant::now();
                if self.auto_select_best_node {
                    if let Some((best_idx, _)) = self
                        .nodes
                        .iter()
                        .enumerate()
                        .filter_map(|(i, n)| n.ping_ms.map(|p| (i, p)))
                        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    {
                        self.selected_node = best_idx;
                    }
                }
            }
        }

        // Poll DNS Benchmark
        if let Some(rx) = &self.dns_bench_rx {
            let mut count_done = 0;
            while let Ok((idx, val)) = rx.try_recv() {
                if idx < self.dns_presets.len() {
                    self.dns_presets[idx].ping_ms = val;
                }
                count_done += 1;
            }
            if count_done > 0 && self.dns_presets.iter().all(|d| d.ping_ms.is_some()) {
                self.is_benching_dns = false;
            }
            if self.is_benching_dns && count_done > 0 {
                // fallback clear if final packet timed out
                if self.dns_bench_rx.is_some() {
                    self.is_benching_dns = false;
                }
            }
        }

        // Poll MTU discovery
        if let Some(rx) = &self.mtu_rx {
            while let Ok((mtu, msg)) = rx.try_recv() {
                self.mtu_result = mtu;
                self.mtu_status_msg = msg;
                self.is_probing_mtu = false;
            }
        }

        // Poll Tweaks Async Runner
        if let Some(rx) = &self.tweaks_rx {
            while let Ok(lines) = rx.try_recv() {
                for l in lines {
                    self.tweak_status_log.push(l);
                }
                self.tweaks_busy = false;
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
                    if let Some(idx) = self
                        .supported_games
                        .iter()
                        .position(|g| g.exe.eq_ignore_ascii_case(&game.exe))
                    {
                        self.selected_game = idx;
                        if !self.auto_select_best_node {
                            self.selected_node = self.supported_games[idx].server_idx;
                        }
                    }
                    self.detected_game = Some(game);

                    if was_none && self.auto_boost_on_detect && !self.is_boosted {
                        self.start_boost_session();
                    }
                }
                None => {
                    if self.detected_game.is_some() {
                        self.detected_game = None;
                    }
                }
            }
        }

        // Poll real telemetry channel (exact sub-ms ping, real jitter, real loss)
        if let Some(rx) = &self.ping_rx {
            while let Ok(sample) = rx.try_recv() {
                if self.recent_outcomes.len() >= 20 {
                    self.recent_outcomes.pop_front();
                }
                match sample.rtt_ms {
                    Some(ms) => {
                        self.recent_outcomes.push_back(true);
                        let prev = self.current_ping;
                        self.current_ping = ms;
                        let delta = (ms - prev).abs();
                        self.jitter = self.jitter + (delta - self.jitter) / 8.0;
                        if self.ping_history.len() >= 40 {
                            self.ping_history.pop_front();
                        }
                        self.ping_history.push_back(ms);
                    }
                    None => {
                        self.recent_outcomes.push_back(false);
                    }
                }
                if !self.recent_outcomes.is_empty() {
                    let lost = self.recent_outcomes.iter().filter(|&&ok| !ok).count();
                    self.packet_loss = (lost as f32 / self.recent_outcomes.len() as f32) * 100.0;
                }
            }
        }

        // Poll diag channel
        if let Some(rx) = &self.diag_rx {
            while let Ok((msg, done)) = rx.try_recv() {
                self.diag_results.push(msg);
                if done {
                    self.is_pinging = false;
                }
            }
        }

        ctx.request_repaint_after(Duration::from_millis(300));

        // Dark Modern Gaming Theme
        let mut style = (*ctx.style()).clone();
        style.visuals.dark_mode = true;
        style.visuals.override_text_color = Some(egui::Color32::from_rgb(224, 232, 244));
        style.visuals.window_fill = egui::Color32::from_rgb(13, 16, 22);
        style.visuals.panel_fill = egui::Color32::from_rgb(17, 21, 28);
        ctx.set_style(style);

        let lang = self.language;

        // Top Header
        egui::TopBottomPanel::top("top_header").show(ctx, |ui| {
            ui.add_space(7.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("⚡ WPING")
                        .font(egui::FontId::proportional(22.0))
                        .color(egui::Color32::from_rgb(0, 235, 160))
                        .strong(),
                );
                ui.label(
                    egui::RichText::new("v2.0")
                        .font(egui::FontId::proportional(12.0))
                        .color(egui::Color32::from_rgb(0, 200, 255))
                        .strong(),
                );
                ui.label(
                    egui::RichText::new("PING, FPS & INPUT OPTIMIZER")
                        .font(egui::FontId::proportional(11.0))
                        .color(egui::Color32::from_rgb(120, 140, 165)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let close_label = match lang {
                        Language::Ukrainian => "❌ Вийти",
                        Language::English => "❌ Exit",
                        Language::Russian => "❌ Выйти",
                    };
                    if ui.button(close_label).clicked() {
                        self.stop_boost_session();
                        self.allow_exit = true;
                        std::process::exit(0);
                    }

                    ui.add_space(4.0);

                    // Language Selector
                    egui::ComboBox::from_id_salt("lang_selector_combo")
                        .selected_text(self.language.label())
                        .width(72.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.language, Language::Ukrainian, Language::Ukrainian.label());
                            ui.selectable_value(&mut self.language, Language::English, Language::English.label());
                            ui.selectable_value(&mut self.language, Language::Russian, Language::Russian.label());
                        });

                    ui.add_space(4.0);

                    // Admin Badge / 1-click elevation
                    if self.is_admin {
                        ui.label(
                            egui::RichText::new("🛡️ ADMIN")
                                .font(egui::FontId::proportional(11.0))
                                .color(egui::Color32::from_rgb(0, 255, 140))
                                .strong(),
                        );
                    } else {
                        let elev_txt = match lang {
                            Language::Ukrainian => "⚠️ Адмін ↻",
                            Language::English => "⚠️ Admin ↻",
                            Language::Russian => "⚠️ Админ ↻",
                        };
                        let btn = egui::Button::new(
                            egui::RichText::new(elev_txt)
                                .font(egui::FontId::proportional(11.0))
                                .color(egui::Color32::BLACK)
                                .strong(),
                        )
                        .fill(egui::Color32::from_rgb(255, 190, 60));
                        if ui.add(btn).clicked() {
                            if restart_as_admin() {
                                self.stop_boost_session();
                                self.allow_exit = true;
                                std::process::exit(0);
                            }
                        }
                    }

                    ui.add_space(6.0);

                    if self.is_boosted {
                        let active_label = match lang {
                            Language::Ukrainian => "● БУСТ АКТИВНИЙ",
                            Language::English => "● BOOST ACTIVE",
                            Language::Russian => "● БУСТ АКТИВЕН",
                        };
                        ui.label(
                            egui::RichText::new(active_label)
                                .color(egui::Color32::from_rgb(0, 255, 128))
                                .strong(),
                        );
                    } else {
                        let std_label = match lang {
                            Language::Ukrainian => "○ СТАНДАРТ",
                            Language::English => "○ STANDBY",
                            Language::Russian => "○ СТАНДАРТ",
                        };
                        ui.label(
                            egui::RichText::new(std_label)
                                .color(egui::Color32::from_rgb(135, 145, 160)),
                        );
                    }
                });
            });

            ui.add_space(5.0);
            ui.separator();
            ui.add_space(5.0);

            // Navigation Tabs
            ui.horizontal(|ui| {
                let tab_titles = match lang {
                    Language::Ukrainian => [
                        (AppTab::Dashboard, "🎮 Оптимізатор"),
                        (AppTab::InputFps, "⚡ Інпут, 0.5ms & FPS"),
                        (AppTab::NetworkDns, "🌐 Мережа, DNS & MTU"),
                        (AppTab::LocalTweaks, "⚙️ Твіки Windows"),
                        (AppTab::Diagnostics, "📊 Діагностика"),
                    ],
                    Language::English => [
                        (AppTab::Dashboard, "🎮 Game Optimizer"),
                        (AppTab::InputFps, "⚡ Input, 0.5ms & FPS"),
                        (AppTab::NetworkDns, "🌐 Network, DNS & MTU"),
                        (AppTab::LocalTweaks, "⚙️ Windows Tweaks"),
                        (AppTab::Diagnostics, "📊 Diagnostics"),
                    ],
                    Language::Russian => [
                        (AppTab::Dashboard, "🎮 Оптимизатор"),
                        (AppTab::InputFps, "⚡ Инпут, 0.5ms & FPS"),
                        (AppTab::NetworkDns, "🌐 Сеть, DNS & MTU"),
                        (AppTab::LocalTweaks, "⚙️ Твики Windows"),
                        (AppTab::Diagnostics, "📊 Диагностика"),
                    ],
                };

                for (tab, title) in tab_titles {
                    let is_selected = self.current_tab == tab;
                    let btn = egui::Button::new(
                        egui::RichText::new(title)
                            .font(egui::FontId::proportional(13.5))
                            .color(if is_selected {
                                egui::Color32::from_rgb(0, 235, 160)
                            } else {
                                egui::Color32::from_rgb(180, 195, 212)
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
            ui.add_space(5.0);
        });

        // Bottom Bar
        egui::TopBottomPanel::bottom("bottom_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let adapter_summary = if let Some(a) = self.adapters.get(self.selected_adapter_idx) {
                    format!("{} ({})", a.description, a.ip.as_deref().unwrap_or("No IP"))
                } else {
                    match lang {
                        Language::Ukrainian => "Пошук адаптера...".into(),
                        Language::English => "Detecting adapter...".into(),
                        Language::Russian => "Поиск адаптера...".into(),
                    }
                };

                let timer_ms = self.timer_res_current_100ns as f32 / 10000.0;
                let bot_label = format!("WPing v2.0 • {} • Timer: {:.3}ms", adapter_summary, timer_ms);
                ui.label(
                    egui::RichText::new(bot_label)
                        .font(egui::FontId::proportional(11.0))
                        .color(egui::Color32::from_rgb(120, 138, 158)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "RAM: {}% Free: {:.1}GB | Jitter: {:.1}ms | Loss: {:.0}%",
                            self.mem_stats.load_percent,
                            self.mem_stats.avail_mb as f32 / 1024.0,
                            self.jitter,
                            self.packet_loss
                        ))
                        .font(egui::FontId::proportional(11.0))
                        .color(egui::Color32::from_rgb(0, 215, 145)),
                    );
                });
            });
            ui.add_space(4.0);
        });

        // Main Central Panel
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(6.0);
            match self.current_tab {
                AppTab::Dashboard => self.render_dashboard(ui),
                AppTab::InputFps => self.render_input_fps(ui),
                AppTab::NetworkDns => self.render_network_dns(ui),
                AppTab::LocalTweaks => self.render_local_tweaks(ui),
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
                        Language::Ukrainian => "Пошук інтерфейсів...",
                        Language::English => "Scanning interfaces...",
                        Language::Russian => "Поиск интерфейсов...",
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
                        .width(440.0)
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
                        Language::Ukrainian => "⚠️ УВАГА: Обрано віртуальний VPN! Для ігор та мінімального пінгу оберіть фізичну карту (Ethernet / Wi-Fi).",
                        Language::English => "⚠️ WARNING: Virtual VPN adapter selected! Select your physical Ethernet/Wi-Fi adapter for lowest latency.",
                        Language::Russian => "⚠️ ВНИМАНИЕ: Выбран виртуальный VPN! Для игр и минимального пинга выберите физическую карту (Ethernet / Wi-Fi).",
                    };
                    ui.label(egui::RichText::new(warn_msg).font(egui::FontId::proportional(11.0)).color(egui::Color32::from_rgb(255, 170, 0)).strong());
                } else {
                    let ok_msg = match lang {
                        Language::Ukrainian => "✔ Прямий фізичний канал активний. Усі виміри ICMP прив'язані до цієї карти.",
                        Language::English => "✔ Direct physical channel active. All ICMP probes bind directly to this NIC.",
                        Language::Russian => "✔ Прямой физический канал активен. Все замеры ICMP привязаны к этой карте.",
                    };
                    ui.label(egui::RichText::new(ok_msg).font(egui::FontId::proportional(11.0)).color(egui::Color32::from_rgb(0, 230, 153)));
                }
            }
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
                    ui.label(egui::RichText::new(active_tag).font(egui::FontId::proportional(13.5)).color(egui::Color32::from_rgb(0, 255, 128)).strong());
                    ui.label(egui::RichText::new(format!("{} (PID: {})", game.name, game.pid)).font(egui::FontId::proportional(13.5)).color(egui::Color32::WHITE).strong());
                } else {
                    let detect_tag = match lang {
                        Language::Ukrainian => "🔍 АВТО-ДЕТЕКЦІЯ:",
                        Language::English => "🔍 AUTO-DETECTION:",
                        Language::Russian => "🔍 АВТО-ДЕТЕКЦИЯ:",
                    };
                    let waiting_msg = match lang {
                        Language::Ukrainian => "Очікування запуску гри (CS2, Fortnite, Valorant...)",
                        Language::English => "Waiting for game launch (CS2, Fortnite, Valorant...)",
                        Language::Russian => "Ожидание запуска игры (CS2, Fortnite, Valorant...)",
                    };
                    ui.label(egui::RichText::new(detect_tag).font(egui::FontId::proportional(13.0)).color(egui::Color32::from_rgb(0, 210, 255)).strong());
                    ui.label(egui::RichText::new(waiting_msg).font(egui::FontId::proportional(13.0)).color(egui::Color32::from_rgb(160, 175, 195)));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let auto_boost_lbl = match lang {
                        Language::Ukrainian => "Авто-буст при старті гри",
                        Language::English => "Auto-boost on launch",
                        Language::Russian => "Авто-буст при запуске игры",
                    };
                    ui.checkbox(&mut self.auto_boost_on_detect, egui::RichText::new(auto_boost_lbl).color(egui::Color32::from_rgb(0, 230, 153)));
                });
            });
        });

        ui.add_space(6.0);
        self.render_adapter_selector(ui);
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            // Left Column
            ui.vertical(|ui| {
                ui.set_width(455.0);
                ui.group(|ui| {
                    let profile_heading = match lang {
                        Language::Ukrainian => "Профіль гри та комплексний буст",
                        Language::English => "Game Profile & All-in-One Boost",
                        Language::Russian => "Профиль игры и комплексный буст",
                    };
                    ui.heading(profile_heading);
                    ui.add_space(6.0);

                    let current_game_label = format!(
                        "{} ({})",
                        self.supported_games[self.selected_game].name,
                        self.supported_games[self.selected_game].exe
                    );

                    egui::ComboBox::from_id_salt("game_profile_combo")
                        .width(390.0)
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

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(8.0);

                    let node_location = self.nodes[self.selected_node].location(lang);
                    let node_name = self.nodes[self.selected_node].name;
                    let node_ping_str = match self.nodes[self.selected_node].ping_ms {
                        Some(p) => format!(" • {:.1} ms", p),
                        None => " • ...".into(),
                    };
                    ui.label(egui::RichText::new(format!("📍 {} ({}){}", node_location, node_name, node_ping_str)).strong());

                    ui.add_space(8.0);

                    // Live System Stack Status Pills
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(20, 26, 35))
                        .rounding(6.0)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            let timer_ok = self.timer_res_current_100ns <= 6000;
                            let fk_ok = self.filterkeys_enabled;
                            let power_ok = self.active_power_plan_name.to_lowercase().contains("ultimate")
                                || self.active_power_plan_name.to_lowercase().contains("висок")
                                || self.active_power_plan_name.to_lowercase().contains("высок")
                                || self.active_power_plan_name.to_lowercase().contains("максимальн");

                            ui.horizontal(|ui| {
                                let badge = |ui: &mut egui::Ui, label: &str, ok: bool| {
                                    let col = if ok { egui::Color32::from_rgb(0, 240, 140) } else { egui::Color32::GRAY };
                                    ui.label(egui::RichText::new(format!("{} {}", if ok { "●" } else { "○" }, label)).font(egui::FontId::proportional(11.5)).color(col).strong());
                                };
                                badge(ui, "0.5ms Timer", timer_ok);
                                ui.add_space(8.0);
                                badge(ui, "FilterKeys", fk_ok);
                                ui.add_space(8.0);
                                badge(ui, &format!("Power: {}", self.active_power_plan_name), power_ok);
                            });
                        });

                    ui.add_space(12.0);

                    if !self.is_boosted {
                        let boost_btn_text = match lang {
                            Language::Ukrainian => "🚀 УВІМКНУТИ ПОВНИЙ БУСТ (PING + FPS)",
                            Language::English => "🚀 ENABLE FULL BOOST (PING + FPS)",
                            Language::Russian => "🚀 ВКЛЮЧИТЬ ПОЛНЫЙ БУСТ (PING + FPS)",
                        };
                        let boost_btn = egui::Button::new(
                            egui::RichText::new(boost_btn_text)
                                .font(egui::FontId::proportional(15.0))
                                .color(egui::Color32::BLACK)
                                .strong(),
                        )
                        .fill(egui::Color32::from_rgb(0, 235, 160))
                        .min_size(egui::vec2(420.0, 50.0));

                        if ui.add(boost_btn).clicked() {
                            self.start_boost_session();
                        }
                    } else {
                        let stop_btn_text = match lang {
                            Language::Ukrainian => "⏹ ЗУПИНИТИ БУСТ ТА ПОВЕРНУТИ РЕЖИМ",
                            Language::English => "⏹ STOP BOOST & RESTORE SESSION",
                            Language::Russian => "⏹ ОСТАНОВИТЬ БУСТ И ВЕРНУТЬ РЕЖИМ",
                        };
                        let stop_btn = egui::Button::new(
                            egui::RichText::new(stop_btn_text)
                                .font(egui::FontId::proportional(15.0))
                                .color(egui::Color32::WHITE)
                                .strong(),
                        )
                        .fill(egui::Color32::from_rgb(215, 55, 65))
                        .min_size(egui::vec2(420.0, 50.0));

                        if ui.add(stop_btn).clicked() {
                            self.stop_boost_session();
                        }
                    }

                    ui.add_space(8.0);
                    if self.is_boosted {
                        let elapsed = self.boost_start_time.map_or(0, |t| t.elapsed().as_secs());
                        let active_str = match lang {
                            Language::Ukrainian => format!(
                                "✔ Активно: {:02}:{:02} | Оптимізовано для {} | RAM: {:.1}GB вільно",
                                elapsed / 60, elapsed % 60,
                                self.supported_games[self.selected_game].name,
                                self.mem_stats.avail_mb as f32 / 1024.0
                            ),
                            Language::English => format!(
                                "✔ Active: {:02}:{:02} | Optimized for {} | RAM Free: {:.1}GB",
                                elapsed / 60, elapsed % 60,
                                self.supported_games[self.selected_game].name,
                                self.mem_stats.avail_mb as f32 / 1024.0
                            ),
                            Language::Russian => format!(
                                "✔ Активно: {:02}:{:02} | Оптимизировано для {} | RAM: {:.1}GB свободно",
                                elapsed / 60, elapsed % 60,
                                self.supported_games[self.selected_game].name,
                                self.mem_stats.avail_mb as f32 / 1024.0
                            ),
                        };
                        ui.label(egui::RichText::new(active_str).font(egui::FontId::proportional(11.5)).color(egui::Color32::from_rgb(0, 230, 153)));
                    } else {
                        let desc = match lang {
                            Language::Ukrainian => "Активує 0.5ms таймер, 0 Delay клавіатуру, Ultimate Power, очищення RAM та нативну телеметрію.",
                            Language::English => "Activates 0.5ms kernel timer, zero-delay keyboard, Ultimate Power, RAM cleaner, and native telemetry.",
                            Language::Russian => "Активирует 0.5ms таймер, 0 Delay клавиатуру, Ultimate Power, очистку RAM и нативную телеметрию.",
                        };
                        ui.label(egui::RichText::new(desc).font(egui::FontId::proportional(11.5)).color(egui::Color32::GRAY));
                    }
                });
            });

            ui.add_space(10.0);

            // Right Column: Real-Time Telemetry
            ui.vertical(|ui| {
                ui.group(|ui| {
                    ui.set_width(395.0);
                    let telem_heading = match lang {
                        Language::Ukrainian => "Нативна телеметрія каналу",
                        Language::English => "Native Kernel Telemetry",
                        Language::Russian => "Нативная телеметрия канала",
                    };
                    ui.heading(telem_heading);
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            let ping_lbl = match lang {
                                Language::Ukrainian => "РЕАЛЬНИЙ ПІНГ",
                                Language::English => "REAL PING",
                                Language::Russian => "РЕАЛЬНЫЙ ПИНГ",
                            };
                            ui.label(egui::RichText::new(ping_lbl).font(egui::FontId::proportional(11.5)));
                            let ping_color = if self.current_ping < 20.0 {
                                egui::Color32::from_rgb(0, 255, 128)
                            } else if self.current_ping < 40.0 {
                                egui::Color32::from_rgb(0, 210, 255)
                            } else {
                                egui::Color32::from_rgb(255, 170, 0)
                            };
                            ui.label(
                                egui::RichText::new(format!("{:.1} ms", self.current_ping))
                                    .font(egui::FontId::proportional(34.0))
                                    .color(ping_color)
                                    .strong(),
                            );
                        });

                        ui.add_space(24.0);

                        ui.vertical(|ui| {
                            let loss_lbl = match lang {
                                Language::Ukrainian => "ВТРАТА ПАКЕТІВ",
                                Language::English => "PACKET LOSS",
                                Language::Russian => "ПОТЕРЯ ПАКЕТОВ",
                            };
                            ui.label(egui::RichText::new(loss_lbl).font(egui::FontId::proportional(11.5)));
                            let loss_col = if self.packet_loss == 0.0 {
                                egui::Color32::from_rgb(0, 235, 155)
                            } else {
                                egui::Color32::from_rgb(255, 90, 90)
                            };
                            ui.label(egui::RichText::new(format!("{:.1}%", self.packet_loss)).font(egui::FontId::proportional(20.0)).color(loss_col).strong());
                            ui.add_space(4.0);
                            let jit_lbl = match lang {
                                Language::Ukrainian => "JITTER (ВАРІАЦІЯ)",
                                Language::English => "REAL JITTER",
                                Language::Russian => "JITTER (ВАРИАЦИЯ)",
                            };
                            ui.label(egui::RichText::new(jit_lbl).font(egui::FontId::proportional(11.5)));
                            ui.label(egui::RichText::new(format!("{:.2} ms", self.jitter)).font(egui::FontId::proportional(18.0)).color(egui::Color32::from_rgb(0, 200, 240)).strong());
                        });
                    });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(6.0);

                    let (min_p, max_p) = self.ping_history.iter().fold((f32::INFINITY, 0.0f32), |(mn, mx), &v| (mn.min(v), mx.max(v)));
                    let avg_p = if !self.ping_history.is_empty() {
                        self.ping_history.iter().sum::<f32>() / (self.ping_history.len() as f32)
                    } else {
                        0.0
                    };

                    let graph_lbl = match lang {
                        Language::Ukrainian => format!("Графік стабільності (Min: {:.1} | Avg: {:.1} | Max: {:.1} ms):", min_p, avg_p, max_p),
                        Language::English => format!("Latency stability (Min: {:.1} | Avg: {:.1} | Max: {:.1} ms):", min_p, avg_p, max_p),
                        Language::Russian => format!("График стабильности (Min: {:.1} | Avg: {:.1} | Max: {:.1} ms):", min_p, avg_p, max_p),
                    };
                    ui.label(egui::RichText::new(graph_lbl).font(egui::FontId::proportional(11.5)).strong());
                    ui.add_space(6.0);

                    let (response, painter) = ui.allocate_painter(egui::vec2(375.0, 95.0), egui::Sense::hover());
                    let rect = response.rect;
                    painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(10, 13, 18));

                    let count = self.ping_history.len();
                    if count > 1 {
                        let scale_max = (max_p * 1.2).max(50.0);
                        let bar_width = rect.width() / (count as f32);
                        for (i, &p) in self.ping_history.iter().enumerate() {
                            let norm = (p / scale_max).clamp(0.06, 1.0);
                            let bar_height = rect.height() * norm;
                            let bar_rect = egui::Rect::from_min_size(
                                egui::pos2(rect.left() + (i as f32) * bar_width + 1.0, rect.bottom() - bar_height),
                                egui::vec2((bar_width - 2.0).max(2.0), bar_height),
                            );

                            let col = if p < 20.0 {
                                egui::Color32::from_rgb(0, 255, 128)
                            } else if p < 40.0 {
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

    fn render_input_fps(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;

        let (heading, sub) = match lang {
            Language::Ukrainian => (
                "⚡ Інпут-лаг, 0.5ms Таймер ядра та Підвищення FPS",
                "Мінімізація затримки ядра Windows, нульова затримка клавіатури (FilterKeys), 1:1 миша, очищення Standby RAM і живлення Ultimate Performance.",
            ),
            Language::English => (
                "⚡ Input Delay, 0.5ms Kernel Timer & FPS Booster",
                "Kernel 0.500ms precision timer, FilterKeys zero keyboard delay, 1:1 Raw Mouse, Standby RAM purge & Ultimate Performance power switching.",
            ),
            Language::Russian => (
                "⚡ Инпут-лаг, 0.5ms Таймер ядра и Повышение FPS",
                "Минимизация задержки ядра Windows, нулевая задержка клавиатуры (FilterKeys), 1:1 мышь, очистка Standby RAM и питание Ultimate Performance.",
            ),
        };

        ui.heading(heading);
        ui.label(egui::RichText::new(sub).font(egui::FontId::proportional(11.5)).color(egui::Color32::from_rgb(0, 230, 153)));
        ui.add_space(6.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal(|ui| {
                // LEFT CARD: SetTimerResolution (0.5ms)
                ui.vertical(|ui| {
                    ui.set_width(430.0);
                    ui.group(|ui| {
                        let timer_active = self.timer_res_current_100ns <= 6000;
                        let (card_title, status_text) = match lang {
                            Language::Ukrainian => (
                                "⏱️ Таймер ядра Windows (SetTimerResolution 0.5ms)",
                                if timer_active { "● 0.500 ms АКТИВНО (2000 Hz)" } else { "○ СТАНДАРТНИЙ ТАЙМЕР (64 Hz)" },
                            ),
                            Language::English => (
                                "⏱️ Windows Kernel Timer (SetTimerResolution 0.5ms)",
                                if timer_active { "● 0.500 ms ACTIVE (2000 Hz)" } else { "○ DEFAULT TIMER (64 Hz)" },
                            ),
                            Language::Russian => (
                                "⏱️ Таймер ядра Windows (SetTimerResolution 0.5ms)",
                                if timer_active { "● 0.500 ms АКТИВНО (2000 Hz)" } else { "○ СТАНДАРТНЫЙ ТАЙМЕР (64 Hz)" },
                            ),
                        };

                        ui.label(egui::RichText::new(card_title).font(egui::FontId::proportional(14.5)).strong());
                        ui.label(egui::RichText::new(status_text).font(egui::FontId::proportional(12.0)).color(if timer_active { egui::Color32::from_rgb(0, 255, 128) } else { egui::Color32::from_rgb(255, 170, 50) }).strong());
                        ui.add_space(6.0);

                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(20, 26, 35))
                            .rounding(6.0)
                            .inner_margin(8.0)
                            .show(ui, |ui| {
                                let (lbl_cur, lbl_max) = match lang {
                                    Language::Ukrainian => ("Поточний тік ядра:", "Максимум (Windows 0.5ms):"),
                                    Language::English => ("Current kernel tick:", "Max precision (0.5ms):"),
                                    Language::Russian => ("Текущий тик ядра:", "Максимум (Windows 0.5ms):"),
                                };
                                ui.horizontal(|ui| {
                                    ui.label(lbl_cur);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(egui::RichText::new(format!("{:.3} ms", self.timer_res_current_100ns as f32 / 10000.0)).color(if timer_active { egui::Color32::from_rgb(0, 255, 150) } else { egui::Color32::YELLOW }).strong());
                                    });
                                });
                                ui.horizontal(|ui| {
                                    ui.label(lbl_max);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(egui::RichText::new(format!("{:.3} ms", self.timer_res_max_100ns as f32 / 10000.0)).color(egui::Color32::from_rgb(100, 200, 255)));
                                    });
                                });
                            });

                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let (en_t, dis_t) = match lang {
                                Language::Ukrainian => ("🚀 УВІМКНУТИ 0.5 ms", "⏸ СТАНДАРТНИЙ"),
                                Language::English => ("🚀 ENABLE 0.5 ms", "⏸ DEFAULT"),
                                Language::Russian => ("🚀 ВКЛЮЧИТЬ 0.5 ms", "⏸ СТАНДАРТНЫЙ"),
                            };
                            if ui.add(egui::Button::new(egui::RichText::new(en_t).color(egui::Color32::BLACK).strong()).fill(egui::Color32::from_rgb(0, 230, 153))).clicked() {
                                self.enable_timer_resolution();
                            }
                            if ui.add(egui::Button::new(egui::RichText::new(dis_t).color(egui::Color32::WHITE)).fill(egui::Color32::from_rgb(50, 60, 75))).clicked() {
                                self.disable_timer_resolution();
                            }
                        });

                        ui.add_space(6.0);
                        let auto_timer_lbl = match lang {
                            Language::Ukrainian => "Авто-активація 0.5ms при старті бусту",
                            Language::English => "Auto-enable 0.5ms on boost start",
                            Language::Russian => "Авто-активация 0.5ms при старте буста",
                        };
                        ui.checkbox(&mut self.auto_apply_timer_res, auto_timer_lbl);

                        ui.add_space(4.0);
                        let win11_tip = match lang {
                            Language::Ukrainian => "💡 Порада: для Windows 11 увімкніть 'Win11 Global Timer Fix' у вкладці 'Твіки Windows', щоб 0.5ms працював для всіх ігор.",
                            Language::English => "💡 Tip: On Windows 11 enable 'Win11 Global Timer Fix' in 'Windows Tweaks' so 0.5ms applies globally to games.",
                            Language::Russian => "💡 Совет: для Windows 11 включите 'Win11 Global Timer Fix' во вкладке 'Твики Windows', чтобы 0.5ms работал для всех игр.",
                        };
                        ui.label(egui::RichText::new(win11_tip).font(egui::FontId::proportional(11.0)).color(egui::Color32::GRAY));
                    });
                });

                ui.add_space(8.0);

                // RIGHT CARD: FilterKeys + 1:1 Mouse Raw Input
                ui.vertical(|ui| {
                    ui.set_width(430.0);
                    ui.group(|ui| {
                        let fk_active = self.filterkeys_enabled;
                        let (fk_title, fk_status) = match lang {
                            Language::Ukrainian => (
                                "⌨️ Клавіатура & Миша (FilterKeys 0 Delay + Raw 1:1)",
                                if fk_active {
                                    format!("● 0 DELAY АКТИВНО ({}ms / {}ms)", self.filterkeys_delay_ms, self.filterkeys_repeat_ms)
                                } else {
                                    "○ СТАНДАРТ WINDOWS (1000ms / 500ms)".into()
                                },
                            ),
                            Language::English => (
                                "⌨️ Keyboard & Mouse (FilterKeys 0 Delay + Raw 1:1)",
                                if fk_active {
                                    format!("● 0 DELAY ACTIVE ({}ms / {}ms)", self.filterkeys_delay_ms, self.filterkeys_repeat_ms)
                                } else {
                                    "○ WINDOWS DEFAULT (1000ms / 500ms)".into()
                                },
                            ),
                            Language::Russian => (
                                "⌨️ Клавиатура и Мышь (FilterKeys 0 Delay + Raw 1:1)",
                                if fk_active {
                                    format!("● 0 DELAY АКТИВНО ({}ms / {}ms)", self.filterkeys_delay_ms, self.filterkeys_repeat_ms)
                                } else {
                                    "○ СТАНДАРТ WINDOWS (1000ms / 500ms)".into()
                                },
                            ),
                        };

                        ui.label(egui::RichText::new(fk_title).font(egui::FontId::proportional(14.5)).strong());
                        ui.label(egui::RichText::new(fk_status).font(egui::FontId::proportional(12.0)).color(if fk_active { egui::Color32::from_rgb(0, 255, 128) } else { egui::Color32::GRAY }).strong());
                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            if ui.button("🎮 Fortnite Pro (150/25)").clicked() {
                                self.filterkeys_delay_ms = 150;
                                self.filterkeys_repeat_ms = 25;
                                self.apply_fk();
                            }
                            if ui.button("⚡ Ultra (120/20)").clicked() {
                                self.filterkeys_delay_ms = 120;
                                self.filterkeys_repeat_ms = 20;
                                self.apply_fk();
                            }
                            if ui.button("↩ Off").clicked() {
                                self.disable_fk();
                            }
                        });

                        ui.add(egui::Slider::new(&mut self.filterkeys_delay_ms, 100..=300).text("Delay ms"));
                        ui.add(egui::Slider::new(&mut self.filterkeys_repeat_ms, 10..=50).text("Repeat ms"));

                        ui.horizontal(|ui| {
                            let apply_txt = match lang {
                                Language::Ukrainian => "⚡ ЗАСТОСУВАТИ",
                                Language::English => "⚡ APPLY",
                                Language::Russian => "⚡ ПРИМЕНИТЬ",
                            };
                            if ui.add(egui::Button::new(egui::RichText::new(apply_txt).color(egui::Color32::BLACK).strong()).fill(egui::Color32::from_rgb(0, 200, 255))).clicked() {
                                self.apply_fk();
                            }

                            let auto_fk_lbl = match lang {
                                Language::Ukrainian => "Авто при бусті",
                                Language::English => "Auto on boost",
                                Language::Russian => "Авто при бусте",
                            };
                            ui.checkbox(&mut self.auto_apply_filterkeys, auto_fk_lbl);
                        });

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let (m_lbl, m_btn) = match lang {
                                Language::Ukrainian => (
                                    if !self.mouse_accel_enabled { "🎯 Акселерація миші: ВИМКНЕНА (1:1 Raw Aim ✔)" } else { "⚠️ Акселерація миші: УВІМКНЕНА" },
                                    if self.mouse_accel_enabled { "Вимкнути (1:1)" } else { "Увімкнути" },
                                ),
                                Language::English => (
                                    if !self.mouse_accel_enabled { "🎯 Mouse Accel: OFF (1:1 Raw Aim ✔)" } else { "⚠️ Mouse Accel: ON" },
                                    if self.mouse_accel_enabled { "Disable (1:1)" } else { "Enable" },
                                ),
                                Language::Russian => (
                                    if !self.mouse_accel_enabled { "🎯 Акселерация мыши: ВЫКЛ (1:1 Raw Aim ✔)" } else { "⚠️ Акселерация мыши: ВКЛ" },
                                    if self.mouse_accel_enabled { "Отключить (1:1)" } else { "Включить" },
                                ),
                            };
                            ui.label(egui::RichText::new(m_lbl).font(egui::FontId::proportional(11.5)).color(if !self.mouse_accel_enabled { egui::Color32::from_rgb(0, 230, 153) } else { egui::Color32::from_rgb(255, 170, 0) }));
                            if ui.button(m_btn).clicked() {
                                let target = !self.mouse_accel_enabled;
                                if set_mouse_accel(target) {
                                    self.mouse_accel_enabled = is_mouse_accel_enabled();
                                }
                            }
                        });

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut self.filterkeys_test_input);
                            if ui.button("🧪 Clear").clicked() {
                                self.filterkeys_test_input.clear();
                            }
                        });
                    });
                });
            });

            ui.add_space(8.0);

            // BOTTOM WIDE CARD: FPS Booster (Power Plan + Standby RAM Cleaner + Process Balancer)
            ui.group(|ui| {
                ui.set_width(874.0);
                let fps_title = match lang {
                    Language::Ukrainian => "🚀 FPS & Стабільність кадрів (Standby RAM Cleaner + Power Plan + CPU Балансер)",
                    Language::English => "🚀 FPS & Frametime Booster (Standby RAM Cleaner + Ultimate Power Plan + CPU Balancer)",
                    Language::Russian => "🚀 FPS и Стабильность кадров (Standby RAM Cleaner + Ultimate Power Plan + CPU Балансер)",
                };
                ui.label(egui::RichText::new(fps_title).font(egui::FontId::proportional(14.5)).strong().color(egui::Color32::from_rgb(0, 220, 255)));
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    // Memory & Standby Cleaner
                    ui.vertical(|ui| {
                        ui.set_width(420.0);
                        let ram_text = format!(
                            "RAM: {} / {} MB ({}% {}) • {} MB Free",
                            self.mem_stats.used_mb,
                            self.mem_stats.total_mb,
                            self.mem_stats.load_percent,
                            match lang {
                                Language::Ukrainian => "зайнято",
                                Language::English => "used",
                                Language::Russian => "занято",
                            },
                            self.mem_stats.avail_mb
                        );
                        ui.label(egui::RichText::new(ram_text).strong());

                        let clean_btn_txt = match lang {
                            Language::Ukrainian => "🧹 ОЧИСТИТИ RAM ТА STANDBY LIST ЗАРАЗ",
                            Language::English => "🧹 CLEAN RAM & PURGE STANDBY LIST NOW",
                            Language::Russian => "🧹 ОЧИСТИТЬ RAM И STANDBY LIST СЕЙЧАС",
                        };
                        if ui.add(egui::Button::new(egui::RichText::new(clean_btn_txt).color(egui::Color32::BLACK).strong()).fill(egui::Color32::from_rgb(0, 230, 153))).clicked() {
                            self.trigger_ram_clean();
                        }
                        if !self.ram_clean_status.is_empty() {
                            ui.label(egui::RichText::new(&self.ram_clean_status).font(egui::FontId::proportional(11.0)).color(egui::Color32::from_rgb(0, 230, 153)));
                        }
                        let auto_ram_lbl = match lang {
                            Language::Ukrainian => "Авто-очищення RAM при старті бусту (+ кожні 3 хв при навантаженні)",
                            Language::English => "Auto-clean RAM on boost (+ every 3m if >=70% used)",
                            Language::Russian => "Авто-очистка RAM при старте буста (+ каждые 3 мин при нагрузке)",
                        };
                        ui.checkbox(&mut self.auto_clean_ram, auto_ram_lbl);
                    });

                    ui.add_space(16.0);

                    // Power Plan & Process Balancer
                    ui.vertical(|ui| {
                        ui.set_width(420.0);
                        let pwr_lbl = match lang {
                            Language::Ukrainian => format!("⚡ План живлення: {}", self.active_power_plan_name),
                            Language::English => format!("⚡ Power Plan: {}", self.active_power_plan_name),
                            Language::Russian => format!("⚡ План питания: {}", self.active_power_plan_name),
                        };
                        ui.label(egui::RichText::new(pwr_lbl).strong());

                        let ult_btn_txt = match lang {
                            Language::Ukrainian => "⚡ УВІМКНУТИ ULTIMATE PERFORMANCE",
                            Language::English => "⚡ ACTIVATE ULTIMATE PERFORMANCE",
                            Language::Russian => "⚡ ВКЛЮЧИТЬ ULTIMATE PERFORMANCE",
                        };
                        if ui.add(egui::Button::new(egui::RichText::new(ult_btn_txt).color(egui::Color32::BLACK).strong()).fill(egui::Color32::from_rgb(0, 200, 255))).clicked() {
                            let (ok, name) = activate_ultimate_or_high_perf();
                            if ok {
                                self.active_power_plan_name = name;
                            }
                        }

                        let auto_pwr_lbl = match lang {
                            Language::Ukrainian => "Авто-перемикання на Ultimate Performance під час бусту",
                            Language::English => "Auto-switch to Ultimate Performance during game boost",
                            Language::Russian => "Авто-переключение на Ultimate Performance во время буста",
                        };
                        ui.checkbox(&mut self.auto_apply_power_plan, auto_pwr_lbl);

                        let auto_cpu_lbl = match lang {
                            Language::Ukrainian => "Знижувати пріоритет Chrome/Edge/OneDrive під час гри",
                            Language::English => "Throttle background Chrome/Edge/OneDrive CPU while gaming",
                            Language::Russian => "Снижать приоритет Chrome/Edge/OneDrive во время игры",
                        };
                        ui.checkbox(&mut self.auto_throttle_bg_procs, auto_cpu_lbl);
                    });
                });
            });
        });
    }

    fn render_network_dns(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;

        self.render_adapter_selector(ui);
        ui.add_space(6.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            // Game Relay Table
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    let h = match lang {
                        Language::Ukrainian => "🌐 Ігрові релеї та сервери (Нативний ICMP замір кожні 10с)",
                        Language::English => "🌐 Game Relays & Cloud Nodes (Native ICMP probed every 10s)",
                        Language::Russian => "🌐 Игровые релеи и серверы (Нативный ICMP замер каждые 10с)",
                    };
                    ui.heading(h);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let smart_lbl = match lang {
                            Language::Ukrainian => "Smart Routing (Авто-вибір)",
                            Language::English => "Smart Routing (Auto-pick fastest)",
                            Language::Russian => "Smart Routing (Авто-выбор)",
                        };
                        ui.checkbox(&mut self.auto_select_best_node, smart_lbl);
                    });
                });

                ui.add_space(4.0);
                for (idx, node) in self.nodes.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let is_sel = self.selected_node == idx;
                        if ui.selectable_label(is_sel, format!("📍 {}", node.location(lang))).clicked() {
                            self.selected_node = idx;
                            self.auto_select_best_node = false;
                            if let Some(p) = node.ping_ms {
                                self.current_ping = p;
                            }
                        }
                        ui.label(egui::RichText::new(format!("• {} ({})", node.name, node.host)).font(egui::FontId::proportional(11.5)).color(egui::Color32::GRAY));

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if let Some(p) = node.ping_ms {
                                let col = if p < 20.0 {
                                    egui::Color32::from_rgb(0, 255, 128)
                                } else if p < 40.0 {
                                    egui::Color32::from_rgb(0, 210, 255)
                                } else {
                                    egui::Color32::from_rgb(255, 170, 0)
                                };
                                ui.label(egui::RichText::new(format!("{:.1} ms", p)).color(col).strong());
                            } else {
                                ui.label(egui::RichText::new("⏳ ...").color(egui::Color32::GRAY));
                            }
                        });
                    });
                    ui.separator();
                }

                let ref_btn = match lang {
                    Language::Ukrainian => "🔄 Оновити пінг релеїв зараз",
                    Language::English => "🔄 Refresh all relay pings now",
                    Language::Russian => "🔄 Обновить пинг релеев сейчас",
                };
                if ui.button(ref_btn).clicked() {
                    self.refresh_nodes_ping();
                }
            });

            ui.add_space(8.0);

            // Two Bottom Cards: DNS Benchmark + MTU Optimizer
            ui.horizontal(|ui| {
                // DNS Switcher
                ui.vertical(|ui| {
                    ui.set_width(470.0);
                    ui.group(|ui| {
                        let dns_h = match lang {
                            Language::Ukrainian => "⚡ Gaming DNS Benchmark & Швидке перемикання",
                            Language::English => "⚡ Gaming DNS Benchmark & 1-Click Switcher",
                            Language::Russian => "⚡ Gaming DNS Benchmark и Быстрое переключение",
                        };
                        ui.label(egui::RichText::new(dns_h).font(egui::FontId::proportional(14.0)).strong());
                        ui.add_space(4.0);

                        let adapter_name = self.adapters.get(self.selected_adapter_idx).map(|a| a.name.clone()).unwrap_or_default();
                        let mut apply_target: Option<(&'static str, &'static str)> = None;

                        for d in &self.dns_presets {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(d.name).font(egui::FontId::proportional(12.0)));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let btn_txt = match lang {
                                        Language::Ukrainian => "Обрати",
                                        Language::English => "Apply",
                                        Language::Russian => "Выбрать",
                                    };
                                    if ui.add_enabled(!adapter_name.is_empty(), egui::Button::new(btn_txt)).clicked() {
                                        apply_target = Some((d.primary, d.secondary));
                                    }
                                    if let Some(ms) = d.ping_ms {
                                        let col = if ms < 20.0 { egui::Color32::from_rgb(0, 255, 128) } else { egui::Color32::from_rgb(0, 210, 255) };
                                        ui.label(egui::RichText::new(format!("{:.1} ms", ms)).color(col).strong());
                                    }
                                });
                            });
                        }

                        if let Some((p, s)) = apply_target {
                            let is_adm = self.is_admin;
                            let (tx, rx) = mpsc::channel();
                            self.tweaks_rx = Some(rx);
                            self.tweaks_busy = true;
                            thread::spawn(move || {
                                let res = apply_dns_server(&adapter_name, p, s, is_adm);
                                let _ = tx.send(res);
                            });
                        }

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            let bench_txt = match lang {
                                Language::Ukrainian => "🔍 Тест швидкості DNS",
                                Language::English => "🔍 Benchmark DNS",
                                Language::Russian => "🔍 Тест скорости DNS",
                            };
                            if ui.button(bench_txt).clicked() {
                                self.run_dns_benchmark();
                            }

                            let reset_txt = match lang {
                                Language::Ukrainian => "↩ Скинути DNS на Авто",
                                Language::English => "↩ Reset to Auto DHCP",
                                Language::Russian => "↩ Сбросить DNS на Авто",
                            };
                            let ad_name = self.adapters.get(self.selected_adapter_idx).map(|a| a.name.clone()).unwrap_or_default();
                            if ui.add_enabled(!ad_name.is_empty(), egui::Button::new(reset_txt)).clicked() {
                                let is_adm = self.is_admin;
                                let (tx, rx) = mpsc::channel();
                                self.tweaks_rx = Some(rx);
                                thread::spawn(move || {
                                    let res = reset_dns_dhcp(&ad_name, is_adm);
                                    let _ = tx.send(res);
                                });
                            }
                        });
                    });
                });

                ui.add_space(8.0);

                // Path MTU Discovery
                ui.vertical(|ui| {
                    ui.set_width(390.0);
                    ui.group(|ui| {
                        let mtu_h = match lang {
                            Language::Ukrainian => "📐 Оптимальний Path MTU (Без фрагментації)",
                            Language::English => "📐 Optimal Path MTU Finder (Zero Fragmentation)",
                            Language::Russian => "📐 Оптимальный Path MTU (Без фрагментации)",
                        };
                        ui.label(egui::RichText::new(mtu_h).font(egui::FontId::proportional(14.0)).strong());
                        ui.add_space(4.0);

                        let mtu_desc = match lang {
                            Language::Ukrainian => "Знаходить максимальний розмір пакета без роздроблення на лінії провайдера (1500 Ethernet / 1492 PPPoE).",
                            Language::English => "Finds highest non-fragmented packet size on your ISP line (1500 Ethernet / 1492 PPPoE).",
                            Language::Russian => "Находит максимальный размер пакета без фрагментации на линии провайдера (1500 Ethernet / 1492 PPPoE).",
                        };
                        ui.label(egui::RichText::new(mtu_desc).font(egui::FontId::proportional(11.0)).color(egui::Color32::GRAY));
                        ui.add_space(6.0);

                        let find_txt = if self.is_probing_mtu {
                            "⏳ Probing DF MTU...".to_string()
                        } else {
                            match lang {
                                Language::Ukrainian => "📐 Знайти оптимальний MTU",
                                Language::English => "📐 Discover Optimal MTU",
                                Language::Russian => "📐 Найти оптимальный MTU",
                            }.into()
                        };
                        if ui.add_enabled(!self.is_probing_mtu, egui::Button::new(find_txt)).clicked() {
                            self.run_mtu_discovery();
                        }

                        if !self.mtu_status_msg.is_empty() {
                            ui.label(egui::RichText::new(&self.mtu_status_msg).font(egui::FontId::proportional(11.5)).color(egui::Color32::from_rgb(0, 230, 153)));
                        }

                        if let Some(mtu) = self.mtu_result {
                            let apply_mtu_txt = format!("⚡ Apply MTU = {}", mtu);
                            let ad_name = self.adapters.get(self.selected_adapter_idx).map(|a| a.name.clone()).unwrap_or_default();
                            if ui.add_enabled(!ad_name.is_empty(), egui::Button::new(apply_mtu_txt)).clicked() {
                                let is_adm = self.is_admin;
                                let (tx, rx) = mpsc::channel();
                                self.tweaks_rx = Some(rx);
                                thread::spawn(move || {
                                    let res = apply_mtu_to_adapter(&ad_name, mtu, is_adm);
                                    let _ = tx.send(res);
                                });
                            }
                        }
                    });
                });
            });
        });
    }

    fn render_local_tweaks(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;

        let heading = match lang {
            Language::Ukrainian => "Системні твіки Windows (Максимальне зниження пінгу та підняття FPS)",
            Language::English => "Deep Windows Tweaks (Max Ping Reduction & FPS Boost)",
            Language::Russian => "Системные твики Windows (Максимальное снижение пинга и повышение FPS)",
        };
        ui.heading(heading);
        ui.add_space(4.0);
        self.render_adapter_selector(ui);
        ui.add_space(6.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal(|ui| {
                // LEFT: Network & Ping Tweaks
                ui.vertical(|ui| {
                    ui.set_width(430.0);
                    ui.group(|ui| {
                        let net_hdr = match lang {
                            Language::Ukrainian => "🌐 Мережа, Пінг та NIC Драйвер",
                            Language::English => "🌐 Network, Ping & NIC Hardware",
                            Language::Russian => "🌐 Сеть, Пинг и NIC Драйвер",
                        };
                        ui.label(egui::RichText::new(net_hdr).font(egui::FontId::proportional(14.0)).color(egui::Color32::from_rgb(0, 220, 255)).strong());
                        ui.add_space(6.0);

                        let (t1, d1) = match lang {
                            Language::Ukrainian => ("Вимкнути Nagle (TcpAckFrequency=1, TCPNoDelay=1)", "Миттєва відправка пакетів без буферної черги."),
                            Language::English => ("Disable Nagle (TcpAckFrequency=1, TCPNoDelay=1)", "Instant packet dispatch without TCP buffering delay."),
                            Language::Russian => ("Отключить Nagle (TcpAckFrequency=1, TCPNoDelay=1)", "Моментальная отправка пакетов без буферной очереди."),
                        };
                        ui.checkbox(&mut self.tweak_opts.tweak_nagle, egui::RichText::new(t1).strong());
                        ui.label(egui::RichText::new(d1).font(egui::FontId::proportional(10.5)).color(egui::Color32::GRAY));

                        let (t2, d2) = match lang {
                            Language::Ukrainian => ("NetworkThrottlingIndex=0xffffffff & Resp=0", "Виділяє 100% мережевого планувальника іграм."),
                            Language::English => ("NetworkThrottlingIndex=0xffffffff & Resp=0", "Gives 100% network scheduling priority to games."),
                            Language::Russian => ("NetworkThrottlingIndex=0xffffffff & Resp=0", "Выделяет 100% сетевого планировщика играм."),
                        };
                        ui.checkbox(&mut self.tweak_opts.tweak_throttling, egui::RichText::new(t2).strong());
                        ui.label(egui::RichText::new(d2).font(egui::FontId::proportional(10.5)).color(egui::Color32::GRAY));

                        let (t3, d3) = match lang {
                            Language::Ukrainian => ("QoS DSCP 46 (EF) + 0% ліміту оновлень Windows", "Пріоритет пакетів гри на роутері + зняття 20% резерву."),
                            Language::English => ("QoS DSCP 46 (EF) + 0% Reserved Bandwidth", "Tags game traffic Expedited Forwarding & removes 20% limit."),
                            Language::Russian => ("QoS DSCP 46 (EF) + 0% лимита обновлений Windows", "Приоритет пакетов игры на роутере + снятие 20% резерва."),
                        };
                        ui.checkbox(&mut self.tweak_opts.tweak_qos, egui::RichText::new(t3).strong());
                        ui.label(egui::RichText::new(d3).font(egui::FontId::proportional(10.5)).color(egui::Color32::GRAY));

                        let (t4, d4) = match lang {
                            Language::Ukrainian => ("Пріоритет фізичної карти над VPN (Metric 5 vs 50)", "Radmin/Hamachi більше не крадуть маршрут гри."),
                            Language::English => ("Physical Adapter Priority over VPN (Metric 5 vs 50)", "Prevents Radmin/Hamachi from hijacking routes."),
                            Language::Russian => ("Приоритет физической карты над VPN (Metric 5 vs 50)", "Radmin/Hamachi больше не крадут маршрут игры."),
                        };
                        ui.checkbox(&mut self.tweak_opts.tweak_nic_metric, egui::RichText::new(t4).strong());
                        ui.label(egui::RichText::new(d4).font(egui::FontId::proportional(10.5)).color(egui::Color32::GRAY));

                        let (t5, d5) = match lang {
                            Language::Ukrainian => ("Глибокий тюнінг карти (Interrupt Moderation=Off, EEE=Off)", "Прибирає паузи драйвера мережевої карти та енергозбереження."),
                            Language::English => ("Deep NIC Driver Tuning (InterruptMod=Off, EEE=Off, RSS=On)", "Eliminates NIC packet coalescing jitter & micro-sleeps."),
                            Language::Russian => ("Глубокий тюнинг карты (Interrupt Moderation=Off, EEE=Off)", "Убирает паузы драйвера сетевой карты и энергосбережение."),
                        };
                        ui.checkbox(&mut self.tweak_opts.tweak_nic_hardware, egui::RichText::new(t5).strong());
                        ui.label(egui::RichText::new(d5).font(egui::FontId::proportional(10.5)).color(egui::Color32::GRAY));
                    });
                });

                ui.add_space(8.0);

                // RIGHT: FPS & Kernel Tweaks + Actions
                ui.vertical(|ui| {
                    ui.set_width(430.0);
                    ui.group(|ui| {
                        let fps_hdr = match lang {
                            Language::Ukrainian => "🚀 FPS, Відеокарта та Ядро Windows",
                            Language::English => "🚀 FPS, GPU & Windows Kernel Latency",
                            Language::Russian => "🚀 FPS, Видеокарта и Ядро Windows",
                        };
                        ui.label(egui::RichText::new(fps_hdr).font(egui::FontId::proportional(14.0)).color(egui::Color32::from_rgb(0, 235, 160)).strong());
                        ui.add_space(6.0);

                        let (f1, fd1) = match lang {
                            Language::Ukrainian => ("Вимкнути GameDVR & Xbox запис у фоні (+5-15% FPS)", "Зупиняє невидимий запис екрану та мікрофризи DWM."),
                            Language::English => ("Disable GameDVR & Background Capture (+5-15% FPS)", "Stops hidden GPU background recording & DWM stutters."),
                            Language::Russian => ("Отключить GameDVR и фоновую запись (+5-15% FPS)", "Останавливает невидимую запись экрана и микрофризы."),
                        };
                        ui.checkbox(&mut self.tweak_opts.tweak_fps_gamedvr, egui::RichText::new(f1).strong());
                        ui.label(egui::RichText::new(fd1).font(egui::FontId::proportional(10.5)).color(egui::Color32::GRAY));

                        let (f2, fd2) = match lang {
                            Language::Ukrainian => ("PowerThrottling=Off + HAGS GPU=On + PrioritySep=0x26", "Максимальні частоти CPU/GPU для вікна гри."),
                            Language::English => ("PowerThrottling=Off + GPU HAGS=On + PrioritySep=0x26", "Prevents Windows E-core/background CPU throttling."),
                            Language::Russian => ("PowerThrottling=Off + HAGS GPU=On + PrioritySep=0x26", "Максимальные частоты CPU/GPU для окна игры."),
                        };
                        ui.checkbox(&mut self.tweak_opts.tweak_fps_power_throttling, egui::RichText::new(f2).strong());
                        ui.label(egui::RichText::new(fd2).font(egui::FontId::proportional(10.5)).color(egui::Color32::GRAY));

                        let (f3, fd3) = match lang {
                            Language::Ukrainian => ("Win11 Global Timer Fix (GlobalTimerResolutionRequests=1)", "Дозволяє 0.5ms таймеру діяти глобально на ігри у Win11."),
                            Language::English => ("Win11 Global 0.5ms Timer Fix (GlobalTimerResolutionRequests=1)", "Propagates 0.5ms timer resolution to games on Win11."),
                            Language::Russian => ("Win11 Global Timer Fix (GlobalTimerResolutionRequests=1)", "Позволяет 0.5ms таймеру действовать на игры в Win11."),
                        };
                        ui.checkbox(&mut self.tweak_opts.tweak_win11_timer, egui::RichText::new(f3).strong());
                        ui.label(egui::RichText::new(fd3).font(egui::FontId::proportional(10.5)).color(egui::Color32::GRAY));

                        let (f4, fd4) = match lang {
                            Language::Ukrainian => (
                                format!("Постійний High CPU/I-O пріоритет для {}", self.supported_games[self.selected_game].exe),
                                "Нативний реєстр IFEO (100% безпечно для EAC / Vanguard / VAC).",
                            ),
                            Language::English => (
                                format!("Persistent High CPU & I/O Priority for {}", self.supported_games[self.selected_game].exe),
                                "Native IFEO PerfOptions (100% safe for EAC/Vanguard/VAC).",
                            ),
                            Language::Russian => (
                                format!("Постоянный High CPU/I-O приоритет для {}", self.supported_games[self.selected_game].exe),
                                "Нативный реестр IFEO (100% безопасно для EAC / Vanguard).",
                            ),
                        };
                        ui.checkbox(&mut self.tweak_opts.tweak_game_priority_ifeo, egui::RichText::new(f4).strong());
                        ui.label(egui::RichText::new(fd4).font(egui::FontId::proportional(10.5)).color(egui::Color32::GRAY));
                    });
                });
            });

            ui.add_space(8.0);

            // Apply / Rollback Row + Log
            ui.horizontal(|ui| {
                let apply_txt = if self.tweaks_busy {
                    "⏳ Applying...".to_string()
                } else {
                    match lang {
                        Language::Ukrainian => "🛡️ ЗАСТОСУВАТИ ВСІ ОБРАНІ ТВІКИ",
                        Language::English => "🛡️ APPLY ALL SELECTED TWEAKS",
                        Language::Russian => "🛡️ ПРИМЕНИТЬ ВСЕ ВЫБРАННЫЕ ТВИКИ",
                    }.into()
                };
                let apply_btn = egui::Button::new(egui::RichText::new(apply_txt).font(egui::FontId::proportional(14.0)).color(egui::Color32::BLACK).strong())
                    .fill(egui::Color32::from_rgb(0, 235, 160))
                    .min_size(egui::vec2(380.0, 42.0));

                if ui.add_enabled(!self.tweaks_busy, apply_btn).clicked() {
                    self.tweaks_busy = true;
                    let opts = self.tweak_opts.clone();
                    let adapter = self.adapters.get(self.selected_adapter_idx).map(|a| a.name.clone()).unwrap_or_default();
                    let game_exe = self.supported_games[self.selected_game].exe.to_string();
                    let is_adm = self.is_admin;
                    let (tx, rx) = mpsc::channel();
                    self.tweaks_rx = Some(rx);
                    thread::spawn(move || {
                        let res = apply_selected_tweaks(&opts, &adapter, &game_exe, is_adm);
                        let _ = tx.send(res);
                    });
                }

                let rb_txt = match lang {
                    Language::Ukrainian => "↩ ВІДНОВИТИ СТАНДАРТ WINDOWS (ROLLBACK)",
                    Language::English => "↩ RESTORE WINDOWS DEFAULTS (ROLLBACK)",
                    Language::Russian => "↩ ВЕРНУТЬ СТАНДАРТ WINDOWS (ROLLBACK)",
                };
                let rb_btn = egui::Button::new(egui::RichText::new(rb_txt).font(egui::FontId::proportional(12.5)).color(egui::Color32::WHITE).strong())
                    .fill(egui::Color32::from_rgb(160, 55, 55))
                    .min_size(egui::vec2(310.0, 42.0));

                if ui.add_enabled(!self.tweaks_busy, rb_btn).clicked() {
                    self.tweaks_busy = true;
                    let adapter = self.adapters.get(self.selected_adapter_idx).map(|a| a.name.clone()).unwrap_or_default();
                    let game_exe = self.supported_games[self.selected_game].exe.to_string();
                    let is_adm = self.is_admin;
                    let (tx, rx) = mpsc::channel();
                    self.tweaks_rx = Some(rx);
                    thread::spawn(move || {
                        let res = rollback_default_tweaks(&adapter, &game_exe, is_adm);
                        let _ = tx.send(res);
                    });
                }
            });

            ui.add_space(6.0);
            ui.group(|ui| {
                ui.set_width(874.0);
                egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| {
                    for log in &self.tweak_status_log {
                        ui.label(egui::RichText::new(log).font(egui::FontId::monospace(11.0)).color(egui::Color32::from_rgb(0, 215, 145)));
                    }
                });
            });
        });
    }

    fn render_diagnostics(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;

        self.render_adapter_selector(ui);
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            let pre_lbl = match lang {
                Language::Ukrainian => "Швидкий вибір цілі:",
                Language::English => "Quick targets:",
                Language::Russian => "Быстрый выбор цели:",
            };
            ui.label(pre_lbl);
            if ui.button("Valve Frankfurt").clicked() { self.diag_host = "155.133.226.1".into(); }
            if ui.button("Valve Warsaw").clicked() { self.diag_host = "155.133.230.1".into(); }
            if ui.button("Fortnite AWS EU").clicked() { self.diag_host = "dynamodb.eu-central-1.amazonaws.com".into(); }
            if ui.button("Cloudflare 1.1.1.1").clicked() { self.diag_host = "1.1.1.1".into(); }
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let host_lbl = match lang {
                Language::Ukrainian => "Хост або IP:",
                Language::English => "Host or IP:",
                Language::Russian => "Хост или IP:",
            };
            ui.label(host_lbl);
            ui.text_edit_singleline(&mut self.diag_host);

            let std_btn = match lang {
                Language::Ukrainian => "▶ Стандартний тест (10)",
                Language::English => "▶ Standard Test (10)",
                Language::Russian => "▶ Стандартный тест (10)",
            };
            if ui.add_enabled(!self.is_pinging, egui::Button::new(std_btn)).clicked() {
                self.run_diag_test(false);
            }

            let burst_btn = match lang {
                Language::Ukrainian => "⚡ Тест на Bufferbloat / Стрибки (30)",
                Language::English => "⚡ Bufferbloat / Jitter Burst (30)",
                Language::Russian => "⚡ Тест на Bufferbloat / Скачки (30)",
            };
            if ui.add_enabled(!self.is_pinging, egui::Button::new(egui::RichText::new(burst_btn).color(egui::Color32::BLACK).strong()).fill(egui::Color32::from_rgb(0, 210, 255))).clicked() {
                self.run_diag_test(true);
            }
        });

        ui.add_space(10.0);

        ui.group(|ui| {
            ui.set_width(874.0);
            egui::ScrollArea::vertical().max_height(310.0).show(ui, |ui| {
                if self.diag_results.is_empty() {
                    let hint = match lang {
                        Language::Ukrainian => "Оберіть ціль і запустіть тест для перевірки Min / Avg / Max / Jitter та втрати пакетів.",
                        Language::English => "Select a target and run a test to measure exact sub-ms Min / Avg / Max / Jitter & Loss.",
                        Language::Russian => "Выберите цель и запустите тест для проверки Min / Avg / Max / Jitter и потери пакетов.",
                    };
                    ui.label(egui::RichText::new(hint).color(egui::Color32::GRAY));
                } else {
                    for res in &self.diag_results {
                        ui.label(egui::RichText::new(res).font(egui::FontId::monospace(11.5)));
                    }
                }
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([920.0, 645.0])
            .with_min_inner_size([860.0, 560.0])
            .with_title(APP_WINDOW_TITLE),
        ..Default::default()
    };

    eframe::run_native(
        APP_WINDOW_TITLE,
        native_options,
        Box::new(|cc| {
            let mut app = AppState::default();
            app.sync_system_metrics();

            // Background Network Adapters Detection
            let (adapter_tx, adapter_rx) = mpsc::channel();
            app.adapters_rx = Some(adapter_rx);
            let egui_ctx_adapters = cc.egui_ctx.clone();
            thread::spawn(move || {
                let list = query_network_adapters();
                let _ = adapter_tx.send(list);
                egui_ctx_adapters.request_repaint();
            });

            // Background Native ICMP Relay Monitor (Every 10s, zero child process spawn)
            let (node_tx, node_rx) = mpsc::channel();
            app.node_ping_rx = Some(node_rx);
            let nodes_info: Vec<(usize, String)> = app
                .nodes
                .iter()
                .enumerate()
                .map(|(i, n)| (i, n.host.to_string()))
                .collect();
            let egui_ctx_nodes = cc.egui_ctx.clone();
            let adapter_ip_arc = app.shared_adapter_ip.clone();

            thread::spawn(move || loop {
                let bind_ip = adapter_ip_arc.read().ok().and_then(|g| g.clone());
                for &(idx, ref host) in &nodes_info {
                    let ping = native_icmp_ping(host, bind_ip.as_deref(), 32, false, 800);
                    let _ = node_tx.send((idx, ping));
                }
                egui_ctx_nodes.request_repaint();
                thread::sleep(Duration::from_secs(10));
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

            // Setup Localized Tray Icon
            let (show_tx, show_rx) = mpsc::channel();
            app.tray_show_rx = Some(show_rx);
            let lang = app.language;

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
                let _ = tray_item.add_label("⚡ WPing v2.0 • Created by Vsqy");
                let _ = tray_item.add_label("───────────────");

                let show_txt = match lang {
                    Language::Ukrainian => "Показати WPing",
                    Language::English => "Show WPing",
                    Language::Russian => "Показать WPing",
                };
                let exit_txt = match lang {
                    Language::Ukrainian => "❌ Вийти з програми",
                    Language::English => "❌ Exit WPing",
                    Language::Russian => "❌ Выйти из программы",
                };

                let show_tx_c = show_tx.clone();
                let ctx_c1 = cc.egui_ctx.clone();
                let _ = tray_item.add_menu_item(show_txt, move || {
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

                let _ = tray_item.add_menu_item(exit_txt, move || {
                    std::process::exit(0);
                });
            }

            app._tray = tray;
            Ok(Box::new(app))
        }),
    )
}
