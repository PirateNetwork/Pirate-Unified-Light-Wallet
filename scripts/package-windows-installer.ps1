param(
    [Parameter(Mandatory)][string]$SourceDir,
    [Parameter(Mandatory)][string]$OutputDir,
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$AppVersion,
    [string]$OutputBaseFilename = 'Stashi-Wallet-windows-installer-unsigned',
    [string]$IsccPath = 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe'
)
$ErrorActionPreference = 'Stop'
$SourceDir = (Resolve-Path -LiteralPath $SourceDir).Path
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
$OutputDir = (Resolve-Path -LiteralPath $OutputDir).Path
if (-not (Test-Path -LiteralPath $IsccPath)) { throw 'Install Inno Setup 6.5 or newer, or supply IsccPath.' }
if (-not (Test-Path -LiteralPath (Join-Path $SourceDir 'Stashi Wallet.exe'))) { throw 'Wallet executable missing.' }

# Each optional executable is visible as its own release asset. Hashes are
# compiled into the authenticated installer; never fetch an unpinned latest URL.
$components = @(
    @{ Path = 'i2p\i2pd.exe'; Component = 'i2p' },
    @{ Path = 'tor-pt\snowflake-client.exe'; Component = 'bridges' },
    @{ Path = 'tor-pt\obfs4proxy.exe'; Component = 'bridges' }
)
$entries = foreach ($component in $components) {
    $source = Join-Path $SourceDir $component.Path
    if (-not (Test-Path -LiteralPath $source)) { throw "Missing privacy component: $($component.Path)" }
    $name = Split-Path $source -Leaf
    $asset = "Stashi-Wallet-windows-component-$name"
    $destination = Join-Path $OutputDir $asset
    Copy-Item -LiteralPath $source -Destination $destination -Force
    $hash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $asset" | Set-Content -LiteralPath "$destination.sha256" -Encoding ascii
    $size = (Get-Item -LiteralPath $destination).Length
    $folder = Split-Path $component.Path -Parent
    $url = "https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet/releases/download/v$AppVersion/$asset"
    "Source: `"$url`"; DestDir: `"{app}\$folder`"; DestName: `"$name`"; ExternalSize: $size; Hash: `"$hash`"; Components: $($component.Component); Flags: external download ignoreversion"
}
$include = Join-Path $OutputDir 'windows-components.iss'
$entries | Set-Content -LiteralPath $include -Encoding utf8
& $IsccPath "/DSourceDir=$SourceDir" "/DOutputDir=$OutputDir" "/DOutputBaseFilename=$OutputBaseFilename" "/DAppVersion=$AppVersion" "/DComponentsFile=$include" (Join-Path $PSScriptRoot 'windows-installer.iss')
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed: $LASTEXITCODE" }
