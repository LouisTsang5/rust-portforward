use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::mpsc::{self, Receiver, SendError, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum Direction {
    To,
    From,
}

#[derive(Debug)]
pub struct Message {
    src_sockaddr: SocketAddr,
    direction: Direction,
    instant: Instant,
    n_bytes: usize,
}

pub struct Meter {
    shutdown_sender: Sender<()>,
    t_handle: JoinHandle<()>,
}

const SLEEP_MS: u64 = 500;

fn spawn_meter_thread(
    message_receiver: Receiver<Message>,
    shutdown_receiver: Receiver<()>,
) -> JoinHandle<()> {
    let t_handle = thread::spawn(move || loop {
        let mut map: HashMap<(SocketAddr, Direction), Vec<(Instant, usize)>> = HashMap::new();

        loop {
            // Sleep for a duration
            thread::sleep(Duration::from_millis(SLEEP_MS));

            // Read the channel until it is clear
            loop {
                let msg = match message_receiver.try_recv() {
                    Ok(m) => m,
                    Err(e) => match e {
                        TryRecvError::Empty => break,
                        TryRecvError::Disconnected => {
                            panic!("Unexpected disconnection of message channel")
                        }
                    },
                };

                // Push message to map
                let key = (msg.src_sockaddr, msg.direction);
                let val = (msg.instant, msg.n_bytes);
                if let Some(v) = map.get_mut(&key) {
                    v.push(val);
                } else {
                    map.insert(key, vec![val]);
                }
            }

            // Construct the rates
            for ((sockaddr, dir), v) in map.iter_mut() {
                // Skip the vector if number elements are less than 2
                if v.len() <= 1 {
                    continue;
                }

                // Reduce the vec to get min instant, max instant, and total number of bytes
                let rate = v.iter().fold(None, |acc, (instant, n_bytes)| match acc {
                    None => Some((instant, instant, *n_bytes)),
                    Some((o_min_instant, o_max_instant, t_n_bytes)) => {
                        let n_min_instant = match o_min_instant > instant {
                            true => instant,
                            false => o_min_instant,
                        };
                        let n_max_instant = match o_max_instant < instant {
                            true => instant,
                            false => o_max_instant,
                        };
                        let t_n_bytes = t_n_bytes + n_bytes;
                        Some((n_min_instant, n_max_instant, t_n_bytes))
                    }
                });

                // Print the vector
                if let Some((min_instant, max_instant, t_n_bytes)) = rate {
                    let dur_ms = max_instant.duration_since(*min_instant).as_micros();
                    if dur_ms <= 0 {
                        println!("{:?}", rate);
                    }
                    let kbytes_per_sec = t_n_bytes as f64 / (dur_ms as f64 / 1000f64);
                    println!("{:?} {}: {:.2} KB/s", dir, sockaddr, kbytes_per_sec);
                }

                // Drain the vector and leave only 1 element as the start of the next calculation
                v.drain(..v.len() - 1);
            }

            // Check if the shutdown command has been sent
            match shutdown_receiver.try_recv() {
                Ok(_) => {
                    println!("Shutdown message received");
                    break;
                }
                Err(e) => match e {
                    TryRecvError::Empty => (),
                    TryRecvError::Disconnected => {
                        panic!("Unexpected disconnection of shutdown command channel")
                    }
                },
            }
        }
    });
    t_handle
}

#[derive(Debug)]
pub enum ShutdownError {
    SendCommandError(SendError<()>),
    JoinError,
}

#[derive(Clone)]
pub struct MeterMessageSender(Sender<Message>);
impl MeterMessageSender {
    pub fn send(
        &self,
        src_sockaddr: SocketAddr,
        direction: Direction,
        n_bytes: usize,
    ) -> Result<(), SendError<Message>> {
        let instant = Instant::now();
        self.0.send(Message {
            src_sockaddr,
            direction,
            instant,
            n_bytes,
        })
    }
}

impl Meter {
    pub fn new() -> (Self, MeterMessageSender) {
        // Create message and shutdown command channels
        let (message_sender, message_receiver) = mpsc::channel::<Message>();
        let (shutdown_sender, shutdown_receiver) = mpsc::channel::<()>();

        // Spawn meter thread
        let t_handle = spawn_meter_thread(message_receiver, shutdown_receiver);

        // Return
        (
            Meter {
                shutdown_sender,
                t_handle,
            },
            MeterMessageSender(message_sender),
        )
    }

    pub fn shutdown(self) -> Result<(), ShutdownError> {
        // Send shutdown command
        if let Err(e) = self.shutdown_sender.send(()) {
            return Err(ShutdownError::SendCommandError(e));
        }

        // Wait for thread to join
        if let Err(_) = self.t_handle.join() {
            return Err(ShutdownError::JoinError);
        }
        Ok(())
    }
}
