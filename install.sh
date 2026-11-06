#!/bin/sh
# Install a checksum-verified release without Rust, Python or root.
set -eu
main() {
    fail() { printf 'termune: %s\n' "$*" >&2; exit 1; }
    for command in curl uname awk mktemp; do
        command -v "$command" >/dev/null 2>&1 || fail "Required command not found: $command"
    done
    case "$(uname -s)" in
        Linux) system=linux ;;
        Darwin) system=macos ;;
        *) fail 'Use install.ps1 on Windows. Supported Unix systems: Linux and macOS.' ;;
    esac
    case "$(uname -m)" in
        x86_64|amd64) architecture=x86_64 ;;
        aarch64|arm64) architecture=aarch64 ;;
        *) fail 'Supported CPU architectures: x86_64 and ARM64.' ;;
    esac
    if [ "$system" = macos ] && [ "$(sysctl -n hw.optional.arm64 2>/dev/null || true)" = 1 ]; then
        architecture=aarch64
    fi
    case "$system" in
        linux) target=$architecture-unknown-linux-gnu ;;
        macos) target=$architecture-apple-darwin ;;
    esac
    repository=https://github.com/Nyakooo/termune
    version=${TERMUNE_VERSION:-}
    if [ -z "$version" ]; then
        release_url=$(curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSLI --retry 3 --connect-timeout 15 --max-time 120 -o /dev/null -w '%{url_effective}' "$repository/releases/latest") || fail 'No published release is available, or GitHub cannot be reached.'
        case "$release_url" in
            "$repository"/releases/tag/v*) version=${release_url##*/} ;;
            *) fail 'No published release is available. Try again after Actions finishes.' ;;
        esac
    fi
    case "$version" in v*) tag=$version ;; *) tag=v$version ;; esac
    case "$tag" in *[!a-zA-Z0-9.+-]*|v|v[!0-9]*) fail "Invalid version: $tag" ;; esac
    asset=termune-$target
    directory=${TERMUNE_INSTALL_DIR:-"$HOME/.local/bin"}
    case "$directory" in /*) ;; *) directory=$PWD/$directory ;; esac
    temporary=$(mktemp -d)
    staged=
    cleanup() {
        rm -rf "$temporary"
        if [ -n "$staged" ]; then rm -f "$staged"; fi
    }
    trap cleanup 0
    trap 'exit 1' HUP INT TERM
    download() {
        curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL --retry 3 --connect-timeout 15 --max-time 300 --max-filesize 134217728 "$repository/releases/download/$tag/$1" -o "$temporary/$1"
    }
    printf 'Downloading Termune %s for %s…\n' "$tag" "$target"
    download "$asset" || fail 'Binary download failed; this release may not include your platform.'
    download SHA256SUMS || fail 'Cannot download checksums; nothing was installed.'
    expected=$(awk -v file="$asset" '$2 == file && NF == 2 { print tolower($1) }' "$temporary/SHA256SUMS")
    [ "${#expected}" -eq 64 ] || fail 'Missing, duplicate or invalid checksum.'
    case "$expected" in *[!0-9a-f]*) fail 'Invalid checksum.' ;; esac
    if command -v sha256sum >/dev/null 2>&1; then
        actual=$(sha256sum "$temporary/$asset" | awk '{print $1}')
    elif command -v shasum >/dev/null 2>&1; then
        actual=$(shasum -a 256 "$temporary/$asset" | awk '{print $1}')
    else
        fail 'SHA256 verification requires sha256sum or shasum.'
    fi
    [ "$actual" = "$expected" ] || fail 'SHA256 mismatch; nothing was installed.'
    [ -s "$temporary/$asset" ] || fail 'The binary is empty.'
    mkdir -p "$directory"
    [ ! -d "$directory/termune" ] || fail 'The destination is a directory.'
    staged=$(mktemp "$directory/.termune.XXXXXX")
    cp "$temporary/$asset" "$staged"
    chmod 755 "$staged"
    mv -f "$staged" "$directory/termune"
    staged=
    printf 'Installed: %s/termune\n' "$directory"
    case ":$PATH:" in
        *":$directory:"*) printf 'Run: termune\n' ;;
        *)
            if [ -z "${TERMUNE_INSTALL_DIR:-}" ] && [ "${TERMUNE_NO_MODIFY_PATH:-0}" != 1 ]; then
                case "${SHELL:-}" in
                    */zsh) profile_path=${ZDOTDIR:-$HOME}/.zshrc ;;
                    */bash) if [ "$system" = macos ]; then profile_path=$HOME/.bash_profile; else profile_path=$HOME/.bashrc; fi ;;
                    */fish) profile_path=${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/termune.fish ;;
                    *) profile_path=$HOME/.profile ;;
                esac
                mkdir -p "$(dirname "$profile_path")"
                if ! grep -F '# Termune user executables' "$profile_path" >/dev/null 2>&1; then
                    # Keep HOME/PATH literal for the next shell.
                    # shellcheck disable=SC2016
                    case "${SHELL:-}" in
                        */fish) printf '\n# Termune user executables\nif not contains -- "$HOME/.local/bin" $PATH\n    set -gx PATH "$HOME/.local/bin" $PATH\nend\n' >> "$profile_path" ;;
                        *) printf '\n# Termune user executables\nexport PATH="$HOME/.local/bin:$PATH"\n' >> "$profile_path" ;;
                    esac
                fi
                printf 'PATH configured in %s. Open a new terminal.\n' "$profile_path"
            else
                printf 'Add this directory to PATH: %s\n' "$directory"
            fi
            printf 'Run now: %s/termune\n' "$directory"
            ;;
    esac
    printf 'Playback requires VLC. Your NAS credentials are not changed.\n'
}
# Execute only after the complete function definition has been received.
main "$@"
