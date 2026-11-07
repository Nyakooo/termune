# 构建与发布

工作流：[Build, test and release](https://github.com/Nyakooo/termune/actions/workflows/ci.yml)。

## 本地构建

使用支持 Rust 2024 的稳定版 Rust、C 编译器和 CMake 3.16 或更新版本。Linux 构建还需要 ALSA 开发库。Debian 或 Ubuntu 可安装以下依赖。

```sh
sudo apt-get install build-essential cmake pkg-config libasound2-dev
```

macOS 使用 Xcode 命令行工具，Windows 使用 MSVC C++ 构建工具。

在仓库根目录构建并运行测试。

```sh
cargo build --release --locked
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

生成的程序位于 `target/release/termune`，Windows 下文件名为 `termune.exe`。

## 平台与校验

| Rust target | GitHub runner |
| --- | --- |
| x86_64-unknown-linux-gnu | ubuntu-22.04 |
| aarch64-unknown-linux-gnu | ubuntu-22.04-arm |
| x86_64-apple-darwin | macos-15-intel |
| aarch64-apple-darwin | macos-15 |
| x86_64-pc-windows-msvc | windows-latest |
| aarch64-pc-windows-msvc | windows-11-arm |

每个平台检查格式、Clippy、Rust 测试、打包测试及 release 构建，并执行 `--version` / `--help`。播放集成测试使用本地模拟 NAS 和合成音频，覆盖认证、解码、音效切换、Range 拖动、暂停和曲终。声卡静音测试默认忽略，可在有设备的环境中手动运行。

## 发布步骤

1. 同步修改 `Cargo.toml`、`Cargo.lock` 中的项目版本与 `RELEASE_NOTES.md`。
2. 本地验证 `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked` 和 `python3 scripts/package_release.py --check`。
3. 提交并推送分支，创建匹配版本的注释标签：

   ```sh
   git tag -a v0.1.3 -m "Termune v0.1.3"
   git push origin master
   git push origin v0.1.3
   ```

4. 打开 Actions 页面自行查看结果。推送 `v*` 标签会运行发布工作流。普通分支推送由 PR 检查或手动运行验证，发布时只跑标签这一套构建。六个平台及安装脚本测试全部成功后自动创建 Release，无需本地打包或提供 PAT。工作流使用自带 `GITHUB_TOKEN`，只有发布 job 有 `contents: write` 权限。

Linux / macOS 输出 `termune-<target>.tar.gz` 和无扩展名的 `termune-<target>`，Windows 输出 `termune-<target>.zip` 和 `termune-<target>.exe`。包内目录为 `termune-v<version>-<target>`，包含程序、文档、安装脚本和版本记录。汇总阶段要求十二份资源全部存在、非空且无意外文件，才生成 `SHA256SUMS` 并上传。SHA256 用于传输完整性检查，不等于独立数字签名。

标签必须精确匹配 Cargo 版本。形如 `v0.2.0-beta.1` 的预发布标签会标为 prerelease；`termune update` 只查询最新正式版本。不要移动或覆盖已经发布的标签。

如需重试，可在对应标签的 Actions 页面重新运行失败任务，或手动以该标签运行工作流。当前工作流会覆盖同名发布资源；启用 GitHub 不可变 Release 后应改发新版本，不能覆盖已有资源。

## 自更新契约

自更新读取 `https://api.github.com/repos/Nyakooo/termune/releases/latest`，只接受正式 Release、合法 SemVer 以及本仓库对应标签下的精确资源地址。下载大小上限 128 MiB，检查 SHA256SUMS、长度及 ELF / Mach-O / PE 系统架构标识，校验完成前不修改当前程序。

临时文件与程序位于同一目录，Unix 使用原子 rename，Windows 使用 self-replace 处理运行中程序的文件锁和旧文件清理；校验失败清理临时文件并保留原程序。同一安装目录以 `.termune-update.lock` 排除并发更新。更新不访问 NAS 配置或凭据存储。安装在不可写目录时会报错，建议使用安装脚本默认的用户目录。

## GitHub 项目信息

建议 Description：

> A terminal music client for Feiniu NAS, built with Rust.

建议 Topics（按需填写，均为 GitHub 可用的小写格式）：

`termune`, `feiniu`, `fnos`, `nas`, `music-player`, `terminal`, `tui`, `cli`, `rust`, `ratatui`, `self-hosted`, `fn-connect`

## 安装脚本验证

`install.sh` 下载对应平台的独立程序，校验 SHA256 后在安装目录中暂存并替换。`install.ps1` 另外检查 Windows PE 格式和系统架构，通过用户 PATH 提供命令。默认安装最新正式 Release，可用 `TERMUNE_VERSION`、`TERMUNE_INSTALL_DIR` 和 `TERMUNE_NO_MODIFY_PATH` 调整。

CI 在 Linux / macOS 运行 Unix 安装 fixture，在 Windows x64 / ARM64 上分别用 PowerShell 5.1 和 PowerShell 7 运行安装 fixture，覆盖架构、校验失败保留旧程序、自定义路径、重复安装及 `irm | iex` 调用。所有 fixture 拦截下载，不接触真实用户安装或 NAS 凭据。

音频解码器编入各平台二进制。构建内置 Opus 需要 CMake 和 C 编译器，Linux 另需 `libasound2-dev`。DSP 方案和验证见 [音效研究](AUDIO_EFFECTS_RESEARCH.md)。Windows 自更新采用 [self-replace](https://docs.rs/self-replace/latest/self_replace/)。
