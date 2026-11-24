# Sign and publish a des-k release that CI left as a draft (spec 0031, RELEASING.md).
# Run it on your PC from your des-k checkout, after the tag's CI run made the draft:
#   pwsh -File sign-release.ps1 X.Y.Z [-Key main|backup]
# It checks that the draft holds what your own commit built, signs SHA256SUMS.txt with your
# minisign key, checks that signature the way des-k will, then uploads it and publishes.
# Any failure stops it and leaves the release a draft, and no copy of des-k can download a draft.
#
# The private key never leaves this PC: the main key comes from your Bitwarden note (or a masked
# prompt) into a temp file deleted at the end; the backup key is read from where you keep it.
#Requires -Version 7.4
param(
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version,
    [ValidateSet('main', 'backup')][string]$Key = 'main'
)
$ErrorActionPreference = 'Stop'
# A failed git, gh or minisign call stops the script instead of letting it carry on half informed.
$PSNativeCommandUseErrorActionPreference = $true
$ProgressPreference = 'SilentlyContinue'

$repo = 'CanaanGM/des-k'
$tag = "v$Version"
$sums = 'SHA256SUMS.txt'
$sig = "$sums.minisig"
$setup = "des-k-$tag-setup.exe"
$work = Join-Path ([IO.Path]::GetTempPath()) "des-k-sign-$([guid]::NewGuid().ToString('n'))"

Push-Location $PSScriptRoot
try {
    foreach ($tool in 'git', 'gh', 'minisign') {
        if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "$tool is not on PATH (RELEASING.md lists what you need)" }
    }

    # Only a commit you have: Cargo.toml's version, the tag, HEAD and origin/main all agree.
    git fetch --quiet --tags origin
    $head = git rev-parse HEAD
    $m = git show HEAD:Cargo.toml | Select-String -Pattern '^version = "(.+)"' | Select-Object -First 1
    $cargo = if ($m) { $m.Matches[0].Groups[1].Value } else { 'no version' }
    if ($cargo -cne $Version) { throw "Cargo.toml at HEAD says $cargo, not $Version" }
    $tagged = try { git rev-parse --verify --quiet "$tag^{commit}" } catch { $null }
    if ($tagged -ne $head) { throw "tag $tag is $(if ($tagged) { "at $tagged" } else { 'missing' }), but HEAD is $head" }
    $main = git rev-parse origin/main
    if ($main -ne $head) { throw "HEAD $head is not origin/main ($main); pull or push first, so you sign the commit everyone sees" }

    # The public key des-k carries, read from the commit being released, so the self check below
    # proves the very copy this release ships will accept it.
    $pk = @(git show HEAD:src/install.rs | Select-String -CaseSensitive -Pattern "^\s*\(""$Key"", ""([A-Za-z0-9+/=]+)""\),?\s*$")
    if ($pk.Count -ne 1) { throw "found $($pk.Count) $Key keys in KEYS in src/install.rs, need exactly one" }
    $pk = $pk[0].Matches[0].Groups[1].Value

    # The one draft for this tag. gh finds a draft by its tag, so there must be exactly one.
    $releases = @(gh api "repos/$repo/releases?per_page=100" | ConvertFrom-Json | Where-Object { $_.tag_name -ceq $tag })
    if (@($releases | Where-Object { -not $_.draft }).Count) { throw "$tag is already published" }
    if ($releases.Count -ne 1) { throw "found $($releases.Count) drafts for $tag, need exactly one; wait for CI, or delete the extra drafts on GitHub" }
    $release = $releases[0]

    $run = gh run list --repo $repo --workflow build.yml --branch $tag --limit 1 --json headSha,conclusion,url,createdAt | ConvertFrom-Json | Select-Object -First 1
    if (-not $run) { throw "no CI run found for $tag" }
    # The draft must come from a run that built your HEAD, not another commit under the same tag.
    if ($run.headSha -ne $head) { throw "the latest CI run for $tag built $($run.headSha), not HEAD $head" }

    New-Item -ItemType Directory $work | Out-Null
    gh release download $tag --repo $repo --dir $work
    $files = @(Get-ChildItem -LiteralPath $work -File | Where-Object { $_.Name -cne $sig })
    if ($files.Name -cnotcontains $sums) { throw "the draft has no $sums" }

    # Every file matches its line, and every line names a file: what you sign is what is there.
    $lines = [Collections.Generic.Dictionary[string, string]]::new([StringComparer]::Ordinal)
    foreach ($line in Get-Content -LiteralPath (Join-Path $work $sums)) {
        if ($line -cnotmatch '^([0-9a-f]{64})  (\S+)$') { throw "$sums has a line it should not: $line" }
        if ($lines.ContainsKey($Matches[2])) { throw "$sums names $($Matches[2]) twice" }
        $lines[$Matches[2]] = $Matches[1]
    }
    $others = @($files | Where-Object { $_.Name -cne $sums })
    foreach ($f in $others) {
        if (-not $lines.ContainsKey($f.Name)) { throw "$sums does not list $($f.Name)" }
        $got = (Get-FileHash -LiteralPath $f.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($got -cne $lines[$f.Name]) { throw "$($f.Name) hashes to $got, but $sums says $($lines[$f.Name])" }
    }
    foreach ($name in $lines.Keys) {
        if ($others.Name -cnotcontains $name) { throw "$sums lists $name, which the draft does not have" }
    }
    if (-not $lines.ContainsKey($setup)) { throw "$sums does not list $setup, so des-k could not install this release" }

    Write-Host ''
    Write-Host "Commit  $(git log -1 --format='%H %s' HEAD)"
    Write-Host "CI run  $($run.url) ($($run.conclusion), $($run.createdAt))"
    Write-Host "Draft   $($release.html_url)"
    foreach ($f in $files) { Write-Host ('  {0,14:N0} bytes  {1}' -f $f.Length, $f.Name) }
    Write-Host ''
    $typed = Read-Host "Every file matches $sums. Type $Version to sign it with the $Key key"
    if ($typed -cne $Version) { throw "you typed '$typed', not $Version" }

    if ($Key -eq 'main') {
        $text = $null
        if ($env:BW_SESSION -and (Get-Command bw -ErrorAction SilentlyContinue)) {
            try { $text = (bw get notes 'des-k minisign main' --nointeraction) -join "`n" }
            catch { Write-Warning 'Bitwarden did not give the key (locked, or no such note); paste it instead.' }
        }
        if (-not $text) {
            $text = Read-Host -MaskInput 'Paste the main secret key (the long line under "untrusted comment")'
        }
        # A note holding only the key's base64 line still makes a key file minisign can read.
        if (-not $text.StartsWith('untrusted comment:')) { $text = "untrusted comment: minisign encrypted secret key`n$($text.Trim())`n" }
        $keyFile = Join-Path $work 'main.key'
        [IO.File]::WriteAllText($keyFile, $text, [Text.UTF8Encoding]::new($false))
        $text = $null
    } else {
        $keyFile = Read-Host 'Path to the backup secret key file'
        if (-not (Test-Path -LiteralPath $keyFile -PathType Leaf)) { throw "no file at $keyFile" }
    }

    Push-Location $work
    try {
        minisign -S -s $keyFile -m $sums -t "des-k $Version" -x $sig
        if ($Key -eq 'main') { Remove-Item -LiteralPath $keyFile }

        # Check it the way des-k will: this key, and the prehashed algorithm (bytes "ED"; a legacy
        # "Ed" signature is refused by every copy).
        try { minisign -V -P $pk -m $sums -x $sig } catch { throw "the new signature does not check with the $Key key in src/install.rs" }
        $alg = [Text.Encoding]::ASCII.GetString([Convert]::FromBase64String((Get-Content -LiteralPath $sig)[1])[0..1])
        if ($alg -cne 'ED') { throw "the signature is not prehashed ($alg); use the minisign version RELEASING.md names" }

        gh release upload $tag $sig --repo $repo --clobber
        gh release edit $tag --repo $repo --draft=false
    } finally { Pop-Location }
    Write-Host "Published des-k $Version, signed with the $Key key: https://github.com/$repo/releases/tag/$tag"
} catch {
    Write-Host "Stopped: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "des-k $Version stays a draft." -ForegroundColor Red
    $failed = $true
} finally {
    Pop-Location
    # Holds the downloaded files and, for the main key, the key file.
    if (Test-Path -LiteralPath $work) { Remove-Item -LiteralPath $work -Recurse -Force }
}
if ($failed) { exit 1 }
