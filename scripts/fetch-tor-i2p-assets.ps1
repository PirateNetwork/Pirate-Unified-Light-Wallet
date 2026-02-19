param(
    [string]$TorBrowserVersion = $(if ($env:TOR_BROWSER_VERSION) { $env:TOR_BROWSER_VERSION } else { "15.0.5" }),
    [string]$TorBrowserBaseUrl = $(if ($env:TOR_BROWSER_BASE_URL) { $env:TOR_BROWSER_BASE_URL } else { "https://dist.torproject.org/torbrowser/$TorBrowserVersion" }),
    [string]$TorBrowserFile = $(if ($env:TOR_BROWSER_WINDOWS_FILE) { $env:TOR_BROWSER_WINDOWS_FILE } else { "tor-browser-windows-x86_64-portable-$TorBrowserVersion.exe" }),
    [string]$TorBrowserSha256 = $(if ($env:TOR_BROWSER_WINDOWS_SHA256) { $env:TOR_BROWSER_WINDOWS_SHA256 } else { "15448e951583b624c3f8fdfa8bc55fa9b65e1bcafd474f3f2dfd5444e4178846" }),
    [string]$I2pdVersion = $(if ($env:I2PD_VERSION) { $env:I2PD_VERSION } else { "2.59.0" }),
    [string]$I2pdBaseUrl = $env:I2PD_BASE_URL,
    [string]$I2pdSha512 = $(if ($env:I2PD_WINDOWS_SHA512) { $env:I2PD_WINDOWS_SHA512 } else { "c5cae4b2b2166935f1bed9f302f5647e3c201c784a9cf7c4a605ca47906b57358ffe3ce6b45762d4857ce060299e17e1a3ea70f8f1e3f72472b87ea7bb96d0b5" })
)

if ($env:SKIP_TOR_I2P_FETCH -eq "1") {
    Write-Host "[INFO] Skipping Tor/I2P asset fetch (SKIP_TOR_I2P_FETCH=1)."
    exit 0
}

if (-not $I2pdBaseUrl) {
    $I2pdBaseUrl = "https://github.com/PurpleI2P/i2pd/releases/download/$I2pdVersion"
}

$TorPtSource = if ($env:TOR_PT_SOURCE) { $env:TOR_PT_SOURCE } else { "auto" }
$SnowflakeRepoUrl = if ($env:SNOWFLAKE_REPO_URL) { $env:SNOWFLAKE_REPO_URL } else { "https://gitlab.torproject.org/tpo/anti-censorship/pluggable-transports/snowflake.git" }
$SnowflakeRef = if ($env:SNOWFLAKE_REF) { $env:SNOWFLAKE_REF } else { "v2.11.0" }
$SnowflakeCommit = if ($env:SNOWFLAKE_COMMIT) { $env:SNOWFLAKE_COMMIT } else { "6472bd86cdd5d13fe61dc851edcf83b03df7bda1" }
$Obfs4RepoUrl = if ($env:OBFS4_REPO_URL) { $env:OBFS4_REPO_URL } else { "https://gitlab.com/yawning/obfs4.git" }
$Obfs4Ref = if ($env:OBFS4_REF) { $env:OBFS4_REF } else { "obfs4proxy-0.0.14" }
$Obfs4Commit = if ($env:OBFS4_COMMIT) { $env:OBFS4_COMMIT } else { "336a71d6e4cfd2d33e9c57797828007ad74975e9" }

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

function Normalize-Mode {
    param([string]$Mode)
    if (-not $Mode) { return "auto" }
    return $Mode.ToLowerInvariant()
}

function Use-TorPtSource {
    param([string]$Mode)
    switch ($Mode) {
        "1" { return $true }
        "true" { return $true }
        "yes" { return $true }
        "on" { return $true }
        "auto" { return $true }
        "only" { return $true }
        default { return $false }
    }
}

function TorPtSourceOnly {
    param([string]$Mode)
    return $Mode -eq "only"
}

function Ensure-Repo {
    param(
        [string]$Url,
        [string]$Ref,
        [string]$Commit,
        [string]$Dest
    )
    if (-not (Test-Path (Join-Path $Dest ".git"))) {
        Write-Host "[INFO] Cloning $Url"
        git clone $Url $Dest | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to clone $Url"
        }
    }
    git -C $Dest fetch --depth 1 origin $Commit | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to fetch $Commit from $Url"
    }
    git -C $Dest checkout -q $Commit | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to checkout $Commit in $Dest"
    }
    $head = (git -C $Dest rev-parse HEAD).Trim()
    if ($head -ne $Commit) {
        throw "Expected $Ref ($Commit) but found $head in $Dest"
    }
}

function Build-TorPtFromSource {
    $mode = Normalize-Mode -Mode $TorPtSource
    if (-not (Use-TorPtSource -Mode $mode)) {
        return $false
    }
    $goCmd = Get-Command go -ErrorAction SilentlyContinue
    if (-not $goCmd) {
        Write-Warning "Go not found; skipping Tor PT source build."
        return $false
    }
    $gitCmd = Get-Command git -ErrorAction SilentlyContinue
    if (-not $gitCmd) {
        Write-Warning "Git not found; skipping Tor PT source build."
        return $false
    }

    $buildRoot = Join-Path $TorPtDir ".build"
    $snowflakeRepo = Join-Path $buildRoot "snowflake"
    $obfs4Repo = Join-Path $buildRoot "obfs4"
    Ensure-Dir $buildRoot
    Ensure-Repo -Url $SnowflakeRepoUrl -Ref $SnowflakeRef -Commit $SnowflakeCommit -Dest $snowflakeRepo
    Ensure-Repo -Url $Obfs4RepoUrl -Ref $Obfs4Ref -Commit $Obfs4Commit -Dest $obfs4Repo

    $snowflakeDest = Join-Path $TorPtDir "windows\\snowflake-client.exe"
    $obfs4Dest = Join-Path $TorPtDir "windows\\obfs4proxy.exe"
    Remove-Item -Force -ErrorAction SilentlyContinue $snowflakeDest, $obfs4Dest

    $env:CGO_ENABLED = "0"
    $env:GOWORK = "off"
    $env:GOOS = "windows"
    $env:GOARCH = "amd64"
    $goLdFlags = "-s -w -buildid= -H=windowsgui"

    Write-Host "[INFO] Building snowflake-client (GOOS=windows GOARCH=amd64)"
    Push-Location $snowflakeRepo
    & go build -mod=readonly -buildvcs=false -trimpath -ldflags $goLdFlags -o $snowflakeDest ./client
    if ($LASTEXITCODE -ne 0) {
        Pop-Location
        throw "snowflake-client build failed with exit code $LASTEXITCODE"
    }
    Pop-Location
    if (-not (Test-Path $snowflakeDest)) {
        throw "Failed to build snowflake-client.exe"
    }

    Write-Host "[INFO] Building obfs4proxy (GOOS=windows GOARCH=amd64)"
    Push-Location $obfs4Repo
    & go build -mod=readonly -buildvcs=false -trimpath -ldflags $goLdFlags -o $obfs4Dest ./obfs4proxy
    if ($LASTEXITCODE -ne 0) {
        Pop-Location
        throw "obfs4proxy build failed with exit code $LASTEXITCODE"
    }
    Pop-Location
    if (-not (Test-Path $obfs4Dest)) {
        throw "Failed to build obfs4proxy.exe"
    }

    return $true
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

$torSourceBuilt = $false
try {
    if (Build-TorPtFromSource) {
        Write-Host "[INFO] Tor pluggable transports built from source."
        $torSourceBuilt = $true
    }
} catch {
    Write-Warning $_.Exception.Message
}

$modeNormalized = Normalize-Mode -Mode $TorPtSource
if ($torSourceBuilt) {
    Write-Host "[INFO] Tor/I2P assets downloaded."
    exit 0
}
if (TorPtSourceOnly -Mode $modeNormalized) {
    throw "Tor PT source build requested but failed. Install Go and Git or set TOR_PT_SOURCE=off."
}

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
    if (-not $snowflake) {
        $snowflake = Get-ChildItem -Path $Root -Recurse -File -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^snowflake.*$' } | Select-Object -First 1
    }
    $obfs4 = Get-ChildItem -Path $Root -Recurse -Filter "obfs4proxy.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $obfs4) {
        $obfs4 = Get-ChildItem -Path $Root -Recurse -File -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^obfs4.*\.exe$' } | Select-Object -First 1
    }
    if (-not $obfs4) {
        $obfs4 = Get-ChildItem -Path $Root -Recurse -File -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^obfs4.*$' } | Select-Object -First 1
    }
    return @{ Snowflake = $snowflake; Obfs4 = $obfs4 }
}

function Get-ArchiveCandidates {
    param([string]$Root)
    Get-ChildItem -Path $Root -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '\.(7z|zip|tar|xz|lzma)$' }
}

function Extract-PluggableTransportsFromArchive {
    param([string]$Archive, [string]$DestDir)
    Ensure-Dir $DestDir
    $snowflakePath = $null
    $obfs4Path = $null
    $listing = & $sevenZip l -slt $Archive 2>$null
    if ($LASTEXITCODE -eq 0) {
        foreach ($line in $listing) {
            if ($line -match '^Path = (.+)$') {
                $path = $Matches[1]
                if (-not $snowflakePath -and $path -match '(?i)snowflake-client(\.exe)?$') {
                    $snowflakePath = $path
                }
                if (-not $obfs4Path -and $path -match '(?i)obfs4proxy(\.exe)?$') {
                    $obfs4Path = $path
                }
            }
            if ($snowflakePath -and $obfs4Path) { break }
        }
        if ($snowflakePath) {
            & $sevenZip e $Archive "-o$DestDir" -y $snowflakePath | Out-Null
        }
        if ($obfs4Path) {
            & $sevenZip e $Archive "-o$DestDir" -y $obfs4Path | Out-Null
        }
    } else {
        & $sevenZip e $Archive "-o$DestDir" -y "*snowflake*" "*obfs4*" | Out-Null
    }
    $snowflake = Get-ChildItem -Path $DestDir -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^snowflake.*(\.exe)?$' } | Select-Object -First 1
    $obfs4 = Get-ChildItem -Path $DestDir -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^obfs4.*(\.exe)?$' } | Select-Object -First 1
    return @{ Snowflake = $snowflake; Obfs4 = $obfs4 }
}

$found = Find-PluggableTransports -Root $torExtract
$snowflakeBin = $found.Snowflake
$obfs4Bin = $found.Obfs4

if (-not $snowflakeBin -or -not $obfs4Bin) {
    $archiveQueue = New-Object "System.Collections.Generic.Queue[System.IO.FileInfo]"
    $seenArchives = New-Object "System.Collections.Generic.HashSet[string]"
    foreach ($archive in (Get-ArchiveCandidates -Root $torExtract)) {
        if ($seenArchives.Add($archive.FullName)) {
            $archiveQueue.Enqueue($archive)
        }
    }
    $maxArchiveExtractions = 25
    $extractedCount = 0
    while ($archiveQueue.Count -gt 0 -and (-not $snowflakeBin -or -not $obfs4Bin)) {
        if ($extractedCount -ge $maxArchiveExtractions) {
            break
        }
        $archive = $archiveQueue.Dequeue()
        $nestedDir = Join-Path $torExtract ("nested-" + [Guid]::NewGuid().ToString("N"))
        Ensure-Dir $nestedDir
        & $sevenZip x $archive.FullName "-o$nestedDir" -y | Out-Null
        if ($LASTEXITCODE -ne 0) {
            continue
        }
        $extractedCount++
        $nestedFound = Find-PluggableTransports -Root $nestedDir
        if ($nestedFound.Snowflake -and $nestedFound.Obfs4) {
            $snowflakeBin = $nestedFound.Snowflake
            $obfs4Bin = $nestedFound.Obfs4
            break
        }
        foreach ($more in (Get-ArchiveCandidates -Root $nestedDir)) {
            if ($seenArchives.Add($more.FullName)) {
                $archiveQueue.Enqueue($more)
            }
        }
    }
}

if (-not $snowflakeBin -or -not $obfs4Bin) {
    $directDir = Join-Path $torExtract "direct"
    $archives = Get-ArchiveCandidates -Root $torExtract
    $archives = @($torArchive) + $archives
    foreach ($archive in $archives) {
        $extracted = Extract-PluggableTransportsFromArchive -Archive $archive -DestDir $directDir
        if ($extracted.Snowflake -and $extracted.Obfs4) {
            $snowflakeBin = $extracted.Snowflake
            $obfs4Bin = $extracted.Obfs4
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
