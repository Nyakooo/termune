# 连接与资源探针 / Protocol notes

## 来源与范围

- 官方说明：<https://help.fnnas.com/articles/v1/access/how-access>
- 官方 FN Connect 网页：<https://fnos.net>
- 本次核对的官方前端资产：<https://static2.fnnas.com/connect/assets/1MPg8Gvv7C7Lrf46.js>
- 音乐 API 行为参考：<https://github.com/kuilei0926/FeiNiuMusic/blob/main/lib/app/services/feiniu/api_client.dart>

2026-11-05 检查公开协议行为，独立实现 Rust 客户端，未复制第三方业务代码。发现接口是网页内部接口，不能保证长期兼容。官方浏览器客户端的 STUN/iframe 探测在这里替换为原生 HTTP 音乐配置探测；不声称完整复刻官方连接算法。

## FN ID 地址发现

`POST https://fnos.net/api/v1/fn/con`，紧凑 JSON `{"fnId":"..."}`。

官方前端包含两类请求签名：`authx`（nonce、毫秒时间戳、MD5 请求体及协议常量组成），以及 `fn-sign`（SHA256 的 `trim_connect\`FN_ID\`TIMESTAMP\`anna`）。常量是公开客户端协议参数，不是用户凭据；签名仅用于地址发现，音乐密码不发给发现服务。客户端系统时间需要准确。

候选来自 `ipv4`、`ipv6`、`publicIpv4`、`publicIpv6`、`ddns`、`fn` 及 `port.httpsPort/httpPort`。地址和端口需要验证，候选限制数量。尊重 `forbbidPublicIpv6`（服务端原拼写）。不缓存实时地址；每次运行重新发现。

## 音乐认证

1. 公开 GET `/music/api/v1/sys/config` 探测服务。
2. GET `/music/api/v1/initialization/state`，未知初始化结构仅报告未确认，不作为登录成功证据。
3. POST `/music/api/v1/user/password-login`：`username`、原始 UTF-8 密码的 SHA256 小写 hex、持久化的 16 随机字节 hex `deviceId`。
4. HTTP 及业务成功、`userToken` 非空后，GET `/music/api/v1/user/me`，Cookie `music-token=...`。中继所有音乐请求附加 `mode=relay`。
5. 当前探针对用户字段要求 `name`、`guid`，接受 `data` 或 `data.user`。其他结构失败并等待真实版本核对。

`code=120001` 报账号密码错误。其他业务码仅输出编号，避免服务端消息回显秘密或终端控制符。JSON 最大读取 1 MiB，关闭认证重定向，不输出原始网络错误中的 URL。只同源跟随公开探测重定向，跨源跳转通过后续候选探测处理。

## 曲库、搜索与媒体

- GET `/track/list`、`/album/list`、`/artist/list`、`/playlist/list`，参数 `page=1&size=50`，total 超过 50 时验证第二页。读取 `data.list` 和 `data.total`，total 兼容数字字符串；guid 必须有效且页内/跨页不重复。其他元数据暂保留原始 JSON，不把空可选字段当成错误；尚未构建完整音乐业务模型。
- GET `/search/track`：用 URL 查询参数编码 q，支持中文及 `&`、`?` 等字符；空列表正常，搜索结果相关性由用户确认。
- GET `/static/cover?coverId=...&size=800`：最多 4 MiB + 1 字节，验证 MIME 和文件头后按服务器/用户/封面 ID 生成散列缓存键。不解码图片，不以缓存代替本次真实请求。
- GET `/track/stream?guid=...`：Range 为 `bytes=0-4095`，较最初方案的两字节探测扩大到 4 KiB，以便在 octet-stream 响应上识别 FLAC/MP3/Ogg/WAV/MP4/DSD 等文件头。200 最多读取 4 KiB；206 按 Content-Range 读取并额外检查 1 字节，拒绝区间、总长度、字节数不一致。Content-Type 为 audio/* 或 octet-stream 配合已知文件头才能通过初步检查，HTML/JSON 不算音频；这不是解码验收。
- 媒体最多跟随 5 次 301/302/303/307/308，相对 Location 按当前 URL 解析；每一跳重新按最初 NAS 源构造 Cookie。跨源不发送任何飞牛 Cookie，返回最初源时才恢复。禁止 HTTPS 降级和跨源 HTTP，允许已明确启用的同源局域网 HTTP。记录状态码列表，不记录签名 URL。
- 一项失败不会取消其他验证；缺少可用样本显示 BLOCKED。封面与音频分别只抽样一个，不能推断整个曲库资源有效。

## TUI 第一版

默认启动 Ratatui 0.29 / Crossterm 0.28 界面，沿用 Tunerift 的依赖范围及主题配色。`--diagnose` 保留已验证的命令行诊断。新增 Serde derive、TOML 0.8 处理非敏感配置，播放由本机 VLC 解码，音频流通过标准输入传入；自动登录新增 keyring 3.6.3，并按目标平台显式启用安全存储后端。

- 主线程绘制和处理终端事件，单独后台线程持有 Session、处理发现/登录与曲库请求，消息通道返回结果。请求编号与原子版本号阻止陈旧响应覆盖新页面，并跳过尚未执行的过期请求。正在执行的网络请求受超时限制，不强制中断 socket；退出不等待后台网络任务。
- 只在登录和 `/user/me` 成功后标记 Connected。密码提交后从输入框移出，错误时保留 FN ID、用户名。Token 失效回到登录页；无效配置显示错误，密码、Token 不进入 TOML 或日志。
- 每页 50 条，四类列表分别缓存最近浏览页，搜索与详情使用独立视图。分页失败保留已显示页面；搜索失败保留原曲库，Esc 返回。
- 新增三个只读接口：`/track/album-detail/list?albumGUID=...`、`/track/artist-detail/list?artistGUID=...`、`/track/playlist-detail/list?playlistGUID=...`，各带 page/size。它们基于参考客户端协议独立实现，Mock 通过，真实 NAS 尚待确认。
- 鼠标点击、滚轮、粘贴、中英文、五主题、简洁/专业模式、小终端提示。界面文本过滤控制字符，密码始终遮罩。正常退出、I/O 错误及 panic 时恢复原始模式、鼠标捕获和备用屏幕。
- 登录页新增可选自动登录：勾选成功登录后，将 Token 及 NAS/用户 GUID 放入系统凭据存储，TOML 仅记录 `auto_login`。启动重新发现 FN ID、比对保存的 NAS GUID（若有），调用 `/user/me` 验证 Token 与用户 GUID 后才标记 Connected。
- 不勾选则不保存 Token；取消勾选立即将 TOML 开关置 false，并在后台删除当前已保存账号的凭据。关闭前未能删除凭据时也不会在下次启动读取它；配置写入失败和密钥环删除失败都会显示错误。
- 明确认证失效/身份变化清除凭据；连接错误、超时、HTTP 5xx、协议异常不被误判成 Token 过期。密钥环不可用时只允许本次会话，不明文回退，无法保存自动登录会显示持久提示。若操作系统弹出密钥环解锁/授权窗口，需解锁或取消后继续；凭据操作在后台执行。
- Linux 显式选择 keyring 的 sync-secret-service + crypto-rust + vendored（静态构建 D-Bus，无须开发包；运行仍需用户 D-Bus 和 Secret Service）；macOS apple-native，Windows windows-native。不使用平台默认的 mock 后端。诊断模式不读写自动登录凭据。
- 用户已确认真实 NAS 自动登录通过。专业模式展示歌词，封面暂不展示，媒体诊断仍用 `--diagnose`。

## 播放器与曲库补齐

- M 在非文字输入时全局切换极简；1～6 / Alt+1～6 切换全部、专辑、歌手、歌单、风格、收藏；F 收藏；O 切换顺序/列表循环/单曲循环/随机。
- 只读新增 `/genre/list`、`/track/genre-detail/list?genreGUID=...`、`/favorite-track/list`。收藏写入为 POST `/favorite-track/create` 或 `/favorite-track/delete`，JSON `{ "trackGUID": "..." }`，业务 code 0 才更新本地，不要求响应有 data。
- 播放器改为 loopback 随机路径 HTTP 音频桥。VLC 只知道本机地址，桥接负责所有 NAS 认证。GET/HEAD、单区间 Range、Content-Length/Content-Range 转发均受限制，校验 NAS 返回区间，拒绝任意目标 URL；跨源 Cookie 规则不变。
- VLC Unix RC 的 get_time/get_length 在后台查询；暂停与 seek 同一控制通道处理。拖动只在释放鼠标时提交，界面位置来自真实播放器反馈。自然播放结束才触发队列下一首，显式停止或错误不会触发循环。
- 队列保留开始播放的过滤条件与绝对索引，跨页按需读取。单曲循环复用原歌曲 GUID；随机避免立即选中同一首（来源只有一首除外）；队列变更不匹配时停止并提示重新选曲。
- LRC 多时间标签、offset 被解析为时间轴，暂停不推进、seek 后重新定位；纯文本没有同步时间。手动滚动解除跟随，Alt+Home 恢复。封面展示及 TUI 封面后台请求已按用户要求移除，诊断仍保留封面资源验证。
- 本地真实 VLC + 模拟 NAS/WAV 已验证：读取时长、Range seek、位置回报、暂停、EOF。它不代表真实 NAS 全格式播放通过；新增风格/收藏写入、队列模式及新播放桥仍需用户实机确认。

## 验收边界

Unit / Mock 测试不能证明真实 FN ID、真实远程网络及 NAS 版本兼容。用户已于 2026-11-05 提供真实运行输出，确认登录、Token 非空和 `/user/me` 验证 **PASS（用户报告）**。NAS 版本和线路类型尚未提供。用户随后提供曲库与媒体诊断输出：**PASS 8 / FAIL 0 / BLOCKED 0**。歌曲 440、专辑 220、歌手 194（各抽样两页 100 条），歌单 0；搜索匹配 1 条；封面 HTTP 200 / 45,524 字节；音频 HTTP 206 / 4,096 字节且 Content-Range 有效。本次没有发生跳转，真实跨源跳转尚未覆盖。新版 TUI 和三个详情接口的真实操作仍为 **BLOCKED（等待用户测试）**。用户已确认自动登录正常。基础播放已改为 VLC 本机 Range 音频桥；本轮新增播放桥、播放模式、风格/收藏、歌词同步仍待用户实机验证。

本地验证（2026-11-05，Linux / Rust 1.99.1）：

| 项目 | 结果 |
| --- | --- |
| Unit Tests | PASS，31 项：认证/凭据、TUI 输入与弹窗作用域、五布局与极简模式、媒体过期结果隔离、LRC 时间轴、全局 M、拖动事件、跨页队列、随机历史和收藏确认等 |
| Mock Integration Tests | PASS，14 项：原 API 及详情、播放流拒绝错误页、歌词首选版本/空列表/GUID 编码 |
| 官方发现接口负向探测 | PASS：用随机测试 FN ID 运行 Rust 程序，返回“FN ID 未找到”、退出码 1；没有输入任何音乐凭据 |
| TUI 伪终端冒烟 | PASS：原启动/遮罩/退出检查，新增复选框切换、关闭开关持久化、TOML 不含密码/Token；强制不可用 D-Bus 后端时显示错误并允许手动输入密码。使用隔离配置目录和随机测试账号，无真实 NAS 凭据 |
| cargo fmt --check | PASS |
| cargo check | PASS |
| cargo test | PASS，合计 45 项；另 2 项按需测试（预览导出、真实 VLC），真实 VLC 测试本轮单独执行通过 |
| cargo build --release | PASS |
| Real NAS Integration | 登录/曲库/搜索/封面/Range PASS（用户报告）；自动登录 PASS（用户报告）；新播放桥、队列模式、风格/收藏与歌词同步仍待实机验证 |

可直接执行当前工作区 `target/release/termune`。本机编译只证明当前 Linux 环境可运行，其他操作系统尚未编译验证。
