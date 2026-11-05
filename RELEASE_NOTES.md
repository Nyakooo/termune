# Termune v0.1.0

首个公开版本：在终端连接飞牛音乐，浏览并播放 NAS 中的音乐。

- 官方 FN Connect 地址发现与线路选择；支持音乐独立账号登录。
- 可选择将 Token 保存到系统凭据存储；有效会话启动时直接恢复，不保存密码。
- 歌曲、专辑、歌手、歌单、风格、收藏和搜索；按需分页读取。
- VLC 播放、暂停、跨页切歌、进度拖动，以及顺序 / 循环 / 单曲 / 随机模式。
- 专业 / 极简界面，中英文、主题与布局偏好；LRC 跟随和可选 FFmpeg 频谱。
- `termune update`：正式版本检查、SHA256 验证及原子替换，保留个人配置。
- GitHub Actions 原生构建 Linux / macOS 的 x86_64 / ARM64，发布资源包、更新二进制和 SHA256SUMS。

## 使用前

播放需要 `vlc` 在 PATH 中；频谱可选依赖 `ffmpeg`。NAS 需要启用 FN Connect，安装并初始化音乐，并创建音乐独立账号。Linux 自动登录依赖 Secret Service 与用户 D-Bus，macOS 使用 Keychain。

Linux 以 Ubuntu 22.04 / GNU libc 为构建基线；macOS 使用 macOS 15 构建。当前本地验证 Linux x86_64，其他架构需结合 Actions 与实际设备验证。Windows 暂不支持播放。

这是早期版本，不保证所有 NAS 版本、线路及音频格式兼容。不支持 FN Connect 安全码、WebRTC/P2P 隧道或封面显示。频谱使用额外音频流并增加网络流量。Termune 为非官方第三方客户端。
