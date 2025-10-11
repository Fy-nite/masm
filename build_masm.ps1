# build_all.ps1

$buildType = "release"


function Get-Platform {
    if ($IsWindows) { return "windows" }
    elseif ($IsLinux) { return "linux" }
    elseif ($IsMacOS) { return "macos" }
    else { throw "Unsupported platform" }
}

$platform = Get-Platform
$editorProj = "Editor"
$masmProj = "masm"
$buildDir = "build"
$tcDir = "$buildDir/tc"

# Build Editor
Write-Host "Building Editor for $platform..."
cd $editorProj
if ($buildType -eq "release") { cargo build --release } else { cargo build }
cd $PSScriptRoot

# Build masm
Write-Host "Building masm for $platform..."
cd $masmProj
if ($buildType -eq "release") { cargo build --release } else { cargo build }
cd $PSScriptRoot

# Create output dirs
New-Item -ItemType Directory -Force -Path $buildDir | Out-Null
New-Item -ItemType Directory -Force -Path $tcDir | Out-Null

# Copy Editor binary
switch ($platform) {
    "windows" {
        $editorBin = "$editorProj/target/$buildType/Editor.exe"
        Copy-Item $editorBin "$buildDir/MasmEditor.exe" -Force
        $masmBin = "$masmProj/target/$buildType/masm.exe"
        Copy-Item $masmBin "$tcDir/masm.exe" -Force
    }
    "linux" {
        $editorBin = "$editorProj/target/$buildType/Editor"
        Copy-Item $editorBin "$buildDir/MasmEditor" -Force
        $masmBin = "$masmProj/target/$buildType/masm"
        Copy-Item $masmBin "$tcDir/masm.elf" -Force
    }
    "macos" {
        $editorBin = "$editorProj/target/$buildType/Editor"
        Copy-Item $editorBin "$buildDir/MasmEditor" -Force
        $masmBin = "$masmProj/target/$buildType/masm"
        Copy-Item $masmBin "$tcDir/masm.dylib" -Force
    }
}

Write-Host "Build complete."