param(
    [string]$SourcePng = "app\\assets\\icons\\p-logo-url-no-bg.png"
)

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$srcPath = Join-Path $root $SourcePng

if (-not (Test-Path $srcPath)) {
    throw "Source PNG not found: $srcPath"
}

$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Drawing

function New-SquareImage {
    param([System.Drawing.Image]$Source)

    $size = [Math]::Min($Source.Width, $Source.Height)
    $x = [Math]::Floor(($Source.Width - $size) / 2)
    $y = [Math]::Floor(($Source.Height - $size) / 2)
    $rect = New-Object System.Drawing.Rectangle($x, $y, $size, $size)

    $square = New-Object System.Drawing.Bitmap -ArgumentList @(
        $size,
        $size,
        [System.Drawing.Imaging.PixelFormat]::Format32bppArgb
    )
    $gfx = [System.Drawing.Graphics]::FromImage($square)
    $gfx.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
    $gfx.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $gfx.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $gfx.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $gfx.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $destRect = New-Object System.Drawing.Rectangle(0, 0, $size, $size)
    $gfx.DrawImage($Source, $destRect, $rect, [System.Drawing.GraphicsUnit]::Pixel)
    $gfx.Dispose()

    return $square
}

function Save-ResizedPng {
    param(
        [System.Drawing.Image]$Source,
        [int]$Size,
        [string]$Path
    )
    $dir = Split-Path $Path
    if (-not (Test-Path $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }

    $bmp = New-Object System.Drawing.Bitmap -ArgumentList @(
        $Size,
        $Size,
        [System.Drawing.Imaging.PixelFormat]::Format32bppArgb
    )
    $gfx = [System.Drawing.Graphics]::FromImage($bmp)
    $gfx.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
    $gfx.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $gfx.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $gfx.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $gfx.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $gfx.DrawImage($Source, 0, 0, $Size, $Size)
    $gfx.Dispose()

    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
}

function Get-PngBytes {
    param(
        [System.Drawing.Image]$Source,
        [int]$Size
    )
    $bmp = New-Object System.Drawing.Bitmap -ArgumentList @(
        $Size,
        $Size,
        [System.Drawing.Imaging.PixelFormat]::Format32bppArgb
    )
    $gfx = [System.Drawing.Graphics]::FromImage($bmp)
    $gfx.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
    $gfx.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $gfx.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $gfx.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $gfx.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $gfx.DrawImage($Source, 0, 0, $Size, $Size)
    $gfx.Dispose()

    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    return $ms.ToArray()
}

function Write-Ico {
    param(
        [array]$Images,
        [string]$Path
    )
    $ms = New-Object System.IO.MemoryStream
    $bw = New-Object System.IO.BinaryWriter($ms)

    $bw.Write([UInt16]0)
    $bw.Write([UInt16]1)
    $bw.Write([UInt16]$Images.Count)

    $offset = 6 + (16 * $Images.Count)
    $entries = @()

    foreach ($item in $Images) {
        $size = [int]$item.Size
        $bytes = [byte[]]$item.Bytes
        $entries += [pscustomobject]@{
            Width = $(if ($size -ge 256) { 0 } else { $size })
            Height = $(if ($size -ge 256) { 0 } else { $size })
            ColorCount = 0
            Reserved = 0
            Planes = 1
            BitCount = 32
            BytesInRes = $bytes.Length
            Offset = $offset
            Bytes = $bytes
        }
        $offset += $bytes.Length
    }

    foreach ($e in $entries) {
        $bw.Write([Byte]$e.Width)
        $bw.Write([Byte]$e.Height)
        $bw.Write([Byte]$e.ColorCount)
        $bw.Write([Byte]$e.Reserved)
        $bw.Write([UInt16]$e.Planes)
        $bw.Write([UInt16]$e.BitCount)
        $bw.Write([UInt32]$e.BytesInRes)
        $bw.Write([UInt32]$e.Offset)
    }

    foreach ($e in $entries) {
        $bw.Write($e.Bytes)
    }

    $dir = Split-Path $Path
    if (-not (Test-Path $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }

    [System.IO.File]::WriteAllBytes($Path, $ms.ToArray())
    $bw.Dispose()
    $ms.Dispose()
}

$srcImage = [System.Drawing.Image]::FromFile($srcPath)
try {
    $square = New-SquareImage -Source $srcImage

    $macDir = Join-Path $root "app\\macos\\Runner\\Assets.xcassets\\AppIcon.appiconset"
    $macMap = @{
        16 = "app_icon_16.png"
        32 = "app_icon_32.png"
        64 = "app_icon_64.png"
        128 = "app_icon_128.png"
        256 = "app_icon_256.png"
        512 = "app_icon_512.png"
        1024 = "app_icon_1024.png"
    }
    foreach ($entry in $macMap.GetEnumerator()) {
        Save-ResizedPng -Source $square -Size $entry.Key -Path (Join-Path $macDir $entry.Value)
    }

    $iosDir = Join-Path $root "app\\ios\\Runner\\Assets.xcassets\\AppIcon.appiconset"
    $iosTargets = @(
        @{ Size = 20; File = "Icon-App-20x20@1x.png" },
        @{ Size = 40; File = "Icon-App-20x20@2x.png" },
        @{ Size = 60; File = "Icon-App-20x20@3x.png" },
        @{ Size = 29; File = "Icon-App-29x29@1x.png" },
        @{ Size = 58; File = "Icon-App-29x29@2x.png" },
        @{ Size = 87; File = "Icon-App-29x29@3x.png" },
        @{ Size = 40; File = "Icon-App-40x40@1x.png" },
        @{ Size = 80; File = "Icon-App-40x40@2x.png" },
        @{ Size = 120; File = "Icon-App-40x40@3x.png" },
        @{ Size = 120; File = "Icon-App-60x60@2x.png" },
        @{ Size = 180; File = "Icon-App-60x60@3x.png" },
        @{ Size = 76; File = "Icon-App-76x76@1x.png" },
        @{ Size = 152; File = "Icon-App-76x76@2x.png" },
        @{ Size = 167; File = "Icon-App-83.5x83.5@2x.png" },
        @{ Size = 1024; File = "Icon-App-1024x1024@1x.png" }
    )
    foreach ($target in $iosTargets) {
        Save-ResizedPng -Source $square -Size $target.Size -Path (Join-Path $iosDir $target.File)
    }

    $androidDir = Join-Path $root "app\\android\\app\\src\\main\\res"
    $androidMap = @{
        "mipmap-mdpi\\ic_launcher.png" = 48
        "mipmap-hdpi\\ic_launcher.png" = 72
        "mipmap-xhdpi\\ic_launcher.png" = 96
        "mipmap-xxhdpi\\ic_launcher.png" = 144
        "mipmap-xxxhdpi\\ic_launcher.png" = 192
    }
    foreach ($entry in $androidMap.GetEnumerator()) {
        Save-ResizedPng -Source $square -Size $entry.Value -Path (Join-Path $androidDir $entry.Key)
    }

    $windowsIco = Join-Path $root "app\\windows\\runner\\resources\\app_icon.ico"
    $icoSizes = @(16, 24, 32, 48, 64, 128, 256)
    $icoImages = @()
    foreach ($size in $icoSizes) {
        $icoImages += [pscustomobject]@{ Size = $size; Bytes = (Get-PngBytes -Source $square -Size $size) }
    }
    Write-Ico -Images $icoImages -Path $windowsIco
} finally {
    if ($square) { $square.Dispose() }
    $srcImage.Dispose()
}

Write-Host "Icons generated from $srcPath"
