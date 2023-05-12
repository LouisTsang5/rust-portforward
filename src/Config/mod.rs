use std::{fs, io::ErrorKind, net::SocketAddr};

use dns_lookup::lookup_host;
use getopts::Options;

#[derive(Debug)]
pub struct Forward {
    pub s_port: u16,
    pub target: SocketAddr,
}

#[derive(Debug)]
pub struct Config {
    pub forwards: Vec<Forward>,
    pub buffer_size_kb: usize,
    pub n_thread: usize,
}

fn get_opts() -> Options {
    // Read options
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optopt(
        "b",
        "buff",
        "The buffer size of each handler thread",
        "BUFF_SIZE",
    );
    opts.optopt(
        "f",
        "conf",
        "A list of information for port forwarding",
        "CONFIG_FILE",
    );
    opts.optopt("t", "nthread", "The number of handler threads", "N_THREAD");
    return opts;
}

pub fn print_usage(program: &str) {
    let brief = format!("Usage: {} FORWARD_LIST [options]", program);
    print!("{}", get_opts().usage(&brief));
}

fn get_forward(s: &str) -> Result<Forward, String> {
    let s_port = s.split(':').take(1).collect::<Vec<&str>>()[0];

    let target = &s[s_port.len() + 1..];
    let vs = target.split(':').collect::<Vec<&str>>();
    if vs.len() != 2 {
        return Err(format!("invalid target: {}", s));
    }

    let host = match lookup_host(vs[0]) {
        Ok(hosts) => hosts,
        Err(e) => return Err(format!("{}", e)),
    }[0];

    let port = match vs[1].parse::<u16>() {
        Ok(port) => port,
        Err(_) => return Err(format!("{} is not a valid port", vs[1])),
    };

    let target = SocketAddr::new(host, port);
    let s_port = match s_port.parse::<u16>() {
        Ok(port) => port,
        Err(_) => return Err(format!("{} is not a valid port", s_port)),
    };
    return Ok(Forward { s_port, target });
}

pub fn get_config(args: &[String]) -> Result<Config, String> {
    let mut buffer_size_kb: usize = 8;
    let mut n_thread: usize = 5;

    // Read options
    let opts = get_opts();
    let matches = match opts.parse(args) {
        Ok(m) => m,
        Err(_) => return Err("Help".to_string()),
    };

    // Help
    if matches.opt_present("h") {
        return Err("Help".to_string());
    }

    // Buffer size
    if let Some(bs) = matches.opt_str("b") {
        buffer_size_kb = match bs.parse() {
            Ok(b) => b,
            Err(_) => return Err(format!("{bs} is not a valid buffer size")),
        }
    }

    // N thread
    if let Some(nt) = matches.opt_str("t") {
        n_thread = match nt.parse() {
            Ok(n) => n,
            Err(_) => return Err(format!("{nt} is not a valid number of threads")),
        }
    }

    // Forwards
    let mut forwards: Vec<Forward> = Vec::with_capacity(matches.free.len());
    for s in &matches.free {
        let forward = get_forward(&s)?;
        if forwards
            .iter()
            .map(|f| f.s_port)
            .collect::<Vec<u16>>()
            .contains(&forward.s_port)
        {
            return Err(format!(
                "Cannot declare the same port twice. Found {} twice.",
                forward.s_port
            ));
        }
        forwards.push(forward);
    }

    // Read config file put into the forwards vector if it is not present
    if let Some(file_path) = matches.opt_str("f") {
        for file_f in read_config_file(&file_path)? {
            if forwards.len() == 0 || forwards.iter().all(|f| f.s_port != file_f.s_port) {
                forwards.push(file_f);
            }
        }
    }

    // If no forward list return error
    if forwards.len() == 0 {
        return Err("no forward list found".to_string());
    }

    // Sort the array in ascending order of source port
    forwards.sort_by(|f1, f2| f1.s_port.cmp(&f2.s_port));

    return Ok(Config {
        forwards,
        buffer_size_kb,
        n_thread,
    });
}

fn read_config_file(file_path: &str) -> Result<Vec<Forward>, String> {
    let config = match fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(format!("{file_path} does not exists"));
        }
        Err(e) => {
            return Err(e.to_string());
        }
    };
    let lines: Vec<&str> = config.lines().collect();
    let mut forwards: Vec<Forward> = Vec::with_capacity(lines.len());
    for line in lines {
        forwards.push(get_forward(line)?);
    }
    return Ok(forwards);
}
