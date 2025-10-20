# build_masm.ps1
# Build using CMake + Ninja for the Editor, keep cargo for masm

param(
    [string]$buildType = "release"
)

function Get-Platform {
    if ($IsWindows) { return "windows" }
    elseif ($IsLinux) { return "linux" }
    elseif ($IsMacOS) { return "macos" }
    else { throw "Unsupported platform" }
}

function Find-Exe {
    param($root, $name)
    $matches = Get-ChildItem -Path $root -Recurse -File -ErrorAction SilentlyContinue |
               Where-Object { $_.Name -eq $name -or $_.Name -eq "$name.exe" }
    return $matches | Select-Object -First 1
}

$platform = Get-Platform
$editorProj = "Editor"
$masmProj = "masm"
$buildDir = "build"
$tcDir = Join-Path $buildDir "tc"

# CMake config name for single-config generators
$cmakeCfg = if ($buildType -eq "release") { "Release" } else { "Debug" }

# Ensure build dirs
New-Item -ItemType Directory -Force -Path $buildDir | Out-Null
New-Item -ItemType Directory -Force -Path $tcDir | Out-Null

# Build Editor with CMake + Ninja
Write-Host "Configuring and building Editor with Ninja ($cmakeCfg)..."
$editorBuildDir = Join-Path $buildDir "editor"
New-Item -ItemType Directory -Force -Path $editorBuildDir | Out-Null

# Configure
cmake -S $editorProj -B $editorBuildDir -G "Ninja" -DCMAKE_BUILD_TYPE=$cmakeCfg

# Build
cmake --build $editorBuildDir --config $cmakeCfg

# Locate produced Editor binary (best-effort)
$editorExe = Find-Exe -root $editorBuildDir -name "Editor"
if (-not $editorExe) {
    # fallback locations
    if ($platform -eq "windows") {
        $editorPath = Join-Path $editorBuildDir "Editor.exe"
    } else {
        $editorPath = Join-Path $editorBuildDir "Editor"
    }
    if (Test-Path $editorPath) { $editorExe = Get-Item $editorPath }
}

if ($editorExe) {
    $dest = if ($platform -eq "windows") { Join-Path $buildDir "MasmEditor.exe" } else { Join-Path $buildDir "MasmEditor" }
    Copy-Item $editorExe.FullName $dest -Force
    Write-Host "Editor copied to $dest"
} else {
    Write-Warning "Could not find Editor binary in $editorBuildDir; adjust search or CMake targets."
}

# Build masm (Rust) via cargo as before
Write-Host "Building masm (cargo) for $platform..."
if ($buildType -eq "release") { cargo build --release } else { cargo build }

# Copy masm binary into tc dir
switch ($platform) {
    "windows" {
        $masmBin = "target\$buildType\masm.exe"
        Copy-Item $masmBin (Join-Path $tcDir "masm.exe") -Force
    }
    "linux" {
        $masmBin = "target/$buildType/masm"
        Copy-Item $masmBin (Join-Path $tcDir "masm.elf") -Force
    }
    "macos" {
        $masmBin = "target/$buildType/masm"
        Copy-Item $masmBin (Join-Path $tcDir "masm.dylib") -Force
    }
}

Write-Host "Build complete."