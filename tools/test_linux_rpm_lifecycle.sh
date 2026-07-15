#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 --package RPM --evidence-dir DIR [--launch-seconds N]" >&2
  exit 2
}

package=""
evidence_dir=""
launch_seconds=12
while [[ $# -gt 0 ]]; do
  case "$1" in
    --package) package="${2:-}"; shift 2 ;;
    --evidence-dir) evidence_dir="${2:-}"; shift 2 ;;
    --launch-seconds) launch_seconds="${2:-}"; shift 2 ;;
    *) usage ;;
  esac
done

[[ -n "$package" && -n "$evidence_dir" ]] || usage
[[ "$launch_seconds" =~ ^[1-9][0-9]*$ ]] || usage
[[ $(id -u) -eq 0 ]] || { echo "RPM lifecycle must run as root in an ephemeral Fedora container" >&2; exit 1; }
package=$(realpath -e "$package")
[[ -f "$package" && ! -L "$package" && "$package" == *.rpm ]] || {
  echo "package must be one regular non-symlink RPM" >&2
  exit 1
}
mkdir -p "$evidence_dir"
evidence_dir=$(realpath -e "$evidence_dir")

progress="$evidence_dir/rpm-progress.log"
: > "$progress"
log_progress() {
  printf '%s %s\n' "$(date --utc +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$progress"
}

package_name=$(rpm -qp --queryformat '%{NAME}' "$package")
package_arch=$(rpm -qp --queryformat '%{ARCH}' "$package")
package_nevra=$(rpm -qp --queryformat '%{NEVRA}' "$package")
[[ -n "$package_name" && "$package_arch" == "x86_64" ]] || {
  echo "expected a named x86_64 RPM" >&2
  exit 1
}
rpm -K "$package" | tee "$evidence_dir/rpm-signature-status.log"
rpm -qp --queryformat 'name=%{NAME}\nversion=%{VERSION}\nrelease=%{RELEASE}\narch=%{ARCH}\nnevra=%{NEVRA}\n' \
  "$package" > "$evidence_dir/rpm-package-metadata.txt"
rpm -qp --requires "$package" | sort -u > "$evidence_dir/rpm-package-requires.txt"
for requirement in \
  gtk-layer-shell pipewire-libs libX11 libXcomposite libXfixes libXinerama libxkbcommon libwayland-client; do
  grep -Fxq "$requirement" "$evidence_dir/rpm-package-requires.txt"
done

installed=false
cleanup() {
  if $installed && rpm -q "$package_name" >/dev/null 2>&1; then
    dnf -y remove "$package_name" >> "$evidence_dir/rpm-uninstall.log" 2>&1 || true
  fi
}
trap cleanup EXIT

if rpm -q "$package_name" >/dev/null 2>&1 || [[ -e /usr/bin/liveblock-linux || -e /usr/lib/LiveBlock ]]; then
  echo "LiveBlock unexpectedly installed before clean-install test" >&2
  exit 1
fi

log_progress "installing lifecycle prerequisites"
dnf -y --setopt=install_weak_deps=False install \
  xorg-x11-server-Xvfb dbus-daemon util-linux shadow-utils \
  > "$evidence_dir/rpm-prerequisites.log" 2>&1

log_progress "clean-installing $package_nevra"
dnf -y --setopt=install_weak_deps=False install "$package" \
  > "$evidence_dir/rpm-install.log" 2>&1
installed=true
rpm -q "$package_name" > "$evidence_dir/rpm-installed-query.txt"
[[ -x /usr/bin/liveblock-linux && ! -L /usr/bin/liveblock-linux ]]
for relative in \
  onnxruntime/libonnxruntime.so \
  onnxruntime/LICENSE.onnxruntime.txt \
  onnxruntime/THIRD-PARTY-NOTICES.txt \
  onnxruntime/STAGING-MANIFEST.json \
  trusted-model-keys.json; do
  path="/usr/lib/LiveBlock/resources/$relative"
  [[ -f "$path" && ! -L "$path" ]]
done
rpm -V "$package_name" > "$evidence_dir/rpm-verify-installed.log"
rpm -ql --dump "$package_name" > "$evidence_dir/rpm-installed-files.txt"
ldd /usr/bin/liveblock-linux | tee "$evidence_dir/rpm-runtime-linkage.txt"
! grep -q "not found" "$evidence_dir/rpm-runtime-linkage.txt"
log_progress "installed payload and runtime linkage verified"

user_name=liveblock-ci
useradd --create-home --home-dir /tmp/liveblock-ci-home --shell /sbin/nologin "$user_name"
install -d -m 0700 -o "$user_name" -g "$user_name" \
  /tmp/liveblock-ci-home/.local/share /tmp/liveblock-ci-runtime

set +e
runuser -u "$user_name" -- env \
  HOME=/tmp/liveblock-ci-home \
  XDG_DATA_HOME=/tmp/liveblock-ci-home/.local/share \
  XDG_RUNTIME_DIR=/tmp/liveblock-ci-runtime \
  XDG_SESSION_TYPE=x11 \
  timeout --kill-after=5s "${launch_seconds}s" \
  xvfb-run -a dbus-run-session -- /usr/bin/liveblock-linux \
  > "$evidence_dir/rpm-launch.log" 2>&1
launch_status=$?
set -e
[[ "$launch_status" -eq 124 ]]
grep -q "Loaded ONNX Runtime dylib with version '1.18.1'" "$evidence_dir/rpm-launch.log"
grep -q "trusted model keyring is empty" "$evidence_dir/rpm-launch.log"
grep -q "X11 global shortcuts registered" "$evidence_dir/rpm-launch.log"
log_progress "application survived ${launch_seconds}s and emitted expected build-only runtime evidence"

python3 - "$evidence_dir/rpm-lifecycle-summary.json" "$package_nevra" "$launch_seconds" "$launch_status" <<'PY'
import json
import sys
from pathlib import Path

output, nevra, seconds, status = sys.argv[1:]
Path(output).write_text(json.dumps({
    "schemaVersion": 1,
    "evidenceClass": "build-only-fedora-container-rpm",
    "packageNevra": nevra,
    "containerImage": "registry.fedoraproject.org/fedora@sha256:e70db1fd517c7d6990715fb200b1aae95ef6a1ef1492dc12c8ca25cc129a5094",
    "cleanInstall": True,
    "rpmDatabasePayloadVerification": True,
    "launchSurvivedSeconds": int(seconds),
    "launchTimeoutStatus": int(status),
    "packagedOnnxRuntimeLoaded": True,
    "emptyDevelopmentKeyringRejected": True,
    "x11GlobalShortcutsRegistered": True,
    "uninstallRemovedApplicationFilesAndRegistration": True,
    "harnessRemovedEmptyPackageDirectories": False,
    "productionModelAndTrustRoots": False,
    "nativeFedoraHost": False,
    "hardwareCertification": False,
}, sort_keys=True, indent=2) + "\n")
PY

log_progress "uninstalling $package_name"
dnf -y remove "$package_name" > "$evidence_dir/rpm-uninstall.log" 2>&1
installed=false
! rpm -q "$package_name" >/dev/null 2>&1
[[ ! -e /usr/bin/liveblock-linux ]]
harness_removed_empty_directories=false
: > "$evidence_dir/rpm-uninstall-residue.txt"
if [[ -e /usr/lib/LiveBlock ]]; then
  find /usr/lib/LiveBlock -mindepth 1 -printf '%y %p\n' | sort \
    > "$evidence_dir/rpm-uninstall-residue.txt"
  if find /usr/lib/LiveBlock -mindepth 1 ! -type d -print -quit | grep -q .; then
    echo "RPM uninstall left application files or special nodes" >&2
    exit 1
  fi
  find /usr/lib/LiveBlock -depth -type d -empty -delete
  harness_removed_empty_directories=true
fi
[[ ! -e /usr/lib/LiveBlock ]]
if $harness_removed_empty_directories; then
  sed -i 's/"harnessRemovedEmptyPackageDirectories": false/"harnessRemovedEmptyPackageDirectories": true/' \
    "$evidence_dir/rpm-lifecycle-summary.json"
fi
log_progress "RPM lifecycle completed"
