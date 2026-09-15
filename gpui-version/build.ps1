# SAP Login Manager GPUI - 编译脚本
# 主工具链: C:\Users\Yuqing.Zhang\rust-dev (被亚信安全拦截时自动切到备份)
# 备份工具链: C:\Users\Yuqing.Zhang\AppData\Local\Temp\rust
# 用法: 在 gpui-version 目录下执行 .\build.ps1

# 优先用 rust-dev，失败则回退到 Temp 工具链
$cargoHome = 'C:\Users\Yuqing.Zhang\rust-dev\cargo\bin\cargo.exe'
if (-not (Test-Path $cargoHome)) {
    $cargoHome = 'C:\Users\Yuqing.Zhang\AppData\Local\Temp\rust\cargo\bin\cargo.exe'
    $env:RUSTUP_HOME = 'C:\Users\Yuqing.Zhang\AppData\Local\Temp\rust\rustup'
    $env:CARGO_HOME  = 'C:\Users\Yuqing.Zhang\AppData\Local\Temp\rust\cargo'
} else {
    # 先试 rust-dev，如果 os error 5 则切到 Temp
    $env:RUSTUP_HOME = 'C:\Users\Yuqing.Zhang\rust-dev\rustup'
    $env:CARGO_HOME  = 'C:\Users\Yuqing.Zhang\rust-dev\cargo'
    $testResult = & $cargoHome build --release 2>&1 | Out-String
    if ($testResult -match 'os error 5|拒绝访问') {
        Write-Host 'rust-dev 被亚信安全拦截，切换到备份工具链 ...' -ForegroundColor Yellow
        $cargoHome = 'C:\Users\Yuqing.Zhang\AppData\Local\Temp\rust\cargo\bin\cargo.exe'
        $env:RUSTUP_HOME = 'C:\Users\Yuqing.Zhang\AppData\Local\Temp\rust\rustup'
        $env:CARGO_HOME  = 'C:\Users\Yuqing.Zhang\AppData\Local\Temp\rust\cargo'
    } else {
        # rust-dev 成功，直接输出结果
        $testResult -split "`n" | ForEach-Object { Write-Host $_ }
        $exe = 'target\release\sap-login-manager-gpui.exe'
        if (Test-Path $exe) {
            $info = Get-Item $exe
            Write-Host ''
            Write-Host "编译成功: $($info.Name), $([math]::Round($info.Length/1MB, 1)) MB, $($info.LastWriteTime)" -ForegroundColor Green
        }
        return
    }
}

Write-Host '开始编译 release ...'
& $cargoHome build --release 2>&1 | ForEach-Object {
    Write-Host $_
    if ($_ -match '^error') { $script:hasError = $true }
}

if ($script:hasError) {
    Write-Host ''
    Write-Host '编译失败' -ForegroundColor Red
    exit 1
} else {
    $exe = 'target\release\sap-login-manager-gpui.exe'
    if (Test-Path $exe) {
        $info = Get-Item $exe
        Write-Host ''
        Write-Host "编译成功: $($info.Name), $([math]::Round($info.Length/1MB, 1)) MB, $($info.LastWriteTime)" -ForegroundColor Green
    }
}
