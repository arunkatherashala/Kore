"""
KORE Binary Format - Complete 8-language ecosystem for efficient data storage and querying

A high-performance binary file format with built-in compression, designed for distributed 
systems (Hadoop, Spark) and cloud storage platforms (S3, GCS, Azure).

Available in: Rust, Python, Java, Go, Scala, JavaScript, Killer DSL, and more.
"""

__version__ = "0.1.0"
__author__ = "Arun Kather Ashala"
__email__ = "arunkatherashala@gmail.com"

# Core parser class - would import from compiled extension in production
try:
    from .parser import KoreBinaryParser
except ImportError:
    # Fallback if compiled extension not available
    pass


__all__ = ["__version__", "KoreBinaryParser"]
