use axum::{
    async_trait,
    extract::{Extension, FromRequest, RequestParts, Path},
    handler::{get, post},
    http::StatusCode,
    response::IntoResponse,
    AddExtensionLayer, Json, Router,
};
use bb8::{Pool, PooledConnection};
use bb8_postgres::PostgresConnectionManager;

use std::{collections::HashMap, net::SocketAddr};
use tokio::runtime::Builder;
use tokio_postgres::NoTls;

use serde::Serialize;

fn main() {
    let rt = Builder::new_multi_thread().enable_all().build().unwrap();

    rt.block_on(async {
        // Set the RUST_LOG, if it hasn't been explicitly defined
        if std::env::var_os("RUST_LOG").is_none() {
            std::env::set_var("RUST_LOG", "example_tokio_postgres=debug")
        }

        tracing_subscriber::fmt::init();

        let conf = "host=localhost user=postgres password=postgrespassword dbname=postgres";

        // setup connection pool
        let manager = PostgresConnectionManager::new_from_stringlike(conf, NoTls).unwrap();
        let pool = Pool::builder().build(manager).await.unwrap();

        // build our application with some routes
        let app = Router::new()
            .route("/", post(using_connection_extractor))
            .route("/:id", get(using_connection_pool_extractor))
            .layer(AddExtensionLayer::new(pool));

        // run it with hyper
        let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
        tracing::debug!("listening on {}", addr);
        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await
            .unwrap();
    });
}

#[derive(Debug, Serialize)]
struct User {
    id: i32,
    name: String,
    age: i32,
}

type ConnectionPool = Pool<PostgresConnectionManager<NoTls>>;

// we can exact the connection pool with `Extension`
async fn using_connection_pool_extractor(
    Extension(pool): Extension<ConnectionPool>,
    Path(parts): Path<HashMap<String, String>>,
) -> Result<(StatusCode, impl IntoResponse), (StatusCode, String)> {
    let id = parts.get("id").unwrap();
    let conn = pool.get_owned().await.map_err(internal_error)?;

    let user = get_user_witd_id(&conn, id.clone()).await?;

    Ok((StatusCode::FOUND, Json(user)))
}

// we can also write a custom extractor that grabs a connection from the pool
// which setup is appropriate depends on your application
type Conn = PooledConnection<'static, PostgresConnectionManager<NoTls>>;
struct DatabaseConnection(Conn);

#[async_trait]
impl<B> FromRequest<B> for DatabaseConnection
where
    B: Send,
{
    type Rejection = (StatusCode, String);

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let Extension(pool) = Extension::<ConnectionPool>::from_request(req)
            .await
            .map_err(internal_error)?;

        let conn = pool.get_owned().await.map_err(internal_error)?;

        Ok(Self(conn))
    }
}

async fn using_connection_extractor(
    DatabaseConnection(conn): DatabaseConnection
) -> Result<(StatusCode, Json<User>), (StatusCode, String)> {
    let user = get_user(&conn).await?;
    Ok((StatusCode::FOUND, Json(user)))
}

async fn get_user(conn: &Conn) -> Result<User, (StatusCode, String)> {
    let row = conn
        .query_one("select * from users limit 1", &[])
        .await
        .map_err(internal_error)?;

    let id: i32 = row.try_get("id").map_err(internal_error)?;
    let name: String = row.try_get("name").map_err(internal_error)?;
    let age: i32 = row.try_get("age").map_err(internal_error)?;

    Ok(User { id, name, age })
}

async fn get_user_witd_id(conn: &Conn, id: String) -> Result<User, (StatusCode, String)> {
    let query = format!("select * from users where id={} limit 1", id);
    
    let row = conn
        .query_one(query.as_str().clone(), &[])
        .await
        .map_err(internal_error)?;

    let id: i32 = row.try_get("id").map_err(internal_error)?;
    let name: String = row.try_get("name").map_err(internal_error)?;
    let age: i32 = row.try_get("age").map_err(internal_error)?;

    Ok(User { id, name, age })
}

/// Utility function for mapping any error into a `500 Internal Server Error`
/// response.
fn internal_error<E>(err: E) -> (StatusCode, String)
where
    E: std::error::Error, {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}
