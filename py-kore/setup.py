from setuptools import setup, find_packages

setup(
    name="kore",
    version="0.1.0",
    description="KORE Python bindings — PySpark-compatible DataFrame API",
    author="KORE Team",
    packages=find_packages(exclude=["tests", "tests.*"]),
    py_modules=["kore"],
    python_requires=">=3.9",
    extras_require={
        "jupyter": ["ipykernel>=6.0", "jupyter_client>=7.0"],
        "dev": ["pytest>=7.0"],
    },
    entry_points={
        "console_scripts": [
            "kore-kernel-install=kore_kernel.install:main",
        ],
    },
    classifiers=[
        "Development Status :: 3 - Alpha",
        "Intended Audience :: Developers",
        "License :: OSI Approved :: Apache Software License",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.9",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
        "Programming Language :: Python :: 3.12",
        "Topic :: Database :: Database Engines/Servers",
    ],
)
