param(
    [Parameter(Mandatory=$true)][string]$DataDir,
    [string]$TargetId = 'pixiv',
    [string]$UserId,
    [string[]]$Tags,
    [ValidateSet('notAiGenerated','aiGenerated')][string]$AiType,
    [ValidateSet('private','public','mypixiv')][string]$Visibility = 'private',
    [ValidateSet('workspace','general','r18')][string]$Rating = 'workspace',
    [switch]$Original,
    [switch]$BrowserLogin,
    [switch]$ManualCookie,
    [Security.SecureString]$Session
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Security') -ErrorAction Stop
if ($env:OS -ne 'Windows_NT') { throw '凭据存储需要 Windows。' }
if ($TargetId -notmatch '^[a-zA-Z0-9-]{1,128}$') { throw '上传目标 ID 无效。' }
if (-not [IO.Path]::IsPathRooted($DataDir)) { throw 'DataDir 必须为当前工作区数据目录的绝对路径。' }
$workspaceRoot = [IO.Path]::GetFullPath($DataDir)
if (-not (Test-Path -LiteralPath $workspaceRoot -PathType Container)) { throw '工作区数据目录不存在；请从主程序复制配置命令。' }
if ($BrowserLogin -and ($ManualCookie -or $PSBoundParameters.ContainsKey('Session'))) { throw 'BrowserLogin 不能和 ManualCookie / Session 同时使用。' }
$useBrowser = $BrowserLogin -or (-not $ManualCookie -and -not $PSBoundParameters.ContainsKey('Session'))
$accountPath = Join-Path (Join-Path (Join-Path $workspaceRoot 'plugin-data') 'pixiv') $TargetId
$configurationPath = Join-Path $accountPath 'config.json'
$previous = $null
if (Test-Path -LiteralPath $configurationPath) {
    try { $previous = Get-Content -LiteralPath $configurationPath -Raw -Encoding UTF8 | ConvertFrom-Json }
    catch { throw '现有 Pixiv config.json 格式错误，请备份并检查后再配置。' }
    if (-not $UserId) { $UserId = [string]$previous.userId }
    if (-not $Tags) { $Tags = @($previous.tags) }
    if (-not $AiType) { $AiType = [string]$previous.aiType }
    if (-not $PSBoundParameters.ContainsKey('Visibility')) { $Visibility = [string]$previous.visibility }
    if (-not $PSBoundParameters.ContainsKey('Rating')) { $Rating = if ($previous.rating) { [string]$previous.rating } else { 'workspace' } }
    if (-not $PSBoundParameters.ContainsKey('Original')) { $Original = [bool]$previous.original }
    if ($UserId -ne [string]$previous.userId) { throw '此目标已绑定其他 Pixiv 账号，请新建上传目标后配置。' }
}
if (-not $useBrowser -and -not $UserId) { $UserId = Read-Host 'Pixiv 数字用户 ID（个人主页 /users/ 后的数字）' }
if ($UserId -and $UserId -notmatch '^[0-9]+$') { throw '用户 ID 必须为数字。' }
if (-not $Tags) { $Tags = (Read-Host '默认投稿标签，使用英文逗号分隔（1–10 个）').Split(',') }
$Tags = @($Tags | ForEach-Object { $_.Trim() } | Select-Object -Unique)
if ($Tags.Count -lt 1 -or $Tags.Count -gt 10 -or @($Tags | Where-Object { $_.Length -lt 1 -or $_.Length -gt 32 -or $_ -match '\s' }).Count -gt 0) { throw '每个标签需为 1–32 字，不含空白；最多 10 个。' }
if (-not $AiType) {
    $answer = Read-Host '是否为 AI 生成？输入 yes 或 no（必须明确选择）'
    if ($answer -eq 'yes') { $AiType = 'aiGenerated' }
    elseif ($answer -eq 'no') { $AiType = 'notAiGenerated' }
    else { throw '请明确选择 yes 或 no。' }
}
if ($Visibility -notin @('private','public','mypixiv') -or $Rating -notin @('workspace','general','r18') -or $AiType -notin @('notAiGenerated','aiGenerated')) { throw '投稿设置无效，请检查现有配置。' }
Write-Host "可见性：$Visibility；年龄限制：$Rating。未分类工作区须指定 -Rating general 或 r18。"
$config = [ordered]@{
    userId=[string]$UserId; tags=$Tags; aiType=$AiType; visibility=$Visibility
    rating=$(if ($Rating -eq 'workspace') { $null } else { $Rating })
    original=[bool]$Original
    allowTagEdit=$(if ($previous) { [bool]$previous.allowTagEdit } else { $false })
    allowComment=$(if ($previous -and $null -ne $previous.allowComment) { [bool]$previous.allowComment } else { $true })
}
if ($useBrowser) {
    $entry = Join-Path $PSScriptRoot 'plugin.exe'
    if (-not (Test-Path -LiteralPath $entry -PathType Leaf)) { throw '请先运行 npm run plugin:build:pixiv。' }
    # Only non-secret settings enter the child over stdin. Credentials return
    # through its private Node pipe and are encrypted by the native executable.
    $previousOutputEncoding = $OutputEncoding
    try {
        $OutputEncoding = [Text.UTF8Encoding]::new($false)
        ($config | ConvertTo-Json -Depth 3 -Compress) | & $entry --browser-login --data-dir $workspaceRoot --target-id $TargetId
        if ($LASTEXITCODE -ne 0) { throw 'Pixiv 浏览器登录未完成；请按上方提示处理后重新配置。' }
    } finally { $OutputEncoding = $previousOutputEncoding }
    return
}
New-Item -ItemType Directory -Path $accountPath -ErrorAction SilentlyContinue | Out-Null
Write-Host '手动 Cookie 模式：输入自己已登录 pixiv.net 的 PHPSESSID 值，不要发送到聊天。'
if ($null -eq $Session) { $Session = Read-Host 'PHPSESSID（输入隐藏；留空保留已有凭据）' -AsSecureString }
$sessionPath = Join-Path $accountPath 'session.dpapi'
if ($Session.Length -gt 0) {
    $encryptedSession = ConvertFrom-SecureString -SecureString $Session
    [IO.File]::WriteAllText($sessionPath, $encryptedSession, [Text.UTF8Encoding]::new($false))
} elseif (-not (Test-Path -LiteralPath $sessionPath)) { throw '首次配置必须填写 PHPSESSID。' }
[IO.File]::WriteAllText($configurationPath, ($config | ConvertTo-Json -Depth 3), [Text.UTF8Encoding]::new($false))
Write-Host "配置已保存：$configurationPath"
Write-Host '本向导不发布作品；回到主程序选择该目标上传。'
