use rust_portforward::{
    Config::{get_config, print_usage, Config},
    ThreadPool::ThreadPool,
};
use std::{
    env,
    io::{BufRead, BufReader, Write},
    net::{IpAddr, SocketAddr, TcpListener, TcpStream},
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

    // Create listener threads
    let mut handlers: Vec<JoinHandle<()>> = Vec::with_capacity(config.forwards.len());
    for forward in config.forwards {
        let listener = match TcpListener::bind(format!("127.0.0.1:{}", forward.source_port)) {
            Ok(l) => l,
            Err(_) => return Err(format!("Failed to bind to port {}", forward.source_port)),
        };
        let targets = Arc::new(forward.targets);
        let buff_size = config.buffer_size_kb;
        let threadpool = Arc::clone(&threadpool);
        handlers.push(thread::spawn(move || {
            accept_conn(listener, targets, buff_size, threadpool)
        }));
    }
    for handle in handlers {
        handle.join().unwrap();
    }

    return Ok(());
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

fn accept_conn(
    listener: TcpListener,
    targets: Arc<Vec<(IpAddr, u16)>>,
    buff_size: usize,
    threadpool: Arc<Mutex<ThreadPool>>,
) {
    for read_stream in listener.incoming() {
        let read_stream = read_stream.unwrap();
        let targets = Arc::clone(&targets);
        threadpool.lock().unwrap().execute(move |id| {
            println!(
                "Connection accepted from {}. Dispatching handler {id}...",
                read_stream.peer_addr().unwrap()
            );
            handle_forward(read_stream, targets, buff_size).unwrap();
        });
    }
}

fn is_empty<T>(lists: &Vec<Option<T>>) -> bool {
    return lists.iter().all(|item| {
        return match item {
            Some(_) => false,
            None => true,
        };
    });
}

fn handle_forward(
    read_stream: TcpStream,
    targets: Arc<Vec<(IpAddr, u16)>>,
    buff_size: usize,
) -> Result<(), String> {
    let mut buff_reader = BufReader::with_capacity(buff_size * 1024, &read_stream);
    let mut write_streams: Vec<Option<(TcpStream, SocketAddr)>> =
        get_write_streams(targets.as_ref())?
            .into_iter()
            .map(|s| {
                let addr = s.peer_addr().unwrap();
                return Some((s, addr));
            })
            .collect();
    let source_addr = read_stream.peer_addr().unwrap();

    println!("Opening handle for {}...", source_addr);

    loop {
        if is_empty(&write_streams) {
            println!("No targets can be written to. Exiting handle...");
            break;
        }

        let (buff, buff_len) = match buff_reader.fill_buf() {
            Ok(b) if b.len() == 0 => break,
            Ok(b) => (b, b.len()),
            Err(_) => {
                println!("Cannot read from {}. Exiting handle...", source_addr);
                break;
            }
        };
        println!("Read {} bytes from {}", buff_len, source_addr);

        for ws_sock in write_streams.iter_mut() {
            if let Some((ws, target_addr)) = ws_sock {
                if let Err(_) = ws.write_all(buff) {
                    println!("Failed to write to {}.", target_addr);
                    ws_sock.take();
                } else {
                    println!("Written {} bytes to {}.", buff_len, target_addr);
                }
            }
        }

        buff_reader.consume(buff_len);
    }

    println!("Closing handle for {}...", source_addr);

    return Ok(());
}

fn get_write_streams(targets: &Vec<(IpAddr, u16)>) -> Result<Vec<TcpStream>, String> {
    let mut write_streams: Vec<TcpStream> = vec![];
    for (ip, port) in targets {
        if let Ok(ws) = TcpStream::connect(SocketAddr::new(*ip, *port)) {
            write_streams.push(ws);
        } else {
            eprintln!("Failed to connect to {}:{}", ip, port);
        };
    }
    if write_streams.len() > 0 {
        return Ok(write_streams);
    } else {
        return Err("Unable to connect".to_string());
    }
}
