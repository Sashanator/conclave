# Meetily GPU build wrapper: calls build-gpu.bat (unsigned build, no signing secrets needed)

Write-Host ""
Write-Host "========================================"
Write-Host "   Meetily GPU Build"
Write-Host "========================================"
Write-Host ""

# Call the main build-gpu.bat script
Write-Host "🚀 Starting build process..."
Write-Host ""

& ".\build-gpu.bat" $args

$buildExitCode = $LASTEXITCODE

if ($buildExitCode -eq 0) {
    Write-Host ""
    Write-Host "========================================" -ForegroundColor Green
    Write-Host "✅ Build completed successfully!" -ForegroundColor Green
    Write-Host "========================================" -ForegroundColor Green
    Write-Host ""
} else {
    Write-Host ""
    Write-Host "❌ Build failed with exit code: $buildExitCode" -ForegroundColor Red
}

exit $buildExitCode
