use std::net::IpAddr;

use dns_lookup::lookup_host;
use getopts::Options;

#[derive(Debug)]
pub struct Forward {
    pub source_port: u16,
    pub targets: Vec<(IpAddr, u16)>,
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
    opts.optopt("t", "nthread", "The number of handler threads", "N_THREAD");
    return opts;
}

pub fn print_usage(program: &str) {
    let brief = format!("Usage: {} FORWARD_LIST [options]", program);
    print!("{}", get_opts().usage(&brief));
}

fn get_forward(s: &str) -> Result<Forward, String> {
    let source_port = s.split(':').take(1).collect::<Vec<&str>>()[0];
    let mut targets: Vec<(IpAddr, u16)> = vec![];
    for s in s[source_port.len() + 1..].split(',') {
        let vs = s.split(':').collect::<Vec<&str>>();
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

        targets.push((host, port));
    }
    let source_port = match source_port.parse::<u16>() {
        Ok(port) => port,
        Err(_) => return Err(format!("{} is not a valid port", source_port)),
    };
    return Ok(Forward {
        source_port,
        targets,
    });
}

pub fn get_config(args: &[&str]) -> Result<Config, String> {
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
    if matches.free.len() == 0 {
        return Err("no forward list found".to_string());
    }
    let mut forwards: Vec<Forward> = Vec::with_capacity(matches.free.len());
    for s in matches.free {
        forwards.push(get_forward(&s)?);
    }

    return Ok(Config {
        forwards,
        buffer_size_kb,
        n_thread,
    });
}
