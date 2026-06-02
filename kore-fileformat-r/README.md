# korefileformat

[![CRAN status](https://www.r-pkg.org/badges/version/korefileformat)](https://CRAN.R-project.org/package=korefileformat)
[![R-CMD-check](https://github.com/arunkatherashala/Kore/actions/workflows/publish-r.yml/badge.svg)](https://github.com/arunkatherashala/Kore/actions/workflows/publish-r.yml)

R bindings for the [KORE](https://github.com/arunkatherashala/Kore) high-performance columnar file format.

## Installation

```r
# From CRAN (once released)
install.packages("korefileformat")

# Development version from GitHub
remotes::install_github("arunkatherashala/Kore", subdir = "kore-fileformat-r")
```

## Usage

```r
library(korefileformat)

# Write a data frame to KORE format
kore_write(mtcars, "mtcars.kore")

# Read it back
df <- kore_read("mtcars.kore")

# Inspect metadata without loading data
kore_metadata("mtcars.kore")

# Get column schema
kore_schema("mtcars.kore")
```

## Format

KORE achieves **87%+ compression ratios** with streaming support for files of any size. See the [main repository](https://github.com/arunkatherashala/Kore) for the specification.
