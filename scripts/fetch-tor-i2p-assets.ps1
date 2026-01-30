param(
    [string]$TorBrowserVersion = $(if ($env:TOR_BROWSER_VERSION) { $env:TOR_BROWSER_VERSION } else { "15.0.5" }),
    [string]$TorBrowserBaseUrl = $(if ($env:TOR_BROWSER_BASE_URL) { $env:TOR_BROWSER_BASE_URL } else { "https://dist.torproject.org/torbrowser/$TorBrowserVersion" }),
    [string]$TorBrowserFile = $(if ($env:TOR_BROWSER_WINDOWS_FILE) { $env:TOR_BROWSER_WINDOWS_FILE } else { "tor-browser-windows-x86_64-portable-$TorBrowserVersion.exe" }),
    [string]$TorBrowserSha256 = $(if ($env:TOR_BROWSER_WINDOWS_SHA256) { $env:TOR_BROWSER_WINDOWS_SHA256 } else { "15448e951583b624c3f8fdfa8bc55fa9b65e1bcafd474f3f2dfd5444e4178846" }),
    [string]$I2pdVersion = $(if ($env:I2PD_VERSION) { $env:I2PD_VERSION } else { "2.58.0" }),
    [string]$I2pdBaseUrl = $env:I2PD_BASE_URL,
    [string]$I2pdSha512 = $(if ($env:I2PD_WINDOWS_SHA512) { $env:I2PD_WINDOWS_SHA512 } else { "e0cdfa9e416f9580fd57a5466cd37f8a8d067b3602fa826b820bdbfa631a8bbbfa20a484db9641aab9855ac2799667b475fb5fe92cd1f11e26eef469279315a1" })
)

if ($env:SKIP_TOR_I2P_FETCH -eq "1") {
    Write-Host "[INFO] Skipping Tor/I2P asset fetch (SKIP_TOR_I2P_FETCH=1)."
    exit 0
}

if (-not $I2pdBaseUrl) {
    $I2pdBaseUrl = "https://github.com/PurpleI2P/i2pd/releases/download/$I2pdVersion"
}

$TorBrowserUrl = if ($env:TOR_BROWSER_WINDOWS_URL) {
    $env:TOR_BROWSER_WINDOWS_URL
} elseif ($env:TOR_BROWSER_URL) {
    $env:TOR_BROWSER_URL
} else {
    "$TorBrowserBaseUrl/$TorBrowserFile"
}

if (-not $TorBrowserSha256) {
    throw "Missing TOR_BROWSER_WINDOWS_SHA256. Pin the Tor Browser checksum for reproducible builds."
}
if (-not $I2pdSha512) {
    throw "Missing I2PD_WINDOWS_SHA512. Pin the i2pd checksum for reproducible builds."
}

$ProjectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$AppDir = Join-Path $ProjectRoot "app"
$I2pDir = Join-Path $AppDir "i2p"
$TorPtDir = Join-Path $AppDir "tor-pt"

function Ensure-Dir {
    param([string]$Path)
    if (-not (Test-Path $Path)) {
        New-Item -ItemType Directory -Path $Path | Out-Null
    }
}

function Download-File {
    param([string]$Url, [string]$Destination)
    Write-Host "[INFO] Downloading $Url"
    Invoke-WebRequest -Uri $Url -OutFile $Destination -UseBasicParsing
}

function Check-Hash {
    param(
        [string]$Path,
        [string]$Expected,
        [string]$Algorithm = "SHA256"
    )
    if (-not $Expected) {
        throw "Missing expected $Algorithm hash for $Path"
    }
    $hash = (Get-FileHash -Algorithm $Algorithm -Path $Path).Hash.ToLowerInvariant()
    if ($hash -ne $Expected.ToLowerInvariant()) {
        throw "$Algorithm mismatch for $Path"
    }
}

function Find-SevenZip {
    if ($env:SEVEN_ZIP_PATH -and (Test-Path $env:SEVEN_ZIP_PATH)) {
        return $env:SEVEN_ZIP_PATH
    }
    $cmd = Get-Command 7z.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $candidates = @(
        "C:\\Program Files\\7-Zip\\7z.exe",
        "C:\\Program Files (x86)\\7-Zip\\7z.exe"
    )
    foreach ($path in $candidates) {
        if (Test-Path $path) { return $path }
    }
    return $null
}

Ensure-Dir $I2pDir
Ensure-Dir $TorPtDir
Ensure-Dir (Join-Path $I2pDir "windows")
Ensure-Dir (Join-Path $TorPtDir "windows")

$i2pdZip = "i2pd_${I2pdVersion}_win64_mingw.zip"
$i2pdUrl = "$I2pdBaseUrl/$i2pdZip"
$i2pdTmp = Join-Path $env:TEMP $i2pdZip
Write-Host "[INFO] Downloading i2pd: $i2pdUrl"
Download-File -Url $i2pdUrl -Destination $i2pdTmp
Check-Hash -Path $i2pdTmp -Expected $I2pdSha512 -Algorithm "SHA512"

$i2pdExtract = Join-Path $env:TEMP ("i2pd-extract-" + [Guid]::NewGuid().ToString("N"))
Ensure-Dir $i2pdExtract
Expand-Archive -Path $i2pdTmp -DestinationPath $i2pdExtract -Force
$i2pdExe = Get-ChildItem -Path $i2pdExtract -Recurse -Filter "i2pd.exe" | Select-Object -First 1
if (-not $i2pdExe) {
    throw "i2pd.exe not found in $i2pdZip"
}
Copy-Item -Path $i2pdExe.FullName -Destination (Join-Path $I2pDir "windows\\i2pd.exe") -Force
Remove-Item -Recurse -Force $i2pdExtract
Remove-Item -Force $i2pdTmp

$torTmp = Join-Path $env:TEMP ("torbrowser-" + [Guid]::NewGuid().ToString("N"))
Ensure-Dir $torTmp
$torArchive = Join-Path $torTmp $TorBrowserFile
Write-Host "[INFO] Downloading Tor Browser bundle: $TorBrowserUrl"
Download-File -Url $TorBrowserUrl -Destination $torArchive
Check-Hash -Path $torArchive -Expected $TorBrowserSha256 -Algorithm "SHA256"

$sevenZip = Find-SevenZip
if (-not $sevenZip) {
    throw "7-Zip is required to extract Tor Browser on Windows. Install it or set SEVEN_ZIP_PATH."
}

$torExtract = Join-Path $torTmp "extract"
Ensure-Dir $torExtract
& $sevenZip x $torArchive "-o$torExtract" -y | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "Failed to extract Tor Browser bundle with 7-Zip"
}

$snowflakeDest = Join-Path $TorPtDir "windows\\snowflake-client.exe"
$obfs4Dest = Join-Path $TorPtDir "windows\\obfs4proxy.exe"

function Find-PluggableTransports {
    param([string]$Root)
    $snowflake = Get-ChildItem -Path $Root -Recurse -Filter "snowflake-client.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $snowflake) {
        $snowflake = Get-ChildItem -Path $Root -Recurse -File -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^snowflake.*\.exe$' } | Select-Object -First 1
    }
    $obfs4 = Get-ChildItem -Path $Root -Recurse -Filter "obfs4proxy.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $obfs4) {
        $obfs4 = Get-ChildItem -Path $Root -Recurse -File -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^obfs4.*\.exe$' } | Select-Object -First 1
    }
    return @{ Snowflake = $snowflake; Obfs4 = $obfs4 }
}

$found = Find-PluggableTransports -Root $torExtract
$snowflakeBin = $found.Snowflake
$obfs4Bin = $found.Obfs4

if (-not $snowflakeBin -or -not $obfs4Bin) {
    $nestedArchives = Get-ChildItem -Path $torExtract -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^(app|browser|torbrowser).*\.7z$' }
    foreach ($archive in $nestedArchives) {
        $nestedDir = Join-Path $torExtract ("nested-" + [Guid]::NewGuid().ToString("N"))
        Ensure-Dir $nestedDir
        & $sevenZip x $archive.FullName "-o$nestedDir" -y | Out-Null
        if ($LASTEXITCODE -ne 0) {
            continue
        }
        $nestedFound = Find-PluggableTransports -Root $nestedDir
        if ($nestedFound.Snowflake -and $nestedFound.Obfs4) {
            $snowflakeBin = $nestedFound.Snowflake
            $obfs4Bin = $nestedFound.Obfs4
            break
        }
    }
}

if (-not $snowflakeBin -or -not $obfs4Bin) {
    throw "Pluggable transports not found in Tor Browser bundle."
}

Copy-Item -Path $snowflakeBin.FullName -Destination $snowflakeDest -Force
Copy-Item -Path $obfs4Bin.FullName -Destination $obfs4Dest -Force

Remove-Item -Recurse -Force $torTmp

Write-Host "[INFO] Tor/I2P assets downloaded."
