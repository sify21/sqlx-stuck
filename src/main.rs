mod error;

use actix_web::{get, middleware, web, App, HttpServer};
use chrono::Local;
use dotenv::dotenv;
use error::ServerError;
use log::info;
use rustls::internal::pemfile::{certs, pkcs8_private_keys};
use rustls::{NoClientAuth, ServerConfig};
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::io::Write;
use std::time::Duration;
use std::{env, thread};
use tokio::runtime;

pub type Result<T> = std::result::Result<T, error::ServerError>;

struct AppState {
    db_pool: PgPool,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(env::var("DB_MAXCONN").map_or(10, |s| s.parse::<u32>().unwrap_or(10)))
        .min_connections(env::var("DB_MINCONN").map_or(2, |s| s.parse::<u32>().unwrap_or(2)))
        .connect_timeout(Duration::from_secs(
            env::var("DB_CONNTIMEOUT").map_or(60, |s| s.parse::<u64>().unwrap_or(60)),
        ))
        .idle_timeout(Duration::from_secs(1800))
        .connect(env::var("DB_URL").unwrap().as_str())
        .await
        .unwrap();
    env_logger::builder()
        .format(|buf, record| {
            writeln!(
                buf,
                "{} [{}] - {}",
                Local::now().format("%Y-%m-%dT%H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .init();
    let app_state = web::Data::new(AppState { db_pool: pool });
    let mut config = ServerConfig::new(NoClientAuth::new());
    let cert_bytes = include_bytes!("cert.pem");
    let key_bytes = include_bytes!("key.pem");
    let mut cert = &cert_bytes[..];
    let mut key = &key_bytes[..];
    let cert_chain = certs(&mut cert).unwrap();
    let mut keys = pkcs8_private_keys(&mut key).unwrap();
    config.set_single_cert(cert_chain, keys.remove(0)).unwrap();
    let server = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(app_state.clone())
            .service(test)
            .service(nostuck)
            .service(stuck)
    })
    .bind_rustls(
        format!(
            "0.0.0.0:{}",
            env::var("WEB_PORT").unwrap_or("8443".to_string())
        ),
        config,
    )?
    .run();
    server.await
}

#[get("/test")]
async fn test() -> Result<&'static str> {
    Ok("actix_web is ok")
}

#[get("/nostuck")]
async fn nostuck(app_state: web::Data<AppState>) -> Result<String> {
    info!("nostuck start");
    let mut tx = app_state.db_pool.begin().await?;
    info!("nostuck tx ok");
    let s: Vec<(i64, String)> = sqlx::query_as("select * from test where id <=2")
        .fetch_all(&mut tx)
        .await?;
    info!("nostuck id<=2 is {}", s.len());
    let r: Vec<(i64, String)> = sqlx::query_as("select * from test where id > 2")
        .fetch_all(&mut tx)
        .await?;
    info!("nostuck id>2 is {}", r.len());
    tx.commit().await?;
    info!("nostuck end");
    Ok(format!("id<=2 is {}, id>2 is {}", s.len(), r.len()))
}

/// this will get stuck after being queried {DB_MAXCONN} times
#[get("/stuck")]
async fn stuck(app_state: web::Data<AppState>) -> Result<String> {
    let pool = app_state.db_pool.clone();
    let f =
        thread::Builder::new()
            .name("stuck".to_string())
            .spawn(move || -> anyhow::Result<()> {
                runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(async move {
                        info!("stuck start");
                        let mut tx = pool.begin().await?;
                        info!("stuck tx ok");
                        let s: Vec<(i64, String)> =
                            sqlx::query_as("select * from test where id <=2")
                                .fetch_all(&mut tx)
                                .await?;
                        info!("stuck id<=2 is {}", s.len());
                        let s: Vec<(i64, String)> =
                            sqlx::query_as("select * from test where id > 2")
                                .fetch_all(&mut tx)
                                .await?;
                        info!("stuck id>2 is {}", s.len());
                        tx.commit().await?;
                        info!("stuck finish");
                        Ok(())
                    })
            });
    match f {
        Err(_) => return Err(ServerError::Str("thread spawn error")),
        Ok(jh) => match jh.join() {
            Err(_) => return Err(ServerError::Str("thread join error")),
            Ok(Err(e)) => return Err(ServerError::String(e.to_string())),
            _ => (),
        },
    }
    Ok("stuck".to_string())
}
