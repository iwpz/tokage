//! The single write thread (docs/08 §1): jobs arriving within 100ms of the first, at most 200,
//! commit as one transaction. Each job runs in its own savepoint, so one failing job is rolled
//! back alone and the rest of the batch still commits.

use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use rusqlite::Connection;

pub type Job = Box<dyn FnOnce(&Connection) -> rusqlite::Result<()> + Send>;
type Reply = tokio::sync::oneshot::Sender<Result<(), String>>;

const WINDOW: Duration = Duration::from_millis(100);
const MAX_JOBS: usize = 200;
const STOPPED: &str = "Database writer stopped";

enum Msg {
    Job(Job, Option<Reply>),
    /// Commit what is queued now and signal back.
    Flush(mpsc::Sender<()>),
}

pub struct Writer {
    tx: mpsc::Sender<Msg>,
}

impl Writer {
    pub fn start(conn: Connection) -> Writer {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("tokage-db-writer".into())
            .spawn(move || run(conn, rx))
            .expect("spawn database writer");
        Writer { tx }
    }

    /// Fire and forget; a failure is logged by the writer.
    pub fn send(&self, job: Job) -> Result<(), String> {
        self.tx.send(Msg::Job(job, None)).map_err(|_| STOPPED.to_string())
    }

    /// Resolves once the job's transaction has committed (or failed).
    pub async fn call(&self, job: Job) -> Result<(), String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(Msg::Job(job, Some(tx)))
            .map_err(|_| STOPPED.to_string())?;
        rx.await.map_err(|_| STOPPED.to_string())?
    }

    pub fn flush_blocking(&self) {
        let (tx, rx) = mpsc::channel();
        if self.tx.send(Msg::Flush(tx)).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(5));
        }
    }
}

fn run(mut conn: Connection, rx: mpsc::Receiver<Msg>) {
    while let Ok(first) = rx.recv() {
        let deadline = Instant::now() + WINDOW;
        let mut jobs = Vec::new();
        let mut flushes = Vec::new();
        let mut next = Some(first);
        while let Some(msg) = next.take() {
            match msg {
                Msg::Job(job, reply) => jobs.push((job, reply)),
                Msg::Flush(done) => {
                    flushes.push(done);
                    break;
                }
            }
            let now = Instant::now();
            if jobs.len() >= MAX_JOBS || now >= deadline {
                break;
            }
            next = rx.recv_timeout(deadline - now).ok();
        }
        commit(&mut conn, jobs);
        for done in flushes {
            let _ = done.send(());
        }
    }
}

fn commit(conn: &mut Connection, jobs: Vec<(Job, Option<Reply>)>) {
    if jobs.is_empty() {
        return;
    }
    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(e) => {
            tracing::error!("database transaction failed to start: {e}");
            for (_, reply) in jobs {
                if let Some(reply) = reply {
                    let _ = reply.send(Err(e.to_string()));
                }
            }
            return;
        }
    };
    let mut results = Vec::with_capacity(jobs.len());
    for (job, reply) in jobs {
        let result = run_job(&tx, job).map_err(|e| e.to_string());
        if let Err(e) = &result {
            tracing::warn!("database write failed: {e}");
        }
        results.push((reply, result));
    }
    let committed = tx.commit().map_err(|e| e.to_string());
    if let Err(e) = &committed {
        tracing::error!("database commit failed: {e}");
    }
    for (reply, result) in results {
        if let Some(reply) = reply {
            let _ = reply.send(committed.clone().and(result));
        }
    }
}

fn run_job(c: &Connection, job: Job) -> rusqlite::Result<()> {
    c.execute_batch("SAVEPOINT job")?;
    match job(c) {
        Ok(()) => c.execute_batch("RELEASE job"),
        Err(e) => {
            let _ = c.execute_batch("ROLLBACK TO job; RELEASE job");
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failing_job_rolls_back_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("w.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE t (v INTEGER NOT NULL)").unwrap();
        let w = Writer::start(conn);
        w.send(Box::new(|c| c.execute("INSERT INTO t VALUES (1)", []).map(|_| ())))
            .unwrap();
        w.send(Box::new(|c| {
            c.execute("INSERT INTO t VALUES (2)", [])?;
            c.execute("INSERT INTO t VALUES (NULL)", []).map(|_| ())
        }))
        .unwrap();
        w.send(Box::new(|c| c.execute("INSERT INTO t VALUES (3)", []).map(|_| ())))
            .unwrap();
        w.flush_blocking();
        let read = Connection::open(&path).unwrap();
        let mut stmt = read.prepare("SELECT v FROM t ORDER BY v").unwrap();
        let vals: Vec<i64> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(vals, vec![1, 3]);
    }
}
