# MedAssist OSS - Deployment Guide

**License:** GPL-3.0-or-later  
**Status:** Work in Progress  
**Last Updated:** 2026-10-16

---

## Overview

This guide provides comprehensive instructions for deploying MedAssist OSS in various environments, from local development to production. MedAssist OSS is designed for **self-hosting**, giving you full control over your data and infrastructure.

**Target Audience:**
- System administrators
- DevOps engineers
- Healthcare IT professionals
- Developers

---

## Prerequisites

### Hardware Requirements

| Component | Minimum | Recommended | Production |
|-----------|---------|-------------|------------|
| **CPU** | 2 cores | 4 cores | 8+ cores |
| **RAM** | 4 GB | 8 GB | 16+ GB |
| **Storage** | 20 GB | 50 GB | 100+ GB (SSD) |
| **Network** | 100 Mbps | 1 Gbps | 1 Gbps+ |

### Software Requirements

| Component | Version | Notes |
|-----------|---------|-------|
| **Operating System** | Linux (x86_64) | Ubuntu 22.04 LTS, Debian 12, or equivalent |
| **Rust** | 1.75+ | [Install Rust](https://www.rust-lang.org/tools/install) |
| **PostgreSQL** | 14+ | [Install PostgreSQL](https://www.postgresql.org/download/) |
| **Git** | 2.30+ | [Install Git](https://git-scm.com/downloads) |
| **Docker** (optional) | 20.10+ | For containerized deployment |
| **Podman** (optional) | 4.0+ | Docker alternative |

### Knowledge Requirements

- Basic Linux system administration
- PostgreSQL administration
- Rust/Cargo build system
- Network configuration
- Security best practices

---

## Installation Methods

Choose the installation method that best fits your environment:

| Method | Complexity | Use Case | Notes |
|--------|------------|----------|-------|
| **Source** | Medium | Development, Production | Full control, custom configuration |
| **Docker** | Easy | Development, Production | Containerized, reproducible |
| **Docker Compose** | Easy | Development, Production | Multi-container, easy setup |
| **Kubernetes** | High | Production (Future) | Scalable, highly available |

---

## Method 1: Source Installation

### Step 1: Clone the Repository

```bash
# Clone the repository
git clone https://github.com/Lucasovski/MEDASSIST-OSS.git
cd MEDASSIST-OSS

# Check out the latest stable version
git checkout main
```

### Step 2: Install Rust Toolchain

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add Rust to PATH
source "$HOME/.cargo/env"

# Verify installation
rustc --version
cargo --version
```

### Step 3: Install PostgreSQL

#### Ubuntu/Debian:

```bash
# Install PostgreSQL
sudo apt update
sudo apt install -y postgresql postgresql-contrib

# Start PostgreSQL service
sudo systemctl start postgresql
sudo systemctl enable postgresql

# Verify installation
sudo -u postgres psql --version
```

#### CentOS/RHEL:

```bash
# Install PostgreSQL
sudo dnf install -y postgresql-server postgresql-contrib

# Initialize database
sudo postgresql-setup --initdb

# Start PostgreSQL service
sudo systemctl start postgresql
sudo systemctl enable postgresql
```

### Step 4: Configure PostgreSQL

#### Create Database and User:

```bash
# Switch to postgres user
sudo -u postgres psql

-- Create database
CREATE DATABASE medassist;

-- Create user
CREATE USER medassist WITH PASSWORD 'secure_password_here';

-- Grant privileges
GRANT ALL PRIVILEGES ON DATABASE medassist TO medassist;

-- Exit
\q
```

#### Configure PostgreSQL for Production:

Edit `/etc/postgresql/14/main/postgresql.conf` (path may vary):

```ini
# Connection settings
max_connections = 100
shared_buffers = 4GB
effective_cache_size = 12GB
maintenance_work_mem = 1GB
work_mem = 16MB

# Security
ssl = on
ssl_cert_file = '/etc/ssl/certs/ssl-cert-snakeoil.pem'
ssl_key_file = '/etc/ssl/private/ssl-cert-snakeoil.key'

# Logging
logging_collector = on
log_directory = '/var/log/postgresql'
log_statement = 'all'
```

Edit `/etc/postgresql/14/main/pg_hba.conf`:

```ini
# TYPE  DATABASE        USER            ADDRESS                 METHOD
# Admin access
local   all             postgres                                peer

# Local connections
local   medassist       medassist                              md5

# Remote connections (adjust as needed)
host    medassist       medassist      127.0.0.1/32            md5
host    medassist       medassist      ::1/128                 md5

# For production, restrict to specific IPs
host    medassist       medassist      192.168.1.0/24          md5
```

Restart PostgreSQL:

```bash
sudo systemctl restart postgresql
```

### Step 5: Build MedAssist OSS

```bash
# Build in release mode
cargo build --release --workspace

# Verify build
ls -lh target/release/medassist-api
target/release/medassist-cli
```

### Step 6: Configure Environment

Create `.env` file in the project root:

```bash
# Copy example file
cp .env.example .env

# Edit .env file
nano .env
```

Example `.env` file:

```ini
# Database configuration
DATABASE_URL=postgres://medassist:secure_password_here@localhost:5432/medassist

# Application configuration
RUST_LOG=info
SECRET_KEY=generate_a_secure_random_key_here
PORT=3000
HOST=0.0.0.0
MAX_CONNECTIONS=10

# AI configuration (optional)
OLLAMA_URL=http://localhost:11434

# Blob store configuration
BLOB_STORE_PATH=./data/blob-store
```

**Generate a secure secret key:**

```bash
# Generate a 32-byte random key
openssl rand -base64 32
```

### Step 7: Initialize Database

```bash
# Run migrations
./target/release/medassist-cli migrate

# Or create database and run migrations
./target/release/medassist-cli setup
```

### Step 8: Create Administrator User

```bash
# Create admin user
./target/release/medassist-cli create-admin \
    --org-name "My Healthcare Organization" \
    --email admin@example.com \
    --password "secure_admin_password" \
    --display-name "Admin User"
```

### Step 9: Run the Application

```bash
# Run API server
./target/release/medassist-api

# Or with custom configuration
RUST_LOG=debug ./target/release/medassist-api
```

### Step 10: Verify Installation

```bash
# Check health endpoint
curl http://localhost:3000/healthz

# Check readiness endpoint (requires DB connection)
curl http://localhost:3000/readyz

# Login with admin credentials
curl -X POST http://localhost:3000/login \
    -H "Content-Type: application/json" \
    -d '{"email": "admin@example.com", "password": "secure_admin_password"}'
```

---

## Method 2: Docker Installation

### Prerequisites

- Docker 20.10+
- Docker Compose 2.0+

### Step 1: Clone the Repository

```bash
git clone https://github.com/Lucasovski/MEDASSIST-OSS.git
cd MEDASSIST-OSS
```

### Step 2: Create Docker Compose File

Create `docker-compose.yml`:

```yaml
version: '3.8'

services:
  # PostgreSQL Database
  postgres:
    image: postgres:14-alpine
    container_name: medassist-postgres
    environment:
      POSTGRES_DB: medassist
      POSTGRES_USER: medassist
      POSTGRES_PASSWORD: secure_password_here
    volumes:
      - postgres_data:/var/lib/postgresql/data
    ports:
      - "5432:5432"
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U medassist -d medassist"]
      interval: 5s
      timeout: 5s
      retries: 5
    restart: unless-stopped

  # MedAssist API
  api:
    build:
      context: .
      dockerfile: Dockerfile.api
    container_name: medassist-api
    environment:
      DATABASE_URL: postgres://medassist:secure_password_here@postgres:5432/medassist
      SECRET_KEY: generate_a_secure_random_key_here
      RUST_LOG: info
      PORT: 3000
      HOST: 0.0.0.0
    ports:
      - "3000:3000"
    depends_on:
      postgres:
        condition: service_healthy
    restart: unless-stopped

  # MedAssist CLI (for initialization)
  cli:
    build:
      context: .
      dockerfile: Dockerfile.cli
    container_name: medassist-cli
    environment:
      DATABASE_URL: postgres://medassist:secure_password_here@postgres:5432/medassist
    volumes:
      - ./migrations:/app/migrations:ro
    depends_on:
      postgres:
        condition: service_healthy
    # Run once for initialization
    command: ["/app/medassist-cli", "setup"]

  # Ollama for AI (optional)
  ollama:
    image: ollama/ollama:latest
    container_name: medassist-ollama
    ports:
      - "11434:11434"
    volumes:
      - ollama_data:/root/.ollama
    restart: unless-stopped
    # GPU support (uncomment if you have NVIDIA GPU)
    # deploy:
    #   resources:
    #     reservations:
    #       devices:
    #         - driver: nvidia
    #           count: 1
    #           capabilities: [gpu]

volumes:
  postgres_data:
  ollama_data:
```

### Step 3: Create Dockerfiles

Create `Dockerfile.api`:

```dockerfile
# Build stage
FROM rust:1.75-slim AS builder

WORKDIR /app

# Copy source code
COPY . .

# Build release binary
RUN cargo build --release -p medassist-api

# Runtime stage
FROM debian:12-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from build stage
COPY --from=builder /app/target/release/medassist-api .

# Copy migrations
COPY --from=builder /app/migrations ./migrations

# Create non-root user
RUN useradd -r -u 1000 -g 1000 medassist
USER medassist

EXPOSE 3000

CMD ["./medassist-api"]
```

Create `Dockerfile.cli`:

```dockerfile
# Build stage
FROM rust:1.75-slim AS builder

WORKDIR /app

# Copy source code
COPY . .

# Build release binary
RUN cargo build --release -p medassist-cli

# Runtime stage
FROM debian:12-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from build stage
COPY --from=builder /app/target/release/medassist .

# Create non-root user
RUN useradd -r -u 1000 -g 1000 medassist
USER medassist

CMD ["./medassist"]
```

### Step 4: Build and Start Containers

```bash
# Build containers
docker-compose build

# Start services
docker-compose up -d postgres

# Wait for PostgreSQL to be ready
docker-compose logs -f postgres

# Run database setup (one-time)
docker-compose run --rm cli

# Start API server
docker-compose up -d api

# Start Ollama (optional)
docker-compose up -d ollama
```

### Step 5: Create Administrator User

```bash
# Run CLI command to create admin
docker-compose run --rm cli create-admin \
    --org-name "My Healthcare Organization" \
    --email admin@example.com \
    --password "secure_admin_password" \
    --display-name "Admin User"
```

### Step 6: Verify Installation

```bash
# Check health endpoint
curl http://localhost:3000/healthz

# Check readiness endpoint
curl http://localhost:3000/readyz
```

---

## Method 3: Docker Compose (Development)

For development purposes, use the provided `docker-compose.dev.yml`:

```yaml
version: '3.8'

services:
  postgres:
    image: postgres:14-alpine
    container_name: medassist-postgres-dev
    environment:
      POSTGRES_DB: medassist
      POSTGRES_USER: medassist
      POSTGRES_PASSWORD: dev_password
    volumes:
      - postgres_dev_data:/var/lib/postgresql/data
    ports:
      - "5432:5432"
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U medassist -d medassist"]
      interval: 5s
      timeout: 5s
      retries: 5

  ollama:
    image: ollama/ollama:latest
    container_name: medassist-ollama-dev
    ports:
      - "11434:11434"
    volumes:
      - ollama_dev_data:/root/.ollama

volumes:
  postgres_dev_data:
  ollama_dev_data:
```

Start development services:

```bash
# Start PostgreSQL and Ollama
docker-compose -f docker-compose.dev.yml up -d

# In another terminal, run the application
cargo run -p medassist-api
```

---

## Production Deployment

### Recommended Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Production Environment                     │
│                                                                  │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │   Client     │    │   Reverse    │    │   MedAssist     │  │
│  │  (Browser)   │───▶│   Proxy      │───▶│   API Server     │  │
│  └─────────────┘    │  (Nginx)     │    │                 │  │
│                     └────────┬────────┘    └────────┬────────┘  │
│                              │                         │              │
│                     ┌────────▼────────┐            ┌─────▼─────┐  │
│                     │   PostgreSQL     │            │  Ollama    │  │
│                     │   (Primary)      │            │  (Optional)│  │
│                     └─────────────────┘            └─────────────┘  │
│                                                                  │
│                     ┌─────────────────────────────────────────────┐│
│                     │              Backup Storage                 ││
│                     │  (Encrypted, Offsite)                         ││
│                     └─────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

### Reverse Proxy Configuration (Nginx)

Create `/etc/nginx/sites-available/medassist`:

```nginx
upstream medassist_api {
    server 127.0.0.1:3000;
    keepalive 32;
}

server {
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name medassist.yourdomain.com;

    # SSL Configuration
    ssl_certificate /etc/letsencrypt/live/medassist.yourdomain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/medassist.yourdomain.com/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256;
    ssl_prefer_server_ciphers on;
    ssl_session_cache shared:SSL:10m;
    ssl_session_timeout 10m;

    # Security Headers
    add_header X-Frame-Options "SAMEORIGIN" always;
    add_header X-Content-Type-Options "nosniff" always;
    add_header X-XSS-Protection "1; mode=block" always;
    add_header Strict-Transport-Security "max-age=31536000; includeSubDomains" always;
    add_header Content-Security-Policy "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:;" always;

    # Rate Limiting
    limit_req_zone $binary_remote_addr zone=medassist:10m rate=10r/s;
    limit_req zone=medassist burst=20 nodelay;

    # Proxy Configuration
    location / {
        proxy_pass http://medassist_api;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_read_timeout 300s;
        proxy_send_timeout 300s;
    }

    # Health checks (bypass auth)
    location /healthz {
        proxy_pass http://medassist_api/healthz;
        access_log off;
    }

    location /readyz {
        proxy_pass http://medassist_api/readyz;
        access_log off;
    }

    # Static files (future Web UI)
    location /static/ {
        alias /var/www/medassist/static/;
        expires 30d;
        access_log off;
    }
}

# HTTP to HTTPS redirect
server {
    listen 80;
    listen [::]:80;
    server_name medassist.yourdomain.com;
    return 301 https://$host$request_uri;
}
```

Enable the site:

```bash
sudo ln -s /etc/nginx/sites-available/medassist /etc/nginx/sites-enabled/
sudo nginx -t
sudo systemctl reload nginx
```

### Systemd Service Files

Create `/etc/systemd/system/medassist-api.service`:

```ini
[Unit]
Description=MedAssist OSS API Server
After=network.target postgresql.service

[Service]
Type=simple
User=medassist
Group=medassist
WorkingDirectory=/opt/medassist
EnvironmentFile=/opt/medassist/.env
ExecStart=/opt/medassist/medassist-api
Restart=always
RestartSec=5

# Security
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/medassist/data

[Install]
WantedBy=multi-user.target
```

Create `/etc/systemd/system/medassist-ollama.service` (optional):

```ini
[Unit]
Description=Ollama AI Server
After=network.target

[Service]
Type=simple
User=medassist
Group=medassist
WorkingDirectory=/opt/medassist
ExecStart=/usr/bin/ollama serve
Restart=always
RestartSec=5

# GPU support (if available)
Environment="OLLAMA_ORIGINS=*"

[Install]
WantedBy=multi-user.target
```

Enable and start services:

```bash
# Reload systemd
sudo systemctl daemon-reload

# Enable services
sudo systemctl enable medassist-api.service
sudo systemctl enable medassist-ollama.service

# Start services
sudo systemctl start medassist-api.service
sudo systemctl start medassist-ollama.service

# Check status
sudo systemctl status medassist-api.service
sudo systemctl status medassist-ollama.service
```

---

## Configuration Reference

### Environment Variables

| Variable | Description | Default | Required |
|----------|-------------|---------|----------|
| `DATABASE_URL` | PostgreSQL connection URL | - | Yes |
| `RUST_LOG` | Logging level | `info` | No |
| `SECRET_KEY` | Session signing key (32+ bytes) | - | Yes |
| `PORT` | API server port | `3000` | No |
| `HOST` | API server host | `0.0.0.0` | No |
| `MAX_CONNECTIONS` | DB connection pool size | `10` | No |
| `OLLAMA_URL` | Ollama API URL | `http://localhost:11434` | No |
| `BLOB_STORE_PATH` | Filesystem blob store path | `./data/blob-store` | No |

### PostgreSQL Connection URL Format

```
postgres://username:password@host:port/database
```

Examples:
- Local: `postgres://medassist:password@localhost:5432/medassist`
- Remote: `postgres://medassist:password@192.168.1.100:5432/medassist`
- With SSL: `postgres://medassist:password@localhost:5432/medassist?sslmode=require`

### Logging Levels

| Level | Description | Use Case |
|-------|-------------|----------|
| `error` | Only errors | Production (minimal logging) |
| `warn` | Errors and warnings | Production |
| `info` | Informational messages | Production, Development |
| `debug` | Debug information | Development, Troubleshooting |
| `trace` | Very verbose | Deep debugging |

---

## Database Management

### Migrations

MedAssist OSS uses SQLx migrations for database schema management.

```bash
# List migrations
./target/release/medassist-cli migrate --help

# Apply pending migrations
./target/release/medassist-cli migrate

# Create database and apply migrations
./target/release/medassist-cli setup
```

### Backups

#### PostgreSQL Backup

```bash
# Create backup
sudo -u postgres pg_dump -Fc -d medassist -f /backup/medassist-$(date +%Y%m%d).dump

# Restore backup
sudo -u postgres pg_restore -d medassist -c /backup/medassist-20241016.dump

# Automated backups (cron)
0 2 * * * sudo -u postgres pg_dump -Fc -d medassist -f /backup/medassist-$(date +\%Y\%m\%d).dump
```

#### Blob Store Backup

```bash
# Backup blob store
rsync -av /opt/medassist/data/blob-store/ /backup/blob-store/

# Automated backups (cron)
0 3 * * * rsync -av /opt/medassist/data/blob-store/ /backup/blob-store/
```

### Monitoring

#### PostgreSQL Monitoring

```bash
# Check database size
sudo -u postgres psql -c "SELECT pg_size_pretty(pg_database_size('medassist'));"

# Check connection count
sudo -u postgres psql -c "SELECT count(*) FROM pg_stat_activity WHERE datname = 'medassist';"

# Check table sizes
sudo -u postgres psql -d medassist -c "\dt+"
```

#### Application Monitoring

```bash
# Check API health
curl http://localhost:3000/healthz

# Check readiness (DB connection)
curl http://localhost:3000/readyz

# Check logs
journalctl -u medassist-api.service -f

# Check resource usage
top -u medassist
```

---

## Security Hardening

### 1. PostgreSQL Security

```bash
# Create dedicated user with limited privileges
sudo -u postgres psql -c "
    CREATE ROLE medassist_app WITH LOGIN PASSWORD 'secure_password';
    GRANT CONNECT ON DATABASE medassist TO medassist_app;
    GRANT USAGE ON SCHEMA public TO medassist_app;
    GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO medassist_app;
    GRANT USAGE ON ALL SEQUENCES IN SCHEMA public TO medassist_app;
"

# Enable SSL
# Edit postgresql.conf
ssl = on
ssl_cert_file = '/etc/ssl/certs/server.crt'
ssl_key_file = '/etc/ssl/private/server.key'

# Restrict access
# Edit pg_hba.conf
hostssl medassist medassist_app 127.0.0.1/32 md5
```

### 2. System Security

```bash
# Create dedicated user
sudo useradd -r -s /bin/false medassist

# Set proper permissions
sudo chown -R medassist:medassist /opt/medassist
sudo chmod -R 750 /opt/medassist

# Enable firewall
sudo ufw allow 22/tcp       # SSH
sudo ufw allow 80/tcp       # HTTP
sudo ufw allow 443/tcp      # HTTPS
sudo ufw enable

# Enable automatic security updates
sudo apt install -y unattended-upgrades
sudo dpkg-reconfigure -plow unattended-upgrades
```

### 3. Application Security

```bash
# Generate strong secret key
openssl rand -base64 64

# Set secure permissions on .env file
chmod 600 /opt/medassist/.env

# Use HTTPS only
# Configure reverse proxy with TLS
```

---

## Troubleshooting

### Common Issues

#### 1. Database Connection Failed

**Symptoms:**
- `Connection refused` error
- `readyz` endpoint fails

**Solutions:**
- Verify PostgreSQL is running: `sudo systemctl status postgresql`
- Check connection URL in `.env` file
- Test connection manually: `psql -U medassist -d medassist -h localhost`
- Check firewall: `sudo ufw status`

#### 2. Migration Failed

**Symptoms:**
- Migration errors during setup
- Database schema mismatch

**Solutions:**
- Check migration files exist in `migrations/` directory
- Verify database user has sufficient privileges
- Check PostgreSQL logs: `sudo journalctl -u postgresql -f`
- Manually apply migrations if needed

#### 3. Build Failed

**Symptoms:**
- Cargo build errors
- Dependency resolution failures

**Solutions:**
- Update Rust: `rustup update`
- Clean build: `cargo clean`
- Update dependencies: `cargo update`
- Check disk space: `df -h`
- Check memory: `free -h`

#### 4. Permission Denied

**Symptoms:**
- File access errors
- Blob store write failures

**Solutions:**
- Check file permissions: `ls -la /opt/medassist/data`
- Set correct ownership: `sudo chown -R medassist:medassist /opt/medassist`
- Create directory if missing: `mkdir -p /opt/medassist/data/blob-store`

#### 5. Out of Memory

**Symptoms:**
- Application crashes with OOM error
- Slow performance

**Solutions:**
- Increase system memory or swap
- Reduce `MAX_CONNECTIONS` in `.env`
- Optimize PostgreSQL memory settings
- Use smaller AI models

### Debug Mode

Enable debug logging for troubleshooting:

```bash
# Set RUST_LOG to debug
RUST_LOG=debug ./target/release/medassist-api

# Or update .env file
RUST_LOG=debug
```

### Log Analysis

```bash
# View application logs
journalctl -u medassist-api.service -f

# View PostgreSQL logs
sudo journalctl -u postgresql -f

# View system logs
sudo journalctl -f
```

---

## Upgrading

### Upgrade Process

```bash
# Stop application
sudo systemctl stop medassist-api.service

# Backup database
sudo -u postgres pg_dump -Fc -d medassist -f /backup/medassist-$(date +%Y%m%d).dump

# Backup blob store
rsync -av /opt/medassist/data/blob-store/ /backup/blob-store/

# Pull latest code
cd /opt/medassist
git pull origin main

# Rebuild
cargo build --release --workspace

# Run migrations (if any)
./target/release/medassist-cli migrate

# Restart application
sudo systemctl start medassist-api.service

# Verify
curl http://localhost:3000/healthz
```

### Migration Notes

Check the `CHANGELOG.md` file for version-specific upgrade instructions and breaking changes.

---

## Performance Tuning

### PostgreSQL Tuning

Adjust `postgresql.conf` based on available memory:

```ini
# For 8GB RAM system
shared_buffers = 2GB
effective_cache_size = 6GB
maintenance_work_mem = 512MB
work_mem = 16MB
max_connections = 100
```

### Application Tuning

Adjust `.env` file:

```ini
# For high-traffic systems
MAX_CONNECTIONS=20
PORT=3000
HOST=0.0.0.0
RUST_LOG=warn
```

### Ollama Tuning

For GPU systems:

```ini
# In ollama service configuration
Environment="OLLAMA_MAX_LOADED_MODELS=2"
Environment="OLLAMA_KEEP_ALIVE=24h"
```

---

## Monitoring and Alerting

### Prometheus + Grafana (Future)

Configure Prometheus to scrape metrics from MedAssist OSS (future feature).

### Health Check Endpoints

| Endpoint | Description | Expected Response |
|----------|-------------|-------------------|
| `GET /healthz` | Liveness probe | `200 OK` |
| `GET /readyz` | Readiness probe (with DB check) | `200 OK` |

### Alert Rules

Example alert rules for Prometheus:

```yaml
- alert: MedAssistDown
  expr: up{job="medassist-api"} == 0
  for: 5m
  labels:
    severity: critical
  annotations:
    summary: "MedAssist API is down"
    description: "MedAssist API has been down for more than 5 minutes"

- alert: HighErrorRate
  expr: rate(http_requests_total{status=~"5.."}[5m]) / rate(http_requests_total[5m]) > 0.1
  for: 5m
  labels:
    severity: warning
  annotations:
    summary: "High error rate on MedAssist API"
    description: "Error rate is {{ $value }}%"
```

---

## Disaster Recovery

### Backup Strategy

| Data | Frequency | Retention | Storage |
|------|-----------|-----------|---------|
| Database | Daily | 30 days | Encrypted, offsite |
| Blob Store | Daily | 30 days | Encrypted, offsite |
| Configuration | On change | Forever | Version controlled |
| Logs | Daily | 90 days | Encrypted, offsite |

### Recovery Procedure

1. **Restore Database:**
   ```bash
   # Create new database
   sudo -u postgres createdb medassist
   
   # Restore from backup
   sudo -u postgres pg_restore -d medassist -c /backup/medassist-20241016.dump
   ```

2. **Restore Blob Store:**
   ```bash
   # Restore from backup
   rsync -av /backup/blob-store/ /opt/medassist/data/blob-store/
   ```

3. **Restore Configuration:**
   ```bash
   # Restore from version control
   cd /opt/medassist
   git checkout main
   git pull origin main
   ```

4. **Restart Application:**
   ```bash
   sudo systemctl restart medassist-api.service
   ```

---

## Compliance Checklist

### Pre-Deployment Checklist

- [ ] PostgreSQL installed and configured
- [ ] Rust toolchain installed
- [ ] Environment variables configured
- [ ] Database created and migrations applied
- [ ] Administrator user created
- [ ] TLS certificates configured
- [ ] Reverse proxy configured
- [ ] Firewall configured
- [ ] Backup procedures in place
- [ ] Monitoring configured
- [ ] Security hardening applied

### Post-Deployment Checklist

- [ ] Health checks passing
- [ ] All services running
- [ ] Logs being collected
- [ ] Backups verified
- [ ] Monitoring alerts configured
- [ ] Incident response procedures documented
- [ ] User training completed
- [ ] Privacy policy updated
- [ ] Security assessment completed

---

## Support and Community

### Getting Help

1. **Documentation:** Read this guide and other documentation in the `docs/` directory
2. **Issues:** Check for existing issues on [GitHub Issues](https://github.com/Lucasovski/MEDASSIST-OSS/issues)
3. **Discussions:** Join the discussion on [GitHub Discussions](https://github.com/Lucasovski/MEDASSIST-OSS/discussions)
4. **Community:** Join the MedAssist OSS community (future)

### Reporting Issues

When reporting issues, include:
- Version of MedAssist OSS
- Operating system and version
- PostgreSQL version
- Rust version
- Steps to reproduce
- Error messages and logs
- Configuration (sanitized)

### Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for information on contributing to MedAssist OSS.

---

## Appendix A: CLI Commands Reference

| Command | Description | Options |
|---------|-------------|---------|
| `medassist migrate` | Apply database migrations | - |
| `medassist setup` | Create database and apply migrations | - |
| `medassist create-admin` | Create administrator user | `--org-name`, `--email`, `--password`, `--display-name` |
| `medassist reset-password` | Reset user password | `--email`, `--password` |
| `medassist check-db` | Verify database connection | - |

### Examples

```bash
# Apply migrations
./target/release/medassist-cli migrate

# Setup database
./target/release/medassist-cli setup

# Create admin
./target/release/medassist-cli create-admin \
    --org-name "My Organization" \
    --email admin@example.com \
    --password "secure_password" \
    --display-name "Admin User"

# Reset password
./target/release/medassist-cli reset-password \
    --email user@example.com \
    --password "new_password"

# Check database
./target/release/medassist-cli check-db
```

---

## Appendix B: API Endpoints Reference

### Health and Status

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/healthz` | Liveness probe |
| GET | `/readyz` | Readiness probe |

### Authentication

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/login` | User login |
| POST | `/logout` | User logout |
| GET | `/sessions/me` | Get current session |

### Patients

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/patients` | List patients |
| POST | `/patients` | Create patient |
| GET | `/patients/:id` | Get patient |
| PATCH | `/patients/:id` | Update patient |
| DELETE | `/patients/:id` | Delete patient |

### Encounters

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/patients/:id/encounters` | List encounters |
| POST | `/patients/:id/encounters` | Create encounter |
| GET | `/encounters/:id` | Get encounter |

### Notes

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/patients/:id/notes` | List notes |
| POST | `/patients/:id/notes` | Create note |
| GET | `/notes/:id` | Get note |
| POST | `/notes/:id/review` | Review note |
| POST | `/notes/:id/finalize` | Finalize note |
| POST | `/notes/:id/amend` | Amend note |

### Documents

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/documents` | List documents |
| POST | `/documents/upload` | Upload document |
| GET | `/documents/:id/download` | Download document |

### Transcriptions

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/transcriptions/consent` | Request consent |
| POST | `/transcriptions/start` | Start transcription |
| POST | `/transcriptions/:id/cancel` | Cancel transcription |
| POST | `/transcriptions/:id/review` | Review transcription |

### AI

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/ai/draft` | Generate AI draft |

### System

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/system/capabilities` | Get capabilities |
| GET | `/system/audit` | Get audit log |
| GET | `/system/templates` | List templates |
| GET | `/system/codes` | Search codes |

---

## Document History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2026-10-16 | MedAssist OSS Contributors | Initial deployment guide |
