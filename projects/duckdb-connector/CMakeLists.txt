cmake_minimum_required(VERSION 3.15)
project(kore_duckdb)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# DuckDB installation directory
set(DUCKDB_DIR "/opt/duckdb" CACHE PATH "DuckDB installation directory")

# Find DuckDB
find_package(DuckDB REQUIRED HINTS "${DUCKDB_DIR}")

# Kore library directory
set(KORE_LIB_DIR "/opt/kore/lib" CACHE PATH "Kore library directory")

# Create the extension
add_library(kore_extension SHARED
    src/kore_extension.cpp
    src/kore_reader.cpp
    src/kore_file_parser.cpp
)

# Include directories
target_include_directories(kore_extension PRIVATE
    ${DUCKDB_INCLUDE_DIRS}
    /opt/kore/include
)

# Link libraries
target_link_libraries(kore_extension
    ${DUCKDB_LIBRARIES}
    kore_fileformat
)

# Set output directory
set_target_properties(kore_extension PROPERTIES
    PREFIX ""
    SUFFIX ".so"
)

# Install target
install(TARGETS kore_extension
    LIBRARY DESTINATION lib/duckdb/extensions
)

# Create version file
set(KORE_VERSION "1.0.0")
configure_file(
    ${CMAKE_CURRENT_SOURCE_DIR}/version.h.in
    ${CMAKE_CURRENT_BINARY_DIR}/version.h
)
