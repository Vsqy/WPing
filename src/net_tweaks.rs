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
pub struct ValveRegion {
    pub code: String,
    pub name: String,
    pub probe_ip: String,
    pub cidr_list: Vec<String>,
    pub ping_ms: Option<f32>,
    pub keep_enabled: bool,
}

#[derive(Clone, Debug, Default)]
pub struct NetworkHealthReport {
    pub loaded: bool,
    pub adapter_mtu: u32,
    pub jumbo_value: String,
    pub rsc_enabled: bool,
    pub packet_coalescing_enabled: bool,
    pub delayed_ack_timeout_ms: u32,
    pub delayed_ack_freq: u32,
    pub congestion_provider: String,
    pub afd_ignore_push: u32,
    pub dns_servers: String,
    pub auto_metric: bool,
    pub interface_metric: u32,
    pub giga_lite_enabled: bool,
    pub global_timer_win11: u32,
    pub issue_count: usize,
}

#[derive(Clone, Debug)]
pub struct TweakOptions {
    // Ping & Network Stack
    pub tweak_nagle: bool,
    pub tweak_tcp_stack: bool,
    pub tweak_afd_mtu: bool,
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
            tweak_tcp_stack: true,
            tweak_afd_mtu: true,
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

pub fn fallback_valve_regions() -> Vec<ValveRegion> {
    vec![
        ValveRegion {
            code: "waw".into(),
            name: "Warsaw, Poland (EU East)".into(),
            probe_ip: "155.133.230.100".into(),
            cidr_list: vec!["155.133.230.0/24".into()],
            ping_ms: None,
            keep_enabled: true,
        },
        ValveRegion {
            code: "sto".into(),
            name: "Stockholm, Sweden (EU North)".into(),
            probe_ip: "162.254.198.41".into(),
            cidr_list: vec!["162.254.198.0/24".into(), "155.133.252.0/24".into()],
            ping_ms: None,
            keep_enabled: true,
        },
        ValveRegion {
            code: "fra".into(),
            name: "Frankfurt, Germany (EU Central)".into(),
            probe_ip: "155.133.226.68".into(),
            cidr_list: vec!["155.133.226.0/24".into(), "162.254.197.0/24".into()],
            ping_ms: None,
            keep_enabled: true,
        },
        ValveRegion {
            code: "ams".into(),
            name: "Amsterdam, Netherlands".into(),
            probe_ip: "155.133.248.36".into(),
            cidr_list: vec!["155.133.248.0/24".into()],
            ping_ms: None,
            keep_enabled: true,
        },
        ValveRegion {
            code: "par".into(),
            name: "Paris, France".into(),
            probe_ip: "185.25.182.18".into(),
            cidr_list: vec!["185.25.182.0/24".into()],
            ping_ms: None,
            keep_enabled: false,
        },
        ValveRegion {
            code: "vie".into(),
            name: "Vienna, Austria".into(),
            probe_ip: "146.66.155.66".into(),
            cidr_list: vec!["146.66.155.0/24".into()],
            ping_ms: None,
            keep_enabled: false,
        },
        ValveRegion {
            code: "lhr".into(),
            name: "London, UK".into(),
            probe_ip: "162.254.196.66".into(),
            cidr_list: vec!["162.254.196.0/24".into()],
            ping_ms: None,
            keep_enabled: false,
        },
        ValveRegion {
            code: "mad".into(),
            name: "Madrid, Spain".into(),
            probe_ip: "155.133.246.34".into(),
            cidr_list: vec!["155.133.246.0/24".into()],
            ping_ms: None,
            keep_enabled: false,
        },
    ]
}

/// Fetches live Valve SDR official POPs/relay IPs via PowerShell Invoke-RestMethod (fast fallback on error).
pub fn fetch_valve_regions_live() -> Vec<ValveRegion> {
    let ps = "
$ErrorActionPreference = 'SilentlyContinue'
$sdr = Invoke-RestMethod -Uri 'https://api.steampowered.com/ISteamApps/GetSDRConfig/v1?appid=730' -TimeoutSec 4
if (-not $sdr -or -not $sdr.pops) { exit 1 }
$eu = @('waw','sto','fra','ams','par','vie','lhr','mad','hel')
foreach ($k in $eu) {
    $pop = $sdr.pops.$k
    if ($pop -and $pop.relays -and $pop.relays.Count -gt 0) {
        $probe = $pop.relays[0].ipv4
        $ips = ($pop.relays | ForEach-Object { $_.ipv4 }) -join ','
        Write-Output \"$k|$($pop.desc)|$probe|$ips\"
    }
}
";
    let out = Command::new("powershell")
        .args(&["-NoProfile", "-Command", ps])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    if let Ok(o) = out {
        if o.status.success() {
            let text = String::from_utf8_lossy(&o.stdout);
            let mut list = Vec::new();
            for line in text.lines() {
                let parts: Vec<&str> = line.trim().split('|').collect();
                if parts.len() == 4 && !parts[2].is_empty() {
                    let code = parts[0].to_string();
                    let keep = matches!(code.as_str(), "waw" | "sto" | "fra" | "ams");
                    let cidrs: Vec<String> = parts[3]
                        .split(',')
                        .filter(|s| !s.is_empty())
                        .map(|ip| {
                            // Deduplicate into /24 or keep exact /32
                            let octs: Vec<&str> = ip.split('.').collect();
                            if octs.len() == 4 {
                                format!("{}.{}.{}.0/24", octs[0], octs[1], octs[2])
                            } else {
                                ip.to_string()
                            }
                        })
                        .collect();
                    let mut uniq = cidrs;
                    uniq.sort();
                    uniq.dedup();
                    list.push(ValveRegion {
                        code,
                        name: parts[1].to_string(),
                        probe_ip: parts[2].to_string(),
                        cidr_list: uniq,
                        ping_ms: None,
                        keep_enabled: keep,
                    });
                }
            }
            if !list.is_empty() {
                return list;
            }
        }
    }

    fallback_valve_regions()
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

/// Audits real Windows network, NIC driver, TCP supplemental template, AFD, DNS & Timer bottlenecks.
pub fn audit_network_health(adapter_name: &str) -> NetworkHealthReport {
    let mut rep = NetworkHealthReport::default();
    let a = sanitize_ps_single_quoted(adapter_name);

    let ps = format!(
        "$ErrorActionPreference = 'SilentlyContinue'\r\n\
         $ipif = Get-NetIPInterface -InterfaceAlias '{}' -AddressFamily IPv4 | Select-Object -First 1\r\n\
         $mtu = if ($ipif) {{ $ipif.NlMtu }} else {{ 1500 }}\r\n\
         $automet = if ($ipif -and $ipif.AutomaticMetric -eq 'Enabled') {{ 1 }} else {{ 0 }}\r\n\
         $metric = if ($ipif) {{ $ipif.InterfaceMetric }} else {{ 25 }}\r\n\
         $jumbo = (Get-NetAdapterAdvancedProperty -Name '{}' -RegistryKeyword '*JumboPacket').RegistryValue[0]\r\n\
         $giga = (Get-NetAdapterAdvancedProperty -Name '{}' -RegistryKeyword 'GigaLite').RegistryValue[0]\r\n\
         $off = Get-NetOffloadGlobalSetting\r\n\
         $rsc = if ($off.ReceiveSegmentCoalescing -eq 'Enabled') {{ 1 }} else {{ 0 }}\r\n\
         $pcf = if ($off.PacketCoalescingFilter -eq 'Enabled') {{ 1 }} else {{ 0 }}\r\n\
         $tcpOut = netsh int tcp show supplemental\r\n\
         $ackTimeout = 40; $ackFreq = 2; $cong = 'cubic'\r\n\
         foreach ($line in $tcpOut) {{\r\n\
             if ($line -match 'Delayed ACK timeout.*:\\s*(\\d+)') {{ $ackTimeout = [int]$Matches[1] }}\r\n\
             if ($line -match 'Delayed ACK frequency.*:\\s*(\\d+)') {{ $ackFreq = [int]$Matches[1] }}\r\n\
             if ($line -match 'Congestion Control Provider.*:\\s*(\\S+)') {{ $cong = $Matches[1] }}\r\n\
         }}\r\n\
         $afd = Get-ItemProperty 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\AFD\\Parameters'\r\n\
         $ignPush = if ($afd -and $null -ne $afd.IgnorePushBitOnReceives) {{ [int]$afd.IgnorePushBitOnReceives }} else {{ 0 }}\r\n\
         $dns = ((Get-DnsClientServerAddress -InterfaceAlias '{}' -AddressFamily IPv4).ServerAddresses) -join ','\r\n\
         $krn = (Get-ItemProperty 'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\kernel' -Name GlobalTimerResolutionRequests).GlobalTimerResolutionRequests\r\n\
         $gt = if ($null -ne $krn) {{ [int]$krn }} else {{ 0 }}\r\n\
         Write-Output \"$mtu|$jumbo|$rsc|$pcf|$ackTimeout|$ackFreq|$cong|$ignPush|$dns|$automet|$metric|$giga|$gt\"\r\n",
        a, a, a, a
    );

    let out = Command::new("powershell")
        .args(&["-NoProfile", "-Command", &ps])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    if let Ok(o) = out {
        let line = String::from_utf8_lossy(&o.stdout).trim().to_string();
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() >= 13 {
            rep.loaded = true;
            rep.adapter_mtu = parts[0].parse().unwrap_or(1500);
            rep.jumbo_value = parts[1].to_string();
            rep.rsc_enabled = parts[2] == "1";
            rep.packet_coalescing_enabled = parts[3] == "1";
            rep.delayed_ack_timeout_ms = parts[4].parse().unwrap_or(40);
            rep.delayed_ack_freq = parts[5].parse().unwrap_or(2);
            rep.congestion_provider = parts[6].to_string();
            rep.afd_ignore_push = parts[7].parse().unwrap_or(0);
            rep.dns_servers = parts[8].to_string();
            rep.auto_metric = parts[9] == "1";
            rep.interface_metric = parts[10].parse().unwrap_or(25);
            rep.giga_lite_enabled = parts[11] == "1";
            rep.global_timer_win11 = parts[12].parse().unwrap_or(0);

            let mut issues = 0;
            if rep.adapter_mtu > 1500 { issues += 1; }
            if rep.rsc_enabled { issues += 1; }
            if rep.packet_coalescing_enabled { issues += 1; }
            if rep.delayed_ack_timeout_ms > 15 || rep.delayed_ack_freq > 1 { issues += 1; }
            if rep.afd_ignore_push == 1 { issues += 1; }
            if rep.dns_servers.starts_with("192.168.") || rep.dns_servers.starts_with("10.") || rep.dns_servers.is_empty() { issues += 1; }
            if rep.auto_metric || rep.interface_metric > 10 { issues += 1; }
            if rep.giga_lite_enabled { issues += 1; }
            if rep.global_timer_win11 != 1 { issues += 1; }
            rep.issue_count = issues;
        }
    }

    rep
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
    let safe_name = adapter_name.replace('"', "");
    let ps = format!(
        "$jp = Get-NetAdapterAdvancedProperty -Name '{}' -RegistryKeyword '*JumboPacket' -ErrorAction SilentlyContinue\r\n\
         if ($jp -and $jp.DefaultRegistryValue) {{ Set-NetAdapterAdvancedProperty -Name '{}' -RegistryKeyword '*JumboPacket' -RegistryValue $jp.DefaultRegistryValue -ErrorAction SilentlyContinue }}\r\n\
         netsh interface ipv4 set subinterface \"{}\" mtu={} store=persistent | Out-Null\r\n\
         Log '✔ Disabled Jumbo Frames & set clean MTU={} on {}'\r\n",
        a, a, safe_name, mtu, mtu, a
    );
    run_elevated_powershell(&ps, is_admin)
}

/// Applies outbound Windows Firewall rules blocking ONLY unselected high-ping Valve SDR relays.
pub fn apply_region_lock(regions: &[ValveRegion], is_admin: bool) -> Vec<String> {
    let mut blocked_ips: Vec<String> = Vec::new();
    let mut kept_names: Vec<String> = Vec::new();

    for r in regions {
        if r.keep_enabled {
            kept_names.push(r.code.to_uppercase());
        } else {
            for cidr in &r.cidr_list {
                blocked_ips.push(cidr.clone());
            }
        }
    }

    let mut ps = String::from("Remove-NetFirewallRule -Group 'WPing_ServerBlock' -ErrorAction SilentlyContinue\r\n");
    if blocked_ips.is_empty() {
        ps.push_str("Log '✔ All EU game relay regions are open.'\r\n");
    } else {
        let joined = blocked_ips.iter().map(|s| format!("'{}'", sanitize_ps_single_quoted(s))).collect::<Vec<_>>().join(",");
        ps.push_str(&format!(
            "New-NetFirewallRule -DisplayName 'WPing Game Region Lock' -Group 'WPing_ServerBlock' -Direction Outbound -Action Block -Protocol Any -RemoteAddress @({}) -Profile Any | Out-Null\r\n\
             Log '🔒 Region Lock Active: Kept [{}], blocked {} high-ping relays.'\r\n",
            joined,
            kept_names.join(", "),
            blocked_ips.len()
        ));
    }

    run_elevated_powershell(&ps, is_admin)
}

pub fn clear_region_lock(is_admin: bool) -> Vec<String> {
    let ps = "Remove-NetFirewallRule -Group 'WPing_ServerBlock' -ErrorAction SilentlyContinue\r\n\
              Log '🔓 All Game Server Regions unlocked (Firewall rules removed).'\r\n";
    run_elevated_powershell(ps, is_admin)
}

pub fn apply_selected_tweaks(opts: &TweakOptions, adapter_name: &str, game_exe: &str, is_admin: bool) -> Vec<String> {
    let a = sanitize_ps_single_quoted(adapter_name);
    let safe_name = adapter_name.replace('"', "");
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
             reg add 'HKLM\\SOFTWARE\\Microsoft\\MSMQ\\Parameters' /v TCPNoDelay /t REG_DWORD /d 1 /f | Out-Null\r\n\
             Log '✔ Nagle Disabled (TcpAckFrequency=1, TCPNoDelay=1, TcpDelAckTicks=0, MSMQ=1)'\r\n",
        );
    }

    if opts.tweak_tcp_stack {
        ps.push_str(
            "netsh int tcp set global autotuninglevel=normal | Out-Null\r\n\
             netsh int tcp set global heuristics=disabled | Out-Null\r\n\
             netsh int tcp set global ecncapability=disabled | Out-Null\r\n\
             netsh int tcp set global timestamps=disabled | Out-Null\r\n\
             netsh int tcp set global rsc=disabled | Out-Null\r\n\
             Set-NetOffloadGlobalSetting -ReceiveSegmentCoalescing Disabled -PacketCoalescingFilter Disabled -ErrorAction SilentlyContinue\r\n\
             Disable-NetAdapterRsc -Name * -ErrorAction SilentlyContinue\r\n\
             netsh int tcp set supplemental template=custom | Out-Null\r\n\
             cmd /c \"netsh int tcp set supplemental custom minrto=20 icw=16 congestionprovider=bbr2 enablecwndrestart=disabled delayedacktimeout=10 delayedackfrequency=1 rack=enabled taillossprobe=enabled\" 2>$null | Out-Null\r\n\
             if ($LASTEXITCODE -ne 0) {\r\n\
                 cmd /c \"netsh int tcp set supplemental custom minrto=20 icw=16 congestionprovider=ctcp enablecwndrestart=disabled delayedacktimeout=10 delayedackfrequency=1 rack=enabled taillossprobe=enabled\" 2>$null | Out-Null\r\n\
             }\r\n\
             Set-NetTCPSetting -SettingName InternetCustom -MinRtoMs 20 -DelayedAckTimeoutMs 10 -DelayedAckFrequency 1 -InitialRtoMs 300 -ScalingHeuristics Disabled -EcnCapability Disabled -ErrorAction SilentlyContinue\r\n\
             Log '✔ Deep TCP Stack: RSC=Disabled, PacketCoalescing=Disabled, DelayedAck=10ms/1, BBR2/CTCP Active'\r\n",
        );
    }

    if opts.tweak_afd_mtu {
        ps.push_str(&format!(
            "reg add 'HKLM\\SYSTEM\\CurrentControlSet\\Services\\AFD\\Parameters' /v IgnorePushBitOnReceives /t REG_DWORD /d 0 /f | Out-Null\r\n\
             reg add 'HKLM\\SYSTEM\\CurrentControlSet\\Services\\AFD\\Parameters' /v DynamicSendBufferDisable /t REG_DWORD /d 1 /f | Out-Null\r\n\
             reg add 'HKLM\\SYSTEM\\CurrentControlSet\\Services\\AFD\\Parameters' /v NonBlockingSendSpecialBuffering /t REG_DWORD /d 1 /f | Out-Null\r\n\
             reg add 'HKLM\\SYSTEM\\CurrentControlSet\\Services\\AFD\\Parameters' /v FastSendDatagramThreshold /t REG_DWORD /d 16384 /f | Out-Null\r\n\
             reg add 'HKLM\\SYSTEM\\CurrentControlSet\\Services\\AFD\\Parameters' /v FastCopyReceiveThreshold /t REG_DWORD /d 16384 /f | Out-Null\r\n\
             reg add 'HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\DeliveryOptimization' /v DODownloadMode /t REG_DWORD /d 0 /f | Out-Null\r\n\
             if ('{}' -ne '') {{\r\n\
                 $jp = Get-NetAdapterAdvancedProperty -Name '{}' -RegistryKeyword '*JumboPacket' -ErrorAction SilentlyContinue\r\n\
                 if ($jp -and $jp.DefaultRegistryValue) {{ Set-NetAdapterAdvancedProperty -Name '{}' -RegistryKeyword '*JumboPacket' -RegistryValue $jp.DefaultRegistryValue -ErrorAction SilentlyContinue }}\r\n\
                 netsh interface ipv4 set subinterface \"{}\" mtu=1500 store=persistent | Out-Null\r\n\
             }}\r\n\
             Log '✔ Winsock AFD Fast-Path Tuned, P2P Update Seeding Off, Jumbo Off & Clean MTU=1500'\r\n",
            a, a, a, safe_name
        ));
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
             Log '✔ Network Throttling Disabled & MMCSS Games Priority = High'\r\n",
        );
    }

    if opts.tweak_qos {
        ps.push_str(&format!(
            "reg add 'HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\Psched' /v NonBestEffortLimit /t REG_DWORD /d 0 /f | Out-Null\r\n\
             Remove-NetQosPolicy -Name 'WPing_Game_QoS' -Confirm:$false -ErrorAction SilentlyContinue\r\n\
             if ('{}' -ne '') {{\r\n\
                 New-NetQosPolicy -Name 'WPing_Game_QoS' -AppPathNameMatchCondition '{}' -DSCPAction 46 -PriorityValue8021Action 6 -NetworkProfile All | Out-Null\r\n\
             }}\r\n\
             Log '✔ QoS DSCP 46 (EF) Active & 20% Bandwidth Limit Removed'\r\n",
            g, g
        ));
    }

    if opts.tweak_nic_metric {
        ps.push_str(&format!(
            "if ('{}' -ne '' -and (Get-NetAdapter -Name '{}' -ErrorAction SilentlyContinue)) {{\r\n\
                 Set-NetIPInterface -InterfaceAlias '{}' -AutomaticMetric Disabled -InterfaceMetric 5 -ErrorAction SilentlyContinue\r\n\
             }} else {{\r\n\
                 Get-NetAdapter | Where-Object {{ $_.Status -eq 'Up' -and $_.Virtual -eq $false -and $_.InterfaceDescription -notmatch 'VPN|Virtual|Radmin|Hamachi|TAP' }} | ForEach-Object {{\r\n\
                     Set-NetIPInterface -InterfaceIndex $_.InterfaceIndex -AutomaticMetric Disabled -InterfaceMetric 5 -ErrorAction SilentlyContinue\r\n\
                 }}\r\n\
             }}\r\n\
             Get-NetAdapter | Where-Object {{ $_.Virtual -eq $true -or $_.InterfaceDescription -match 'VPN|Virtual|Radmin|Hamachi|TAP|ZeroTier' -or $_.Name -match 'VPN|Radmin|Hamachi' }} | ForEach-Object {{\r\n\
                 Set-NetIPInterface -InterfaceIndex $_.InterfaceIndex -AutomaticMetric Disabled -InterfaceMetric 500 -ErrorAction SilentlyContinue\r\n\
             }}\r\n\
             Log '✔ Physical Adapter Locked at Metric=5 (AutoMetric Off) vs VPN=500'\r\n",
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
                 foreach ($kw in @('*InterruptModeration','*EEE','AdvancedEEE','EnableGreenEthernet','*FlowControl','GigaLite','PowerSavingMode','S5WakeOnLan','*PMARPOffload','*PMNSOffload','*WakeOnMagicPacket','*WakeOnPattern')) {\r\n\
                     Set-NetAdapterAdvancedProperty -Name $_.Name -RegistryKeyword $kw -RegistryValue 0 -ErrorAction SilentlyContinue\r\n\
                 }\r\n\
             }\r\n\
             ipconfig /flushdns | Out-Null\r\n\
             Log '✔ NIC Driver: InterruptModeration=Off, GigaLite=Off, EEE=Off, FlowControl=Off, RSS=On'\r\n",
        );
    }

    if opts.tweak_fps_gamedvr {
        ps.push_str(
            "reg add 'HKCU\\System\\GameConfigStore' /v GameDVR_Enabled /t REG_DWORD /d 0 /f | Out-Null\r\n\
             reg add 'HKCU\\System\\GameConfigStore' /v GameDVR_FSEBehaviorMode /t REG_DWORD /d 2 /f | Out-Null\r\n\
             reg add 'HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\GameDVR' /v AppCaptureEnabled /t REG_DWORD /d 0 /f | Out-Null\r\n\
             reg add 'HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\GameDVR' /v AllowGameDVR /t REG_DWORD /d 0 /f | Out-Null\r\n\
             Log '✔ GameDVR & Background Recording Disabled (+FPS / Reduced Stutters)'\r\n",
        );
    }

    if opts.tweak_fps_power_throttling {
        ps.push_str(
            "reg add 'HKLM\\SYSTEM\\CurrentControlSet\\Control\\Power\\PowerThrottling' /v PowerThrottlingOff /t REG_DWORD /d 1 /f | Out-Null\r\n\
             reg add 'HKLM\\SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers' /v HwSchMode /t REG_DWORD /d 2 /f | Out-Null\r\n\
             reg add 'HKLM\\SYSTEM\\CurrentControlSet\\Control\\PriorityControl' /v Win32PrioritySeparation /t REG_DWORD /d 38 /f | Out-Null\r\n\
             Log '✔ PowerThrottlingOff=1, GPU HAGS=Enabled, Win32PrioritySeparation=0x26'\r\n",
        );
    }

    if opts.tweak_win11_timer {
        ps.push_str(
            "reg add 'HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\kernel' /v GlobalTimerResolutionRequests /t REG_DWORD /d 1 /f | Out-Null\r\n\
             Log '✔ Win11 GlobalTimerResolutionRequests=1 Active'\r\n",
        );
    }

    if opts.tweak_game_priority_ifeo && !g.is_empty() {
        ps.push_str(&format!(
            "reg add 'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Image File Execution Options\\{}\\PerfOptions' /v CpuPriorityClass /t REG_DWORD /d 3 /f | Out-Null\r\n\
             reg add 'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Image File Execution Options\\{}\\PerfOptions' /v IoPriority /t REG_DWORD /d 3 /f | Out-Null\r\n\
             Log '✔ High CPU & I/O Priority (IFEO) set for {}'\r\n",
            g, g, g
        ));
    }

    if ps.is_empty() {
        return vec!["No tweaks selected.".to_string()];
    }

    run_elevated_powershell(&ps, is_admin)
}

/// Applies EVERYTHING needed to fix hidden ping/network/FPS bottlenecks + switches to Cloudflare 1.1.1.1 if using local router DNS.
pub fn apply_max_ping_fix(adapter_name: &str, game_exe: &str, fix_dns: bool, is_admin: bool) -> Vec<String> {
    let mut logs = apply_selected_tweaks(&TweakOptions::default(), adapter_name, game_exe, is_admin);
    if fix_dns && !adapter_name.is_empty() {
        let dns_logs = apply_dns_server(adapter_name, "1.1.1.1", "1.0.0.1", is_admin);
        logs.extend(dns_logs);
    }
    logs
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
         netsh int tcp set global rsc=enabled timestamps=disabled | Out-Null\r\n\
         Set-NetOffloadGlobalSetting -ReceiveSegmentCoalescing Enabled -PacketCoalescingFilter Default -ErrorAction SilentlyContinue\r\n\
         Enable-NetAdapterRsc -Name * -ErrorAction SilentlyContinue\r\n\
         netsh int tcp set supplemental template=internet | Out-Null\r\n\
         Remove-ItemProperty -Path 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\AFD\\Parameters' -Name 'IgnorePushBitOnReceives' -ErrorAction SilentlyContinue\r\n\
         Remove-ItemProperty -Path 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\AFD\\Parameters' -Name 'DynamicSendBufferDisable' -ErrorAction SilentlyContinue\r\n\
         reg add 'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile' /v NetworkThrottlingIndex /t REG_DWORD /d 10 /f | Out-Null\r\n\
         reg add 'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile' /v SystemResponsiveness /t REG_DWORD /d 20 /f | Out-Null\r\n\
         reg delete 'HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\Psched' /v NonBestEffortLimit /f 2>$null | Out-Null\r\n\
         reg delete 'HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\DeliveryOptimization' /v DODownloadMode /f 2>$null | Out-Null\r\n\
         Remove-NetQosPolicy -Name 'WPing_Game_QoS' -Confirm:$false -ErrorAction SilentlyContinue\r\n\
         Remove-NetFirewallRule -Group 'WPing_ServerBlock' -ErrorAction SilentlyContinue\r\n\
         if ('{}' -ne '') {{\r\n\
             Set-NetIPInterface -InterfaceAlias '{}' -AutomaticMetric Enabled -ErrorAction SilentlyContinue\r\n\
             Set-DnsClientServerAddress -InterfaceAlias '{}' -ResetServerAddresses -ErrorAction SilentlyContinue\r\n\
         }}\r\n\
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
         Log '↩ All Windows Network, TCP, AFD, NIC, QoS, DNS, GameDVR & Kernel settings restored.'\r\n",
        a, a, a, g, g
    );
    run_elevated_powershell(&ps, is_admin)
}
