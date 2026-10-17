# MedAssist OSS - Deployment Guide

**License:** GPL-3.0-or-later  
**Version:** 0.1.0  
**Last Updated:** 2026-10-16

---

## Overview

This guide covers deployment options for MedAssist OSS, from local development to production self-hosting.

---

## Prerequisites

### System Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| **OS** | Linux, macOS | Linux (Ubuntu 22.04+, Debian 12+) |
| **CPU** | 2 cores | 4+ cores |
| **RAM** | 2 GB | 4+ GB (8+ GB for AI inference) |
| **Disk** | 10 GB | 50+ GB (for blobs, AI models) |
| **Rust** | 1.75+ | Latest stable |
| **PostgreSQL** | 14+ | 15+ |

### Required Tools

- [Rust](https://rustup.rs/) - Language and toolchain
- [PostgreSQL](https://www.postgresql.org/) - Database
- [Git](https://git-scm.com/) - Version control
- [Docker](https://www.docker.com/) or [Podman](https://podman.io/) - Container runtime (optional)

---

## Installation

### 1. Clone Repository

```bash
# Clone the repository
git clone https://github.com/Lucasovski/MEDASSIST-OSS.git
cd MEDASSIST-OSS

# Check out the latest release or main branch
git checkout main
```

### 2. Install Rust Toolchain

```bash
# Install rustup (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Use the project's toolchain
rustup show  # Should show stable with rustfmt, clippy

# If toolchain is not installed:
rustup install stable
rustup component add rustfmt clippy
```

### 3. Configure Environment

```bash
# Copy example configuration
cp .env.example .env

# Edit .env with your settings
nano .env  # or use your preferred editor
```

**Required Environment Variables:**

```bash
# Database connection
DATABASE_URL=postgres://medassist:your-strong-password@localhost:5432/medassist

# Server configuration
MEDASSIST_BIND=127.0.0.1:8080
MEDASSIST_ENV=development  # or "production"

# Blob storage
MEDASSIST_BLOB_DIR=/var/lib/medassist/blobs

# Create blob directory
mkdir -p /var/lib/medassist/blobs
chown -R $USER:$USER /var/lib/medassist/blobs
```

### 4. Install PostgreSQL

#### Option A: Docker

```bash
# Run PostgreSQL container
docker run -d \
  --name medassist-pg \
  -e POSTGRES_USER=medassist \
  -e POSTGRES_PASSWORD=your-strong-password \
  -e POSTGRES_DB=medassist \
  -p 127.0.0.1:5432:5432 \
  -v medassist_pg_data:/var/lib/postgresql/data \
  postgres:15-alpine

# Verify it's running
docker exec medassist-pg psql -U medassist -c "SELECT version();"
```

#### Option B: Native Installation (Ubuntu/Debian)

```bash
# Install PostgreSQL
sudo apt update
sudo apt install postgresql postgresql-contrib

# Create user and database
sudo -u postgres psql -c "CREATE USER medassist WITH PASSWORD 'your-strong-password';"
sudo -u postgres psql -c "CREATE DATABASE medassist OWNER medassist;"
sudo -u postgres psql -c "GRANT ALL PRIVILEGES ON DATABASE medassist TO medassist;"

# Enable in pg_hba.conf (for local development)
echo "host    medassist    medassist    127.0.0.1/32    md5" | sudo tee -a /etc/postgresql/*/main/pg_hba.conf
sudo systemctl restart postgresql
```

---

## Database Setup

### Apply Migrations

```bash
# Install sqlx-cli (if not already installed)
cargo install sqlx-cli --no-default-features --features postgres

# Apply migrations
cargo run -p medassist-cli -- migrate

# Or use sqlx directly
sqlx database setup
sqlx migrate run
```

### Verify Database

```bash
# Connect to database
psql postgresql://medassist:your-strong-password@localhost:5432/medassist

# List tables
\dt

# Check schema version
SELECT * FROM _sqlx_migrations;
```

---

## Running the Application

### Development Mode

```bash
# Run API server
cargo run -p medassist-api

# In another terminal, run CLI (if needed)
cargo run -p medassist-cli
```

### Production Mode

```bash
# Build release binary
cargo build --release -p medassist-api

# Run with production configuration
MEDASSIST_ENV=production ./target/release/medassist-api
```

### Verify It's Running

```bash
# Check health
curl http://127.0.0.1:8080/healthz
# Expected: {"status":"ok","version":"0.1.0"}

# Check readiness (requires DB connection)
curl http://127.0.0.1:8080/readyz
# Expected: HTTP 200 if DB is reachable
```

---

## Docker Deployment

### Build Docker Image

```bash
# Create Dockerfile (see below for example)
# Build image
docker build -t medassist-oss:latest .

# Run container
docker run -d \
  --name medassist \
  -p 8080:8080 \
  -e DATABASE_URL=postgres://medassist:password@host.docker.internal:5432/medassist \
  -e MEDASSIST_BIND=0.0.0.0:8080 \
  -e MEDASSIST_ENV=production \
  -e MEDASSIST_BLOB_DIR=/data/blobs \
  -v medassist_blobs:/data/blobs \
  medassist-oss:latest
```

### Example Dockerfile

```dockerfile
# Build stage
FROM rust:1.75-slim as builder

WORKDIR /usr/src/medassist
COPY . .
RUN apt-get update && apt-get install -y pkg-config libssl-dev
RUN cargo build --release -p medassist-api

# Runtime stage
FROM debian:12-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy binary
COPY --from=builder /usr/src/medassist/target/release/medassist-api /usr/local/bin/

# Create blob directory
RUN mkdir -p /data/blobs
VOLUME ["/data/blobs"]

# Set non-root user
RUN useradd -r -s /bin/false medassist
USER medassist
WORKDIR /data

# Expose port
EXPOSE 8080

# Run
CMD ["medassist-api"]
```

### Docker Compose

```yaml
version: '3.8'

services:
  postgres:
    image: postgres:15-alpine
    environment:
      POSTGRES_USER: medassist
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD:-change-me}
      POSTGRES_DB: medassist
    volumes:
      - pg_data:/var/lib/postgresql/data
    ports:
      - "5432:5432"
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U medassist"]
      interval: 5s
      timeout: 5s
      retries: 5

  medassist:
    build: .
    environment:
      DATABASE_URL: postgres://medassist:${POSTGRES_PASSWORD:-change-me}@postgres:5432/medassist
      MEDASSIST_BIND: 0.0.0.0:8080
      MEDASSIST_ENV: production
      MEDASSIST_BLOB_DIR: /data/blobs
    volumes:
      - blob_data:/data/blobs
    ports:
      - "8080:8080"
    depends_on:
      postgres:
        condition: service_healthy

volumes:
  pg_data:
  blob_data:
```

---

## Configuration Reference

### Environment Variables

| Variable | Description | Default | Required |
|----------|-------------|---------|----------|
| `DATABASE_URL` | PostgreSQL connection string | - | Yes |
| `MEDASSIST_BIND` | Server address to bind to | `127.0.0.1:8080` | Yes |
| `MEDASSIST_ENV` | Environment mode | `development` | Yes |
| `MEDASSIST_BLOB_DIR` | Blob storage directory | - | Yes |
| `MEDASSIST_LLM_ID` | AI model identifier | `test-double` | No |
| `MEDASSIST_LLM_EXTERNAL` | Whether AI sends data externally | `false` | No |

### PostgreSQL Connection String Format

```
postgres://USER:PASSWORD@HOST:PORT/DATABASE
```

Examples:
- Local development: `postgres://medassist:password@localhost:5432/medassist`
- Docker: `postgres://medassist:password@postgres:5432/medassist`
- Production: `postgres://medassist:${DB_PASSWORD}@db.example.com:5432/medassist?sslmode=require`

### Development vs Production Mode

| Setting | Development | Production |
|---------|-------------|------------|
| Allows insecure defaults | Yes | No |
| Requires all config | No | Yes |
| Detailed error messages | Yes | No (generic) |
| Logging level | Debug | Info |

---

## Reverse Proxy Configuration

### Nginx Example

```nginx
server {
    listen 80;
    server_name medassist.example.com;
    return 301 https://$host$request_uri;
}

server {
    listen 443 ssl;
    server_name medassist.example.com;

    ssl_certificate /etc/letsencrypt/live/medassist.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/medassist.example.com/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header Cookie $http_cookie;

        # WebSocket support (future)
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }

    # Health checks
    location /healthz {
        proxy_pass http://127.0.0.1:8080/healthz;
        access_log off;
    }

    location /readyz {
        proxy_pass http://127.0.0.1:8080/readyz;
        access_log off;
    }
}
```

### Caddy Example

```caddyfile
medassist.example.com {
    reverse_proxy 127.0.0.1:8080
}
```

---

## First Administrator

### Option 1: SQL Bootstrap (Current)

Until the CLI `create-admin` command is implemented, use SQL:

```sql
-- Insert organization
INSERT INTO organizations (id, name, slug) 
VALUES ('<org-uuid>', 'My Organization', 'my-org');

-- Insert user
INSERT INTO users (id, org_id, email, password_hash, display_name, roles, created_at, updated_at) 
VALUES (
    '<user-uuid>',
    '<org-uuid>',
    'admin@example.com',
    '$argon2id$v=19$m=65536,t=3,p=4$...',  -- Use argon2 CLI to generate
    'Administrator',
    '{"admin": true, "clinician": true}',
    NOW(),
    NOW()
);

-- Create session (optional, for immediate login)
INSERT INTO sessions (id, user_id, org_id, token_hash, created_at, expires_at, revoked_at) 
VALUES (
    '<session-uuid>',
    '<user-uuid>',
    '<org-uuid>',
    '$argon2id$v=19$...',  -- Hash of session token
    NOW(),
    NOW() + INTERVAL '24 hours',
    NULL
);
```

### Generate Argon2 Hash

```bash
# Install argon2 CLI
cargo install argon2-cli

# Generate hash
argon2 some-password -e -id -t 3 -m 16 -p 4
# Output: $argon2id$v=19$m=65536,t=3,p=4$...
```

### Option 2: CLI Command (Future)

```bash
# When implemented:
cargo run -p medassist-cli -- create-admin \
  --email admin@example.com \
  --password your-strong-password \
  --org-name "My Organization" \
  --display-name "Administrator"
```

---

## Backup and Restore

### Database Backup

#### PostgreSQL Dump

```bash
# Full database dump
pg_dump -U medassist -h localhost -p 5432 medassist > medassist_backup.sql

# Compressed dump
pg_dump -U medassist -h localhost -p 5432 medassist | gzip > medassist_backup_$(date +%Y%m%d).sql.gz

# Custom format (recommended for large databases)
pg_dump -U medassist -h localhost -p 5432 -Fc medassist > medassist_backup.dump
```

#### Scheduled Backup

```bash
# Add to crontab (daily at 2am)
0 2 * * * pg_dump -U medassist -h localhost medassist | gzip > /backups/medassist_$(date +\%Y\%m\%d).sql.gz

# Keep last 30 days
0 3 * * * find /backups -name "medassist_*.sql.gz" -mtime +30 -delete
```

### Blob Store Backup

```bash
# Copy blob directory
tar -czvf medassist_blobs_$(date +%Y%m%d).tar.gz /var/lib/medassist/blobs

# Or use rsync
rsync -av /var/lib/medassist/blobs/ /backups/medassist/blobs/
```

### Restore from Backup

```bash
# Stop application
# systemctl stop medassist  # or docker stop medassist

# Restore database
psql -U medassist -h localhost -p 5432 medassist < medassist_backup.sql

# Or for custom format
pg_restore -U medassist -h localhost -p 5432 -d medassist medassist_backup.dump

# Restore blobs
rm -rf /var/lib/medassist/blobs/*
tar -xzvf medassist_blobs.tar.gz -C /

# Reapply migrations (if schema changed)
cargo run -p medassist-cli -- migrate

# Restart application
# systemctl start medassist
```

---

## Monitoring

### Health Endpoints

| Endpoint | Purpose | Expected Response |
|----------|---------|-----------------|
| `GET /healthz` | Liveness check | HTTP 200, `{"status":"ok","version":"..."}` |
| `GET /readyz` | Readiness check (includes DB) | HTTP 200 if DB connected, 503 otherwise |

### Metrics (Future)

- Prometheus metrics endpoint (planned)
- Request count and latency
- Database connection pool status
- Error rates

### Logging

**Log Levels:**
- `ERROR`: Critical failures, security events
- `WARN`: Unexpected but recoverable situations
- `INFO`: Normal operations, requests
- `DEBUG`: Detailed debugging (development only)
- `TRACE`: Very verbose (development only)

**Log Format:**
```json
{
  "timestamp": "2026-10-16T12:34:56.789Z",
  "level": "INFO",
  "target": "medassist_api::routes",
  "message": "Request completed",
  "method": "GET",
  "path": "/patients",
  "status": 200,
  "duration_ms": 45,
  "user_id": "...",
  "org_id": "..."
}
```

**Note:** Clinical content is **never** logged. Audit events record metadata only.

---

## Scaling

### Vertical Scaling

- Increase CPU/RAM for the application server
- PostgreSQL can handle significant load with proper tuning
- For AI inference: GPU recommended for local models

### Horizontal Scaling (Future)

- Stateless API servers can be scaled horizontally
- PostgreSQL read replicas for read-heavy workloads
- Redis for session caching (future)
- Shared blob storage (S3-compatible) for multi-instance deployments

### Database Tuning

```conf
# postgresql.conf recommendations
shared_buffers = 4GB                   # 25% of total RAM
work_mem = 16MB                        # For complex sorts
maintenance_work_mem = 512MB          # For VACUUM, index creation
effective_cache_size = 12GB           # 75% of total RAM
max_connections = 100                 # Adjust based on application needs

# For MedAssist OSS workload (many reads, some writes)
random_page_cost = 1.1                # SSD storage
max_worker_processes = 4              # Number of CPU cores
max_parallel_workers_per_gather = 2
max_parallel_workers = 8
```

---

## Security Hardening

### PostgreSQL

```sql
-- Create dedicated user with limited privileges
CREATE USER medassist_app WITH PASSWORD 'strong-password';
GRANT CONNECT ON DATABASE medassist TO medassist_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO medassist_app;
GRANT USAGE ON ALL SEQUENCES IN SCHEMA public TO medassist_app;

-- For read-only reporting user
CREATE USER medassist_ro WITH PASSWORD 'strong-password';
GRANT CONNECT ON DATABASE medassist TO medassist_ro;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO medassist_ro;
```

### Application

- Run as non-root user
- Use read-only filesystem where possible
- Enable SELinux/AppArmor (Linux)
- Use container security (if using Docker)

### Network

- Firewall: Only allow 8080 from trusted IPs
- Database: Only allow from application server
- Rate limiting: Configure at reverse proxy
- Fail2Ban: For SSH and repeated failed logins

---

## Upgrading

### Application Upgrade

```bash
# Pull latest code
git pull origin main

# Update dependencies
cargo update

# Rebuild
cargo build --release -p medassist-api

# Apply migrations (if any)
cargo run -p medassist-cli -- migrate

# Restart service
systemctl restart medassist
```

### Database Migration

Migrations are applied automatically via `sqlx`:

```bash
# Check for new migrations
sqlx migrate info

# Apply pending migrations
sqlx migrate run

# Rollback (if needed)
sqlx migrate revert
```

---

## Troubleshooting

### Common Issues

#### 1. Database Connection Failed

```
Error: connection to server at "localhost:5432" failed
```

**Solutions:**
- Verify PostgreSQL is running: `pg_isready`
- Check connection string in `.env`
- Verify credentials
- Check firewall settings

#### 2. Migration Failed

```
Error: migration "20261017000001_schema.sql" failed
```

**Solutions:**
- Check PostgreSQL version (requires 14+)
- Verify database user has sufficient privileges
- Check for existing tables (migrations must be applied in order)

#### 3. Blob Store Permission Denied

```
Error: Permission denied (os error 13)
```

**Solutions:**
- Create blob directory: `mkdir -p /var/lib/medassist/blobs`
- Set permissions: `chown -R medassist:medassist /var/lib/medassist/blobs`
- Verify `MEDASSIST_BLOB_DIR` points to correct path

#### 4. Port Already in Use

```
Error: Address already in use (os error 98)
```

**Solutions:**
- Check running processes: `lsof -i :8080`
- Kill existing process: `kill <PID>`
- Or change `MEDASSIST_BIND` to different port

#### 5. Out of Memory

**Solutions:**
- Increase system memory
- Reduce PostgreSQL `shared_buffers`
- Use smaller AI models
- Enable swap (temporary solution)

---

## Support

### Getting Help

1. **Documentation:** Check this guide and other docs
2. **Issues:** Open an issue on GitHub with:
   - Rust version (`rustc --version`)
   - OS and version
   - Exact error message
   - Steps to reproduce
3. **Community:** (Future: Discord, forum)

### Reporting Security Issues

See [SECURITY.md](../SECURITY.md) for vulnerability reporting procedures.

---

## Checklist: Production Deployment

- [ ] PostgreSQL installed and configured
- [ ] Database created with proper user
- [ ] Migrations applied
- [ ] Environment variables configured
- [ ] Blob storage directory created
- [ ] Reverse proxy configured with TLS
- [ ] Firewall configured
- [ ] Backup strategy in place
- [ ] Monitoring configured
- [ ] First administrator created
- [ ] Application running and healthy
- [ ] Health endpoints responding
