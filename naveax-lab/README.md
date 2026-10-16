# GuardAdaptive Remote Lab v0.1.3

Stable HTTPS public pilot via Tailscale Funnel (v0.1.6, three-button UI, manual Gamepad API axis mapping, isolated browser-only filter preview and consent-gated survey, separately provisioned secret and submission storage) is now configured for the existing Naveax Windows device: https://naveax.tail10f02d.ts.net/. The node server runs on 127.0.0.1:48928 with a pinned HTTPS Origin, an independent per-user scheduled task checks the backend every 60 seconds after interactive login, and an existing Tailscale Funnel 443-to-localhost mapping is configured with --bg to survive daemon restarts. A second Windows computer in the same tailnet successfully accessed the HTTPS site and submitted a 600-sample synthetic record (201). **Access from a browser outside this tailnet and uninterrupted 24/7 hosting have NOT yet been independently proven**. The prior Cloudflare Quick Tunnel remains available separately; it was not disturbed. Do not recruit real public volunteers at scale without validating external DNS from outside the tailnet, data erasure contacts, and rate limiting. See deploy/windows/README.md.

Preview records go to the isolated `preview-feedback` subdirectory and are explicitly opt-in; the demo is a fixed radial deadzone visualization, not the GuardAdaptive live algorithm or injected game input. See docs/REMOTE_LAB_PREVIEW_SURVEY_V016.md. The stable 48928 site now starts with three choices (healthy, regular drift, severe drift). Its self-reported drift severity is recorded separately without changing existing submissions or the Cloudflare 48927 interface. See docs/REMOTE_LAB_SIMPLE_UI_V015.md. The stable backend does NOT trust forwarded IP headers, so all clients share a conservative 8-successful-uploads-per-hour limiter; model training/promotion remains BLOCKED. The 48928 stable backend now uses %LOCALAPPDATA%\\GuardAdaptiveRemoteLabStable\\ for its own private key and submissions, separate from the existing 48927 Cloudflare pilot. See deploy/windows/README.md. Reboot-after-logon and true out-of-tailnet public connectivity remain unverified.

# GuardAdaptive Remote Lab v0.1.2

**Temporary public HTTPS pilot verified (2026-10-15):** A Cloudflare Quick Tunnel connected the existing Windows localhost service to a publicly accessible HTTPS address. Both the origin PC and a second Windows PC retrieved `/api/status` over HTTPS. An explicit-consent synthetic sample upload received HTTP 201 through the public tunnel; authenticated retrieval/deletion and anonymous HTTP 401 were verified. The public origin is ephemeral, NOT an uptime-guaranteed or domain-owned production service. Detailed operations, active locally stored secret/data locations, and limitations: `docs/REMOTE_LAB_PUBLIC_PILOT_V012.md`.

In Quick Tunnel mode, set REMOTE_LAB_TRUST_CF_TUNNEL=enabled and the exact REMOTE_LAB_PUBLIC_ORIGIN=https://current-temporary-hostname, plus REMOTE_LAB_ADMIN_TOKEN_FILE=path-to-private-token-file, REMOTE_LAB_DATA_DIR=private-data-directory and REMOTE_LAB_UPLOADS=enabled. The tunnel must connect from loopback. Cloudflare's CF-Connecting-IP is used ONLY for short-lived RAM rate limits, never written into submitted JSON. The Windows origin binds only to 127.0.0.1 and exposes no inbound LAN port. For permanent hosting, use a named Cloudflare Tunnel with authorized account/domain or restore standalone Ubuntu access.

The GA-RL-01 browser data schema remains compatible with client version 0.1.0.

# GuardAdaptive Remote Lab v0.1.1

v0.1.1 adds read-only offline session-quality review; GA-RL-01 recordings continue using the compatible client data schema version 0.1.0.

A privacy-first, controller-free-research **website** for volunteers in other cities. Participants connect a controller to **their own** Windows/macOS/Linux PC and run a guided analog-axis observation using the browser Gamepad API. There is no software installation for participants. The website never changes controller values, drives games, calibrates a controller, or attempts to authenticate the participant's physical intent.

**Repository status:** This is a research collection prototype, NOT GuardAdaptive production, NOT a validated drift correction model. Active installed GuardAdaptive v0.34.0 and all driver/game paths stay unchanged. Real physical-controller testing has not yet occurred.

## How to run locally (Naveax)

Requirements: Node.js 22+. No npm packages, external fonts, telemetry services or database required.

From a PowerShell terminal:

    cd C:\Users\navea\source\GuardAdaptive\remote-lab
    $env:REMOTE_LAB_PORT='48927'
    node server.cjs

Visit:
- Participant website: http://127.0.0.1:48927/
- Administrator screen: http://127.0.0.1:48927/admin

**Important:** Port 8787 is already used by the Nexowire local service. Remote Lab deliberately uses **48927**, never disturb or rebind the Nexowire service.

By default, **uploads are disabled**. Participants can still run measurements and download their JSON locally; nothing is automatically sent.

## Participant flow

1. Open the HTTPS website on a desktop computer. Pair a gamepad via USB or Bluetooth. In some browsers, press a controller button while the page is focused.
2. Select the detected gamepad and self-report Xbox/PlayStation/Nintendo/other, whether drift is suspected, and transport (USB/Bluetooth/unknown).
3. Acknowledge this is research rather than verified physical intent and explicitly start.
4. Six guided 12-second phases, each with a 3-second preparation countdown:
   - Left stick untouched / left small intentional movements / left full sweep.
   - Right stick untouched / right small intentional movements / right full sweep.
5. View baseline left/right RMS magnitude and total captured samples, and export JSON. Measurements use the browser's reported standard axis values [LX, LY, RX, RY]. Browsers may already normalize controller hardware readings; this is NOT firmware-level raw ADC data.
6. If the server explicitly enables collection, optionally tick the **separate upload consent** and send the dataset. There is no upload without this second action.

Sampling targets approximately 60 samples per second but depends on frame scheduling. On tab hide or controller disconnect, the test aborts and throws away partial results. Browser hardware IDs may appear in the participant's local selection dropdown, but are **never included in uploaded datasets**.

The six self-described phases are NOT independently verified contact/release ground truth. False claims, very low quality/mismapped hardware or browser-processed inputs are expected and must be handled in later model research.

## Recorded schema (GA-RL-01)

    {
      "schema":"ga-remote-lab-v1",
      "protocol":"GA-RL-01",
      "session_id":"random UUID v4",
      "controller":{"family":"xbox","condition":"drift","transport":"usb"},
      "client":{"mapping":"standard","version":"0.1.0"},
      "consent":{"information_ack":true,"upload":true},
      "samples":[[0,3017,0.012,-0.008,0.000,0.000]]
    }

Each six-element sample is [phase index 0..5, milliseconds since start, LX, LY, RX, RY]. There is no name, city, GPS, persistent user ID, device ID, user-agent string, email, IP address or hardware serial in stored JSON.

The server enforces an exact allowlisted schema, finite normalized axes, strict monotonic time, six ordered completed phases, minimum per-phase duration/sample counts, a max of 9000 samples and 850000 bytes per body. Self-report remains unverified. Analysis RMS is a descriptive statistic, NOT a drift diagnosis or auto-calibration recommendation.

## Enable uploads after deployment preparation

**Do not enable internet uploads until you have a public HTTPS site, explicit consent notice, data storage, anti-abuse and administration configured.** For a local integration test only:

    $env:REMOTE_LAB_PORT='48927'
    $env:REMOTE_LAB_UPLOADS='enabled'
    $env:REMOTE_LAB_DATA_DIR='D:\GuardAdaptiveRemoteLabData'
    $env:REMOTE_LAB_ADMIN_TOKEN=[Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
    node server.cjs

Never commit real secrets or data files. Use a cryptographically random admin token such as a freshly generated 32-byte hex value.

### Internet hosting on an already managed standalone Ubuntu host

**2026-10-15 deployment status: NOT DEPLOYED.** Previously known Ubuntu VM SSH address 192.168.68.61:22 timed out. Nexowire Windows shell can see Hyper-V cmdlets but does not have permission to enumerate the VM. The existing ReVoltVPN host may be using nginx STREAM/Xray on HTTPS port 443. Do not use a second Caddy/nginx 443 listener or modify any VPN configuration without verifying the current topology. See deploy/README.md, deploy/check-ubuntu-prereqs.sh and dedicated service templates. A live domain, TLS certificate and physical participant pilot are NOT confirmed.

In proxy mode, the application strictly requires a preconfigured HTTPS REMOTE_LAB_PUBLIC_ORIGIN when accepting uploads; it binds only to localhost and refuses startup when uploads are on but the admin token is missing/too short.

1. Copy ONLY the remote-lab/ directory to the server, excluding data/, private keys and local .env files.
2. Run Node 22+ as a dedicated unprivileged service, listening on **127.0.0.1:48927**.
3. Reverse proxy an owned public HTTPS domain (e.g. lab.example.org) to 127.0.0.1:48927 using Caddy or nginx. Keep the Node port unreachable from the internet. TLS certificates, renewal, DNS and firewall are the host administrator's responsibility.
4. Ensure the proxy preserves the public Host header, sets the forwarding header X-Real-IP from the real remote client address itself, and **discards client-supplied X-Real-IP**. Only then set REMOTE_LAB_TRUST_LOOPBACK_PROXY=enabled. The Node server trusts forwarding headers **only from loopback**.
5. Disable proxy access/IP logs or establish a separate retention/privacy policy; the Node app itself does not write IP logs. Proxy and hosting providers may still process network metadata.
6. Set REMOTE_LAB_PUBLIC_ORIGIN to the exact verified HTTPS origin (e.g. https://lab.example.org), REMOTE_LAB_UPLOADS=enabled only after a safe pilot, and a strong admin token. In production use the separate systemd LoadCredential=admin_token secret file rather than putting the secret in an environment variable. A dedicated DynamicUser with private data directory and restart policy is recommended. Back up only with a documented retention policy.
7. Test publicly on HTTPS using a real volunteer's STANDARD 4-axis controller. Do a harmless upload with explicit consent before inviting participants.

Example Caddyfile template (replace the domain):

    lab.example.org {
        encode zstd gzip
        reverse_proxy 127.0.0.1:48927 {
            header_up Host {host}
            header_up X-Real-IP {remote_host}
        }
    }

Configure Caddy logs and privacy policy separately. Do not turn on REMOTE_LAB_TRUST_LOOPBACK_PROXY unless the proxy sanitizes X-Real-IP. A Caddy/HTTPS reverse proxy needs a domain or a managed TLS configuration; this repository does not automatically register a domain or expose the machine.

For pure static hosting, the three participant-page files can be served via an HTTPS static service, but **uploads and admin/API are unavailable** until a compatible backend is configured. Browser-side download still works.

## Admin panel

At https://YOUR-DOMAIN/admin enter the strong server admin token in the browser. The token stays only in memory while that tab is open. You can view summaries, download individual JSON sessions, or permanently delete them. The public site has no upload privilege to read or list other people's sessions.

API (use HTTPS in production):
- GET /api/status (public, does not expose secrets)
- POST /api/submissions (requires user consent, same origin, strictly validated data)
- GET /api/admin/sessions (admin Bearer)
- GET /api/admin/sessions/:uuid (admin Bearer, raw samples)
- DELETE /api/admin/sessions/:uuid (admin Bearer, permanent deletion)

Rate/capacity: 8 successful submissions/hour/source IP in memory; 2000 files or 256 MB data directory; automatic cleanup around 30 days (on uploads, admin listing, startup and hourly). IPs are never stored in submitted JSON; only short-lived in-memory hash keys are used for rate limiting.

These are baseline protections, not complete DDoS/bot defense. For a heavily publicized service, add reverse-proxy-level rate limits/anti-bot challenges and monitoring. No external cloud service is configured by this source release.

## Local validation

    node --test test\remote-lab.test.cjs

Tests start real localhost HTTP servers on random ports, verify strict validation/consent, origin/cross-site rejection, protected administrator access/list/delete, simultaneous rate-limit handling, proxy identity sanitization and disabled-by-default uploads.

The UI was visually inspected using an isolated headless Edge browser on the authorized Naveax PC. A real physical drifted controller was **not** part of these tests; such a trial is a separate milestone.

## Offline research dataset review (never trains a model)

After valid, opt-in submissions have been exported from the administrator UI, scan a folder of recorded sessions locally. This command DOES NOT upload or change them:

    node review-sessions.cjs D:\GuardAdaptiveRemoteLabData D:\GuardAdaptiveReports\remote-lab-triage.json

The output must be a NEW file outside the input directory. The reviewer checks strict GA-RL-01 schema and phase timing, sample gaps/rate, suspiciously static sweeps, coverage of stick directions and duplicate sample streams under different UUIDs. It quarantines bad inputs and never overwrites reports. The analysis is heuristic, not a medical/device diagnosis or proof that a real human followed instructions. All recordings remain unverified, require manual review and are automatically INELIGIBLE for model training or blind-holdout admission. The administrator UI shows heuristic warnings without granting physical trust. See docs/REMOTE_LAB_QUALITY_V1.md.

## Data governance / do not train automatically

Never feed uploads directly to the live drift decision model. Human-guided phase labels are unverified; independent dataset screening, per-controller family balancing, collection-quality checks, labeled controls, session leakage prevention, blinded holdouts and physical user-intent validation are still required. Original and shadow blind holdout quality gates in GuardAdaptive have FAILED; model promotion and production remain BLOCKED.

A participant can request record removal by providing their **receipt UUID** to the data custodian. The admin can delete it via /admin. Do not request participants' real names or personal contact details to use this website.
