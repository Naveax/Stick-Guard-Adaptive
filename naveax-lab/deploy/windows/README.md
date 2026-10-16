# GuardAdaptive Remote Lab v0.1.6: stable Tailscale HTTPS Windows pilot

This is a user-logon, continuously checked pilot backend. It is NOT a 24/7 hosted/cloud service, independent controller truth, or GuardAdaptive production.

## Verified connectivity on 2026-10-15

- Tailscale Windows client 1.102.2 is already connected on Naveax.
- Public Funnel is configured with tailscale funnel --bg --https=443 48928 and reports:
  https://naveax.tail10f02d.ts.net -> http://127.0.0.1:48928
- The secondary Nexowire-connected Windows PC retrieved GET /api/status with HTTPS 200.
- That PC submitted a real HTTP request containing **synthetic** (not physical controller) GA-RL-01 test series, 600 samples, and received HTTP 201 with a matching UUID.
- The synthetic sample was verified by schema and removed from the local submission directory. No new training or production acceptance is implied.
- IMPORTANT: the second Windows PC belongs to the same Tailscale network. The successful secondary-PC test **does not itself prove access from a client outside that tailnet**. Tailscale Funnel status indicates public exposure; independently verify with a browser/device not signed into this tailnet before wide participant recruitment. Public DNS changes can take minutes to propagate.
- A Cloudflare Quick Tunnel was already running independently, connected to 127.0.0.1:48927, with its own temporary hostname. It has deliberately not been stopped or repointed.
- Tailscale Funnel uses a stable ts.net host name, unlike Cloudflare Quick Tunnel. It persists its Funnel configuration across Tailscale restarts; the web backend still needs its separate startup task.
- ReVoltVPN, other Windows services and GuardAdaptive installed v0.34.0 were not altered.

## Service and data

The backend on port 48928 binds ONLY 127.0.0.1. The v0.1.6 stable site adds custom four-axis mapping (two-axis-only preview), a 20-second non-invasive demo with explicitly opt-in survey, and separate admin-managed preview records. It does not enable the GuardAdaptive live drift correction algorithm. See `docs/REMOTE_LAB_PREVIEW_SURVEY_V016.md`.

The service uses these config properties:
  REMOTE_LAB_HOST=127.0.0.1
  REMOTE_LAB_PORT=48928
  REMOTE_LAB_SIMPLE_UI=enabled
  REMOTE_LAB_UPLOADS=enabled
  REMOTE_LAB_PUBLIC_ORIGIN=https://naveax.tail10f02d.ts.net
  REMOTE_LAB_ADMIN_TOKEN_FILE=%LOCALAPPDATA%\GuardAdaptiveRemoteLabStable\secrets\admin_token.txt
  REMOTE_LAB_DATA_DIR=%LOCALAPPDATA%\GuardAdaptiveRemoteLabStable\submissions
  REMOTE_LAB_TRUST_CF_TUNNEL=disabled
  REMOTE_LAB_TRUST_LOOPBACK_PROXY=disabled

This v0.1.4 stable instance has a separate secret and separate submission directory from the Cloudflare 48927 pilot. Previous Cloudflare submissions remain untouched in the original GuardAdaptiveRemoteLab folder. The 48928 backend must be restarted once for these private paths to take effect. NO secrets are in Git, CLI arguments, task definition or public pages. The backend reads the token from the user's private file, not an environment variable containing its value. There is no public direct listener or controller write.

### Important limitation: rate limits under Funnel

We did NOT authorize Tailscale proxy-specific source-IP forwarding because the forwarded client IP's sanitization was not independently validated. The strict backend therefore sees the local Funnel proxy as one source. The existing software quota is 8 successful uploads per hour **in total on this stable port**. This prevents unbounded pilot intake but is not sufficient for high-volume data collection. Real visitor-rate differentiation requires a separately trusted edge or rate-limiting middleware, not blindly trusting arbitrary X-Forwarded-For values.

## Windows background watchdog

Scripts:
- Initialize-GuardAdaptive-StableStorage.ps1 creates a separate stable-pilot 256-bit random token (only if missing) and private records folder, with user/SYSTEM-only ACL. It never reads or overwrites Cloudflare data.
- Start-GuardAdaptive-Stable.ps1 checks expected Tailscale node, Funnel target, strong token existence, expected Node localhost socket, and exact pinned URL. Never changes VPN/Tailscale or stops any process.
- Watch-GuardAdaptive-Stable.ps1 uses a named mutex; every 60 seconds it runs the above idempotent startup/health check, logs only status without secrets. It restarts only a missing backend. If the port is used by an unexpected process it fails closed rather than killing it.
- Register-GuardAdaptive-StableTask.ps1 creates only the scheduled task GuardAdaptive-RemoteLab-StableWatch as the interactive user with **Limited** run level, triggered on logon. It refuses to overwrite any task of the same name whose action does not match. A PreviewOnly option shows what it would configure.

One-time setup (completed on Naveax during this pilot):
  pwsh -NoProfile -File remote-lab\deploy\windows\Initialize-GuardAdaptive-StableStorage.ps1
  pwsh -NoProfile -File remote-lab\deploy\windows\Register-GuardAdaptive-StableTask.ps1
  Start-ScheduledTask -TaskName 'GuardAdaptive-RemoteLab-StableWatch'

Check locally:
  Get-ScheduledTask -TaskName 'GuardAdaptive-RemoteLab-StableWatch'
  Get-Content "$env:LOCALAPPDATA\GuardAdaptiveRemoteLab\logs\stable-watch.log" -Tail 10
  Invoke-RestMethod http://127.0.0.1:48928/api/status
  & 'C:\Program Files\Tailscale\tailscale.exe' funnel status

The Windows scheduler task requires the interactive user to be logged in; it is not a LocalSystem machine-level always-on service. It does not prevent Windows sleep or power outages. No sleep/power configuration was changed.

## Rollback (only Remote Lab)

Stop the dedicated scheduled task, then unregister only that task:
  Stop-ScheduledTask -TaskName 'GuardAdaptive-RemoteLab-StableWatch'
  Unregister-ScheduledTask -TaskName 'GuardAdaptive-RemoteLab-StableWatch' -Confirm:$false

Disable ONLY the Funnel established for this project:
  & 'C:\Program Files\Tailscale\tailscale.exe' funnel --https=443 off

The script registration does not change other Tailscale shares and never modifies ReVoltVPN/Xray, installed GuardAdaptive, cloudflared or GitHub. Separately stop the exact Node process bound to 127.0.0.1:48928 only after checking its command line, or leave the local-only backend for manual use. No automatic deletion of submitted volunteer records: follow 30-day retention and receipt-based deletion procedures.

## Still missing for real production and recruitment

- Independent external browser outside the user's tailnet confirms public DNS and HTTPS (the existing Cloudflare quick URL WAS tested through HTTPS from a second PC but this PC may also be on the tailnet).
- Published participant privacy notice, named data custodian/erasure contact, country/regional legal review, abuse control.
- Independently validated client-IP attribution to permit higher intake safely.
- A genuine drifted and known-good gamepad pilot with consent and physically verified movement/release.
- Server that remains online while the user's Windows PC sleeps/shuts down; separate Ubuntu host SSH remains unavailable.
- Original and shadow blind drift safety holdout gates remain FAILED. Production stays BLOCKED.
