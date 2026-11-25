use std::{
    fmt,
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpStream},
    str::FromStr,
    time::{Duration, Instant},
};

use serde::Serialize;

use crate::{raster, Error, PreparedLabel, PrinterStatus, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrinterAddress(SocketAddr);

impl FromStr for PrinterAddress {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        let value = value.trim();
        let socket = value.parse::<IpAddr>()
            .map(|ip| SocketAddr::new(ip, 9100))
            .or_else(|_| value.parse::<SocketAddr>())
            .map_err(|_| Error::Network(
                "Enter the printer's IP address, for example 192.168.1.50.                  An explicit port such as 192.168.1.50:9100 is also supported.".into()
            ))?;
        if socket.port() == 0 || socket.ip().is_unspecified() || socket.ip().is_multicast() {
            return Err(Error::Network(
                "Use a unicast printer IP address and a nonzero port.".into(),
            ));
        }
        Ok(Self(socket))
    }
}

impl fmt::Display for PrinterAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NetworkOptions {
    pub connect_timeout: Duration,
    pub status_timeout: Duration,
    pub write_timeout: Duration,
    pub completion_timeout: Duration,
}

impl Default for NetworkOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(3),
            status_timeout: Duration::from_secs(2),
            write_timeout: Duration::from_secs(10),
            completion_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterCheck {
    pub status_verified: bool,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintOutcome {
    pub confirmed: bool,
    pub bytes_sent: usize,
    pub message: String,
}

fn connect(address: PrinterAddress, options: NetworkOptions) -> Result<TcpStream> {
    let stream = TcpStream::connect_timeout(&address.0, options.connect_timeout)
        .map_err(|e| Error::Network(format!(
            "Could not connect to {address}: {e}. Check the printer's IP, power,              Wi-Fi connection, and whether raw TCP printing is enabled."
        )))?;
    stream.set_write_timeout(Some(options.write_timeout))?;
    stream.set_nodelay(true)?;
    Ok(stream)
}

/// Read one complete status frame even when TCP splits it across packets.
/// No status query is issued while printing.
fn read_status(stream: &mut TcpStream, deadline: Instant) -> Result<Option<PrinterStatus>> {
    let mut packet = [0u8; 32];
    let mut filled = 0;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            if filled == 0 {
                return Ok(None);
            }
            return Err(Error::Protocol("Incomplete 32-byte status packet.".into()));
        }
        stream.set_read_timeout(Some(remaining))?;
        match stream.read(&mut packet[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => {
                return Err(Error::Protocol(
                    "Connection closed during a status packet.".into(),
                ))
            }
            Ok(count) => {
                filled += count;
                if filled == packet.len() {
                    return PrinterStatus::parse(&packet).map(Some);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                if filled == 0 {
                    return Ok(None);
                }
                return Err(Error::Protocol("Timed out during a status packet.".into()));
            }
            Err(e) => {
                return Err(Error::Network(format!(
                    "Could not read printer status: {e}"
                )))
            }
        }
    }
}

fn preflight(stream: &mut TcpStream, options: NetworkOptions) -> Result<Option<PrinterStatus>> {
    // Switch modes without initializing or clearing a possibly active job.
    stream.write_all(raster::SWITCH_RASTER)?;
    stream.write_all(raster::STATUS_REQUEST)?;
    let deadline = Instant::now() + options.status_timeout;
    while let Some(status) = read_status(stream, deadline)? {
        status.check_ready()?;
        if status.status_type == 0 {
            return Ok(Some(status));
        }
    }
    Ok(None)
}

/// Query the printer and verify its model, media, and ready state when it
/// supplies status over TCP. This never sends raster data or a print command.
pub fn check_printer(address: PrinterAddress, options: NetworkOptions) -> Result<PrinterCheck> {
    let mut stream = connect(address, options)?;
    let status_verified = preflight(&mut stream, options)?.is_some();
    Ok(PrinterCheck {
        status_verified,
        message: if status_verified {
            "QL-810W ready · 62 mm continuous tape detected.".into()
        } else {
            "TCP connection works, but the printer did not return status.              Confirm that 62 mm continuous black-on-white tape is loaded.".into()
        },
    })
}

/// Print once. Never retries a job: an ambiguous send failure must not produce
/// duplicate labels. TCP write success alone is not treated as print completion.
pub fn print_label(
    address: PrinterAddress,
    label: &PreparedLabel,
    options: NetworkOptions,
) -> Result<PrintOutcome> {
    let bytes = label.raster_bytes();
    let mut stream = connect(address, options)?;
    let verified = preflight(&mut stream, options)?.is_some();
    stream.write_all(&bytes).map_err(|e| Error::Network(format!(
        "The connection failed while sending the label: {e}. A partial job may          have reached the printer. Check it before retrying; BroQR has not retried."
    )))?;
    let wait = if verified {
        options.completion_timeout
    } else {
        options.status_timeout.min(options.completion_timeout)
    };
    let deadline = Instant::now() + wait;
    loop {
        let status = read_status(&mut stream, deadline).map_err(|e| {
            Error::Network(format!(
            "The label was sent, but status reading failed: {e}. Check the printer before retrying."
        ))
        })?;
        let Some(status) = status else {
            return Ok(PrintOutcome {
                confirmed: false,
                bytes_sent: bytes.len(),
                message: "Label sent, but the printer did not confirm completion.                           Check the printer before printing again.".into(),
            });
        };
        status.check_errors()?;
        if status.status_type == 0x01 {
            return Ok(PrintOutcome {
                confirmed: true,
                bytes_sent: bytes.len(),
                message: "Printing completed — the printer confirmed the label.".into(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{prepare_image, status::tests::packet};
    use image::{GrayImage, Luma};
    use std::{net::TcpListener, thread};

    fn options() -> NetworkOptions {
        NetworkOptions {
            connect_timeout: Duration::from_secs(2),
            status_timeout: Duration::from_millis(150),
            write_timeout: Duration::from_secs(2),
            completion_timeout: Duration::from_secs(2),
        }
    }

    fn address(listener: &TcpListener) -> PrinterAddress {
        listener.local_addr().unwrap().to_string().parse().unwrap()
    }

    fn read_query(stream: &mut TcpStream) {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut query = [0; 7];
        stream.read_exact(&mut query).unwrap();
        assert_eq!(query, [0x1b, 0x69, 0x61, 1, 0x1b, 0x69, 0x53]);
    }

    fn label() -> PreparedLabel {
        prepare_image(GrayImage::from_pixel(696, 696, Luma([255])).into()).unwrap()
    }

    #[test]
    fn parses_ipv4_ipv6_and_explicit_ports_without_dns() {
        assert_eq!(
            "192.168.1.50"
                .parse::<PrinterAddress>()
                .unwrap()
                .to_string(),
            "192.168.1.50:9100"
        );
        assert_eq!(
            " ::1 ".parse::<PrinterAddress>().unwrap().to_string(),
            "[::1]:9100"
        );
        assert_eq!(
            "127.0.0.1:1234"
                .parse::<PrinterAddress>()
                .unwrap()
                .to_string(),
            "127.0.0.1:1234"
        );
        for bad in [
            "",
            "printer.local",
            "https://example.com",
            "0.0.0.0",
            "224.0.0.1",
            "127.0.0.1:0",
        ] {
            assert!(bad.parse::<PrinterAddress>().is_err(), "{bad}");
        }
    }

    #[test]
    fn handles_fragmented_preflight_and_completion_status() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = address(&listener);
        let label = label();
        let expected = label.raster_bytes();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_query(&mut stream);
            let ready = packet(0);
            stream.write_all(&ready[..9]).unwrap();
            thread::sleep(Duration::from_millis(5));
            stream.write_all(&ready[9..]).unwrap();
            let mut received = vec![0; expected.len()];
            stream.read_exact(&mut received).unwrap();
            assert_eq!(received, expected);
            let mut phase = packet(6);
            phase[19] = 1;
            stream.write_all(&phase).unwrap();
            stream.write_all(&packet(1)).unwrap();
        });
        let outcome = print_label(addr, &label, options()).unwrap();
        assert!(outcome.confirmed);
        assert_eq!(outcome.bytes_sent, label.raster_bytes().len());
        server.join().unwrap();
    }

    #[test]
    fn wrong_media_prevents_any_job_data_from_being_sent() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = address(&listener);
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_query(&mut stream);
            let mut wrong = packet(0);
            wrong[10] = 29;
            stream.write_all(&wrong).unwrap();
            let mut extra = [0; 1];
            assert_eq!(stream.read(&mut extra).unwrap(), 0);
        });
        assert!(print_label(addr, &label(), options())
            .unwrap_err()
            .to_string()
            .contains("62 mm"));
        server.join().unwrap();
    }

    #[test]
    fn surfaces_errors_during_printing_without_retrying() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = address(&listener);
        let label = label();
        let length = label.raster_bytes().len();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_query(&mut stream);
            stream.write_all(&packet(0)).unwrap();
            stream.read_exact(&mut vec![0; length]).unwrap();
            let mut error = packet(2);
            error[9] = 0x10;
            stream.write_all(&error).unwrap();
        });
        assert!(print_label(addr, &label, options())
            .unwrap_err()
            .to_string()
            .contains("cover is open"));
        server.join().unwrap();
    }

    #[test]
    fn missing_completion_is_reported_as_sent_not_printed() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = address(&listener);
        let label = label();
        let length = label.raster_bytes().len();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_query(&mut stream);
            stream.write_all(&packet(0)).unwrap();
            stream.read_exact(&mut vec![0; length]).unwrap();
            // Close without acknowledging; the job must not be retried.
        });
        assert!(!print_label(addr, &label, options()).unwrap().confirmed);
        server.join().unwrap();
    }

    #[test]
    fn printers_without_status_can_still_receive_a_job() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = address(&listener);
        let label = label();
        let length = label.raster_bytes().len();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_query(&mut stream);
            stream.read_exact(&mut vec![0; length]).unwrap();
        });
        assert!(!print_label(addr, &label, options()).unwrap().confirmed);
        server.join().unwrap();
    }

    #[test]
    fn connection_check_does_not_send_a_print_job() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = address(&listener);
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_query(&mut stream);
            stream.write_all(&packet(0)).unwrap();
            let mut extra = [0; 1];
            assert_eq!(stream.read(&mut extra).unwrap(), 0);
        });
        assert!(check_printer(addr, options()).unwrap().status_verified);
        server.join().unwrap();
    }
}
