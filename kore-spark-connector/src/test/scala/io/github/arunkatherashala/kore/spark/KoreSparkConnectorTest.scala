package io.github.arunkatherashala.kore.spark

import org.scalatest.funspec.AnyFunSpec
import org.scalatest.matchers.should.Matchers
import org.apache.spark.sql.types._

/**
 * Kore Spark Connector Tests
 */
class KoreSparkTypesTest extends AnyFunSpec with Matchers {

  describe("Type Mapping") {

    it("should map Spark ByteType to Kore i8") {
      KoreSparkTypes.sparkTypeToKore(ByteType) should equal("i8")
    }

    it("should map Spark IntegerType to Kore i32") {
      KoreSparkTypes.sparkTypeToKore(IntegerType) should equal("i32")
    }

    it("should map Spark DoubleType to Kore f64") {
      KoreSparkTypes.sparkTypeToKore(DoubleType) should equal("f64")
    }

    it("should map Spark StringType to Kore string") {
      KoreSparkTypes.sparkTypeToKore(StringType) should equal("string")
    }

    it("should map Kore i32 to Spark IntegerType") {
      KoreSparkTypes.koreTypeToSpark("i32") should equal(IntegerType)
    }

    it("should map Kore f64 to Spark DoubleType") {
      KoreSparkTypes.koreTypeToSpark("f64") should equal(DoubleType)
    }

    it("should default unknown types to StringType") {
      KoreSparkTypes.koreTypeToSpark("unknown_type") should equal(StringType)
    }
  }

  describe("Compression Estimation") {

    it("should estimate high compression for ByteType") {
      val ratio = KoreSparkTypes.estimateCompressionRatio(ByteType, 1000000)
      ratio should be >= 0.75
    }

    it("should estimate lower compression for StringType") {
      val ratio = KoreSparkTypes.estimateCompressionRatio(StringType, 1000000)
      ratio should be < 0.50
    }

    it("should estimate compression for LongType") {
      val ratio = KoreSparkTypes.estimateCompressionRatio(LongType, 1000000)
      ratio should be >= 0.60
    }
  }
}

/**
 * Filter Push-down Tests
 */
class FilterPushdownTest extends AnyFunSpec with Matchers {

  describe("Filter Support") {

    it("should support EqualTo filter") {
      FilterPushdown.isFilterSupported("EqualTo") should equal(true)
    }

    it("should support StringContains filter") {
      FilterPushdown.isFilterSupported("StringContains") should equal(true)
    }

    it("should support And filter") {
      FilterPushdown.isFilterSupported("And") should equal(true)
    }

    it("should not support unknown filter") {
      FilterPushdown.isFilterSupported("UnknownFilter") should equal(false)
    }
  }

  describe("Filter Selectivity") {

    it("should estimate low selectivity for EqualTo") {
      FilterPushdown.estimateSelectivity("EqualTo") should equal(0.01)
    }

    it("should estimate medium selectivity for GreaterThan") {
      FilterPushdown.estimateSelectivity("GreaterThan") should equal(0.50)
    }

    it("should estimate high selectivity for Or") {
      FilterPushdown.estimateSelectivity("Or") should equal(0.20)
    }
  }
}

/**
 * Configuration Tests
 */
class KoreConnectorConfigTest extends AnyFunSpec with Matchers {

  describe("Configuration") {

    it("should create config with defaults") {
      val config = KoreConnectorConfig("/path/to/file.kore")
      config.compression should equal("hybrid")
      config.enableFilterPushdown should equal(true)
      config.parallelism should equal(4)
    }

    it("should create config with custom values") {
      val config = KoreConnectorConfig(
        filePath = "/data/file.kore",
        compression = "zstd",
        parallelism = 8
      )
      config.compression should equal("zstd")
      config.parallelism should equal(8)
    }
  }
}
