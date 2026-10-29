# Installs the latest skills-sync release on Windows:
#   irm https://github.com/alexcrisan1611/skills-sync/releases/latest/download/install.ps1 | iex
& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'

    $base = 'https://github.com/alexcrisan1611/skills-sync/releases/latest/download'
    # x64 builds also run on ARM64 under emulation, but use the native one when we can tell.
    $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64' -or $env:PROCESSOR_ARCHITEW6432 -eq 'ARM64') { 'aarch64' } else { 'x86_64' }
    $name = "skills-sync-$arch-pc-windows-msvc"

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid())
    New-Item -ItemType Directory $tmp | Out-Null
    try {
        Write-Host "Downloading $name..."
        Invoke-WebRequest "$base/$name.zip" -OutFile "$tmp\$name.zip" -UseBasicParsing
        Invoke-WebRequest "$base/SHA256SUMS" -OutFile "$tmp\SHA256SUMS" -UseBasicParsing

        $line = Get-Content "$tmp\SHA256SUMS" | Where-Object { $_ -match " $([regex]::Escape("$name.zip"))$" }
        $expected = if ($line) { ($line -split ' ')[0] } else { '' }
        $actual = (Get-FileHash "$tmp\$name.zip" -Algorithm SHA256).Hash
        if (-not $expected -or $expected -ne $actual.ToLower()) { throw "checksum mismatch for $name.zip" }

        Expand-Archive "$tmp\$name.zip" $tmp
        & "$tmp\$name\skills-sync.exe" install @args
        if ($LASTEXITCODE -ne 0) { throw "skills-sync install failed (exit code $LASTEXITCODE)" }
    } finally {
        Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
}
