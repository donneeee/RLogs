[CmdletBinding()]
param(
    [string]$ExecutablePath = (Join-Path (Split-Path $PSScriptRoot -Parent) 'target\debug\rlogs-submission-service.exe'),
    [string]$ClientId = '1544125568461709353',
    [string]$PublicApiUrl = 'https://rlogs-submissions.pages.dev',
    [string]$CallbackUrl = 'https://rlogs-app.github.io/account/',
    [string]$PublicSiteUrl = 'https://rlogs-app.github.io',
    [string]$ListenAddress = '127.0.0.1:8788',
    [string]$DataRoot = (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'RLogs\runtime-data\submission-service'),
    [string]$ClientSecretPath = (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'RLogs\runtime-data\submission-auth\discord-client-secret.dpapi'),
    [string]$TokenPepperPath = (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'RLogs\runtime-data\submission-auth\auth-token-pepper.dpapi')
)

$ErrorActionPreference = 'Stop'

function Read-DpapiProtectedText {
    param([Parameter(Mandatory)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Protected credential file does not exist: $Path"
    }

    $secure = Get-Content -LiteralPath $Path -Raw | ConvertTo-SecureString
    $pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
    try {
        return [Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer)
    }
    finally {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer)
        $secure = $null
    }
}

$resolvedExecutable = (Resolve-Path -LiteralPath $ExecutablePath).Path
$workingDirectory = Split-Path $PSScriptRoot -Parent
if (-not (Test-Path -LiteralPath $resolvedExecutable -PathType Leaf)) {
    throw "Submission receiver executable does not exist: $ExecutablePath"
}
if ($ListenAddress -notmatch '^127\.0\.0\.1:\d+$') {
    throw 'The public tunnel receiver must remain bound to an explicit 127.0.0.1 port.'
}

New-Item -ItemType Directory -Path $DataRoot -Force | Out-Null
$logRoot = Join-Path $DataRoot 'logs'
New-Item -ItemType Directory -Path $logRoot -Force | Out-Null
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$stdoutPath = Join-Path $logRoot "receiver-$stamp.stdout.log"
$stderrPath = Join-Path $logRoot "receiver-$stamp.stderr.log"

$clientSecret = Read-DpapiProtectedText -Path $ClientSecretPath
$tokenPepper = Read-DpapiProtectedText -Path $TokenPepperPath
if ($clientSecret.Length -lt 16) {
    throw 'The protected Discord client secret is not valid.'
}
if ($tokenPepper.Length -lt 32) {
    throw 'The protected authentication token pepper must contain at least 32 characters.'
}

$receiverEnvironment = [ordered]@{
    RLOGS_DISCORD_CLIENT_ID = $ClientId
    RLOGS_DISCORD_CLIENT_SECRET = $clientSecret
    RLOGS_PUBLIC_API_URL = $PublicApiUrl
    RLOGS_DISCORD_CALLBACK_URL = $CallbackUrl
    RLOGS_AUTH_TOKEN_PEPPER = $tokenPepper
    RLOGS_PUBLIC_SITE_URL = $PublicSiteUrl
    RLOGS_SUBMISSION_LISTEN = $ListenAddress
    RLOGS_SUBMISSION_DATA = $DataRoot
}
$previousEnvironment = @{}

try {
    foreach ($name in $receiverEnvironment.Keys) {
        $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        [Environment]::SetEnvironmentVariable($name, $receiverEnvironment[$name], 'Process')
    }

    $process = Start-Process `
        -FilePath $resolvedExecutable `
        -WorkingDirectory $workingDirectory `
        -WindowStyle Hidden `
        -RedirectStandardOutput $stdoutPath `
        -RedirectStandardError $stderrPath `
        -PassThru
}
finally {
    foreach ($name in $receiverEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $previousEnvironment[$name], 'Process')
    }
    $clientSecret = $null
    $tokenPepper = $null
    $receiverEnvironment = $null
}

Start-Sleep -Milliseconds 400
if ($process.HasExited) {
    throw "Submission receiver exited during startup. Inspect $stderrPath"
}

[pscustomobject]@{
    ProcessId = $process.Id
    ListenAddress = $ListenAddress
    PublicApiUrl = $PublicApiUrl
    StandardOutput = $stdoutPath
    StandardError = $stderrPath
}
