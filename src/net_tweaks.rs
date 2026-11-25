use crate::sys_native::CREATE_NO_WINDOW;
use std::fs;
use std::os::windows::process::CommandExt;
use std::process::Command;

#[derive(Clone, Debug, PartialEq)]
pub struct NetworkAdapter {
    pub name: String,
    pub description: String,
    pub is_virtual: bool,
    pub ip: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DnsPreset {
    pub name: &'static str,
    pub primary: &'static str,
    pub secondary: &'static str,
    pub ping_ms: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct TweakOptions {
    // Ping & Network Stack
    pub tweak_nagle: bool,
    pub tweak_throttling: bool,
    pub tweak_qos: bool,
    pub tweak_nic_metric: bool,
    pub tweak_nic_hardware: bool,
    // FPS & Kernel Latency
    pub tweak_fps_gamedvr: bool,
    pub tweak_fps_power_throttling: bool,
    pub tweak_win11_timer: bool,
    pub tweak_game_priority_ifeo: bool,
}

impl Default for TweakOptions {
    fn default() -> Self {
        Self {
            tweak_nagle: true,
            tweak_throttling: true,
            tweak_qos: true,
            tweak_nic_metric: true,
            tweak_nic_hardware: true,
            tweak_fps_gamedvr: true,
            tweak_fps_power_throttling: true,
            tweak_win11_timer: true,
            tweak_game_priority_ifeo: true,
        }
    }
}

pub fn default_dns_presets() -> Vec<DnsPreset> {
    vec![
        DnsPreset {
            name: "Cloudflare Gaming (1.1.1.1)",
            primary: "1.1.1.1",
            secondary: "1.0.0.1",
            ping_ms: None,
        },
        DnsPreset {
            name: "Google Public DNS (8.8.8.8)",
            primary: "8.8.8.8",
            secondary: "8.8.4.4",
            ping_ms: None,
        },
        DnsPreset {
            name: "Quad9 Security (9.9.9.9)",
            primary: "9.9.9.9",
            secondary: "149.112.112.112",
            ping_ms: None,
        },
        DnsPreset {
            name: "OpenDNS Home (208.67.222.222)",
            primary: "208.67.222.222",
            secondary: "208.67.220.220",
            ping_ms: None,
        },
        DnsPreset {
            name: "AdGuard DNS (94.140.14.14)",
            primary: "94.140.14.14",
            secondary: "94.140.15.15",
            ping_ms: None,
        },
    ]
}

pub fn query_network_adapters() -> Vec<NetworkAdapter> {
    let out = Command::new("powershell")
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

    list.sort_by_key(|a| (a.is_virtual, a.ip.is_none()));
    list
}

fn sanitize_ps_single_quoted(s: &str) -> String {
    s.replace('\'', "''")
}

/// Executes a PowerShell script with Admin rights (directly if already elevated, else via RunAs UAC),
/// capturing structured lines written to a temp log file.
pub fn run_elevated_powershell(ps_body: &str, is_admin: bool) -> Vec<String> {
    let temp = std::env::temp_dir();
    let script_path = temp.join("wping_task.ps1");
    let log_path = temp.join("wping_task_log.txt");

    let _ = fs::remove_file(&log_path);

    let wrapped = format!(
        "$ErrorActionPreference = 'SilentlyContinue'\r\n\
         $logFile = '{}'\r\n\
         function Log($msg) {{ Add-Content -Path $logFile -Value $msg -Encoding UTF8 }}\r\n\
         {}\r\n\
         Log 'DONE_MARKER'\r\n",
        sanitize_ps_single_quoted(&log_path.to_string_lossy()),
        ps_body
    );

    if fs::write(&script_path, wrapped.as_bytes()).is_err() {
        return vec!["Error writing temporary PowerShell script.".to_string()];
    }

    if is_admin {
        let _ = Command::new("powershell")
            .args(&[
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                &script_path.to_string_lossy(),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    } else {
        let sp = sanitize_ps_single_quoted(&script_path.to_string_lossy());
        let invoke_cmd = format!(
            "Start-Process -FilePath 'powershell.exe' -ArgumentList '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File \"{}\"' -Verb RunAs -Wait",
            sp
        );
        let status = Command::new("powershell")
            .args(&["-NoProfile", "-Command", &invoke_cmd])
            .creation_flags(CREATE_NO_WINDOW)
            .status();

        if status.is_err() {
            return vec!["✖ UAC elevation cancelled or failed.".to_string()];
        }
    }

    let _ = fs::remove_file(&script_path);

    if let Ok(content) = fs::read_to_string(&log_path) {
        let _ = fs::remove_file(&log_path);
        let lines: Vec<String> = content
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && l != "DONE_MARKER")
            .collect();
        if !lines.is_empty() {
            return lines;
        }
    }

    vec!["⚠ UAC prompt was cancelled or no output recorded.".to_string()]
}

pub fn apply_dns_server(adapter_name: &str, primary: &str, secondary: &str, is_admin: bool) -> Vec<String> {
    let a = sanitize_ps_single_quoted(adapter_name);
    let ps = format!(
        "Set-DnsClientServerAddress -InterfaceAlias '{}' -ServerAddresses ('{}','{}')\r\n\
         ipconfig /flushdns | Out-Null\r\n\
         Log '✔ DNS updated to {} / {} on {}'\r\n",
        a, primary, secondary, primary, secondary, a
    );
    run_elevated_powershell(&ps, is_admin)
}

pub fn reset_dns_dhcp(adapter_name: &str, is_admin: bool) -> Vec<String> {
    let a = sanitize_ps_single_quoted(adapter_name);
    let ps = format!(
        "Set-DnsClientServerAddress -InterfaceAlias '{}' -ResetServerAddresses\r\n\
         ipconfig /flushdns | Out-Null\r\n\
         Log '✔ DNS reset to automatic ISP DHCP on {}'\r\n",
        a, a
    );
    run_elevated_powershell(&ps, is_admin)
}

pub fn apply_mtu_to_adapter(adapter_name: &str, mtu: u16, is_admin: bool) -> Vec<String> {
    let a = sanitize_ps_single_quoted(adapter_name);
    let ps = format!(
        "netsh interface ipv4 set subinterface \"{}\" mtu={} store=persistent | Out-Null\r\n\
         Log '✔ Applied MTU={} on adapter {}'\r\n",
        adapter_name.replace('"', ""),
        mtu,
        mtu,
        a
    );
    run_elevated_powershell(&ps, is_admin)
}

pub fn apply_selected_tweaks(opts: &TweakOptions, adapter_name: &str, game_exe: &str, is_admin: bool) -> Vec<String> {
    let a = sanitize_ps_single_quoted(adapter_name);
    let g = sanitize_ps_single_quoted(game_exe);
    let mut ps = String::new();

    if opts.tweak_nagle {
        ps.push_str(
            "$ifaces = Get-ChildItem 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\Parameters\\Interfaces'\r\n\
             foreach ($iface in $ifaces) {\r\n\
                 Set-ItemProperty -Path $iface.PSPath -Name 'TcpAckFrequency' -Value 1 -Type DWord -Force\r\n\
                 Set-ItemProperty -Path $iface.PSPath -Name 'TCPNoDelay' -Value 1 -Type DWord -Force\r\n\
                 Set-ItemProperty -Path $iface.PSPath -Name 'TcpDelAckTicks' -Value 0 -Type DWord -Force\r\n\
             }\r\n\
             netsh int tcp set global autotuninglevel=normal | Out-Null\r\n\
             netsh int tcp set global heuristics=disabled | Out-Null\r\n\
             netsh int tcp set global ecncapability=disabled | Out-Null\r\n\
             Log '✔ Nagle Disabled (TcpAckFrequency=1, TCPNoDelay=1, TcpDelAckTicks=0)'\r\n",
        );
    }

    if opts.tweak_throttling {
        ps.push_str(
            "reg add 'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile' /v NetworkThrottlingIndex /t REG_DWORD /d 0xffffffff /f | Out-Null\r\n\
             reg add 'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile' /v SystemResponsiveness /t REG_DWORD /d 0 /f | Out-Null\r\n\
             reg add 'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile\\Tasks\\Games' /v 'GPU Priority' /t REG_DWORD /d 8 /f | Out-Null\r\n\
             reg add 'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile\\Tasks\\Games' /v 'Priority' /t REG_DWORD /d 6 /f | Out-Null\r\n\
             reg add 'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile\\Tasks\\Games' /v 'Scheduling Category' /t REG_SZ /d 'High' /f | Out-Null\r\n\
             reg add 'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile\\Tasks\\Games' /v 'SFIO Priority' /t REG_SZ /d 'High' /f | Out-Null\r\n\
             reg add 'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile\\Tasks\\Games' /v 'Latency Sensitive' /t REG_SZ /d 'True' /f | Out-Null\r\n\
             Log '✔ Network Throttling Disabled & MMCSS Games GPU/CPU Priority = High'\r\n",
        );
    }

    if opts.tweak_qos {
        ps.push_str(&format!(
            "reg add 'HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\Psched' /v NonBestEffortLimit /t REG_DWORD /d 0 /f | Out-Null\r\n\
             Remove-NetQosPolicy -Name 'WPing_Game_QoS' -Confirm:$false -ErrorAction SilentlyContinue\r\n\
             if ('{}' -ne '') {{\r\n\
                 New-NetQosPolicy -Name 'WPing_Game_QoS' -AppPathNameMatchCondition '{}' -DSCPAction 46 -PriorityValue8021Action 6 -NetworkProfile All | Out-Null\r\n\
             }}\r\n\
             Log '✔ QoS DSCP 46 (Expedited Forwarding) Active & Reserve Bandwidth = 0%'\r\n",
            g, g
        ));
    }

    if opts.tweak_nic_metric {
        ps.push_str(&format!(
            "if ('{}' -ne '' -and (Get-NetAdapter -Name '{}' -ErrorAction SilentlyContinue)) {{\r\n\
                 Set-NetIPInterface -InterfaceAlias '{}' -InterfaceMetric 5 -ErrorAction SilentlyContinue\r\n\
             }} else {{\r\n\
                 Get-NetAdapter | Where-Object {{ $_.Status -eq 'Up' -and $_.Virtual -eq $false -and $_.InterfaceDescription -notmatch 'VPN|Virtual|Radmin|Hamachi|TAP' }} | ForEach-Object {{\r\n\
                     Set-NetIPInterface -InterfaceIndex $_.InterfaceIndex -InterfaceMetric 5 -ErrorAction SilentlyContinue\r\n\
                 }}\r\n\
             }}\r\n\
             Get-NetAdapter | Where-Object {{ $_.Virtual -eq $true -or $_.InterfaceDescription -match 'VPN|Virtual|Radmin|Hamachi|TAP|ZeroTier' -or $_.Name -match 'VPN|Radmin|Hamachi' }} | ForEach-Object {{\r\n\
                 Set-NetIPInterface -InterfaceIndex $_.InterfaceIndex -InterfaceMetric 50 -ErrorAction SilentlyContinue\r\n\
             }}\r\n\
             Log '✔ Physical Adapter Metric=5 vs VPN Metric=50 Applied'\r\n",
            a, a, a
        ));
    }

    if opts.tweak_nic_hardware {
        ps.push_str(
            "netsh int tcp set global rss=enabled | Out-Null\r\n\
             netsh int ip set global taskoffload=enabled | Out-Null\r\n\
             Get-NetAdapter | Where-Object { $_.Status -eq 'Up' -and $_.Virtual -eq $false } | ForEach-Object {\r\n\
                 Enable-NetAdapterRss -Name $_.Name -ErrorAction SilentlyContinue\r\n\
                 Disable-NetAdapterPowerManagement -Name $_.Name -ErrorAction SilentlyContinue\r\n\
                 Set-NetAdapterAdvancedProperty -Name $_.Name -RegistryKeyword '*InterruptModeration' -RegistryValue 0 -ErrorAction SilentlyContinue\r\n\
                 Set-NetAdapterAdvancedProperty -Name $_.Name -RegistryKeyword '*EEE' -RegistryValue 0 -ErrorAction SilentlyContinue\r\n\
                 Set-NetAdapterAdvancedProperty -Name $_.Name -RegistryKeyword 'GreenEthernet' -RegistryValue 0 -ErrorAction SilentlyContinue\r\n\
                 Set-NetAdapterAdvancedProperty -Name $_.Name -RegistryKeyword '*FlowControl' -RegistryValue 0 -ErrorAction SilentlyContinue\r\n\
             }\r\n\
             Log '✔ NIC Interrupt Moderation=Off, EEE Power Saving=Off, FlowControl=Off, RSS=On'\r\n",
        );
    }

    if opts.tweak_fps_gamedvr {
        ps.push_str(
            "reg add 'HKCU\\System\\GameConfigStore' /v GameDVR_Enabled /t REG_DWORD /d 0 /f | Out-Null\r\n\
             reg add 'HKCU\\System\\GameConfigStore' /v GameDVR_FSEBehaviorMode /t REG_DWORD /d 2 /f | Out-Null\r\n\
             reg add 'HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\GameDVR' /v AppCaptureEnabled /t REG_DWORD /d 0 /f | Out-Null\r\n\
             reg add 'HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\GameDVR' /v AllowGameDVR /t REG_DWORD /d 0 /f | Out-Null\r\n\
             Log '✔ GameDVR & Background Capture Disabled (+FPS / Reduced Stutters)'\r\n",
        );
    }

    if opts.tweak_fps_power_throttling {
        ps.push_str(
            "reg add 'HKLM\\SYSTEM\\CurrentControlSet\\Control\\Power\\PowerThrottling' /v PowerThrottlingOff /t REG_DWORD /d 1 /f | Out-Null\r\n\
             reg add 'HKLM\\SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers' /v HwSchMode /t REG_DWORD /d 2 /f | Out-Null\r\n\
             reg add 'HKLM\\SYSTEM\\CurrentControlSet\\Control\\PriorityControl' /v Win32PrioritySeparation /t REG_DWORD /d 38 /f | Out-Null\r\n\
             Log '✔ PowerThrottlingOff=1, GPU HAGS=Enabled (HwSchMode=2), Win32PrioritySeparation=0x26'\r\n",
        );
    }

    if opts.tweak_win11_timer {
        ps.push_str(
            "reg add 'HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\kernel' /v GlobalTimerResolutionRequests /t REG_DWORD /d 1 /f | Out-Null\r\n\
             Log '✔ Win11 GlobalTimerResolutionRequests=1 (0.5ms applies globally to games)'\r\n",
        );
    }

    if opts.tweak_game_priority_ifeo && !g.is_empty() {
        ps.push_str(&format!(
            "reg add 'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Image File Execution Options\\{}\\PerfOptions' /v CpuPriorityClass /t REG_DWORD /d 3 /f | Out-Null\r\n\
             reg add 'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Image File Execution Options\\{}\\PerfOptions' /v IoPriority /t REG_DWORD /d 3 /f | Out-Null\r\n\
             Log '✔ Persistent High CPU & I/O Priority (IFEO PerfOptions) set for {}'\r\n",
            g, g, g
        ));
    }

    if ps.is_empty() {
        return vec!["No tweaks selected.".to_string()];
    }

    run_elevated_powershell(&ps, is_admin)
}

pub fn rollback_default_tweaks(adapter_name: &str, game_exe: &str, is_admin: bool) -> Vec<String> {
    let a = sanitize_ps_single_quoted(adapter_name);
    let g = sanitize_ps_single_quoted(game_exe);
    let ps = format!(
        "$ifaces = Get-ChildItem 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\Parameters\\Interfaces'\r\n\
         foreach ($iface in $ifaces) {{\r\n\
             Remove-ItemProperty -Path $iface.PSPath -Name 'TcpAckFrequency' -ErrorAction SilentlyContinue\r\n\
             Remove-ItemProperty -Path $iface.PSPath -Name 'TCPNoDelay' -ErrorAction SilentlyContinue\r\n\
             Remove-ItemProperty -Path $iface.PSPath -Name 'TcpDelAckTicks' -ErrorAction SilentlyContinue\r\n\
         }}\r\n\
         reg add 'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile' /v NetworkThrottlingIndex /t REG_DWORD /d 10 /f | Out-Null\r\n\
         reg add 'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile' /v SystemResponsiveness /t REG_DWORD /d 20 /f | Out-Null\r\n\
         reg delete 'HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\Psched' /v NonBestEffortLimit /f 2>$null | Out-Null\r\n\
         Remove-NetQosPolicy -Name 'WPing_Game_QoS' -Confirm:$false -ErrorAction SilentlyContinue\r\n\
         if ('{}' -ne '') {{ Set-NetIPInterface -InterfaceAlias '{}' -AutomaticMetric Enabled -ErrorAction SilentlyContinue }}\r\n\
         Get-NetAdapter | Where-Object {{ $_.Status -eq 'Up' }} | ForEach-Object {{\r\n\
             Set-NetAdapterAdvancedProperty -Name $_.Name -RegistryKeyword '*InterruptModeration' -RegistryValue 1 -ErrorAction SilentlyContinue\r\n\
             Enable-NetAdapterPowerManagement -Name $_.Name -ErrorAction SilentlyContinue\r\n\
         }}\r\n\
         reg add 'HKCU\\System\\GameConfigStore' /v GameDVR_Enabled /t REG_DWORD /d 1 /f | Out-Null\r\n\
         reg delete 'HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\GameDVR' /v AllowGameDVR /f 2>$null | Out-Null\r\n\
         reg delete 'HKLM\\SYSTEM\\CurrentControlSet\\Control\\Power\\PowerThrottling' /v PowerThrottlingOff /f 2>$null | Out-Null\r\n\
         reg add 'HKLM\\SYSTEM\\CurrentControlSet\\Control\\PriorityControl' /v Win32PrioritySeparation /t REG_DWORD /d 2 /f | Out-Null\r\n\
         reg delete 'HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\kernel' /v GlobalTimerResolutionRequests /f 2>$null | Out-Null\r\n\
         if ('{}' -ne '') {{ reg delete 'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Image File Execution Options\\{}\\PerfOptions' /f 2>$null | Out-Null }}\r\n\
         Log '↩ All Windows Network, NIC, QoS, GameDVR & Kernel settings restored to defaults.'\r\n",
        a, a, g, g
    );
    run_elevated_powershell(&ps, is_admin)
}
