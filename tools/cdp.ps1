# Tiny Chrome-DevTools-Protocol driver for the app's web view (developer testing only; start the app with MANGAPOINT_DEBUG_PORT).
# Dot-source this file, then call Connect-App <port> and use Eval / Wait-Until.
$script:ws = $null
$script:msgId = 0

function Connect-App([int]$Port) {
    $targets = Invoke-RestMethod "http://127.0.0.1:$Port/json"
    $page = $targets | Where-Object { $_.type -eq 'page' } | Select-Object -First 1
    if (-not $page) { throw "no page target on port $Port" }
    $script:ws = [System.Net.WebSockets.ClientWebSocket]::new()
    $script:ws.ConnectAsync([Uri]$page.webSocketDebuggerUrl, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
}

function Send-Cdp([string]$Method, $Params = @{}) {
    $script:msgId++
    $id = $script:msgId
    $json = @{ id = $id; method = $Method; params = $Params } | ConvertTo-Json -Depth 10 -Compress
    $bytes = [Text.Encoding]::UTF8.GetBytes($json)
    $script:ws.SendAsync([ArraySegment[byte]]::new($bytes), 'Text', $true, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
    $buffer = New-Object byte[] 1048576
    while ($true) {
        $sb = [Text.StringBuilder]::new()
        do {
            $r = $script:ws.ReceiveAsync([ArraySegment[byte]]::new($buffer), [Threading.CancellationToken]::None).GetAwaiter().GetResult()
            [void]$sb.Append([Text.Encoding]::UTF8.GetString($buffer, 0, $r.Count))
        } while (-not $r.EndOfMessage)
        $msg = $sb.ToString() | ConvertFrom-Json
        if ($msg.id -eq $id) { return $msg }
    }
}

# Evaluate JavaScript in the page and return its value.
function Eval([string]$Js) {
    $res = Send-Cdp 'Runtime.evaluate' @{ expression = $Js; returnByValue = $true; awaitPromise = $true }
    if ($res.result.exceptionDetails) { throw ("JS error: " + $res.result.exceptionDetails.text + " " + $res.result.exceptionDetails.exception.description) }
    return $res.result.result.value
}

# Poll a JavaScript condition until it is truthy or the time runs out.
function Wait-Until([string]$Js, [int]$Seconds = 60, [string]$What = $Js) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        if (Eval $Js) { return }
        Start-Sleep -Milliseconds 500
    }
    throw "timed out after ${Seconds}s waiting for: $What"
}

function Click-Button([string]$TextStartsWith) {
    $ok = Eval "(() => { const b = [...document.querySelectorAll('button')].find(b => b.textContent.trim().startsWith('$TextStartsWith') && !b.disabled); if (!b) return false; b.click(); return true; })()"
    if (-not $ok) { throw "no enabled button starting with '$TextStartsWith'" }
}

function Set-Input([string]$Selector, [string]$Value) {
    $ok = Eval "(() => { const el = document.querySelector('$Selector'); if (!el) return false; const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set; set.call(el, '$Value'); el.dispatchEvent(new Event('input', { bubbles: true })); return true; })()"
    if (-not $ok) { throw "no input matching $Selector" }
}

# Save a screenshot of just the web view (not the desktop).
function Save-Shot([string]$Path) {
    $res = Send-Cdp 'Page.captureScreenshot' @{ format = 'png' }
    [IO.File]::WriteAllBytes($Path, [Convert]::FromBase64String($res.result.data))
}

# A real mouse click at the middle of the first element matching a selector (hit-tested, with pointer events),
# unlike Click-Button which calls element.click() and skips all of that.
function Mouse-Click([string]$Selector, [string]$TextLike = '') {
    $js = "(() => { const els = [...document.querySelectorAll('$Selector')]; const el = '$TextLike' ? els.find(e => new RegExp('$TextLike', 'i').test(e.innerText)) : els[0]; if (!el) return null; el.scrollIntoView({block:'center'}); const r = el.getBoundingClientRect(); return [r.left + r.width/2, r.top + r.height/2]; })()"
    $pt = Eval $js
    if (-not $pt) { throw "nothing matches $Selector $TextLike" }
    $x = [double]$pt[0]; $y = [double]$pt[1]
    Send-Cdp 'Input.dispatchMouseEvent' @{ type='mouseMoved'; x=$x; y=$y } | Out-Null
    Send-Cdp 'Input.dispatchMouseEvent' @{ type='mouseMoved'; x=($x+1); y=$y } | Out-Null
    Send-Cdp 'Input.dispatchMouseEvent' @{ type='mousePressed'; x=$x; y=$y; button='left'; clickCount=1 } | Out-Null
    Send-Cdp 'Input.dispatchMouseEvent' @{ type='mouseReleased'; x=$x; y=$y; button='left'; clickCount=1 } | Out-Null
}
