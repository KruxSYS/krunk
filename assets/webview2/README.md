# WebView2Loader.dll

Official Microsoft WebView2 loader, embedded into LightBrowser.exe so the exe
is standalone. At startup it is written to `%LOCALAPPDATA%\LightBrowser\bin\`,
its SHA-256 is checked against the value compiled into the exe, and it is loaded
with `LoadLibraryExW` (the exe has no import-table dependency on it).

- Source: NuGet package `Microsoft.Web.WebView2` **1.0.4314.35**
  (https://www.nuget.org/packages/Microsoft.Web.WebView2/1.0.4314.35),
  file `build/native/x64/WebView2Loader.dll`
- Size: 194,912 bytes
- SHA-256: `d3fce90bdeecb08596e377dea6a316354dd8949e32ff09b89d51be00c14e21b6` (also in `SHA256SUMS`)
- Licence: BSD-style Microsoft licence, see `LICENSE.txt` (redistribution in
  binary form permitted with the notice reproduced); `NOTICE.txt` from the package.

To update: download the new nupkg, replace `x64/WebView2Loader.dll`, update
`SHA256SUMS`, this file and `LOADER_SHA256` in `app/src/ffi/loader.rs`.
