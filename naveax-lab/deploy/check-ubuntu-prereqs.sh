#!/usr/bin/env bash
# Read-only Ubuntu preflight. Never modifies services, firewall or networking.
set -uo pipefail
printf 'GA_REMOTE_LAB_UBUNTU_PREFLIGHT=START\n'
printf 'hostname=%s\n' "$(hostname 2>/dev/null || echo unknown)"
if [[ -f /etc/os-release ]]; then
  . /etc/os-release
  printf 'os=%s version=%s\n' "$ID" "$VERSION_ID"
else
  printf 'os=unknown\n'
fi
printf 'kernel=%s\n' "$(uname -r 2>/dev/null || echo unknown)"
for binary in node nginx caddy ss systemctl systemd-analyze; do
  if command -v "$binary" >/dev/null 2>&1; then
    printf 'binary_%s=%s\n' "$binary" "$(command -v "$binary")"
  else
    printf 'binary_%s=MISSING\n' "$binary"
  fi
done
if command -v node >/dev/null 2>&1; then
  node --version
  if node -e 'process.exit(Number(process.versions.node.split(".")[0]) >= 22 ? 0 : 1)'; then
    echo 'node22_requirement=PASS'
  else
    echo 'node22_requirement=FAIL'
  fi
else
  echo 'node22_requirement=FAIL'
fi
if command -v ss >/dev/null 2>&1; then
  echo 'PORT_BINDINGS_22_80_443_48927=READONLY'
  ss -ltn 2>/dev/null | grep -E '(:22|:80|:443|:48927)[[:space:]]' || true
else
  echo 'port_binding_scan=UNAVAILABLE'
fi
for file in /etc/nginx/nginx.conf /etc/caddy/Caddyfile /etc/systemd/system/guardadaptive-remote-lab.service; do
  if [[ -e "$file" ]];then printf 'present=%s\n' "$file"; fi
done
echo 'GA_REMOTE_LAB_PRODUCTION=BLOCKED_UNTIL_DOMAIN_TLS_HOST_ACCEPTANCE'
