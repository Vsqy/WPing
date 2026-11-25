use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::str::FromStr;
use std::time::Instant;

pub const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Copy, Debug)]
pub struct MemoryStats {
    pub load_percent: u32,
    pub total_mb: u64,
    pub avail_mb: u64,
    pub used_mb: u64,
}

#[derive(Clone, Debug)]
pub struct PowerPlanInfo {
    pub guid: String,
    pub name: String,
    pub is_active: bool,
}

#[derive(Clone)]
pub struct GameDefinition {
    pub name: &'static str,
    pub exe: &'static str,
    pub server_idx: usize,
}

#[derive(Clone)]
pub struct DetectedGame {
    pub name: String,
    pub exe: String,
    pub pid: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(non_snake_case)]
pub struct FILTERKEYS {
    pub cbSize: u32,
    pub dwFlags: u32,
    pub iWaitMSec: u32,
    pub iDelayMSec: u32,
    pub iRepeatMSec: u32,
    pub iBounceMSec: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct IpOptionInformation {
    ttl: u8,
    tos: u8,
    flags: u8,
    options_size: u8,
    options_data: *mut u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct IcmpEchoReply {
    address: u32,
    status: u32,
    round_trip_time: u32,
    data_size: u16,
    reserved: u16,
    data: *mut u8,
    options: IpOptionInformation,
}

#[repr(C)]
#[allow(non_snake_case)]
struct MEMORYSTATUSEX {
    dwLength: u32,
    dwMemoryLoad: u32,
    ullTotalPhys: u64,
    ullAvailPhys: u64,
    ullTotalPageFile: u64,
    ullAvailPageFile: u64,
    ullTotalVirtual: u64,
    ullAvailVirtual: u64,
    ullAvailExtendedVirtual: u64,
}

#[repr(C)]
#[allow(non_snake_case)]
struct LUID {
    LowPart: u32,
    HighPart: i32,
}

#[repr(C)]
#[allow(non_snake_case)]
struct LUID_AND_ATTRIBUTES {
    Luid: LUID,
    Attributes: u32,
}

#[repr(C)]
#[allow(non_snake_case)]
struct TOKEN_PRIVILEGES {
    PrivilegeCount: u32,
    Privileges: [LUID_AND_ATTRIBUTES; 1],
}

const SPI_GETMOUSE: u32 = 0x0003;
const SPI_SETMOUSE: u32 = 0x0004;
const SPI_GETFILTERKEYS: u32 = 0x0032;
const SPI_SETFILTERKEYS: u32 = 0x0033;
const SPIF_UPDATEINIFILE: u32 = 0x0001;
const SPIF_SENDCHANGE: u32 = 0x0002;
const FKF_FILTERKEYSON: u32 = 0x00000001;
const FKF_AVAILABLE: u32 = 0x00000002;

const TOKEN_ADJUST_PRIVILEGES: u32 = 0x0020;
const TOKEN_QUERY: u32 = 0x0008;
const SE_PRIVILEGE_ENABLED: u32 = 0x00000002;

const PROCESS_SET_QUOTA: u32 = 0x0100;
const PROCESS_SET_INFORMATION: u32 = 0x0200;
const PROCESS_QUERY_INFORMATION: u32 = 0x0400;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

const NORMAL_PRIORITY_CLASS: u32 = 0x00000020;
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x00004000;
const HIGH_PRIORITY_CLASS: u32 = 0x00000080;

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
    fn NtSetSystemInformation(
        system_info_class: i32,
        system_info: *mut std::ffi::c_void,
        system_info_len: u32,
    ) -> i32;
}

#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(uPeriod: u32) -> u32;
    fn timeEndPeriod(uPeriod: u32) -> u32;
}

#[link(name = "iphlpapi")]
extern "system" {
    fn IcmpCreateFile() -> isize;
    fn IcmpCloseHandle(handle: isize) -> i32;
    fn IcmpSendEcho2Ex(
        icmp_handle: isize,
        event: isize,
        apc_routine: isize,
        apc_context: isize,
        source_address: u32,
        destination_address: u32,
        request_data: *const u8,
        request_size: u16,
        request_options: *const IpOptionInformation,
        reply_buffer: *mut u8,
        reply_size: u32,
        timeout: u32,
    ) -> u32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GlobalMemoryStatusEx(lpBuffer: *mut MEMORYSTATUSEX) -> i32;
    fn GetCurrentProcess() -> isize;
    fn OpenProcess(dwDesiredAccess: u32, bInheritHandle: i32, dwProcessId: u32) -> isize;
    fn CloseHandle(hObject: isize) -> i32;
    fn SetProcessWorkingSetSize(hProcess: isize, dwMinimumWorkingSetSize: usize, dwMaximumWorkingSetSize: usize) -> i32;
    fn SetPriorityClass(hProcess: isize, dwPriorityClass: u32) -> i32;
    fn GetPriorityClass(hProcess: isize) -> u32;
}

#[link(name = "advapi32")]
extern "system" {
    fn OpenProcessToken(ProcessHandle: isize, DesiredAccess: u32, TokenHandle: *mut isize) -> i32;
    fn LookupPrivilegeValueW(lpSystemName: *const u16, lpName: *const u16, lpLuid: *mut LUID) -> i32;
    fn AdjustTokenPrivileges(
        TokenHandle: isize,
        DisableAllPrivileges: i32,
        NewState: *const TOKEN_PRIVILEGES,
        BufferLength: u32,
        PreviousState: *mut std::ffi::c_void,
        ReturnLength: *mut u32,
    ) -> i32;
}

#[link(name = "shell32")]
extern "system" {
    fn IsUserAnAdmin() -> i32;
    fn ShellExecuteW(
        hwnd: isize,
        lpOperation: *const u16,
        lpFile: *const u16,
        lpParameters: *const u16,
        lpDirectory: *const u16,
        nShowCmd: i32,
    ) -> isize;
}

// ---------------- ADMIN & ELEVATION ----------------

pub fn is_user_admin() -> bool {
    unsafe { IsUserAnAdmin() != 0 }
}

pub fn restart_as_admin() -> bool {
    if let Ok(exe_path) = std::env::current_exe() {
        let op: Vec<u16> = "runas\0".encode_utf16().collect();
        let file: Vec<u16> = exe_path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        let dir: Vec<u16> = exe_path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let res = unsafe {
            ShellExecuteW(
                0,
                op.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                if dir.len() > 1 { dir.as_ptr() } else { std::ptr::null() },
                1, // SW_SHOWNORMAL
            )
        };
        if res > 32 {
            return true;
        }
    }
    false
}

// ---------------- NATIVE ICMP PING & PATH MTU ----------------

pub fn resolve_ipv4(host_or_ip: &str) -> Option<Ipv4Addr> {
    let trimmed = host_or_ip.trim();
    if let Ok(ip) = Ipv4Addr::from_str(trimmed) {
        return Some(ip);
    }
    if let Ok(addrs) = format!("{}:80", trimmed).to_socket_addrs() {
        for addr in addrs {
            if let SocketAddr::V4(v4) = addr {
                return Some(*v4.ip());
            }
        }
    }
    None
}

pub struct IcmpProber {
    handle: isize,
}

impl IcmpProber {
    pub fn new() -> Option<Self> {
        let h = unsafe { IcmpCreateFile() };
        if h == -1 || h == 0 {
            None
        } else {
            Some(Self { handle: h })
        }
    }

    /// Sends a real kernel ICMP echo bound to optional source IPv4, returning exact ms via QPC/Instant.
    pub fn ping_ipv4(
        &self,
        dest_ip: Ipv4Addr,
        source_ip: Option<Ipv4Addr>,
        payload_size: usize,
        dont_fragment: bool,
        timeout_ms: u32,
    ) -> Option<f32> {
        let dst = u32::from_ne_bytes(dest_ip.octets());
        let src = source_ip.map(|ip| u32::from_ne_bytes(ip.octets())).unwrap_or(0);
        let payload = vec![0x61u8; payload_size.clamp(8, 1500)];
        let reply_size = std::mem::size_of::<IcmpEchoReply>() + payload.len() + 64;
        let mut reply_buf = vec![0u8; reply_size];

        let opt = IpOptionInformation {
            ttl: 128,
            tos: 0,
            flags: if dont_fragment { 0x02 } else { 0 },
            options_size: 0,
            options_data: std::ptr::null_mut(),
        };

        let t0 = Instant::now();
        let ret = unsafe {
            IcmpSendEcho2Ex(
                self.handle,
                0,
                0,
                0,
                src,
                dst,
                payload.as_ptr(),
                payload.len() as u16,
                &opt,
                reply_buf.as_mut_ptr(),
                reply_buf.len() as u32,
                timeout_ms,
            )
        };
        let dt_ms = t0.elapsed().as_secs_f32() * 1000.0;

        if ret > 0 {
            let reply = unsafe { &*(reply_buf.as_ptr() as *const IcmpEchoReply) };
            if reply.status == 0 {
                return Some(dt_ms.max(0.2));
            }
        }
        None
    }
}

impl Drop for IcmpProber {
    fn drop(&mut self) {
        if self.handle != 0 && self.handle != -1 {
            unsafe {
                IcmpCloseHandle(self.handle);
            }
        }
    }
}

pub fn native_icmp_ping(
    target: &str,
    source_ip_str: Option<&str>,
    payload_size: usize,
    dont_fragment: bool,
    timeout_ms: u32,
) -> Option<f32> {
    let dest_ip = resolve_ipv4(target)?;
    let source_ip = source_ip_str.and_then(|s| Ipv4Addr::from_str(s.trim()).ok());
    let prober = IcmpProber::new()?;
    prober.ping_ipv4(dest_ip, source_ip, payload_size, dont_fragment, timeout_ms)
}

/// Binary searches highest non-fragmented Path MTU (range 1200..=1500) using IP_FLAG_DF.
pub fn discover_optimal_mtu(target: &str, source_ip_str: Option<&str>) -> Option<u16> {
    let dest_ip = resolve_ipv4(target)?;
    let source_ip = source_ip_str.and_then(|s| Ipv4Addr::from_str(s.trim()).ok());
    let prober = IcmpProber::new()?;

    // Verify target responds to basic DF ping first
    prober.ping_ipv4(dest_ip, source_ip, 572, true, 900)?;

    // Quick test 1472 (1500 MTU) directly first
    if prober.ping_ipv4(dest_ip, source_ip, 1472, true, 800).is_some() {
        return Some(1500);
    }

    let mut low: usize = 1200;
    let mut high: usize = 1471;
    let mut best: usize = 1200;

    while low <= high {
        let mid = (low + high) / 2;
        if prober.ping_ipv4(dest_ip, source_ip, mid, true, 700).is_some() {
            best = mid;
            low = mid + 1;
        } else {
            if mid == 0 {
                break;
            }
            high = mid - 1;
        }
    }

    Some((best + 28) as u16)
}

// ---------------- TIMER RESOLUTION ----------------

pub fn query_timer_resolution() -> (u32, u32, u32) {
    unsafe {
        let mut min = 156250u32;
        let mut max = 5000u32;
        let mut cur = 10000u32;
        NtQueryTimerResolution(&mut min, &mut max, &mut cur);
        (min, max, cur)
    }
}

pub fn enable_05ms_timer() {
    unsafe {
        timeBeginPeriod(1);
        let mut cur = 0u32;
        NtSetTimerResolution(5000, 1, &mut cur);
    }
}

pub fn disable_05ms_timer(min_100ns: u32) {
    unsafe {
        let mut cur = 0u32;
        let min = if min_100ns > 0 { min_100ns } else { 156250 };
        NtSetTimerResolution(min, 0, &mut cur);
        timeEndPeriod(1);
    }
}

// ---------------- FILTERKEYS & MOUSE 1:1 ----------------

pub fn query_filterkeys() -> (bool, u32, u32) {
    let mut fk = FILTERKEYS {
        cbSize: std::mem::size_of::<FILTERKEYS>() as u32,
        dwFlags: 0,
        iWaitMSec: 0,
        iDelayMSec: 0,
        iRepeatMSec: 0,
        iBounceMSec: 0,
    };
    unsafe {
        if SystemParametersInfoW(
            SPI_GETFILTERKEYS,
            fk.cbSize,
            &mut fk as *mut _ as *mut std::ffi::c_void,
            0,
        ) != 0
        {
            let on = (fk.dwFlags & FKF_FILTERKEYSON) != 0;
            let delay = if fk.iDelayMSec >= 50 && fk.iDelayMSec <= 2000 {
                fk.iDelayMSec
            } else {
                150
            };
            let repeat = if fk.iRepeatMSec >= 5 && fk.iRepeatMSec <= 1000 {
                fk.iRepeatMSec
            } else {
                25
            };
            return (on, delay, repeat);
        }
    }
    (false, 150, 25)
}

pub fn apply_filterkeys(delay_ms: u32, repeat_ms: u32) -> bool {
    let mut fk = FILTERKEYS {
        cbSize: std::mem::size_of::<FILTERKEYS>() as u32,
        dwFlags: FKF_FILTERKEYSON | FKF_AVAILABLE,
        iWaitMSec: 0,
        iDelayMSec: delay_ms,
        iRepeatMSec: repeat_ms,
        iBounceMSec: 0,
    };
    unsafe {
        SystemParametersInfoW(
            SPI_SETFILTERKEYS,
            fk.cbSize,
            &mut fk as *mut _ as *mut std::ffi::c_void,
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        ) != 0
    }
}

pub fn disable_filterkeys() -> bool {
    let mut fk = FILTERKEYS {
        cbSize: std::mem::size_of::<FILTERKEYS>() as u32,
        dwFlags: FKF_AVAILABLE,
        iWaitMSec: 0,
        iDelayMSec: 1000,
        iRepeatMSec: 500,
        iBounceMSec: 0,
    };
    unsafe {
        SystemParametersInfoW(
            SPI_SETFILTERKEYS,
            fk.cbSize,
            &mut fk as *mut _ as *mut std::ffi::c_void,
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        ) != 0
    }
}

pub fn is_mouse_accel_enabled() -> bool {
    let mut mouse = [0i32; 3];
    unsafe {
        if SystemParametersInfoW(SPI_GETMOUSE, 0, mouse.as_mut_ptr() as *mut _, 0) != 0 {
            return mouse[2] != 0;
        }
    }
    false
}

pub fn set_mouse_accel(enable: bool) -> bool {
    let mut mouse = if enable { [6i32, 10, 1] } else { [0i32, 0, 0] };
    unsafe {
        SystemParametersInfoW(
            SPI_SETMOUSE,
            0,
            mouse.as_mut_ptr() as *mut _,
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        ) != 0
    }
}

// ---------------- RAM TELEMETRY & STANDBY CLEANER ----------------

pub fn query_memory_stats() -> MemoryStats {
    let mut mem: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    mem.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    unsafe {
        if GlobalMemoryStatusEx(&mut mem) != 0 {
            let total_mb = mem.ullTotalPhys / (1024 * 1024);
            let avail_mb = mem.ullAvailPhys / (1024 * 1024);
            return MemoryStats {
                load_percent: mem.dwMemoryLoad,
                total_mb,
                avail_mb,
                used_mb: total_mb.saturating_sub(avail_mb),
            };
        }
    }
    MemoryStats {
        load_percent: 0,
        total_mb: 0,
        avail_mb: 0,
        used_mb: 0,
    }
}

fn enable_profile_privilege() {
    unsafe {
        let mut token: isize = 0;
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token) != 0 {
            let priv_name: Vec<u16> = "SeProfileSingleProcessPrivilege\0".encode_utf16().collect();
            let mut luid = LUID { LowPart: 0, HighPart: 0 };
            if LookupPrivilegeValueW(std::ptr::null(), priv_name.as_ptr(), &mut luid) != 0 {
                let tp = TOKEN_PRIVILEGES {
                    PrivilegeCount: 1,
                    Privileges: [LUID_AND_ATTRIBUTES {
                        Luid: luid,
                        Attributes: SE_PRIVILEGE_ENABLED,
                    }],
                };
                AdjustTokenPrivileges(
                    token,
                    0,
                    &tp,
                    std::mem::size_of::<TOKEN_PRIVILEGES>() as u32,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
            CloseHandle(token);
        }
    }
}

/// Trims unused working sets of heavy background apps AND purges Windows Standby List when elevated.
/// Returns MB of physical RAM freed.
pub fn clean_ram_and_standby(is_admin: bool, exclude_game_pid: Option<u32>) -> (u64, bool) {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };

    let before = query_memory_stats();

    let trim_targets = [
        "chrome.exe",
        "msedge.exe",
        "opera.exe",
        "firefox.exe",
        "brave.exe",
        "discord.exe",
        "spotify.exe",
        "steamwebhelper.exe",
        "epicwebhelper.exe",
        "onedrive.exe",
        "slack.exe",
        "teams.exe",
        "searchhost.exe",
        "widgets.exe",
    ];

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot != INVALID_HANDLE_VALUE && snapshot != 0 {
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

            if Process32FirstW(snapshot, &mut entry) != 0 {
                loop {
                    let pid = entry.th32ProcessID;
                    if Some(pid) != exclude_game_pid {
                        let null_pos = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                        let exe = String::from_utf16_lossy(&entry.szExeFile[..null_pos]).to_lowercase();
                        if trim_targets.iter().any(|&t| exe == t) {
                            let h = OpenProcess(PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                            if h != 0 {
                                SetProcessWorkingSetSize(h, usize::MAX, usize::MAX);
                                CloseHandle(h);
                            }
                        }
                    }
                    if Process32NextW(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            windows_sys::Win32::Foundation::CloseHandle(snapshot);
        }
    }

    let mut standby_purged = false;
    if is_admin {
        enable_profile_privilege();
        unsafe {
            // SystemMemoryListInformation = 80
            // MemoryFlushModifiedList = 3, MemoryPurgeStandbyList = 4
            let mut cmd_flush: i32 = 3;
            let _ = NtSetSystemInformation(80, &mut cmd_flush as *mut _ as *mut _, 4);
            let mut cmd_purge: i32 = 4;
            let status = NtSetSystemInformation(80, &mut cmd_purge as *mut _ as *mut _, 4);
            if status == 0 {
                standby_purged = true;
            }
        }
    }

    let after = query_memory_stats();
    let freed_mb = after.avail_mb.saturating_sub(before.avail_mb);
    (freed_mb, standby_purged)
}

// ---------------- GAME DETECTION & LIVE PROCESS PRIORITY BALANCER ----------------

pub fn scan_for_games(supported_games: &[GameDefinition]) -> Option<DetectedGame> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
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

/// Lowers CPU priority of heavy background bloat processes during gaming, and tries boosting the detected game PID.
/// Returns list of (pid, original_priority) for safe restoration on Stop.
pub fn balance_processes_for_gaming(active_game_pid: Option<u32>) -> Vec<(u32, u32)> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };

    let bg_hogs = [
        "chrome.exe",
        "msedge.exe",
        "opera.exe",
        "firefox.exe",
        "brave.exe",
        "onedrive.exe",
        "searchindexer.exe",
        "widgets.exe",
        "tiworker.exe",
        "trustedinstaller.exe",
    ];

    let mut adjusted = Vec::new();

    unsafe {
        if let Some(gpid) = active_game_pid {
            let h = OpenProcess(PROCESS_SET_INFORMATION | PROCESS_QUERY_INFORMATION, 0, gpid);
            if h != 0 {
                let old = GetPriorityClass(h);
                if old == NORMAL_PRIORITY_CLASS {
                    if SetPriorityClass(h, HIGH_PRIORITY_CLASS) != 0 {
                        adjusted.push((gpid, old));
                    }
                }
                CloseHandle(h);
            }
        }

        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot != INVALID_HANDLE_VALUE && snapshot != 0 {
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

            if Process32FirstW(snapshot, &mut entry) != 0 {
                loop {
                    let pid = entry.th32ProcessID;
                    if Some(pid) != active_game_pid {
                        let null_pos = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                        let exe = String::from_utf16_lossy(&entry.szExeFile[..null_pos]).to_lowercase();
                        if bg_hogs.iter().any(|&b| exe == b) {
                            let h = OpenProcess(PROCESS_SET_INFORMATION | PROCESS_QUERY_INFORMATION, 0, pid);
                            if h != 0 {
                                let old = GetPriorityClass(h);
                                if old == NORMAL_PRIORITY_CLASS {
                                    if SetPriorityClass(h, BELOW_NORMAL_PRIORITY_CLASS) != 0 {
                                        adjusted.push((pid, old));
                                    }
                                }
                                CloseHandle(h);
                            }
                        }
                    }
                    if Process32NextW(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snapshot);
        }
    }

    adjusted
}

pub fn restore_balanced_processes(adjusted: &[(u32, u32)]) {
    unsafe {
        for &(pid, old_prio) in adjusted {
            let h = OpenProcess(PROCESS_SET_INFORMATION, 0, pid);
            if h != 0 {
                SetPriorityClass(h, old_prio);
                CloseHandle(h);
            }
        }
    }
}

// ---------------- WINDOWS POWER PLAN MANAGER ----------------

pub fn list_power_plans() -> Vec<PowerPlanInfo> {
    let mut plans = Vec::new();
    let out = Command::new("powercfg")
        .args(&["/list"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    if let Ok(o) = out {
        let text = String::from_utf8_lossy(&o.stdout);
        for line in text.lines() {
            // Format: Power Scheme GUID: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx  (Name) *
            if let Some(pos) = line.find("GUID:") {
                let rest = line[pos + 5..].trim();
                let mut parts = rest.splitn(2, ' ');
                if let (Some(guid), Some(tail)) = (parts.next(), parts.next()) {
                    let guid = guid.trim().to_string();
                    let is_active = tail.trim_end().ends_with('*');
                    let name = if let (Some(s), Some(e)) = (tail.find('('), tail.rfind(')')) {
                        tail[s + 1..e].to_string()
                    } else {
                        tail.replace('*', "").trim().to_string()
                    };
                    if guid.len() >= 32 {
                        plans.push(PowerPlanInfo { guid, name, is_active });
                    }
                }
            }
        }
    }
    plans
}

pub fn get_active_power_plan() -> Option<PowerPlanInfo> {
    list_power_plans().into_iter().find(|p| p.is_active)
}

pub fn activate_power_plan(guid: &str) -> bool {
    Command::new("powercfg")
        .args(&["/setactive", guid])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Activates Ultimate Performance (or duplicates Microsoft Ultimate template if missing, falling back to High Performance).
/// Returns (activated_ok, active_plan_name_or_guid).
pub fn activate_ultimate_or_high_perf() -> (bool, String) {
    let plans = list_power_plans();

    // 1. Prefer existing Ultimate Performance plan (GUID ends or contains or name matches)
    for p in &plans {
        let lower = p.name.to_lowercase();
        if lower.contains("ultimate") || lower.contains("максимальн") {
            if activate_power_plan(&p.guid) {
                return (true, p.name.clone());
            }
        }
    }

    // 2. Try unlocking Windows hidden Ultimate Performance template
    let dup = Command::new("powercfg")
        .args(&["-duplicatescheme", "e9a42b02-d5df-448d-aa00-03f14749eb61"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    if let Ok(o) = dup {
        let out_str = String::from_utf8_lossy(&o.stdout);
        for tok in out_str.split_whitespace() {
            if tok.len() == 36 && tok.chars().filter(|&c| c == '-').count() == 4 {
                if activate_power_plan(tok) {
                    return (true, "Ultimate Performance".to_string());
                }
            }
        }
    }

    // 3. Fallback to High Performance standard GUID
    let high_guid = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
    if activate_power_plan(high_guid) {
        return (true, "High Performance".to_string());
    }

    for p in &plans {
        let lower = p.name.to_lowercase();
        if lower.contains("high") || lower.contains("висок") || lower.contains("высок") {
            if activate_power_plan(&p.guid) {
                return (true, p.name.clone());
            }
        }
    }

    (false, "Unavailable".to_string())
}

// ---------------- WI-FI ANTI-SCAN & NETWORK SESSION OPTIMIZATIONS ----------------

pub fn flush_dns_and_arp_cache() {
    let _ = Command::new("ipconfig")
        .args(&["/flushdns"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

/// Pauses Windows WLAN background SSID scanning during Boost (prevents every-60s 100ms+ Wi-Fi ping spikes).
/// Safely ignored on Ethernet; re-enabled on Stop Boost / Exit.
pub fn set_wifi_scan_enabled(adapter_name: &str, enable: bool) -> bool {
    if adapter_name.is_empty() {
        return false;
    }
    let state = if enable { "yes" } else { "no" };
    Command::new("netsh")
        .args(&[
            "wlan",
            "set",
            "autoconfig",
            &format!("enabled={}", state),
            &format!("interface={}", adapter_name),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

