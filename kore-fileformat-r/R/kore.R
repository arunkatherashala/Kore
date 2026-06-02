# KORE magic bytes and format constants
KORE_MAGIC <- as.raw(c(0x4B, 0x4F, 0x52, 0x45))  # "KORE"
KORE_VERSION <- 1L

#' Read a KORE file into a data frame
#'
#' @param path Path to a .kore file
#' @param columns Optional character vector of column names to read (NULL = all)
#' @param max_rows Maximum number of rows to read (NULL = all)
#' @return A data.frame with the file contents
#' @export
#' @examples
#' \dontrun{
#'   df <- kore_read("data.kore")
#'   head(df)
#' }
kore_read <- function(path, columns = NULL, max_rows = NULL) {
  if (!file.exists(path)) stop("File not found: ", path)
  con <- file(path, "rb")
  on.exit(close(con))

  magic <- readBin(con, raw(), 4L)
  if (!identical(magic, KORE_MAGIC)) stop("Not a valid KORE file (magic mismatch)")

  version <- readBin(con, integer(), 1L, size = 4L, endian = "little")
  num_cols <- readBin(con, integer(), 1L, size = 4L, endian = "little")
  num_rows <- readBin(con, integer(), 1L, size = 8L, endian = "little")

  if (!is.null(max_rows)) num_rows <- min(num_rows, max_rows)

  col_names <- character(num_cols)
  col_types <- integer(num_cols)
  for (i in seq_len(num_cols)) {
    name_len <- readBin(con, integer(), 1L, size = 4L, endian = "little")
    col_names[i] <- rawToChar(readBin(con, raw(), name_len))
    col_types[i] <- readBin(con, integer(), 1L, size = 1L, endian = "little")
  }

  result <- vector("list", num_cols)
  names(result) <- col_names

  for (i in seq_len(num_cols)) {
    data_len <- readBin(con, integer(), 1L, size = 8L, endian = "little")
    raw_data <- readBin(con, raw(), data_len)
    result[[i]] <- .decode_column(raw_data, col_types[i], num_rows)
  }

  as.data.frame(result, stringsAsFactors = FALSE)
}

#' Write a data frame to a KORE file
#'
#' @param df A data.frame to write
#' @param path Output path for the .kore file
#' @param compress Compression level 1-9 (default 6)
#' @return Invisibly returns path
#' @export
#' @examples
#' \dontrun{
#'   kore_write(mtcars, "mtcars.kore")
#' }
kore_write <- function(df, path, compress = 6L) {
  if (!is.data.frame(df)) stop("df must be a data.frame")
  con <- file(path, "wb")
  on.exit(close(con))

  writeBin(KORE_MAGIC, con)
  writeBin(KORE_VERSION, con, size = 4L, endian = "little")
  writeBin(ncol(df), con, size = 4L, endian = "little")
  writeBin(as.integer(nrow(df)), con, size = 8L, endian = "little")

  for (nm in names(df)) {
    name_bytes <- charToRaw(nm)
    writeBin(length(name_bytes), con, size = 4L, endian = "little")
    writeBin(name_bytes, con)
    col_type <- .r_type_to_kore(df[[nm]])
    writeBin(col_type, con, size = 1L, endian = "little")
  }

  for (nm in names(df)) {
    encoded <- .encode_column(df[[nm]])
    writeBin(length(encoded), con, size = 8L, endian = "little")
    writeBin(encoded, con)
  }

  invisible(path)
}

#' Read KORE file metadata without loading data
#'
#' @param path Path to a .kore file
#' @return A list with version, num_rows, num_cols, columns (names + types)
#' @export
kore_metadata <- function(path) {
  if (!file.exists(path)) stop("File not found: ", path)
  con <- file(path, "rb")
  on.exit(close(con))

  magic <- readBin(con, raw(), 4L)
  if (!identical(magic, KORE_MAGIC)) stop("Not a valid KORE file")

  list(
    version  = readBin(con, integer(), 1L, size = 4L, endian = "little"),
    num_cols = readBin(con, integer(), 1L, size = 4L, endian = "little"),
    num_rows = readBin(con, integer(), 1L, size = 8L, endian = "little"),
    format   = "KORE"
  )
}

#' Get schema (column names and types) from a KORE file
#'
#' @param path Path to a .kore file
#' @return A data.frame with columns: name, type
#' @export
kore_schema <- function(path) {
  meta <- kore_metadata(path)
  con <- file(path, "rb")
  on.exit(close(con))
  readBin(con, raw(), 20L)  # skip header

  col_names <- character(meta$num_cols)
  col_types <- character(meta$num_cols)
  type_map  <- c("0"="integer","1"="double","2"="character","3"="logical")

  for (i in seq_len(meta$num_cols)) {
    name_len     <- readBin(con, integer(), 1L, size = 4L, endian = "little")
    col_names[i] <- rawToChar(readBin(con, raw(), name_len))
    t            <- as.character(readBin(con, integer(), 1L, size = 1L, endian = "little"))
    col_types[i] <- type_map[[t]] %||% "unknown"
  }
  data.frame(name = col_names, type = col_types, stringsAsFactors = FALSE)
}

# ---- internal helpers ----

.r_type_to_kore <- function(x) {
  if (is.integer(x))   return(0L)
  if (is.double(x))    return(1L)
  if (is.character(x)) return(2L)
  if (is.logical(x))   return(3L)
  2L  # default: string
}

.encode_column <- function(x) {
  if (is.integer(x))   return(writeBin(x, raw(), size = 4L, endian = "little"))
  if (is.double(x))    return(writeBin(x, raw(), size = 8L, endian = "little"))
  if (is.logical(x))   return(writeBin(as.integer(x), raw(), size = 1L, endian = "little"))
  # character: length-prefixed strings
  out <- raw(0)
  for (s in as.character(x)) {
    b <- charToRaw(s)
    out <- c(out, writeBin(length(b), raw(), size = 4L, endian = "little"), b)
  }
  out
}

.decode_column <- function(raw_data, type_code, n) {
  con <- rawConnection(raw_data, "rb")
  on.exit(close(con))
  switch(as.character(type_code),
    "0" = readBin(con, integer(),   n, size = 4L, endian = "little"),
    "1" = readBin(con, double(),    n, size = 8L, endian = "little"),
    "3" = as.logical(readBin(con, integer(), n, size = 1L, endian = "little")),
    {   # character (type 2)
      res <- character(n)
      for (i in seq_len(n)) {
        len    <- readBin(con, integer(), 1L, size = 4L, endian = "little")
        res[i] <- rawToChar(readBin(con, raw(), len))
      }
      res
    }
  )
}

`%||%` <- function(x, y) if (!is.null(x)) x else y
