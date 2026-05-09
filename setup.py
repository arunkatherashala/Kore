from setuptools import setup

with open("README.md", "r", encoding="utf-8") as fh:
    long_description = fh.read()

setup(
    name="kore-fileformat",
    version="0.1.0",
    author="Arun Kather Ashala",
    author_email="arunkatherashala@gmail.com",
    description="KORE Binary Format - Complete 8-language ecosystem",
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/arunkatherashala/Kore",
    packages=["kore_fileformat"],
    python_requires=">=3.8",
)
