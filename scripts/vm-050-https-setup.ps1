# One-time VM setup for the https part of the login-suggestion test (Krunk 0.5.0, M8).
# Run over ssh as an administrator: scripts/vm.sh put ... ; ssh windows powershell -File ...
# - two hostnames with one registrable domain point at the VM itself (hosts file);
# - a self-signed certificate for both, trusted machine-wide (Krunk's engine trusts the
#   Windows root store), bound to https port 8443 for serve.ps1 (HttpListener);
# - URL reservations so the desktop task's user may listen.
$ErrorActionPreference = 'Stop'
$hosts = 'C:\Windows\System32\drivers\etc\hosts'
$names = 'login.krunk-test.test', 'www.krunk-test.test'
$content = Get-Content $hosts -Raw
foreach ($n in $names) {
  if ($content -notmatch [regex]::Escape($n)) { Add-Content $hosts "`r`n127.0.0.1 $n" }
}
$cert = Get-ChildItem Cert:\LocalMachine\My | Where-Object { $_.FriendlyName -eq 'Krunk test https' } | Select-Object -First 1
if (-not $cert) {
  $cert = New-SelfSignedCertificate -DnsName $names -CertStoreLocation Cert:\LocalMachine\My -FriendlyName 'Krunk test https' `
    -NotAfter (Get-Date).AddYears(2) -KeyExportPolicy Exportable
  $store = New-Object System.Security.Cryptography.X509Certificates.X509Store('Root', 'LocalMachine')
  $store.Open('ReadWrite'); $store.Add($cert); $store.Close()
}
"cert $($cert.Thumbprint) $($cert.Subject)"
$bound = netsh http show sslcert ipport=0.0.0.0:8443 2>&1 | Out-String
if ($bound -notmatch $cert.Thumbprint) {
  if ($bound -match 'Certificate Hash') { netsh http delete sslcert ipport=0.0.0.0:8443 | Out-Null }
  netsh http add sslcert ipport=0.0.0.0:8443 certhash=$($cert.Thumbprint) appid='{7a2b2c0e-5a51-4f0c-9d4e-0c1e9b6a8443}' | Out-String
}
foreach ($u in 'https://*:8443/') {
  if (-not ((netsh http show urlacl url=$u | Out-String) -match [regex]::Escape($u))) { netsh http add urlacl url=$u user=Everyone | Out-String }
}
New-Item -ItemType Directory -Force C:\pipeline\krunk\www\8443 | Out-Null
Resolve-DnsName login.krunk-test.test -ErrorAction SilentlyContinue | Select-Object Name, IPAddress | Out-String
'done'
