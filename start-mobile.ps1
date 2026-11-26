$AppDir = "C:\Users\Hossam\.gemini\antigravity\scratch\watchlist"
$ExePath = Join-Path $AppDir "watchlist.exe"
if (-not (Test-Path $ExePath)) { $ExePath = "E:\watchlist-target\release\watchlist.exe" }
if (-not (Test-Path $ExePath)) { $ExePath = "E:\watchlist-target\debug\watchlist.exe" }

Set-Location $AppDir

$EnvFile = Join-Path $AppDir ".env"
if (Test-Path $EnvFile) {
  Get-Content $EnvFile | ForEach-Object {
    if ($_ -match '^\s*([^#=]+?)\s*=\s*(.*)\s*$') {
      [Environment]::SetEnvironmentVariable($matches[1], $matches[2], "Process")
    }
  }
}

if (-not $env:WATCHLIST_PASSWORD) {
  $pw = Read-Host "Enter a password for mobile access (saved to .env for next time)"
  if ([string]::IsNullOrWhiteSpace($pw)) { Write-Error "Password is required."; exit 1 }
  $env:WATCHLIST_PASSWORD = $pw
  "WATCHLIST_PASSWORD=$pw" | Out-File -FilePath $EnvFile -Encoding utf8
}

# Stop any local-only instance on port 3000 so mobile mode can bind 0.0.0.0:3000
Get-Process watchlist -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 400

$LanIp = (Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.InterfaceAlias -notmatch 'Loopback|vEthernet' -and $_.IPAddress -notlike '169.254.*' } | Select-Object -First 1 -ExpandProperty IPAddress)
Write-Host ""
Write-Host "Starting Watchlist for Mobile + PC..." -ForegroundColor Cyan
if ($LanIp) {
  Write-Host "  Home Wi-Fi URL : http://${LanIp}:3000" -ForegroundColor Green
}
Write-Host "  PC URL         : http://127.0.0.1:3000" -ForegroundColor Green
Write-Host ""

$CloudflaredCmd = Get-Command cloudflared -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
if (-not $CloudflaredCmd) {
  $CloudflaredCmd = @("C:\Program Files (x86)\cloudflared\cloudflared.exe", "C:\Program Files\cloudflared\cloudflared.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if ($CloudflaredCmd) {
  Start-Process -FilePath $ExePath -ArgumentList "--host","0.0.0.0","--port","3000" -WorkingDirectory $AppDir -WindowStyle Minimized
  Start-Sleep -Seconds 1
  Write-Host "Starting Cloudflare HTTPS Tunnel (open the https://*.trycloudflare.com URL on your phone)..." -ForegroundColor Yellow
  & $CloudflaredCmd tunnel --url http://127.0.0.1:3000
} else {
  Write-Host "Note: cloudflared not found; running on Home Wi-Fi only (install via: winget install Cloudflare.cloudflared)." -ForegroundColor Yellow
  & $ExePath --host 0.0.0.0 --port 3000
}
