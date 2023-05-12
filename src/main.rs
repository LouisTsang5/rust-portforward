use rust_portforward::{
    Config::{get_config, print_usage, Config},
    ConnHandle::accept_conn,
};
use std::env;

#[tokio::main]
async fn main() {
    //Read Args
    let args = env::args().collect::<Vec<_>>();
    let config = match get_config(&args[1..]) {
        Ok(c) => c,
        Err(e) if e == "Help" => return print_usage(&args[0]),
        Err(e) => return eprintln!("{}", e),
    };
    print_config(&config);

    // Accept connection and dispatch tasks
    let mut join_handles: Vec<tokio::task::JoinHandle<()>> =
        Vec::with_capacity(config.forwards.len());
    for forward in config.forwards {
        join_handles.push(tokio::spawn(async move {
            if let Err(e) = accept_conn(forward.s_port, forward.target, config.buffer_size_kb).await
            {
                eprintln!("{}", e);
            }
        }));
    }

    let join_results = futures::future::join_all(join_handles).await;
    for result in join_results {
        if let Err(e) = result {
            eprintln!("{}", e);
        }
    }
}

fn print_config(config: &Config) {
    println!(
        "Program started with BUFF_SIZE={}, N_THREAD={}, and FORWARD_LIST:",
        config.buffer_size_kb, config.n_thread
    );
    for f in &config.forwards {
        println!("\t{} -> {}", f.s_port, f.target);
    }
}
