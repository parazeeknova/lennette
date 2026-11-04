//! Synchronous UI boundary over a SeaORM/SQLx connection owned by a dedicated worker.
//! No Tokio runtime is blocked or nested on the GPUI thread. Existing repository
//! queries and SQLCipher files remain compatible while all live I/O uses SeaORM.
use crate::db::{DbError, Result};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, QueryResult, Statement,
    TransactionTrait, Value,
};
use std::{
    cell::Cell,
    marker::PhantomData,
    path::Path,
    sync::{Arc, mpsc},
    thread,
};

enum Op {
    Execute(String, Vec<Value>),
    Query(String, Vec<Value>),
    Batch(String),
    Begin,
    Commit,
    Rollback,
    Shutdown,
}
enum Reply {
    Rows(Vec<QueryResult>),
    Count(usize, i64),
    Unit,
}
struct Job {
    op: Op,
    reply: mpsc::SyncSender<Result<Reply>>,
}
struct Inner {
    sender: mpsc::Sender<Job>,
    worker: std::sync::Mutex<Option<thread::JoinHandle<()>>>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        let (tx, _) = mpsc::sync_channel(1);
        self.sender
            .send(Job {
                op: Op::Shutdown,
                reply: tx,
            })
            .ok();
        if let Some(w) = self.worker.get_mut().unwrap().take() {
            w.join().ok();
        }
    }
}
pub struct Connection {
    inner: Arc<Inner>,
    last_id: Cell<i64>,
    path: Option<String>,
    _not_sync: PhantomData<Cell<()>>,
}
impl Connection {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::connect(Some(path.as_ref()), None)
    }
    pub fn open_in_memory() -> Result<Self> {
        Self::connect(None, None)
    }
    pub(super) fn encrypted(path: &Path, key: &str) -> Result<Self> {
        Self::connect(Some(path), Some(key))
    }
    fn connect(path: Option<&Path>, key: Option<&str>) -> Result<Self> {
        let display_path = path.map(|p| p.to_string_lossy().into_owned());
        let path = path.map(Path::to_path_buf);
        let key = key.map(str::to_owned);
        let (tx, rx) = mpsc::channel::<Job>();
        let (ready, initialized) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("lennette-database".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(1)
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        ready.send(Err(DbError::Io(e))).ok();
                        return;
                    }
                };
                let result: Result<DatabaseConnection> = runtime.block_on(async {
                    let mut options = sqlx::sqlite::SqliteConnectOptions::new()
                        .create_if_missing(true)
                        .foreign_keys(true)
                        .busy_timeout(std::time::Duration::from_secs(5));
                    options = if let Some(path) = path {
                        options.filename(path)
                    } else {
                        options.in_memory(true)
                    };
                    if let Some(key) = key {
                        options = options.pragma("key", format!("\"{key}\""));
                    }
                    let pool = sqlx::sqlite::SqlitePoolOptions::new()
                        .max_connections(1)
                        .connect_with(options)
                        .await
                        .map_err(|e| DbError::Orm(sea_orm::DbErr::Custom(e.to_string())))?;
                    let db = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool);
                    db.query_one_raw(Statement::from_string(
                        sea_orm::DbBackend::Sqlite,
                        "SELECT count(*) FROM sqlite_master",
                    ))
                    .await?;
                    Ok(db)
                });
                let db = match result {
                    Ok(db) => {
                        ready.send(Ok(())).ok();
                        db
                    }
                    Err(e) => {
                        ready.send(Err(e)).ok();
                        return;
                    }
                };
                let mut transaction: Option<DatabaseTransaction> = None;
                while let Ok(job) = rx.recv() {
                    if matches!(job.op, Op::Shutdown) {
                        break;
                    }
                    let result = runtime.block_on(async {
                        match job.op {
                            Op::Begin => {
                                if transaction.is_some() {
                                    return Err(DbError::Storage("nested database transaction"));
                                }
                                transaction = Some(db.begin().await?);
                                Ok(Reply::Unit)
                            }
                            Op::Commit => {
                                transaction
                                    .take()
                                    .ok_or(DbError::Storage("no active transaction"))?
                                    .commit()
                                    .await?;
                                Ok(Reply::Unit)
                            }
                            Op::Rollback => {
                                if let Some(tx) = transaction.take() {
                                    tx.rollback().await?;
                                }
                                Ok(Reply::Unit)
                            }
                            op => {
                                if let Some(tx) = &transaction {
                                    run(tx, op).await
                                } else {
                                    run(&db, op).await
                                }
                            }
                        }
                    });
                    job.reply.send(result).ok();
                }
                runtime.block_on(async {
                    if let Some(tx) = transaction {
                        tx.rollback().await.ok();
                    }
                    db.close().await.ok();
                });
            })?;
        initialized
            .recv()
            .map_err(|_| DbError::Storage("database worker stopped"))??;
        Ok(Self {
            inner: Arc::new(Inner {
                sender: tx,
                worker: std::sync::Mutex::new(Some(worker)),
            }),
            last_id: Cell::new(0),
            path: display_path,
            _not_sync: PhantomData,
        })
    }
    fn call(&self, op: Op) -> Result<Reply> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.inner
            .sender
            .send(Job { op, reply: tx })
            .map_err(|_| DbError::Storage("database worker stopped"))?;
        rx.recv()
            .map_err(|_| DbError::Storage("database worker stopped"))?
    }
    pub fn execute(&self, sql: &str, params: impl Params) -> Result<usize> {
        match self.call(Op::Execute(sql.into(), params.values()))? {
            Reply::Count(n, id) => {
                self.last_id.set(id);
                Ok(n)
            }
            _ => unreachable!(),
        }
    }
    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        self.call(Op::Batch(sql.into()))?;
        Ok(())
    }
    pub fn last_insert_rowid(&self) -> i64 {
        self.last_id.get()
    }
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }
    pub fn prepare(&self, sql: &str) -> Result<Prepared<'_>> {
        Ok(Prepared {
            connection: self,
            sql: sql.into(),
        })
    }
    pub fn query_row<T>(
        &self,
        sql: &str,
        params: impl Params,
        map: impl FnOnce(&Row) -> Result<T>,
    ) -> Result<T> {
        let rows = self.query(sql, params)?;
        map(&Row(rows.into_iter().next().ok_or(DbError::NoRows)?))
    }
    fn query(&self, sql: &str, params: impl Params) -> Result<Vec<QueryResult>> {
        match self.call(Op::Query(sql.into(), params.values()))? {
            Reply::Rows(rows) => Ok(rows),
            _ => unreachable!(),
        }
    }
    pub fn unchecked_transaction(&self) -> Result<Transaction<'_>> {
        self.call(Op::Begin)?;
        Ok(Transaction {
            connection: self,
            done: false,
        })
    }
}
async fn run(db: &impl ConnectionTrait, op: Op) -> Result<Reply> {
    match op {
        Op::Execute(sql, params) => {
            let result = db
                .execute_raw(Statement::from_sql_and_values(
                    sea_orm::DbBackend::Sqlite,
                    sql,
                    params,
                ))
                .await?;
            Ok(Reply::Count(
                result.rows_affected() as usize,
                result.last_insert_id() as i64,
            ))
        }
        Op::Query(sql, params) => Ok(Reply::Rows(
            db.query_all_raw(Statement::from_sql_and_values(
                sea_orm::DbBackend::Sqlite,
                sql,
                params,
            ))
            .await?,
        )),
        Op::Batch(sql) => {
            db.execute_unprepared(&sql).await?;
            Ok(Reply::Unit)
        }
        _ => unreachable!(),
    }
}
pub struct Transaction<'a> {
    connection: &'a Connection,
    done: bool,
}
impl std::ops::Deref for Transaction<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.connection
    }
}
impl Transaction<'_> {
    pub fn commit(mut self) -> Result<()> {
        self.connection.call(Op::Commit)?;
        self.done = true;
        Ok(())
    }
}
impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.connection.call(Op::Rollback).ok();
        }
    }
}
pub struct Row(QueryResult);
impl Row {
    pub fn get<I: TryInto<usize>, T: sea_orm::TryGetable>(&self, index: I) -> Result<T> {
        let index = index
            .try_into()
            .map_err(|_| DbError::Storage("invalid column index"))?;
        Ok(self.0.try_get_by_index(index)?)
    }
}
pub struct Prepared<'a> {
    connection: &'a Connection,
    sql: String,
}
impl Prepared<'_> {
    pub fn query_row<T>(
        &mut self,
        params: impl Params,
        map: impl FnOnce(&Row) -> Result<T>,
    ) -> Result<T> {
        self.connection.query_row(&self.sql, params, map)
    }
    pub fn query_map<T>(
        &mut self,
        params: impl Params,
        mut map: impl FnMut(&Row) -> Result<T>,
    ) -> Result<std::vec::IntoIter<Result<T>>> {
        Ok(self
            .connection
            .query(&self.sql, params)?
            .into_iter()
            .map(|row| map(&Row(row)))
            .collect::<Vec<_>>()
            .into_iter())
    }
    pub fn query(&mut self, params: impl Params) -> Result<Rows> {
        Ok(Rows {
            rows: self
                .connection
                .query(&self.sql, params)?
                .into_iter()
                .map(Row)
                .collect(),
            index: 0,
        })
    }
}
pub struct Rows {
    rows: Vec<Row>,
    index: usize,
}
impl Rows {
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<&Row>> {
        let row = self.rows.get(self.index);
        self.index += 1;
        Ok(row)
    }
}
pub trait ToValue {
    fn to_value(&self) -> Value;
}
impl<T: ToValue + ?Sized> ToValue for &T {
    fn to_value(&self) -> Value {
        (*self).to_value()
    }
}
impl ToValue for str {
    fn to_value(&self) -> Value {
        self.into()
    }
}
impl ToValue for String {
    fn to_value(&self) -> Value {
        self.as_str().into()
    }
}
macro_rules! numeric{($($t:ty),*)=>{$(impl ToValue for $t{fn to_value(&self)->Value{(*self).into()}})*};}
numeric!(i64, i32, u64, u32, f64, bool);
impl ToValue for usize {
    fn to_value(&self) -> Value {
        (*self as i64).into()
    }
}
impl<T: ToValue> ToValue for Option<T> {
    fn to_value(&self) -> Value {
        self.as_ref()
            .map(ToValue::to_value)
            .unwrap_or(Value::String(None))
    }
}
pub trait Params {
    fn values(self) -> Vec<Value>;
}
impl Params for Vec<Value> {
    fn values(self) -> Vec<Value> {
        self
    }
}
impl Params for [Value; 0] {
    fn values(self) -> Vec<Value> {
        vec![]
    }
}
macro_rules! arrays{($($n:literal),*)=>{$(impl<T:ToValue>Params for [T;$n]{fn values(self)->Vec<Value>{self.iter().map(ToValue::to_value).collect()}})*};}
arrays!(1, 2, 3, 4, 5, 6, 7, 8);
#[macro_export]
macro_rules! db_params{($($value:expr),* $(,)?)=>{vec![$($crate::db::connection::ToValue::to_value(&$value)),*]};}
pub use crate::db_params as params;
pub trait OptionalExtension<T> {
    fn optional(self) -> Result<Option<T>>;
}
impl<T> OptionalExtension<T> for Result<T> {
    fn optional(self) -> Result<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(DbError::NoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
