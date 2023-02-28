use rust_portforward::{
    Config::{get_config, print_usage, Config},
    ThreadPool::ThreadPool,
};
use std::{
    env,
    io::{self, BufRead, BufReader, ErrorKind, Write},
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
    let mut handles: Vec<JoinHandle<Result<(), String>>> =
        Vec::with_capacity(config.forwards.len());
    for forward in config.forwards {
        let threadpool = Arc::clone(&threadpool);
        let src_port = forward.s_port;
        let buff_size = config.buffer_size_kb;
        handles.push(thread::spawn(move || {
            accept_conn(src_port, forward.target, buff_size, threadpool)
        }));
    }
    for handle in handles {
        handle.join().unwrap().unwrap();
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

fn accept_conn(
    src_port: u16,
    target: (IpAddr, u16),
    buff_size: usize,
    threadpool: Arc<Mutex<ThreadPool>>,
) -> Result<(), String> {
    let listener = match TcpListener::bind(format!("127.0.0.1:{}", src_port)) {
        Ok(l) => l,
        Err(_) => return Err(format!("Failed to bind to port {}", src_port)),
    };

    for src_stream in listener.incoming() {
        let src_stream = match src_stream {
            Ok(s) => s,
            Err(_) => continue,
        };
        threadpool.lock().unwrap().execute(move |id| {
            println!(
                "Connection accepted from {}. Dispatching handle {id}...",
                src_stream.peer_addr().unwrap()
            );
            handle_conn(src_stream, &target, buff_size).unwrap();
        });
    }
    return Ok(());
}

fn handle_conn(
    src_stream: TcpStream,
    target: &(IpAddr, u16),
    buff_size: usize,
) -> Result<(), String> {
    let tgt_stream = match TcpStream::connect(SocketAddr::new(target.0, target.1)) {
        Ok(s) => s,
        Err(_) => return Err(format!("Failed to connect to {}:{}", target.0, target.1)),
    };
    let source_addr = src_stream.peer_addr().unwrap();

    println!("Opening handle for {}...", source_addr);

    let s2t = {
        let src_stream = src_stream.try_clone().unwrap();
        let tgt_stream = tgt_stream.try_clone().unwrap();
        thread::spawn(move || forward(src_stream, tgt_stream, buff_size))
    };

    let t2s = thread::spawn(move || forward(tgt_stream, src_stream, buff_size));

    s2t.join().unwrap();
    t2s.join().unwrap();
    println!("Closing handle for {}...", source_addr);

    return Ok(());
}

fn forward(src: TcpStream, mut tgt: TcpStream, buff_size: usize) {
    let src_addr = src.peer_addr().unwrap();
    let tgt_addr = tgt.peer_addr().unwrap();
    let mut buff_reader = BufReader::with_capacity(buff_size * 1024, &src);
    loop {
        let (buff, buff_len) = match buff_reader.fill_buf() {
            Ok(b) if b.len() == 0 => {
                println!("No more content can be read from {}", src_addr);
                break;
            }
            Ok(b) => (b, b.len()),
            Err(_) => {
                println!("Failed to read from {}.", src_addr);
                break;
            }
        };
        println!("Read {} bytes from {}", buff_len, src_addr);

        if let Err(_) = tgt.write_all(buff) {
            println!("Failed to write to {}.", tgt_addr);
            break;
        } else {
            println!("Written {} bytes to {}.", buff_len, tgt_addr);
        }

        buff_reader.consume(buff_len);
    }
    shudown(&src).unwrap();
    shudown(&tgt).unwrap();
}

fn shudown(s: &TcpStream) -> Result<(), io::Error> {
    return match s.shutdown(std::net::Shutdown::Both) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotConnected => Ok(()),
        Err(e) => Err(e),
    };
}
