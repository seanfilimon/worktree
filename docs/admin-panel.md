# Worktree Admin Panel Documentation

## Overview

The Worktree Admin Panel is a web-based management interface for monitoring and controlling worktree-server instances. It provides a RESTful API and real-time insights into server operations, repository management, and system health.

## Table of Contents

- [Installation](#installation)
- [Configuration](#configuration)
- [API Reference](#api-reference)
- [Authentication](#authentication)
- [Monitoring & Metrics](#monitoring--metrics)
- [Server Management](#server-management)
- [Repository Operations](#repository-operations)
- [Maintenance Tasks](#maintenance-tasks)
- [Deployment](#deployment)
- [Troubleshooting](#troubleshooting)

## Installation

### From Source

```bash
cd crates/worktree-admin
cargo build --release
```

The compiled binary will be available at `target/release/worktree-admin`.

### Using Cargo

```bash
cargo install --path crates/worktree-admin
```

## Configuration

The admin panel can be configured using either a TOML configuration file or environment variables.

### Configuration File

Create a `config.toml` file:

```toml
[server]
host = "127.0.0.1"
port = 8080
worker_threads = 0              # 0 = auto-detect based on CPU cores
max_connections = 1000
request_timeout_secs = 30

[worktree]
server_endpoint = "/tmp/worktree.sock"
connection_timeout_secs = 10
reconnect_interval_secs = 5
max_reconnect_attempts = 0      # 0 = infinite retries

[security]
auth_enabled = true
api_key = "your-secret-api-key-change-me"
cors_enabled = true
cors_origins = []               # Empty = allow all origins
tls_enabled = false
tls_cert_path = "/path/to/cert.pem"
tls_key_path = "/path/to/key.pem"

[logging]
level = "info"                  # trace, debug, info, warn, error
format = "pretty"               # pretty or json
file_enabled = false
file_path = "/var/log/worktree-admin.log"
max_file_size_mb = 100
```

### Environment Variables

All configuration options can be overridden using environment variables:

| Variable | Description | Default |
|----------|-------------|---------|
| `WORKTREE_ADMIN_CONFIG` | Path to config file | None |
| `WORKTREE_ADMIN_HOST` | Host address to bind | `127.0.0.1` |
| `WORKTREE_ADMIN_PORT` | Port number | `8080` |
| `WORKTREE_SERVER_ENDPOINT` | Worktree server socket/endpoint | `/tmp/worktree.sock` |
| `WORKTREE_ADMIN_API_KEY` | API key for authentication | None |
| `RUST_LOG` | Log level | `info` |

### Example: Starting with Environment Variables

```bash
export WORKTREE_ADMIN_HOST="0.0.0.0"
export WORKTREE_ADMIN_PORT="9090"
export WORKTREE_SERVER_ENDPOINT="/var/run/worktree.sock"
export WORKTREE_ADMIN_API_KEY="supersecret123"
export RUST_LOG="info,worktree_admin=debug"

worktree-admin
```

## API Reference

### Base URL

All API endpoints are prefixed with `/api`:

```
http://localhost:8080/api
```

### Response Format

All responses follow a standard JSON format:

**Success Response:**
```json
{
  "status": "success",
  "message": "Operation completed",
  "data": { }
}
```

**Error Response:**
```json
{
  "status": "error",
  "code": "ERROR_CODE",
  "message": "Human-readable error message"
}
```

### Health & Status Endpoints

#### GET /api/health

Health check endpoint for monitoring tools.

**Response:**
```json
{
  "status": "healthy",
  "timestamp": "2024-01-15T10:30:00Z"
}
```

#### GET /api/status

Get current server status and connection information.

**Response:**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "worktree-server",
  "running": true,
  "uptime_seconds": 86400,
  "active_connections": 12,
  "tracked_repositories": 25,
  "last_updated": "2024-01-15T10:30:00Z"
}
```

#### GET /api/metrics

Get admin panel and connection metrics.

**Response:**
```json
{
  "admin_panel": {
    "uptime_seconds": 172800,
    "total_requests": 1500,
    "total_errors": 3,
    "error_rate": 0.002
  },
  "server_connection": {
    "connected": true,
    "connection_attempts": 5,
    "failed_connections": 0,
    "last_connected": "2024-01-15T08:00:00Z"
  }
}
```

### Server Control Endpoints

#### POST /api/server/start

Start the worktree server.

**Response:**
```json
{
  "status": "success",
  "message": "Server started successfully"
}
```

#### POST /api/server/stop

Stop the worktree server gracefully.

**Response:**
```json
{
  "status": "success",
  "message": "Server stopped successfully"
}
```

#### POST /api/server/restart

Restart the worktree server.

**Response:**
```json
{
  "status": "success",
  "message": "Server restarted successfully"
}
```

### Repository Endpoints

#### GET /api/repositories

List all tracked repositories.

**Response:**
```json
[
  {
    "id": "123e4567-e89b-12d3-a456-426614174000",
    "path": "/home/user/projects/myapp",
    "branch_count": 5,
    "commit_count": 342,
    "last_activity": "2024-01-15T10:25:00Z",
    "size_bytes": 52428800
  },
  {
    "id": "987fcdeb-51a2-43f1-b9c8-123456789abc",
    "path": "/home/user/projects/library",
    "branch_count": 3,
    "commit_count": 158,
    "last_activity": "2024-01-14T16:30:00Z",
    "size_bytes": 20971520
  }
]
```

#### GET /api/repositories/:id

Get detailed information about a specific repository.

**Parameters:**
- `id` (UUID): Repository identifier

**Response:**
```json
{
  "id": "123e4567-e89b-12d3-a456-426614174000",
  "path": "/home/user/projects/myapp",
  "branch_count": 5,
  "commit_count": 342,
  "last_activity": "2024-01-15T10:25:00Z",
  "size_bytes": 52428800
}
```

### Statistics Endpoints

#### GET /api/stats

Get aggregated statistics across all repositories.

**Response:**
```json
{
  "total_repositories": 25,
  "total_commits": 8450,
  "total_branches": 87,
  "total_storage_bytes": 1073741824,
  "total_operations": 15230,
  "collected_at": "2024-01-15T10:30:00Z"
}
```

### Maintenance Endpoints

#### POST /api/maintenance/gc

Trigger garbage collection to reclaim disk space.

**Response:**
```json
{
  "status": "success",
  "message": "Garbage collection completed",
  "data": {
    "reclaimed_bytes": 5242880,
    "duration_ms": 1250
  }
}
```

## Authentication

When authentication is enabled (`auth_enabled = true`), all API requests must include an Authorization header.

### Authorization Header Format

```
Authorization: Bearer <api_key>
```

### Example Request

```bash
curl -H "Authorization: Bearer your-secret-api-key" \
     http://localhost:8080/api/status
```

### Error Responses

**401 Unauthorized** - Missing or invalid API key:
```json
{
  "status": "error",
  "code": "AUTH_FAILED",
  "message": "Invalid API key"
}
```

**403 Forbidden** - Valid API key but insufficient permissions:
```json
{
  "status": "error",
  "code": "NOT_AUTHORIZED",
  "message": "Not authorized to perform this operation"
}
```

## Monitoring & Metrics

### Prometheus-Compatible Metrics

The admin panel exposes metrics in a format compatible with Prometheus and other monitoring tools.

**Endpoint:** `GET /api/metrics`

### Key Metrics

| Metric | Description | Type |
|--------|-------------|------|
| `admin_panel_uptime_seconds` | Admin panel uptime | Gauge |
| `admin_panel_requests_total` | Total requests received | Counter |
| `admin_panel_errors_total` | Total errors encountered | Counter |
| `server_connection_status` | Server connection status (0=disconnected, 1=connected) | Gauge |
| `server_repositories_total` | Number of tracked repositories | Gauge |
| `server_commits_total` | Total commits across all repositories | Gauge |
| `server_storage_bytes` | Total storage used | Gauge |

### Health Checks

For load balancers and orchestration tools:

```bash
# Returns 200 OK if healthy
curl http://localhost:8080/api/health
```

## Server Management

### Starting the Server

```bash
# Start with default configuration
worktree-admin

# Start with custom config
worktree-admin --config /etc/worktree/admin.toml

# Start on specific port
WORKTREE_ADMIN_PORT=9090 worktree-admin
```

### Stopping the Server

Send SIGTERM or SIGINT (Ctrl+C) for graceful shutdown:

```bash
kill -TERM $(pgrep worktree-admin)
```

### Running as a Service

#### systemd (Linux)

Create `/etc/systemd/system/worktree-admin.service`:

```ini
[Unit]
Description=Worktree Admin Panel
After=network.target worktree-server.service
Requires=worktree-server.service

[Service]
Type=simple
User=worktree
Group=worktree
Environment="WORKTREE_ADMIN_PORT=8080"
Environment="WORKTREE_SERVER_ENDPOINT=/var/run/worktree.sock"
Environment="RUST_LOG=info"
ExecStart=/usr/local/bin/worktree-admin
Restart=on-failure
RestartSec=5s

[Install]
WantedBy=multi-user.target
```

Enable and start:

```bash
sudo systemctl daemon-reload
sudo systemctl enable worktree-admin
sudo systemctl start worktree-admin
```

## Repository Operations

### Viewing Repository Details

```bash
# List all repositories
curl http://localhost:8080/api/repositories

# Get specific repository
curl http://localhost:8080/api/repositories/123e4567-e89b-12d3-a456-426614174000
```

### Repository Metrics

Each repository includes:
- **Path**: Filesystem path to the repository
- **Branch Count**: Number of branches
- **Commit Count**: Total number of commits
- **Last Activity**: Timestamp of most recent operation
- **Size**: Total disk space used in bytes

## Maintenance Tasks

### Garbage Collection

Periodically run garbage collection to reclaim disk space:

```bash
curl -X POST http://localhost:8080/api/maintenance/gc
```

This will:
1. Remove unreferenced objects
2. Compress object database
3. Optimize repository structures
4. Return statistics about space reclaimed

### Scheduled Maintenance

Set up a cron job for automatic maintenance:

```bash
# Run garbage collection daily at 2 AM
0 2 * * * curl -X POST http://localhost:8080/api/maintenance/gc
```

## Deployment

### Production Deployment Checklist

- [ ] Enable authentication with strong API key
- [ ] Configure TLS/HTTPS
- [ ] Set up proper logging
- [ ] Configure CORS for specific origins
- [ ] Use reverse proxy (nginx/caddy)
- [ ] Set up monitoring and alerting
- [ ] Configure firewall rules
- [ ] Use systemd or equivalent for process management
- [ ] Set up log rotation
- [ ] Configure backups

### Reverse Proxy Configuration

#### nginx

```nginx
server {
    listen 443 ssl http2;
    server_name admin.example.com;

    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

### Docker Deployment

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release --bin worktree-admin

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/worktree-admin /usr/local/bin/
EXPOSE 8080
CMD ["worktree-admin"]
```

## Troubleshooting

### Common Issues

#### Cannot Connect to Worktree Server

**Symptom:** `SERVER_CONNECTION` errors in logs

**Solutions:**
1. Verify worktree-server is running:
   ```bash
   ps aux | grep worktree-server
   ```
2. Check socket file exists and has correct permissions:
   ```bash
   ls -la /tmp/worktree.sock
   ```
3. Verify endpoint configuration matches server socket
4. Check server logs for errors

#### Port Already in Use

**Symptom:** `Address already in use` error on startup

**Solutions:**
1. Check what's using the port:
   ```bash
   lsof -i :8080
   ```
2. Use a different port:
   ```bash
   WORKTREE_ADMIN_PORT=8081 worktree-admin
   ```

#### Authentication Failures

**Symptom:** 401 Unauthorized responses

**Solutions:**
1. Verify API key is correct
2. Check Authorization header format: `Bearer <key>`
3. Ensure `auth_enabled = true` in config if using API key
4. Check for extra whitespace in API key

#### High Memory Usage

**Symptom:** Admin panel consuming excessive memory

**Solutions:**
1. Check for connection leaks in metrics endpoint
2. Reduce `max_connections` in configuration
3. Enable request timeout
4. Review logs for error patterns

### Debug Logging

Enable detailed logging for troubleshooting:

```bash
RUST_LOG=debug,worktree_admin=trace worktree-admin
```

### Log Files

Default log locations:
- **stdout/stderr**: Console output
- **syslog**: System logs (if configured)
- **Custom file**: As configured in `logging.file_path`

### Getting Help

1. Check the logs: `journalctl -u worktree-admin`
2. Verify configuration: Review `config.toml`
3. Test connectivity: `curl http://localhost:8080/api/health`
4. Review server status: `curl http://localhost:8080/api/status`

## Best Practices

### Security

1. **Always use authentication in production**
2. **Enable TLS/HTTPS for remote access**
3. **Rotate API keys regularly**
4. **Restrict CORS origins to specific domains**
5. **Use firewall rules to limit access**
6. **Run as unprivileged user**
7. **Keep logs secure and rotate them**

### Performance

1. **Use connection pooling for database access**
2. **Enable HTTP/2 for better performance**
3. **Configure appropriate timeouts**
4. **Monitor metrics regularly**
5. **Scale horizontally if needed**

### Monitoring

1. **Set up health check monitoring**
2. **Alert on high error rates**
3. **Track response times**
4. **Monitor disk usage**
5. **Set up log aggregation**

## See Also

- [Server Architecture](server-architecture.md)
- [Protocol Specification](protocol-spec.md)
- [CLI Reference](cli-reference.md)
- [SDK Guide](sdk-guide.md)