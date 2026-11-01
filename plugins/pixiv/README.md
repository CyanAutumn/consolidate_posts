# Pixiv 图片投稿插件

基于 `plugins/template` 的 `UploadDriver` + `run_plugin` 入口实现。支持单张 / 多张 PNG、JPEG、静态 GIF，标题、说明、标签、AI 标记、原创标记、公开范围和年龄限制；复用主程序的归档、账号、队列与定时补传。

该插件适配 Pixiv 当前网页投稿接口，并非 Pixiv 官方公开上传 SDK。已通过本地模拟服务器和真实插件入口测试；**尚未使用真实 Pixiv 账号发布作品**。Pixiv 页面或接口改变可能需要更新插件，验证码需由用户在网页处理。

## 配置与使用

1. 重启最新桌面版，在工作区管理中启用 Pixiv。工作区分类选“全年龄”或“R18”。
2. 在“平台登录”找到 Pixiv，点击“登录 Pixiv”；不需要添加账号或选择默认账号。
3. 点击 **“登录 Pixiv”**，直接在打开的 CloakBrowser 窗口中手动登录，并由本人完成验证码 / 二次验证。无需 PowerShell、复制命令或 Cookie。完成后界面显示成功提示，凭据使用 Windows DPAPI 加密保存。向导不会投稿，取消或失败不会覆盖原凭据。
4. 默认仅自己可见，可在界面选择公开或好友可见。年龄限制默认跟随工作区，未分类工作区须明确选择全年龄或 R18。登录完成后无需保持浏览器运行。
5. 在主程序导入图片、填写标题和说明、选择该目标并上传。成功任务回执为 `https://www.pixiv.net/artworks/<id>`。历史补传也使用这个账号配置。

可以单独构建：

```powershell
npm run plugin:build:pixiv
npm run test:pixiv
```

主程序的 `desktop:build` / `desktop:build:debug` 会先编译 Pixiv 插件再复制到分发目录。开发模式第一次运行前也应先构建插件。原始 `tauri build` 不会代为编译插件。

浏览器登录需要 Node.js 22.12+ / 24。构建会安装本插件固定版本的 Node 依赖，分发时需携带本目录的 `node_modules/`。首次登录可能下载约 200 MB 的浏览器组件到用户缓存；不会把浏览器二进制打包进项目。

```powershell
# 高级用法：也保留独立 PowerShell 配置向导（普通使用直接点击界面按钮）
& .\plugins\pixiv\configure.ps1 -DataDir "$env:APPDATA\com.uploadhub.desktop" -TargetId pixiv -BrowserLogin

# 需要原来的粘贴 Cookie 方式时使用 -ManualCookie
# & .\plugins\pixiv\configure.ps1 -DataDir '当前工作区数据目录' -TargetId pixiv -ManualCookie
```

上面的数据目录对应默认工作区。其他工作区请从主程序复制命令，不能共用默认区的路径。已有配置重新登录会保留标签、AI、可见性、年龄限制、原创和评论等设置；同一目标拒绝更换成其他 Pixiv 用户，需要换账号时先在主程序新建目标。

## 保存位置与配置

```text
<当前工作区数据目录>/plugin-data/pixiv/<上传目标 ID>/
  config.json       非机密投稿默认值
  session.dpapi     通过 Windows DPAPI 加密的 PHPSESSID
  upload.lock       本账号投稿互斥锁
  submissions/      各操作的提交状态与已确认回执
```

`config.json` 示例：

```json
{
  "userId": "123456",
  "tags": ["オリジナル", "イラスト"],
  "aiType": "notAiGenerated",
  "visibility": "private",
  "rating": null,
  "original": true,
  "allowTagEdit": false,
  "allowComment": true
}
```

`aiType` 必须为 `notAiGenerated` 或 `aiGenerated`。`rating: null` 跟随工作区：全年龄发送 `general`，R18 发送 `r18`；冲突配置会拒绝上传，不能仅靠添加 R-18 标签替代年龄限制。此版不提供 R18G 投稿。标签、AI、原创和公开范围为账号默认值，执行任务时读取；不同作品需要不同设置时，分别配置目标或先暂停计划再修改配置。插件不自动识别内容类别。

凭据只可由同一 Windows 用户在相应电脑上解密，不进入作品归档或任务数据库。跨电脑恢复数据后需重新配置登录。插件每次上传先核对登录用户 ID，避免把账号 A 的 Cookie 用在账号 B。Cookie 过期时重新运行浏览器登录向导；原来的 `-ManualCookie` 模式仍可用，输入空凭据会保留旧值。

首次登录显示窗口供本人操作；之后上传直接读取加密凭据，不需要保持浏览器运行。登录使用临时独立浏览器上下文，不保留明文 Cookie 文件、浏览器账号配置或密码；凭据通过 Node 与原生插件之间的私有管道传递，不输出到终端。关闭上下文时也关闭浏览器，原生 Windows 进程组负责退出时清理子进程。

## 校验和恢复

- 标题 1–32 个 UTF-16 单元，说明不超过 3000；1–200 张图（插件数量上限），每图 32 MiB、总计 200 MiB。实际解码图片，不接受伪装扩展名或动画 GIF；解码内存限 256 MiB、宽高限 32768。极大图片需缩小后上传。
- 网络只发往固定的 `https://www.pixiv.net`，不提供自定义服务器参数，不携带 Cookie 跟随重定向。连接超时 20 秒，每次请求最多 300 秒；转换结果最多查询约 3 分钟。
- 先获取登录页中的 CSRF token，再检查投稿初始化；遇到验证码、账号引导、登录失效、限流会明确失败，不自动解决挑战、不自动重复 POST。
- `operationId` 跨重试保持一致，记录先落盘再提交。取得 `convertKey` 后中断，点击原任务“重试”只继续查询；收到 `COMPLETE` 和数字作品 ID 才保存成功回执。
- 若提交后断线、还没拿到 `convertKey`，状态为 `submitting`，无法知道远端是否已创建。该账号新投稿也会阻止，避免重复发布。先打开 Pixiv 作品管理核对；确认未创建或已手动处理后，将该记录移出 `submissions/` 备份再创建新任务。不要未经核对删除记录或盲目重发。若作品已存在，主程序目前不能手工补录成功状态。
- 取消本地任务不代表撤回远端作品。已提交的任务可能仍在 Pixiv 处理；应重试原任务查询状态，不直接创建同样的新任务。
- 同一目标一次只接受一项投稿；并发任务会等待前一项释放账号锁，最长等待 10 分钟。超时会返回账号忙，需手动重试。批量补传建议设置间隔，不要同时对同一账号运行多个计划。定时仍要求主程序开启。

## 实现依据

图片格式和大小依据 [Pixiv 投稿帮助](https://www.pixiv.help/hc/en-us/articles/235584588-How-can-I-post-illustrations-on-pixiv) 与 [文件大小说明](https://www.pixiv.help/hc/en-us/articles/235645687-Are-there-any-limits-on-image-file-size)。接口字段来自 2026-10-23 获取的 [Pixiv 投稿页面](https://www.pixiv.net/illustration/create) 第一方网页代码：`/ajax/work/create/illustration/init`、multipart 投稿和 `/progress?convertKey=...`。不将模拟测试视为在线可用性证明。

凭据实现依据 [Microsoft ConvertFrom-SecureString](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.security/convertfrom-securestring) 与 [CryptUnprotectData](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptunprotectdata)。

浏览器登录采用 [CloakBrowser](https://github.com/CloakHQ/CloakBrowser) + Playwright。包装代码和浏览器二进制的许可不同，二进制由上游下载到用户缓存，不随项目重新分发。浏览器登录测试使用真实无头浏览器访问本地模拟页面，验证账号识别、Cookie、取消和加密保存；不代表已用真实 Pixiv 账号登录或投稿验证。

每个平台只显示一个登录账号，旧版已登录配置会自动沿用。投稿设置放在折叠项中，可以单独保存，不需要重新登录。登录本身不要求先填写标签或 AI 标记；发布前仍必须明确填写，程序不会猜测 AI 属性。

## 投稿实现调研（2026-11-01）

已核对开源项目 [civitai-post-splitter 的 Pixiv 投稿实现](https://github.com/1756141021/civitai-post-splitter/blob/main/pixiv/support.py#L3665)：使用 Playwright 操作网页文件输入、标题、标签、AI、年龄限制、原创与公开范围，并等待安全检查及提交结果。这提供了浏览器投稿的参考，但不能证明本插件本次失败的具体原因；也不采用其“离开投稿页或文件输入消失即成功”的宽松判断，本插件仍要求取得作品 ID。

同日只读实号检查通过登录用户核对和投稿 init；对照第一方网页代码，`files[]`、`imageOrder[i][type/fileKey]`、`tags[]`、AI / 年龄 / 公开范围的取值与当前实现一致。网页还会携带安全验证等上下文字段，不能仅凭主要字段一致断言直接 HTTP 投稿可用。此次没有重新提交作品。

明确返回 `error: true` 且无转换编号时，现在显示 `message` 与 `body.errors` 中的错误信息，投稿记录标为 `rejected`，允许用户手动重试；缺少标准响应、网络中断或响应含转换编号却声称失败时，仍按结果未知处理。旧版丢失响应的 `submitting` 记录不能自动转换为拒绝状态。
