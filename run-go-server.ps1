Set-Location C:\Users\admin\Desktop\Programming\worktree\server-go
$env:WT_SERVER_GRPC_ADDR = "127.0.0.1:9877"
$env:WT_SERVER_AUTH_MODE = "bearer"
$env:WT_SERVER_AUTH_TOKEN = "dev-secret"
$env:WT_SERVER_AUTH_TENANT = "acme"
$env:WT_SERVER_AUTH_ACCOUNT = "alice"
$env:WT_SERVER_AUTH_SCOPES = "staged:*"
$env:WT_SERVER_IAM_MODE = "policy"
go run ./cmd/wt-server
