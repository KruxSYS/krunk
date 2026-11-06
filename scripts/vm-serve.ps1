# Tiny test web server for Krunk verification (VM only; installed as C:\pipeline\krunk\serve.ps1).
# One directory per port under www\<port>. Every request is appended to www\access.log, so a
# test page can report results with fetch('/log?...'). Answers carry permissive CORS headers
# (the Local Network Access test fetches them from a public https page).
param([string]$PrefixList)
$root = 'C:\pipeline\krunk\www'
$types = @{ '.html' = 'text/html; charset=utf-8'; '.js' = 'text/javascript'; '.css' = 'text/css'; '.json' = 'application/json';
  '.webm' = 'video/webm'; '.mp4' = 'video/mp4'; '.m4s' = 'video/iso.segment'; '.mpd' = 'application/dash+xml';
  '.m3u8' = 'application/vnd.apple.mpegurl'; '.ts' = 'video/mp2t'; '.png' = 'image/png'; '.txt' = 'text/plain; charset=utf-8' }
$l = New-Object System.Net.HttpListener
foreach ($p in $PrefixList.Split(',')) { $l.Prefixes.Add($p) }
$l.Start()
while ($l.IsListening) {
  $ctx = $l.GetContext(); $req = $ctx.Request; $res = $ctx.Response
  $path = $req.Url.AbsolutePath.TrimStart('/'); if (-not $path) { $path = 'index.html' }
  "$(Get-Date -Format o) $($req.HttpMethod) $($req.Url) cookie=[$($req.Headers['Cookie'])] referer=[$($req.UrlReferrer)] origin=[$($req.Headers['Origin'])]" | Add-Content "$root\access.log"
  $file = Join-Path (Join-Path $root $req.Url.Port) $path
  $res.AddHeader('Access-Control-Allow-Origin', '*')
  $res.AddHeader('Access-Control-Allow-Private-Network', 'true')
  $res.AddHeader('Access-Control-Allow-Headers', '*')
  if ($req.HttpMethod -eq 'OPTIONS') {
    $res.StatusCode = 204; $bytes = [byte[]]::new(0)
  } elseif ($path -eq 'log') {
    $res.AddHeader('Cache-Control', 'no-store'); $res.ContentType = 'text/plain'; $bytes = [Text.Encoding]::UTF8.GetBytes('ok')
  } elseif ($path -eq 'set-cookie') {
    $res.AddHeader('Set-Cookie', 'tp_header=1; Path=/; SameSite=None; Secure')
    $res.AddHeader('Cache-Control', 'no-store')
    $bytes = [Text.Encoding]::UTF8.GetBytes((Get-Content (Join-Path $root 'cookie-frame.html') -Raw)); $res.ContentType = 'text/html'
  } elseif (Test-Path $file -PathType Leaf) {
    $bytes = [IO.File]::ReadAllBytes($file)
    $ext = [IO.Path]::GetExtension($file).ToLower()
    $res.ContentType = if ($types.ContainsKey($ext)) { $types[$ext] } else { 'application/octet-stream' }
  } else { $res.StatusCode = 404; $bytes = [Text.Encoding]::UTF8.GetBytes('not found') }
  $res.ContentLength64 = $bytes.Length; $res.OutputStream.Write($bytes, 0, $bytes.Length); $res.Close()
}
