@echo off
:: BatchGotAdmin
:-------------------------------------
REM --> Check for permissions
IF "%PROCESSOR_ARCHITECTURE%" EQU "amd64" (
>nul 2>&1 "%SYSTEMROOT%\SysWOW64\cacls.exe" "%SYSTEMROOT%\SysWOW64\config\system"
) ELSE (
>nul 2>&1 "%SYSTEMROOT%\system32\cacls.exe" "%SYSTEMROOT%\system32\config\system"
)

REM --> If error flag set, we do not have admin.
if '%errorlevel%' NEQ '0' (
    echo [WPing] Requesting Administrator Privileges...
    goto UACPrompt
) else ( goto gotAdmin )

:UACPrompt
    echo Set UAC = CreateObject^("Shell.Application"^) > "%temp%\getadmin.vbs"
    set params = %*:"=""
    echo UAC.ShellExecute "cmd.exe", "/c ""%~s0"" %params%", "", "runas", 1 >> "%temp%\getadmin.vbs"

    "%temp%\getadmin.vbs"
    del "%temp%\getadmin.vbs"
    exit /B

:gotAdmin
    pushd "%CD%"
    CD /D "%~dp0"
:--------------------------------------

echo ========================================================
echo   WPing v2.0 - Ultimate Ping, FPS ^& Kernel Optimizer
echo   Created by Vsqy
echo ========================================================
echo.

echo [*] 1. Disabling Network Throttling, Reserved Bandwidth ^& Tuning MMCSS...
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile" /v NetworkThrottlingIndex /t REG_DWORD /d 0xffffffff /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile" /v SystemResponsiveness /t REG_DWORD /d 0 /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games" /v "GPU Priority" /t REG_DWORD /d 8 /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games" /v "Priority" /t REG_DWORD /d 6 /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games" /v "Scheduling Category" /t REG_SZ /d "High" /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games" /v "SFIO Priority" /t REG_SZ /d "High" /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games" /v "Latency Sensitive" /t REG_SZ /d "True" /f >nul
reg add "HKLM\SOFTWARE\Policies\Microsoft\Windows\Psched" /v NonBestEffortLimit /t REG_DWORD /d 0 /f >nul

echo [*] 2. Disabling Nagle's Algorithm (TCP_NODELAY ^& Instant ACK)...
powershell -NoProfile -Command "
$interfaces = Get-ChildItem 'HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces';
foreach ($iface in $interfaces) {
    Set-ItemProperty -Path $iface.PSPath -Name 'TcpAckFrequency' -Value 1 -Type DWord -Force -ErrorAction SilentlyContinue;
    Set-ItemProperty -Path $iface.PSPath -Name 'TCPNoDelay' -Value 1 -Type DWord -Force -ErrorAction SilentlyContinue;
    Set-ItemProperty -Path $iface.PSPath -Name 'TcpDelAckTicks' -Value 0 -Type DWord -Force -ErrorAction SilentlyContinue;
}
"

echo [*] 3. Optimizing Global TCP Stack ^& NIC Hardware Driver (InterruptModeration=Off)...
netsh int tcp set global autotuninglevel=normal >nul
netsh int tcp set global rss=enabled >nul
netsh int tcp set heuristics disabled >nul
netsh int tcp set global ecncapability=disabled >nul
netsh int ip set global taskoffload=enabled >nul
powershell -NoProfile -Command "
Get-NetAdapter | Where-Object { $_.Status -eq 'Up' -and $_.Virtual -eq $false } | ForEach-Object {
    Enable-NetAdapterRss -Name $_.Name -ErrorAction SilentlyContinue;
    Disable-NetAdapterPowerManagement -Name $_.Name -ErrorAction SilentlyContinue;
    Set-NetAdapterAdvancedProperty -Name $_.Name -RegistryKeyword '*InterruptModeration' -RegistryValue 0 -ErrorAction SilentlyContinue;
    Set-NetAdapterAdvancedProperty -Name $_.Name -RegistryKeyword '*EEE' -RegistryValue 0 -ErrorAction SilentlyContinue;
    Set-NetAdapterAdvancedProperty -Name $_.Name -RegistryKeyword 'GreenEthernet' -RegistryValue 0 -ErrorAction SilentlyContinue;
    Set-NetAdapterAdvancedProperty -Name $_.Name -RegistryKeyword '*FlowControl' -RegistryValue 0 -ErrorAction SilentlyContinue;
}
"

echo [*] 4. Prioritizing Physical Adapter Route Metric (Preventing VPN hijacking)...
powershell -NoProfile -Command "
$chosen = '%~1';
if ($chosen -and (Get-NetAdapter -Name $chosen -ErrorAction SilentlyContinue)) {
    Set-NetIPInterface -InterfaceAlias $chosen -InterfaceMetric 5 -ErrorAction SilentlyContinue;
} else {
    Get-NetAdapter | Where-Object { $_.Status -eq 'Up' -and $_.Virtual -eq $false -and $_.InterfaceDescription -notmatch 'VPN|Virtual|Radmin|Hamachi|TAP' } | ForEach-Object {
        Set-NetIPInterface -InterfaceIndex $_.InterfaceIndex -InterfaceMetric 5 -ErrorAction SilentlyContinue;
    }
}
Get-NetAdapter | Where-Object { $_.Virtual -eq $true -or $_.InterfaceDescription -match 'VPN|Virtual|Radmin|Hamachi|TAP|ZeroTier' -or $_.Name -match 'VPN|Radmin|Hamachi' } | ForEach-Object {
    Set-NetIPInterface -InterfaceIndex $_.InterfaceIndex -InterfaceMetric 50 -ErrorAction SilentlyContinue;
}
"

echo [*] 5. Applying FPS, GPU HAGS, PowerThrottling ^& Win11 0.5ms Global Timer Tweaks...
reg add "HKCU\System\GameConfigStore" /v GameDVR_Enabled /t REG_DWORD /d 0 /f >nul
reg add "HKCU\System\GameConfigStore" /v GameDVR_FSEBehaviorMode /t REG_DWORD /d 2 /f >nul
reg add "HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\GameDVR" /v AppCaptureEnabled /t REG_DWORD /d 0 /f >nul
reg add "HKLM\SOFTWARE\Policies\Microsoft\Windows\GameDVR" /v AllowGameDVR /t REG_DWORD /d 0 /f >nul
reg add "HKLM\SYSTEM\CurrentControlSet\Control\Power\PowerThrottling" /v PowerThrottlingOff /t REG_DWORD /d 1 /f >nul
reg add "HKLM\SYSTEM\CurrentControlSet\Control\GraphicsDrivers" /v HwSchMode /t REG_DWORD /d 2 /f >nul
reg add "HKLM\SYSTEM\CurrentControlSet\Control\PriorityControl" /v Win32PrioritySeparation /t REG_DWORD /d 38 /f >nul
reg add "HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\kernel" /v GlobalTimerResolutionRequests /t REG_DWORD /d 1 /f >nul

echo.
echo ========================================================
echo   [SUCCESS] WPing v2.0 System Optimization Complete!
echo   Network, NIC Hardware, QoS, GameDVR ^& Timers Tuned.
echo ========================================================
echo.
pause