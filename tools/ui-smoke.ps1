# Drives a real build of the app with real mouse clicks and checks that the Export screen responds.
# It uses its own empty data folder, never your library.
#   cargo build -p mangapoint
#   powershell -File tools/ui-smoke.ps1 [-Exe target\debug\mangapoint.exe] [-Port 9333]
param(
    [string]$Exe = 'target\debug\mangapoint.exe',
    [int]$Port = 9333
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'cdp.ps1')

$data = Join-Path ([IO.Path]::GetTempPath()) ("mangapoint-smoke-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $data | Out-Null
$env:MANGAPOINT_HOME = $data
$env:MANGAPOINT_DEBUG_PORT = "$Port"
$app = Start-Process -FilePath (Join-Path $root $Exe) -PassThru
$failures = @()

function Check([string]$Name, [scriptblock]$Test) {
    try { & $Test; Write-Host "ok    $Name" }
    catch { Write-Host "FAIL  $Name  ($($_.Exception.Message))"; $script:failures += $Name }
}

try {
    for ($i = 0; $i -lt 60; $i++) {
        try { Connect-App $Port; break } catch { Start-Sleep -Milliseconds 500 }
    }
    Wait-Until "document.querySelector('.nav') !== null" 60 'the app window'

    Check 'the Export page opens on the sample pages' {
        Mouse-Click '.nav-item' 'export'
        Wait-Until "document.querySelector('.export-screen') !== null" 30 'the Export screen'
        Wait-Until "document.querySelectorAll('.px-panel-head').length >= 3" 30 'the panels'
    }
    Check 'a panel folds when its header is clicked' {
        $before = Eval "document.querySelectorAll('.px-panel.collapsed').length"
        Mouse-Click '.px-panel-head' 'presets'
        Start-Sleep -Milliseconds 600
        $after = Eval "document.querySelectorAll('.px-panel.collapsed').length"
        if ($after -le $before) { throw "still $before collapsed panels" }
    }
    Check 'a built-in reader is copied when its size changes, and the lock keeps the shape' {
        Mouse-Click '.size-boxes .btn.icon'
        Eval "(() => { const el = document.querySelector('.size-boxes input'); const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set; set.call(el, '240'); el.dispatchEvent(new Event('change', { bubbles: true })); return true; })()" | Out-Null
        Wait-Until "[...document.querySelectorAll('.size-boxes input')].map(i => i.value).join('x') === '240x400'" 15 'the boxes to read 240x400'
        $reader = Eval "document.querySelector('.panel-section select').selectedOptions[0].textContent"
        if ($reader -notmatch 'My reader') { throw "the chosen reader is '$reader'" }
    }
    Check 'a column bar can be dragged' {
        $w0 = Eval "document.querySelector('.export-side.left').getBoundingClientRect().width"
        $pt = Eval "(() => { const r = document.querySelector('.px-split-v[data-split=left]').getBoundingClientRect(); return [r.left + 1.5, r.top + 200]; })()"
        $x = [double]$pt[0]; $y = [double]$pt[1]
        Send-Cdp 'Input.dispatchMouseEvent' @{ type='mouseMoved'; x=$x; y=$y } | Out-Null
        Send-Cdp 'Input.dispatchMouseEvent' @{ type='mousePressed'; x=$x; y=$y; button='left'; clickCount=1 } | Out-Null
        Send-Cdp 'Input.dispatchMouseEvent' @{ type='mouseMoved'; x=($x+60); y=$y; buttons=1 } | Out-Null
        Send-Cdp 'Input.dispatchMouseEvent' @{ type='mouseReleased'; x=($x+60); y=$y; button='left'; clickCount=1 } | Out-Null
        Start-Sleep -Milliseconds 300
        $w1 = Eval "document.querySelector('.export-side.left').getBoundingClientRect().width"
        if ([math]::Abs($w1 - $w0) -lt 20) {
            $hit = Eval "(() => { const e = document.elementFromPoint($x, $y); return (e ? e.className : 'none') + ' bound=' + document.querySelector('.export-screen').dataset.dragBound; })()"
            throw "width stayed $w0 (point hits: $hit)"
        }
    }
    Check 'clicking the chapter still works after dragging' {
        Mouse-Click '.px-chapter-open'
        Start-Sleep -Milliseconds 400
        if (-not (Eval "document.querySelector('.export-screen') !== null")) { throw 'the screen is gone' }
    }
    Check 'Ctrl+K goes to the search' {
        Send-Cdp 'Input.dispatchKeyEvent' @{ type='keyDown'; key='k'; code='KeyK'; modifiers=2; windowsVirtualKeyCode=75 } | Out-Null
        Send-Cdp 'Input.dispatchKeyEvent' @{ type='keyUp'; key='k'; code='KeyK'; modifiers=2; windowsVirtualKeyCode=75 } | Out-Null
        Wait-Until "document.querySelector('.nav-item.active').innerText.trim().startsWith('Search')" 10 'the Search page'
    }
}
finally {
    if ($app -and -not $app.HasExited) { Stop-Process -Id $app.Id -Force }
    if ($data -like (Join-Path ([IO.Path]::GetTempPath()) 'mangapoint-smoke-*')) { Remove-Item -LiteralPath $data -Recurse -Force -ErrorAction SilentlyContinue }
}
if ($failures.Count) { Write-Host "$($failures.Count) check(s) failed"; exit 1 }
Write-Host 'all checks passed'
