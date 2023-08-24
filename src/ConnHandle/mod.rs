use std::{
    fmt::Display,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener, TcpStream,
    },
};

use crate::Meter::MeterMessageSender;

pub async fn accept_conn(
    src_port: u16,
    target: SocketAddr,
    buff_size: usize,
    meter_msg_sender: MeterMessageSender,
) -> Result<(), std::io::Error> {
    let listener = TcpListener::bind(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        src_port,
    ))
    .await?;

    loop {
        // Listen on incoming connections
        let (stream, peer) = match listener.accept().await {
            Ok((s, p)) => (s, p),
            Err(e) => {
                eprintln!("{e}");
                continue;
            }
        };

        // Handle connection
        let meter_msg_sender = meter_msg_sender.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(stream, peer, target, buff_size, meter_msg_sender).await {
                eprintln!("{}", e);
            }
        });
    }
}

async fn handle_conn(
    src_stream: TcpStream,
    src_sockaddr: SocketAddr,
    tgt_sockaddr: SocketAddr,
    buff_size: usize,
    meter_msg_sender: MeterMessageSender,
) -> Result<(), Box<dyn std::error::Error>> {
    let tgt_stream = TcpStream::connect(tgt_sockaddr).await?;

    println!("Opening handle for {}...", src_sockaddr);
    let (src_rstream, src_wstream) = src_stream.into_split();
    let (tgt_rstream, tgt_wstream) = tgt_stream.into_split();

    let s2t = {
        let meter_msg_sender = Arc::new(Mutex::new(meter_msg_sender.clone()));
        tokio::spawn(async move {
            handle_forward(src_rstream, tgt_wstream, buff_size, |n_bytes| {
                meter_msg_sender
                    .lock()
                    .unwrap()
                    .send(src_sockaddr, crate::Meter::Direction::From, n_bytes)
                    .unwrap();
            })
            .await
        })
    };

    let t2s = {
        let meter_msg_sender = Arc::new(Mutex::new(meter_msg_sender));
        tokio::spawn(async move {
            handle_forward(tgt_rstream, src_wstream, buff_size, |n_bytes| {
                meter_msg_sender
                    .lock()
                    .unwrap()
                    .send(src_sockaddr, crate::Meter::Direction::To, n_bytes)
                    .unwrap()
            })
            .await
        })
    };

    let (s2t_r, t2s_r) = tokio::join!(s2t, t2s);
    match s2t_r {
        Ok(task_result) => {
            if let Err(e) = task_result {
                eprintln!("{}", e);
            }
        }
        Err(join_err) => eprintln!("{}", join_err),
    };
    match t2s_r {
        Ok(task_result) => {
            if let Err(e) = task_result {
                eprintln!("{}", e);
            }
        }
        Err(join_err) => eprintln!("{}", join_err),
    };

    println!("Closing handle for {}...", src_sockaddr);
    Ok(())
}

struct HandleForwardError {
    loop_error: Option<std::io::Error>,
    shutdown_error: Option<std::io::Error>,
}

impl Display for HandleForwardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut error_strings: Vec<String> = Vec::with_capacity(2);
        if let Some(e) = &self.loop_error {
            error_strings.push(format!("{}", e));
        }
        if let Some(e) = &self.shutdown_error {
            error_strings.push(format!("{}", e));
        }
        let error_string = error_strings.join(", ");
        write!(f, "{}", error_string)
    }
}

async fn handle_forward<F: Fn(usize)>(
    mut src_rstream: OwnedReadHalf,
    mut tgt_wstream: OwnedWriteHalf,
    buff_size: usize,
    meter_msg_send_fn: F,
) -> Result<(), HandleForwardError> {
    let loop_res = forward_loop(
        &mut src_rstream,
        &mut tgt_wstream,
        buff_size,
        meter_msg_send_fn,
    )
    .await;

    let shutdown_res = match tgt_wstream.shutdown().await {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotConnected => Ok(()),
        Err(e) => Err(e),
    };

    // Error gathering
    let mut error = HandleForwardError {
        loop_error: None,
        shutdown_error: None,
    };
    if let Err(e) = loop_res {
        error.loop_error = Some(e);
    }
    if let Err(e) = shutdown_res {
        error.shutdown_error = Some(e);
    }

    if error.loop_error.is_none() && error.shutdown_error.is_none() {
        return Ok(());
    }
    Err(error)
}

async fn forward_loop<F: Fn(usize)>(
    src_rstream: &mut OwnedReadHalf,
    tgt_wstream: &mut OwnedWriteHalf,
    buff_size: usize,
    meter_msg_send_fn: F,
) -> Result<(), std::io::Error> {
    let mut buff = vec![0; buff_size * 1024];
    meter_msg_send_fn(0); // Send 0 to initialize the meter
    loop {
        let bytes_read = src_rstream.read(&mut buff).await?;
        if bytes_read == 0 {
            break;
        };
        tgt_wstream.write(&buff[..bytes_read]).await?;
        meter_msg_send_fn(bytes_read);
    }
    Ok(())
}
