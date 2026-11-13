[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern("^[a-z0-9][a-z0-9-]*$")]
    [string]$ModName,

    [Parameter(Mandatory = $true)]
    [string]$Tooth,

    [string]$Namespace,

    [string]$DisplayName,

    [string]$Description,

    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Normalize-Tooth {
    param([string]$InputTooth)

    $normalized = $InputTooth.Trim()
    $normalized = $normalized -replace "^https?://", ""
    $normalized = $normalized.TrimEnd("/")
    if ($normalized -notmatch "^github\.com/[^/\s]+/[^/\s]+$") {
        throw "Invalid -Tooth format. Expected: github.com/<owner>/<repo>"
    }
    return $normalized
}

function New-DisplayNameFromModName {
    param([string]$InputModName)

    $words = $InputModName -split "[-_]+" | Where-Object { $_ -ne "" }
    if ($words.Count -eq 0) {
        return $InputModName
    }
    return ($words | ForEach-Object {
        if ($_.Length -le 1) {
            $_.ToUpperInvariant()
        }
        else {
            $_.Substring(0, 1).ToUpperInvariant() + $_.Substring(1)
        }
    }) -join " "
}

function New-NamespaceFromModName {
    param([string]$InputModName)

    $name = $InputModName -replace "-", "_"
    $name = $name -replace "[^A-Za-z0-9_]", "_"
    if ($name -match "^[0-9]") {
        $name = "_" + $name
    }
    return $name
}

$rootDir     = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$toothValue  = Normalize-Tooth -InputTooth $Tooth
$repoSlug    = ($toothValue -split "/")[-1]

if ([string]::IsNullOrWhiteSpace($Namespace)) {
    $Namespace = New-NamespaceFromModName -InputModName $ModName
}
if ($Namespace -notmatch "^[A-Za-z_][A-Za-z0-9_]*$") {
    throw "Invalid -Namespace. Expected C++ identifier style, e.g. my_mod."
}

if ([string]::IsNullOrWhiteSpace($DisplayName)) {
    $DisplayName = New-DisplayNameFromModName -InputModName $ModName
}

if ([string]::IsNullOrWhiteSpace($Description)) {
    $Description = "Description of $DisplayName"
}

$replacements = [ordered]@{
    "github.com/LiteLDev/levilamina-mod-template" = $toothValue
    "levilamina-mod-template"                      = $repoSlug
    "Description of my mod"                        = $Description
    "My Mod"                                       = $DisplayName
    "my_mod"                                       = $Namespace
    "my-mod"                                       = $ModName
}

$allowExt      = @(".md", ".json", ".lua", ".h", ".hpp", ".cpp", ".yml", ".yaml")
$ignoreTopDirs = @(".git", ".xmake", ".cache", ".vscode", "build", "bin")

$files = Get-ChildItem -Path $rootDir -Recurse -File | Where-Object {
    $ext = $_.Extension.ToLowerInvariant()
    if ($allowExt -notcontains $ext) {
        return $false
    }

    $relative = $_.FullName.Substring($rootDir.Length).TrimStart("\", "/")
    if ([string]::IsNullOrWhiteSpace($relative)) {
        return $false
    }
    $firstPart = ($relative -split "[\\/]+")[0]
    return ($ignoreTopDirs -notcontains $firstPart)
}

$utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$changed   = New-Object System.Collections.Generic.List[string]

foreach ($file in $files) {
    if ($file.Name -ieq "compile_commands.json") {
        continue
    }

    $oldText = Get-Content -Path $file.FullName -Raw -Encoding UTF8
    $newText = $oldText

    foreach ($pair in $replacements.GetEnumerator()) {
        $newText = $newText.Replace($pair.Key, $pair.Value)
    }

    if ($newText -ne $oldText) {
        $relative = $file.FullName.Substring($rootDir.Length).TrimStart("\", "/")
        $changed.Add($relative)
        if (-not $DryRun) {
            [System.IO.File]::WriteAllText($file.FullName, $newText, $utf8NoBom)
        }
    }
}

Write-Host "Template init parameters:"
Write-Host "  ModName:     $ModName"
Write-Host "  Namespace:   $Namespace"
Write-Host "  DisplayName: $DisplayName"
Write-Host "  Tooth:       $toothValue"
Write-Host "  RepoSlug:    $repoSlug"
Write-Host "  Description: $Description"

if ($changed.Count -eq 0) {
    Write-Warning "No placeholder changes were made. The template may already be initialized."
    return
}

if ($DryRun) {
    Write-Host ""
    Write-Host "Dry run only. The following files would be updated:"
}
else {
    Write-Host ""
    Write-Host "Updated files:"
}

$changed | Sort-Object | ForEach-Object { Write-Host "  - $_" }

if (-not $DryRun) {
    Write-Host ""
    Write-Host "Done. Review changes with: git diff"
}
