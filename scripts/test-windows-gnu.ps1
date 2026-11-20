param([string]$CompilerBin = '')
$ErrorActionPreference = 'Stop'
$repoDir = Split-Path -Parent $PSScriptRoot
if ($CompilerBin) { $env:Path = $CompilerBin + ';' + $env:Path }
$env:CC = 'gcc'
$env:CXX = 'g++'
if ($CompilerBin) { $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $CompilerBin 'gcc.exe' }
$env:CARGO_BUILD_JOBS = '4'
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:CARGO_PROFILE_TEST_DEBUG = '0'
Push-Location $repoDir
try {
    cargo +stable-x86_64-pc-windows-gnu test --manifest-path src-tauri/Cargo.toml --lib --locked --no-run
    if ($LASTEXITCODE -ne 0) { throw 'Rust test compilation failed' }
    $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $repoDir 'src-tauri/target' }
    $testExe = Get-ChildItem (Join-Path $targetDir 'debug/deps') -Filter 'grok_signup_helper_lib-*.exe' |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $testExe) { throw 'Missing compiled test executable' }
    Copy-Item -LiteralPath (Join-Path $targetDir 'debug/WebView2Loader.dll') -Destination $testExe.Directory.FullName -Force
    # rfd imports TaskDialogIndirect. Library test binaries need the same v6
    # common-controls manifest that Tauri already embeds in production binaries.
    if (-not ('ZPoolTestResources' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class ZPoolTestResources {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern IntPtr BeginUpdateResourceW(string path, bool deleteExisting);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern bool UpdateResourceW(IntPtr handle, IntPtr type, IntPtr name, ushort lang, byte[] data, uint size);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool EndUpdateResourceW(IntPtr handle, bool discard);
}
'@
    }
    $manifest = '<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0"><dependency><dependentAssembly><assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" /></dependentAssembly></dependency></assembly>'
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($manifest)
    $resourceHandle = [ZPoolTestResources]::BeginUpdateResourceW($testExe.FullName, $false)
    if ($resourceHandle -eq [IntPtr]::Zero) { throw 'Cannot open test binary resources' }
    if (-not [ZPoolTestResources]::UpdateResourceW($resourceHandle, [IntPtr]24, [IntPtr]1, 0, $bytes, $bytes.Length)) {
        [ZPoolTestResources]::EndUpdateResourceW($resourceHandle, $true) | Out-Null
        throw 'Cannot add test runtime manifest'
    }
    if (-not [ZPoolTestResources]::EndUpdateResourceW($resourceHandle, $false)) { throw 'Cannot save test runtime manifest' }
    & $testExe.FullName
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed' }
} finally { Pop-Location }
