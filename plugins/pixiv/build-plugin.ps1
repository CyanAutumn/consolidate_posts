$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    if (-not (Get-Command node.exe -ErrorAction SilentlyContinue)) { throw '浏览器登录需要 Node.js 22.12+ / 24。' }
    & npm.cmd ci --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { throw 'Pixiv 浏览器登录依赖安装失败。' }
    cargo build --release --manifest-path (Join-Path $PSScriptRoot 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw '插件编译失败。' }
    $cargo = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Cargo.toml') -Raw
    $packageName = [regex]::Match($cargo, '(?m)^name\s*=\s*"([^"]+)"').Groups[1].Value
    if (-not $packageName) { throw 'Cargo.toml 缺少 package.name。' }
    $buildRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $PSScriptRoot 'target' }
    $binary = Join-Path $buildRoot "release\$packageName.exe"
    Copy-Item -LiteralPath $binary -Destination (Join-Path $PSScriptRoot 'plugin.exe')
    Write-Output "已生成 $PSScriptRoot\plugin.exe；在 plugin.json 启用插件后，重启主程序加载。"
} finally { Pop-Location }
