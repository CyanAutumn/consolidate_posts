param(
    [Parameter(Mandatory=$true)][string]$DataDir,
    [string]$TargetId = 'x',
    [string]$Username,
    [switch]$Headed
)
$ErrorActionPreference = 'Stop'
if ($TargetId -notmatch '^[a-zA-Z0-9-]{1,128}$') { throw '上传目标 ID 无效。' }
if (-not [IO.Path]::IsPathRooted($DataDir) -or -not (Test-Path -LiteralPath $DataDir -PathType Container)) { throw 'DataDir 必须是当前工作区数据目录的绝对路径；请从主程序复制配置命令。' }
if (-not $Username) { $Username = Read-Host 'X 用户名（@ 后的 handle，不是显示名称）' }
$Username = $Username.Trim().TrimStart('@')
if ($Username -notmatch '^[A-Za-z0-9_]{1,15}$') { throw 'X 用户名无效。' }
$entry = Join-Path $PSScriptRoot 'plugin.exe'
if (-not (Test-Path -LiteralPath $entry -PathType Leaf)) { throw '请先运行 npm run plugin:build:x。' }
$loginArgs = @('--login', '--data-dir', [IO.Path]::GetFullPath($DataDir), '--target-id', $TargetId, '--username', $Username)
if ($Headed) { $loginArgs += '--headed' }
& $entry @loginArgs
if ($LASTEXITCODE -ne 0) { throw 'X 登录配置未完成；请查看上方错误。' }
