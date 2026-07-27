
import sys, warnings
warnings.filterwarnings('ignore')
from pyspark.sql import SparkSession
from pyspark.sql.functions import *
from pyspark.sql.window import Window

spark = SparkSession.builder.appName("limits").master("local[*]") \
    .config("spark.ui.enabled","false") \
    .config("spark.driver.memory","4g") \
    .getOrCreate()
spark.sparkContext.setLogLevel("ERROR")

CSV = r"C:\Users\skathera\Downloads\asistent\kore\tpch_lineitem.csv"
df = spark.read.option("header","true").option("inferSchema","true").csv(CSV)
df.cache(); df.count()

try:
    df.groupBy('l_returnflag').count().orderBy('l_returnflag').collect()
    print("SPARK_RESULT:PASS")
except Exception as e:
    print(f"SPARK_RESULT:FAIL:{str(e)[:120]}")
spark.stop()
