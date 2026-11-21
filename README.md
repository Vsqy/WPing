# ⚡ WPing — Game Latency & Route Optimizer
> **Created by Vsqy**

An ultra-fast, lightweight network routing and packet queue optimizer for competitive online gaming, built natively with **Rust** (memory footprint ~14 MB RAM, 0% CPU overhead).

---

## 🌟 Key Features

### 1. 🎮 Real-Time Game Optimizer & Auto-Detection
* **Intelligent Auto-Detection:** Automatically detects when games like **Counter-Strike 2, Valorant, Fortnite, Dota 2, Apex Legends, PUBG**, etc. are launched.
* **Instant Boost:** Prioritizes network packet queues, eliminates buffering, and routes gaming traffic through the optimal relay path.
* **Live Network Telemetry:** Real-time metrics for RTT (Ping), Jitter, Packet Loss, and a visual latency stability graph.

### 2. ⚙️ Windows TCP/IP & Kernel Optimizations
* **Disable Nagle's Algorithm (`TCP_NODELAY = 1`, `TcpAckFrequency = 1`):** Eliminates packet accumulation and buffering delays, resulting in instantaneous hit registration (HitReg).
* **Network Throttling Removal (`NetworkThrottlingIndex = 0xffffffff`):** Prevents Windows Multimedia background services from throttling network traffic.
* **Maximum System Responsiveness (`SystemResponsiveness = 0`):** Allocates 100% of network scheduling resources to competitive gaming.
* **Route Metric Prioritization:** Enforces high priority (Metric 5) on the physical network card (Ethernet/Wi-Fi), preventing virtual VPNs (Radmin VPN, Hamachi, etc.) from hijacking default routing.
* **Receive Side Scaling (RSS) & Offload:** Distributes packet processing across all CPU cores and disables network card power saving.

### 3. 🌐 Cloud Anycast Relay Monitoring
* **10-Second Live Monitoring:** Continuously probes leading cloud game relays (Cloudflare Anycast, Valve Game Relays in Warsaw & Frankfurt, Oracle Cloud, AWS Global Accelerator).
* **Smart Routing:** Automatically selects the lowest-latency node based on live ping from your physical network connection.
* **Physical Adapter Binding:** All measurements bind directly to your physical network interface via source IP (`ping -S <IP>`).

### 4. 🌍 Multilingual Interface
* **Automatic System Language Detection:** Automatically starts in Ukrainian, Russian, or English matching your Windows OS locale, with a seamless manual language switcher (🇺🇦 UA / 🇬🇧 EN / 🇷🇺 RU).

### 5. 🛡️ 100% Anti-Cheat Safe
* Completely safe for **Valve Anti-Cheat (VAC), Riot Vanguard, EasyAntiCheat (EAC), and BattlEye**.
* Does not modify game binaries, inject code, or access game memory — strictly tunes the Windows TCP/IP network stack.

---

## 🚀 Quick Start

1. Download **`WPing_Setup.exe`** from [Releases](https://github.com/Vsqy/WPing/releases).
2. Run the installer as Administrator.
3. Launch **WPing**, select your game, and click **🚀 ENABLE BOOST** (or toggle **Auto-Boost**).
