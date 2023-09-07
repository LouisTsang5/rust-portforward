use std::{
    collections::HashSet,
    fmt::Display,
    hash::Hash,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::OnceLock,
};

use futures::io;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener, TcpStream,
    },
    select,
    sync::{mpsc::Receiver, Mutex},
    task::JoinHandle,
};

use crate::Meter::MeterMessageSender;

static JOIN_HANDLE_ID: OnceLock<Mutex<u32>> = OnceLock::new();
struct JoinHandleWithId<T>(u32, JoinHandle<T>);
impl<T> JoinHandleWithId<T> {
    async fn new(handle: JoinHandle<T>) -> Result<JoinHandleWithId<T>, io::Error> {
        let id = {
            // Get id value
            let mut id_guard = JOIN_HANDLE_ID.get_or_init(|| Mutex::new(0)).lock().await;
            let id = *id_guard;

            // Update id to +1
            if *id_guard >= u32::MAX {
                *id_guard = 0;
            } else {
                *id_guard += 1;
            }

            // Return id
            id
        };
        Ok(JoinHandleWithId(id, handle))
    }
}
impl<T> std::ops::Deref for JoinHandleWithId<T> {
    type Target = JoinHandle<T>;

    fn deref(&self) -> &Self::Target {
        &self.1
    }
}
impl<T> std::ops::DerefMut for JoinHandleWithId<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.1
    }
}
impl<T> PartialEq for JoinHandleWithId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<T> Eq for JoinHandleWithId<T> {}
impl<T> Hash for JoinHandleWithId<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

pub async fn accept_conn(
    src_port: u16,
    target: SocketAddr,
    buff_size: usize,
    meter_msg_sender: MeterMessageSender,
    mut shutdown_msg_receiver: Receiver<()>,
) -> Result<(), std::io::Error> {
    let listener = TcpListener::bind(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        src_port,
    ))
    .await?;

    let mut conns = HashSet::new();

    loop {
        // Wait for an incoming connections or a shutdown command
        let (stream, peer) = select! {
            conn_future = listener.accept() => {
                match conn_future {
                    Ok((s, p)) => (s, p),
                    Err(e) => {
                        eprintln!("{e}");
                        continue;
                    }
                }
            },
            shutdown_future = shutdown_msg_receiver.recv() => {
                shutdown_future.expect("Unexpected shutdown of channel");
                break;
            },
        };

        // Handle connection
        let meter_msg_sender = meter_msg_sender.clone();
        let join_handle = tokio::spawn(async move {
            if let Err(e) = handle_conn(stream, peer, target, buff_size, meter_msg_sender).await {
                eprintln!("{}", e);
            }
        });

        // Insert handle to hashset
        conns.insert(JoinHandleWithId::new(join_handle).await.unwrap());

        // Remove closed connections from hashset
        conns.retain(|c| !c.is_finished());
    }

    // Wait for existing connections to disconnect
    for c in conns {
        if let Err(e) = c.1.await {
            eprintln!("{}", e);
        }
    }

    Ok(())
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
        let meter_msg_sender = meter_msg_sender.clone();
        tokio::spawn(async move {
            handle_forward(src_rstream, tgt_wstream, buff_size, move |n_bytes| {
                meter_msg_sender
                    .send(src_sockaddr, crate::Meter::Direction::From, n_bytes)
                    .unwrap();
            })
            .await
        })
    };

    let t2s = {
        let meter_msg_sender = meter_msg_sender;
        tokio::spawn(async move {
            handle_forward(tgt_rstream, src_wstream, buff_size, move |n_bytes| {
                meter_msg_sender
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
