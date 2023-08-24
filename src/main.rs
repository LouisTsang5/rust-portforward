use rust_portforward::{
    Config::{get_config, print_usage, Config},
    ConnHandle::accept_conn,
    Meter,
};
use std::env;

fn main() {
    // Read Args
    let args = env::args().collect::<Vec<_>>();
    let config = match get_config(&args[1..]) {
        Ok(c) => c,
        Err(e) if e == "Help" => return print_usage(&args[0]),
        Err(e) => return eprintln!("{}", e),
    };
    print_config(&config);

    // Create a meter
    let (meter, meter_msg_sender) = Meter::Meter::new();

    // Configure async runtime
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(config.n_thread)
        .build()
        .expect("Failed to build the async run time")
        .block_on(async {
            // Accept connection and dispatch tasks
            let mut join_handles: Vec<tokio::task::JoinHandle<()>> =
                Vec::with_capacity(config.forwards.len());
            for forward in config.forwards {
                let meter_msg_sender = meter_msg_sender.clone();
                join_handles.push(tokio::spawn(async move {
                    if let Err(e) = accept_conn(
                        forward.s_port,
                        forward.target,
                        config.buffer_size_kb,
                        meter_msg_sender,
                    )
                    .await
                    {
                        eprintln!("{}", e);
                    }
                }));
            }

            // Join forwarding threads
            let join_results = futures::future::join_all(join_handles).await;
            for result in join_results {
                if let Err(e) = result {
                    eprintln!("{}", e);
                }
            }
        });

    meter.shutdown().unwrap();
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
