<#
.SYNOPSIS
    vcxproj / vcxproj.filters 를 소스 디렉토리 기준으로 자동 갱신합니다.

.DESCRIPTION
    소스 루트를 재귀 탐색하여 .cpp / .h / .hpp 파일을 수집한 뒤,
    vcxproj 및 vcxproj.filters 에 등록되지 않은 항목을 추가합니다.
    - 기존 항목은 변경하지 않음
    - 디렉토리 계층에 맞는 Filter 를 자동 생성 (소스 파일\... / 헤더 파일\...)
    - 누락된 부모 필터도 함께 생성
    - 빌드 산출물 디렉토리(x64, Debug, Release 등)는 제외

.EXAMPLE
    .\source_refresh.ps1
    .\source_refresh.ps1 -Verbose
#>

[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ─────────────────────────────────────────────────────────────────────────────
# 경로 설정
# BuildScript/Windows/ 기준으로 소스 루트는 ../../NexusEngine/
# ─────────────────────────────────────────────────────────────────────────────
$ScriptDir  = $PSScriptRoot
$SourceRoot = (Resolve-Path (Join-Path $ScriptDir '..\..\NexusEngine')).Path

$VcxprojPath = Join-Path $SourceRoot 'NexusEngine.vcxproj'
$FiltersPath = Join-Path $SourceRoot 'NexusEngine.vcxproj.filters'

if (-not (Test-Path $VcxprojPath)) { Write-Error "vcxproj 없음: $VcxprojPath"; exit 1 }
if (-not (Test-Path $FiltersPath)) { Write-Error "filters 없음: $FiltersPath";  exit 1 }

Write-Verbose "소스 루트  : $SourceRoot"
Write-Verbose "vcxproj    : $VcxprojPath"
Write-Verbose "filters    : $FiltersPath"

# MSBuild XML 네임스페이스 — 요소 생성 시 사용
$NS = 'http://schemas.microsoft.com/developer/msbuild/2003'

# ─────────────────────────────────────────────────────────────────────────────
# 탐색 제외 디렉토리 (대소문자 무관 비교용 hashtable)
# ─────────────────────────────────────────────────────────────────────────────
$ExcludeDirSet = @{}
foreach ($d in @('x64', '.vs', 'Debug', 'Release', 'ipch', '.git')) {
    $ExcludeDirSet[$d.ToLower()] = $true
}

# ─────────────────────────────────────────────────────────────────────────────
# 소스 파일 탐색 → 상대경로(백슬래시) 목록 반환
# ─────────────────────────────────────────────────────────────────────────────
function Get-SourceFiles {
    param([string]$Root)

    $results = New-Object System.Collections.ArrayList

    $items = Get-ChildItem -Path $Root -Recurse -File |
             Where-Object { $_.Extension -in @('.cpp', '.h', '.hpp') }

    foreach ($item in $items) {
        $rel   = $item.FullName.Substring($Root.Length).TrimStart('\')
        $parts = @($rel -split '\\')

        $skip = $false
        if ($parts.Count -gt 1) {
            for ($i = 0; $i -lt $parts.Count - 1; $i++) {
                if ($ExcludeDirSet.ContainsKey($parts[$i].ToLower())) {
                    $skip = $true
                    break
                }
            }
        }

        if (-not $skip) {
            $null = $results.Add([PSCustomObject]@{
                RelPath = $rel
                Ext     = $item.Extension.ToLower()
            })
        }
    }

    return $results
}

# ─────────────────────────────────────────────────────────────────────────────
# 필터 이름: .cpp → "소스 파일\<dir>", .h/.hpp → "헤더 파일\<dir>"
# ─────────────────────────────────────────────────────────────────────────────
function Get-FilterName {
    param([string]$RelPath, [string]$Ext)
    $prefix = if ($Ext -eq '.cpp') { '소스 파일' } else { '헤더 파일' }
    $dir    = Split-Path $RelPath -Parent
    if ([string]::IsNullOrEmpty($dir)) { return $prefix }
    return "$prefix\$dir"
}

# "소스 파일\Game\Actors" → @("소스 파일", "소스 파일\Game", "소스 파일\Game\Actors")
function Get-AllParentFilters {
    param([string]$FilterName)
    $parts  = @($FilterName -split '\\')
    $result = New-Object System.Collections.ArrayList
    for ($i = 0; $i -lt $parts.Count; $i++) {
        $null = $result.Add(($parts[0..$i] -join '\'))
    }
    return $result
}

# ─────────────────────────────────────────────────────────────────────────────
# XML 유틸
# ─────────────────────────────────────────────────────────────────────────────
function Load-Xml {
    param([string]$Path)
    $xml = New-Object System.Xml.XmlDocument
    $xml.PreserveWhitespace = $false
    $xml.Load($Path)
    return $xml
}

function Save-Xml {
    param([System.Xml.XmlDocument]$Xml, [string]$Path)
    $settings = New-Object System.Xml.XmlWriterSettings
    $settings.Indent             = $true
    $settings.IndentChars        = '  '
    $settings.Encoding           = New-Object System.Text.UTF8Encoding($true)
    $settings.OmitXmlDeclaration = $false
    $writer = [System.Xml.XmlWriter]::Create($Path, $settings)
    try   { $Xml.Save($writer) }
    finally { $writer.Close() }
}

# local-name() 기반 XPath — 네임스페이스 매니저 없이도 동작
function xNodes {
    param([System.Xml.XmlDocument]$Xml, [string]$XPath)
    return $Xml.SelectNodes($XPath)
}

function xNode {
    param([System.Xml.XmlDocument]$Xml, [string]$XPath)
    return $Xml.SelectSingleNode($XPath)
}

# ─────────────────────────────────────────────────────────────────────────────
# 기존 Include 항목 수집 → hashtable (대소문자 무관)
# ─────────────────────────────────────────────────────────────────────────────
function Get-ExistingIncludes {
    param([System.Xml.XmlDocument]$Xml)
    $map   = @{}
    $nodes = xNodes $Xml '//*[local-name()="ClCompile" or local-name()="ClInclude"]/@Include'
    foreach ($n in $nodes) { $map[$n.Value.ToLower()] = $true }
    return $map
}

# ─────────────────────────────────────────────────────────────────────────────
# 기존 Filter 이름 수집 → hashtable (대소문자 무관)
# ─────────────────────────────────────────────────────────────────────────────
function Get-ExistingFilters {
    param([System.Xml.XmlDocument]$Xml)
    $map   = @{}
    $nodes = xNodes $Xml '//*[local-name()="Filter"]/@Include'
    foreach ($n in $nodes) { $map[$n.Value.ToLower()] = $true }
    return $map
}

# ─────────────────────────────────────────────────────────────────────────────
# ItemGroup 탐색 / 생성
# ─────────────────────────────────────────────────────────────────────────────
function Get-OrCreate-ItemGroupByChild {
    param([System.Xml.XmlDocument]$Xml, [string]$ChildLocalName)
    $node = xNode $Xml "//*[local-name()='ItemGroup'][*[local-name()='$ChildLocalName']]"
    if ($null -ne $node) { return $node }
    $group = $Xml.CreateElement('ItemGroup', $NS)
    $null  = $Xml.DocumentElement.AppendChild($group)
    return $group
}

function Get-FilterDefGroup {
    param([System.Xml.XmlDocument]$Xml)
    $node = xNode $Xml "//*[local-name()='ItemGroup'][*[local-name()='Filter']]"
    if ($null -ne $node) { return $node }
    $group = $Xml.CreateElement('ItemGroup', $NS)
    $null  = $Xml.DocumentElement.InsertBefore($group, $Xml.DocumentElement.FirstChild)
    return $group
}

# ─────────────────────────────────────────────────────────────────────────────
# 메인 처리
# ─────────────────────────────────────────────────────────────────────────────
Write-Host '=== NexusEngine source_refresh ===' -ForegroundColor Cyan

# 1. 파일 목록 수집
$allFiles = Get-SourceFiles -Root $SourceRoot
Write-Verbose "디스크 파일 수: $($allFiles.Count)"

# 2. XML 로드
$vcxproj = Load-Xml -Path $VcxprojPath
$filters  = Load-Xml -Path $FiltersPath

# 3. 기존 등록 현황 (hashtable)
$existingIncludes = Get-ExistingIncludes -Xml $vcxproj
$existingFilters  = Get-ExistingFilters  -Xml $filters

# 4. 신규 파일만 추려내기
$newFiles = New-Object System.Collections.ArrayList
foreach ($f in $allFiles) {
    if (-not $existingIncludes.ContainsKey($f.RelPath.ToLower())) {
        $null = $newFiles.Add($f)
    }
}

if ($newFiles.Count -eq 0) {
    Write-Host '새로 추가된 파일 없음 — 프로젝트가 최신 상태입니다.' -ForegroundColor Green
    exit 0
}

Write-Host "새 파일 $($newFiles.Count)개 발견:" -ForegroundColor Yellow
foreach ($f in $newFiles) { Write-Host "  + $($f.RelPath)" }

# 5. ItemGroup 참조
$vcxCompileGroup = Get-OrCreate-ItemGroupByChild -Xml $vcxproj -ChildLocalName 'ClCompile'
$vcxIncludeGroup = Get-OrCreate-ItemGroupByChild -Xml $vcxproj -ChildLocalName 'ClInclude'
$filterDefGroup  = Get-FilterDefGroup            -Xml $filters
$fltCompileGroup = Get-OrCreate-ItemGroupByChild -Xml $filters -ChildLocalName 'ClCompile'
$fltIncludeGroup = Get-OrCreate-ItemGroupByChild -Xml $filters -ChildLocalName 'ClInclude'

# 6. 파일별 등록
foreach ($file in $newFiles) {
    $filterName = Get-FilterName -RelPath $file.RelPath -Ext $file.Ext
    $isCpp      = $file.Ext -eq '.cpp'
    $itemType   = if ($isCpp) { 'ClCompile' } else { 'ClInclude' }

    # ── 6a. 누락된 필터 계층 추가 (filters 파일) ─────────────────────────────
    $parentFilters = Get-AllParentFilters -FilterName $filterName
    foreach ($pf in $parentFilters) {
        if (-not $existingFilters.ContainsKey($pf.ToLower())) {
            $guid     = [System.Guid]::NewGuid().ToString('B').ToUpper()
            $filterEl = $filters.CreateElement('Filter', $NS)
            $filterEl.SetAttribute('Include', $pf)
            $guidEl   = $filters.CreateElement('UniqueIdentifier', $NS)
            $guidEl.InnerText = $guid
            $null     = $filterEl.AppendChild($guidEl)
            $null     = $filterDefGroup.AppendChild($filterEl)
            $existingFilters[$pf.ToLower()] = $true
            Write-Verbose "  필터 추가: $pf"
        }
    }

    # ── 6b. vcxproj 에 파일 추가 ─────────────────────────────────────────────
    $vcxEl = $vcxproj.CreateElement($itemType, $NS)
    $vcxEl.SetAttribute('Include', $file.RelPath)
    if ($isCpp) { $null = $vcxCompileGroup.AppendChild($vcxEl) }
    else        { $null = $vcxIncludeGroup.AppendChild($vcxEl) }

    # ── 6c. filters 에 파일 + 필터 매핑 추가 ─────────────────────────────────
    $fltEl       = $filters.CreateElement($itemType, $NS)
    $fltEl.SetAttribute('Include', $file.RelPath)
    $fltFilterEl = $filters.CreateElement('Filter', $NS)
    $fltFilterEl.InnerText = $filterName
    $null        = $fltEl.AppendChild($fltFilterEl)
    if ($isCpp) { $null = $fltCompileGroup.AppendChild($fltEl) }
    else        { $null = $fltIncludeGroup.AppendChild($fltEl) }
}

# 7. 저장
Save-Xml -Xml $vcxproj -Path $VcxprojPath
Save-Xml -Xml $filters  -Path $FiltersPath

Write-Host "완료: $($newFiles.Count)개 파일 등록, vcxproj/filters 갱신됨" -ForegroundColor Green
