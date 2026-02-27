#!/bin/sh
# shellcheck disable=SC1091
set -e

main() {
    check_dependencies
    platform=$(detect_platform)
    version=$(get_latest_version)
    bin_dir=$(get_install_dir)

    if [ -z "$version" ]; then
        echo "Error: failed to fetch latest version from GitHub." >&2
        echo "Check your internet connection and try again." >&2
        exit 1
    fi

    echo "Installing md ${version} (${platform})..."

    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT

    url="https://github.com/Harsh-2002/MD/releases/download/${version}/md-${platform}.tar.gz"

    if ! curl -fsSL --retry 3 --retry-delay 2 "$url" -o "${tmp}/md.tar.gz"; then
        echo "Error: failed to download from:" >&2
        echo "  ${url}" >&2
        echo "Release binaries may still be building. Try again in a few minutes." >&2
        exit 1
    fi

    tar xzf "${tmp}/md.tar.gz" -C "$tmp"

    if [ ! -f "${tmp}/md" ]; then
        echo "Error: binary not found in archive." >&2
        exit 1
    fi

    mkdir -p "$bin_dir"
    cp "${tmp}/md" "${bin_dir}/md"
    chmod +x "${bin_dir}/md"

    if ! "${bin_dir}/md" --version >/dev/null 2>&1; then
        echo "Error: installed binary is not executable." >&2
        exit 1
    fi

    ensure_in_path "$bin_dir"
    setup_completions "${bin_dir}/md"
    reload_shell

    echo ""
    echo "  md ${version} installed to ${bin_dir}/md"
    echo "  Run 'md --help' to get started."
    echo ""
}

check_dependencies() {
    for cmd in curl tar uname grep; do
        if ! command -v "$cmd" >/dev/null 2>&1; then
            echo "Error: required command '$cmd' not found." >&2
            exit 1
        fi
    done
}

detect_platform() {
    os=$(uname -s)
    arch=$(uname -m)

    case "$arch" in
        x86_64|amd64)  arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        armv7*)
            echo "armv7-unknown-linux-gnueabihf"
            return
            ;;
        *) echo "Unsupported architecture: $arch" >&2; exit 1 ;;
    esac

    case "$os" in
        Linux)
            if ldd --version 2>&1 | grep -qi musl; then
                echo "${arch}-unknown-linux-musl"
            else
                echo "${arch}-unknown-linux-gnu"
            fi
            ;;
        Darwin)
            echo "${arch}-apple-darwin"
            ;;
        *)
            echo "Unsupported OS: $os" >&2
            exit 1
            ;;
    esac
}

get_latest_version() {
    curl -fsSL --retry 3 --retry-delay 1 \
        https://api.github.com/repos/Harsh-2002/MD/releases/latest 2>/dev/null \
        | grep '"tag_name"' | head -1 | cut -d'"' -f4
}

get_install_dir() {
    if [ "$(id -u)" = 0 ]; then
        echo "/usr/local/bin"
    else
        echo "${HOME}/.local/bin"
    fi
}

ensure_in_path() {
    case ":${PATH}:" in
        *":$1:"*) return ;;
    esac

    shell=$(basename "$SHELL")
    # shellcheck disable=SC2016
    line='export PATH="'"$1"':$PATH"'

    case "$shell" in
        bash)
            rc=$(get_bash_rc)
            add_line "$rc" "$line"
            ;;
        zsh)
            add_line "${HOME}/.zshrc" "$line"
            ;;
        fish)
            mkdir -p "${HOME}/.config/fish"
            add_line "${HOME}/.config/fish/config.fish" "fish_add_path $1"
            ;;
        *)
            echo "  Add $1 to your PATH manually."
            ;;
    esac
}

get_bash_rc() {
    if [ -f "${HOME}/.bashrc" ]; then
        echo "${HOME}/.bashrc"
    else
        echo "${HOME}/.bash_profile"
    fi
}

add_line() {
    if [ ! -f "$1" ]; then
        touch "$1"
    fi
    grep -qF "$2" "$1" 2>/dev/null || echo "$2" >> "$1"
}

setup_completions() {
    md_bin="$1"
    shell=$(basename "$SHELL")
    case "$shell" in
        bash) setup_bash "$md_bin" ;;
        zsh)  setup_zsh "$md_bin" ;;
        fish) setup_fish "$md_bin" ;;
        *) ;;
    esac
}

setup_bash() {
    dir="${HOME}/.local/share/bash-completion/completions"
    mkdir -p "$dir"
    "$1" --completions bash > "$dir/md"

    # shellcheck disable=SC2016
    line='[ -f "${HOME}/.local/share/bash-completion/completions/md" ] && . "${HOME}/.local/share/bash-completion/completions/md"'

    rc=$(get_bash_rc)
    add_line "$rc" "$line"
}

setup_zsh() {
    dir="${HOME}/.local/share/zsh/site-functions"
    mkdir -p "$dir"
    "$1" --completions zsh > "$dir/_md"

    # shellcheck disable=SC2016
    add_line "${HOME}/.zshrc" 'fpath=("'"$dir"'" $fpath)'

    if ! grep -q 'compinit' "${HOME}/.zshrc" 2>/dev/null; then
        # shellcheck disable=SC2016
        add_line "${HOME}/.zshrc" 'autoload -Uz compinit && compinit'
    fi
}

setup_fish() {
    dir="${HOME}/.config/fish/completions"
    mkdir -p "$dir"
    "$1" --completions fish > "$dir/md.fish"
}

reload_shell() {
    shell=$(basename "$SHELL")
    case "$shell" in
        bash)
            rc=$(get_bash_rc)
            # shellcheck disable=SC1090
            . "$rc" 2>/dev/null || true
            ;;
        zsh)
            . "${HOME}/.zshrc" 2>/dev/null || true
            ;;
        fish)
            # Fish auto-loads completions, no reload needed
            ;;
    esac
}

main
