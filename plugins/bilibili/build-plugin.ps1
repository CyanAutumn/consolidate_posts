$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    if (-not (Get-Command node.exe -ErrorAction SilentlyContinue)) { throw '请先安装 Node.js 22.12+ 或 24。' }
    & npm.cmd ci --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { throw '哔哩哔哩浏览器运行依赖安装失败。' }
    & cargo build --release --manifest-path (Join-Path $PSScriptRoot 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw '哔哩哔哩插件入口编译失败。' }
    $buildRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $PSScriptRoot 'target' }
    Copy-Item -LiteralPath (Join-Path $buildRoot 'release\upload-plugin-bilibili.exe') -Destination (Join-Path $PSScriptRoot 'plugin.exe')
    Write-Host '哔哩哔哩插件已构建。首次使用请到平台登录点击哔哩哔哩登录。'
} finally { Pop-Location }
