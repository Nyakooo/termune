![Termune — 让 NAS 音乐，在终端响起](docs/assets/termune-cover.svg)

# Termune

用 Rust 构建的飞牛音乐终端客户端。输入 FN ID 和音乐独立账号，就能在终端里浏览、搜索和播放 NAS 中的音乐。

目前为早期版本。支持 Windows、Linux、macOS，播放依赖 VLC 3；Termune 是非官方客户端，与飞牛官方没有隶属或背书关系。

## 下载

从 [GitHub Releases](https://github.com/Nyakooo/termune/releases/latest) 下载对应系统和架构的资源包，Windows 使用 `.zip`，Linux / macOS 使用 `.tar.gz`，解压后运行其中的 `termune.exe` / `termune`。资源包包含说明和版本记录，发布页提供 `SHA256SUMS`；同页独立二进制文件供直接安装和程序自更新使用。

### 一条命令安装 / 更新

Linux / macOS（curl）：

```sh
curl -fsSL https://raw.githubusercontent.com/Nyakooo/termune/master/install.sh | sh
```

Windows（PowerShell 5.1 或 PowerShell 7）：

```powershell
irm https://raw.githubusercontent.com/Nyakooo/termune/master/install.ps1 | iex
```

自动识别系统与 x86_64 / ARM64，下载最新正式 Release，校验 SHA256 后安装。不需要 Rust、Python 或管理员权限；再次执行同一命令即可更新。脚本只安装 Termune，VLC 请按下文准备。

默认目录：Linux / macOS 为 `~/.local/bin/termune`，Windows 为 `%LOCALAPPDATA%\Termune\bin\termune.exe`。脚本会配置用户 PATH；安装后重新打开终端运行 `termune`，也可立即使用脚本输出的完整路径。配置和登录凭据保留。

Linux / macOS 需要 `curl`、`sha256sum` 或 `shasum`。Windows 需要系统 PowerShell 5.1+。自定义版本和安装目录：

```sh
curl -fsSL https://raw.githubusercontent.com/Nyakooo/termune/master/install.sh | TERMUNE_VERSION=v0.1.2 TERMUNE_INSTALL_DIR="$HOME/.local/bin" sh
```

```powershell
$env:TERMUNE_VERSION = 'v0.1.2'
$env:TERMUNE_INSTALL_DIR = "$env:LOCALAPPDATA\Termune\bin"
irm https://raw.githubusercontent.com/Nyakooo/termune/master/install.ps1 | iex
```

设置 `TERMUNE_NO_MODIFY_PATH=1` 可禁止修改 PATH；Unix 自定义安装目录需要自行加入 PATH。脚本只下载已发布资源，不读取 Actions 中间产物，也不在本机编译。

### 手动下载

| 系统 | 架构 | 资源包 |
| --- | --- | --- |
| Linux | x86_64 | [termune-x86_64-unknown-linux-gnu.tar.gz](https://github.com/Nyakooo/termune/releases/latest/download/termune-x86_64-unknown-linux-gnu.tar.gz) |
| Linux | ARM64 | [termune-aarch64-unknown-linux-gnu.tar.gz](https://github.com/Nyakooo/termune/releases/latest/download/termune-aarch64-unknown-linux-gnu.tar.gz) |
| macOS | Intel | [termune-x86_64-apple-darwin.tar.gz](https://github.com/Nyakooo/termune/releases/latest/download/termune-x86_64-apple-darwin.tar.gz) |
| macOS | Apple Silicon | [termune-aarch64-apple-darwin.tar.gz](https://github.com/Nyakooo/termune/releases/latest/download/termune-aarch64-apple-darwin.tar.gz) |
| Windows | x86_64 | [termune-x86_64-pc-windows-msvc.zip](https://github.com/Nyakooo/termune/releases/latest/download/termune-x86_64-pc-windows-msvc.zip) |
| Windows | ARM64 | [termune-aarch64-pc-windows-msvc.zip](https://github.com/Nyakooo/termune/releases/latest/download/termune-aarch64-pc-windows-msvc.zip) |

Linux 使用 GNU libc，以 Ubuntu 22.04 为构建基线，不提供 Alpine/musl 二进制。macOS 使用 macOS 15 原生构建，尚未验证旧系统兼容性。当前主机验证 Linux x86_64，其他平台由 Actions 构建和测试，仍需实际设备验证。现在通过 LibVLC 3 解码，在 Termune 内完成音效和设备输出；Windows 原生构建、播放和自更新仍需结合 Actions 与实际设备验证。

Windows 资源从 v0.1.1 起提供，需要对应发布工作流成功完成后才能下载。运行程序不需要 Rust 或 Python。

## 准备

电脑安装与 Termune 架构一致的 VLC 3。Linux 需安装包含 `libvlc.so.5` 的 VLC 包；macOS 和 Windows 会尝试常见安装目录：

```sh
# Debian / Ubuntu
sudo apt install vlc

# macOS（Homebrew）
brew install --cask vlc
# 若命令不存在，将 VLC 加入 PATH：
export PATH="/Applications/VLC.app/Contents/MacOS:$PATH"
```

Windows 可使用 winget：

```powershell
winget install --id VideoLAN.VLC -e
```

安装后重新打开终端。VLC 3 与 Termune 必须具有相同架构（例如均为 x64）。Windows ARM64 若没有原生 ARM64 VLC 3，请使用 x64 Termune 和 x64 VLC；不支持在 ARM64 Termune 中加载 x64 VLC 库。


NAS 需要在线、开启 FN Connect、安装并初始化飞牛音乐，并创建**音乐独立账号**。FN ID 用于发现设备，不代替音乐账号认证。

## 使用

```sh
termune
```

手动解压的版本可在目录内运行 `./termune`，Windows PowerShell 使用 `.\termune.exe`。

依次输入 **FN ID、音乐用户名、音乐密码**，密码隐藏显示。无需填写完整 NAS 地址。通过官方 FN Connect 地址发现服务查询线路，自动探测局域网、公网、DDNS 和中继；默认验证 HTTPS 证书。

建议终端至少 **36 × 20**，推荐 **100 × 30** 或更大，支持 UTF-8、真彩色和鼠标事件。输入时字母仍是文字，快捷键不会抢占密码或搜索内容。

曲库按专辑、歌手、风格、全部歌曲、收藏排列；自动记住上次使用的分类。Enter 打开分类详情或播放选中歌曲；播放队列保留开始播放时的来源，浏览其他分类不改变当前队列。列表按需分页，不会在启动时下载整个曲库。

## 操作

| 操作 | 鼠标 | 键盘 |
| --- | --- | --- |
| 选择条目 | 点击 / 滚轮 | ↑↓、Home / End |
| 播放 / 打开专辑、歌手等 | 点击选中后 Enter | Enter |
| 切换五类曲库 | 点击分类 | 1～5 / Alt+1～5、Tab / Shift+Tab |
| 翻页 | 点击页码箭头 | ←→ / PageUp / PageDown |
| 搜索 / 刷新 | — | /、R |
| 暂停 / 继续 / 停止 | 播放器按钮 | 空格、S |
| 上一首 / 下一首 | 播放器按钮 | [ / ] |
| 跳转进度 | 点击或拖动进度条 | Ctrl+←→ / Ctrl+h l（也支持 Alt+←→），每次 5 秒 |
| 切换播放模式 | 点击播放器模式标签 | O |
| 切换音效 | 点击音效标签 | E |
| 调整音量 | 点击音量刻度两侧 | − / + / = |
| 收藏 / 取消收藏 | — | F |
| 极简 / 专业 | P 设置 | M（非文字输入时全局可用） |
| 浏览歌词 / 跳转 | 滚轮浏览；双击带时间戳的歌词跳转 | Alt+↑↓，Alt+Home 恢复跟随 |
| 帮助 / 设置 | 顶部按钮 | ? / P |
| 返回 / 取消 | 弹窗关闭按钮 | Esc |
| 退出 | — | Q（曲库）/ Ctrl+C |

设置中 ↑↓ / j k 选择，←→ / h l 修改，Enter 确认。播放模式在播放器内切换，支持顺序、列表循环、单曲循环和随机；单曲循环仅在自然结束时重复，手动切歌仍可跳过。完整键位见 ?。

音效在播放器中按 E 或点击切换：原声、耳机空间、醇厚低音、现场、Livehouse。切换在同一解码实例内渐变，不重启歌曲，不清空时间轴。耳机空间采用延迟低通交叉馈送与少量混响；现场使用较长扩散混响，Livehouse 使用较短混响与温暖低频。均含峰值保护，原声不叠加均衡或混响。它们不是酷狗蝰蛇、杜比或测量 HRTF 的复刻，也不需要 FFmpeg。开源方案与算法出处见 [音效研究](docs/AUDIO_EFFECTS_RESEARCH.md)。

## 命令行更新

```sh
termune update
```

查询本仓库最新正式 Release，根据系统和架构下载程序，校验 `SHA256SUMS` 中的 SHA256、下载长度和二进制格式，再替换**当前可执行文件**（Windows 使用兼容运行中文件锁的替换流程）。相同或更新版本不会降级，预发布版本不自动安装。更新不启动登录流程，不读取或修改 NAS 密码、Token、个人配置及缓存。

安装目录需要可写，建议使用安装脚本的默认用户目录并加入 PATH。更新前退出其他 Termune 实例；成功后重新运行程序。下载、校验或替换失败保留原程序。更新连接 GitHub，使用标准 `HTTPS_PROXY` / `NO_PROXY` 环境变量，不经过 NAS。

旧版本若没有 `update` 命令，先从 Releases 手动安装一次。异常退出留下 `.termune-update.lock` 时，确认没有更新进程后删除安装目录中的该文件即可重试。

## 界面

P 中分别选择布局、主题、语言和显示模式。M 随时切换极简 / 专业，保留布局偏好。专业模式显示歌词，极简保留曲库及基本播放控制。当前不展示封面。

带时间戳的 LRC 歌词跟随 VLC 返回的实际位置，暂停停止、拖动后重新定位；纯文本歌词手动浏览。缺失歌词不会中断音乐。

## 自动登录与配置

登录页勾选“下次自动登录”，成功后只将 Token 和身份信息保存到系统凭据存储，**不保存密码**。下次启动重新发现 NAS 并验证 `/user/me`；Token 有效则直接进入曲库，恢复期间不显示密码表单。

不勾选则下次输入密码。P 断开连接后可在登录页取消勾选，关闭自动登录并删除已保存凭据。断网、超时不会删除 Token；明确失效或身份不匹配才清理并要求重新登录。

Linux 自动登录依赖桌面 Secret Service（GNOME Keyring 或支持 Secret Service 的 KWallet）及用户 D-Bus 会话；macOS 使用 Keychain，Windows 使用 Credential Manager。凭据存储不可用时，本次手动登录仍可使用，但不会启用自动登录。

普通配置保存 FN ID、音乐用户名、语言、主题、布局、播放模式和自动登录开关：

- Linux：`$XDG_CONFIG_HOME/termune/config.toml`，通常为 `~/.config/termune/config.toml`。
- macOS：`~/Library/Application Support/termune/config.toml`。
- Windows：`%APPDATA%\termune\config\config.toml`。

密码和 Token 不写入普通配置。随机 deviceId 单独保存，个人配置请勿提交到 Git。

## 连接与诊断

```sh
termune --version       # 版本
termune --diagnose      # 连接、登录、曲库、搜索及媒体抽样检查
termune --login-only    # 仅验证登录
termune --help          # 命令行帮助
```

官方发现服务提供候选地址，Termune 在本机选择线路；不运行网页登录脚本，不实现官方 App 的 WebRTC/P2P 隧道，也暂不支持 FN Connect 安全码。直连证书不匹配时尝试其他线路，不提供关闭证书验证的选项。

仅在可信局域网可显式使用 `termune --allow-lan-http`。HTTP 可能暴露密码摘要和 Token，请优先 HTTPS。

诊断只抽样验证，不代表全曲库可播放。反馈问题时提供版本、操作系统和脱敏输出，不要附带密码、Token、Cookie 或完整网络抓包。[使用及验证细节](docs/USAGE.md) · [接口记录](docs/FEINIU_API.md)

## 从源码构建

安装支持 Rust 2024 的稳定版工具链：

```sh
git clone https://github.com/Nyakooo/termune.git
cd termune
# Debian / Ubuntu 编译依赖
sudo apt install pkg-config libdbus-1-dev libasound2-dev
cargo build --release --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

程序位于 `target/release/termune`。VLC 3（与程序架构一致）为播放运行依赖，不随资源包分发。

## 自动构建与发布

[GitHub Actions](https://github.com/Nyakooo/termune/actions) 在分支推送、PR 或手动触发时检查格式、Clippy、测试，并原生构建六个平台，上传资源到 Actions Artifacts。

发布时同步修改 `Cargo.toml`、`Cargo.lock` 和 [RELEASE_NOTES.md](RELEASE_NOTES.md)，再推送匹配版本的 `v*` 标签，例如 `v0.1.0`。六个平台及安装脚本测试全部通过后，工作流生成 `SHA256SUMS` 并创建 GitHub Release；版本标签不匹配、平台失败或资源不齐全时不发布。预发布标签标为 prerelease，不参与自动更新。

Python 只供 Actions 打包使用。操作流程见 [发布说明](docs/RELEASING.md)。

## 致谢

感谢 Ratatui、Crossterm、VLC 及相关 Rust 项目。
