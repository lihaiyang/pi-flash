#!/usr/bin/env bash
# macOS 打包：产出一个**自包含**的 dist/pi-flash.app（内置 node + vendored pi）。
#
# 刻意不产 dmg / img（用户 2026-10-29 明确要求）：.app 直接拖进 /Applications
# 或原地双击即可。签名一律 ad-hoc（无 Apple Developer Program，见
# docs/RELEASE.md §1）；换机器首次打开需右键 → 打开，或
# `xattr -dr com.apple.quarantine dist/pi-flash.app`。
#
# 用法：scripts/release-macos.sh [--skip-build]
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT=$(pwd)
APP="$ROOT/dist/pi-flash.app"
SKIP_BUILD=0
[ "${1:-}" = "--skip-build" ] && SKIP_BUILD=1

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/app/Cargo.toml | head -1)
[ -n "$VERSION" ] || { echo "解析 crates/app/Cargo.toml 版本失败" >&2; exit 1; }
echo "== pi-flash macOS 打包 v$VERSION =="

# 1) 编译（release 档：数据目录落 ~/.pi-flash，dev-flags 全关）
if [ "$SKIP_BUILD" = 0 ]; then
  cargo build --release -p app
  # 钥匙串助手：auth.json 的标记条目会执行它（见 pi_link::credentials）
  cargo build --release -p pi-link --bin pif-secret
fi

NODE_SRC=$(node -p 'process.execPath' 2>/dev/null || true)
[ -n "$NODE_SRC" ] && [ -x "$NODE_SRC" ] || {
  echo "找不到 node（PATH 里没有）。先装 Node 20+ 再打包。" >&2; exit 1; }
[ -d vendor/pi/node_modules ] || {
  echo "vendor/pi/node_modules 不存在：先 cd vendor/pi && npm ci" >&2; exit 1; }

# 2) bundle 骨架
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp target/release/pi-flash "$APP/Contents/MacOS/pi-flash"
# 附属可执行一律放 Resources：Contents/MacOS 只应放主可执行文件
cp target/release/pif-secret "$APP/Contents/Resources/pif-secret"
cp "$NODE_SRC" "$APP/Contents/Resources/node"
chmod +x "$APP/Contents/Resources/pif-secret" "$APP/Contents/Resources/node"
cp crates/app/assets/macos/pi-flash.icns "$APP/Contents/Resources/pi-flash.icns"

# 3) vendored pi（vendor/pi 整目录：cli.js 与它依赖的全部 node_modules）
#    node_modules 不入库，这里只拷不用；拷完不进 target/，避免污染开发树
mkdir -p "$APP/Contents/Resources/vendor"
cp -R vendor/pi "$APP/Contents/Resources/vendor/pi"

# 4) Info.plist
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>pi-flash</string>
  <key>CFBundleDisplayName</key><string>pi-flash</string>
  <key>CFBundleIdentifier</key><string>dev.pi-flash.app</string>
  <key>CFBundleExecutable</key><string>pi-flash</string>
  <key>CFBundleIconFile</key><string>pi-flash</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <!-- 终端 tab 里跑 shell / git / node 子进程：不明文声明反而会被人肉尝试 -->
  <key>NSAppleEventsUsageDescription</key><string>pi-flash 用自动化指令驱动自身界面（调试用）</string>
</dict>
</plist>
PLIST

# 5) 由内而外 ad-hoc 签名：禁用 --deep（会把 node_modules 里带点的目录误判成
#    bundle 报 "bundle format unrecognized"）；先签包内所有 Mach-O，再签外层。
while IFS= read -r -d '' f; do
  case "$(file -b "$f")" in
    Mach-O*) codesign --force --sign - "$f" >/dev/null 2>&1 || {
      echo "codesign 失败: $f" >&2; exit 1; } ;;
  esac
done < <(find "$APP" -type f \( -perm -u+x -o -name '*.node' -o -name '*.dylib' \) -print0)
codesign --force --sign - "$APP"

echo
echo "产物: $APP"
echo "版本: $VERSION   node: $("$APP/Contents/Resources/node" -v)"
du -sh "$APP"
echo "启动自检: PI_FLASH_AUTOMATION=auto ./dist/pi-flash.app/Contents/MacOS/pi-flash"
