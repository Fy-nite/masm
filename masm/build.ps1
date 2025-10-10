param(
    [string] $buildType = "release",
    [string] $targetArch = "x86_64",
    [string] $outputDir = "build",
    [string] $platform = "windows",
    [switch] $clean = $false
)
# if linux, cargo build --target x86_64-unknown-linux-gnu
# if windows, cargo build should be enough

if ($clean) {
    Write-Host "Cleaning previous build..."
    cargo clean
    if (Test-Path $outputDir) {
        Remove-Item -Recurse -Force $outputDir
    }
}
Write-Host "Building for $platform, $targetArch, $buildType..."
$env:TARGET = switch ($platform) {
    "linux" { "$targetArch-unknown-linux-gnu" }
    "windows" { "$targetArch-pc-windows-msvc" }
    default { throw "Unsupported platform: $platform" }
}
$env:OUTPUT_DIR = $outputDir
$buildCmd = "cargo build --target $env:TARGET"
if ($buildType -eq "release") {
    $buildCmd += " --release"
}
Invoke-Expression $buildCmd
if (!(Test-Path $outputDir)) {
    New-Item -ItemType Directory -Path $outputDir | Out-Null
}
$dirsToCopy = @("modules")
foreach ($dir in $dirsToCopy) {
    if (Test-Path $dir) {
        $destDir = Join-Path $outputDir $dir
        Copy-Item -Path $dir -Destination $destDir -Recurse -Force
    }
}
$binaryExt = switch ($platform) { "windows" { ".exe" } default { "" } }
$binaryName = "masm$binaryExt"
$sourcePath = "target\$env:TARGET\$buildType\$binaryName"
$destPath = Join-Path $outputDir $binaryName
Copy-Item -Path $sourcePath -Destination $destPath -Force
Write-Host "Build completed. Binary located at $destPath"
Write-Host "Build script finished."