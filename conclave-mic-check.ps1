# Conclave mic check: read-only. Prints who Windows says uses the microphone, from both sources Conclave can use.
# Run in a normal (not "Run as administrator") PowerShell:
#   powershell -ExecutionPolicy Bypass -File .\conclave-mic-check.ps1

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

[ComImport, Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceEnumerator {
    [PreserveSig] int EnumAudioEndpoints(int dataFlow, int stateMask, out IMMDeviceCollection devices);
}
[ComImport, Guid("0BD7A1BE-7A1A-44DB-8397-CC5392387B5E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceCollection {
    [PreserveSig] int GetCount(out uint count);
    [PreserveSig] int Item(uint index, out IMMDevice device);
}
[ComImport, Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDevice {
    [PreserveSig] int Activate(ref Guid iid, int clsCtx, IntPtr activationParams, [MarshalAs(UnmanagedType.IUnknown)] out object iface);
}
[ComImport, Guid("77AA99A0-1BD6-484F-8BC7-2C654C9A9B6F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IAudioSessionManager2 {
    [PreserveSig] int GetAudioSessionControl(IntPtr guid, uint flags, out IntPtr control);
    [PreserveSig] int GetSimpleAudioVolume(IntPtr guid, uint flags, out IntPtr volume);
    [PreserveSig] int GetSessionEnumerator(out IAudioSessionEnumerator sessions);
}
[ComImport, Guid("E2F5BB11-0570-40CA-ACDD-3AA01277DEE8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IAudioSessionEnumerator {
    [PreserveSig] int GetCount(out int count);
    [PreserveSig] int GetSession(int index, out IAudioSessionControl2 session);
}
[ComImport, Guid("bfb7ff88-7239-4fc9-8fa2-07c950be9c6d"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IAudioSessionControl2 {
    [PreserveSig] int GetState(out int state);
    [PreserveSig] int GetDisplayName(out IntPtr name);
    [PreserveSig] int SetDisplayName(IntPtr name, IntPtr ctx);
    [PreserveSig] int GetIconPath(out IntPtr path);
    [PreserveSig] int SetIconPath(IntPtr path, IntPtr ctx);
    [PreserveSig] int GetGroupingParam(out Guid param);
    [PreserveSig] int SetGroupingParam(IntPtr param, IntPtr ctx);
    [PreserveSig] int RegisterAudioSessionNotification(IntPtr client);
    [PreserveSig] int UnregisterAudioSessionNotification(IntPtr client);
    [PreserveSig] int GetSessionIdentifier(out IntPtr id);
    [PreserveSig] int GetSessionInstanceIdentifier(out IntPtr id);
    [PreserveSig] int GetProcessId(out uint pid);
}
[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class MMDeviceEnumerator {}

public static class MicSessions {
    // One line per capture session: "<state> <pid>". State 0 inactive, 1 active, 2 expired.
    public static List<string> List() {
        var result = new List<string>();
        var en = (IMMDeviceEnumerator)new MMDeviceEnumerator();
        IMMDeviceCollection devices;
        Marshal.ThrowExceptionForHR(en.EnumAudioEndpoints(1 /* eCapture */, 1 /* ACTIVE */, out devices));
        uint n; devices.GetCount(out n);
        var iid = typeof(IAudioSessionManager2).GUID;
        for (uint i = 0; i < n; i++) {
            IMMDevice dev; devices.Item(i, out dev);
            object o;
            if (dev.Activate(ref iid, 23 /* CLSCTX_ALL */, IntPtr.Zero, out o) != 0) continue;
            IAudioSessionEnumerator sessions;
            if (((IAudioSessionManager2)o).GetSessionEnumerator(out sessions) != 0) continue;
            int count; sessions.GetCount(out count);
            for (int s = 0; s < count; s++) {
                IAudioSessionControl2 c; sessions.GetSession(s, out c);
                int state; c.GetState(out state);
                uint pid; c.GetProcessId(out pid);
                result.Add(state + " " + pid + " device" + i);
            }
        }
        return result;
    }
}
'@

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$os = Get-CimInstance Win32_OperatingSystem
"Windows: $($os.Caption) build $($os.BuildNumber); elevated: $admin"

""
"== 1. Microphone usage record (registry; what Conclave reads today) =="
foreach ($hive in 'HKCU:', 'HKLM:') {
    $root = "$hive\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone"
    if (-not (Test-Path $root)) { "$hive missing"; continue }
    $packaged = @(Get-ChildItem $root | Where-Object PSChildName -ne 'NonPackaged')
    $desktop = @(if (Test-Path "$root\NonPackaged") { Get-ChildItem "$root\NonPackaged" })
    "$hive packaged apps: $($packaged.Count), desktop apps: $($desktop.Count)"
    foreach ($k in $packaged + $desktop) {
        $p = Get-ItemProperty $k.PSPath
        if ($p.LastUsedTimeStart) {
            $inUse = $p.LastUsedTimeStart -gt $p.LastUsedTimeStop
            "  in_use=$inUse start=$($p.LastUsedTimeStart) stop=$($p.LastUsedTimeStop) $($k.PSChildName)"
        }
    }
}

""
"== 2. Open microphone streams (audio sessions) =="
foreach ($line in [MicSessions]::List()) {
    $state, $procId, $device = $line -split ' '
    $name = if ($procId -eq '0') { 'system' } else { (Get-Process -Id $procId -ErrorAction SilentlyContinue).Path }
    $label = @{ '0' = 'inactive'; '1' = 'ACTIVE'; '2' = 'expired' }[$state]
    "  $label pid=$procId $device $name"
}
