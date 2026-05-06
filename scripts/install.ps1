$ErrorActionPreference = "Stop"

$BaseUrl = if ($env:CLEARMESH_BASE_URL) { $env:CLEARMESH_BASE_URL } else { "https://clearmesh.net/releases/latest" }
$InstallDir = if ($env:CLEARMESH_INSTALL_DIR) { $env:CLEARMESH_INSTALL_DIR } else { Join-Path $HOME ".clearmesh\bin" }

$Name = "clearmesh-windows-x86_64"
$Archive = "$Name.zip"
$ShaFile = "$Archive.sha256"

$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("clearmesh-install-" + [System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Force -Path $Tmp | Out-Null

try {
  Write-Host "Downloading ClearMesh CLI..."
  Invoke-WebRequest -Uri "$BaseUrl/$Archive" -OutFile (Join-Path $Tmp $Archive)
  Invoke-WebRequest -Uri "$BaseUrl/$ShaFile" -OutFile (Join-Path $Tmp $ShaFile)

  Write-Host "Verifying checksum..."
  $Expected = (Get-Content (Join-Path $Tmp $ShaFile) | Select-Object -First 1).Split(" ")[0].Trim().ToLowerInvariant()
  $Actual = (Get-FileHash -Algorithm SHA256 (Join-Path $Tmp $Archive)).Hash.ToLowerInvariant()

  if ($Expected -ne $Actual) {
    throw "Checksum mismatch. Expected $Expected but got $Actual"
  }

  Write-Host "Installing..."
  Expand-Archive -Path (Join-Path $Tmp $Archive) -DestinationPath $Tmp -Force

  $Candidate = Get-ChildItem -Path $Tmp -Recurse -File |
    Where-Object { $_.Name -eq "clearmesh.exe" -or $_.Name -eq "clearmesh-cli.exe" } |
    Select-Object -First 1

  if (-not $Candidate) {
    Write-Host ""
    Write-Host "Archive contents:"
    Get-ChildItem -Path $Tmp -Recurse | ForEach-Object { Write-Host $_.FullName }
    throw "Could not find clearmesh.exe or clearmesh-cli.exe in the downloaded archive."
  }

  New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

  $DestExe = Join-Path $InstallDir "clearmesh.exe"
  Copy-Item $Candidate.FullName $DestExe -Force

  Write-Host ""
  Write-Host "Installed clearmesh -> $DestExe"

  $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
  if (-not $UserPath) { $UserPath = "" }

  $PathParts = $UserPath -split ";" | Where-Object { $_ -ne "" }
  if ($PathParts -notcontains $InstallDir) {
    Write-Host ""
    Write-Host "Adding install directory to your User PATH..."
    $NewPath = if ($UserPath) { "$UserPath;$InstallDir" } else { "$InstallDir" }
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    Write-Host "PATH updated. Open a new PowerShell window before running clearmesh from PATH."
  }

  Write-Host ""
  Write-Host "Next:"
  Write-Host "  & `"$HOME\.clearmesh\bin\clearmesh.exe`" --help"
  Write-Host "  clearmesh config set-api https://api.clearmesh.net"
  Write-Host "  clearmesh --help"
}
finally {
  Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}
