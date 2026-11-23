# Deploying Watchlist (PC, Home Wi-Fi, & Always-On Mobile Access)

Watchlist is designed to run anywhere: locally on Windows, on your home network, or on a small always-on cloud server with **HTTPS** and **Android PWA** installation.

---

## Option 1: Always-On Cloud Hosting (Works When Your PC Is Off)

Running Watchlist on a small VPS (~$4–$6/month, or Oracle Cloud Free Tier) with Docker and automatic HTTPS gives you 24/7 access from your Android phone, Windows PC, or any browser.

### 1. Prepare Your Server
On an Ubuntu/Debian VPS with Docker installed:
```bash
git clone https://github.com/SamDozer/watchlist.git
cd watchlist
mkdir -p data
```

### 2. Copy Your Existing Local Database
From your Windows PC (so you keep all your videos and watched/queue progress):
```powershell
scp C:\Users\Hossam\.gemini\antigravity\scratch\watchlist\data\watchlist.db user@YOUR_SERVER_IP:~/watchlist/data/watchlist.db
```

### 3. Configure Environment Secrets
Create a `.env` file on the server (never committed to git):
```bash
WATCHLIST_PASSWORD=choose-a-strong-password
WATCHLIST_SESSION_SECRET=replace-with-32-plus-random-characters
WATCHLIST_SECURE_COOKIE=1
```

### 4. Start Watchlist with Persistent Storage
```bash
docker compose up -d --build
```
The SQLite database persists in `./data/watchlist.db` on the host, surviving container restarts and upgrades.

### 5. Enable HTTPS (Required for Android PWA Install & Secure Cookies)
Using **Caddy** (automatic free Let's Encrypt TLS) on the host:
```text
watchlist.yourdomain.com {
    reverse_proxy 127.0.0.1:3000
}
```
*(Or use a free **Cloudflare Tunnel** pointing `watchlist.yourdomain.com` → `http://127.0.0.1:3000` with zero open firewall ports.)*

---

## Option 2: Home Wi-Fi or Tailscale (Using Your Windows PC)

If your PC is on and you want to access it from your phone without a VPS:

```powershell
$env:WATCHLIST_PASSWORD = "choose-a-strong-password"
watchlist.exe --host 0.0.0.0 --port 3000
```
*(Watchlist refuses to bind to non-localhost addresses unless `WATCHLIST_PASSWORD` is set.)*

- **Home Wi-Fi:** Open `http://<YOUR_PC_LAN_IP>:3000` on your phone (allow TCP port `3000` in Windows Defender Firewall for Private networks).
- **Anywhere via Tailscale:** Install [Tailscale](https://tailscale.com/) on PC and Android, then open `http://<YOUR_PC_TAILSCALE_IP>:3000` over 4G/5G without exposing any ports.

---

## Installing as an Android PWA

1. Open your HTTPS Watchlist URL in **Android Chrome**.
2. Sign in with your `WATCHLIST_PASSWORD` (session stays active for 30 days via `HttpOnly` cookie).
3. Tap **⋮ (Menu) → Install app** (or **Add to Home screen**).
4. Watchlist launches full-screen like a native app, and clicking **Watch** opens videos directly in the YouTube app.
