mod config;
mod detector;
mod mqtt;

use clap::Parser;
use config::Config;

fn main() {
    let config = Config::parse();
    println!("slap-your-openclaw: config={config:?}");
}
