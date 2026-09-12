pub struct Metrics {
    pub queries_total: prometheus::Counter,
    pub query_duration: prometheus::Histogram,
}

impl Metrics {
    pub fn new() -> Result<Self, prometheus::Error> {
        let queries_total = prometheus::Counter::new("kore_queries_total", "Total queries")?;
        let query_duration = prometheus::Histogram::new("kore_query_duration", "Query time")?;
        
        Ok(Self {
            queries_total,
            query_duration,
        })
    }
}
