test_that("kore_write and kore_read round-trip", {
  tmp <- tempfile(fileext = ".kore")
  on.exit(unlink(tmp))

  df <- data.frame(
    id    = 1L:5L,
    value = c(1.1, 2.2, 3.3, 4.4, 5.5),
    label = c("a", "b", "c", "d", "e"),
    flag  = c(TRUE, FALSE, TRUE, FALSE, TRUE),
    stringsAsFactors = FALSE
  )

  kore_write(df, tmp)
  expect_true(file.exists(tmp))
  expect_gt(file.size(tmp), 0)

  result <- kore_read(tmp)
  expect_equal(nrow(result), 5L)
  expect_equal(ncol(result), 4L)
  expect_equal(result$id,    df$id)
  expect_equal(result$value, df$value)
  expect_equal(result$label, df$label)
  expect_equal(result$flag,  df$flag)
})

test_that("kore_metadata returns correct header info", {
  tmp <- tempfile(fileext = ".kore")
  on.exit(unlink(tmp))

  df <- data.frame(x = 1L:10L, y = letters[1:10], stringsAsFactors = FALSE)
  kore_write(df, tmp)

  meta <- kore_metadata(tmp)
  expect_equal(meta$num_rows, 10L)
  expect_equal(meta$num_cols, 2L)
  expect_equal(meta$format,   "KORE")
})

test_that("kore_schema returns column names and types", {
  tmp <- tempfile(fileext = ".kore")
  on.exit(unlink(tmp))

  df <- data.frame(a = 1L:3L, b = c("x","y","z"), stringsAsFactors = FALSE)
  kore_write(df, tmp)

  schema <- kore_schema(tmp)
  expect_equal(schema$name, c("a", "b"))
  expect_equal(schema$type, c("integer", "character"))
})
