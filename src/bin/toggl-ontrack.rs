use dotenv::dotenv;
use log::*;
use std::env;
use structopt::StructOpt;
use toggl_utils::ontrack::*;

fn main() {
    dotenv().ok();
    let options = Options::from_args();
    env_logger::builder()
        .filter_module(env!("CARGO_CRATE_NAME"), options.verbosity)
        .filter_module(toggl_utils::cargo_crate_name(), options.verbosity)
        .format_module_path(false)
        .format_timestamp(None)
        .parse_default_env()
        .init();
    if let Err(err) = run(options) {
        error!("{:#}", err);
        std::process::exit(1);
    }
}
