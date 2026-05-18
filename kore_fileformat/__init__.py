"""
KORE Binary Format - Complete 8-language ecosystem for efficient data storage and querying

A high-performance binary file format with built-in compression, designed for distributed 
systems (Hadoop, Spark) and cloud storage platforms (S3, GCS, Azure).

Available in: Rust, Python, Java, Go, Scala, JavaScript, Killer DSL, and more.
"""

__version__ = "1.1.6"
__author__ = "Sai Arun Kumar Ktherashala"
__email__ = "arunkatherashala@gmail.com"

# Import from compiled Rust extension
# The PyO3 module is exposed at the package level due to maturin configuration
try:
    # Direct import from the compiled .pyd extension
    from . import kore_fileformat as _core
    
    # Expose functions and classes
    compress_csv = _core.compress_csv
    get_kore_info = _core.get_kore_info  
    KoreWriter = _core.PyKoreWriter
    KoreReader = _core.PyKoreReader
    
except (ImportError, AttributeError) as e:
    raise ImportError(
        f"Failed to import KORE bindings: {e}. "
        "Make sure you've built the wheel with: maturin build --release"
    )

# Legacy compatibility imports
try:
    from .parser import KoreBinaryParser
except ImportError:
    KoreBinaryParser = None


__all__ = [
    "__version__",
    "__author__",
    "compress_csv",
    "get_kore_info",
    "KoreWriter",
    "KoreReader",
    "KoreBinaryParser",
]
