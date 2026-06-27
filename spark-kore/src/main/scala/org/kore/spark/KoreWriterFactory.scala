package org.kore.spark

import org.apache.spark.sql.sources.v2.writer.WriterFactory
import org.apache.spark.sql.sources.v2.writer.DataWriter
import org.apache.spark.sql.catalyst.InternalRow
import org.apache.spark.sql.types.StructType
import org.slf4j.LoggerFactory

/**
 * Kore WriterFactory - creates data writers for each partition
 */
class KoreWriterFactory(
  filePath: String,
  schema: StructType,
  compression: String
) extends WriterFactory {
  
  private val log = LoggerFactory.getLogger(classOf[KoreWriterFactory])
  
  log.info(s"Created KoreWriterFactory with compression=$compression")
  
  /**
   * Create a writer for a specific partition
   */
  override def createDataWriter(
    partitionId: Int,
    taskId: Long,
    epochId: Long
  ): DataWriter[InternalRow] = {
    log.info(s"Creating data writer for partition=$partitionId, task=$taskId, epoch=$epochId")
    new KoreDataWriter(filePath, partitionId, schema, compression)
  }
}
