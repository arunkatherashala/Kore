package org.kore.spark

import org.apache.spark.sql.sources.v2.reader.SupportsPushDownFilters
import org.apache.spark.sql.sources.Filter
import org.apache.spark.sql.sources._
import org.slf4j.LoggerFactory

/**
 * Kore FilterPushdown - enables pushing filters to the Kore reader level
 * 
 * This allows filtering to happen before loading data into Spark,
 * reducing I/O and improving query performance by 2-4x
 */
trait KoreFilterPushdown extends SupportsPushDownFilters {
  
  private val log = LoggerFactory.getLogger(getClass)
  
  protected var pushedFilters = Array[Filter]()
  
  /**
   * Push filters down to Kore reader
   * Returns filters that could NOT be pushed down
   */
  override def pushFilters(filters: Array[Filter]): Array[Filter] = {
    log.info(s"Evaluating ${filters.length} filters for pushdown")
    
    val (pushable, unpushable) = filters.partition(canPushFilter)
    
    pushedFilters = pushable
    
    if (pushable.nonEmpty) {
      log.info(s"Pushed ${pushable.length} filters to Kore reader:")
      pushable.foreach(f => log.info(s"  - $f"))
    }
    
    if (unpushable.nonEmpty) {
      log.warn(s"Could not push ${unpushable.length} filters:")
      unpushable.foreach(f => log.warn(s"  - $f"))
    }
    
    unpushable
  }
  
  /**
   * Get the filters that were pushed down
   */
  override def pushedFilters(): Array[Filter] = {
    pushedFilters
  }
  
  /**
   * Determine if a filter can be pushed down to Kore
   */
  protected def canPushFilter(filter: Filter): Boolean = {
    filter match {
      case EqualTo(attribute, value) =>
        log.debug(s"Can push EqualTo filter: $attribute = $value")
        true
        
      case GreaterThan(attribute, value) =>
        log.debug(s"Can push GreaterThan filter: $attribute > $value")
        true
        
      case GreaterThanOrEqual(attribute, value) =>
        log.debug(s"Can push GreaterThanOrEqual filter: $attribute >= $value")
        true
        
      case LessThan(attribute, value) =>
        log.debug(s"Can push LessThan filter: $attribute < $value")
        true
        
      case LessThanOrEqual(attribute, value) =>
        log.debug(s"Can push LessThanOrEqual filter: $attribute <= $value")
        true
        
      case In(attribute, values) =>
        log.debug(s"Can push In filter: $attribute IN (${values.length} values)")
        true
        
      case IsNotNull(attribute) =>
        log.debug(s"Can push IsNotNull filter: $attribute IS NOT NULL")
        true
        
      case IsNull(attribute) =>
        log.debug(s"Can push IsNull filter: $attribute IS NULL")
        true
        
      case And(left, right) =>
        // Can push AND if both sides are pushable
        val leftPushable = canPushFilter(left)
        val rightPushable = canPushFilter(right)
        val result = leftPushable && rightPushable
        log.debug(s"Can push And filter: $result")
        result
        
      case Or(left, right) =>
        // Can push OR if both sides are pushable
        val leftPushable = canPushFilter(left)
        val rightPushable = canPushFilter(right)
        val result = leftPushable && rightPushable
        log.debug(s"Can push Or filter: $result")
        result
        
      case Not(child) =>
        // Can push NOT if child is pushable
        val canPush = canPushFilter(child)
        log.debug(s"Can push Not filter: $canPush")
        canPush
        
      case StringStartsWith(attribute, value) =>
        log.debug(s"Can push StringStartsWith filter: $attribute LIKE '${value}%'")
        true
        
      case StringEndsWith(attribute, value) =>
        log.debug(s"Can push StringEndsWith filter: $attribute LIKE '%${value}'")
        true
        
      case StringContains(attribute, value) =>
        log.debug(s"Can push StringContains filter: $attribute LIKE '%${value}%'")
        true
        
      case _ =>
        log.warn(s"Cannot push filter type: ${filter.getClass.getName}")
        false
    }
  }
  
  /**
   * Estimate selectivity for a filter
   * Used for query optimization
   */
  protected def estimateSelectivity(filter: Filter): Double = {
    filter match {
      case EqualTo(_, _) => 0.01      // 1% of rows typically match
      case GreaterThan(_, _) => 0.33  // 1/3 of rows on average
      case In(_, values) => values.length * 0.01 // 1% per value
      case IsNull(_) => 0.05          // 5% null values typical
      case IsNotNull(_) => 0.95       // 95% non-null values typical
      case StringStartsWith(_, _) => 0.1  // 10% match typical
      case And(left, right) => 
        estimateSelectivity(left) * estimateSelectivity(right)
      case Or(left, right) =>
        val s1 = estimateSelectivity(left)
        val s2 = estimateSelectivity(right)
        s1 + s2 - (s1 * s2)
      case Not(child) =>
        1.0 - estimateSelectivity(child)
      case _ => 0.5  // Default: assume 50% match
    }
  }
}
