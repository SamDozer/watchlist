$ErrorActionPreference = "SilentlyContinue"
$AppDir = "C:\Users\Hossam\.gemini\antigravity\scratch\watchlist"
$ExePath = Join-Path $AppDir "watchlist.exe"
if (-not (Test-Path $ExePath)) { $ExePath = "E:\watchlist-target\release\watchlist.exe" }
if (-not (Test-Path $ExePath)) { $ExePath = "E:\watchlist-target\debug\watchlist.exe" }

function Test-Health {
  try {
    $r = Invoke-WebRequest -Uri "http://127.0.0.1:3000/health" -UseBasicParsing -TimeoutSec 1
    return ($r.StatusCode -eq 200)
  } catch {
    return $false
  }
}

if (-not (Test-Health)) {
  Start-Process -FilePath $ExePath -ArgumentList "--port","3000" -WorkingDirectory $AppDir -WindowStyle Hidden
  for ($i = 0; $i -lt 25; $i++) {
    if (Test-Health) { break }
    Start-Sleep -Milliseconds 200
  }
}

$Chrome = "C:\Program Files\Google\Chrome\Application\chrome.exe"
if (Test-Path $Chrome) {
  Start-Process -FilePath $Chrome -ArgumentList "--app=http://127.0.0.1:3000"
} else {
  Start-Process "http://127.0.0.1:3000"
}
