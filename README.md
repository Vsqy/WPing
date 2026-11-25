# ⚡ WPing v2.2 — Fortnite Pro, CS2, Latency & FPS Optimizer
> **Created by Vsqy**

An ultra-fast, lightweight network routing, kernel latency, and FPS optimizer for competitive online gaming, built natively with **Rust** (~14 MB RAM, 0% CPU overhead).

---

## 🌟 Key Features

### 1. 🎮 Native Kernel ICMP Telemetry & All-in-One Game Boost
* **Zero Process Overhead Telemetry:** Uses native Windows `iphlpapi.dll` (`IcmpSendEcho2Ex`) bound directly to your physical NIC IPv4 — zero background `ping.exe` spawning, fractional sub-millisecond ping accuracy, real **Jitter**, and real **Packet Loss %**.
* **Intelligent Game Auto-Detection:** Automatically detects 19+ competitive titles (**CS2, Fortnite, Valorant, Dota 2, Apex Legends, PUBG, Escape from Tarkov, Overwatch 2, Rust, Warzone, Marvel Rivals, The Finals**, etc.).
* **One-Click Full Boost Session:** Simultaneously engages 0.5ms timer resolution, FilterKeys 0 Delay, Ultimate Performance power switching, Standby RAM cleanup, and background CPU priority balancing — cleanly restoring everything when stopped or exited.

### 2. ⚡ Input Delay, 0.500 ms Kernel Timer & FPS Booster
* **0.500 ms High-Precision Kernel Timer (`NtSetTimerResolution` & `timeBeginPeriod`):** Elevates Windows kernel scheduling from 64 Hz (15.6 ms) to **2,000 Hz (0.500 ms)**, paired with Windows 11 `GlobalTimerResolutionRequests = 1` support.
* **FilterKeys 0 Keyboard Delay & 1:1 Raw Mouse:** Eliminates Windows repeat/debounce latency (`Fortnite Pro 150/25`, `Ultra Fast 120/20`, live typing test box) plus 1-click Mouse Acceleration disable (`1:1 Raw Aim`).
* **Standby RAM Purge & Working Set Cleaner:** Empties idle background browser bloat (`Chrome`, `Edge`, `Discord`, `SteamWebHelper`) and purges Windows Standby Page List (`NtSetSystemInformation`) to prevent 1% Low FPS stutters.
* **Ultimate Performance Power Switching & CPU Balancer:** Automatically activates Ultimate/High Performance while gaming and throttles heavy background updaters/browsers (`Below Normal`) without hurting Discord audio.

### 3. 🌐 Smart Network, Gaming DNS Benchmark & Path MTU Finder
* **Verified Game Relay Monitoring:** Probes Valve Relays (Warsaw, Frankfurt, Vienna, Stockholm), Fortnite/AWS EU nodes, and Cloudflare/AWS Anycast every 10 seconds natively.
* **1-Click Gaming DNS Benchmark & Switcher:** Benchmarks live RTT to Cloudflare (`1.1.1.1`), Google (`8.8.8.8`), Quad9 (`9.9.9.9`), OpenDNS, and AdGuard, applying the fastest DNS directly to your physical NIC (with 1-click DHCP reset).
* **Optimal Path MTU Discovery:** Runs binary search ICMP (`IP_FLAG_DF`) probes to find the exact highest non-fragmented MTU (`1500` Ethernet vs `1492` PPPoE) and applies it persistently.

### 4. ⚙️ Reversible Windows Registry, NIC Driver & QoS Tweaks
* **Network & NIC Hardware Tuning:** Nagle Disable (`TCPNoDelay=1`, `TcpAckFrequency=1`, `TcpDelAckTicks=0`), `NetworkThrottlingIndex=0xffffffff`, `NonBestEffortLimit=0` (0% Windows Update bandwidth reserve), per-game **QoS DSCP EF (46)** tagging, NIC `*InterruptModeration=0`, `*EEE=0`, `*FlowControl=0`, RSS Enabled, and Physical Adapter Metric=5 vs VPN Metric=50.
* **FPS & GPU Latency:** Disables GameDVR/Xbox Background Capture, sets `PowerThrottlingOff=1`, Hardware-Accelerated GPU Scheduling (`HwSchMode=2`), `Win32PrioritySeparation=0x26`, and persistent High CPU/I-O game priority (`IFEO PerfOptions`).
* **Full 1-Click Rollback:** Restore Windows defaults anytime with one click.

### 5. 📊 Native Bufferbloat & Jitter Diagnostics
* Run standard 10-packet precision checks or 30-packet rapid burst tests (50ms intervals) against any IPv4 or domain hostname to diagnose ISP bufferbloat and jitter spikes.

### 6. 🛡️ 100% Anti-Cheat Safe & Trilingual UI
* Completely safe for **VAC, Riot Vanguard, Easy Anti-Cheat (EAC), and BattlEye** — strictly uses documented Windows APIs and system configuration without game memory injection.
* Auto-detects Windows locale with seamless switching between **Ukrainian (🇺🇦 UA)**, **English (🇬🇧 EN)**, and **Russian (🇷🇺 RU)**.

---

## 🚀 Quick Start

1. Download **`WPing_Setup.exe`** from [Releases](https://github.com/Vsqy/WPing/releases/latest).
2. Run the installer as Administrator (automatically adapts your NIC & applies Windows tweaks).
3. Launch **WPing**, choose your game, and click **🚀 ENABLE FULL BOOST**!
