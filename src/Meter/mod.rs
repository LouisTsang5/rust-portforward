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
        let mut last_run_instant = Instant::now();
        loop {
            // Sleep for a duration
            thread::sleep(Duration::from_millis(SLEEP_MS));

            // Read the channel and summarize the total number of bytes
            let mut map: HashMap<SocketAddr, (usize, usize)> = HashMap::new();
            loop {
                let Message {
                    src_sockaddr,
                    direction,
                    n_bytes,
                    instant: _,
                } = match message_receiver.try_recv() {
                    Ok(m) => m,
                    Err(e) => match e {
                        TryRecvError::Empty => break,
                        TryRecvError::Disconnected => {
                            panic!("Unexpected disconnection of message channel")
                        }
                    },
                };

                // Add to total
                if let Some((from_t_n_bytes, to_t_n_bytes)) = map.get_mut(&src_sockaddr) {
                    match direction {
                        Direction::From => *from_t_n_bytes += n_bytes,
                        Direction::To => *to_t_n_bytes += n_bytes,
                    };
                } else {
                    match direction {
                        Direction::From => map.insert(src_sockaddr, (n_bytes, 0)),
                        Direction::To => map.insert(src_sockaddr, (0, n_bytes)),
                    };
                }
            }

            // Calculate current instant
            let now = Instant::now();

            // Print the vector
            for (sockaddr, (from_t_n_bytes, to_t_n_bytes)) in map.iter() {
                let dur_microsec = now.duration_since(last_run_instant).as_micros();
                let kbytes_per_sec_from = *from_t_n_bytes as f64 / (dur_microsec as f64 / 1000f64); // B/ms = KB/s
                let kbytes_per_sec_to = *to_t_n_bytes as f64 / (dur_microsec as f64 / 1000f64); // B/ms = KB/s
                println!(
                    "[{}] ul: {:.2} KB/s, dl: {:.2} KB/s",
                    sockaddr, kbytes_per_sec_from, kbytes_per_sec_to
                );
            }

            // Update last run instant
            last_run_instant = now;

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
