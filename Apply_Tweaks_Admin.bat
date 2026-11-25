@echo off
:: BatchGotAdmin
:-------------------------------------
IF "%PROCESSOR_ARCHITECTURE%" EQU "amd64" (
>nul 2>&1 "%SYSTEMROOT%\SysWOW64\cacls.exe" "%SYSTEMROOT%\SysWOW64\config\system"
) ELSE (
>nul 2>&1 "%SYSTEMROOT%\system32\cacls.exe" "%SYSTEMROOT%\system32\config\system"
)

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
echo   WPing v2.1 - Max Ping, TCP BBR2/CTCP, NIC ^& FPS Fix
echo   Created by Vsqy
echo ========================================================
echo.

echo [*] 1. Disabling Network Throttling, Reserved Bandwidth, IPv6 Stall ^& Tuning MMCSS...
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile" /v NetworkThrottlingIndex /t REG_DWORD /d 0xffffffff /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile" /v SystemResponsiveness /t REG_DWORD /d 0 /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games" /v "GPU Priority" /t REG_DWORD /d 8 /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games" /v "Priority" /t REG_DWORD /d 6 /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games" /v "Scheduling Category" /t REG_SZ /d "High" /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games" /v "SFIO Priority" /t REG_SZ /d "High" /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games" /v "Latency Sensitive" /t REG_SZ /d "True" /f >nul
reg add "HKLM\SOFTWARE\Policies\Microsoft\Windows\Psched" /v NonBestEffortLimit /t REG_DWORD /d 0 /f >nul
reg add "HKLM\SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization" /v DODownloadMode /t REG_DWORD /d 0 /f >nul
reg add "HKLM\SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters" /v DisabledComponents /t REG_DWORD /d 32 /f >nul
reg add "HKLM\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters" /v MaxUserPort /t REG_DWORD /d 65534 /f >nul
reg add "HKLM\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters" /v TcpTimedWaitDelay /t REG_DWORD /d 30 /f >nul
reg add "HKLM\SYSTEM\CurrentControlSet\Services\Dnscache\Parameters" /v NegativeCacheTime /t REG_DWORD /d 0 /f >nul

powershell -NoProfile -Command "
Remove-NetQosPolicy -Name 'WPing_Fortnite_*' -Confirm:$false -ErrorAction SilentlyContinue;
New-NetQosPolicy -Name 'WPing_Fortnite_Exe' -AppPathNameMatchCondition 'FortniteClient-Win64-Shipping.exe' -DSCPAction 46 -PriorityValue8021Action 6 -NetworkProfile All -ErrorAction SilentlyContinue | Out-Null;
New-NetQosPolicy -Name 'WPing_Fortnite_UDP_Match' -IPProtocolMatchCondition UDP -IPDstPortStartMatchCondition 9000 -IPDstPortEndMatchCondition 9200 -DSCPAction 46 -PriorityValue8021Action 6 -NetworkProfile All -ErrorAction SilentlyContinue | Out-Null;
New-NetQosPolicy -Name 'WPing_Fortnite_UDP_Beacon' -IPProtocolMatchCondition UDP -IPDstPortStartMatchCondition 15000 -IPDstPortEndMatchCondition 15200 -DSCPAction 46 -PriorityValue8021Action 6 -NetworkProfile All -ErrorAction SilentlyContinue | Out-Null;
New-NetQosPolicy -Name 'WPing_Fortnite_UDP_QoS' -IPProtocolMatchCondition UDP -IPDstPortMatchCondition 22222 -DSCPAction 46 -PriorityValue8021Action 6 -NetworkProfile All -ErrorAction SilentlyContinue | Out-Null;
"

echo [*] 2. Disabling Nagle ^& Optimizing Winsock AFD Fast-Path...
powershell -NoProfile -Command "
$interfaces = Get-ChildItem 'HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces';
foreach ($iface in $interfaces) {
    Set-ItemProperty -Path $iface.PSPath -Name 'TcpAckFrequency' -Value 1 -Type DWord -Force -ErrorAction SilentlyContinue;
    Set-ItemProperty -Path $iface.PSPath -Name 'TCPNoDelay' -Value 1 -Type DWord -Force -ErrorAction SilentlyContinue;
    Set-ItemProperty -Path $iface.PSPath -Name 'TcpDelAckTicks' -Value 0 -Type DWord -Force -ErrorAction SilentlyContinue;
}
"
reg add "HKLM\SOFTWARE\Microsoft\MSMQ\Parameters" /v TCPNoDelay /t REG_DWORD /d 1 /f >nul
reg add "HKLM\SYSTEM\CurrentControlSet\Services\AFD\Parameters" /v IgnorePushBitOnReceives /t REG_DWORD /d 0 /f >nul
reg add "HKLM\SYSTEM\CurrentControlSet\Services\AFD\Parameters" /v DynamicSendBufferDisable /t REG_DWORD /d 1 /f >nul
reg add "HKLM\SYSTEM\CurrentControlSet\Services\AFD\Parameters" /v NonBlockingSendSpecialBuffering /t REG_DWORD /d 1 /f >nul
reg add "HKLM\SYSTEM\CurrentControlSet\Services\AFD\Parameters" /v FastSendDatagramThreshold /t REG_DWORD /d 16384 /f >nul
reg add "HKLM\SYSTEM\CurrentControlSet\Services\AFD\Parameters" /v FastCopyReceiveThreshold /t REG_DWORD /d 16384 /f >nul

echo [*] 3. Disabling RSC, PacketCoalescing ^& Setting Low-Latency TCP Profile (DelayedAck=10ms/1)...
netsh int tcp set global autotuninglevel=normal >nul
netsh int tcp set global rss=enabled >nul
netsh int tcp set global rsc=disabled >nul
netsh int tcp set global heuristics=disabled >nul
netsh int tcp set global ecncapability=disabled >nul
netsh int tcp set global timestamps=disabled >nul
netsh int ip set global taskoffload=enabled >nul
netsh int tcp set supplemental template=custom >nul
netsh int tcp set supplemental custom minrto=20 icw=16 congestionprovider=bbr2 enablecwndrestart=disabled delayedacktimeout=10 delayedackfrequency=1 rack=enabled taillossprobe=enabled >nul 2>&1 || netsh int tcp set supplemental custom minrto=20 icw=16 congestionprovider=ctcp enablecwndrestart=disabled delayedacktimeout=10 delayedackfrequency=1 rack=enabled taillossprobe=enabled >nul 2>&1
powershell -NoProfile -Command "
Set-NetOffloadGlobalSetting -ReceiveSegmentCoalescing Disabled -PacketCoalescingFilter Disabled -ErrorAction SilentlyContinue;
Disable-NetAdapterRsc -Name * -ErrorAction SilentlyContinue;
Set-NetTCPSetting -SettingName InternetCustom -MinRtoMs 20 -DelayedAckTimeoutMs 10 -DelayedAckFrequency 1 -InitialRtoMs 300 -ScalingHeuristics Disabled -EcnCapability Disabled -ErrorAction SilentlyContinue;
"

echo [*] 4. Fixing Jumbo Frames (MTU=1500) ^& Deep NIC Driver Latency Tuning...
powershell -NoProfile -Command "
$chosen = '%~1';
Get-NetAdapter | Where-Object { $_.Status -eq 'Up' -and $_.Virtual -eq $false } | ForEach-Object {
    Enable-NetAdapterRss -Name $_.Name -ErrorAction SilentlyContinue;
    Disable-NetAdapterPowerManagement -Name $_.Name -ErrorAction SilentlyContinue;
    $jp = Get-NetAdapterAdvancedProperty -Name $_.Name -RegistryKeyword '*JumboPacket' -ErrorAction SilentlyContinue;
    if ($jp -and $jp.DefaultRegistryValue) { Set-NetAdapterAdvancedProperty -Name $_.Name -RegistryKeyword '*JumboPacket' -RegistryValue $jp.DefaultRegistryValue -ErrorAction SilentlyContinue }
    foreach ($kw in @('*InterruptModeration','*EEE','AdvancedEEE','EnableGreenEthernet','*FlowControl','GigaLite','PowerSavingMode','S5WakeOnLan','*PMARPOffload','*PMNSOffload','*WakeOnMagicPacket','*WakeOnPattern')) {
        Set-NetAdapterAdvancedProperty -Name $_.Name -RegistryKeyword $kw -RegistryValue 0 -ErrorAction SilentlyContinue;
    }
    netsh interface ipv4 set subinterface \"$($_.Name)\" mtu=1500 store=persistent | Out-Null;
}
if ($chosen -and (Get-NetAdapter -Name $chosen -ErrorAction SilentlyContinue)) {
    Set-NetIPInterface -InterfaceAlias $chosen -AutomaticMetric Disabled -InterfaceMetric 5 -ErrorAction SilentlyContinue;
} else {
    Get-NetAdapter | Where-Object { $_.Status -eq 'Up' -and $_.Virtual -eq $false -and $_.InterfaceDescription -notmatch 'VPN|Virtual|Radmin|Hamachi|TAP' } | ForEach-Object {
        Set-NetIPInterface -InterfaceIndex $_.InterfaceIndex -AutomaticMetric Disabled -InterfaceMetric 5 -ErrorAction SilentlyContinue;
    }
}
Get-NetAdapter | Where-Object { $_.Virtual -eq $true -or $_.InterfaceDescription -match 'VPN|Virtual|Radmin|Hamachi|TAP|ZeroTier' -or $_.Name -match 'VPN|Radmin|Hamachi' } | ForEach-Object {
    Set-NetIPInterface -InterfaceIndex $_.InterfaceIndex -AutomaticMetric Disabled -InterfaceMetric 500 -ErrorAction SilentlyContinue;
}
"
ipconfig /flushdns >nul

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
echo   [SUCCESS] All Ping, MTU, RSC, TCP ^& FPS Fixes Applied!
echo ========================================================
echo.
pause