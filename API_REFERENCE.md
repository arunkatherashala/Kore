# KORE v1.3.3 REST API Reference

**Last Updated:** June 3, 2026  
**Status:** Production Ready  
**Version:** v1.0  
**Base URL:** `http://localhost:8000/api`

---

## 📋 Table of Contents

1. [Authentication](#authentication)
2. [Core Endpoints](#core-endpoints)
3. [File Operations](#file-operations)
4. [Query Operations](#query-operations)
5. [Metadata Operations](#metadata-operations)
6. [Admin Operations](#admin-operations)
7. [Error Handling](#error-handling)

---

## Authentication

**Method:** Bearer Token  
**Header:** `Authorization: Bearer YOUR_TOKEN`

```bash
# Example
curl -H "Authorization: Bearer abc123xyz" http://localhost:8000/api/files
```

---

## Core Endpoints

### GET /api/version
Returns KORE version and build info

**Response (200 OK):**
```json
{
  "version": "1.3.3",
  "build_date": "2026-06-03",
  "git_hash": "493aacf",
  "rust_version": "1.75.0",
  "commit_count": 42
}
```

### GET /api/health
Health check endpoint

**Response (200 OK):**
```json
{
  "status": "healthy",
  "uptime_seconds": 3600,
  "memory_usage_mb": 256,
  "timestamp": "2026-06-03T10:30:45Z"
}
```

### GET /api/stats
System statistics and metrics

**Response (200 OK):**
```json
{
  "queries_total": 1524,
  "queries_failed": 3,
  "files_processed": 42,
  "total_bytes_compressed": 5368709120,
  "total_bytes_uncompressed": 24159191040,
  "compression_ratio": 4.49,
  "avg_query_latency_ms": 2.34
}
```

---

## File Operations

### POST /api/files/upload
Upload and process new file

**Request:**
```bash
curl -X POST \
  -F "file=@data.csv" \
  -H "Authorization: Bearer token" \
  http://localhost:8000/api/files/upload
```

**Parameters:**
- `file` (required): CSV file to process
- `compression` (optional): auto, huffman, lz77, for, dictionary, rle, delta
- `encryption` (optional): boolean (default: false)

**Response (201 Created):**
```json
{
  "file_id": "f-5a2b8c9d",
  "filename": "data.kore",
  "size_bytes": 524288,
  "row_count": 10000,
  "column_count": 5,
  "compression_ratio": 4.2,
  "codecs_used": {
    "column_1": "huffman",
    "column_2": "delta_int",
    "column_3": "dictionary",
    "column_4": "huffman",
    "column_5": "for"
  },
  "created_at": "2026-06-03T10:30:45Z"
}
```

### GET /api/files
List all files

**Query Parameters:**
- `limit` (optional, default: 50)
- `offset` (optional, default: 0)
- `sort` (optional): name, size, created, modified

**Response (200 OK):**
```json
{
  "total": 42,
  "files": [
    {
      "file_id": "f-5a2b8c9d",
      "filename": "data.kore",
      "size_bytes": 524288,
      "row_count": 10000,
      "created_at": "2026-06-03T10:30:45Z",
      "modified_at": "2026-06-03T10:35:12Z"
    }
  ]
}
```

### GET /api/files/{file_id}
Get file metadata

**Response (200 OK):**
```json
{
  "file_id": "f-5a2b8c9d",
  "filename": "data.kore",
  "size_bytes": 524288,
  "row_count": 10000,
  "columns": [
    {
      "name": "id",
      "type": "integer",
      "codec": "delta_int"
    },
    {
      "name": "value",
      "type": "float",
      "codec": "huffman"
    }
  ],
  "created_at": "2026-06-03T10:30:45Z"
}
```

### DELETE /api/files/{file_id}
Delete file

**Response (204 No Content)**

---

## Query Operations

### POST /api/query
Execute query against file

**Request:**
```json
{
  "file_id": "f-5a2b8c9d",
  "columns": ["id", "name", "value"],
  "limit": 100,
  "offset": 0,
  "where": {
    "value": {">": 100, "<": 500}
  },
  "format": "json"
}
```

**Response (200 OK):**
```json
{
  "query_id": "q-7f3a9b2c",
  "rows": 100,
  "data": [
    {"id": 1, "name": "Alice", "value": 250},
    {"id": 2, "name": "Bob", "value": 320}
  ],
  "query_time_ms": 2.34,
  "execution_plan": {
    "strategy": "column_scan",
    "columns_accessed": 3,
    "compression_codecs": ["delta_int", "huffman", "huffman"]
  }
}
```

### GET /api/query/{query_id}
Get query results

**Response:** Same as POST /api/query

---

## Metadata Operations

### POST /api/metadata/analyze
Analyze file and recommend codecs

**Request:**
```json
{
  "file_id": "f-5a2b8c9d",
  "sample_size": 1000
}
```

**Response (200 OK):**
```json
{
  "file_id": "f-5a2b8c9d",
  "analysis": {
    "column_1": {
      "pattern": "monotonic",
      "recommended_codec": "delta_int",
      "confidence": 0.95,
      "estimated_ratio": 5.2
    },
    "column_2": {
      "pattern": "random",
      "recommended_codec": "huffman",
      "confidence": 0.85,
      "estimated_ratio": 2.1
    }
  }
}
```

### GET /api/metadata/schema/{file_id}
Get file schema

**Response (200 OK):**
```json
{
  "columns": [
    {"name": "id", "type": "int64", "nullable": false},
    {"name": "name", "type": "string", "nullable": true},
    {"name": "value", "type": "float64", "nullable": false}
  ]
}
```

---

## Admin Operations

### POST /api/admin/backup
Create backup

**Request:**
```json
{
  "file_ids": ["f-5a2b8c9d", "f-3c7e9a1b"],
  "destination": "/backups/2026-06-03"
}
```

**Response (202 Accepted):**
```json
{
  "backup_id": "b-2f8a5c9d",
  "status": "in_progress",
  "files_count": 2,
  "estimated_completion": "2026-06-03T10:45:00Z"
}
```

### GET /api/admin/backup/{backup_id}
Check backup status

**Response:**
```json
{
  "backup_id": "b-2f8a5c9d",
  "status": "completed",
  "files_processed": 2,
  "bytes_backed_up": 1048576,
  "completion_time": "2026-06-03T10:42:15Z"
}
```

### POST /api/admin/restore
Restore from backup

**Request:**
```json
{
  "backup_id": "b-2f8a5c9d"
}
```

**Response (202 Accepted):**
```json
{
  "restore_id": "r-9c3f2b8a",
  "status": "in_progress"
}
```

---

## Error Handling

### Error Response Format

```json
{
  "error": {
    "code": "FILE_NOT_FOUND",
    "message": "File f-5a2b8c9d not found",
    "timestamp": "2026-06-03T10:30:45Z",
    "request_id": "req-abc123xyz"
  }
}
```

### HTTP Status Codes

| Code | Meaning |
|------|---------|
| 200 | OK |
| 201 | Created |
| 202 | Accepted (async) |
| 204 | No Content |
| 400 | Bad Request |
| 401 | Unauthorized |
| 403 | Forbidden |
| 404 | Not Found |
| 409 | Conflict |
| 500 | Internal Server Error |

### Common Errors

**400 Bad Request:**
```json
{"error": {"code": "INVALID_PARAMETERS", "message": "Missing required field: columns"}}
```

**401 Unauthorized:**
```json
{"error": {"code": "UNAUTHORIZED", "message": "Invalid or missing authorization token"}}
```

**404 Not Found:**
```json
{"error": {"code": "FILE_NOT_FOUND", "message": "File f-xxx not found"}}
```

---

## Rate Limiting

**Limits:**
- 1000 requests per minute per token
- 100 concurrent queries
- 10 GB/day data transfer

**Headers:**
```
X-RateLimit-Limit: 1000
X-RateLimit-Remaining: 987
X-RateLimit-Reset: 2026-06-03T10:31:45Z
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | Initial REST API documentation for KORE v1.3.3 |

---

**Status: ✅ Production Ready**
