# Shows, for every Krunk process (browser + CEF subprocesses), its process type, the
# integrity level of its token, whether it is an AppContainer / restricted token, and whether
# its command line contains --no-sandbox. Renderers must be Untrusted or Low and sandboxed.
#   powershell -NoProfile -ExecutionPolicy Bypass -File vm-sandbox.ps1 [-Name Krunk.exe]
param([string]$Name = 'Krunk.exe')
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class TokenInfo {
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool OpenProcessToken(IntPtr p, uint access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool GetTokenInformation(IntPtr token, int cls, IntPtr info, int len, out int retLen);
    [DllImport("advapi32.dll")] static extern IntPtr GetSidSubAuthority(IntPtr sid, uint index);
    [DllImport("advapi32.dll")] static extern IntPtr GetSidSubAuthorityCount(IntPtr sid);
    [DllImport("advapi32.dll")] static extern bool IsTokenRestricted(IntPtr token);
    const int TokenIntegrityLevel = 25, TokenIsAppContainer = 29;
    // Returns "<level>|<appcontainer>|<restricted>" or "error:<code>".
    public static string Query(uint pid) {
        IntPtr p = OpenProcess(0x1000 /* PROCESS_QUERY_LIMITED_INFORMATION */, false, pid);
        if (p == IntPtr.Zero) return "error:open " + Marshal.GetLastWin32Error();
        IntPtr t;
        if (!OpenProcessToken(p, 0x8 /* TOKEN_QUERY */, out t)) { int e = Marshal.GetLastWin32Error(); CloseHandle(p); return "error:token " + e; }
        try {
            int len;
            GetTokenInformation(t, TokenIntegrityLevel, IntPtr.Zero, 0, out len);
            IntPtr buf = Marshal.AllocHGlobal(len);
            string level = "?";
            if (GetTokenInformation(t, TokenIntegrityLevel, buf, len, out len)) {
                IntPtr sid = Marshal.ReadIntPtr(buf);
                int count = Marshal.ReadByte(GetSidSubAuthorityCount(sid));
                int rid = Marshal.ReadInt32(GetSidSubAuthority(sid, (uint)(count - 1)));
                level = rid == 0 ? "Untrusted" : rid == 0x1000 ? "Low" : rid == 0x2000 ? "Medium" :
                        rid == 0x2100 ? "Medium+" : rid == 0x3000 ? "High" : rid == 0x4000 ? "System" : "0x" + rid.ToString("X");
            }
            Marshal.FreeHGlobal(buf);
            IntPtr ac = Marshal.AllocHGlobal(4);
            string appContainer = GetTokenInformation(t, TokenIsAppContainer, ac, 4, out len) && Marshal.ReadInt32(ac) != 0 ? "yes" : "no";
            Marshal.FreeHGlobal(ac);
            return level + "|" + appContainer + "|" + (IsTokenRestricted(t) ? "yes" : "no");
        } finally { CloseHandle(t); CloseHandle(p); }
    }
}
'@
$rows = foreach ($p in Get-CimInstance Win32_Process | Where-Object { $_.Name -eq $Name }) {
    $type = if ($p.CommandLine -match '--type=([\w-]+)') { $Matches[1] } else { 'browser' }
    if ($type -eq 'utility' -and $p.CommandLine -match '--utility-sub-type=([\w.]+)') { $type = 'utility:' + ($Matches[1] -split '\.')[-1] }
    $info = [TokenInfo]::Query([uint32]$p.ProcessId)
    $parts = $info.Split('|')
    [pscustomobject]@{
        Pid          = $p.ProcessId
        Type         = $type
        Integrity    = $parts[0]
        AppContainer = if ($parts.Count -gt 1) { $parts[1] } else { '' }
        Restricted   = if ($parts.Count -gt 2) { $parts[2] } else { '' }
        NoSandbox    = [bool]($p.CommandLine -match '--no-sandbox')
    }
}
$rows | Sort-Object Type | Format-Table -AutoSize | Out-String -Width 200
$renderers = @($rows | Where-Object Type -eq 'renderer')
$bad = @($renderers | Where-Object { $_.NoSandbox -or ($_.Integrity -notin 'Untrusted', 'Low') })
"renderers=$($renderers.Count) unsandboxed_or_wrong_integrity=$($bad.Count) any_no_sandbox_flag=$([bool]($rows | Where-Object NoSandbox))"
