use log::info;
use num_derive::FromPrimitive;
use std::f32::consts::E;
use std::fmt::Display;
use std::io::Read;
use std::net::TcpStream;
use std::str;
use std::time::Duration;

pub struct Protocol {
    ip: String,
    stream: Option<TcpStream>,
}

#[derive(FromPrimitive)]
pub enum PacketType {
    Error = 0,
    Command,
    XML,
    Data,
    NoMoreData,
    C3DFile,
    Event,
    Discover,
    QTMFile,
    None,
}

impl From<u32> for PacketType {
    fn from(value: u32) -> Self {
        match num::FromPrimitive::from_u32(value) {
            Some(p) => p,
            None => PacketType::None,
        }
    }
}

impl Display for PacketType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PacketType::Error => write!(f, "Error"),
            PacketType::Command => write!(f, "Command"),
            PacketType::XML => write!(f, "XML"),
            PacketType::Data => write!(f, "Data"),
            PacketType::NoMoreData => write!(f, "NoMoreData"),
            PacketType::C3DFile => write!(f, "C3DFile"),
            PacketType::Event => write!(f, "Event"),
            PacketType::Discover => write!(f, "Discover"),
            PacketType::QTMFile => write!(f, "QTMFile"),
            PacketType::None => write!(f, "None"),
        }
    }
}

pub struct Packet {
    size: u32,
    packet_type: PacketType,
    data: Vec<u8>,
}

impl Protocol {
    const QTM_CONNECTED_RESPONSE: &str = "QTM RT Interface connected";
    const QTM_LITTLEENDIAN_PORT: &str = "22223";

    pub fn new(ip: String) -> Protocol {
        Protocol {
            ip,
            stream: Default::default(),
            // data: vec![1],
        }
    }

    pub fn connect(&mut self) -> bool {
        // const TIMEOUT: Duration = Duration::from_secs(5);
        const RW_TIMEOUT: Duration = Duration::from_secs(1);

        // let socket_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 86, 23)), 22223);
        let addr = self.ip.clone() + ":" + Self::QTM_LITTLEENDIAN_PORT;
        info!("Connecting to {}", addr);
        match TcpStream::connect(addr) {
            Ok(stream) => {
                stream.set_read_timeout(Some(RW_TIMEOUT)).unwrap();
                stream.set_write_timeout(Some(RW_TIMEOUT)).unwrap();
                self.stream = Some(stream);

                // TODO::: Check connect response...
                self.receive();

                return true;
            }
            Err(e) => {
                println!("connect failed: {}", e);
            }
        };
        return false;
    }

    pub fn receive(&mut self) -> Option<Packet> {
        if self.stream.is_none() {
            info!("no stream connected");
            return None;
        }
        let mut stream = self.stream.as_ref().unwrap();
        let mut buffer = [0; 65535];
        match stream.read(&mut buffer[..]) {
            Ok(cnt) => {
                println!("{:?} {:?}", &buffer[0..cnt], &buffer[0..4]);
                let size = u32::from_le_bytes(buffer[0..4].try_into().unwrap());
                println!("{} {}", cnt, size);
                let packet_type =
                    PacketType::from(u32::from_le_bytes(buffer[4..8].try_into().unwrap()));
                let s = std::str::from_utf8(&buffer[8..cnt])
                    .expect("invalid utf-8 sequence")
                    .trim_end_matches(char::from(0));

                println!("{} {} {} {}", cnt, size, packet_type, s);
                println!("{}", s);
                println!("{}", Self::QTM_CONNECTED_RESPONSE);
                if String::from(s) == String::from(Self::QTM_CONNECTED_RESPONSE) {
                    println!("Matched connected string");
                    return Some(Packet {
                        size: size,
                        packet_type: packet_type,
                        data: buffer[8..cnt].to_vec(),
                    });
                }
                // println!("LEN: {} {}", Self::QTM_CONNECTED_RESPONSE.len(), s.len());
                // let c = Self::QTM_CONNECTED_RESPONSE.as_bytes();
                // for i in 0..s.len() {
                //     print!("{}", s.as_bytes()[i] as char);
                //     print!("{}", c[i] as char);
                // }
            }
            Err(e) => {
                println!("{}", e);
            }
        }
        return None;
    }

    pub fn disconnect(&mut self) -> std::io::Result<()> {
        if self.stream.is_none() {
            return Ok(());
        }
        self.stream = None;
        return Ok(());
    }
}
