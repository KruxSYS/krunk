# Makes the "inner" twin of a Krunk (Windows) vault.json: the same file with the DPAPI layer
# around the vault key removed. Run under the Windows account that wrote the vault.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File strip-dpapi.ps1 -In vault.json -Out vault-inner.json
#
# Only two values change: "wrap": "dpapi" -> "none", and "vaultKey" -> base64 of the
# DPAPI-unprotected bytes (still sealed by the key derived from the master password). Every
# other byte is kept: the two values are replaced in the text, nothing is re-serialised.
# DPAPI: CurrentUser scope, optional entropy = ASCII "LightBrowser vault key wrap v1"
# (app/src/ffi/secure.rs, ENTROPY). Base64: standard alphabet with padding, as in the file.
param([Parameter(Mandatory = $true)][string]$In, [Parameter(Mandatory = $true)][string]$Out)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Security
$utf8 = New-Object System.Text.UTF8Encoding($false)
$text = $utf8.GetString([System.IO.File]::ReadAllBytes($In))

$wrapRe = [regex]'("wrap"\s*:\s*")dpapi(")'
$keyRe = [regex]'("vaultKey"\s*:\s*")([A-Za-z0-9+/]+={0,2})(")'
if ($wrapRe.Matches($text).Count -ne 1) { throw 'expected exactly one "wrap": "dpapi"' }
$m = $keyRe.Matches($text)
if ($m.Count -ne 1) { throw 'expected exactly one "vaultKey" with a standard base64 value' }

$entropy = [System.Text.Encoding]::ASCII.GetBytes('LightBrowser vault key wrap v1')
$blob = [Convert]::FromBase64String($m[0].Groups[2].Value)
$inner = [System.Security.Cryptography.ProtectedData]::Unprotect($blob, $entropy, [System.Security.Cryptography.DataProtectionScope]::CurrentUser)
$innerB64 = [Convert]::ToBase64String($inner)

$text = $keyRe.Replace($text, { param($x) $x.Groups[1].Value + $innerB64 + $x.Groups[3].Value }, 1)
$text = $wrapRe.Replace($text, '${1}none${2}', 1)
[System.IO.File]::WriteAllBytes($Out, $utf8.GetBytes($text))
Write-Output "account $([Security.Principal.WindowsIdentity]::GetCurrent().Name); DPAPI blob $($blob.Length) bytes -> inner $($inner.Length) bytes; wrote $Out"
