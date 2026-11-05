# 构建与发布

工作流：[Build, test and release](https://github.com/Nyakooo/termune/actions/workflows/ci.yml)。

## 平台与校验

| Rust target | GitHub runner |
| --- | --- |
| x86_64-unknown-linux-gnu | ubuntu-22.04 |
| aarch64-unknown-linux-gnu | ubuntu-22.04-arm |
| x86_64-apple-darwin | macos-15-intel |
| aarch64-apple-darwin | macos-15 |

每个平台检查格式、Clippy、Rust 测试、打包测试及 release 构建，并执行 `--version` / `--help`。播放集成测试需要本机 VLC 和音频 fixture，默认忽略；工作流不会连接真实 NAS 或使用个人凭据。

## 发布步骤

1. 同步修改 `Cargo.toml`、`Cargo.lock` 中的项目版本与 `RELEASE_NOTES.md`。
2. 本地验证 `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked` 和 `python3 scripts/package_release.py --check`。
3. 提交并推送分支，创建匹配版本的注释标签：

   ```sh
   git tag -a v0.1.0 -m "Termune v0.1.0"
   git push origin master
   git push origin v0.1.0
   ```

4. 打开 Actions 页面自行查看结果。四个平台全部成功后自动创建 Release，无需本地打包或提供 PAT。工作流使用自带 `GITHUB_TOKEN`，只有发布 job 有 `contents: write` 权限。

每个平台输出 `termune-<target>.tar.gz` 和无扩展名的 `termune-<target>`。包内目录为 `termune-v<version>-<target>`，包含程序、文档、海报和版本记录。汇总阶段要求八份资源全部存在、非空且无意外文件，才生成 `SHA256SUMS` 并上传。SHA256 用于传输完整性检查，不等于独立数字签名。

标签必须精确匹配 Cargo 版本。形如 `v0.2.0-beta.1` 的预发布标签会标为 prerelease；`termune update` 只查询最新正式版本。不要移动或覆盖已经发布的标签。

如需重试，可在对应标签的 Actions 页面重新运行失败任务，或手动以该标签运行工作流。当前工作流会覆盖同名发布资源；启用 GitHub 不可变 Release 后应改发新版本，不能覆盖已有资源。

## 自更新契约

自更新读取 `https://api.github.com/repos/Nyakooo/termune/releases/latest`，只接受正式 Release、合法 SemVer 以及本仓库对应标签下的精确资源地址。下载大小上限 128 MiB，检查 SHA256SUMS、长度及 ELF / Mach-O 系统架构标识，校验完成前不修改当前程序。

临时文件与程序位于同一目录，使用 Unix 原子 rename 替换；失败清理临时文件并保留原程序。同一安装目录以 `.termune-update.lock` 排除并发更新。更新不访问 NAS 配置或凭据存储。安装在不可写目录时会报错，建议使用 `~/.local/bin`。

## GitHub 项目信息

建议 Description：

> A terminal music client for Feiniu NAS, built with Rust.

建议 Topics（按需填写，均为 GitHub 可用的小写格式）：

`termune`, `feiniu`, `fnos`, `nas`, `music-player`, `terminal`, `tui`, `cli`, `rust`, `ratatui`, `self-hosted`, `fn-connect`
