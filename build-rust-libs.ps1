# Build Rust libraries for Windows and Android
# This script automates building the Rust FFI libraries needed by Flutter

param(
    [switch]$Windows,
    [switch]$Android,
    [switch]$All
)

$ErrorActionPreference = "Stop"

# Colors for output
$BLUE = "`e[0;34m"
$GREEN = "`e[0;32m"
$YELLOW = "`e[1;33m"
$RED = "`e[0;31m"
$NC = "`e[0m" # No Color

function Write-ColorOutput($ForegroundColor, $Message) {
    $fc = switch ($ForegroundColor) {
        "Blue" { $BLUE }
        "Green" { $GREEN }
        "Yellow" { $YELLOW }
        "Red" { $RED }
        default { $NC }
    }
    Write-Host "$fc$Message$NC"
}

function Convert-ToUnixPath($Path) {
    if (-not $Path) { return $Path }
    return ($Path -replace '\\', '/')
}

# Get project root (script is in root, so use script directory)
$PROJECT_ROOT = $PSScriptRoot
$CRATES_DIR = Join-Path $PROJECT_ROOT "crates"
$APP_DIR = Join-Path $PROJECT_ROOT "app"
$env:CARGO_INCREMENTAL = "0"

# Check for cargo
$CARGO = "$env:USERPROFILE\.cargo\bin\cargo.exe"
if (-not (Test-Path $CARGO)) {
    Write-ColorOutput "Red" "❌ Cargo not found at $CARGO"
    Write-ColorOutput "Yellow" "   Please install Rust from https://rustup.rs"
    exit 1
}

# Set up OpenSSL for Windows builds
$OPENSSL_DIR = $env:OPENSSL_DIR
if (-not $OPENSSL_DIR) {
    $OPENSSL_DIR = "$env:USERPROFILE\OpenSSL-Win64"
}
if (Test-Path $OPENSSL_DIR) {
    $env:OPENSSL_DIR = $OPENSSL_DIR
    $env:OPENSSL_ROOT_DIR = $OPENSSL_DIR
    # OpenSSL libraries are in VC\x64\MD for release builds (Multi-threaded DLL)
    # Use MD for release, MT for static linking
    $OPENSSL_LIB_SUBDIR = "lib\VC\x64\MD"
    if (-not $env:OPENSSL_LIB_DIR) {
        $env:OPENSSL_LIB_DIR = Join-Path $OPENSSL_DIR $OPENSSL_LIB_SUBDIR
    }
    if (-not $env:OPENSSL_INCLUDE_DIR) {
        $env:OPENSSL_INCLUDE_DIR = Join-Path $OPENSSL_DIR "include"
    }
    $env:OPENSSL_NO_VENDOR = "1"
    # Add OpenSSL lib directory to PATH for linker
    $env:PATH = "$env:PATH;$OPENSSL_DIR\bin"
    Write-ColorOutput "Green" "✅ Found OpenSSL at $OPENSSL_DIR"
    Write-ColorOutput "Blue" "   Setting OPENSSL_LIB_DIR=$env:OPENSSL_LIB_DIR"
    Write-ColorOutput "Blue" "   Setting OPENSSL_INCLUDE_DIR=$env:OPENSSL_INCLUDE_DIR"
} else {
    Write-ColorOutput "Yellow" "⚠️  OpenSSL not found at $OPENSSL_DIR"
    Write-ColorOutput "Yellow" "   Attempting to build without explicit OpenSSL path..."
}

# Build Windows DLL
if ($Windows -or $All) {
    Write-ColorOutput "Blue" "🪟 Building Rust library for Windows..."
    
    Push-Location $CRATES_DIR
    try {
        & $CARGO build --release --target x86_64-pc-windows-msvc --package pirate-ffi-frb --features frb --no-default-features --locked
        
        if ($LASTEXITCODE -eq 0) {
            # DLL is in target/x86_64-pc-windows-msvc/release/deps/ for cdylib
            $DLL_SOURCE = Join-Path $CRATES_DIR "target\x86_64-pc-windows-msvc\release\deps\pirate_ffi_frb.dll"
            # Also check the direct release folder
            if (-not (Test-Path $DLL_SOURCE)) {
                $DLL_SOURCE = Join-Path $CRATES_DIR "target\x86_64-pc-windows-msvc\release\pirate_ffi_frb.dll"
            }
            $DLL_DEST = Join-Path $APP_DIR "build\windows\x64\runner\Release\pirate_ffi_frb.dll"
            
            if (Test-Path $DLL_SOURCE) {
                $DLL_DEST_DIR = Split-Path -Parent $DLL_DEST
                if (-not (Test-Path $DLL_DEST_DIR)) {
                    New-Item -ItemType Directory -Path $DLL_DEST_DIR -Force | Out-Null
                }
                Copy-Item $DLL_SOURCE $DLL_DEST -Force
                Write-ColorOutput "Green" "✅ Windows DLL built and copied to: $DLL_DEST"
            } else {
                Write-ColorOutput "Red" "❌ DLL not found. Searched:"
                Write-ColorOutput "Red" "   - $DLL_SOURCE"
                Write-ColorOutput "Yellow" "   Searching for DLL..."
                $found = Get-ChildItem (Join-Path $CRATES_DIR "target\x86_64-pc-windows-msvc\release") -Recurse -Filter "pirate_ffi_frb.dll" -ErrorAction SilentlyContinue | Select-Object -First 1
                if ($found) {
                    Write-ColorOutput "Green" "   Found at: $($found.FullName)"
                    $DLL_DEST_DIR = Split-Path -Parent $DLL_DEST
                    if (-not (Test-Path $DLL_DEST_DIR)) {
                        New-Item -ItemType Directory -Path $DLL_DEST_DIR -Force | Out-Null
                    }
                    Copy-Item $found.FullName $DLL_DEST -Force
                    Write-ColorOutput "Green" "✅ Windows DLL copied to: $DLL_DEST"
                }
            }
        } else {
            Write-ColorOutput "Red" "❌ Windows build failed"
            Pop-Location
            exit 1
        }
    } finally {
        Pop-Location
    }
}

# Build Android libraries
if ($Android -or $All) {
    Write-ColorOutput "Blue" "Building Rust libraries for Android (PowerShell)..."

    # Ensure MSYS2 tools are on PATH for perl/make
    $msysBin = "C:\\msys64\\usr\\bin"
    if (Test-Path $msysBin) {
        $env:PATH = "$msysBin;$env:PATH"
    }

    # Find Android NDK
    $ANDROID_SDK = $env:ANDROID_SDK_ROOT
    if (-not $ANDROID_SDK) {
        $ANDROID_SDK = "$env:LOCALAPPDATA\\Android\\sdk"
    }
    $NDK_PATH = $null
    if (Test-Path "$ANDROID_SDK\\ndk") {
        $ndkVersions = Get-ChildItem "$ANDROID_SDK\\ndk" -Directory | Sort-Object Name -Descending
        if ($ndkVersions) {
            $NDK_PATH = $ndkVersions[0].FullName
        }
    }
    if (-not $NDK_PATH -and (Test-Path "$ANDROID_SDK\\ndk-bundle")) {
        $NDK_PATH = "$ANDROID_SDK\\ndk-bundle"
    }
    if (-not $NDK_PATH) {
        Write-ColorOutput "Red" "Android NDK not found."
        exit 1
    }
    Write-ColorOutput "Green" "Found Android NDK at: $NDK_PATH"

    $ndkBin = Join-Path $NDK_PATH "toolchains\\llvm\\prebuilt\\windows-x86_64\\bin"
    $ndkBinUnix = Convert-ToUnixPath $ndkBin
    $env:ANDROID_NDK_HOME = $NDK_PATH
    $env:ANDROID_NDK_ROOT = $NDK_PATH
    $env:MSYS2_ARG_CONV_EXCL = "*"
    $env:MSYS_NO_PATHCONV = "1"

    # Linkers: use NDK target wrappers (ELF mode)
    $env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = Join-Path $ndkBin "aarch64-linux-android21-clang.cmd"
    $env:CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER = Join-Path $ndkBin "armv7a-linux-androideabi21-clang.cmd"
    $env:CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER = Join-Path $ndkBin "x86_64-linux-android21-clang.cmd"

    # OpenSSL build helpers (used by openssl-sys)
    $clangUnix = "$ndkBinUnix/clang.exe"
    $llvmArUnix = "$ndkBinUnix/llvm-ar.exe"
    $llvmRanlibUnix = "$ndkBinUnix/llvm-ranlib.exe"
    $env:CC = $clangUnix
    $env:AR = $llvmArUnix
    $env:RANLIB = $llvmRanlibUnix
    $env:CC_aarch64_linux_android = $clangUnix
    $env:CC_armv7_linux_androideabi = $clangUnix
    $env:CC_x86_64_linux_android = $clangUnix
    Set-Item -Path "env:CC_aarch64-linux-android" -Value $clangUnix
    Set-Item -Path "env:CC_armv7-linux-androideabi" -Value $clangUnix
    Set-Item -Path "env:CC_x86_64-linux-android" -Value $clangUnix
    $env:AR_aarch64_linux_android = $llvmArUnix
    $env:AR_armv7_linux_androideabi = $llvmArUnix
    $env:AR_x86_64_linux_android = $llvmArUnix
    Set-Item -Path "env:AR_aarch64-linux-android" -Value $llvmArUnix
    Set-Item -Path "env:AR_armv7-linux-androideabi" -Value $llvmArUnix
    Set-Item -Path "env:AR_x86_64-linux-android" -Value $llvmArUnix
    $env:RANLIB_aarch64_linux_android = $llvmRanlibUnix
    $env:RANLIB_armv7_linux_androideabi = $llvmRanlibUnix
    $env:RANLIB_x86_64_linux_android = $llvmRanlibUnix
    Set-Item -Path "env:RANLIB_aarch64-linux-android" -Value $llvmRanlibUnix
    Set-Item -Path "env:RANLIB_armv7-linux-androideabi" -Value $llvmRanlibUnix
    Set-Item -Path "env:RANLIB_x86_64-linux-android" -Value $llvmRanlibUnix
    $env:CFLAGS_aarch64_linux_android = "--target=aarch64-linux-android21"
    $env:CFLAGS_armv7_linux_androideabi = "--target=armv7a-linux-androideabi21"
    $env:CFLAGS_x86_64_linux_android = "--target=x86_64-linux-android21"

    $ARCHS = @(
        @{ Rust = "aarch64-linux-android"; ABI = "arm64-v8a" },
        @{ Rust = "armv7-linux-androideabi"; ABI = "armeabi-v7a" },
        @{ Rust = "x86_64-linux-android"; ABI = "x86_64" }
    )

    $RUSTUP = "$env:USERPROFILE\\.cargo\\bin\\rustup.exe"
    if (Test-Path $RUSTUP) {
        & $RUSTUP target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
    } elseif (Get-Command rustup -ErrorAction SilentlyContinue) {
        rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
    }

    Push-Location $CRATES_DIR
    try {
        foreach ($arch in $ARCHS) {
            Write-ColorOutput "Blue" "Building for $($arch.Rust) ($($arch.ABI))..."
            & $CARGO build --release --target $arch.Rust --package pirate-ffi-frb --features frb --no-default-features --locked
            if ($LASTEXITCODE -ne 0) {
                Write-ColorOutput "Red" "Android build failed for $($arch.Rust)"
                exit 1
            }
            $SO_SOURCE = Join-Path $CRATES_DIR "target\\$($arch.Rust)\\release\\libpirate_ffi_frb.so"
            if (-not (Test-Path $SO_SOURCE)) {
                Write-ColorOutput "Red" "Library not found at $SO_SOURCE"
                exit 1
            }
            $DEST_DIR = Join-Path $APP_DIR "android\\app\\src\\main\\jniLibs\\$($arch.ABI)"
            if (-not (Test-Path $DEST_DIR)) {
                New-Item -ItemType Directory -Path $DEST_DIR -Force | Out-Null
            }
            Copy-Item $SO_SOURCE $DEST_DIR -Force
            Write-ColorOutput "Green" "Copied to: $DEST_DIR"
        }
    } finally {
        Pop-Location
    }
}
Write-ColorOutput "Green" "✅ Build complete!"




