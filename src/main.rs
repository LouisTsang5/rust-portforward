use rust_portforward::Config::{get_config, print_usage, Config};
use std::env;

fn main() {
    let args = env::args().collect::<Vec<_>>();
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let config = match get_config(&args[1..]) {
        Ok(c) => c,
        Err(e) if e == "Help" => {
            print_usage(&args[0]);
            return;
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            return;
        }
    };
    print_config(&config);
}

fn print_config(config: &Config) {
    println!(
        "Program started with BUFF_SIZE={}, N_THREAD={}, and FORWARD_LIST:",
        config.buffer_size_kb, config.n_thread
    );
    for f in &config.forwards {
        for (ip, port) in &f.targets {
            println!("\t{} -> {}:{}", f.source_port, ip, port);
        }
    }
}
