"""
AWS Athena Connector for KORE format via Lambda.
Deploy as AWS Lambda + register with Athena Federated Query.

Usage:
    -- In Athena SQL:
    SELECT * FROM lambda:kore_connector.default.sales
    WHERE region = 'US'
"""
import json
import os
import sys
import boto3

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "kore-python"))


def lambda_handler(event, context):
    """Athena Federated Query Lambda handler."""
    request_type = event.get("@type", "")

    if "PingRequest" in request_type:
        return handle_ping(event)
    elif "ListSchemasRequest" in request_type:
        return handle_list_schemas(event)
    elif "ListTablesRequest" in request_type:
        return handle_list_tables(event)
    elif "GetTableRequest" in request_type:
        return handle_get_table(event)
    elif "GetTableLayoutRequest" in request_type:
        return handle_get_table_layout(event)
    elif "GetSplitsRequest" in request_type:
        return handle_get_splits(event)
    elif "ReadRecordsRequest" in request_type:
        return handle_read_records(event)
    else:
        raise ValueError(f"Unknown request type: {request_type}")


def handle_ping(event):
    return {
        "@type": "PingResponse",
        "catalogName": event.get("catalogName", "kore"),
        "queryId": event.get("queryId", ""),
        "sourceType": "kore",
        "capabilities": 1,
    }


def handle_list_schemas(event):
    return {
        "@type": "ListSchemasResponse",
        "catalogName": event.get("catalogName"),
        "schemas": ["default"],
        "requestType": "LIST_SCHEMAS",
    }


def handle_list_tables(event):
    bucket = os.environ.get("KORE_S3_BUCKET", "kore-data")
    prefix = os.environ.get("KORE_S3_PREFIX", "")
    s3 = boto3.client("s3")

    tables = []
    resp = s3.list_objects_v2(Bucket=bucket, Prefix=prefix, Delimiter="/")
    for obj in resp.get("Contents", []):
        key = obj["Key"]
        if key.endswith(".kore"):
            name = os.path.basename(key).replace(".kore", "")
            tables.append({"schemaName": "default", "tableName": name})

    return {
        "@type": "ListTablesResponse",
        "catalogName": event.get("catalogName"),
        "tables": tables,
        "requestType": "LIST_TABLES",
    }


def handle_get_table(event):
    import kore_fileformat as kf

    table_name = event["tableName"]["tableName"]
    bucket = os.environ.get("KORE_S3_BUCKET", "kore-data")
    key = f"{os.environ.get('KORE_S3_PREFIX', '')}{table_name}.kore"

    s3 = boto3.client("s3")
    obj = s3.get_object(Bucket=bucket, Key=key)
    data = obj["Body"].read()
    block = kf.read_file_from_bytes(data) if hasattr(kf, "read_file_from_bytes") else _read_block(data)

    type_map = {"I64": "BIGINT", "F64": "DOUBLE", "BOOL": "BOOLEAN", "STR": "VARCHAR", "STR_DICT": "VARCHAR"}
    columns = []
    for col in block.columns:
        dt = col.dtype.name if hasattr(col.dtype, "name") else str(col.dtype)
        columns.append({"name": col.name, "type": type_map.get(dt, "VARCHAR")})

    return {
        "@type": "GetTableResponse",
        "catalogName": event.get("catalogName"),
        "tableName": event["tableName"],
        "schema": {"columns": columns},
    }


def handle_get_table_layout(event):
    return {
        "@type": "GetTableLayoutResponse",
        "catalogName": event.get("catalogName"),
        "tableName": event.get("tableName"),
        "partitions": {"columns": [], "rows": [{}]},
    }


def handle_get_splits(event):
    table_name = event["tableName"]["tableName"]
    return {
        "@type": "GetSplitsResponse",
        "catalogName": event.get("catalogName"),
        "splits": [{"properties": {"table": table_name}}],
    }


def handle_read_records(event):
    import kore_fileformat as kf

    table_name = event["split"]["properties"]["table"]
    bucket = os.environ.get("KORE_S3_BUCKET", "kore-data")
    key = f"{os.environ.get('KORE_S3_PREFIX', '')}{table_name}.kore"

    s3 = boto3.client("s3")
    obj = s3.get_object(Bucket=bucket, Key=key)
    data = obj["Body"].read()
    block = kf.read_file_from_bytes(data) if hasattr(kf, "read_file_from_bytes") else _read_block(data)

    records = []
    for row in range(block.num_rows):
        record = {}
        for col in block.columns:
            record[col.name] = col.data[row] if row < len(col.data) else None
        records.append(record)

    return {
        "@type": "ReadRecordsResponse",
        "catalogName": event.get("catalogName"),
        "records": records,
        "recordCount": len(records),
    }


def _read_block(data):
    """Fallback: write to temp file and read."""
    import tempfile
    import kore_fileformat as kf

    with tempfile.NamedTemporaryFile(suffix=".kore", delete=False) as f:
        f.write(data)
        tmp = f.name
    try:
        return kf.read_file(tmp)
    finally:
        os.unlink(tmp)
