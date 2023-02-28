use std::net::IpAddr;

use dns_lookup::lookup_host;
use getopts::Options;

#[derive(Debug)]
pub struct Forward {
    pub s_port: u16,
    pub target: (IpAddr, u16),
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

    let target = (host, port);
    let s_port = match s_port.parse::<u16>() {
        Ok(port) => port,
        Err(_) => return Err(format!("{} is not a valid port", s_port)),
    };
    return Ok(Forward { s_port, target });
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
    forwards.sort_by(|f1, f2| f1.s_port.cmp(&f2.s_port));

    return Ok(Config {
        forwards,
        buffer_size_kb,
        n_thread,
    });
}
