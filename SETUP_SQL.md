# SQL Setup & Integration Guide for KORE v1.3.3

**Last Updated:** June 3, 2026  
**Status:** Production Ready  
**Version:** v1.0

---

## 📋 Table of Contents

1. [Prerequisites](#prerequisites)
2. [Installation](#installation)
3. [Verification](#verification)
4. [KORE Integration](#kore-integration)
5. [Common Tasks](#common-tasks)
6. [Troubleshooting](#troubleshooting)

---

## Prerequisites

| Requirement | Minimum | Recommended | Notes |
|-------------|---------|-------------|-------|
| Database Engine | PostgreSQL 12, MySQL 8 | PostgreSQL 15+ | For testing/integration |
| SQL Client | Any | DBeaver, VS Code | Query execution tool |
| OS Support | Windows 10+ | Windows 10, 11 | Also supports Linux/macOS |
| RAM | 1 GB | 2 GB | For database operations |
| Disk Space | 500 MB | 2 GB | Database storage |

---

## Installation

### Option 1: PostgreSQL (Recommended for KORE)

**Download:**
```powershell
# From: https://www.postgresql.org/download/windows/

# Or use Windows Package Manager
winget install PostgreSQL.PostgreSQL

# Or use Chocolatey
choco install postgresql
```

**Installation Steps:**
1. Run installer
2. Choose installation directory
3. Set admin password (remember this!)
4. Accept port 5432 (default)
5. Select English locale
6. Install

**Verify PostgreSQL:**
```powershell
# Check version
psql --version

# Connect to database
psql -U postgres
```

### Option 2: MySQL

**Download:**
```powershell
# From: https://dev.mysql.com/downloads/mysql/

# Or use package manager
winget install MySQL.Server

# Or use Chocolatey
choco install mysql
```

### Option 3: SQLite (Lightweight, File-based)

**No installation needed!**
SQLite is included in most systems.

```powershell
# Check if installed
sqlite3 --version

# Create database file
sqlite3 test.db

# Query database
sqlite3 test.db "SELECT 1;"
```

---

## Verification

### PostgreSQL Verification

```powershell
# Check version
psql --version

# Check service status
Get-Service postgresql-x64-* | Select-Object Status

# Connect and query
psql -U postgres -c "SELECT version();"

# List databases
psql -U postgres -l
```

### MySQL Verification

```powershell
# Check version
mysql --version

# Check service
Get-Service MySQL* | Select-Object Status

# Connect
mysql -u root -p
# Then at prompt: SELECT VERSION();
```

### SQLite Verification

```powershell
# Check version
sqlite3 --version

# Create test database
sqlite3 test.db "CREATE TABLE test (id INTEGER, name TEXT);"
sqlite3 test.db "SELECT * FROM test;"
```

---

## KORE Integration

### KORE Database Storage

KORE uses custom binary format (KORE v2) for data storage, but can integrate with SQL databases for:
- Metadata storage
- Query results export
- Data import/export
- Analytics
- Reporting

### Setup SQL Integration with KORE

**Step 1: Create KORE Metadata Database**

```powershell
# Create PostgreSQL database for KORE metadata
psql -U postgres -c "CREATE DATABASE kore_metadata;"

# Or MySQL
mysql -u root -p -e "CREATE DATABASE kore_metadata;"
```

**Step 2: Create Schema**

**PostgreSQL:**
```sql
-- Connect to kore_metadata database
\c kore_metadata

-- Create tables for KORE metadata
CREATE TABLE kore_files (
    id SERIAL PRIMARY KEY,
    filename VARCHAR(255) NOT NULL UNIQUE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    size_bytes BIGINT,
    version VARCHAR(10),
    status VARCHAR(50)
);

CREATE TABLE kore_columns (
    id SERIAL PRIMARY KEY,
    file_id INTEGER REFERENCES kore_files(id),
    column_name VARCHAR(255),
    data_type VARCHAR(50),
    codec VARCHAR(50),
    compression_ratio DECIMAL(5,2),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE query_results (
    id SERIAL PRIMARY KEY,
    query_text TEXT,
    result_count INTEGER,
    execution_time_ms INTEGER,
    executed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

**MySQL:**
```sql
CREATE DATABASE IF NOT EXISTS kore_metadata;
USE kore_metadata;

CREATE TABLE kore_files (
    id INT AUTO_INCREMENT PRIMARY KEY,
    filename VARCHAR(255) NOT NULL UNIQUE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    size_bytes BIGINT,
    version VARCHAR(10),
    status VARCHAR(50)
);

CREATE TABLE kore_columns (
    id INT AUTO_INCREMENT PRIMARY KEY,
    file_id INT REFERENCES kore_files(id),
    column_name VARCHAR(255),
    data_type VARCHAR(50),
    codec VARCHAR(50),
    compression_ratio DECIMAL(5,2),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE query_results (
    id INT AUTO_INCREMENT PRIMARY KEY,
    query_text TEXT,
    result_count INT,
    execution_time_ms INT,
    executed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

**Step 3: Verify Schema**

```powershell
# PostgreSQL
psql -U postgres kore_metadata -c "\dt"

# MySQL
mysql -u root -p kore_metadata -e "SHOW TABLES;"

# SQLite
sqlite3 kore_metadata.db ".tables"
```

---

## Common Tasks

### Connecting to Databases

**PostgreSQL:**
```powershell
# Interactive connection
psql -U postgres -d kore_metadata

# Execute query
psql -U postgres -d kore_metadata -c "SELECT COUNT(*) FROM kore_files;"

# Execute from file
psql -U postgres -d kore_metadata -f queries.sql
```

**MySQL:**
```powershell
# Interactive connection
mysql -u root -p kore_metadata

# Execute query
mysql -u root -p kore_metadata -e "SELECT COUNT(*) FROM kore_files;"

# Execute from file
mysql -u root -p kore_metadata < queries.sql
```

**SQLite:**
```powershell
# Interactive connection
sqlite3 kore_metadata.db

# Execute query
sqlite3 kore_metadata.db "SELECT COUNT(*) FROM kore_files;"

# Execute from file
sqlite3 kore_metadata.db < queries.sql
```

### KORE Data Management Queries

**Insert KORE file metadata:**
```sql
INSERT INTO kore_files (filename, size_bytes, version, status)
VALUES ('data_2026_06_03.kore', 1024000, '1.3.3', 'active');
```

**Query KORE statistics:**
```sql
-- Total files stored
SELECT COUNT(*) as total_files FROM kore_files;

-- Storage by version
SELECT version, SUM(size_bytes) as total_size 
FROM kore_files 
GROUP BY version;

-- Compression effectiveness
SELECT column_name, AVG(compression_ratio) 
FROM kore_columns 
GROUP BY column_name;
```

**Monitor query performance:**
```sql
SELECT 
  query_text,
  AVG(execution_time_ms) as avg_time,
  MAX(execution_time_ms) as max_time,
  COUNT(*) as executions
FROM query_results
GROUP BY query_text
ORDER BY avg_time DESC;
```

### Backup and Restore

**PostgreSQL Backup:**
```powershell
# Full database backup
pg_dump -U postgres kore_metadata > backup.sql

# Restore
psql -U postgres kore_metadata < backup.sql

# Binary backup (faster for large databases)
pg_dump -U postgres -Fc kore_metadata > backup.dump
pg_restore -U postgres -d kore_metadata backup.dump
```

**MySQL Backup:**
```powershell
# Full database backup
mysqldump -u root -p kore_metadata > backup.sql

# Restore
mysql -u root -p kore_metadata < backup.sql

# With compression
mysqldump -u root -p kore_metadata | gzip > backup.sql.gz
gunzip < backup.sql.gz | mysql -u root -p kore_metadata
```

**SQLite Backup:**
```powershell
# Simple file copy
Copy-Item kore_metadata.db kore_metadata_backup.db

# Using SQL dump
sqlite3 kore_metadata.db ".dump" | Out-File backup.sql

# Restore from dump
sqlite3 kore_metadata.db < backup.sql
```

---

## Troubleshooting

### Issue 1: "Connection refused"

**Solution:**
```powershell
# Check if database service is running
Get-Service PostgreSQL* | Select-Object Status
Get-Service MySQL* | Select-Object Status

# Start service
Start-Service PostgreSQL-x64-15

# Or for MySQL
Start-Service MySQL*

# Verify connection
psql -U postgres -c "SELECT 1;"
```

### Issue 2: "Authentication failed"

**Solution:**
```powershell
# PostgreSQL - reset admin password
psql -U postgres

-- In psql:
ALTER USER postgres WITH PASSWORD 'new_password';
\q

# MySQL - reset root password (complex process)
# See: https://dev.mysql.com/doc/mysql-installation-excerpt/8.0/en/resetting-permissions.html
```

### Issue 3: "Port already in use"

**Solution:**
```powershell
# Check what's using the port
Get-NetTCPConnection -LocalPort 5432

# Kill the process (PostgreSQL example)
Stop-Process -Id <PID> -Force

# Start service again
Start-Service PostgreSQL-x64-15
```

### Issue 4: "Disk space full"

**Solution:**
```powershell
# Clean up old backup files
Remove-Item *.sql.gz -OlderThan (Get-Date).AddDays(-30)

# Or restore database from backup
psql -U postgres kore_metadata < recent_backup.sql
```

### Issue 5: "Slow queries"

**Solution:**
```sql
-- PostgreSQL: Enable query logging
ALTER SYSTEM SET log_min_duration_statement = 1000; -- Log queries > 1 second
SELECT pg_reload_conf();

-- MySQL: Enable slow query log
SET GLOBAL slow_query_log = 'ON';
SET GLOBAL long_query_time = 1;

-- Then analyze
SELECT * FROM mysql.slow_log;
```

---

## Best Practices

✅ **DO:**
- Always backup before major changes
- Use prepared statements for security
- Index frequently queried columns
- Monitor query performance
- Document database schema
- Use transactions for data consistency
- Set up automated backups
- Use version control for schema

❌ **DON'T:**
- Store passwords in code
- Use `SELECT *` in production
- Ignore backup warnings
- Mix data types inconsistently
- Allow NULL without reason
- Skip database security setup
- Commit database dumps to git
- Ignore error messages

---

## SQL for KORE Development

### Example: Tracking KORE Deployments

```sql
-- Create deployment table
CREATE TABLE kore_deployments (
    id SERIAL PRIMARY KEY,
    version VARCHAR(10),
    deployed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    environment VARCHAR(50),
    status VARCHAR(50),
    notes TEXT
);

-- Track deployments
INSERT INTO kore_deployments (version, environment, status, notes)
VALUES ('1.3.3', 'production', 'success', 'All tests passing');

-- Query deployment history
SELECT version, environment, status, deployed_at
FROM kore_deployments
ORDER BY deployed_at DESC;
```

### Example: Performance Monitoring

```sql
-- Create performance table
CREATE TABLE kore_performance (
    id SERIAL PRIMARY KEY,
    metric_name VARCHAR(100),
    metric_value DECIMAL(10,2),
    unit VARCHAR(50),
    measured_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Insert test result
INSERT INTO kore_performance (metric_name, metric_value, unit)
VALUES ('test_pass_rate', 100.0, 'percent');

-- Monitor trends
SELECT metric_name, AVG(metric_value), MIN(measured_at), MAX(measured_at)
FROM kore_performance
WHERE measured_at > CURRENT_TIMESTAMP - INTERVAL '7 days'
GROUP BY metric_name;
```

---

## Quick Reference

```powershell
# PostgreSQL commands
psql --version                          # Check version
psql -U postgres                        # Connect
psql -U postgres -c "command"           # Execute command
psql -U postgres -f file.sql            # Execute file
pg_dump -U postgres db_name > file.sql  # Backup

# MySQL commands
mysql --version                         # Check version
mysql -u root -p                        # Connect
mysql -u root -p -e "command"           # Execute command
mysql -u root -p < file.sql             # Execute file
mysqldump -u root -p db_name > file.sql # Backup

# SQLite commands
sqlite3 --version                       # Check version
sqlite3 database.db                     # Connect
sqlite3 database.db < file.sql          # Execute file
sqlite3 database.db ".dump" > file.sql  # Backup
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | Initial SQL setup guide for KORE v1.3.3 |

---

**Status: ✅ Production Ready**

**Recommended for KORE:** PostgreSQL (best performance) or SQLite (simplest setup)
