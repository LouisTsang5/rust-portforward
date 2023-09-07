use rust_portforward::{
    Config::{get_config, print_usage, Config},
    ConnHandle::accept_conn,
    Meter,
};
use std::env;
use tokio::{
    io::{stdin, AsyncReadExt},
    sync::mpsc::{self, Sender},
    task::JoinHandle,
};

const STDIN_BUFF_SIZE: usize = 8;
const SHUTDOWN_COMMAND: &str = "q";

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

    // Main task loop
    let main_task_loop = async {
        // Accept connection and dispatch tasks
        let mut join_handles: Vec<JoinHandle<()>> = Vec::with_capacity(config.forwards.len());
        let mut shutdown_channels: Vec<Sender<()>> = Vec::with_capacity(config.forwards.len());
        for forward in config.forwards {
            let meter_msg_sender = meter_msg_sender.clone();
            let (sender, receiver) = mpsc::channel(1);
            shutdown_channels.push(sender);
            join_handles.push(tokio::spawn(async move {
                if let Err(e) = accept_conn(
                    forward.s_port,
                    forward.target,
                    config.buffer_size_kb,
                    meter_msg_sender,
                    receiver,
                )
                .await
                {
                    eprintln!("{}", e);
                }
            }));
        }

        // Wait for quit command
        let mut stdin = stdin();
        loop {
            let mut buff = [0; STDIN_BUFF_SIZE];
            let bytes_read = match stdin.read(&mut buff).await {
                Ok(n) => n,
                Err(e) => panic!("{}", e),
            };

            // shutdown if stdin cannot be read
            if bytes_read <= 0 {
                break;
            }

            // shutdown if quit command is received
            let command = String::from_utf8_lossy(&buff[..bytes_read]);
            if command.trim() == SHUTDOWN_COMMAND {
                println!("Shutdown command received");
                break;
            }
        }

        // Shutdown threads
        println!("Shutting down threads...");
        for c in shutdown_channels {
            c.send(()).await.unwrap();
        }
        let join_results = futures::future::join_all(join_handles).await;
        for result in join_results {
            if let Err(e) = result {
                eprintln!("{}", e);
            }
        }

        // Shutdown meter
        println!("Shutting down meter...");
        meter.shutdown().unwrap();
    };

    // Configure async runtime
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(config.n_thread)
        .build()
        .expect("Failed to build the async run time")
        .block_on(main_task_loop);
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
