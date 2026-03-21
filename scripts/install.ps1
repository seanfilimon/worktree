Write-Host "Worktree Installer" -ForegroundColor Cyan
Write-Host "===================" -ForegroundColor Cyan
Write-Host ""
Write-Host "Building from source..."
cargo build --release
Write-Host ""
Write-Host "Installing binaries..."
# TODO: Copy binaries to appropriate locations
# TODO: Register Windows service
Write-Host "Installation complete." -ForegroundColor Green
