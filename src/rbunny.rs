#[macro_use]
extern crate log;
#[macro_use]
extern crate clap;

mod blib;

use blib::handlers::get_routes;
use chrono;
use clap::Parser;
use configparser::ini::Ini;
use minijinja::{path_loader, Environment};
use std::sync::Arc;
use tokio::net::TcpListener;

#[derive(Parser, Debug)]
#[command(
    name=crate_name!(),
    author=crate_authors!(),
    version=crate_version!(),
    about=crate_description!(),
    long_about=None)
]
struct Args {
    /// The path to the config file
    #[arg(short, long, default_value = "/etc/rbunny.ini")]
    config: String,

    /// Turn on debug output
    #[arg(short = 'D', long)]
    debug: bool,
}

static LOGGER: GlobalLogger = GlobalLogger;

struct GlobalLogger;

/// This implements the logging to stderr from the `log` crate
impl log::Log for GlobalLogger {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        return meta.level() <= log::max_level();
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let d = chrono::Local::now();
            eprintln!(
                "{} - {} - {}:{} {} - {}",
                d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                record.level(),
                record.file().unwrap(),
                record.line().unwrap(),
                record.target(),
                record.args(),
            );
        }
    }

    fn flush(&self) {}
}

/// Create a set of CLI args via the `clap` crate and return the matches
fn get_args() -> Args {
    return Args::parse();
}

/// Set the global logger from the `log` crate
fn setup_logging(args: &Args) {
    let l = if args.debug {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(l);
}

fn get_config(path: &str) -> Arc<Ini> {
    let mut conf = Ini::new();
    conf.load(path)
        .expect(&format!("Failed to load config at {}", path));

    return Arc::new(conf);
}

async fn get_listener(conf: Arc<Ini>) -> TcpListener {
    let lconf = conf.get("main", "bind_addr").unwrap()
        + ":"
        + conf.get("main", "bind_port").unwrap().as_str();
    let listener = TcpListener::bind(&lconf).await.unwrap();

    return listener;
}

fn load_templates(conf: Arc<Ini>) -> Environment<'static> {
    let mut env = Environment::new();
    env.set_loader(path_loader(&conf.get("main", "tpl_dir").unwrap()));

    return env;
}

#[tokio::main]
async fn main() {
    let args = get_args();
    setup_logging(&args);
    let config = get_config(&args.config);
    let tpls = Arc::new(load_templates(config.clone()));

    let router = get_routes(config.clone(), tpls.clone());
    let listener = get_listener(config.clone()).await;

    axum::serve(listener, router).await.unwrap();
}
