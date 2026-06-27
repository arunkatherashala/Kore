name := "kore-spark"
version := "0.1.0"
scalaVersion := "2.12.17"

lazy val sparkVersion = "3.5.0"
lazy val hadoopVersion = "3.3.4"

libraryDependencies ++= Seq(
  "org.apache.spark" %% "spark-sql" % sparkVersion % "provided",
  "org.apache.hadoop" % "hadoop-common" % hadoopVersion % "provided",
  "org.apache.hadoop" % "hadoop-hdfs" % hadoopVersion % "provided",
  "org.scalatest" %% "scalatest" % "3.2.15" % Test,
  "org.apache.spark" %% "spark-sql" % sparkVersion % Test
)

assemblyOption in assembly := (assemblyOption in assembly).value.copy(includeScala = false)

publishMavenStyle := true
