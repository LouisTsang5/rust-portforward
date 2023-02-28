use rust_portforward::{
    Config::{get_config, print_usage, Config},
    ConnHandle::accept_conn,
    ThreadPool::ThreadPool,
};
use std::{
    env,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

fn main() -> Result<(), String> {
    //Read Args
    let args = env::args().collect::<Vec<_>>();
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let config = match get_config(&args[1..]) {
        Ok(c) => c,
        Err(e) if e == "Help" => {
            print_usage(&args[0]);
            return Ok(());
        }
        Err(e) => return Err(format!("Error: {}", e)),
    };
    print_config(&config);

    // Create thread pool
    let threadpool = Arc::new(Mutex::new(ThreadPool::new(config.n_thread)));

    // Create handle threads
    let mut handles: Vec<JoinHandle<Result<(), String>>> =
        Vec::with_capacity(config.forwards.len());
    for forward in config.forwards {
        let threadpool = Arc::clone(&threadpool);
        handles.push(thread::spawn(move || {
            accept_conn(
                forward.s_port,
                forward.target,
                config.buffer_size_kb,
                threadpool,
            )
        }));
    }
    for handle in handles {
        match handle.join().unwrap() {
            Err(e) => eprintln!("Error: {e}"),
            _ => (),
        }
    }

    return Ok(());
}

fn print_config(config: &Config) {
    println!(
        "Program started with BUFF_SIZE={}, N_THREAD={}, and FORWARD_LIST:",
        config.buffer_size_kb, config.n_thread
    );
    for f in &config.forwards {
        let (ip, port) = f.target;
        println!("\t{} -> {}:{}", f.s_port, ip, port);
    }
}
