# Makes a Wrecktangle release. release.bat runs this.
#
#   release.bat 0.4.1   Opens a pull request into dev that bumps the version to
#                       0.4.1 (Cargo.toml, Cargo.lock, the README badge and
#                       links). Merging dev into main then publishes it: the
#                       Release workflow releases any version on main that
#                       has no release yet. A pre-release such as
#                       0.9.0-beta.2 leaves the README on the latest release
#                       and is published as a GitHub pre-release, which only
#                       beta copies hear about.
#   release.bat         Publishes the version on main by hand: checks CI
#                       passed, tags v<version> on main and pushes the tag,
#                       which starts the Release workflow. If that version is
#                       already released, asks for the next one and opens its
#                       bump pull request.
#
# -DryRun shows what would happen without changing files, git or GitHub.
# Needs git, gh (signed in) and cargo. Keep this file ASCII: Windows
# PowerShell 5.1 reads a script without a BOM as ANSI.

param(
    [string]$Version = "",
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)

function Fail([string]$Message) {
    Write-Host ""
    Write-Host $Message -ForegroundColor Red
    exit 1
}

function Step([string]$Message) {
    Write-Host "> $Message" -ForegroundColor Cyan
}

# Runs a native command and stops on a non-zero exit code.
function Exec([string]$What, [scriptblock]$Command) {
    $output = & $Command
    if ($LASTEXITCODE -ne 0) { Fail "$What failed." }
    $output
}

foreach ($tool in "git", "gh", "cargo") {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { Fail "$tool is not on PATH." }
}
& gh auth status *> $null
if ($LASTEXITCODE -ne 0) { Fail "Sign in to the GitHub CLI first: gh auth login" }

Step "Fetching main, dev and tags"
Exec "git fetch" { git fetch --quiet origin --tags }
$repo = Exec "gh repo view" { gh repo view --json nameWithOwner --jq .nameWithOwner }
$mainSha = Exec "git rev-parse" { git rev-parse origin/main }
$mainToml = (Exec "git show" { git show origin/main:Cargo.toml }) -join "`n"
if ($mainToml -notmatch '(?m)^version = "([^"]+)"') { Fail "Couldn't read the version from main's Cargo.toml." }
$mainVersion = $Matches[1]
$devToml = (Exec "git show" { git show origin/dev:Cargo.toml }) -join "`n"
if ($devToml -notmatch '(?m)^version = "([^"]+)"') { Fail "Couldn't read the version from dev's Cargo.toml." }
$devVersion = $Matches[1]

# Compares two versions the way the update check does (src/update.rs): a
# pre-release sorts below its release, and its dot-separated parts compare
# as numbers when both are numbers. Returns -1, 0 or 1.
function Compare-Version([string]$A, [string]$B) {
    $aCore, $aPre = $A.Split([char[]]'-', 2)
    $bCore, $bPre = $B.Split([char[]]'-', 2)
    $core = ([version]$aCore).CompareTo([version]$bCore)
    if ($core -ne 0) { return [Math]::Sign($core) }
    if (-not $aPre -and -not $bPre) { return 0 }
    if (-not $aPre) { return 1 }
    if (-not $bPre) { return -1 }
    $x = $aPre.Split('.')
    $y = $bPre.Split('.')
    for ($i = 0; $i -lt [Math]::Max($x.Count, $y.Count); $i++) {
        if ($i -ge $x.Count) { return -1 }
        if ($i -ge $y.Count) { return 1 }
        $xn = [long]0
        $yn = [long]0
        $xNumber = [long]::TryParse($x[$i], [ref]$xn)
        $yNumber = [long]::TryParse($y[$i], [ref]$yn)
        if ($xNumber -and $yNumber) { $c = $xn.CompareTo($yn) }
        elseif ($xNumber) { $c = -1 }
        elseif ($yNumber) { $c = 1 }
        else { $c = [string]::CompareOrdinal($x[$i], $y[$i]) }
        if ($c -ne 0) { return [Math]::Sign($c) }
    }
    return 0
}

function Test-PreRelease([string]$Version) {
    return $Version.Contains("-")
}

function Test-Tag([string]$Tag) {
    & git rev-parse --quiet --verify "refs/tags/$Tag" *> $null
    return $LASTEXITCODE -eq 0
}

# The README links must name the version they download, as CI checks.
function Get-ReadmeProblems([string]$Readme, [string]$Want) {
    $problems = @()
    if (-not $Readme.Contains("img.shields.io/badge/release-v$Want-blue")) {
        $problems += "the release badge doesn't show v$Want"
    }
    $links = [regex]::Matches($Readme, 'releases/download/v([^/]+)/Wrecktangle-([^-]+)-x(64|86)')
    if ($links.Count -eq 0) { $problems += "there are no download links" }
    foreach ($link in $links) {
        if ($link.Groups[1].Value -ne $Want -or $link.Groups[2].Value -ne $Want) {
            $problems += "a download link points at $($link.Value)"
        }
    }
    return $problems
}

function Set-ReadmeVersion([string]$Readme, [string]$Next) {
    $Readme = $Readme.Replace("img.shields.io/badge/release-v$devVersion-blue", "img.shields.io/badge/release-v$Next-blue")
    return $Readme.Replace("releases/download/v$devVersion/Wrecktangle-$devVersion-x", "releases/download/v$Next/Wrecktangle-$Next-x")
}

function New-Bump([string]$Next) {
    if ($Next -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z]+(\.[0-9A-Za-z]+)*)?$') {
        Fail "'$Next' isn't a version like 0.4.1 or 0.9.0-beta.2."
    }
    if ((Compare-Version $Next $devVersion) -le 0) {
        Fail "$Next isn't newer than $devVersion, the version on dev."
    }
    # The README keeps pointing at the latest release while a beta is out.
    $preRelease = Test-PreRelease $Next
    if (Test-Tag "v$Next") { Fail "v$Next is already released." }
    $branch = "chore/bump-$Next"
    & git ls-remote --exit-code --heads origin $branch *> $null
    if ($LASTEXITCODE -eq 0) { Fail "The branch $branch already exists on GitHub; its pull request may be open already." }

    Write-Host ""
    Write-Host "Bumping $devVersion -> $Next on a new branch from dev, $branch."
    if ($DryRun) {
        if ($preRelease) {
            Write-Host "Dry run: $Next is a pre-release, so the README would stay as it is."
            Write-Host "Dry run: would change Cargo.toml and Cargo.lock, commit, push and open a pull request."
            return
        }
        $readme = Set-ReadmeVersion ((Exec "git show" { git show origin/dev:README.md }) -join "`n") $Next
        $problems = Get-ReadmeProblems $readme $Next
        if ($problems) { Fail ("README.md would need a hand edit: " + ($problems -join "; ") + ".") }
        $links = [regex]::Matches($readme, 'releases/download/v[^/]+/Wrecktangle-[^-]+-x(64|86)').Count
        Write-Host "Dry run: the README badge and $links download links would move to $Next."
        Write-Host "Dry run: would change Cargo.toml, Cargo.lock and README.md, commit, push and open a pull request."
        return
    }
    if (Exec "git status" { git status --porcelain }) {
        Fail "This checkout has uncommitted changes. Commit or stash them first."
    }
    $start = (& git branch --show-current)

    Step "Creating $branch from dev"
    Exec "git switch" { git switch --quiet -c $branch origin/dev }
    $utf8 = New-Object System.Text.UTF8Encoding $false

    $tomlPath = Join-Path (Get-Location) "Cargo.toml"
    $toml = [IO.File]::ReadAllText($tomlPath)
    $toml = ([regex]'(?m)^version = "[^"]*"').Replace($toml, "version = `"$Next`"", 1)
    [IO.File]::WriteAllText($tomlPath, $toml, $utf8)

    if (-not $preRelease) {
        $readmePath = Join-Path (Get-Location) "README.md"
        $readme = Set-ReadmeVersion ([IO.File]::ReadAllText($readmePath)) $Next
        $problems = Get-ReadmeProblems $readme $Next
        if ($problems) { Fail ("README.md needs a hand edit: " + ($problems -join "; ") + ".") }
        [IO.File]::WriteAllText($readmePath, $readme, $utf8)
    }

    Step "Updating Cargo.lock"
    Exec "cargo update" { cargo update --offline --quiet -p wrecktangle }

    Step "Committing and pushing"
    Exec "git add" { git add Cargo.toml Cargo.lock README.md }
    Exec "git commit" { git commit --quiet -m "chore: bump version to $Next" }
    Exec "git push" { git push --quiet -u origin $branch }

    Step "Opening the pull request"
    $body = "Bumps the version from $devVersion to $Next in Cargo.toml, Cargo.lock, the README release badge and download links.`n`nAfter merging, merge dev into main: the Release workflow publishes $Next."
    if ($preRelease) {
        $body = "Bumps the version from $devVersion to $Next in Cargo.toml and Cargo.lock. It is a pre-release, so the README stays on the latest release.`n`nAfter merging, merge dev into main: the Release workflow publishes $Next as a GitHub pre-release, which only beta copies hear about."
    }
    $url = Exec "gh pr create" { gh pr create --base dev --head $branch --title "Bump version to $Next" --body $body }
    if ($start) { & git switch --quiet $start }

    Write-Host ""
    Write-Host "Pull request: $url" -ForegroundColor Green
    Write-Host "Merge it, then merge dev into main to publish $Next."
}

function Publish {
    $short = $mainSha.Substring(0, 7)
    Write-Host ""
    Write-Host "main is at $short, version $mainVersion."

    if (-not (Test-PreRelease $mainVersion)) {
        $readme = (Exec "git show" { git show origin/main:README.md }) -join "`n"
        $problems = Get-ReadmeProblems $readme $mainVersion
        if ($problems) { Fail ("main's README.md isn't ready: " + ($problems -join "; ") + ".") }
    }

    # Parsed here rather than with --jq: Windows PowerShell 5.1 splits
    # arguments that contain double quotes when it passes them to programs.
    $runs = (Exec "gh run list" {
        gh run list --repo $repo --workflow ci.yml --commit $mainSha --limit 1 --json status,conclusion
    }) -join "`n" | ConvertFrom-Json
    $ci = @($runs) | Select-Object -First 1
    if (-not $ci) { Fail "No CI run found for main ($short)." }
    if ($ci.status -ne "completed") { Fail "CI is still running on main ($short). Try again when it has passed." }
    if ($ci.conclusion -ne "success") { Fail "CI failed on main ($short): https://github.com/$repo/actions/workflows/ci.yml" }
    Write-Host "CI passed on main."

    if ($DryRun) {
        Write-Host "Dry run: would tag v$mainVersion on $short and push it."
        return
    }
    $answer = Read-Host "Publish Wrecktangle $mainVersion now? [y/N]"
    if ($answer -notmatch '^(y|yes)$') { Write-Host "Nothing published."; return }

    Step "Tagging v$mainVersion"
    Exec "git tag" { git tag -a "v$mainVersion" $mainSha -m "Wrecktangle $mainVersion" }
    Exec "git push" { git push --quiet origin "v$mainVersion" }

    Step "Waiting for the Release workflow"
    $run = ""
    for ($i = 0; $i -lt 12 -and -not $run; $i++) {
        Start-Sleep -Seconds 5
        $run = & gh run list --repo $repo --workflow release.yml --branch "v$mainVersion" --limit 1 --json databaseId --jq '.[0].databaseId'
    }
    if ($run) {
        & gh run watch $run --repo $repo --exit-status
        if ($LASTEXITCODE -ne 0) { Fail "The Release workflow failed: https://github.com/$repo/actions/runs/$run" }
    } else {
        Write-Host "The Release workflow hasn't started yet: https://github.com/$repo/actions/workflows/release.yml"
        return
    }

    Write-Host ""
    Write-Host "Published: https://github.com/$repo/releases/tag/v$mainVersion" -ForegroundColor Green
}

if ($Version) {
    New-Bump $Version
} elseif (Test-Tag "v$mainVersion") {
    Write-Host ""
    Write-Host "$mainVersion, the version on main, is already released."
    $next = Read-Host "Next version (empty to stop)"
    if ($next) { New-Bump $next.Trim() }
} else {
    Publish
}
