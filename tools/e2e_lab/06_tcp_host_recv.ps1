# E2E-6: fake host — TCP listen and print LASER lines + short HEX (replay sink later)
param(
  [string]$BindHost = "127.0.0.1",
  [int]$Port = 58102,
  [int]$Seconds = 12
)
$ErrorActionPreference = "Stop"

function Format-HexPreview([byte[]]$bytes, [int]$max = 24) {
  $n = [Math]::Min($bytes.Length, $max)
  if ($n -le 0) { return "" }
  ($bytes[0..($n - 1)] | ForEach-Object { $_.ToString("X2") }) -join " "
}

$listener = New-Object System.Net.Sockets.TcpListener ([System.Net.IPAddress]::Parse($BindHost), $Port)
try {
  $listener.Start()
} catch {
  Write-Host "FAIL: cannot bind TCP ${BindHost}:${Port} — $($_.Exception.Message)"
  Write-Host "Hint: check netsh excludedportrange protocol=tcp; try another free port."
  exit 1
}
Write-Host "[E2E-6] fake-host TCP listen ${BindHost}:${Port} for ${Seconds}s (waiting accept)"
$deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
$got = 0
$client = $null
$stream = $null
$buf = New-Object byte[] 4096
$pending = ""
try {
  while ([DateTime]::UtcNow -lt $deadline -and $null -eq $client) {
    if ($listener.Pending()) {
      $client = $listener.AcceptTcpClient()
      $stream = $client.GetStream()
      $stream.ReadTimeout = 1000
      Write-Host "[E2E-6] accepted from $($client.Client.RemoteEndPoint)"
    } else {
      Start-Sleep -Milliseconds 50
    }
  }
  if ($null -eq $client) {
    Write-Host "FAIL: no TCP connect within ${Seconds}s - start 06_start_tcp_probe.bat"
    exit 1
  }
  while ([DateTime]::UtcNow -lt $deadline) {
    try {
      $n = $stream.Read($buf, 0, $buf.Length)
      if ($n -le 0) { Start-Sleep -Milliseconds 50; continue }
      $chunk = New-Object byte[] $n
      [Array]::Copy($buf, $chunk, $n)
      $pending += [Text.Encoding]::UTF8.GetString($chunk)
      while ($pending.Contains("`n")) {
        $idx = $pending.IndexOf("`n")
        $line = $pending.Substring(0, $idx).TrimEnd("`r")
        $pending = $pending.Substring($idx + 1)
        if ($line.Length -eq 0) { continue }
        $got++
        $lineBytes = [Text.Encoding]::UTF8.GetBytes($line)
        $hex = Format-HexPreview $lineBytes
        Write-Host "recv #$got text=$line"
        Write-Host "         hex=$hex$(if ($lineBytes.Length -gt 24) { ' ...' })"
      }
    } catch [System.IO.IOException] {
    } catch [System.Net.Sockets.SocketException] {
    }
  }
} finally {
  if ($null -ne $stream) { $stream.Close() }
  if ($null -ne $client) { $client.Close() }
  $listener.Stop()
}
Write-Host "[E2E-6] TCP host done, lines=$got"
if ($got -lt 3) {
  Write-Host "FAIL: expected at least 3 LASER lines"
  exit 1
}
Write-Host "PASS"
exit 0
