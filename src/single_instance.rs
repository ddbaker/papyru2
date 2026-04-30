use std::{
    fmt,
    io::{self, BufRead, BufReader, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

const PROTOCOL_ID: &str = "papyru2.single_instance.v1";
const HELLO_PREFIX: &str = "HELLO ";
const OK_PREFIX: &str = "OK ";
const ACTIVATE_WINDOW_COMMAND: &str = "ACTIVATE_WINDOW";
const ACTIVATE_WINDOW_ACK: &str = "ACK ACTIVATE_WINDOW";
const CONNECT_TIMEOUT: Duration = Duration::from_millis(750);
const IO_TIMEOUT: Duration = Duration::from_millis(750);
const ACTIVATE_RETRY_DELAY: Duration = Duration::from_millis(50);
const ACCEPT_IDLE_SLEEP: Duration = Duration::from_millis(25);
const ACTIVATE_ATTEMPTS: usize = 8;

pub(crate) const DEFAULT_SINGLE_INSTANCE_PORT: u16 = 46927;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SingleInstanceUiCommand {
    ActivateWindow,
}

pub(crate) enum SingleInstanceStartup {
    Primary(SingleInstanceServer),
    ActivatedExisting,
    Collision(String),
    Error(String),
}

#[derive(Clone)]
pub(crate) struct SingleInstanceServer {
    state: Arc<SingleInstanceServerState>,
}

struct SingleInstanceServerState {
    endpoint: SocketAddr,
    activation_tx: Mutex<Option<smol::channel::Sender<SingleInstanceUiCommand>>>,
    pending_activation: AtomicBool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum SingleInstanceClientError {
    Connect(String),
    Io(String),
    Protocol(String),
}

impl fmt::Debug for SingleInstanceServer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SingleInstanceServer")
            .field("endpoint", &self.endpoint())
            .finish_non_exhaustive()
    }
}

impl fmt::Display for SingleInstanceClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(message) => write!(formatter, "connect failed: {message}"),
            Self::Io(message) => write!(formatter, "io failed: {message}"),
            Self::Protocol(message) => write!(formatter, "protocol failed: {message}"),
        }
    }
}

impl SingleInstanceClientError {
    fn from_io(context: &str, error: io::Error) -> Self {
        Self::Io(format!("{context}: {error}"))
    }

    fn protocol(message: impl Into<String>) -> Self {
        Self::Protocol(message.into())
    }
}

impl SingleInstanceServer {
    pub(crate) fn endpoint(&self) -> SocketAddr {
        self.state.endpoint
    }

    pub(crate) fn register_activation_sender(
        &self,
        sender: smol::channel::Sender<SingleInstanceUiCommand>,
    ) -> bool {
        let pending_activation = self.state.pending_activation.swap(false, Ordering::SeqCst);
        if let Ok(mut activation_tx) = self.state.activation_tx.lock() {
            *activation_tx = Some(sender.clone());
        }

        if pending_activation
            && sender
                .send_blocking(SingleInstanceUiCommand::ActivateWindow)
                .is_err()
        {
            self.state.pending_activation.store(true, Ordering::SeqCst);
        }

        pending_activation
    }

    #[cfg(test)]
    fn has_pending_activation_for_test(&self) -> bool {
        self.state.pending_activation.load(Ordering::SeqCst)
    }
}

pub(crate) fn default_single_instance_endpoint() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, DEFAULT_SINGLE_INSTANCE_PORT))
}

pub(crate) fn single_instance_startup() -> SingleInstanceStartup {
    single_instance_startup_with_endpoint(default_single_instance_endpoint())
}

pub(crate) fn single_instance_startup_with_endpoint(endpoint: SocketAddr) -> SingleInstanceStartup {
    match TcpListener::bind(endpoint) {
        Ok(listener) => match start_primary_listener(listener) {
            Ok(server) => SingleInstanceStartup::Primary(server),
            Err(error) => SingleInstanceStartup::Error(error.to_string()),
        },
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
            match activate_existing_instance_with_retries(endpoint) {
                Ok(()) => SingleInstanceStartup::ActivatedExisting,
                Err(SingleInstanceClientError::Protocol(message)) => {
                    SingleInstanceStartup::Collision(message)
                }
                Err(error) => SingleInstanceStartup::Error(error.to_string()),
            }
        }
        Err(error) => SingleInstanceStartup::Error(error.to_string()),
    }
}

pub(crate) fn activate_existing_instance(
    endpoint: SocketAddr,
) -> Result<(), SingleInstanceClientError> {
    activate_existing_instance_once(endpoint, CONNECT_TIMEOUT, IO_TIMEOUT)
}

fn activate_existing_instance_with_retries(
    endpoint: SocketAddr,
) -> Result<(), SingleInstanceClientError> {
    let mut last_error = None;

    for attempt in 0..ACTIVATE_ATTEMPTS {
        match activate_existing_instance(endpoint) {
            Ok(()) => return Ok(()),
            Err(error @ SingleInstanceClientError::Protocol(_)) => return Err(error),
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < ACTIVATE_ATTEMPTS {
                    thread::sleep(ACTIVATE_RETRY_DELAY);
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        SingleInstanceClientError::Connect("activation attempt did not run".to_string())
    }))
}

fn start_primary_listener(listener: TcpListener) -> io::Result<SingleInstanceServer> {
    let endpoint = listener.local_addr()?;
    listener.set_nonblocking(true)?;

    let state = Arc::new(SingleInstanceServerState {
        endpoint,
        activation_tx: Mutex::new(None),
        pending_activation: AtomicBool::new(false),
    });
    spawn_accept_loop(listener, Arc::downgrade(&state), endpoint)?;

    Ok(SingleInstanceServer { state })
}

fn spawn_accept_loop(
    listener: TcpListener,
    state: Weak<SingleInstanceServerState>,
    endpoint: SocketAddr,
) -> io::Result<()> {
    thread::Builder::new()
        .name("papyru2-single-instance".to_string())
        .spawn(move || {
            crate::log::trace_debug(format!(
                "req-sinst listener thread started endpoint={endpoint}"
            ));

            loop {
                let Some(state) = state.upgrade() else {
                    crate::log::trace_debug("req-sinst listener thread stopped no_state");
                    break;
                };

                match listener.accept() {
                    Ok((stream, peer)) => {
                        crate::log::trace_debug(format!("req-sinst listener accepted peer={peer}"));
                        if let Err(error) = handle_single_instance_stream(stream, &state) {
                            crate::log::trace_debug(format!(
                                "req-sinst listener protocol failed error={error}"
                            ));
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        drop(state);
                        thread::sleep(ACCEPT_IDLE_SLEEP);
                    }
                    Err(error) => {
                        crate::log::trace_debug(format!(
                            "req-sinst listener accept failed error={error}"
                        ));
                        drop(state);
                        thread::sleep(ACCEPT_IDLE_SLEEP);
                    }
                }
            }
        })?;

    Ok(())
}

fn handle_single_instance_stream(
    mut stream: TcpStream,
    state: &SingleInstanceServerState,
) -> io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut hello = String::new();
    if reader.read_line(&mut hello)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "missing hello line",
        ));
    }

    let expected_hello = format!("{HELLO_PREFIX}{PROTOCOL_ID}");
    if hello.trim_end_matches(['\r', '\n']) != expected_hello {
        stream.write_all(b"ERR protocol\n")?;
        stream.flush()?;
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid hello line",
        ));
    }

    stream.write_all(format!("{OK_PREFIX}{PROTOCOL_ID}\n").as_bytes())?;
    stream.flush()?;

    let mut command = String::new();
    if reader.read_line(&mut command)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "missing command line",
        ));
    }

    if command.trim_end_matches(['\r', '\n']) == ACTIVATE_WINDOW_COMMAND {
        dispatch_activation(state);
        stream.write_all(format!("{ACTIVATE_WINDOW_ACK}\n").as_bytes())?;
        stream.flush()?;
        return Ok(());
    }

    stream.write_all(b"ERR command\n")?;
    stream.flush()?;
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "unknown command",
    ))
}

fn dispatch_activation(state: &SingleInstanceServerState) {
    let activation_sender = state
        .activation_tx
        .lock()
        .ok()
        .and_then(|activation_tx| activation_tx.clone());

    if let Some(sender) = activation_sender {
        if sender
            .send_blocking(SingleInstanceUiCommand::ActivateWindow)
            .is_ok()
        {
            crate::log::trace_debug("req-sinst activation queued to gpui");
            return;
        }
    }

    state.pending_activation.store(true, Ordering::SeqCst);
    crate::log::trace_debug("req-sinst activation queued pending_window=true");
}

fn activate_existing_instance_once(
    endpoint: SocketAddr,
    connect_timeout: Duration,
    io_timeout: Duration,
) -> Result<(), SingleInstanceClientError> {
    let mut stream = TcpStream::connect_timeout(&endpoint, connect_timeout)
        .map_err(|error| SingleInstanceClientError::Connect(error.to_string()))?;
    stream
        .set_read_timeout(Some(io_timeout))
        .map_err(|error| SingleInstanceClientError::from_io("set_read_timeout", error))?;
    stream
        .set_write_timeout(Some(io_timeout))
        .map_err(|error| SingleInstanceClientError::from_io("set_write_timeout", error))?;

    stream
        .write_all(format!("{HELLO_PREFIX}{PROTOCOL_ID}\n{ACTIVATE_WINDOW_COMMAND}\n").as_bytes())
        .map_err(|error| SingleInstanceClientError::from_io("write request", error))?;
    stream
        .flush()
        .map_err(|error| SingleInstanceClientError::from_io("flush request", error))?;

    let mut reader = BufReader::new(stream);
    let mut ok_line = String::new();
    if reader
        .read_line(&mut ok_line)
        .map_err(|error| SingleInstanceClientError::from_io("read ok", error))?
        == 0
    {
        return Err(SingleInstanceClientError::protocol("missing ok response"));
    }

    let expected_ok = format!("{OK_PREFIX}{PROTOCOL_ID}");
    if ok_line.trim_end_matches(['\r', '\n']) != expected_ok {
        return Err(SingleInstanceClientError::protocol(format!(
            "unexpected ok response: {}",
            ok_line.trim()
        )));
    }

    let mut ack_line = String::new();
    if reader
        .read_line(&mut ack_line)
        .map_err(|error| SingleInstanceClientError::from_io("read ack", error))?
        == 0
    {
        return Err(SingleInstanceClientError::protocol(
            "missing activate acknowledgement",
        ));
    }

    if ack_line.trim_end_matches(['\r', '\n']) != ACTIVATE_WINDOW_ACK {
        return Err(SingleInstanceClientError::protocol(format!(
            "unexpected activate acknowledgement: {}",
            ack_line.trim()
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn localhost_port_0() -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
    }

    fn start_test_server() -> SingleInstanceServer {
        let listener = TcpListener::bind(localhost_port_0()).expect("bind test listener");
        start_primary_listener(listener).expect("start primary listener")
    }

    fn wait_until_endpoint_released(endpoint: SocketAddr) -> TcpListener {
        let started = Instant::now();
        loop {
            match TcpListener::bind(endpoint) {
                Ok(listener) => return listener,
                Err(error) => {
                    assert!(
                        started.elapsed() < Duration::from_secs(3),
                        "endpoint {endpoint} was not released: {error}"
                    );
                    thread::sleep(Duration::from_millis(25));
                }
            }
        }
    }

    #[test]
    fn sinst_test1_first_bind_of_unique_loopback_port_becomes_primary() {
        let startup = single_instance_startup_with_endpoint(localhost_port_0());

        let SingleInstanceStartup::Primary(server) = startup else {
            panic!("expected primary startup");
        };

        assert_ne!(server.endpoint().port(), 0);
    }

    #[test]
    fn sinst_test2_second_startup_sends_activate_and_returns_activated_existing() {
        let server = start_test_server();
        let (activation_tx, activation_rx) = smol::channel::unbounded();
        assert!(!server.register_activation_sender(activation_tx));

        let startup = single_instance_startup_with_endpoint(server.endpoint());

        assert!(matches!(startup, SingleInstanceStartup::ActivatedExisting));
        let command = smol::block_on(activation_rx.recv()).expect("receive activation command");
        assert_eq!(command, SingleInstanceUiCommand::ActivateWindow);
    }

    #[test]
    fn sinst_test3_after_dropping_first_listener_same_endpoint_can_bind_again() {
        let server = start_test_server();
        let endpoint = server.endpoint();

        drop(server);
        let rebound_listener = wait_until_endpoint_released(endpoint);

        assert_eq!(rebound_listener.local_addr().expect("local addr"), endpoint);
    }

    #[test]
    fn sinst_test4_invalid_handshake_is_reported_as_protocol_collision() {
        let dummy_listener = TcpListener::bind(localhost_port_0()).expect("bind dummy listener");
        let endpoint = dummy_listener.local_addr().expect("dummy local addr");
        let dummy_thread = thread::spawn(move || {
            let (mut stream, _) = dummy_listener.accept().expect("dummy accept");
            let mut reader = BufReader::new(stream.try_clone().expect("dummy clone"));
            let mut request = String::new();
            reader.read_line(&mut request).expect("dummy read request");
            stream.write_all(b"NOPE\n").expect("dummy write");
            stream.flush().expect("dummy flush");
            thread::sleep(Duration::from_millis(100));
        });

        let startup = single_instance_startup_with_endpoint(endpoint);

        assert!(matches!(startup, SingleInstanceStartup::Collision(_)));
        dummy_thread.join().expect("join dummy thread");
    }

    #[test]
    fn sinst_test5_startup_gate_returns_activated_existing_before_gpui_path() {
        let server = start_test_server();
        let (activation_tx, activation_rx) = smol::channel::unbounded();
        server.register_activation_sender(activation_tx);

        let startup = single_instance_startup_with_endpoint(server.endpoint());

        assert!(matches!(startup, SingleInstanceStartup::ActivatedExisting));
        let command = smol::block_on(activation_rx.recv()).expect("receive activation command");
        assert_eq!(command, SingleInstanceUiCommand::ActivateWindow);
    }

    #[test]
    fn sinst_test6_activation_queue_handles_pre_window_requests() {
        let server = start_test_server();

        activate_existing_instance(server.endpoint()).expect("activate existing");

        assert!(server.has_pending_activation_for_test());

        let (activation_tx, activation_rx) = smol::channel::unbounded();
        assert!(server.register_activation_sender(activation_tx));
        let command = smol::block_on(activation_rx.recv()).expect("receive pending activation");

        assert_eq!(command, SingleInstanceUiCommand::ActivateWindow);
        assert!(!server.has_pending_activation_for_test());
    }

    #[test]
    fn sinst_test7_default_endpoint_is_stable_loopback_tcp() {
        let endpoint = default_single_instance_endpoint();

        assert_eq!(
            endpoint,
            SocketAddr::from((Ipv4Addr::LOCALHOST, DEFAULT_SINGLE_INSTANCE_PORT))
        );
    }
}
