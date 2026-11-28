# Builds and checks des-k's public docs. The site pages are built from site-src/, never edited by hand.
#   pwsh -File site.ps1 build      writes site/index.html and site/how-it-works.html from site-src/
#   pwsh -File site.ps1 check      fails on a stale page, a broken #link, a dash or a private reference;
#                                  warns on long sentences, long paragraphs and a long README
#   pwsh -File site.ps1 pictures   copies the render tests' PNGs from target/ into site/img/
#                                  (run `cargo test render -- --ignored` first; your PC only)
param(
    [Parameter(Mandatory, Position = 0)][ValidateSet('build', 'check', 'pictures')][string]$Command
)
$ErrorActionPreference = 'Stop'

$root = $PSScriptRoot
$utf8 = [Text.UTF8Encoding]::new($false)
$invariant = [Globalization.CultureInfo]::InvariantCulture

# Each source in site-src/ and the page it builds in site/.
$pages = [ordered]@{ 'guide.html' = 'index.html'; 'how-it-works.html' = 'how-it-works.html' }
$keys = 'eyebrow', 'headline', 'lede', 'pills', 'status', 'updated', 'related', 'link',
        'seo_title', 'description', 'og_type', 'og_title', 'og_description', 'footnote'
$statuses = @{ reviewed = 'sage', 'Reviewed'; draft = 'amber', 'Draft' }
$wordsPerMinute = 230

# The guide's pictures of real des-k windows: site/img/<window>-light.png and -dark.png, copied
# from the render tests' target/<dir>/<state>-light.png and -dark.png.
$pictures = [ordered]@{
    indicator = 'indicator/card-names'
    finder    = 'finder/names-search'
    overview  = 'overview/names'
    tray      = 'tray/one'
    hints     = 'hints/defaults'
}
$pictureBytes = 150KB
$allBytes = 1.5MB

function Read-Text([string]$path) {
    [IO.File]::ReadAllText($path, $utf8) -replace "`r`n", "`n"
}

function Write-Text([string]$path, [string]$text) {
    [IO.File]::WriteAllText($path, $text, $utf8)
}

# A source is one `key: value` line per key, a `---` line, then the body.
function Read-Source([string]$name) {
    $text = Read-Text (Join-Path $root "site-src/$name")
    $split = $text.IndexOf("`n---`n")
    if ($split -lt 0) { throw "site-src/${name}: no '---' line after the header" }
    $header = [ordered]@{}
    $line = 0
    foreach ($row in $text.Substring(0, $split).Split("`n")) {
        $line++
        if ($row -notmatch '^(\w+): (.*)$') { throw "site-src/${name}:${line}: not a 'key: value' line" }
        $key, $value = $Matches[1], $Matches[2].Trim()
        if ($key -notin $keys) { throw "site-src/${name}:${line}: unknown key '$key'" }
        if ($header.Contains($key)) { throw "site-src/${name}:${line}: '$key' given twice" }
        $header[$key] = $value
    }
    foreach ($key in $keys) { if (-not $header.Contains($key)) { throw "site-src/${name}: missing key '$key'" } }
    if (-not $statuses.ContainsKey($header.status)) { throw "site-src/${name}: status must be reviewed or draft" }
    @{ header = $header; body = $text.Substring($split + 5).TrimEnd("`n") }
}

# Words in the body, leaving out drawings, code, scripts, styles, tags and entities.
function Get-ReadMinutes([string]$body) {
    $prose = $body -replace '(?is)<(svg|pre|script|style)\b.*?</\1>', ' ' -replace '<[^>]+>', ' ' -replace '&#?\w+;', ' '
    $words = @($prose -split '\s+' | Where-Object { $_ }).Count
    [Math]::Max(1, [int][Math]::Round($words / $wordsPerMinute, [MidpointRounding]::AwayFromZero))
}

function Build-Page([string]$name) {
    $source = Read-Source $name
    $h = $source.header
    $updated = [datetime]::ParseExact($h.updated, 'yyyy-MM-dd', $invariant)
    $values = @{}
    foreach ($key in $keys) { $values[$key] = $h[$key] }
    $values.pills = ($h.pills -split '\|' | ForEach-Object { "<span class=`"pill`">$($_.Trim())</span>" }) -join ''
    $values.related = ($h.related -split '\|' | ForEach-Object { "<li>$($_.Trim())</li>" }) -join ''
    $values.status_class, $values.status_label = $statuses[$h.status]
    $values.updated = $updated.ToString('yyyy-MM-dd', $invariant)
    $values.updated_shown = $updated.ToString('d MMM yyyy', $invariant)
    $values.read_time = Get-ReadMinutes $source.body
    $values.body = $source.body

    $shell = Read-Text (Join-Path $root 'site-src/shell.html')
    foreach ($m in [regex]::Matches($shell, '\{\{(\w+)\}\}')) {
        if (-not $values.ContainsKey($m.Groups[1].Value)) { throw "site-src/shell.html: nothing fills {{$($m.Groups[1].Value)}}" }
    }
    # One pass, so a {{word}} inside the body stays as written.
    [regex]::Replace($shell, '\{\{(\w+)\}\}', { param($m) [string]$values[$m.Groups[1].Value] })
}

# ---- check ----

# The files whose words are checked; the shell is code and gets no prose checks.
$markdown = 'README.md', 'CONTRIBUTING.md', 'SECURITY.md', 'RELEASING.md'
$readmeLines = 120
$sentenceWords = 30
$paragraphSentences = 5

# Blanks a match but keeps its line breaks, so line numbers stay true.
function Hide([string]$text, [string]$pattern, [string]$with = '') {
    [regex]::Replace($text, $pattern, { param($m) ($m.Value -replace '[^\n]', '') + $with })
}

function Get-Line([string]$text, [int]$index) {
    ([regex]::Matches($text.Substring(0, $index), "`n")).Count + 1
}

# The shell's own rule (its contents script): explicit ids, then each card's h2 made lower case
# with every run of other characters turned into '-', 'section' when empty, -2, -3 for repeats.
function Get-SiteTargets([string]$page) {
    $targets = [Collections.Generic.HashSet[string]]::new()
    foreach ($m in [regex]::Matches($page, '\sid="([^"]+)"')) { [void]$targets.Add($m.Groups[1].Value) }
    $used = [Collections.Generic.HashSet[string]]::new()
    $cards = [regex]::Matches($page, '(?s)<section class="card"([^>]*)>(.*?)(?=<section class="card"|</main>)')
    foreach ($card in $cards) {
        if ($card.Groups[1].Value -match '\sid="') { continue }
        $h2 = [regex]::Match($card.Groups[2].Value, '(?s)<h2[^>]*>(.*?)</h2>')
        if (-not $h2.Success) { continue }
        $text = [Net.WebUtility]::HtmlDecode(($h2.Groups[1].Value -replace '<[^>]+>', '')).ToLowerInvariant()
        $slug = ($text -replace '[^a-z0-9]+', '-').Trim('-')
        if (-not $slug) { $slug = 'section' }
        $id = $slug
        for ($n = 2; $used.Contains($id); $n++) { $id = "$slug-$n" }
        [void]$used.Add($id); [void]$targets.Add($id)
    }
    , $targets
}

# GitHub's rule: lower case, punctuation other than '-' and '_' removed, each space a '-', -1, -2 for repeats.
function Get-MarkdownTargets([string]$text) {
    $targets = [Collections.Generic.HashSet[string]]::new()
    $text = Hide $text '(?ms)^```.*?^```'
    foreach ($m in [regex]::Matches($text, '\s(?:id|name)="([^"]+)"')) { [void]$targets.Add($m.Groups[1].Value) }
    foreach ($m in [regex]::Matches($text, '(?m)^#{1,6}[ \t]+(.+?)[ \t#]*$')) {
        $heading = $m.Groups[1].Value -replace '\[([^\]]*)\]\([^)]*\)', '$1' -replace '<[^>]+>', ''
        $slug = ($heading.Trim().ToLowerInvariant() -replace '[^\p{L}\p{N} _-]', '') -replace ' ', '-'
        $id = $slug
        for ($n = 1; $targets.Contains($id); $n++) { $id = "$slug-$n" }
        [void]$targets.Add($id)
    }
    , $targets
}

# Which file a link points into, or $null for a page this script doesn't check.
function Resolve-LinkPage([string]$href, [string]$self) {
    $site = 'https://canaangm.github.io/des-k/'
    $repo = 'https://github.com/CanaanGM/des-k'
    if ($href.StartsWith('#')) { return $self }
    $path = $href.Substring(0, $href.IndexOf('#'))
    if ($path.StartsWith($site)) { $path = $path.Substring($site.Length) }
    elseif ($path -eq $repo -or $path -eq "$repo/") { return 'README.md' }
    elseif ($path.StartsWith("$repo/blob/main/")) { $path = $path.Substring("$repo/blob/main/".Length) }
    elseif ($path -match '^[a-z]+:') { return $null }
    $path = $path -replace '^\./', ''
    if ($path -in '', 'index.html') { return 'index.html' }
    if ($path -in $pages.Values -or $path -in $markdown) { return $path }
    $null
}

# The words a reader sees in a file, code and drawings left out, each tag gone but its line kept.
function Get-Prose([string]$name, [string]$text) {
    $text = Hide $text '(?s)<!--.*?-->'
    if ($name.EndsWith('.md')) {
        $text = Hide $text '(?ms)^```.*?^```'
        $text = Hide $text '`[^`\n]+`' 'code'
        $text = Hide $text '\]\((?:https?:|mailto:)[^)]*\)' ']'
    }
    $text = Hide $text '(?is)<(pre|script|style)\b.*?</\1>'
    $text = Hide $text '(?is)<code\b[^>]*>.*?</code>' 'code'
    # alt and aria-label words are read too, so they are lifted out of their tags first.
    $labels = foreach ($m in [regex]::Matches($text, '(?i)\s(?:alt|aria-label)="([^"]*)"')) {
        @{ line = Get-Line $text $m.Index; text = $m.Groups[1].Value }
    }
    $text = Hide $text '(?is)<svg\b.*?</svg>'
    $text = Hide $text '<[^>]*>'
    $lines = $text.Split("`n")
    $rows = for ($i = 0; $i -lt $lines.Count; $i++) { @{ line = $i + 1; text = $lines[$i] } }
    @($rows) + @($labels) | ForEach-Object { @{ line = $_.line; text = [Net.WebUtility]::HtmlDecode($_.text) } }
}

$private = @(
    @{ pattern = '(?i)\bspecs?\s+#?\d{4}\b'; what = 'a spec number' }
    @{ pattern = '(?i)\b[HMRP]-\d{2}\b'; what = 'a review id' }
    @{ pattern = '(?i)AGENTS\.md'; what = 'the agent file' }
    @{ pattern = '(?i)(?<![\w/.:-])docs/'; what = 'the private notes folder' }
)

function Test-Words([string]$name, $rows) {
    foreach ($row in $rows) {
        $t = $row.text
        if ($t -match '—') { "${name}:$($row.line): an em dash; reword with a comma, a period or parentheses" }
        if ($t -match '\s–\s') { "${name}:$($row.line): a spaced en dash; reword it" }
        $body = $t -replace '^\s*(?:[-*+]|\d+\.)\s+', ''
        if ($body -match '(?<=[\p{L}\p{N}.,;:)!?''"*_]) - (?=[\p{L}\p{N}(''"*_])') { "${name}:$($row.line): a hyphen used as a dash; reword it" }
        foreach ($p in $private) {
            if ($t -match $p.pattern) { "${name}:$($row.line): names $($p.what) ('$($Matches[0])'); public docs never do" }
        }
    }
}

# Paragraphs for the length warnings: HTML <p> elements, or Markdown runs of plain lines
# (headings, lists, tables, quotes and HTML lines skipped).
function Get-Paragraphs([string]$name, [string]$text) {
    $text = Hide $text '(?s)<!--.*?-->'
    if ($name.EndsWith('.md')) {
        $text = Hide $text '(?ms)^```.*?^```'
        $text = Hide $text '`[^`\n]+`' 'code'
        $lines = $text.Split("`n")
        $start = 0; $buffer = @()
        for ($i = 0; $i -le $lines.Count; $i++) {
            $l = if ($i -lt $lines.Count) { $lines[$i] } else { '' }
            $plain = $l.Trim() -and $l -notmatch '^\s*(#|\||[-*+]\s|\d+\.\s|>|<)'
            if ($plain) { if (-not $buffer) { $start = $i + 1 }; $buffer += $l; continue }
            if ($buffer) { @{ line = $start; text = ($buffer -join ' ') -replace '\[([^\]]*)\]\([^)]*\)', '$1' }; $buffer = @() }
        }
    } else {
        $text = Hide $text '(?is)<(pre|script|style|svg)\b.*?</\1>'
        $text = Hide $text '(?is)<code\b[^>]*>.*?</code>' 'code'
        foreach ($m in [regex]::Matches($text, '(?is)<p\b[^>]*>(.*?)</p>')) {
            @{ line = Get-Line $text $m.Index; text = [Net.WebUtility]::HtmlDecode(($m.Groups[1].Value -replace '<[^>]+>', '')) }
        }
    }
}

function Test-Length([string]$name, [string]$text) {
    foreach ($p in Get-Paragraphs $name $text) {
        # A sentence ends at . ? or ! before a capital, or before des-k, which is always lower case.
        $sentences = @([regex]::Split($p.text.Trim(), '(?<=[.?!])\s+(?=[A-Z]|des-k\b)') | Where-Object { $_.Trim() })
        if ($sentences.Count -gt $paragraphSentences) {
            "${name}:$($p.line): warning: a paragraph of $($sentences.Count) sentences (aim for $paragraphSentences or fewer)"
        }
        foreach ($s in $sentences) {
            $words = @($s -split '\s+' | Where-Object { $_ }).Count
            if ($words -gt $sentenceWords) { "${name}:$($p.line): warning: a sentence of $words words (aim for under $sentenceWords)" }
        }
    }
}

function Test-Images([string]$name, [string]$text) {
    $base = if ($name.EndsWith('.md')) { $root } else { Join-Path $root 'site' }
    $text = Hide $text '(?s)<!--.*?-->'
    foreach ($m in [regex]::Matches($text, '(?is)<img\b[^>]*>')) {
        $line = Get-Line $text $m.Index
        if ($m.Value -notmatch '\salt="[^"]*\S[^"]*"') { "${name}:${line}: an <img> with no alt text" }
        if ($m.Value -match '\ssrc="([^"]+)"' -and $Matches[1] -notmatch '^[a-z]+:') {
            if (-not (Test-Path (Join-Path $base $Matches[1]))) { "${name}:${line}: an <img> whose file is missing: $($Matches[1])" }
        }
    }
}

function Invoke-Check {
    $failures = [Collections.Generic.List[string]]::new()
    $warnings = [Collections.Generic.List[string]]::new()

    $built = @{}
    foreach ($name in $pages.Keys) {
        $page = $pages[$name]
        $built[$page] = Build-Page $name
        $path = Join-Path $root "site/$page"
        $now = if (Test-Path $path) { Read-Text $path } else { '' }
        if ($now -ne $built[$page]) {
            $a = $now.Split("`n"); $b = $built[$page].Split("`n"); $i = 0
            while ($i -lt $a.Count -and $i -lt $b.Count -and $a[$i] -eq $b[$i]) { $i++ }
            $failures.Add("site/${page}:$($i + 1): differs from what build writes; edit site-src/$name and run: pwsh -File site.ps1 build")
        }
    }

    # Every file that is checked, by the name a link uses for it.
    $files = [ordered]@{}
    foreach ($md in $markdown) { $files[$md] = @{ source = $md; text = Read-Text (Join-Path $root $md) } }
    foreach ($name in $pages.Keys) { $files[$pages[$name]] = @{ source = "site-src/$name"; text = Read-Text (Join-Path $root "site-src/$name") } }
    $targets = @{}
    foreach ($page in $pages.Values) { $targets[$page] = Get-SiteTargets $built[$page] }
    foreach ($md in $markdown) {
        $t = Get-MarkdownTargets $files[$md].text
        [void]$t.Add('readme')   # GitHub's own anchor for the README on the repo page
        $targets[$md] = $t
    }

    foreach ($key in $files.Keys) {
        $f = $files[$key]; $name = $f.source; $text = $f.text
        $code = Hide (Hide (Hide $text '(?s)<!--.*?-->') '(?ms)^```.*?^```') '`[^`\n]+`'
        $code = Hide $code '(?is)<(pre|code)\b.*?</\1>'
        foreach ($m in [regex]::Matches($code, '(?:href="|\]\()([^")\s]*#[^")\s]*)')) {
            $href = $m.Groups[1].Value
            $into = Resolve-LinkPage $href $key
            if (-not $into) { continue }
            $anchor = $href.Substring($href.IndexOf('#') + 1)
            if (-not $targets[$into].Contains($anchor)) { $failures.Add("${name}:$(Get-Line $code $m.Index): $href points at no heading or id in $into") }
        }
        foreach ($p in Test-Words $name (Get-Prose $key $text)) { $failures.Add($p) }
        foreach ($p in Test-Images $name $text) { $failures.Add($p) }
        foreach ($w in Test-Length $name $text) { $warnings.Add($w) }
    }

    $line = 0
    foreach ($row in (Read-Text (Join-Path $root 'site-src/anchors.txt')).Split("`n")) {
        $line++
        if (-not $row.Trim() -or $row.StartsWith('# ')) { continue }
        $page, $anchor = $row.Trim().Split('#', 2)
        if (-not $targets.ContainsKey($page) -or -not $targets[$page].Contains($anchor)) {
            $failures.Add("site-src/anchors.txt:${line}: $($row.Trim()) no longer resolves; keep that anchor, or remove the line on purpose")
        }
    }

    $readme = $files['README.md'].text.TrimEnd("`n").Split("`n").Count
    if ($readme -gt $readmeLines) { $warnings.Add("README.md: warning: $readme lines (aim for $readmeLines or fewer)") }

    $warnings | ForEach-Object { Write-Host $_ }
    $failures | ForEach-Object { Write-Host $_ }
    if ($failures.Count) {
        Write-Host "check: $($failures.Count) problems, $($warnings.Count) warnings"
        exit 1
    }
    Write-Host "check: fine, $($warnings.Count) warnings"
}

switch ($Command) {
    'build' {
        foreach ($name in $pages.Keys) {
            Write-Text (Join-Path $root "site/$($pages[$name])") (Build-Page $name)
            Write-Host "built site/$($pages[$name])"
        }
    }
    'check' { Invoke-Check }
    'pictures' {
        $dir = Join-Path $root 'site/img'
        [void](New-Item -ItemType Directory -Force $dir)
        foreach ($window in $pictures.Keys) {
            foreach ($theme in 'light', 'dark') {
                $from = Join-Path $root "target/$($pictures[$window])-$theme.png"
                if (-not (Test-Path $from)) { throw "$from is missing; run: cargo test render -- --ignored" }
                Copy-Item $from (Join-Path $dir "$window-$theme.png")
            }
        }
        $sizes = Get-ChildItem $dir -Filter *.png
        foreach ($big in $sizes | Where-Object Length -gt $pictureBytes) { throw "site/img/$($big.Name) is $([int]($big.Length / 1KB)) KB; keep each picture at 150 KB or less" }
        $total = ($sizes | Measure-Object Length -Sum).Sum
        if ($total -ge $allBytes) { throw "site/img/ holds $([int]($total / 1KB)) KB; keep it under 1.5 MB" }
        Write-Host "copied $($pictures.Count * 2) pictures into site/img/ ($([int]($total / 1KB)) KB)"
    }
}
