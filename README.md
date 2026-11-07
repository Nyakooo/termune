# Termune

![Termune — 让 NAS 音乐，在终端响起](docs/assets/termune-cover.svg)

飞牛音乐的第三方终端客户端，支持 Windows、macOS 和 Linux。

在终端浏览和搜索曲库、播放音乐、管理收藏和歌单。

## 安装

### Linux 和 macOS

```sh
curl -fsSL https://raw.githubusercontent.com/Nyakooo/termune/master/install.sh | sh
```

### Windows PowerShell

```powershell
irm https://raw.githubusercontent.com/Nyakooo/termune/master/install.ps1 | iex
```

### 手动下载

[前往 GitHub Releases 下载](https://github.com/Nyakooo/termune/releases/latest)

解压后，在对应目录运行 `./termune` 或 `.\termune.exe`。

## 登录

1. 在 NAS 上启用 FN Connect，安装并初始化飞牛音乐，创建音乐独立账号。
2. 重新打开终端，运行 `termune`。
3. 输入 **FN ID、音乐用户名和音乐密码**。

登录页可勾选「下次自动登录」。连接成功后，顶栏会显示局域网 IP 或当前连接方式。

## 常用快捷键

| 按键 | 操作 |
| --- | --- |
| ↑ / ↓ | 选择条目 |
| Enter | 打开条目或播放歌曲 |
| Tab | 切换分类 |
| ← / → | 翻页 |
| `/` | 搜索歌曲 |
| 空格 | 暂停或继续播放 |
| `[` / `]` | 上一首 / 下一首 |
| E | 切换音效 |
| P | 设置 |
| `?` | 查看全部快捷键 |
| Q | 退出 |

## 更新

```sh
termune update
```

## 更多文档

- [使用指南（含局域网连接与诊断）](docs/USAGE.md)
- [更新记录](RELEASE_NOTES.md)
- [构建与发布](docs/RELEASING.md)
- [音频组件许可](docs/AUDIO_LICENSES.md)
