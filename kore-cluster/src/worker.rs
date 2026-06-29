//! Worker — async TCP server that executes shard tasks.

use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use kore_core::{Column, DataBlock, KoreError};
use crate::protocol::{
    recv_message, send_message, KoreMessage, Operation, ResultPayload, TaskPayload,
};

pub struct Worker {
    pub id:   String,
    pub addr: String,
}

impl Worker {
    pub fn new(id: &str, addr: &str) -> Self {
        Self { id: id.into(), addr: addr.into() }
    }

    /// Bind and start serving; returns a JoinHandle so the caller can await it.
    pub fn start(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let listener = TcpListener::bind(&self.addr).await
                .unwrap_or_else(|e| panic!("Worker {} bind failed: {}", self.id, e));
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let wid = self.id.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, &wid).await {
                                eprintln!("[worker {}] connection error: {}", wid, e);
                            }
                        });
                    }
                    Err(e) => eprintln!("[worker {}] accept error: {}", self.id, e),
                }
            }
        })
    }
}

async fn handle_connection(mut stream: TcpStream, worker_id: &str) -> Result<(), KoreError> {
    loop {
        let msg = match recv_message(&mut stream).await {
            Ok(m)  => m,
            Err(_) => break,  // connection closed
        };

        match msg {
            KoreMessage::Task(task) => {
                let result = execute_task(task);
                send_message(&mut stream, &KoreMessage::Result(result)).await?;
            }
            KoreMessage::Shutdown => break,
            KoreMessage::Heartbeat { .. } => {
                let ack = KoreMessage::Heartbeat { worker_id: worker_id.into() };
                send_message(&mut stream, &ack).await?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn execute_task(task: TaskPayload) -> ResultPayload {
    let data = match run_operation(&task.operation, &task.data) {
        Ok(d)  => d,
        Err(e) => {
            return ResultPayload { task_id: task.task_id, data: DataBlock::empty(), error: Some(e.to_string()) }
        }
    };
    ResultPayload { task_id: task.task_id, data, error: None }
}

fn run_operation(op: &Operation, data: &DataBlock) -> Result<DataBlock, KoreError> {
    match op {
        Operation::PassThrough => Ok(data.clone()),

        Operation::Count => {
            let n = data.num_rows as i64;
            DataBlock::new(vec![Column::int64("count", vec![Some(n)])])
        }

        Operation::Sum { column } => {
            let col = data.column(column)
                .ok_or_else(|| KoreError::ColumnNotFound(column.clone()))?;
            let sum: f64 = (0..data.num_rows)
                .filter_map(|i| col.data.get_value(i).as_f64())
                .sum();
            DataBlock::new(vec![
                Column::str_col("column", vec![Some(column.clone())]),
                Column::float64("sum",    vec![Some(sum)]),
            ])
        }
    }
}
