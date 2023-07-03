use env_logger;
use qualisys::protocol::Protocol;
use std::env;

fn main() {
    if env::args().count() < 1 {
        println!("missing ip")
    }
    env_logger::init();

    let mut p = Protocol::new(String::from("192.168.86.23"));
    if p.connect() {
        println!("Connected!")
    }
    p.receive();
    let _ = p.disconnect();
    // const TIMEOUT: Duration = Duration::from_secs(5);

    // let socket_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 86, 23)), 22223);
    // match TcpStream::connect_timeout(&socket_addr, TIMEOUT) {
    //     Ok(mut stream) => loop {
    //         let mut buffer = [0; 65535];
    //         loop {
    //             println!("smeg");
    //             match stream.read(&mut buffer[..]).is_ {
    //                 Ok(cnt) => {
    //                     println!("{}", cnt);
    //                     for i in 0..cnt {
    //                         print!("{}", buffer[i] as char);
    //                     }
    //                 }
    //                 Err(e) => {
    //                     println!("{}", e);
    //                 }
    //             }
    //         }
    //     },
    //     Err(e) => {
    //         println!("{}", e);
    //     }
    // };
}
