"""
Phase 5: Cloud Storage Connectors for Kore

Enables reading/writing Kore files from AWS S3, Google Cloud Storage, and Azure Blob
"""

from typing import Optional, Dict, List, Any
import struct

# ============================================================================
# Phase 5A: AWS S3 Connector
# ============================================================================

try:
    import boto3
except ImportError:
    boto3 = None


class KoreS3Reader:
    """Read Kore files from AWS S3 with streaming support"""
    
    def __init__(self, bucket: str, region: str = "us-east-1"):
        if boto3 is None:
            raise RuntimeError("boto3 not installed. Install with: pip install boto3")
        self.bucket = bucket
        self.region = region
        self.s3_client = boto3.client('s3', region_name=region)
    
    def read(self, key: str) -> List[List[str]]:
        """Read entire Kore file from S3"""
        try:
            response = self.s3_client.get_object(Bucket=self.bucket, Key=key)
            data = response['Body'].read()
            return self._parse_kore_data(data)
        except Exception as e:
            raise RuntimeError(f"Failed to read {key} from S3: {e}")
    
    def read_stream(self, key: str, chunk_size: int = 1024*1024):
        """Stream Kore file from S3 (memory efficient for large files)"""
        try:
            response = self.s3_client.get_object(Bucket=self.bucket, Key=key)
            while True:
                chunk = response['Body'].read(chunk_size)
                if not chunk:
                    break
                yield chunk
        except Exception as e:
            raise RuntimeError(f"Failed to stream {key}: {e}")
    
    def get_metadata(self, key: str) -> Dict[str, Any]:
        """Get Kore file metadata without full read"""
        try:
            response = self.s3_client.get_object(Bucket=self.bucket, Key=key, Range='bytes=0-63')
            header = response['Body'].read(64)
            return self._parse_kore_header(header)
        except Exception as e:
            raise RuntimeError(f"Failed to get metadata: {e}")
    
    def _parse_kore_header(self, header: bytes) -> Dict[str, Any]:
        """Parse Kore file header (magic + version + metadata)"""
        if len(header) < 16:
            return {}
        
        try:
            magic = header[0:4].decode('ascii')
            version = header[4]
            num_cols = struct.unpack('<H', header[6:8])[0]
            num_rows = struct.unpack('<Q', header[8:16])[0]
            
            return {
                'magic': magic,
                'version': version,
                'columns': num_cols,
                'rows': num_rows
            }
        except Exception:
            return {}
    
    def _parse_kore_data(self, data: bytes) -> List[List[str]]:
        """Parse Kore binary format using KoreBinaryParser"""
        from kore_parser import KoreBinaryParser
        import io
        
        parser = KoreBinaryParser()
        stream = io.BytesIO(data)
        return parser.parse_stream(stream)


class KoreS3Writer:
    """Write Kore files to AWS S3 with multipart upload support"""
    
    def __init__(self, bucket: str, region: str = "us-east-1"):
        if boto3 is None:
            raise RuntimeError("boto3 not installed. Install with: pip install boto3")
        self.bucket = bucket
        self.region = region
        self.s3_client = boto3.client('s3', region_name=region)
    
    def write(self, data: bytes, key: str, metadata: Dict[str, str] = None) -> bool:
        """Write Kore file to S3"""
        try:
            self.s3_client.put_object(
                Bucket=self.bucket,
                Key=key,
                Body=data,
                Metadata=metadata or {}
            )
            return True
        except Exception as e:
            raise RuntimeError(f"Failed to write {key}: {e}")
    
    def write_stream(self, key: str, chunks, metadata: Dict[str, str] = None) -> bool:
        """Stream write to S3 using multipart upload (for large files)"""
        try:
            mpu = self.s3_client.create_multipart_upload(
                Bucket=self.bucket, Key=key, Metadata=metadata or {}
            )
            upload_id = mpu['UploadId']
            parts = []
            
            for part_num, chunk in enumerate(chunks, 1):
                response = self.s3_client.upload_part(
                    Bucket=self.bucket, Key=key, PartNumber=part_num,
                    UploadId=upload_id, Body=chunk
                )
                parts.append({'PartNumber': part_num, 'ETag': response['ETag']})
            
            self.s3_client.complete_multipart_upload(
                Bucket=self.bucket, Key=key, UploadId=upload_id,
                MultipartUpload={'Parts': parts}
            )
            return True
        except Exception as e:
            raise RuntimeError(f"Failed to stream write: {e}")


# ============================================================================
# Phase 5B: Google Cloud Storage
# ============================================================================

try:
    from google.cloud import storage as gcs_storage
except ImportError:
    gcs_storage = None


class KoreGCSReader:
    """Read Kore files from Google Cloud Storage"""
    
    def __init__(self, project: str, bucket: str):
        if gcs_storage is None:
            raise RuntimeError("google-cloud-storage not installed. Install with: pip install google-cloud-storage")
        self.project = project
        self.bucket = bucket
        self.client = gcs_storage.Client(project=project)
    
    def read(self, blob_name: str) -> List[List[str]]:
        """Read Kore file from GCS"""
        try:
            bucket = self.client.bucket(self.bucket)
            blob = bucket.blob(blob_name)
            data = blob.download_as_bytes()
            return self._parse_kore_data(data)
        except Exception as e:
            raise RuntimeError(f"Failed to read {blob_name}: {e}")
    
    def get_metadata(self, blob_name: str) -> Dict[str, Any]:
        """Get GCS blob metadata"""
        try:
            bucket = self.client.bucket(self.bucket)
            blob = bucket.blob(blob_name)
            blob.reload()
            
            return {
                'size': blob.size,
                'created': blob.time_created.isoformat(),
                'updated': blob.updated.isoformat(),
                'metadata': blob.metadata or {}
            }
        except Exception as e:
            raise RuntimeError(f"Failed to get metadata: {e}")
    
    def _parse_kore_data(self, data: bytes) -> List[List[str]]:
        """Parse Kore binary format using KoreBinaryParser"""
        from kore_parser import KoreBinaryParser
        import io
        
        parser = KoreBinaryParser()
        stream = io.BytesIO(data)
        return parser.parse_stream(stream)


class KoreGCSWriter:
    """Write Kore files to Google Cloud Storage"""
    
    def __init__(self, project: str, bucket: str):
        if gcs_storage is None:
            raise RuntimeError("google-cloud-storage not installed")
        self.project = project
        self.bucket = bucket
        self.client = gcs_storage.Client(project=project)
    
    def write(self, data: bytes, blob_name: str, metadata: Dict[str, str] = None) -> bool:
        """Write Kore file to GCS"""
        try:
            bucket = self.client.bucket(self.bucket)
            blob = bucket.blob(blob_name)
            blob.metadata = metadata or {}
            blob.upload_from_string(data)
            return True
        except Exception as e:
            raise RuntimeError(f"Failed to write {blob_name}: {e}")


# ============================================================================
# Phase 5C: Azure Blob Storage
# ============================================================================

try:
    from azure.storage.blob import BlobServiceClient
except ImportError:
    BlobServiceClient = None


class KoreAzureReader:
    """Read Kore files from Azure Blob Storage"""
    
    def __init__(self, connection_string: str, container: str):
        if BlobServiceClient is None:
            raise RuntimeError("azure-storage-blob not installed. Install with: pip install azure-storage-blob")
        self.connection_string = connection_string
        self.container = container
        self.blob_service = BlobServiceClient.from_connection_string(connection_string)
    
    def read(self, blob_name: str) -> List[List[str]]:
        """Read Kore file from Azure"""
        try:
            container_client = self.blob_service.get_container_client(self.container)
            blob_client = container_client.get_blob_client(blob_name)
            data = blob_client.download_blob().readall()
            return self._parse_kore_data(data)
        except Exception as e:
            raise RuntimeError(f"Failed to read {blob_name}: {e}")
    
    def get_metadata(self, blob_name: str) -> Dict[str, Any]:
        """Get Azure blob metadata"""
        try:
            container_client = self.blob_service.get_container_client(self.container)
            blob_client = container_client.get_blob_client(blob_name)
            props = blob_client.get_blob_properties()
            
            return {
                'size': props['size'],
                'created': props['creation_time'].isoformat() if props.get('creation_time') else None,
                'modified': props['last_modified'].isoformat() if props.get('last_modified') else None
            }
        except Exception as e:
            raise RuntimeError(f"Failed to get metadata: {e}")
    
    def _parse_kore_data(self, data: bytes) -> List[List[str]]:
        """Parse Kore binary format using KoreBinaryParser"""
        from kore_parser import KoreBinaryParser
        import io
        
        parser = KoreBinaryParser()
        stream = io.BytesIO(data)
        return parser.parse_stream(stream)


class KoreAzureWriter:
    """Write Kore files to Azure Blob Storage"""
    
    def __init__(self, connection_string: str, container: str):
        if BlobServiceClient is None:
            raise RuntimeError("azure-storage-blob not installed")
        self.connection_string = connection_string
        self.container = container
        self.blob_service = BlobServiceClient.from_connection_string(connection_string)
    
    def write(self, data: bytes, blob_name: str, metadata: Dict[str, str] = None) -> bool:
        """Write Kore file to Azure"""
        try:
            container_client = self.blob_service.get_container_client(self.container)
            blob_client = container_client.get_blob_client(blob_name)
            blob_client.upload_blob(data, overwrite=True, metadata=metadata or {})
            return True
        except Exception as e:
            raise RuntimeError(f"Failed to write {blob_name}: {e}")


# Example usage
if __name__ == "__main__":
    print("Phase 5: Cloud Connectors - All readers/writers implemented")
    print("Available: S3, GCS, Azure")
    print("Install required SDKs: boto3, google-cloud-storage, azure-storage-blob")


# Phase 5B: Google Cloud Storage
from google.cloud import storage

class KoreGCSReader:
    """Read Kore files from GCS"""
    
    def __init__(self, project: str, bucket: str):
        self.project = project
        self.bucket = bucket
        self.client = storage.Client(project=project)
    
    def read(self, blob_name: str):
        """Read Kore file from GCS"""
        # TODO: Stream from GCS
        # - Download chunks
        # - Parse Kore format
        raise NotImplementedError("GCS read not yet implemented")

class KoreGCSWriter:
    """Write Kore files to GCS"""
    
    def __init__(self, project: str, bucket: str):
        self.project = project
        self.bucket = bucket
        self.client = storage.Client(project=project)
    
    def write(self, data, blob_name: str):
        """Write Kore file to GCS"""
        # TODO: Stream to GCS
        # - Upload chunks
        # - Set metadata
        raise NotImplementedError("GCS write not yet implemented")


# Phase 5C: Azure Blob Storage
from azure.storage.blob import BlobServiceClient

class KoreAzureReader:
    """Read Kore files from Azure Blob Storage"""
    
    def __init__(self, connection_string: str, container: str):
        self.connection_string = connection_string
        self.container = container
        self.blob_service = BlobServiceClient.from_connection_string(connection_string)
    
    def read(self, blob_name: str):
        """Read Kore file from Azure"""
        # TODO: Stream from Azure
        # - Download chunks
        # - Parse Kore format
        raise NotImplementedError("Azure read not yet implemented")

class KoreAzureWriter:
    """Write Kore files to Azure Blob Storage"""
    
    def __init__(self, connection_string: str, container: str):
        self.connection_string = connection_string
        self.container = container
        self.blob_service = BlobServiceClient.from_connection_string(connection_string)
    
    def write(self, data, blob_name: str):
        """Write Kore file to Azure"""
        # TODO: Stream to Azure
        # - Upload chunks
        # - Set metadata
        raise NotImplementedError("Azure write not yet implemented")


if __name__ == "__main__":
    # TODO: Add examples for each cloud provider
    pass
