<#
.SYNOPSIS
    Signs the agent-guidance Windows executable with Authenticode SHA-256 and verifies fingerprint.
.PARAMETER Target
    Path to agent-guidance.exe (default: target/release/agent-guidance.exe)
.PARAMETER CertificatePath
    Optional path to custom PFX certificate file.
.PARAMETER TimestampServer
    RFC-3161 Timestamp Server (default: http://timestamp.digicert.com)
#>
param (
    [string]$Target = "target/release/agent-guidance.exe",
    [string]$CertificatePath = "",
    [string]$CertificateBase64 = "",
    [string]$CertificatePassword = "",
    [string]$TimestampServer = "http://timestamp.digicert.com",
    [switch]$EnrollRoot = $false
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $Target)) {
    if (-not $PSBoundParameters.ContainsKey('Target') -and (Test-Path -LiteralPath "target/debug/agent-guidance.exe")) {
        $Target = "target/debug/agent-guidance.exe"
    } else {
        Write-Error "Target executable not found: $Target. Run 'cargo build --release' first."
        exit 1
    }
}

function Load-PfxCertificate {
    param (
        [Parameter(Mandatory = $true)][string]$FilePath,
        [string]$Password = ""
    )
    $resolvedPfx = (Resolve-Path -LiteralPath $FilePath).Path
    $flags = [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::Exportable
    if ($Password) {
        return New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($resolvedPfx, $Password, $flags)
    }
    return New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($resolvedPfx, [string]$null, $flags)
}

$fullPath = (Resolve-Path -LiteralPath $Target).Path
Write-Host "=== Agent Guidance Authenticode Signing & Fingerprint ===" -ForegroundColor Cyan
Write-Host "Target: $fullPath"

$cert = $null
$b64 = if ($CertificateBase64) { $CertificateBase64 } else { $env:WINDOWS_CERT_BASE64 }
$pass = if ($CertificatePassword) { $CertificatePassword } else { $env:WINDOWS_CERT_PASSWORD }

if ($b64) {
    Write-Host "Loading certificate from base64 secret..." -ForegroundColor Cyan
    $pfxBytes = [Convert]::FromBase64String($b64)
    $tmpPfx = Join-Path ([System.IO.Path]::GetTempPath()) "signing-$([System.Guid]::NewGuid().ToString('N')).pfx"
    [System.IO.File]::WriteAllBytes($tmpPfx, $pfxBytes)
    try {
        $cert = Load-PfxCertificate -FilePath $tmpPfx -Password $pass
    } finally {
        Remove-Item -LiteralPath $tmpPfx -Force -ErrorAction SilentlyContinue
    }
} elseif ($CertificatePath) {
    if (-not (Test-Path -LiteralPath $CertificatePath)) {
        Write-Error "Certificate file not found: $CertificatePath"
        exit 1
    }
    Write-Host "Loading certificate from file: $CertificatePath"
    $cert = Load-PfxCertificate -FilePath $CertificatePath -Password $pass
} else {
    # Check CurrentUser\My store for Jun Mystery Code Signing certificate
    $certs = @(Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert | Where-Object { $_.Subject -like "*CN=Jun Mystery*" })
    if ($certs.Count -gt 0) {
        $cert = $certs[0]
        Write-Host "Using existing certificate: $($cert.Thumbprint)"
    } else {
        Write-Host "Creating trusted self-signed Code Signing certificate for Jun Mystery..." -ForegroundColor Yellow
        $cert = New-SelfSignedCertificate `
            -Type CodeSigningCert `
            -Subject "CN=Jun Mystery, O=Jun Mystery, OU=Agent Guidance, E=darkzeuslk@gmail.com" `
            -CertStoreLocation "Cert:\CurrentUser\My" `
            -KeyUsage DigitalSignature `
            -KeyLength 2048 `
            -NotAfter (Get-Date).AddYears(5) `
            -FriendlyName "Jun Mystery Agent Guidance Code Signing"

        # Enroll public key into TrustedPublisher for machine trust
        $pubStore = New-Object System.Security.Cryptography.X509Certificates.X509Store("TrustedPublisher", "CurrentUser")
        try {
            $pubStore.Open([System.Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
            $pubStore.Add($cert)
        } finally {
            $pubStore.Close()
        }

        Write-Host "Enrolled certificate into CurrentUser\TrustedPublisher store." -ForegroundColor Green
    }

    if ($EnrollRoot) {
        try {
            $rootStore = New-Object System.Security.Cryptography.X509Certificates.X509Store("Root", "CurrentUser")
            try {
                $rootStore.Open([System.Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
                $rootStore.Add($cert)
            } finally {
                $rootStore.Close()
            }
            Write-Host "Enrolled certificate into CurrentUser\Root store." -ForegroundColor Green
        } catch {
            Write-Warning "Could not enroll into Root store: $_"
        }
    }
}

Write-Host "Signing executable with SHA-256 and timestamping..."
try {
    $sig = Set-AuthenticodeSignature -FilePath $fullPath -Certificate $cert -TimestampServer $TimestampServer -HashAlgorithm SHA256
} catch {
    Write-Warning "Timestamp server unreachable; signing without timestamp..."
    $sig = Set-AuthenticodeSignature -FilePath $fullPath -Certificate $cert -HashAlgorithm SHA256
}

$verify = Get-AuthenticodeSignature -FilePath $fullPath
$hashSha256 = (Get-FileHash -LiteralPath $fullPath -Algorithm SHA256).Hash
$hashSha512 = (Get-FileHash -LiteralPath $fullPath -Algorithm SHA512).Hash

$statusColor = if ($verify.Status -eq "Valid") { "Green" } else { "Yellow" }
Write-Host "`n[VERIFICATION REPORT]" -ForegroundColor Green
Write-Host "Publisher:    Jun Mystery <darkzeuslk@gmail.com>"
Write-Host "Signature:    $($verify.Status)" -ForegroundColor $statusColor
Write-Host "Status Msg:   $($verify.StatusMessage)"
Write-Host "Thumbprint:   $($cert.Thumbprint)"
Write-Host "SHA-256 Hash: $hashSha256"
Write-Host "SHA-512 Hash: $hashSha512"

if ($verify.Status -in @("NotSigned", "HashMismatch", "NotSupportedFileFormat")) {
    Write-Error "Authenticode signing failed with status: $($verify.Status) ($($verify.StatusMessage))"
    exit 1
}
Write-Host "=== Signing complete ===" -ForegroundColor Cyan
