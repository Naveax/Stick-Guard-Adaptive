# GuardAdaptive Remote Lab | Ubuntu safe rollout

## Status: NOT DEPLOYED

The previously documented standalone Ubuntu host was 192.168.68.61 with user naveax. On 2026-10-15 its TCP 22 SSH timed out. The server may have moved IPs or its VM/network may not be available. No server/VM services were changed. Actual domain and TLS have not been verified.

ReVoltVPN may already own TCP 443 using nginx stream or Xray. DO NOT overwrite the existing nginx/Xray configurations or firewall, restart unrelated services, or bind a competing HTTPS service to 443. The GuardAdaptive Node service should bind 127.0.0.1:48927 exclusively.

## A. Restore Ubuntu access

Verify the Ubuntu VM console and live IP (run ip -br addr on the Ubuntu console), then from the authorized Windows host:

    Test-NetConnection 192.168.68.61 -Port 22
    ssh -o BatchMode=yes -o StrictHostKeyChecking=yes naveax@192.168.68.61 'hostname; uname -a; node -v'

This is a HISTORICAL host/IP, not yet verified as the actual target. Keep SSH known-host fingerprint checks enabled.

## B. Read-only preflight

Copy remote-lab sources to a staging directory on that Ubuntu host, then:

    bash deploy/check-ubuntu-prereqs.sh

It reports OS, Node version, services installed and listeners on 22/80/443/48927, without mutations. Verify Node >=22 and the owner of TCP 443, plus available disk space. If port 443 is occupied by an existing VPN nginx-stream service, neither Caddy on 443 nor a second nginx HTTP :443 listener is appropriate.

## C. Stage separate service (ADMIN APPROVAL REQUIRED)

From the remote-lab staging root, after approving the installation steps:

    sudo install -d -m 0755 /opt/guardadaptive-remote-lab
    sudo cp -R ./server.cjs ./validate.cjs ./package.json ./public ./test /opt/guardadaptive-remote-lab/
    sudo install -d -m 0700 /etc/guardadaptive-remote-lab
    sudo install -m 0600 deploy/service.env.example /etc/guardadaptive-remote-lab/service.env
    sudo sh -c 'umask 077; openssl rand -hex 32 > /etc/guardadaptive-remote-lab/admin_token'
    sudo install -m 0644 deploy/guardadaptive-remote-lab.service /etc/systemd/system/guardadaptive-remote-lab.service

Edit /etc/guardadaptive-remote-lab/service.env with the ACTUAL, registered HTTPS origin, not the lab.example.org placeholder. This file contains no secrets; the admin token resides in a separate root-owned file read via systemd LoadCredential. Do not share the admin token.

The service uses DynamicUser=yes, no Linux capabilities, a read-only filesystem and only /var/lib/guardadaptive-remote-lab as writable state. It starts local-only and with uploads DISABLED.

After reviewing it and approving enablement:

    sudo systemd-analyze verify /etc/systemd/system/guardadaptive-remote-lab.service
    sudo systemctl daemon-reload
    sudo systemctl enable --now guardadaptive-remote-lab.service
    curl --fail http://127.0.0.1:48927/api/status

Check that the listener is only 127.0.0.1:48927 and upload_enabled is false.

## D. HTTPS route

The provided Caddyfile.example and nginx-site.example.conf are ALTERNATIVE templates only. Do not blindly install either: select only after examining the real Ubuntu TCP 443 topology, DNS and certificates. HTTP nginx TLS virtual hosts can share a single HTTP nginx listener by server_name, but nginx stream forwarding / Xray on 443 requires another routing or an authorized outbound tunnel.

A managed Cloudflare Tunnel via an authorized account and stable hostname may avoid port-443 inbound conflicts. Such a tunnel is NOT configured by this package.

The trusted HTTPS proxy must overwrite X-Real-IP with the actual client address and retain Host. Node's REMOTE_LAB_TRUST_LOOPBACK_PROXY requires a pinned REMOTE_LAB_PUBLIC_ORIGIN with HTTPS. Network traffic and any proxy logs require a separate privacy review.

## E. Enable submissions ONLY after a public pilot

After a publicly reachable HTTPS site, participant information/consent terms, strong admin token, protection against spam/DoS, clear erasure contact, 30-day retention, and a real controller trial have been checked, an administrator may change the environment file REMOTE_LAB_UPLOADS to enabled and restart ONLY the GuardAdaptive Remote Lab service.

Until then exports remain local to participants and internet uploads remain disabled. A volunteer's claim of releasing the stick is not physical ground-truth evidence. Never use this data to auto-promote a drift model. Original/shadow blind safety gates remain FAILED.

## F. Rollback

To stop only this new service:

    sudo systemctl disable --now guardadaptive-remote-lab.service

Separately remove only the additional HTTPS route created for Remote Lab if needed; do not alter existing ReVoltVPN routes. Preserve existing contributed data for its disclosed retention/erasure procedure instead of deleting it indiscriminately.

Public hostname, Ubuntu host access, Linux service verification, TLS front end and real physical pilot are not completed. No remote installation, public tunnel, firewall adjustment, driver change or GitHub push is claimed.
